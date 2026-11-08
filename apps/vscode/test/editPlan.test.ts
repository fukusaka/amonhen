import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  applyPreset,
  canReorder,
  choices,
  combinedMessage,
  type EditRow,
  hasChanges,
  moveRow,
  planSteps,
  summary,
  withAction,
} from '../src/history/editPlan.ts';

/** 新しい順（上が HEAD） */
const rows = (...names: string[]): EditRow[] => names.map((n) => ({ oid: n, message: `${n} subject\n\n${n} body`, author: 'a', action: 'pick' }));

test('the oldest row and the row right above a merge have nothing to combine with', () => {
  const r = rows('c', 'b', 'a');
  assert.ok(choices(r, 0).includes('squash'));
  assert.ok(!choices(r, 2).includes('squash'));
  assert.ok(!choices(r, 2).includes('fixup'));
  const merged = rows('c', 'm', 'a');
  merged[1]!.merge = true;
  assert.deepEqual(choices(merged, 1), ['pick']);
  assert.ok(!choices(merged, 0).includes('squash'));
  assert.ok(!canReorder(merged));
  assert.ok(canReorder(r));
});

test('a squash group is planned oldest first, with the combined message on its last squash', () => {
  let r = rows('d', 'c', 'b', 'a');
  r = withAction(r, 1, 'squash');
  r = withAction(r, 0, 'fixup');
  assert.equal(combinedMessage(r, 1), 'b subject\n\nb body\n\nc subject\n\nc body');
  assert.deepEqual(planSteps(r), [
    { oid: 'a', action: 'pick' },
    { oid: 'b', action: 'pick' },
    { oid: 'c', action: 'squash', message: 'b subject\n\nb body\n\nc subject\n\nc body' },
    { oid: 'd', action: 'fixup' },
  ]);
  r = withAction(r, 0, 'squash');
  const steps = planSteps(r);
  assert.equal(steps[2]!.message, undefined);
  assert.equal(steps[3]!.message, 'b subject\n\nb body\n\nc subject\n\nc body\n\nd subject\n\nd body');
});

test('a reworded row carries its new message, and choosing pick forgets it', () => {
  let r = rows('b', 'a');
  r = withAction(r, 0, 'reword');
  r[0] = { ...r[0]!, newMessage: 'new' };
  assert.deepEqual(planSteps(r)[1], { oid: 'b', action: 'reword', message: 'new' });
  r = withAction(r, 0, 'pick');
  assert.equal(r[0]!.newMessage, undefined);
});

test('moving a combining row to the bottom keeps it instead', () => {
  let r = withAction(rows('c', 'b', 'a'), 0, 'squash');
  r = moveRow(r, 0, 3);
  assert.deepEqual(r.map((x) => x.oid), ['b', 'a', 'c']);
  assert.equal(r[2]!.action, 'pick');
  assert.deepEqual(moveRow(rows('c', 'b', 'a'), 2, 0).map((x) => x.oid), ['a', 'c', 'b']);
});

test('the summary counts the changes', () => {
  const original = ['c', 'b', 'a'];
  let r = rows('c', 'b', 'a');
  assert.equal(summary(r, original), 'No changes yet.');
  assert.ok(!hasChanges(r, original));
  r = moveRow(withAction(withAction(r, 0, 'drop'), 1, 'reword'), 2, 0);
  assert.ok(hasChanges(r, original));
  assert.equal(summary(r, original), 'Changes: reordered, 1 message, 1 dropped. You can undo this.');
});

test('presets start the mode with the shortcut’s change', () => {
  const r = rows('c', 'b', 'a');
  assert.deepEqual(applyPreset(r, { kind: 'reword', oid: 'b' }).editMessageAt, 1);
  assert.equal(applyPreset(r, { kind: 'drop', oid: 'a' }).rows[2]!.action, 'drop');
  assert.deepEqual(
    applyPreset(r, { kind: 'combine', oids: ['c', 'b', 'a'] }).rows.map((x) => x.action),
    ['squash', 'squash', 'pick'],
  );
  assert.deepEqual(applyPreset(r, { kind: 'combine', oids: ['b'] }).rows.map((x) => x.action), ['pick', 'squash', 'pick']);
  assert.deepEqual(applyPreset(r, { kind: 'move', oid: 'c', to: 2 }).rows.map((x) => x.oid), ['b', 'a', 'c']);
  assert.deepEqual(applyPreset(r, { kind: 'move', oid: 'a', to: 0 }).rows.map((x) => x.oid), ['a', 'c', 'b']);
});
