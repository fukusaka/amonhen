import assert from 'node:assert/strict';
import { test } from 'node:test';
import { refsScope, scopeRefs, scopeTitle } from '../src/history/scope.ts';

test('one ref stays a ref scope, several keep every name', () => {
  assert.equal(refsScope(['refs/heads/a']), 'ref:refs/heads/a');
  const two = refsScope(['refs/heads/a', 'refs/remotes/origin/b']);
  assert.deepEqual(scopeRefs(two), ['refs/heads/a', 'refs/remotes/origin/b']);
  assert.equal(scopeTitle(two), 'a, origin/b');
});

test('the current branch and all branches have no refs', () => {
  assert.equal(scopeRefs('head'), undefined);
  assert.equal(scopeRefs('all'), undefined);
  assert.equal(scopeTitle('head'), '');
});
