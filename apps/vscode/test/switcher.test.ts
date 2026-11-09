import assert from 'node:assert/strict';
import { test } from 'node:test';
import { creatableName, rememberRecent, switchCandidates } from '../src/branch/switcher.ts';

test('puts recent branches first, then the rest by name, without the current one', () => {
  const choices = switchCandidates(['main', 'b', 'a', 'c'], [], 'main', ['c', 'gone', 'a']);
  assert.deepEqual(
    choices.map((c) => c.name),
    ['c', 'a', 'b'],
  );
});

test('lists remote branches without a local branch of the same name, and not HEAD', () => {
  const choices = switchCandidates(['main'], ['origin/HEAD', 'origin/main', 'origin/topic', 'fork/feature/x'], 'main', []);
  assert.deepEqual(choices, [
    { kind: 'remote', name: 'origin/topic' },
    { kind: 'remote', name: 'fork/feature/x' },
  ]);
});

test('offers to create a branch only with a new, usable name', () => {
  assert.equal(creatableName(' topic ', ['main']), 'topic');
  assert.equal(creatableName('main', ['main']), undefined);
  assert.equal(creatableName('two words', []), undefined);
  assert.equal(creatableName('-x', []), undefined);
  assert.equal(creatableName('', []), undefined);
});

test('keeps the ten most recent branches, newest first', () => {
  assert.deepEqual(rememberRecent(['a', 'b'], 'b'), ['b', 'a']);
  assert.deepEqual(rememberRecent(['a'], 'a'), ['a']);
  const many = Array.from({ length: 10 }, (_, i) => `b${i}`);
  assert.deepEqual(rememberRecent(many, 'new'), ['new', ...many.slice(0, 9)]);
});
