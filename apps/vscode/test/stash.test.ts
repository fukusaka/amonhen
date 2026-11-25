/** stash の行に出すこと */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { relativeTime, stashBaseRelation, stashBaseText, stashBranch, stashDescription, stashTitle } from '../src/git/stash.ts';

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

test('stashBaseRelation says where the base is against HEAD, as the app does', () => {
  assert.equal(stashBaseRelation(0, 0), 'at HEAD');
  assert.equal(stashBaseRelation(3, 0), '3 behind HEAD');
  assert.equal(stashBaseRelation(3, 1), 'not in HEAD’s history');
});

test('relativeTime is short and in the past', () => {
  const now = 1_000_000_000_000;
  assert.equal(relativeTime(now / 1000 - 3 * 86_400, now), '3 days ago');
  assert.equal(relativeTime(now / 1000 - 90, now), '1 min. ago');
});

test('stashDescription follows the app’s row: subject, branch, base, time, untracked, base ID', () => {
  const now = 1_000_000_000_000;
  const stash = { subject: 'On main: wip', date: now / 1000 - 2 * 3_600 };
  assert.equal(stashDescription(stash, undefined, now), 'wip · main · 2 hr. ago');
  assert.equal(
    stashDescription(stash, { base: '0123456789abcdef', ahead: 1, behind: 0, untracked: true }, now),
    'wip · main · 1 behind HEAD · 2 hr. ago · with untracked files · 0123456',
  );
});

test('stashTitle drops the branch, and the short ID after WIP', () => {
  assert.equal(stashTitle('On main: my message'), 'my message');
  assert.equal(stashTitle('WIP on main: 1234567 add a'), 'WIP · add a');
  assert.equal(stashTitle('autostash'), 'autostash');
});
