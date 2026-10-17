import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, test } from 'node:test';
import { CliBackend } from '../src/git/backend.ts';
import { GitExecutor } from '../src/git/exec.ts';
import { rewriteHistory } from '../src/state/rewrite.ts';
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
/** コミット名 → oid */
const oids: Record<string, string> = {};

function git(...args: string[]): string {
  return execFileSync('git', args, { cwd: dir, encoding: 'utf8' }).trim();
}

function commitFile(name: string, content: string, message: string): void {
  writeFileSync(join(dir, name), content);
  git('add', name);
  git('commit', '-q', '-m', message);
  oids[message] = git('rev-parse', 'HEAD');
}

/** 新しい順のコミットメッセージ（subject） */
function subjects(): string[] {
  return git('log', '--format=%s').split('\n');
}

beforeEach(async () => {
  dir = realpathSync.native(mkdtempSync(join(tmpdir(), 'amonhen-rewrite-')));
  git('init', '-q', '-b', 'main');
  commitFile('a.txt', 'a\n', 'A');
  commitFile('b.txt', 'b\n', 'B');
  commitFile('c.txt', 'c\n', 'C');
  commitFile('d.txt', 'd\n', 'D');
  backend = await CliBackend.open(new GitExecutor('git'), dir);
  undo = new UndoLog(backend);
});

afterEach(() => {
  backend.dispose();
  rmSync(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
});

test('reword keeps the tree, and undo restores the original commit', async () => {
  const head = git('rev-parse', 'HEAD');
  const tree = git('rev-parse', 'HEAD^{tree}');
  // 作業中の変更は autostash で退避して戻される
  writeFileSync(join(dir, 'a.txt'), 'dirty\n');
  const result = await rewriteHistory(backend, undo, 'Reword C', {
    kind: 'reword',
    oid: oids.C!,
    message: 'C reworded\n\nwith a body\n',
  });
  assert.equal(result, 'done');
  assert.deepEqual(subjects(), ['D', 'C reworded', 'B', 'A']);
  assert.equal(git('log', '-1', '--format=%b', 'HEAD~1'), 'with a body');
  assert.equal(git('rev-parse', 'HEAD^{tree}'), tree);
  assert.equal(readFileSync(join(dir, 'a.txt'), 'utf8'), 'dirty\n');

  await undo.undo();
  assert.equal(git('rev-parse', 'HEAD'), head);
});

test('squash, fixup and drop', async () => {
  await rewriteHistory(backend, undo, 'Squash', { kind: 'squash', oid: oids.C!, message: 'B and C' });
  assert.deepEqual(subjects(), ['D', 'B and C', 'A']);
  assert.equal(git('show', '--format=', '--name-only', 'HEAD~1'), 'b.txt\nc.txt');

  const d = git('rev-parse', 'HEAD');
  await rewriteHistory(backend, undo, 'Fixup', { kind: 'fixup', oid: d });
  assert.deepEqual(subjects(), ['B and C', 'A']);

  const top = git('rev-parse', 'HEAD');
  await rewriteHistory(backend, undo, 'Drop', { kind: 'drop', oid: top });
  assert.deepEqual(subjects(), ['A']);
  assert.equal(existsSync(join(dir, 'c.txt')), false);
});

test('move reorders commits', async () => {
  await rewriteHistory(backend, undo, 'Move', { kind: 'move', oid: oids.D!, to: 2 });
  assert.deepEqual(subjects(), ['C', 'B', 'D', 'A']);
});

test('reword the root commit', async () => {
  await rewriteHistory(backend, undo, 'Reword A', { kind: 'reword', oid: oids.A!, message: 'Root' });
  assert.deepEqual(subjects(), ['D', 'C', 'B', 'Root']);
});

test('edit stops, and undo is recorded once the rebase finishes', async () => {
  const head = git('rev-parse', 'HEAD');
  assert.equal(await rewriteHistory(backend, undo, 'Edit B', { kind: 'edit', oid: oids.B! }), 'stopped');
  assert.equal(await backend.sequenceInProgress(), 'rebase');
  assert.equal(git('log', '-1', '--format=%s'), 'B');
  // 止まっている間は確定しない
  assert.equal(await undo.settlePending(), false);

  commitFile('extra.txt', 'x\n', 'Extra');
  assert.equal(await backend.continueSequence('rebase'), 'done');
  assert.deepEqual(subjects(), ['D', 'C', 'Extra', 'B', 'A']);
  assert.equal(await undo.settlePending(), true);
  assert.equal((await undo.peek())?.label, 'Edit B');

  await undo.undo();
  assert.equal(git('rev-parse', 'HEAD'), head);
});

test('split puts the changes back into the working tree', async () => {
  writeFileSync(join(dir, 'c2.txt'), 'c2\n');
  git('add', 'c2.txt');
  git('commit', '-q', '--amend', '--no-edit');
  const head = git('rev-parse', 'HEAD');
  assert.equal(await rewriteHistory(backend, undo, 'Split D', { kind: 'split', oid: head }), 'stopped');
  assert.equal(git('log', '-1', '--format=%s'), 'C');
  assert.equal(git('status', '--porcelain'), '?? c2.txt\n?? d.txt'.split('\n').sort().join('\n'));

  commitFile('d.txt', 'd\n', 'D1');
  commitFile('c2.txt', 'c2\n', 'D2');
  assert.equal(await backend.continueSequence('rebase'), 'done');
  assert.deepEqual(subjects(), ['D2', 'D1', 'C', 'B', 'A']);
});

test('a conflict is reported and not recorded; abort restores everything', async () => {
  commitFile('a.txt', 'a2\n', 'A2');
  commitFile('a.txt', 'a3\n', 'A3');
  const head = git('rev-parse', 'HEAD');
  // A3 を A2 より前に動かすとコンフリクトする
  assert.equal(await rewriteHistory(backend, undo, 'Move', { kind: 'move', oid: oids.A3!, to: 1 }), 'conflict');
  await backend.abort('rebase');
  assert.equal(git('rev-parse', 'HEAD'), head);
  assert.equal(await undo.settlePending(), false);
  assert.equal(await undo.peek(), undefined);
});

test('refuses while another rebase is in progress', async () => {
  await rewriteHistory(backend, undo, 'Edit', { kind: 'edit', oid: oids.C! });
  await assert.rejects(rewriteHistory(backend, undo, 'Drop', { kind: 'drop', oid: oids.D! }));
  await backend.abort('rebase');
});
