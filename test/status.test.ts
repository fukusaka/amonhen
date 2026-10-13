import assert from 'node:assert/strict';
import { test } from 'node:test';
import { conflictSides, parseStatus } from '../src/git/status.ts';

test('parseStatus: headers and every record type', () => {
  const out = [
    '# branch.oid 1111111111111111111111111111111111111111',
    '# branch.head feature/x',
    '# branch.upstream origin/feature/x',
    '# branch.ab +2 -3',
    '# stash 4',
    '1 .M N... 100644 100644 100644 aaaa bbbb src/with space.ts',
    '2 R. N... 100644 100755 100755 aaaa bbbb R100 new name.ts',
    'old name.ts',
    'u UU N... 100644 100644 100644 100644 a b c conflict.ts',
    '1 .M S.M. 160000 160000 160000 aaaa bbbb sub',
    '? untracked file',
    '',
  ].join('\0');
  const r = parseStatus(out);
  assert.deepEqual(r.branch, {
    oid: '1111111111111111111111111111111111111111',
    head: 'feature/x',
    upstream: 'origin/feature/x',
    ahead: 2,
    behind: 3,
  });
  assert.equal(r.stashCount, 4);
  assert.deepEqual(r.entries, [
    { kind: 'changed', path: 'src/with space.ts', index: '.', worktree: 'M', submodule: undefined, modeIndex: '100644' },
    {
      kind: 'renamed',
      path: 'new name.ts',
      origPath: 'old name.ts',
      index: 'R',
      worktree: '.',
      submodule: undefined,
      modeIndex: '100755',
    },
    { kind: 'unmerged', path: 'conflict.ts', index: 'U', worktree: 'U', submodule: undefined },
    { kind: 'changed', path: 'sub', index: '.', worktree: 'M', submodule: 'S.M.', modeIndex: '160000' },
    { kind: 'untracked', path: 'untracked file', index: '?', worktree: '?' },
  ]);
});

test('parseStatus: initial commit and detached HEAD', () => {
  const r = parseStatus('# branch.oid (initial)\0# branch.head (detached)\0');
  assert.deepEqual(r.branch, {});
});

test('conflictSides', () => {
  assert.deepEqual(conflictSides('UD'), { label: 'deleted by them', base: true, current: true, incoming: false });
  assert.equal(conflictSides('AA').base, false);
  assert.equal(conflictSides('??').label, 'both modified');
});
