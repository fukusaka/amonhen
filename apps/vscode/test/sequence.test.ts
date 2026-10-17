/** merge・rebase・push・fetch を一時ディレクトリの実リポジトリで確かめる */
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, test } from 'node:test';
import { CliBackend } from '../src/git/backend.ts';
import { GitExecutor } from '../src/git/exec.ts';
import { pullCurrent, pushCurrent } from '../src/state/operations.ts';
import { UndoLog } from '../src/state/undo.ts';

Object.assign(process.env, {
  GIT_CONFIG_GLOBAL: '/dev/null',
  GIT_CONFIG_NOSYSTEM: '1',
  GIT_AUTHOR_NAME: 'a',
  GIT_AUTHOR_EMAIL: 'a@example.com',
  GIT_COMMITTER_NAME: 'a',
  GIT_COMMITTER_EMAIL: 'a@example.com',
});

let tmp: string;
let dir: string;
let backend: CliBackend;

function git(...args: string[]): string {
  return execFileSync('git', args, { cwd: dir, encoding: 'utf8' }).trim();
}

function commitFile(name: string, content: string, message: string): void {
  writeFileSync(join(dir, name), content);
  git('add', name);
  git('commit', '-q', '-m', message);
}

beforeEach(async () => {
  tmp = realpathSync.native(mkdtempSync(join(tmpdir(), 'amonhen-seq-')));
  dir = join(tmp, 'work');
  execFileSync('git', ['init', '-q', '--bare', '-b', 'main', join(tmp, 'remote.git')]);
  execFileSync('git', ['init', '-q', '-b', 'main', dir]);
  commitFile('a.txt', 'base\n', 'base');
  git('remote', 'add', 'origin', join(tmp, 'remote.git'));
  backend = await CliBackend.open(new GitExecutor('git'), dir);
});

afterEach(() => {
  backend.dispose();
  rmSync(tmp, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
});

/** main と feature が同じファイルの同じ行を変えた状態を作る */
function divergeWithConflict(): void {
  git('switch', '-q', '-c', 'feature');
  commitFile('a.txt', 'feature\n', 'feature');
  git('switch', '-q', 'main');
  commitFile('a.txt', 'main\n', 'main');
}

test('merge without conflicts', async () => {
  git('switch', '-q', '-c', 'feature');
  commitFile('f.txt', 'f\n', 'feature');
  git('switch', '-q', 'main');
  commitFile('m.txt', 'm\n', 'main');
  assert.equal(await backend.merge('feature'), 'done');
  assert.equal(git('rev-list', '--count', '--merges', 'HEAD'), '1');
  assert.equal(await backend.sequenceInProgress(), undefined);
});

test('merge conflict can be aborted', async () => {
  divergeWithConflict();
  const main = git('rev-parse', 'main');
  assert.equal(await backend.merge('feature'), 'conflict');
  assert.equal(await backend.sequenceInProgress(), 'merge');
  await backend.abort('merge');
  assert.equal(await backend.sequenceInProgress(), undefined);
  assert.equal(git('rev-parse', 'HEAD'), main);
});

test('rebase a branch that is not checked out', async () => {
  git('switch', '-q', '-c', 'feature');
  commitFile('f.txt', 'f\n', 'feature');
  git('switch', '-q', 'main');
  commitFile('m.txt', 'm\n', 'main');
  assert.equal(await backend.rebase('main', 'feature'), 'done');
  assert.equal(git('branch', '--show-current'), 'feature');
  assert.equal(git('merge-base', 'main', 'feature'), git('rev-parse', 'main'));
});

test('rebase conflict can be aborted', async () => {
  divergeWithConflict();
  git('switch', '-q', 'feature');
  const feature = git('rev-parse', 'feature');
  assert.equal(await backend.rebase('main'), 'conflict');
  assert.equal(await backend.sequenceInProgress(), 'rebase');
  await backend.abort('rebase');
  assert.equal(git('rev-parse', 'HEAD'), feature);
});

test('failures other than conflicts are reported as errors', async () => {
  await assert.rejects(backend.merge('no-such-branch'));
  await assert.rejects(backend.rebase('no-such-branch'));
});

test('push, force push and fetch', async () => {
  await backend.push('origin', 'main', 'main', { setUpstream: true });
  assert.equal(git('rev-parse', 'origin/main'), git('rev-parse', 'main'));
  assert.equal(git('rev-parse', '--abbrev-ref', 'main@{upstream}'), 'origin/main');

  git('commit', '-q', '--amend', '-m', 'rewritten');
  await assert.rejects(backend.push('origin', 'main', 'main'), 'non-fast-forward is rejected');
  await backend.push('origin', 'main', 'main', { force: true });
  assert.equal(
    execFileSync('git', ['rev-parse', 'main'], { cwd: join(tmp, 'remote.git'), encoding: 'utf8' }).trim(),
    git('rev-parse', 'main'),
  );

  git('update-ref', '-d', 'refs/remotes/origin/main');
  await backend.fetch();
  assert.equal(git('rev-parse', 'origin/main'), git('rev-parse', 'main'));
});

test('pushCurrent sets the upstream, and pullCurrent fast-forwards from it', async () => {
  assert.deepEqual(await pushCurrent(backend), { remote: 'origin', branch: 'main' });
  assert.equal(git('rev-parse', '--abbrev-ref', 'main@{upstream}'), 'origin/main');

  // 別の clone から 1 つ進めておく
  const other = join(tmp, 'other');
  execFileSync('git', ['clone', '-q', join(tmp, 'remote.git'), other]);
  writeFileSync(join(other, 'b.txt'), 'b\n');
  execFileSync('git', ['add', 'b.txt'], { cwd: other });
  execFileSync('git', ['commit', '-q', '-m', 'from other'], { cwd: other });
  execFileSync('git', ['push', '-q'], { cwd: other });

  const undo = new UndoLog(backend);
  assert.equal(await pullCurrent(backend, undo), 'done');
  assert.equal(git('log', '-1', '--format=%s'), 'from other');
  assert.equal((await undo.peek())?.label, 'Fast-forward main to origin/main');
});

test('pullCurrent without an upstream', async () => {
  assert.equal(await pullCurrent(backend, new UndoLog(backend)), 'no-upstream');
});

test('trackBranch and deleteBranch', async () => {
  await backend.push('origin', 'main', 'topic');
  await backend.fetch();
  await backend.trackBranch('origin/topic');
  assert.equal(git('branch', '--show-current'), 'topic');
  assert.equal(git('rev-parse', '--abbrev-ref', 'topic@{upstream}'), 'origin/topic');
  git('switch', '-q', 'main');
  git('switch', '-q', '-c', 'unmerged');
  git('commit', '-q', '--allow-empty', '-m', 'only here');
  git('switch', '-q', 'main');
  await assert.rejects(backend.deleteBranch('unmerged', false));
  await backend.deleteBranch('unmerged', true);
  await backend.deleteBranch('topic', false);
  assert.equal(git('branch', '--list', 'unmerged', 'topic'), '');
});

test('createBranch, renameBranch, createTag and checkBranchName', async () => {
  const head = git('rev-parse', 'HEAD');
  await backend.createBranch('topic', 'main', false);
  assert.equal(git('branch', '--show-current'), 'main');
  await backend.createBranch('work', 'topic', true);
  assert.equal(git('branch', '--show-current'), 'work');
  await backend.renameBranch('work', 'renamed');
  assert.equal(git('branch', '--show-current'), 'renamed');

  await backend.createTag('light', head);
  await backend.createTag('heavy', head, 'Release notes\n');
  assert.equal(git('cat-file', '-t', 'light'), 'commit');
  assert.equal(git('cat-file', '-t', 'heavy'), 'tag');
  assert.equal(git('tag', '-l', '--format=%(contents:subject)', 'heavy'), 'Release notes');

  assert.equal(await backend.checkBranchName('feature/ok'), undefined);
  assert.ok(await backend.checkBranchName('bad..name'));
  assert.ok(await backend.checkBranchName('with space'));
});

test('stash push, apply, pop, drop and store', async () => {
  writeFileSync(join(dir, 'a.txt'), 'staged\n');
  git('add', 'a.txt');
  writeFileSync(join(dir, 'new.txt'), 'untracked\n');
  await backend.stashPush({ message: 'work in progress', includeUntracked: true });
  assert.equal(git('status', '--porcelain'), '');
  const [stash] = await backend.stashes();
  assert.equal(stash?.subject, 'On main: work in progress');

  assert.equal(await backend.stashApply('stash@{0}', false), 'done');
  // --index で、stage していた状態も戻る
  assert.equal(git('status', '--porcelain'), 'M  a.txt\n?? new.txt');
  assert.equal((await backend.stashes()).length, 1);
  git('reset', '-q', '--hard');
  rmSync(join(dir, 'new.txt'));

  assert.equal(await backend.stashApply('stash@{0}', true), 'done');
  assert.equal((await backend.stashes()).length, 0);
  await backend.stashPush({});
  const [again] = await backend.stashes();
  await backend.stashDrop('stash@{0}');
  assert.equal((await backend.stashes()).length, 0);
  await backend.stashStore(again!.oid, again!.subject);
  assert.equal((await backend.stashes())[0]?.oid, again!.oid);
});

test('stash apply reports conflicts', async () => {
  writeFileSync(join(dir, 'a.txt'), 'stashed\n');
  await backend.stashPush({});
  writeFileSync(join(dir, 'a.txt'), 'committed\n');
  git('commit', '-q', '-am', 'conflicting');
  assert.equal(await backend.stashApply('stash@{0}', true), 'conflict');
  // pop でコンフリクトしたら stash は残る
  assert.equal((await backend.stashes()).length, 1);
});

test('cherry-pick and revert', async () => {
  git('switch', '-q', '-c', 'feature');
  commitFile('f.txt', 'f\n', 'feature work');
  const picked = git('rev-parse', 'HEAD');
  git('switch', '-q', 'main');
  assert.equal(await backend.cherryPick(picked, false), 'done');
  assert.equal(git('log', '-1', '--format=%s'), 'feature work');
  assert.equal(await backend.revert(git('rev-parse', 'HEAD'), false), 'done');
  assert.match(git('log', '-1', '--format=%s'), /^Revert "feature work"/);
});

test('cherry-pick conflict stops as a sequence', async () => {
  divergeWithConflict();
  const feature = git('rev-parse', 'feature');
  assert.equal(await backend.cherryPick(feature, false), 'conflict');
  assert.equal(await backend.sequenceInProgress(), 'cherry-pick');
  await backend.abort('cherry-pick');
  assert.equal(await backend.sequenceInProgress(), undefined);
});

test('updateSubmodules initializes a submodule', async () => {
  const lib = join(tmp, 'lib');
  execFileSync('git', ['init', '-q', '-b', 'main', lib]);
  writeFileSync(join(lib, 'l.txt'), 'l\n');
  execFileSync('git', ['add', '.'], { cwd: lib });
  execFileSync('git', ['commit', '-q', '-m', 'lib'], { cwd: lib });
  git('-c', 'protocol.file.allow=always', 'submodule', 'add', '-q', lib, 'vendor/lib');
  git('commit', '-q', '-m', 'add submodule');
  // 初期化していない clone を作る
  const clone = join(tmp, 'clone');
  execFileSync('git', ['clone', '-q', dir, clone]);
  const b = await CliBackend.open(new GitExecutor('git'), clone);
  try {
    process.env.GIT_CONFIG_PARAMETERS = "'protocol.file.allow=always'";
    await b.updateSubmodules(['vendor/lib']);
    assert.equal(readFileSync(join(clone, 'vendor/lib/l.txt'), 'utf8'), 'l\n');
  } finally {
    delete process.env.GIT_CONFIG_PARAMETERS;
    b.dispose();
  }
});
