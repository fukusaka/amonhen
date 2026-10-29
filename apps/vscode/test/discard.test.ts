import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import {
  existsSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  rmSync,
  statSync,
  writeFileSync,
  chmodSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, test } from 'node:test';
import { computeDiff } from '../src/diff/fileDiff.ts';
import { CliBackend } from '../src/git/backend.ts';
import { GitExecutor } from '../src/git/exec.ts';
import { discardFiles, discardLines } from '../src/state/discard.ts';
import { UndoConflictError, UndoLog } from '../src/state/undo.ts';
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
const read = (p: string) => readFileSync(join(dir, p), 'utf8');

beforeEach(async () => {
  dir = realpathSync.native(mkdtempSync(join(tmpdir(), 'amonhen-discard-')));
  initMain(dir);
  writeFileSync(join(dir, 'a.txt'), 'one\ntwo\nthree\n');
  writeFileSync(join(dir, 'run.sh'), '#!/bin/sh\n');
  chmodSync(join(dir, 'run.sh'), 0o755);
  git('add', '.');
  git('commit', '-q', '-m', 'base');
  backend = await CliBackend.open(new GitExecutor('git'), dir);
  undo = new UndoLog(backend);
});

afterEach(async () => {
  await backend.dispose();
  rmSync(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
});

test('discard modified and untracked files, then undo brings them back', async () => {
  writeFileSync(join(dir, 'a.txt'), 'changed\n');
  writeFileSync(join(dir, 'run.sh'), '#!/bin/sh\necho hi\n');
  writeFileSync(join(dir, 'new.txt'), 'new\n');
  await discardFiles(backend, undo, [
    { group: 'unstaged', path: 'a.txt', letter: 'M' },
    { group: 'unstaged', path: 'run.sh', letter: 'M' },
    { group: 'unstaged', path: 'new.txt', letter: 'U' },
  ]);
  assert.equal(read('a.txt'), 'one\ntwo\nthree\n');
  assert.equal(existsSync(join(dir, 'new.txt')), false);
  assert.equal(git('status', '--porcelain'), '');
  assert.equal((await undo.peek())?.label, 'Discard 3 files');

  await undo.undo();
  assert.equal(read('a.txt'), 'changed\n');
  assert.equal(read('run.sh'), '#!/bin/sh\necho hi\n');
  // Windows には実行ビットがない。
  if (process.platform !== 'win32') assert.equal(statSync(join(dir, 'run.sh')).mode & 0o111, 0o111);
  assert.equal(read('new.txt'), 'new\n');
});

test('discard a deleted file restores it', async () => {
  rmSync(join(dir, 'a.txt'));
  await discardFiles(backend, undo, [{ group: 'unstaged', path: 'a.txt', letter: 'D' }]);
  assert.equal(read('a.txt'), 'one\ntwo\nthree\n');
  await undo.undo();
  assert.equal(existsSync(join(dir, 'a.txt')), false);
});

test('discard selected lines only', async () => {
  writeFileSync(join(dir, 'a.txt'), 'ONE\ntwo\nTHREE\nfour\n');
  const index = Buffer.from(git('show', ':a.txt') + '\n');
  const computed = computeDiff('a.txt', index, readFileSync(join(dir, 'a.txt')));
  // THREE への変更（old 3 の削除と new 3 の追加）だけを破棄する
  await discardLines(backend, undo, 'a.txt', computed, [
    { kind: '-', old: 3 },
    { kind: '+', new: 3 },
  ]);
  assert.equal(read('a.txt'), 'ONE\ntwo\nthree\nfour\n');
  await undo.undo();
  assert.equal(read('a.txt'), 'ONE\ntwo\nTHREE\nfour\n');
});

test('undo refuses when the file changed after discarding', async () => {
  writeFileSync(join(dir, 'a.txt'), 'changed\n');
  await discardFiles(backend, undo, [{ group: 'unstaged', path: 'a.txt', letter: 'M' }]);
  writeFileSync(join(dir, 'a.txt'), 'edited again\n');
  await assert.rejects(undo.undo(), UndoConflictError);
  assert.equal(read('a.txt'), 'edited again\n');
});
