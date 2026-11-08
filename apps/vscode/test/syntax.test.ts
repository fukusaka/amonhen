import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { Reply } from '../src/daemon/client.ts';
import { applySyntax, charIndex, textSyntaxByLine } from '../src/daemon/diff.ts';
import type { DiffLine } from '../src/diff/fileDiff.ts';

const reply = (spans: number[]) => ({ u32: () => new Uint32Array(spans) }) as unknown as Reply;

test('turns UTF-8 byte positions into string positions', () => {
  assert.equal(charIndex('abc', 2), 2);
  assert.equal(charIndex('日本 x', 6), 2);
  assert.equal(charIndex('😀a', 4), 2);
});

test('puts diff syntax spans on the lines by row', () => {
  const a: DiffLine = { kind: '+', new: 1, text: 'let 名前 = "x";' };
  const b: DiffLine = { kind: ' ', old: 1, new: 2, text: 'fn' };
  // 行 3 の `let`（0..3）と `"x"`（13..16 バイト）、行 5 の `fn`
  const touched = applySyntax(reply([3, 0, 3, 1, 3, 13, 16, 2, 5, 0, 2, 1, 9, 0, 1, 1]), new Map([[3, a], [5, b]]));
  assert.equal(touched.size, 2);
  assert.deepEqual(a.syntax, [
    [0, 3, 1],
    [9, 12, 2],
  ]);
  assert.deepEqual(b.syntax, [[0, 2, 1]]);
});

test('splits text syntax spans by line, cutting the ones that cross lines', () => {
  const lines = ['let a', '/* x', 'y */ b'];
  // `let`（0..3）、複数行のコメント（6..15）
  assert.deepEqual(textSyntaxByLine(reply([0, 0, 3, 1, 0, 6, 15, 3]), lines), [
    [[0, 3, 1]],
    [[0, 4, 3]],
    [[0, 4, 3]],
  ]);
});
