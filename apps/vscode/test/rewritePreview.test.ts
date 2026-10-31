import assert from 'node:assert/strict';
import { test } from 'node:test';
import { previewLines, stackedAlternative, type RewritePreview } from '../src/history/rewritePreview.ts';

const preview = (p: Partial<RewritePreview>): RewritePreview => ({ moving: [], staying: [], ...p });

test('previewLines says which branches move or stay, which are pushed and what conflicts', () => {
  assert.deepEqual(
    previewLines(
      preview({
        moving: [{ name: 'a', upstream: 'origin/a' }, { name: 'b' }],
        staying: [
          { name: 'c', reason: 'setting' },
          { name: 'd', reason: 'git' },
          { name: 'e', reason: 'checkedOut' },
          { name: 'f', reason: 'off' },
        ],
        current: 'main',
        currentUpstream: 'origin/main',
        conflicts: [{ oid: '0123456789abcdef', files: ['x', 'y', 'z', 'w'] }],
      }),
    ),
    [
      'a、b も一緒に動きます。',
      'c は動きません（rebase.updateRefs がオフ）。',
      'd は動きません（動かすには git 2.38 が要ります）。',
      'e は動きません（ほかの worktree で checkout 中）。',
      'f は動きません（今回は動かさない）。',
      'main、a は push 済みなので、あとの push には Force Push が要ります。',
      '⚠︎ 01234567 でコンフリクトする見込みです: x, y, z, …',
    ],
  );
  assert.deepEqual(
    previewLines(preview({ conflicts: [{ oid: 'aaaaaaaa1', files: ['x'] }, { oid: 'b', files: ['y'] }] })),
    ['⚠︎ 2 コミットでコンフリクトする見込みです。最初は aaaaaaaa: x'],
  );
  assert.deepEqual(previewLines(preview({ conflicts: [] })), ['コンフリクトしない見込みです。']);
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
