/** worktree の並べ方（アプリの Working Copy の下と同じ決まり） */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { worktreeLayout, worktreeName, type Worktree } from '../src/git/worktrees.ts';

const wt = (path: string, extra: Partial<Worktree> = {}): Worktree => ({
  path,
  head: 'a'.repeat(40),
  branch: worktreeName({ path }),
  detached: false,
  bare: false,
  locked: null,
  prunable: null,
  main: false,
  ...extra,
});

test('worktreeLayout shows nothing for a single worktree', () => {
  assert.deepEqual(worktreeLayout([wt('/r', { main: true })], () => 0, () => true), { shown: [], clean: [] });
});

test('worktreeLayout puts the main one first, then names in natural order, and folds two or more clean ones', () => {
  const list = [wt('/r-w10'), wt('/r-w2'), wt('/r', { main: true }), wt('/r-edit'), wt('/r-busy'), wt('/r-gone', { prunable: 'gone' })];
  const changes = (w: Worktree) => (w.path === '/r-busy' ? 3 : 0);
  const editing = (w: Worktree) => w.path === '/r-edit';
  const { shown, clean } = worktreeLayout(list, changes, editing);
  assert.deepEqual(shown.map(worktreeName), ['r', 'r-busy', 'r-edit', 'r-gone']);
  assert.deepEqual(clean.map(worktreeName), ['r-w2', 'r-w10']);
  // まとめるものが 1 つなら、まとめずに全部出す
  const one = worktreeLayout([wt('/r', { main: true }), wt('/r-a')], () => 0, () => false);
  assert.deepEqual([one.shown.map(worktreeName), one.clean], [['r', 'r-a'], []]);
});

test('worktreeName is the folder name', () => {
  assert.equal(worktreeName({ path: '/home/me/repo-topic/' }), 'repo-topic');
  assert.equal(worktreeName({ path: 'C:\\work\\repo-topic' }), process.platform === 'win32' ? 'repo-topic' : 'C:\\work\\repo-topic');
});
