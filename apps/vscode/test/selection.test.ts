import assert from 'node:assert/strict';
import { test } from 'node:test';
import { extendTo, isContiguous, singleRow, toggleRow } from '../src/history/selection.ts';

const rows = (s: { rows: ReadonlySet<number> }) => [...s.rows].sort((a, b) => a - b);

test('toggling adds a row and removes it, keeping the last one', () => {
  const two = toggleRow(singleRow(3), 7);
  assert.deepEqual(rows(two), [3, 7]);
  assert.equal(two.focus, 7);
  const back = toggleRow(two, 7);
  assert.deepEqual(rows(back), [3]);
  assert.equal(back.focus, 3);
  assert.deepEqual(rows(toggleRow(back, 3)), [3]);
});

test('extending replaces the selection with the range from the anchor, either way', () => {
  const down = extendTo(singleRow(5), 8);
  assert.deepEqual(rows(down), [5, 6, 7, 8]);
  assert.equal(down.anchor, 5);
  assert.deepEqual(rows(extendTo(down, 3)), [3, 4, 5]);
});

test('extending starts from the row last toggled', () => {
  const toggled = toggleRow(singleRow(1), 10);
  assert.deepEqual(rows(extendTo(toggled, 12)), [10, 11, 12]);
});

test('tells contiguous rows from scattered ones', () => {
  assert.ok(isContiguous(new Set([4, 2, 3])));
  assert.ok(!isContiguous(new Set([2, 4])));
});
