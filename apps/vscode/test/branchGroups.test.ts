/** ローカルブランチの並べ方（アプリのサイドバーの Branches と同じ決まり） */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { arrangeBranches, pinnedOrder, type Arranged, type BranchFacts } from '../src/git/branchGroups.ts';

const DAY = 86_400;
const NOW = 1_800_000_000;
const fact = (name: string, extra: Partial<BranchFacts> = {}): BranchFacts => ({
  name,
  time: NOW - DAY,
  merged: false,
  atBase: false,
  mine: true,
  parent: null,
  archived: false,
  pinned: null,
  ...extra,
});
const names = (list: Arranged[]): unknown => list.map((a) => (a.stack.length ? [a.name, names(a.stack)] : a.name));

test('arrangeBranches puts favorites, the current stack and the base first, then active, inactive and archived', () => {
  const branches = [
    fact('main'),
    fact('fav-b', { pinned: 2 }),
    fact('fav-a', { pinned: 1, parent: 'main' }),
    fact('base-of-head', { time: NOW - 2 * DAY }),
    fact('head', { parent: 'base-of-head' }),
    fact('new', { time: NOW - 10 }),
    fact('old', { time: NOW - 200 * DAY }),
    fact('old-but-elsewhere', { time: NOW - 200 * DAY }),
    fact('gone', { archived: true }),
    fact('on-new', { parent: 'new' }),
  ];
  const a = arrangeBranches(
    branches.map((b) => b.name),
    'head',
    { base: 'main', branches },
    NOW,
    (n) => n === 'old-but-elsewhere',
  );
  assert.deepEqual(names(a.pinned), ['fav-a', 'fav-b']);
  assert.equal(a.pinned[0]!.stackedOn, 'main');
  // 今のブランチは、その積み重ねの根元ごと一番上に（根元を先、既定のブランチを後）
  assert.deepEqual(names(a.top), [['base-of-head', ['head']], 'main']);
  assert.deepEqual(names(a.active), [['new', ['on-new']], 'old-but-elsewhere']);
  assert.deepEqual(names(a.inactive), ['old']);
  assert.deepEqual(names(a.archived), ['gone']);
});

test('arrangeBranches leaves a branch on an archived base in its own place, and the base may be a remote name', () => {
  const branches = [fact('main'), fact('dead', { archived: true }), fact('on-dead', { parent: 'dead' })];
  const a = arrangeBranches(['main', 'dead', 'on-dead'], 'main', { base: 'origin/main', branches }, NOW);
  assert.deepEqual(names(a.top), ['main']);
  assert.deepEqual(names(a.active), ['on-dead']);
  assert.deepEqual(names(a.archived), ['dead']);
});

test('pinnedOrder adds new favorites at the end and removes others', () => {
  assert.deepEqual(pinnedOrder(['a', 'b'], ['c', 'a'], true), ['a', 'b', 'c']);
  assert.deepEqual(pinnedOrder(['a', 'b', 'c'], ['b'], false), ['a', 'c']);
});
