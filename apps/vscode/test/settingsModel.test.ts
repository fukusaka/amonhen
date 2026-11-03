/** リポジトリの設定の画面のもと */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { ConfigView, normalizedBool, pullMethod, pullWrites, type ConfigEntry } from '../src/settings/model.ts';

const entry = (key: string, value: string, scope: string): ConfigEntry => ({ key, value, scope, origin: `file:${scope}` });

test('ConfigView picks the value in effect, the override here and the one above', () => {
  const c = new ConfigView([entry('fetch.prune', 'false', 'global'), entry('Fetch.Prune', 'true', 'local')]);
  assert.equal(c.effective('fetch.prune')?.value, 'true');
  assert.equal(c.local('fetch.prune'), 'true');
  assert.equal(c.inherited('fetch.prune')?.value, 'false');
  assert.deepEqual(c.origin('fetch.prune'), { label: 'this repository', file: 'local' });
  assert.deepEqual(c.origin('rerere.enabled'), { label: 'git’s default' });
});

test('normalizedBool accepts git’s spellings', () => {
  assert.equal(normalizedBool('Yes'), 'true');
  assert.equal(normalizedBool('off'), 'false');
  assert.equal(normalizedBool('maybe'), undefined);
});

test('pullMethod and pullWrites follow pull.ff=only first, then pull.rebase', () => {
  assert.equal(pullMethod(new ConfigView([])), 'merge');
  assert.equal(pullMethod(new ConfigView([entry('pull.rebase', 'merges', 'global')])), 'rebase');
  const ff = new ConfigView([entry('pull.rebase', 'true', 'global'), entry('pull.ff', 'only', 'local')]);
  assert.equal(pullMethod(ff), 'ffOnly');
  assert.deepEqual(pullWrites(ff, 'rebase'), [
    ['pull.ff', null],
    ['pull.rebase', 'true'],
  ]);
  assert.deepEqual(pullWrites(new ConfigView([]), 'ffOnly'), [
    ['pull.rebase', 'false'],
    ['pull.ff', 'only'],
  ]);
});
