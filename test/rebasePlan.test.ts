import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { Commit } from '../src/git/log.ts';
import { planRebase, shellQuote } from '../src/history/rebasePlan.ts';

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
const path = (name: string) => `/g/${name}`;

test('reword starts at the commit and amends right after it', () => {
  const plan = planRebase(chain, { kind: 'reword', oid: 'c', message: 'new' }, path);
  assert.equal(plan.base, 'b');
  assert.equal(
    plan.todo,
    "pick c\nexec git commit --amend --allow-empty --no-verify --file='/g/message-c.txt'\npick d\n",
  );
  assert.deepEqual([...plan.messages], [['message-c.txt', 'new']]);
});

test('squash and fixup start at the parent', () => {
  assert.equal(planRebase(chain, { kind: 'fixup', oid: 'c' }, path).todo, 'pick b\nfixup c\npick d\n');
  const squash = planRebase(chain, { kind: 'squash', oid: 'c', message: 'b+c' }, path);
  assert.equal(squash.base, 'a');
  assert.match(squash.todo, /^pick b\nfixup c\nexec git commit --amend .*message-c\.txt'\npick d\n$/);
});

test('drop, edit and split', () => {
  assert.equal(planRebase(chain, { kind: 'drop', oid: 'd' }, path).todo, 'drop d\n');
  assert.equal(planRebase(chain, { kind: 'edit', oid: 'c' }, path).todo, 'pick c\nbreak\npick d\n');
  assert.equal(
    planRebase(chain, { kind: 'split', oid: 'c' }, path).todo,
    'pick c\nexec git reset --quiet HEAD^\nbreak\npick d\n',
  );
});

test('editing the root commit uses --root', () => {
  assert.equal(planRebase(chain, { kind: 'reword', oid: 'a', message: 'x' }, path).base, null);
  assert.throws(() => planRebase(chain, { kind: 'split', oid: 'a' }, path));
  assert.throws(() => planRebase(chain, { kind: 'fixup', oid: 'a' }, path));
});

test('the chain may end at a merge, whose parent becomes the base', () => {
  const partial = [c('d', 'c'), c('c', 'm')];
  assert.equal(planRebase(partial, { kind: 'drop', oid: 'c' }, path).base, 'm');
});

test('move reorders from the earlier of the two positions', () => {
  // d を、新しい順で 2 番目（b の位置）へ
  const plan = planRebase(chain, { kind: 'move', oid: 'd', to: 2 }, path);
  assert.equal(plan.base, 'a');
  assert.equal(plan.todo, 'pick d\npick b\npick c\n');
  // b を一番上へ
  assert.equal(planRebase(chain, { kind: 'move', oid: 'b', to: 0 }, path).todo, 'pick c\npick d\npick b\n');
});

test('commits outside the chain are rejected', () => {
  assert.throws(() => planRebase(chain, { kind: 'drop', oid: 'zzz' }, path));
});

test('shellQuote', () => {
  assert.equal(shellQuote("it's"), `'it'\\''s'`);
});
