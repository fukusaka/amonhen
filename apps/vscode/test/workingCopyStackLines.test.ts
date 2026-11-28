import assert from 'node:assert/strict';
import { test } from 'node:test';
import { blockAfter, linesOf, nextBlock, parseStackLineKey, splitByFile, stackLineKey } from '../src/workingCopy/stackLines.ts';

test('a stacked line key keeps the path and the line key apart', () => {
  const key = stackLineKey('dir/a:b.txt', 'new:7');
  assert.deepEqual(parseStackLineKey(key), { path: 'dir/a:b.txt', line: 'new:7' });
  assert.notEqual(stackLineKey('a', 'old:1'), stackLineKey('b', 'old:1'));
});

test('lines selected across stacked files are split per file in the stack order', () => {
  const keys = [
    stackLineKey('b.txt', 'new:3'),
    stackLineKey('a.txt', 'old:5'),
    stackLineKey('b.txt', 'old:2'),
    stackLineKey('a.txt', 'new:5'),
    stackLineKey('gone.txt', 'new:1'),
    stackLineKey('b.txt', 'new:3'),
  ];
  assert.deepEqual(splitByFile(keys, ['a.txt', 'b.txt', 'c.txt']), [
    { path: 'a.txt', lines: ['old:5', 'new:5'] },
    { path: 'b.txt', lines: ['new:3', 'old:2'] },
  ]);
  assert.deepEqual(splitByFile([], ['a.txt']), []);
});

test('a stacked file draws only its own selected lines', () => {
  const keys = [stackLineKey('a.txt', 'old:5'), stackLineKey('b.txt', 'new:3'), stackLineKey('a.txt', 'new:5')];
  assert.deepEqual([...linesOf(keys, 'a.txt')], ['old:5', 'new:5']);
  assert.deepEqual([...linesOf(keys, 'c.txt')], []);
});

test('J and K step from the selected block across the stacked files', () => {
  const blocks = [
    [stackLineKey('a', 'old:5'), stackLineKey('a', 'new:5')],
    [stackLineKey('b', 'new:2')],
    [stackLineKey('b', 'old:9'), stackLineKey('b', 'new:9')],
  ];
  // 選んでいなければ、見えている所から探す
  assert.equal(nextBlock(blocks, new Set(), true), undefined);
  // 1 つ目のファイルの最後のかたまりから、次のファイルの最初のかたまりへ
  assert.equal(nextBlock(blocks, new Set([stackLineKey('a', 'new:5')]), true), 1);
  assert.equal(nextBlock(blocks, new Set([stackLineKey('b', 'new:2')]), false), 0);
  // いくつかのかたまりにまたがって選んでいれば、J は最後の次、K は最初の前
  const both = new Set([stackLineKey('a', 'old:5'), stackLineKey('b', 'new:2')]);
  assert.equal(nextBlock(blocks, both, true), 2);
  assert.equal(nextBlock(blocks, both, false), -1);
  assert.equal(nextBlock(blocks, new Set([stackLineKey('b', 'new:9')]), true), 3);
});

test('after Space the next change block is selected, across files, and nothing after the last', () => {
  const blocks = [
    [stackLineKey('a', 'old:2'), stackLineKey('a', 'new:2')],
    [stackLineKey('a', 'new:9')],
    [stackLineKey('b', 'old:4')],
  ];
  assert.equal(blockAfter(blocks, new Set([stackLineKey('a', 'new:2')])), 1);
  // 1 つ目のファイルの最後のかたまりから、次のファイルへ
  assert.equal(blockAfter(blocks, new Set([stackLineKey('a', 'new:9')])), 2);
  // いくつかのかたまりにまたがれば、最後のかたまりの次
  assert.equal(blockAfter(blocks, new Set([stackLineKey('a', 'old:2'), stackLineKey('a', 'new:9')])), 2);
  // 最後のかたまりなら何も選ばない（選び直さない）
  assert.equal(blockAfter(blocks, new Set([stackLineKey('b', 'old:4')])), undefined);
  assert.equal(blockAfter(blocks, new Set()), undefined);
  assert.equal(blockAfter([], new Set(['x'])), undefined);
});
