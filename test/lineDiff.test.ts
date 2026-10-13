import assert from 'node:assert/strict';
import { test } from 'node:test';
import { applySelection, diffLines, splitLines, type Selection } from '../src/diff/lineDiff.ts';

const L = (s: string) => splitLines(s);

/** 全部の hunk にカーソルを置いて当てると target に戻ることを確かめる */
function roundTrip(base: string, target: string): void {
  const b = L(base);
  const t = L(target);
  const changes = diffLines(b, t);
  const cursors: Selection = {
    side: 'target',
    ranges: changes.map((c) => ({ start: c.targetStart, end: c.targetStart })),
  };
  assert.equal(applySelection(b, t, changes, cursors, true), target, `${JSON.stringify(base)} -> ${JSON.stringify(target)}`);
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
  assert.deepEqual(diffLines(L('a\nb\nc\n'), L('a\nc\n')), [{ baseStart: 1, baseEnd: 2, targetStart: 1, targetEnd: 1 }]);
  assert.deepEqual(diffLines(L('a\nc\n'), L('a\nb\nc\n')), [{ baseStart: 1, baseEnd: 1, targetStart: 1, targetEnd: 2 }]);
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

test('cursor stages only the hunk under it', () => {
  const base = L('a\nb\nc\nd\ne\n');
  const target = L('a\nB\nc\nd\nE\n');
  const changes = diffLines(base, target);
  const out = applySelection(base, target, changes, { side: 'target', ranges: [{ start: 4, end: 4 }] }, true);
  assert.equal(out, 'a\nb\nc\nd\nE\n');
});

test('selected lines stage only those additions and deletions', () => {
  const base = L('keep\nold1\nold2\nend\n');
  const target = L('keep\nnew1\nnew2\nend\n');
  const changes = diffLines(base, target);
  // 追加側の new2 だけを選ぶ：削除はしない
  assert.equal(
    applySelection(base, target, changes, { side: 'target', ranges: [{ start: 2, end: 2 }] }, false),
    'keep\nold1\nold2\nnew2\nend\n',
  );
  // 削除側の old1 だけを選ぶ
  assert.equal(
    applySelection(base, target, changes, { side: 'base', ranges: [{ start: 1, end: 1 }] }, false),
    'keep\nold2\nend\n',
  );
});

test('cursor next to a pure deletion selects it', () => {
  const base = L('a\nb\nc\n');
  const target = L('a\nc\n');
  const changes = diffLines(base, target);
  assert.equal(applySelection(base, target, changes, { side: 'target', ranges: [{ start: 1, end: 1 }] }, true), 'a\nc\n');
  assert.equal(applySelection(base, target, changes, { side: 'target', ranges: [{ start: 0, end: 0 }] }, true), 'a\nc\n');
});

test('adding a line after a final line without newline inserts one', () => {
  const base = L('a');
  const target = L('a\nb\n');
  const changes = diffLines(base, target);
  assert.equal(applySelection(base, target, changes, { side: 'target', ranges: [{ start: 1, end: 1 }] }, false), 'a\nb\n');
});

test('too many edits fall back to one change and still round-trip', () => {
  const base = Array.from({ length: 3000 }, (_, i) => `a${i}\n`).join('');
  const target = Array.from({ length: 3000 }, (_, i) => `b${i}\n`).join('');
  assert.equal(diffLines(L(base), L(target)).length, 1);
  roundTrip(base, target);
});
