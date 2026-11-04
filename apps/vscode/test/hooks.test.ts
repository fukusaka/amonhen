/** フックの言い方と置き場所 */
import assert from 'node:assert/strict';
import { join } from 'node:path';
import { test } from 'node:test';
import { hooksDirectory } from '../src/settings/hookFiles.ts';
import { hookWhen } from '../src/settings/hooks.ts';

test('hookWhen names when a hook runs', () => {
  assert.equal(hookWhen('pre-commit'), 'Before a commit');
  assert.equal(hookWhen('post-receive'), 'On the receiving side');
  assert.equal(hookWhen('p4-changelist'), '');
});

test('hooksDirectory follows core.hooksPath', () => {
  assert.equal(hooksDirectory(null, '/w', '/w/.git', '/h'), join('/w/.git', 'hooks'));
  assert.equal(hooksDirectory('.githooks', '/w', '/w/.git', '/h'), join('/w', '.githooks'));
  assert.equal(hooksDirectory('~/hooks', '/w', '/w/.git', '/h'), join('/h', 'hooks'));
  assert.equal(hooksDirectory('/srv/hooks', '/w', '/w/.git', '/h'), '/srv/hooks');
});
