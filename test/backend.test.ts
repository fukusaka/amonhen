/** 一時ディレクトリに実際のリポジトリを作り、git 実行層を通しで確かめる */
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, before, test } from 'node:test';
import { CliBackend } from '../src/git/backend.ts';
import { GitExecutor } from '../src/git/exec.ts';
import { configStamp, refsStamp, stashStamp } from '../src/git/stamp.ts';

let dir: string;
let backend: CliBackend;

// テスト対象の backend が起動する git にも効くよう、プロセス全体で設定を固定する
Object.assign(process.env, {
  GIT_CONFIG_GLOBAL: '/dev/null',
  GIT_CONFIG_NOSYSTEM: '1',
  GIT_AUTHOR_NAME: 'a',
  GIT_AUTHOR_EMAIL: 'a@example.com',
  GIT_COMMITTER_NAME: 'a',
  GIT_COMMITTER_EMAIL: 'a@example.com',
});

function git(...args: string[]): string {
  return execFileSync('git', args, { cwd: dir, encoding: 'utf8' });
}

before(async () => {
  dir = realpathSync(mkdtempSync(join(tmpdir(), 'amonhen-test-')));
  git('init', '-q', '-b', 'main');
  writeFileSync(join(dir, 'a.txt'), 'one\n');
  git('add', '.');
  git('commit', '-q', '-m', 'first commit');
  git('tag', '-a', 'v1', '-m', 'release one');
  git('remote', 'add', 'origin', 'https://example.com/repo.git');
  git('update-ref', 'refs/remotes/origin/main', 'HEAD');
  git('branch', '-u', 'origin/main');
  git('branch', 'feature/x');
  backend = await CliBackend.open(new GitExecutor('git'), dir);
});

after(() => {
  backend.dispose();
  rmSync(dir, { recursive: true, force: true });
});

test('open resolves the root and git dirs', () => {
  assert.equal(backend.root, dir);
  assert.equal(backend.dirs.gitDir, join(dir, '.git'));
  assert.equal(backend.dirs.commonDir, join(dir, '.git'));
});

test('status reports branch info and changes', async () => {
  writeFileSync(join(dir, 'a.txt'), 'two\n');
  writeFileSync(join(dir, 'new file.txt'), 'x\n');
  const s = await backend.status();
  assert.equal(s.branch.head, 'main');
  assert.equal(s.branch.upstream, 'origin/main');
  assert.deepEqual(
    s.entries.map((e) => [e.kind, e.path, e.index, e.worktree]),
    [
      ['changed', 'a.txt', '.', 'M'],
      ['untracked', 'new file.txt', '?', '?'],
    ],
  );
  git('checkout', '-q', '--', 'a.txt');
  rmSync(join(dir, 'new file.txt'));
});

test('refs, tags and remotes', async () => {
  const refs = await backend.refs(['refs/heads', 'refs/remotes']);
  assert.deepEqual(
    refs.map((r) => r.name),
    ['refs/heads/feature/x', 'refs/heads/main', 'refs/remotes/origin/main'],
  );
  const main = refs.find((r) => r.name === 'refs/heads/main')!;
  assert.equal(main.isHead, true);
  assert.equal(main.upstream, 'refs/remotes/origin/main');
  assert.equal(main.subject, 'first commit');

  const [tag] = await backend.refs(['refs/tags']);
  assert.equal(tag?.name, 'refs/tags/v1');
  assert.equal(tag?.peeled, main.oid);
  assert.equal(tag?.subject, 'release one');

  assert.deepEqual(await backend.remotes(), [
    { name: 'origin', fetchUrl: 'https://example.com/repo.git', pushUrl: 'https://example.com/repo.git' },
  ]);
  assert.deepEqual(await backend.submodules(), []);
});

test('cat-file returns objects in order, including missing ones', async () => {
  const [blob, missing, commit] = await Promise.all([
    backend.object('HEAD:a.txt'),
    backend.object('HEAD:nope'),
    backend.object('HEAD'),
  ]);
  assert.equal(blob?.type, 'blob');
  assert.equal(blob?.data.toString(), 'one\n');
  assert.equal(missing, undefined);
  assert.equal(commit?.type, 'commit');
  assert.match(commit!.data.toString(), /first commit/);
});

test('cat-file handles a blob larger than one pipe chunk', async () => {
  const big = 'x'.repeat(1 << 20);
  writeFileSync(join(dir, 'big.txt'), big);
  const oid = git('hash-object', '-w', 'big.txt').trim();
  const obj = await backend.object(oid);
  assert.equal(obj?.size, big.length);
  assert.equal(obj?.data.toString(), big);
  rmSync(join(dir, 'big.txt'));
});

test('stamps change only when the relevant state changes', async () => {
  const refs1 = await refsStamp(backend.dirs);
  const config1 = await configStamp(backend.dirs);
  const stash1 = await stashStamp(backend.dirs);
  assert.equal(await refsStamp(backend.dirs), refs1);

  git('branch', 'another');
  const refs2 = await refsStamp(backend.dirs);
  assert.notEqual(refs2, refs1);

  git('commit', '-q', '--allow-empty', '-m', 'second');
  assert.notEqual(await refsStamp(backend.dirs), refs2);

  git('config', 'branch.another.remote', 'origin');
  assert.notEqual(await configStamp(backend.dirs), config1);

  writeFileSync(join(dir, 'a.txt'), 'stash me\n');
  git('stash', '-q');
  assert.notEqual(await stashStamp(backend.dirs), stash1);
  const [stash] = await backend.stashes();
  assert.equal(stash?.name, 'stash@{0}');
  assert.match(stash!.subject, /^WIP on main/);
});

test('switchBranch', async () => {
  await backend.switchBranch('feature/x');
  assert.equal((await backend.status()).branch.head, 'feature/x');
  await assert.rejects(backend.switchBranch('does-not-exist'));
});

test('stage, unstage and writeIndex', async () => {
  const entry = async (path: string) => (await backend.status()).entries.find((e) => e.path === path);
  writeFileSync(join(dir, 'a.txt'), 'changed\n');
  writeFileSync(join(dir, 'b[1].txt'), 'new\n');
  await backend.stage(['a.txt', 'b[1].txt']);
  assert.equal((await entry('a.txt'))?.index, 'M');
  assert.equal((await entry('b[1].txt'))?.index, 'A');

  await backend.unstage(['a.txt', 'b[1].txt']);
  assert.deepEqual([(await entry('a.txt'))?.index, (await entry('a.txt'))?.worktree], ['.', 'M']);
  assert.equal((await entry('b[1].txt'))?.kind, 'untracked');

  await backend.writeIndex('a.txt', '100644', Buffer.from('partial\n'));
  assert.equal((await backend.object(':a.txt'))?.data.toString(), 'partial\n');
  const e = await entry('a.txt');
  assert.deepEqual([e?.index, e?.worktree], ['M', 'M']);

  await backend.commit('partial commit\n\nbody');
  const head = await backend.object('HEAD');
  assert.match(head!.data.toString(), /\n\npartial commit\n\nbody\n?$/);
  assert.equal((await backend.object('HEAD:a.txt'))?.data.toString(), 'partial\n');
  git('checkout', '-q', '--', 'a.txt');
  rmSync(join(dir, 'b[1].txt'));
});

test('unstage works before the first commit', async () => {
  const fresh = realpathSync(mkdtempSync(join(tmpdir(), 'amonhen-test-')));
  try {
    execFileSync('git', ['init', '-q', '-b', 'main'], { cwd: fresh });
    writeFileSync(join(fresh, 'x'), 'x\n');
    const b = await CliBackend.open(new GitExecutor('git'), fresh);
    await b.stage(['x']);
    assert.equal((await b.status()).entries[0]?.index, 'A');
    await b.unstage(['x']);
    assert.equal((await b.status()).entries[0]?.kind, 'untracked');
    b.dispose();
  } finally {
    rmSync(fresh, { recursive: true, force: true });
  }
});

test('cat-file sees index updates made after it started', async () => {
  writeFileSync(join(dir, 'a.txt'), 'first\n');
  git('add', 'a.txt');
  assert.equal((await backend.object(':a.txt'))?.data.toString(), 'first\n');
  writeFileSync(join(dir, 'a.txt'), 'second\n');
  git('add', 'a.txt');
  assert.equal((await backend.object(':a.txt'))?.data.toString(), 'second\n');
  git('reset', '-q', '--hard');
});
