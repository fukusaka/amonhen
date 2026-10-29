import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, test } from 'node:test';
import { applyDiffSelection, computeDiff } from '../src/diff/fileDiff.ts';
import { CliBackend } from '../src/git/backend.ts';
import { eolOf, lineEndingRule, lineEndingsOf, parseCheckAttr, parseEolConfig, toGitForm } from '../src/git/eol.ts';
import { GitExecutor } from '../src/git/exec.ts';
import { discardLines } from '../src/state/discard.ts';
import { UndoLog } from '../src/state/undo.ts';
import { initMain } from './git.ts';

Object.assign(process.env, {
  GIT_CONFIG_GLOBAL: '/dev/null',
  GIT_CONFIG_NOSYSTEM: '1',
  GIT_AUTHOR_NAME: 'a',
  GIT_AUTHOR_EMAIL: 'a@example.com',
  GIT_COMMITTER_NAME: 'a',
  GIT_COMMITTER_EMAIL: 'a@example.com',
});

const b = (s: string) => Buffer.from(s, 'latin1');

test('converts CRLF to LF as git does', () => {
  const crlf = b('1\r\n2\r\n');
  // core.autocrlf=true（Windows の既定）
  assert.equal(toGitForm(crlf, {}, 'true', b('1\n')).toString(), '1\n2\n');
  // index に CR があれば自動の変換はしない
  assert.equal(toGitForm(crlf, {}, 'true', b('1\r\n')).toString(), '1\r\n2\r\n');
  // 変換しない設定・属性
  assert.equal(toGitForm(crlf, {}, undefined, undefined), crlf);
  assert.equal(toGitForm(crlf, { text: 'unset' }, 'true', undefined), crlf);
  // text・eol は index に CR があっても変換する
  assert.equal(toGitForm(crlf, { text: 'set' }, undefined, b('x\r\n')).toString(), '1\n2\n');
  assert.equal(toGitForm(crlf, { eol: 'lf' }, undefined, undefined).toString(), '1\n2\n');
  // 単独の CR があれば、自動の判定ではバイナリ
  assert.equal(toGitForm(b('1\r2\r\n'), {}, 'true', undefined).toString(), '1\r2\r\n');
  // 再現しない変換がかかるなら、そのまま
  assert.equal(toGitForm(crlf, { filter: 'lfs', text: 'set' }, 'true', undefined), crlf);
});

test('reads git check-attr -z output', () => {
  const out = 'a.txt\0text\0auto\0a.txt\0eol\0crlf\0a.txt\0filter\0unspecified\0a.txt\0ident\0set\0';
  assert.deepEqual(parseCheckAttr(out), { text: 'auto', eol: 'crlf', ident: 'set' });
});

test('tells line endings apart and names the conversion', () => {
  assert.equal(eolOf(b('')), 'none');
  assert.equal(eolOf(b('a\nb\n')), 'lf');
  assert.equal(eolOf(b('a\r\nb\r\n')), 'crlf');
  assert.equal(eolOf(b('a\r\nb\n')), 'mixed');
  assert.deepEqual(parseEolConfig('core.autocrlf true\ncore.eol lf\n'), { autocrlf: 'true', eol: 'lf' });
  assert.equal(lineEndingRule({}, { autocrlf: 'true' }), 'core.autocrlf=true');
  assert.equal(lineEndingRule({ text: 'auto', eol: 'crlf' }, { autocrlf: 'true' }), 'text=auto eol=crlf');
  assert.equal(lineEndingRule({ text: 'unset' }, { autocrlf: 'true' }), '-text');
  assert.equal(lineEndingRule({ text: 'set' }, { eol: 'crlf' }), 'text core.eol=crlf');
  assert.equal(lineEndingRule({}, { eol: 'crlf' }), '');
});

test('reports line endings only when they are worth showing', () => {
  // 普段のファイル（変換なし、どちらも LF）
  const plain = lineEndingsOf(b('1\nTWO\n'), b('1\n2\n'), {}, {});
  assert.equal(plain.notable, false);
  // core.autocrlf=true: 作業ツリーは CRLF、git には LF
  assert.deepEqual(lineEndingsOf(b('1\r\nTWO\r\n'), b('1\n2\n'), {}, { autocrlf: 'true' }), {
    index: 'lf',
    worktree: 'crlf',
    checkout: 'crlf',
    rule: 'core.autocrlf=true',
    notable: true,
    warning: false,
  });
  // 改行が混ざっている
  assert.equal(lineEndingsOf(b('1\r\nTWO\n'), b('1\n2\n'), {}, { autocrlf: 'true' }).warning, true);
  // eol=lf なのに作業ツリーが CRLF（checkout すると LF）
  const attr = lineEndingsOf(b('1\r\nTWO\r\n'), b('1\n2\n'), { eol: 'lf' }, {});
  assert.equal(attr.checkout, 'lf');
  assert.equal(attr.rule, 'eol=lf');
  assert.equal(attr.warning, true);
  // core.autocrlf=true で、ツールが LF で書いたファイル（checkout すると CRLF になるだけ）。見せない
  const lf = lineEndingsOf(b('1\nTWO\n'), b('1\n2\n'), {}, { autocrlf: 'true' });
  assert.equal(lf.checkout, 'crlf');
  assert.equal(lf.notable, false);
});

let dir: string;
let backend: CliBackend;
let undo: UndoLog;

function git(...args: string[]): Buffer {
  return execFileSync('git', args, { cwd: dir });
}

beforeEach(async () => {
  dir = realpathSync.native(mkdtempSync(join(tmpdir(), 'amonhen-eol-')));
  initMain(dir);
  git('config', 'core.autocrlf', 'false');
  writeFileSync(join(dir, 'a.txt'), '1\n2\n3\n4\n');
  git('add', '.');
  git('commit', '-q', '-m', 'base');
  // Windows の既定。作業中のファイルは CRLF、index は LF
  git('config', 'core.autocrlf', 'true');
  backend = await CliBackend.open(new GitExecutor('git'), dir);
  undo = new UndoLog(backend);
});

afterEach(async () => {
  await backend.dispose();
  rmSync(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
});

async function unstagedDiff(path: string) {
  const index = git('cat-file', 'blob', `:${path}`);
  return computeDiff(path, index, await backend.worktreeFile(path, index));
}

test('diffs and stages lines of a CRLF file against an LF index', async () => {
  writeFileSync(join(dir, 'a.txt'), '1\r\nTWO\r\n3\r\n4\r\n5\r\n');
  const computed = await unstagedDiff('a.txt');
  const changed = computed.diff.hunks.flatMap((h) => h.lines).filter((l) => l.kind !== ' ');
  assert.deepEqual(
    changed.map((l) => `${l.kind}${l.text}`),
    ['-2', '+TWO', '+5'],
    '改行の違いは差分にしない',
  );
  // 5 を足すことだけを stage する。index に CR を入れない
  const next = applyDiffSelection(computed, 'stage', [{ kind: '+', new: 5 }]);
  await backend.writeIndex('a.txt', '100644', Buffer.from(next, 'latin1'));
  assert.equal(git('cat-file', 'blob', ':a.txt').toString(), '1\n2\n3\n4\n5\n');
});

test('discards lines keeping the line endings of the other lines', async () => {
  // 改行が混ざったファイル（2 行目だけ LF）。選んだ行のほかは 1 バイトも変えない
  const before = '1\r\nTWO\n3\r\nFOUR\r\n';
  writeFileSync(join(dir, 'a.txt'), before);
  const computed = await unstagedDiff('a.txt');
  await discardLines(backend, undo, 'a.txt', computed, [
    { kind: '-', old: 4 },
    { kind: '+', new: 4 },
  ]);
  // 戻した行は checkout と同じ CRLF
  assert.equal(readFileSync(join(dir, 'a.txt'), 'latin1'), '1\r\nTWO\n3\r\n4\r\n');
  await undo.undo();
  assert.equal(readFileSync(join(dir, 'a.txt'), 'latin1'), before);
});

test('reads the line endings of a CRLF file in a core.autocrlf repository', async () => {
  writeFileSync(join(dir, 'a.txt'), '1\r\nTWO\r\n3\r\n4\r\n');
  const le = await backend.lineEndings('a.txt', git('cat-file', 'blob', ':a.txt'));
  assert.deepEqual(le, {
    index: 'lf',
    worktree: 'crlf',
    checkout: 'crlf',
    rule: 'core.autocrlf=true',
    notable: true,
    warning: false,
  });
});
