import assert from 'node:assert/strict';
import { test } from 'node:test';
import { describePull, gitPullMode, type PullSituation, pullSources } from '../src/sync/pullPlan.ts';

test('reads how git pulls: the branch’s rebase, then pull.rebase, then pull.ff=only', () => {
  assert.deepEqual(gitPullMode('main', new Map()), { mode: 'merge', unset: true });
  assert.deepEqual(gitPullMode('main', new Map([['pull.rebase', 'true']])), { mode: 'rebase', unset: false });
  assert.deepEqual(gitPullMode('main', new Map([['pull.rebase', 'merges']])), { mode: 'rebase', unset: false });
  assert.deepEqual(
    gitPullMode('main', new Map([['pull.rebase', 'true'], ['branch.main.rebase', 'false']])),
    { mode: 'merge', unset: false },
  );
  assert.deepEqual(gitPullMode('main', new Map([['pull.ff', 'only']])), { mode: 'fastForwardOnly', unset: false });
  assert.deepEqual(gitPullMode('main', new Map([['pull.ff', 'false']])), { mode: 'merge', unset: false });
});

const diverged: PullSituation = {
  branch: 'main',
  upstream: 'origin/main',
  incoming: 3,
  outgoing: 2,
  changed: 0,
  byGit: 'merge',
  gitRefusesDiverged: false,
};

test('says what each way does when the branches have diverged', () => {
  assert.deepEqual(describePull('merge', diverged), {
    lines: ['You have 2 local commits, so this makes a merge commit.', 'You can undo this.'],
    blocked: false,
  });
  assert.match(describePull('rebase', diverged).lines[0]!, /replayed on top/);
  assert.equal(describePull('fastForwardOnly', diverged).blocked, true);
  assert.equal(describePull('git', { ...diverged, gitRefusesDiverged: true }).blocked, true);
  assert.equal(describePull('merge', { ...diverged, gitRefusesDiverged: true }).blocked, false);
});

test('fast-forwards, says when there is nothing new, and warns about uncommitted changes', () => {
  assert.deepEqual(describePull('fastForwardOnly', { ...diverged, outgoing: 0 }).lines[0], 'Moves main forward (fast-forward).');
  assert.match(describePull('git', { ...diverged, incoming: 0 }).lines[0]!, /^Nothing new/);
  assert.match(describePull('merge', { ...diverged, changed: 1 }).lines[1]!, /^1 file has uncommitted changes/);
  assert.match(describePull('git', { ...diverged, upstream: undefined, source: 'origin/main' }).lines[0]!, /^Sets origin\/main as the upstream/);
});

test('offers the remote branch of the same name first as the source', () => {
  assert.deepEqual(pullSources('topic', ['origin/HEAD', 'origin/main', 'fork/topic', 'origin/topic']), [
    'fork/topic',
    'origin/topic',
    'origin/main',
  ]);
});
