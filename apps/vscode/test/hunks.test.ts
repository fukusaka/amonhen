import assert from 'node:assert/strict';
import { test } from 'node:test';
import { buildHunks } from '../src/diff/hunks.ts';
import { applyLineSets, diffLines, splitLines } from '../src/diff/lineDiff.ts';

const L = (s: string) => splitLines(s);
const signs = (h: { lines: { kind: string }[] }) => h.lines.map((l) => l.kind).join('');

test('nearby changes share a hunk, distant ones do not', () => {
  const base = L(Array.from({ length: 30 }, (_, i) => `${i}\n`).join(''));
  const target = [...base];
  target[2] = 'two\n';
  target[5] = 'five\n';
  target[25] = 'twenty-five\n';
  const hunks = buildHunks(base.length, target.length, diffLines(base, target));
  assert.equal(hunks.length, 2);
  assert.deepEqual(
    { b: hunks[0]!.baseStart, n: hunks[0]!.baseCount, s: signs(hunks[0]!) },
    { b: 0, n: 9, s: '  -+  -+   ' },
  );
  assert.deepEqual({ b: hunks[1]!.baseStart, s: signs(hunks[1]!) }, { b: 22, s: '   -+   ' });
});

test('hunks of a new file and at the end of a file', () => {
  assert.equal(signs(buildHunks(0, 2, diffLines([], L('a\nb\n')))[0]!), '++');
  const hunks = buildHunks(3, 2, diffLines(L('a\nb\nc\n'), L('a\nb\n')));
  assert.deepEqual(hunks[0]!.lines, [
    { kind: ' ', base: 0, target: 0 },
    { kind: ' ', base: 1, target: 1 },
    { kind: '-', base: 2 },
  ]);
});

test('applyLineSets applies removals and additions chosen independently', () => {
  const base = L('keep\nold1\nold2\nend\n');
  const target = L('keep\nnew1\nnew2\nend\n');
  const changes = diffLines(base, target);
  assert.equal(applyLineSets(base, target, changes, new Set([1]), new Set([2])), 'keep\nold2\nnew2\nend\n');
  assert.equal(applyLineSets(base, target, changes, new Set([1, 2]), new Set([1, 2])), 'keep\nnew1\nnew2\nend\n');
  assert.equal(applyLineSets(base, target, changes, new Set(), new Set()), 'keep\nold1\nold2\nend\n');
});
