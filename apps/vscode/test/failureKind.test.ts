import assert from 'node:assert/strict';
import { test } from 'node:test';
import { classifyFailure, failureRemedies, failureSummary, failureTitle } from '../src/failure/kind.ts';

// コアの crates/core/src/failure.rs と同じ見分け
const kind = (m: string) => classifyFailure(m)?.kind;

test('tells network failures apart', () => {
  assert.equal(kind("fatal: could not read Username for 'https://github.com': terminal prompts disabled"), 'auth');
  assert.equal(kind('git@github.com: Permission denied (publickey).\nfatal: Could not read from remote repository.'), 'auth');
  assert.equal(kind('Host key verification failed.\nfatal: Could not read from remote repository.'), 'hostKey');
  assert.equal(kind("fatal: unable to access 'https://x/': Could not resolve host: x"), 'network');
  assert.equal(kind(' ! [rejected]        main -> main (fetch first)\nerror: failed to push some refs'), 'rejected');
  assert.equal(kind('fatal: Not possible to fast-forward, aborting.'), 'diverged');
});

test('finds local changes, locks, worktrees and octopus failures', () => {
  assert.equal(kind('error: Your local changes to the following files would be overwritten by checkout:\n\ta.txt'), 'localChanges');
  assert.equal(kind("Trying simple merge with b\nerror: Entry 'b' not uptodate. Cannot merge.\nMerge with strategy octopus failed."), 'localChanges');
  assert.equal(kind('Automated merge did not work.\nShould not be doing an octopus.\nMerge with strategy octopus failed.'), 'octopus');
  assert.deepEqual(classifyFailure("fatal: Unable to create '/r/.git/index.lock': File exists."), { kind: 'locked', path: '/r/.git/index.lock' });
  assert.deepEqual(classifyFailure("fatal: 'topic' is already used by worktree at '/w/topic'"), {
    kind: 'worktreeInUse',
    path: '/w/topic',
    branch: 'topic',
  });
  assert.deepEqual(classifyFailure("fatal: 'feature/x' is already checked out at '/w/x'"), {
    kind: 'worktreeInUse',
    path: '/w/x',
    branch: 'feature/x',
  });
  assert.equal(kind('fatal: something else'), undefined);
});

test('a known failure is said plainly with the next steps', () => {
  assert.match(failureSummary({ kind: 'rejected' })!, /Pull them first/);
  assert.match(failureSummary({ kind: 'locked', path: '/r/.git/index.lock' })!, /\/r\/\.git\/index\.lock/);
  assert.deepEqual(failureRemedies({ kind: 'rejected' }, true), ['pull']);
  assert.deepEqual(failureRemedies({ kind: 'network' }, true), ['retry']);
  assert.deepEqual(failureRemedies({ kind: 'network' }, false), []);
  assert.deepEqual(failureRemedies({ kind: 'noUpstream' }, false), ['setUpstream']);
});

test('a failure is headed by the operation, as in the app', () => {
  assert.equal(failureTitle('push'), 'Couldn’t Push');
  assert.equal(failureTitle('pushTags'), 'Couldn’t Push');
  assert.equal(failureTitle('mergeInto'), 'Couldn’t Merge');
  assert.equal(failureTitle('stashApply'), 'Couldn’t Apply the Stash');
  assert.equal(failureTitle('track'), 'Couldn’t Checkout');
  assert.equal(failureTitle('createTag'), 'Couldn’t Finish');
});

test('a branch checked out in another worktree is named', () => {
  const f = classifyFailure("fatal: 'topic' is already used by worktree at '/w/topic-tree'")!;
  assert.equal(failureSummary(f), 'topic is checked out in the worktree topic-tree. A branch can be checked out in only one worktree.');
  assert.equal(failureSummary({ kind: 'worktreeInUse', path: '/w/t' }, 'main'), 'main is checked out in the worktree t. A branch can be checked out in only one worktree.');
  assert.equal(
    failureSummary({ kind: 'worktreeInUse', path: '/w/t' }),
    'The branch is checked out in the worktree t. A branch can be checked out in only one worktree.',
  );
});
