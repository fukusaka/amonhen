import assert from 'node:assert/strict';
import { test } from 'node:test';
import { applyLineSets, diffLines, splitLines, type LineChange } from '../src/diff/lineDiff.ts';

const L = (s: string) => splitLines(s);

/** 変更に含まれる行をすべて選んだときの集合 */
function everything(changes: readonly LineChange[]): [Set<number>, Set<number>] {
  const removed = new Set<number>();
  const added = new Set<number>();
  for (const c of changes) {
    for (let i = c.baseStart; i < c.baseEnd; i++) removed.add(i);
    for (let i = c.targetStart; i < c.targetEnd; i++) added.add(i);
  }
  return [removed, added];
}

/** 変更をすべて当てると target に戻ることを確かめる */
function roundTrip(base: string, target: string): void {
  const b = L(base);
  const t = L(target);
  const changes = diffLines(b, t);
  assert.equal(
    applyLineSets(b, t, changes, ...everything(changes)),
    target,
    `${JSON.stringify(base)} -> ${JSON.stringify(target)}`,
  );
}

test('splitLines keeps line endings', () => {
  assert.deepEqual(L('a\nb\r\nc'), ['a\n', 'b\r\n', 'c']);
  assert.deepEqual(L('a\n'), ['a\n']);
  assert.deepEqual(L(''), []);
});

test('diffLines finds minimal changes', () => {
  assert.deepEqual(diffLines(L('a\nb\nc\n'), L('a\nx\nc\n')), [
    { baseStart: 1, baseEnd: 2, targetStart: 1, targetEnd: 2 },
  ]);
  assert.deepEqual(diffLines(L('a\nb\nc\n'), L('a\nc\n')), [
    { baseStart: 1, baseEnd: 2, targetStart: 1, targetEnd: 1 },
  ]);
  assert.deepEqual(diffLines(L('a\nc\n'), L('a\nb\nc\n')), [
    { baseStart: 1, baseEnd: 1, targetStart: 1, targetEnd: 2 },
  ]);
  assert.deepEqual(diffLines(L('a\nb\n'), L('a\nb\n')), []);
  assert.deepEqual(diffLines([], L('a\n')), [{ baseStart: 0, baseEnd: 0, targetStart: 0, targetEnd: 1 }]);
});

test('applying every hunk reproduces the target', () => {
  const cases: [string, string][] = [
    ['a\nb\nc\n', 'a\nx\nc\n'],
    ['a\nb\nc\nd\ne\n', 'x\nb\nd\ny\ne\nz\n'],
    ['', 'new\nfile\n'],
    ['gone\n', ''],
    ['a', 'a\nb'],
    ['a\nb', 'a\nb\n'],
    ['1\n2\n3\n4\n5\n6\n7\n8\n', '8\n7\n6\n5\n4\n3\n2\n1\n'],
    ['a\r\nb\r\n', 'a\r\nc\r\nb\r\n'],
  ];
  for (const [base, target] of cases) {
    roundTrip(base, target);
  }
});

test('random edits round-trip', () => {
  let seed = 1;
  const rand = (n: number) => {
    seed = (seed * 1103515245 + 12345) & 0x7fffffff;
    return seed % n;
  };
  for (let i = 0; i < 300; i++) {
    const base = Array.from({ length: rand(20) }, () => `${rand(5)}\n`).join('');
    const target = Array.from({ length: rand(20) }, () => `${rand(5)}\n`).join('');
    roundTrip(base, target);
  }
});

test('only the chosen change is applied', () => {
  const base = L('a\nb\nc\nd\ne\n');
  const target = L('a\nB\nc\nd\nE\n');
  const changes = diffLines(base, target);
  assert.equal(applyLineSets(base, target, changes, new Set([4]), new Set([4])), 'a\nb\nc\nd\nE\n');
});

test('adding a line after a final line without newline inserts one', () => {
  const base = L('a');
  const target = L('a\nb\n');
  const changes = diffLines(base, target);
  assert.equal(applyLineSets(base, target, changes, new Set(), new Set([1])), 'a\nb\n');
});

test('too many edits fall back to one change and still round-trip', () => {
  const base = Array.from({ length: 3000 }, (_, i) => `a${i}\n`).join('');
  const target = Array.from({ length: 3000 }, (_, i) => `b${i}\n`).join('');
  assert.equal(diffLines(L(base), L(target)).length, 1);
  roundTrip(base, target);
});
