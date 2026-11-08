import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { FileDiff } from '../src/diff/fileDiff.ts';
import { isGenerated, lineCounts, VIEWED_COMMITS, withViewed, type ViewedStore } from '../src/history/stackedFiles.ts';

test('lockfiles and minified files are generated, by their name only', () => {
  assert.ok(isGenerated('Cargo.lock'));
  assert.ok(isGenerated('web/package-lock.json'));
  assert.ok(isGenerated('dist/app.min.js'));
  assert.ok(!isGenerated('src/Cargo.lock.rs'));
  assert.ok(!isGenerated('lock/main.rs'));
});

test('line counts add up the added and removed lines of every hunk', () => {
  const line = (kind: '+' | '-' | ' ') => ({ kind, text: '', old: 1, new: 1 });
  const diff = {
    path: 'a',
    version: '',
    hunks: [
      { header: '', lines: [line('+'), line('-'), line(' ')] },
      { header: '', lines: [line('+'), line('+')] },
    ],
  } as unknown as FileDiff;
  assert.deepEqual(lineCounts(diff), { added: 3, removed: 1 });
});

test('viewed files are kept per key, and an emptied key is forgotten', () => {
  let store: ViewedStore = { order: [], files: {} };
  store = withViewed(store, 'c1', 'a.txt', true);
  store = withViewed(store, 'c2', 'b.txt', true);
  store = withViewed(store, 'c1', 'c.txt', true);
  assert.deepEqual(store.order, ['c2', 'c1']);
  assert.deepEqual(store.files['c1'], ['a.txt', 'c.txt']);
  store = withViewed(store, 'c2', 'b.txt', false);
  assert.deepEqual(store.order, ['c1']);
  assert.equal(store.files['c2'], undefined);
});

test('only the most recent commits keep their viewed files', () => {
  let store: ViewedStore = { order: [], files: {} };
  for (let i = 0; i <= VIEWED_COMMITS; i++) store = withViewed(store, `c${i}`, 'a', true);
  assert.equal(store.order.length, VIEWED_COMMITS);
  assert.equal(store.files['c0'], undefined);
  assert.deepEqual(store.files[`c${VIEWED_COMMITS}`], ['a']);
});
