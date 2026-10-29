/** コンフリクトの解決を、一時ディレクトリの実リポジトリで確かめる */
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, test } from 'node:test';
import { CliBackend } from '../src/git/backend.ts';
import { GitExecutor } from '../src/git/exec.ts';
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

function git(...args: string[]): string {
  return execFileSync('git', args, { cwd: dir, encoding: 'utf8' }).trim();
}

function commitAll(message: string): void {
  git('add', '-A');
  git('commit', '-q', '-m', message);
}

beforeEach(async () => {
  dir = realpathSync.native(mkdtempSync(join(tmpdir(), 'amonhen-conflict-')));
  initMain(dir);
  for (const f of ['both.txt', 'gone-there.txt', 'gone-here.txt']) writeFileSync(join(dir, f), 'base\n');
  commitAll('base');
  // feature: both.txt を変更、gone-there.txt を削除、gone-here.txt を変更
  git('switch', '-q', '-c', 'feature');
  writeFileSync(join(dir, 'both.txt'), 'feature\n');
  rmSync(join(dir, 'gone-there.txt'));
  writeFileSync(join(dir, 'gone-here.txt'), 'feature\n');
  commitAll('feature');
  // main: both.txt を変更、gone-there.txt を変更、gone-here.txt を削除
  git('switch', '-q', 'main');
  writeFileSync(join(dir, 'both.txt'), 'main\n');
  writeFileSync(join(dir, 'gone-there.txt'), 'main\n');
  rmSync(join(dir, 'gone-here.txt'));
  commitAll('main');
  backend = await CliBackend.open(new GitExecutor('git'), dir);
});

afterEach(async () => {
  await backend.dispose();
  rmSync(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
});

async function conflicts(): Promise<{ path: string; conflict: string }[]> {
  return (await backend.status()).entries
    .filter((e) => e.kind === 'unmerged')
    .map((e) => ({ path: e.path, conflict: e.index + e.worktree }));
}

test('accept current resolves each kind of conflict', async () => {
  assert.equal(await backend.merge('feature'), 'conflict');
  const files = await conflicts();
  assert.deepEqual(files.map((f) => `${f.path}:${f.conflict}`).sort(), [
    'both.txt:UU',
    'gone-here.txt:DU',
    'gone-there.txt:UD',
  ]);
  await backend.resolveConflicts(files, 'current');
  assert.deepEqual(await conflicts(), []);
  assert.equal(readFileSync(join(dir, 'both.txt'), 'utf8'), 'main\n');
  assert.equal(readFileSync(join(dir, 'gone-there.txt'), 'utf8'), 'main\n');
  assert.equal(existsSync(join(dir, 'gone-here.txt')), false);
  assert.equal(await backend.continueSequence('merge'), 'done');
});

test('accept incoming', async () => {
  await backend.merge('feature');
  await backend.resolveConflicts(await conflicts(), 'incoming');
  assert.deepEqual(await conflicts(), []);
  assert.equal(readFileSync(join(dir, 'both.txt'), 'utf8'), 'feature\n');
  assert.equal(existsSync(join(dir, 'gone-there.txt')), false);
  assert.equal(readFileSync(join(dir, 'gone-here.txt'), 'utf8'), 'feature\n');
});

test('rebase progress and skip', async () => {
  git('switch', '-q', 'feature');
  writeFileSync(join(dir, 'extra.txt'), 'x\n');
  commitAll('extra');
  assert.equal(await backend.rebase('main'), 'conflict');
  assert.deepEqual(await backend.rebaseProgress(), { current: 1, total: 2 });
  // 1 つめ（feature）を飛ばすと、2 つめ（extra）は当たって終わる
  assert.equal(await backend.skipSequence('rebase'), 'done');
  assert.equal(await backend.rebaseProgress(), undefined);
  assert.equal(git('log', '--format=%s', '-2'), 'extra\nmain');
});
