import assert from 'node:assert/strict';
import { test } from 'node:test';
import { findAll, findCountText } from '../src/webview/textFind.ts';

test('finds every match, ignoring case, without overlaps', () => {
  assert.deepEqual(findAll('Press Ctrl and ctrl', 'CTRL'), [
    [6, 10],
    [15, 19],
  ]);
  assert.deepEqual(findAll('aaaa', 'aa'), [
    [0, 2],
    [2, 4],
  ]);
  assert.deepEqual(findAll('text', ''), []);
  assert.deepEqual(findAll('text', 'x y'), []);
  // 小文字にすると長さの変わる文字があっても、場所はずれない
  assert.deepEqual(findAll('İstanbul key', 'KEY'), [[9, 12]]);
});

test('the count reads as in the app', () => {
  assert.equal(findCountText('', undefined, 0), '');
  assert.equal(findCountText('x', undefined, 0), 'Not found');
  assert.equal(findCountText('x', 1, 3), '2 / 3');
});
