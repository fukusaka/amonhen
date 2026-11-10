/** status で git の fsmonitor を使うときの設定 */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fsmonitorConfig } from '../src/git/fsmonitor.ts';

test('uses git’s fsmonitor--daemon on macOS and Windows', () => {
  for (const platform of ['darwin', 'win32'] as const) {
    assert.deepEqual(fsmonitorConfig({ enabled: true }, platform, false, undefined), ['-c', 'core.fsmonitor=true']);
    assert.deepEqual(fsmonitorConfig({ enabled: false }, platform, false, undefined), []);
  }
});

test('uses Watchman through the engine elsewhere, with git 2.26 or newer', () => {
  const on = { enabled: true, engine: "/opt/it's/amonhen" };
  assert.deepEqual(fsmonitorConfig(on, 'linux', true, [2, 26]), [
    '-c',
    `core.fsmonitor='/opt/it'\\''s/amonhen' fsmonitor-watchman`,
    '-c',
    'core.fsmonitorHookVersion=2',
  ]);
  assert.deepEqual(fsmonitorConfig(on, 'freebsd', true, [3, 0]).length, 4);
  // Watchman がない・git が古い・版が読めない・エンジンがない・切ってある
  assert.deepEqual(fsmonitorConfig(on, 'linux', false, [2, 40]), []);
  assert.deepEqual(fsmonitorConfig(on, 'linux', true, [2, 25]), []);
  assert.deepEqual(fsmonitorConfig(on, 'linux', true, undefined), []);
  assert.deepEqual(fsmonitorConfig({ enabled: true }, 'linux', true, [2, 40]), []);
  assert.deepEqual(fsmonitorConfig({ ...on, enabled: false }, 'linux', true, [2, 40]), []);
});
