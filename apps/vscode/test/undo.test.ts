import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, test } from 'node:test';
import { CliBackend } from '../src/git/backend.ts';
import { GitExecutor } from '../src/git/exec.ts';
import { UndoConflictError, UndoLog } from '../src/state/undo.ts';

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
let log: UndoLog;

function git(...args: string[]): string {
  return execFileSync('git', args, { cwd: dir, encoding: 'utf8' }).trim();
}

function commitFile(name: string, content: string, message: string): void {
  writeFileSync(join(dir, name), content);
  git('add', name);
  git('commit', '-q', '-m', message);
}

beforeEach(async () => {
  dir = realpathSync.native(mkdtempSync(join(tmpdir(), 'amonhen-undo-')));
  git('init', '-q', '-b', 'main');
  commitFile('a.txt', 'base\n', 'base');
  backend = await CliBackend.open(new GitExecutor('git'), dir);
  log = new UndoLog(backend);
});

afterEach(() => {
  backend.dispose();
  rmSync(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
});

test('undo a commit keeps its changes staged', async () => {
  const base = git('rev-parse', 'HEAD');
  writeFileSync(join(dir, 'a.txt'), 'changed\n');
  git('add', 'a.txt');
  await log.record('Commit', ['refs/heads/main'], 'soft', () => backend.commit('change'));
  assert.equal((await log.peek())?.label, 'Commit');

  const undone = await log.undo();
  assert.equal(undone?.label, 'Commit');
  assert.equal(git('rev-parse', 'HEAD'), base);
  assert.equal(git('diff', '--cached', '--name-only'), 'a.txt');
  assert.equal(await log.peek(), undefined);
});

test('undo a fast-forward of a branch that is not checked out', async () => {
  git('branch', 'feature');
  const old = git('rev-parse', 'feature');
  commitFile('b.txt', 'b\n', 'on main');
  const main = git('rev-parse', 'main');
  await log.record('Fast-forward feature', ['refs/heads/feature'], 'keep', () =>
    backend.updateRef('refs/heads/feature', main, old),
  );
  await log.undo();
  assert.equal(git('rev-parse', 'feature'), old);
  assert.equal(git('branch', '--show-current'), 'main');
});

test('undo a merge that switched to the target branch first', async () => {
  git('switch', '-q', '-c', 'feature');
  commitFile('f.txt', 'f\n', 'feature work');
  git('switch', '-q', 'main');
  commitFile('m.txt', 'm\n', 'main work');
  const main = git('rev-parse', 'main');
  git('switch', '-q', 'feature');
  // 作業中の変更は Undo のあとも残る
  writeFileSync(join(dir, 'wip.txt'), 'wip\n');

  await log.record('Merge feature into main', ['refs/heads/main'], 'keep', async () => {
    git('switch', '-q', 'main');
    git('merge', '-q', '--no-edit', 'feature');
  });
  assert.notEqual(git('rev-parse', 'main'), main);

  await log.undo();
  assert.equal(git('rev-parse', 'main'), main);
  assert.equal(git('branch', '--show-current'), 'feature');
  assert.equal(readFileSync(join(dir, 'wip.txt'), 'utf8'), 'wip\n');
});

test('undo deletes a branch created by the operation', async () => {
  await log.record('Create topic', ['refs/heads/topic'], 'keep', async () => {
    git('switch', '-q', '-c', 'topic');
    commitFile('t.txt', 't\n', 'topic');
  });
  await log.undo();
  assert.equal(git('branch', '--show-current'), 'main');
  assert.equal(git('branch', '--list', 'topic'), '');
});

test('undo refuses when the branch moved afterwards', async () => {
  writeFileSync(join(dir, 'a.txt'), 'x\n');
  git('add', 'a.txt');
  await log.record('Commit', ['refs/heads/main'], 'soft', () => backend.commit('x'));
  git('commit', '-q', '--allow-empty', '-m', 'later');
  const head = git('rev-parse', 'HEAD');
  await assert.rejects(log.undo(), UndoConflictError);
  assert.equal(git('rev-parse', 'HEAD'), head);
  assert.equal((await log.peek())?.label, 'Commit');
});

test('operations that fail or change nothing are not recorded', async () => {
  await assert.rejects(log.record('Fail', ['refs/heads/main'], 'keep', () => backend.reset('keep', 'nope')));
  await log.record('Nothing', ['refs/heads/main'], 'keep', async () => {});
  assert.equal(await log.peek(), undefined);
});

test('the log survives a reload', async () => {
  writeFileSync(join(dir, 'a.txt'), 'x\n');
  git('add', 'a.txt');
  await log.record('Commit', ['refs/heads/main'], 'soft', () => backend.commit('x'));
  const reloaded = new UndoLog(backend);
  assert.equal((await reloaded.peek())?.label, 'Commit');
});

test('undo a dropped stash', async () => {
  writeFileSync(join(dir, 'a.txt'), 'stashed\n');
  git('stash', '-q');
  const oid = git('rev-parse', 'stash@{0}');
  await log.recordStashDrop('stash@{0}', oid, 'On main: stashed');
  assert.equal(git('stash', 'list'), '');
  await log.undo();
  assert.equal(git('rev-parse', 'stash@{0}'), oid);
});
