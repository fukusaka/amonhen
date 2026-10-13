import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { StatusEntry } from '../src/git/status.ts';
import { predictStage, predictUnstage } from '../src/state/predict.ts';

const e = (path: string, index: string, worktree: string, kind: StatusEntry['kind'] = 'changed'): StatusEntry => ({
  kind,
  path,
  index,
  worktree,
});

test('predictStage moves worktree changes into the index', () => {
  const entries = [e('m', '.', 'M'), e('u', '?', '?', 'untracked'), e('am', 'A', 'M'), e('d', '.', 'D'), e('x', '.', 'M')];
  const out = predictStage(entries, new Set(['m', 'u', 'am', 'd']));
  assert.deepEqual(
    out.map((x) => [x.path, x.kind, x.index, x.worktree]),
    [
      ['m', 'changed', 'M', '.'],
      ['u', 'changed', 'A', '.'],
      ['am', 'changed', 'A', '.'],
      ['d', 'changed', 'D', '.'],
      ['x', 'changed', '.', 'M'],
    ],
  );
});

test('predictUnstage moves index changes back to the worktree', () => {
  const entries = [e('m', 'M', '.'), e('mm', 'M', 'M'), e('a', 'A', '.'), e('ad', 'A', 'D'), e('d', 'D', '.')];
  const out = predictUnstage(entries, new Set(['m', 'mm', 'a', 'ad', 'd']));
  assert.deepEqual(
    out.map((x) => [x.path, x.kind, x.index, x.worktree]),
    [
      ['m', 'changed', '.', 'M'],
      ['mm', 'changed', '.', 'M'],
      ['a', 'untracked', '?', '?'],
      ['d', 'changed', '.', 'D'],
    ],
  );
});
