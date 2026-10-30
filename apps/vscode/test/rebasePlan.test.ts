import assert from 'node:assert/strict';
import { test } from 'node:test';
import { shellQuote } from '../src/git/exec.ts';
import type { Commit } from '../src/git/log.ts';
import { planRebase } from '../src/history/rebasePlan.ts';

const c = (oid: string, parent?: string): Commit => ({
  oid,
  parents: parent ? [parent] : [],
  author: '',
  email: '',
  date: 0,
  subject: oid,
});
// 新しい順: d → c → b → a（a は root）
const chain = [c('d', 'c'), c('c', 'b'), c('b', 'a'), c('a')];

test('reword starts at the commit', () => {
  const plan = planRebase(chain, { kind: 'reword', oid: 'c', message: 'new' });
  assert.equal(plan.base, 'b');
  assert.equal(plan.todo, 'reword c\npick d\n');
  assert.equal(plan.message, 'new');
});

test('squash and fixup start at the parent', () => {
  const fixup = planRebase(chain, { kind: 'fixup', oid: 'c' });
  assert.equal(fixup.todo, 'pick b\nfixup c\npick d\n');
  assert.equal(fixup.message, null);
  const squash = planRebase(chain, { kind: 'squash', oid: 'c', message: 'b+c' });
  assert.equal(squash.base, 'a');
  assert.equal(squash.todo, 'pick b\nsquash c\npick d\n');
  assert.equal(squash.message, 'b+c');
});

test('drop, edit and split', () => {
  assert.equal(planRebase(chain, { kind: 'drop', oid: 'd' }).todo, 'drop d\n');
  assert.equal(planRebase(chain, { kind: 'edit', oid: 'c' }).todo, 'edit c\npick d\n');
  const split = planRebase(chain, { kind: 'split', oid: 'c' });
  assert.equal(split.todo, 'edit c\npick d\n');
  assert.equal(split.resetOnStop, true);
});

test('editing the root commit uses --root', () => {
  assert.equal(planRebase(chain, { kind: 'reword', oid: 'a', message: 'x' }).base, null);
  assert.throws(() => planRebase(chain, { kind: 'split', oid: 'a' }));
  assert.throws(() => planRebase(chain, { kind: 'fixup', oid: 'a' }));
});

test('the chain may end at a merge, whose parent becomes the base', () => {
  const partial = [c('d', 'c'), c('c', 'm')];
  assert.equal(planRebase(partial, { kind: 'drop', oid: 'c' }).base, 'm');
});

test('move reorders from the earlier of the two positions', () => {
  // d を、新しい順で 2 番目（b の位置）へ
  const plan = planRebase(chain, { kind: 'move', oid: 'd', to: 2 });
  assert.equal(plan.base, 'a');
  assert.equal(plan.todo, 'pick d\npick b\npick c\n');
  // b を一番上へ
  assert.equal(planRebase(chain, { kind: 'move', oid: 'b', to: 0 }).todo, 'pick c\npick d\npick b\n');
});

test('commits outside the chain are rejected', () => {
  assert.throws(() => planRebase(chain, { kind: 'drop', oid: 'zzz' }));
});

test('shellQuote', () => {
  assert.equal(shellQuote("it's"), `'it'\\''s'`);
});
