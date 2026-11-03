/** stash の行に出すこと */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { stashBaseText, stashBranch } from '../src/git/stash.ts';

test('stashBranch reads the branch from git’s stash message', () => {
  assert.equal(stashBranch('WIP on main: 1234567 add a'), 'main');
  assert.equal(stashBranch('On feature/x: my message'), 'feature/x');
  assert.equal(stashBranch('WIP on (no branch): 1234567 add a'), undefined);
  assert.equal(stashBranch('autostash'), undefined);
});

test('stashBaseText says how far HEAD has moved from the base', () => {
  assert.equal(stashBaseText(0, 0), undefined);
  assert.equal(stashBaseText(2, 0), 'HEAD has 2 commits since');
  assert.equal(stashBaseText(1, 1), 'HEAD has 1 commit since, 1 commit is not on HEAD');
});
