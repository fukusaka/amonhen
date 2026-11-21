import assert from 'node:assert/strict';
import { chmodSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { GitError, GitExecutor, configUnreadable } from '../src/git/exec.ts';

/** 初めの `failures` 回は設定ファイルを読めずに止まり、その後は `ok` を出す git の代わり */
function flakyGit(failures: number, message: string): { path: string; calls: () => number } {
  const dir = mkdtempSync(join(tmpdir(), 'amonhen-exec-retry-'));
  const count = join(dir, 'count');
  writeFileSync(count, '0');
  const git = join(dir, 'git');
  writeFileSync(
    git,
    `#!/bin/sh
n=$(cat '${count}'); n=$((n + 1)); echo $n > '${count}'
if [ $n -le ${failures} ]; then
  echo "warning: unable to access '.git/config': Permission denied" >&2
  echo "fatal: ${message}" >&2
  exit 128
fi
echo ok
`,
  );
  chmodSync(git, 0o755);
  return { path: git, calls: () => Number(readFileSync(count, 'utf8').trim()) };
}

const UNREADABLE = 'unknown error occurred while reading the configuration files';

test('runs git again when it stopped before reading its config', { skip: process.platform === 'win32' }, async () => {
  const git = flakyGit(2, UNREADABLE);
  const executor = new GitExecutor(git.path);
  assert.equal((await executor.text(['status'], { cwd: tmpdir() })).trim(), 'ok');
  assert.equal(git.calls(), 3);
});

test('gives up on an unreadable config after a few tries', { skip: process.platform === 'win32' }, async () => {
  const git = flakyGit(10, UNREADABLE);
  const executor = new GitExecutor(git.path);
  await assert.rejects(executor.text(['status'], { cwd: tmpdir() }), configUnreadable);
  assert.equal(git.calls(), 4);
});

test('does not run git again for other failures', { skip: process.platform === 'win32' }, async () => {
  const git = flakyGit(1, 'not a git repository');
  const executor = new GitExecutor(git.path);
  await assert.rejects(executor.text(['status'], { cwd: tmpdir() }), (e: unknown) => e instanceof GitError && !configUnreadable(e));
  assert.equal(git.calls(), 1);
});
