/** range-diff: git の出力の読み取りと、一時ディレクトリの実際のリポジトリでの比べ合わせ */
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, before, test } from 'node:test';
import { CliBackend } from '../src/git/backend.ts';
import { GitExecutor } from '../src/git/exec.ts';
import { parseRangeDiff, rangeDiffFile, rebaseOnto, rebaseOntoFromHead, reflogKeepsHistory, shortRange } from '../src/git/rangeDiff.ts';
import { initMain } from './git.ts';

Object.assign(process.env, {
  GIT_CONFIG_GLOBAL: '/dev/null',
  GIT_CONFIG_NOSYSTEM: '1',
  GIT_AUTHOR_NAME: 'a',
  GIT_AUTHOR_EMAIL: 'a@example.com',
  GIT_COMMITTER_NAME: 'a',
  GIT_COMMITTER_EMAIL: 'a@example.com',
});

test('parseRangeDiff splits headers and the diff of diffs', () => {
  const pairs = parseRangeDiff(
    [
      '1:  ef98ed8 ! 1:  040a1df five',
      '    @@ Commit message',
      '    -+old',
      '    ++new',
      ' 2:  618be3e = 2:  b7fc003 ten',
      '3:  fa6b06d < -:  ------- add z',
      '-:  ------- > 3:  e373123 new:  spaces',
      '',
    ].join('\n'),
  );
  assert.deepEqual(
    pairs.map((p) => [p.status, p.old?.index ?? null, p.new?.index ?? null, p.subject]),
    [
      ['changed', 1, 1, 'five'],
      ['same', 2, 2, 'ten'],
      ['removed', 3, null, 'add z'],
      ['added', null, 3, 'new:  spaces'],
    ],
  );
  assert.deepEqual(pairs[0]!.diff, ['@@ Commit message', '-+old', '++new']);
});

test('rangeDiffFile turns the diff of diffs into hunks, numbering each version’s patch', () => {
  const file = rangeDiffFile(['@@ Commit message', '     five', '-+old', '++new', '@@ a.txt', ' @@ -1 +1 @@']);
  assert.deepEqual(
    file.hunks.map((h) => [h.header, h.lines.map((l) => [l.kind, l.old ?? 0, l.new ?? 0, l.text])]),
    [
      ['@@ Commit message', [[' ', 1, 1, '    five'], ['-', 2, 0, '+old'], ['+', 0, 2, '+new']]],
      ['@@ a.txt', [[' ', 3, 3, '@@ -1 +1 @@']]],
    ],
  );
  // 見出しのない行も出す
  assert.deepEqual(rangeDiffFile(['+x']).hunks, [{ header: '', lines: [{ kind: '+', new: 1, text: 'x' }] }]);
});

test('shortRange shortens 40-digit IDs only', () => {
  const sha1 = 'a'.repeat(40);
  const sha256 = 'b'.repeat(64);
  assert.equal(shortRange(`${sha1}...${sha1}`), `${'a'.repeat(12)}...${'a'.repeat(12)}`);
  assert.equal(shortRange(sha256), sha256);
  assert.equal(shortRange('main'), 'main');
});

test('rebaseOnto and reflogKeepsHistory read reflog messages', () => {
  assert.equal(rebaseOnto('rebase (finish): refs/heads/t onto 68ca011997ede02ea11e2e016ff1d297a5f00ff4'), '68ca011997ede02ea11e2e016ff1d297a5f00ff4');
  assert.equal(rebaseOnto('rebase (finish): returning to refs/heads/t'), undefined);
  assert.equal(reflogKeepsHistory('commit: x'), true);
  assert.equal(reflogKeepsHistory('commit (amend): x'), false);
});

test('rebaseOntoFromHead finds the base of an old git rebase in the HEAD reflog', () => {
  const line = (n: string, message: string) => ({ old: null, new: n, message });
  const finished = line('c', 'rebase finished: returning to refs/heads/t');
  const head = [
    line('d', 'checkout: moving from t to main'),
    finished,
    line('c', 'rebase: two'),
    line('b', 'rebase: checkout main'),
    line('a', 'commit: two'),
  ];
  assert.equal(rebaseOntoFromHead(head, finished), 'b');
  assert.equal(rebaseOntoFromHead([finished, line('a', 'commit: two')], finished), undefined);
  assert.equal(rebaseOntoFromHead([], finished), undefined);
});

let dir: string;
let backend: CliBackend;

function git(...args: string[]): string {
  return execFileSync('git', args, { cwd: dir, encoding: 'utf8' }).trim();
}

function commit(file: string, content: string, message: string): void {
  writeFileSync(join(dir, file), content);
  git('add', '.');
  git('commit', '-q', '-m', message);
}

before(async () => {
  dir = realpathSync.native(mkdtempSync(join(tmpdir(), 'amonhen-test-')));
  initMain(dir);
  commit('a.txt', 'a\n', 'base');
  git('switch', '-q', '-c', 'topic');
  commit('b.txt', 'b\n', 'add b');
  commit('c.txt', 'c\n', 'add c');
  git('switch', '-q', 'main');
  commit('m.txt', 'm\n', 'main work');
  git('switch', '-q', 'topic');
  backend = await CliBackend.open(new GitExecutor('git'), dir);
});

after(async () => {
  await backend.dispose();
  rmSync(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
});

test('previousVersion finds the version before a rebase, and rangeDiff compares them', async () => {
  assert.equal(await backend.previousVersion(), undefined, 'commits alone are not a rewrite');
  const before = git('rev-parse', 'HEAD');
  git('reset', '-q', '--hard', 'HEAD~1');
  git('rebase', '-q', 'main');
  const previous = await backend.previousVersion('refs/heads/topic');
  assert.ok(previous);
  assert.match(previous.message, /^rebase/);
  assert.equal(previous.branch, 'topic');
  assert.equal(previous.onto, git('rev-parse', 'main'));
  assert.equal(previous.new, git('rev-parse', 'HEAD'));
  // 一番新しい書き換えは rebase（その前の reset ではない）
  assert.equal(previous.selector, 'topic@{1}');
  assert.equal(previous.ranges.length, 3);

  const pairs = await backend.rangeDiff(['main', before, 'topic']);
  assert.deepEqual(pairs.map((p) => p.status), ['same', 'removed']);
  assert.equal(pairs[0]!.old!.oid.length, 40);
  await assert.rejects(backend.rangeDiff(['--output=x']));
});
