import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, test } from 'node:test';
import { CliBackend } from '../src/git/backend.ts';
import { GitExecutor } from '../src/git/exec.ts';
import { mergeInto, rebaseOnto } from '../src/state/operations.ts';
import { UndoLog } from '../src/state/undo.ts';

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
  dir = realpathSync(mkdtempSync(join(tmpdir(), 'amonhen-ops-')));
  git('init', '-q', '-b', 'main');
  commitFile('a.txt', 'base\n', 'base');
  backend = await CliBackend.open(new GitExecutor('git'), dir);
  undo = new UndoLog(backend);
});

afterEach(() => {
  backend.dispose();
  rmSync(dir, { recursive: true, force: true });
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
  await backend.abort('merge');
});
