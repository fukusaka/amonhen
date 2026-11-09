import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, realpathSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { GitExecutor } from '../src/git/exec.ts';
import { initMain } from './git.ts';

// ext:: のリモートで、何も出さないまま待つ git を作る（Windows の ext:: は sh を通らないので除く）
test('stops network git that prints nothing for too long', { skip: process.platform === 'win32' }, async () => {
  const dir = realpathSync(mkdtempSync(join(tmpdir(), 'amonhen-silence-')));
  try {
    initMain(dir);
    // GIT_CONFIG_COUNT は git 2.31 からなので、リポジトリの設定で ext:: を許す（CI は 2.23 でも試す）
    execFileSync('git', ['config', 'protocol.ext.allow', 'always'], { cwd: dir });
    const git = new GitExecutor('git');
    const started = Date.now();
    await assert.rejects(
      git.exec(['fetch', 'ext::sh -c sleep% 30'], {
        cwd: dir,
        network: true,
        silenceMs: 800,
      }),
      /Stopped after 1 seconds without a response/,
    );
    assert.ok(Date.now() - started < 10_000, 'sleep 30 を待たない');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
