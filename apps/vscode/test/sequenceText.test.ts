import assert from 'node:assert/strict';
import { test } from 'node:test';
import { sequenceDetail, sequenceTitle, withoutMarkersLead } from '../src/state/sequenceText.ts';

test('the first line says what is in progress and the step', () => {
  assert.equal(sequenceTitle({ kind: 'rebase', branch: 'topic', onto: 'main', current: 2, total: 5 }), 'Rebasing topic onto main · step 2 of 5');
  assert.equal(sequenceTitle({ kind: 'merge', branch: 'main', merging: ['a', 'b'] }), 'Merging a, b into main');
  assert.equal(sequenceTitle({ kind: 'cherry-pick' }), 'Cherry-pick in progress');
  assert.equal(sequenceTitle({ kind: 'revert', branch: 'main' }), 'Reverting in main');
});

test('the second line says why it stopped and what to do next', () => {
  const s = { kind: 'rebase', step: { action: 'pick', subject: 'Fix it' } };
  assert.equal(sequenceDetail(s, 2), '2 files have conflicts at “Fix it”. Resolve them in Working Copy, then Continue.');
  assert.equal(sequenceDetail({ ...s, editing: true }, 0), 'Stopped to edit “Fix it”. Change files or amend the commit, then Continue.');
  assert.equal(sequenceDetail({ kind: 'rebase', step: { action: 'exec', subject: 'make test' } }, 0), 'The command “make test” failed. Fix the problem, then Continue.');
  assert.equal(sequenceDetail({ kind: 'am', step: { subject: 'Cover' }, empty: true }, 0), '“Cover” has no changes (a cover letter?). Skip it.');
  assert.equal(sequenceDetail({ kind: 'merge' }, 0), 'Conflicts resolved. Continue to finish, or Abort to go back to where you started.');
});

test('the second line counts the conflicts with no markers left, as the app’s bar does', () => {
  const s = { kind: 'rebase', step: { action: 'pick', subject: 'Fix it' } };
  assert.equal(sequenceDetail(s, 3, 2), '3 files have conflicts at “Fix it” (2 without markers). Resolve them in Working Copy, then Continue.');
  assert.equal(sequenceDetail({ kind: 'merge' }, 1, 1), '1 file has conflicts (1 without markers). Resolve them in Working Copy, then Continue.');
  assert.equal(sequenceDetail({ kind: 'merge' }, 0, 0), 'Conflicts resolved. Continue to finish, or Abort to go back to where you started.');
});

test('the sidebar row puts the count without markers first, where the width cuts nothing', () => {
  assert.equal(withoutMarkersLead(2), '2 without markers · ');
  assert.equal(withoutMarkersLead(0), '');
});
