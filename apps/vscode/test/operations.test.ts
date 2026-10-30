import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, join } from 'node:path';
import { afterEach, beforeEach, test } from 'node:test';
import { CliBackend } from '../src/git/backend.ts';
import { GitExecutor } from '../src/git/exec.ts';
import { fastForwardBranch, mergeInto, mergeSeveral, publishChoices, pushCurrent, rebaseOnto } from '../src/state/operations.ts';
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

let dir: string;
let backend: CliBackend;
let undo: UndoLog;

function git(...args: string[]): string {
  return execFileSync('git', args, { cwd: dir, encoding: 'utf8' }).trim();
}

function commitFile(name: string, content: string, message: string): void {
  writeFileSync(join(dir, name), content);
  git('add', name);
  git('commit', '-q', '-m', message);
}

beforeEach(async () => {
  dir = realpathSync.native(mkdtempSync(join(tmpdir(), 'amonhen-ops-')));
  initMain(dir);
  commitFile('a.txt', 'base\n', 'base');
  backend = await CliBackend.open(new GitExecutor('git'), dir);
  undo = new UndoLog(backend);
});

afterEach(async () => {
  await backend.dispose();
  rmSync(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
});

const H = (b: string) => `refs/heads/${b}`;

test('fast-forward a branch that is not checked out without switching', async () => {
  git('branch', 'feature');
  commitFile('m.txt', 'm\n', 'main');
  writeFileSync(join(dir, 'wip.txt'), 'wip\n');
  assert.equal(await mergeInto(backend, undo, H('main'), H('feature')), 'done');
  assert.equal(git('rev-parse', 'feature'), git('rev-parse', 'main'));
  assert.equal(git('branch', '--show-current'), 'main');
  assert.equal((await undo.peek())?.label, 'Fast-forward feature to main');
});

test('merge does not fast-forward a branch checked out in another worktree', async () => {
  git('branch', 'feature');
  commitFile('m.txt', 'm\n', 'main');
  const before = git('rev-parse', 'feature');
  const wt = `${dir}-wt`;
  git('worktree', 'add', '-q', wt, 'feature');
  try {
    await assert.rejects(mergeInto(backend, undo, H('main'), H('feature')), /worktree/);
    assert.equal(git('rev-parse', 'feature'), before);
    assert.equal(await undo.peek(), undefined);
  } finally {
    git('worktree', 'remove', '--force', wt);
  }
});

test('merge into a branch that is not checked out switches to it, and undo switches back', async () => {
  git('switch', '-q', '-c', 'feature');
  commitFile('f.txt', 'f\n', 'feature');
  git('switch', '-q', 'main');
  commitFile('m.txt', 'm\n', 'main');
  git('switch', '-q', 'feature');
  const main = git('rev-parse', 'main');

  assert.equal(await mergeInto(backend, undo, H('feature'), H('main')), 'done');
  assert.equal(git('branch', '--show-current'), 'main');
  assert.equal(git('rev-list', '--count', '--merges', 'main'), '1');
  // git 2.28 より前は、master 以外に merge すると「into main」を付ける
  assert.match(git('log', '-1', '--format=%s', 'main'), /^Merge branch 'feature'( into main)?$/);

  assert.equal((await undo.undo())?.label, 'Merge feature into main');
  assert.equal(git('rev-parse', 'main'), main);
  assert.equal(git('branch', '--show-current'), 'feature');
});

test('rebase onto, and undo restores the original commits', async () => {
  git('switch', '-q', '-c', 'feature');
  commitFile('f.txt', 'f\n', 'feature');
  const feature = git('rev-parse', 'feature');
  git('switch', '-q', 'main');
  commitFile('m.txt', 'm\n', 'main');

  assert.equal(await rebaseOnto(backend, undo, H('feature'), H('main')), 'done');
  assert.equal(git('merge-base', 'main', 'feature'), git('rev-parse', 'main'));
  await undo.undo();
  assert.equal(git('rev-parse', 'feature'), feature);
  assert.equal(git('branch', '--show-current'), 'main');
});

test('nothing to do is reported as up-to-date and not recorded', async () => {
  git('branch', 'feature');
  commitFile('m.txt', 'm\n', 'main');
  assert.equal(await mergeInto(backend, undo, H('feature'), H('main')), 'up-to-date');
  assert.equal(await rebaseOnto(backend, undo, H('main'), H('feature')), 'up-to-date');
  assert.equal(await undo.peek(), undefined);
});

test('a conflict is reported and not recorded', async () => {
  git('switch', '-q', '-c', 'feature');
  commitFile('a.txt', 'feature\n', 'feature');
  git('switch', '-q', 'main');
  commitFile('a.txt', 'main\n', 'main');
  assert.equal(await mergeInto(backend, undo, H('feature'), H('main')), 'conflict');
  assert.equal(await undo.peek(), undefined);
  // merge の途中に始めた merge・rebase・cherry-pick は git が断る。コンフリクトではなく、何の途中かを言って失敗する
  const busy = /merge の途中です/;
  await assert.rejects(mergeInto(backend, undo, H('feature'), H('main')), busy);
  await assert.rejects(backend.rebase('feature'), busy);
  await assert.rejects(backend.cherryPick(git('rev-parse', 'feature'), false), busy);
  assert.equal(await backend.sequenceInProgress(), 'merge');
  await backend.abort('merge');
});

/** main から分かれた a・b（それぞれ自分のファイルを変える）と、main と同じ行を変える x */
function octopusBranches(): void {
  for (const [name, file] of [['a', 'a-file'], ['b', 'b-file'], ['x', 'a.txt']] as const) {
    git('switch', '-q', '-c', name, 'main');
    commitFile(file, `${name}\n`, name);
  }
  git('switch', '-q', 'main');
  commitFile('a.txt', 'main\n', 'main');
}

const parents = () => git('log', '-1', '--format=%P').split(' ').length;

test('several branches merge in one commit, and one undo puts it back', async () => {
  octopusBranches();
  const head = git('rev-parse', 'HEAD');
  assert.deepEqual(await mergeSeveral(backend, undo, [H('a'), H('b')], false), { result: 'done', remaining: [] });
  assert.equal(parents(), 3);
  // git 2.28 より前は、master 以外に merge すると「into main」を付ける
  assert.match(git('log', '-1', '--format=%s'), /^Merge branches 'a' and 'b'/);
  assert.equal((await undo.peek())?.label, 'Merge a and b');
  await undo.undo();
  assert.equal(git('rev-parse', 'HEAD'), head);
});

test('an octopus that gives up leaves the worktree as it was', async () => {
  octopusBranches();
  const head = git('rev-parse', 'HEAD');
  writeFileSync(join(dir, 'b-file'), 'edited\n');
  await assert.rejects(mergeSeveral(backend, undo, [H('a'), H('x'), H('b')], false), /octopus/);
  assert.equal(git('rev-parse', 'HEAD'), head);
  assert.equal(await backend.sequenceInProgress(), undefined);
  // 作業中の変更は残り、途中まで合わせたものは残らない
  assert.equal(git('status', '--porcelain'), '?? b-file');
  assert.equal(await undo.peek(), undefined);
});

test('a conflict in the last branch of an octopus stops with all of them', async () => {
  octopusBranches();
  assert.equal((await mergeSeveral(backend, undo, [H('a'), H('b'), H('x')], false)).result, 'conflict');
  assert.equal(readFileSync(join(dir, '.git', 'MERGE_HEAD'), 'utf8').trim().split('\n').length, 3);
  assert.equal(await undo.peek(), undefined);
  await backend.abort('merge');
});

test('one by one stops at a conflict, and one undo puts back what was merged', async () => {
  octopusBranches();
  const head = git('rev-parse', 'HEAD');
  const result = await mergeSeveral(backend, undo, [H('a'), H('x'), H('b')], true);
  assert.deepEqual(result, { result: 'conflict', remaining: ['b'] });
  assert.match(git('log', '-1', '--format=%s'), /^Merge branch 'a'/);
  await backend.abort('merge');
  await undo.undo();
  assert.equal(git('rev-parse', 'HEAD'), head);
});

test('the forecast checks each branch on top of the ones before', async () => {
  octopusBranches();
  const steps = await backend.predictMerge(['a', 'x', 'b']);
  const version = git('version').match(/(\d+)\.(\d+)/);
  if (Number(version?.[1]) * 100 + Number(version?.[2]) >= 238) {
    assert.deepEqual(steps, [[], ['a.txt'], []]);
  } else {
    assert.equal(steps, undefined);
  }
  assert.equal(git('status', '--porcelain'), '');
});

/** dir の origin になる bare リポジトリを作り、main を送って upstream にする */
function addOrigin(): string {
  const remote = realpathSync.native(mkdtempSync(join(tmpdir(), 'amonhen-origin-')));
  execFileSync('git', ['init', '-q', '--bare', remote]);
  git('remote', 'add', 'origin', remote);
  git('push', '-q', '-u', 'origin', 'main');
  return remote;
}

test('fast-forward a branch that is not checked out to its upstream, and refuse when it can not', async () => {
  const remote = addOrigin();
  try {
    git('branch', '-q', 'feature');
    git('push', '-q', '-u', 'origin', 'feature');
    // origin の feature だけを進める（手元の feature は置いていく）
    git('checkout', '-q', 'feature');
    commitFile('f.txt', 'f\n', 'feature');
    git('push', '-q', 'origin', 'feature');
    git('reset', '-q', '--hard', 'HEAD~1');
    git('checkout', '-q', 'main');
    assert.equal(await fastForwardBranch(backend, undo, H('feature')), 'done');
    assert.equal(git('rev-parse', 'feature'), git('rev-parse', 'origin/feature'));
    assert.equal(git('branch', '--show-current'), 'main');
    assert.equal(await fastForwardBranch(backend, undo, H('feature')), 'up-to-date');
    assert.equal(await fastForwardBranch(backend, undo, H('main')), 'up-to-date');
    git('branch', '-q', 'solo');
    assert.equal(await fastForwardBranch(backend, undo, H('solo')), 'no-upstream');
    // ほかの worktree でチェックアウトしているブランチは動かさない
    git('branch', '-q', '-f', 'feature', 'main');
    const wt = join(remote, '..', `${basename(remote)}-wt`);
    git('worktree', 'add', '-q', wt, 'feature');
    try {
      await assert.rejects(fastForwardBranch(backend, undo, H('feature')), /worktree/);
      assert.equal(git('rev-parse', 'feature'), git('rev-parse', 'main'));
    } finally {
      git('worktree', 'remove', '--force', wt);
    }
    // 分かれていれば断る
    git('checkout', '-q', 'feature');
    commitFile('g.txt', 'g\n', 'local');
    git('checkout', '-q', 'main');
    assert.equal(await fastForwardBranch(backend, undo, H('feature')), 'diverged');
  } finally {
    rmSync(remote, { recursive: true, force: true });
  }
});

test('push without an upstream asks to publish, and publish prefers pushRemote and pushDefault', async () => {
  const remote = addOrigin();
  try {
    git('checkout', '-q', '-b', 'topic');
    assert.equal(await pushCurrent(backend), 'unpublished');
    git('remote', 'add', 'other', remote);
    assert.deepEqual(await publishChoices(backend, H('topic')), { remotes: ['origin', 'other'], preferred: 'origin' });
    git('config', 'remote.pushDefault', 'other');
    assert.equal((await publishChoices(backend, H('topic'))).preferred, 'other');
    git('config', 'branch.topic.pushRemote', 'origin');
    assert.equal((await publishChoices(backend, H('topic'))).preferred, 'origin');
    git('checkout', '-q', 'main');
    assert.deepEqual(await pushCurrent(backend), { remote: 'origin', branch: 'main' });
  } finally {
    rmSync(remote, { recursive: true, force: true });
  }
});
