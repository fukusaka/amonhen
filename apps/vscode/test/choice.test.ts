import assert from 'node:assert/strict';
import { chmodSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { chooseGit, settingsFile } from '../src/git/choice.ts';
import { GitError, GitExecutor } from '../src/git/exec.ts';

test('finds Amon Hen’s settings where the engine does', () => {
  assert.equal(settingsFile({ XDG_CONFIG_HOME: '/c', HOME: '/h' }, 'linux'), '/c/amonhen/settings.json');
  assert.equal(settingsFile({ HOME: '/h' }, 'darwin'), '/h/.config/amonhen/settings.json');
  assert.equal(settingsFile({ APPDATA: 'C:\\Users\\x\\AppData\\Roaming' }, 'win32').endsWith('settings.json'), true);
});

test('chooses the git as the engine does: AMONHEN_GIT, the settings, then VSCode’s', { skip: process.platform === 'win32' }, async () => {
  const dir = mkdtempSync(join(tmpdir(), 'amonhen-choice-'));
  const env = { XDG_CONFIG_HOME: dir };
  const file = settingsFile(env);
  assert.deepEqual(chooseGit('/vscode/git', env), { path: '/vscode/git', source: 'vscode', problem: undefined });

  const git = join(dir, 'git');
  writeFileSync(git, '#!/bin/sh\necho chosen\n');
  chmodSync(git, 0o755);
  mkdirSync(join(dir, 'amonhen'));
  writeFileSync(file, JSON.stringify({ git, other: 1 }));
  assert.deepEqual(chooseGit('/vscode/git', env), { path: git, source: 'settings', problem: undefined });
  assert.equal(chooseGit('/vscode/git', { ...env, AMONHEN_GIT: 'git' }).source, 'environment');

  // 選んだ git がなければ、VSCode の git に戻さずに理由を返す
  writeFileSync(file, JSON.stringify({ git: '/no/such/git' }));
  const missing = chooseGit('/vscode/git', env);
  assert.equal(missing.path, '/no/such/git');
  assert.match(missing.problem ?? '', /\/no\/such\/git.*settings\.json/);

  writeFileSync(file, '[1]');
  assert.match(chooseGit('/vscode/git', env).problem ?? '', /can’t be read/);
});

test('stops a git command when the chosen git isn’t there', async () => {
  const executor = new GitExecutor(() => ({ path: '/no/such/git', source: 'settings', problem: 'not there' }));
  await assert.rejects(executor.text(['status'], { cwd: tmpdir() }), (e: unknown) => e instanceof GitError && e.message === 'not there');
});
