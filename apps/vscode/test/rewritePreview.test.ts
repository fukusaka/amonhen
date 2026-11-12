import assert from 'node:assert/strict';
import { test } from 'node:test';
import { drawingLines, forecastLine, previewLines, stackedAlternative, type RewritePreview } from '../src/history/rewritePreview.ts';

const preview = (p: Partial<RewritePreview>): RewritePreview => ({ moving: [], staying: [], ...p });

test('previewLines says which branches move or stay, which are pushed and what conflicts', () => {
  assert.deepEqual(
    previewLines(
      preview({
        moving: [{ name: 'a', upstream: 'origin/a', pushed: true }, { name: 'b', upstream: 'origin/b', pushed: false }],
        staying: [
          { name: 'c', reason: 'setting' },
          { name: 'd', reason: 'git' },
          { name: 'e', reason: 'checkedOut' },
          { name: 'f', reason: 'off' },
        ],
        current: 'main',
        currentUpstream: 'origin/main',
        currentPushed: true,
        conflicts: [{ oid: '0123456789abcdef', files: ['x', 'y', 'z', 'w'] }],
      }),
    ),
    [
      'Also moves a and b.',
      'c stays where it is (rebase.updateRefs is off).',
      'd stays where it is (moving it needs git 2.38).',
      'e stays where it is (checked out in another worktree).',
      'f stays where it is (not moved this time).',
      'main and a are pushed, so pushing afterwards needs Force Push.',
      '⚠︎ Conflicts expected at 01234567: x, y, z, …',
    ],
  );
  assert.deepEqual(
    previewLines(preview({ conflicts: [{ oid: 'aaaaaaaa1', files: ['x'] }, { oid: 'b', files: ['y'] }] })),
    ['⚠︎ Conflicts expected at 2 commits, first at aaaaaaaa: x'],
  );
  assert.deepEqual(previewLines(preview({ conflicts: [] })), ['No conflicts expected.']);
  // 上流があっても、書き換えるコミットを持っていなければ Force Push は要らない
  assert.deepEqual(previewLines(preview({ current: 'main', currentUpstream: 'origin/main', currentPushed: false })), []);
  // 調べられなければ（git が古い）コンフリクトの行はない
  assert.deepEqual(previewLines(preview({ conflicts: null })), []);
});

test('stackedAlternative offers the opposite of the setting only when a branch is affected', () => {
  const updateRefs = (configured: boolean, available = true) => ({ configured, available });
  assert.equal(stackedAlternative(preview({ staying: [{ name: 'a', reason: 'setting' }], updateRefs: updateRefs(false) })), true);
  assert.equal(stackedAlternative(preview({ moving: [{ name: 'a' }], updateRefs: updateRefs(true) })), false);
  assert.equal(stackedAlternative(preview({ staying: [{ name: 'a', reason: 'off' }], updateRefs: updateRefs(true) })), false);
  // ほかの worktree の checkout は設定では変わらない。git が古ければ選べない
  assert.equal(stackedAlternative(preview({ staying: [{ name: 'a', reason: 'checkedOut' }], updateRefs: updateRefs(false) })), undefined);
  assert.equal(stackedAlternative(preview({ staying: [{ name: 'a', reason: 'git' }], updateRefs: updateRefs(false, false) })), undefined);
  assert.equal(stackedAlternative(preview({ updateRefs: updateRefs(false) })), undefined);
});

test('draws the rebase before and after: the current and stacked branches move to the replayed commits', () => {
  const c = (oid: string, subject: string) => ({ oid, subject });
  const lines = drawingLines({
    moving: [{ name: 'part1' }],
    staying: [],
    current: 'topic',
    drawing: {
      picks: [c('a1', 'First'), c('a2', 'Second')],
      onto: [c('m2', 'Upstream two'), c('m1', 'Upstream one')],
      ontoMore: 3,
      base: c('b', 'Base'),
      labels: [
        { oid: 'a2', name: 'topic', kind: 'local' },
        { oid: 'a1', name: 'part1', kind: 'local' },
        { oid: 'm2', name: 'main', kind: 'local' },
        { oid: 'm2', name: 'origin/main', kind: 'remote' },
      ],
    },
  });
  assert.deepEqual(lines, [
    'Now:',
    '  ○ Second  [topic]',
    '  ○ First  [part1]',
    '  ● Upstream two  [main, origin/main]',
    '  ● Upstream one',
    '  ⋮ 3 more',
    '  ● Base',
    'After the rebase:',
    '  ○ Second′  [topic]',
    '  ○ First′  [part1]',
    '  ● Upstream two  [main, origin/main]',
    '  ● Upstream one',
    '  ⋮ 3 more',
    '  ● Base',
  ]);
  assert.deepEqual(drawingLines({ moving: [], staying: [] }), []);
});

test('forecastLine says which files conflict, and that a rebase’s is an estimate', () => {
  assert.equal(forecastLine([[]], false), 'No conflicts expected.');
  assert.equal(
    forecastLine([['b.txt', 'a.txt'], ['a.txt', 'd.txt', 'c.txt']], false),
    '⚠︎ Conflicts expected in 4 files: a.txt, b.txt, c.txt, …',
  );
  assert.equal(
    forecastLine([['a.txt']], true),
    '⚠︎ Conflicts expected in 1 file: a.txt Rebase applies the commits one at a time, so this is only an estimate.',
  );
  assert.equal(forecastLine(undefined, true), 'Can’t check for conflicts ahead (needs git 2.38 or newer).');
});
