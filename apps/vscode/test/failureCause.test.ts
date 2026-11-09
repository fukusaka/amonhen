import assert from 'node:assert/strict';
import { test } from 'node:test';
import { failureCause, hasMoreThan } from '../src/failure/cause.ts';

test('takes git’s fatal: and error: lines as the cause', () => {
  const message = 'To ../b.git\n ! [rejected]        main -> main (stale info)\nerror: failed to push some refs to \'../b.git\'\n';
  assert.equal(failureCause(message), "error: failed to push some refs to '../b.git'");
  assert.equal(hasMoreThan(failureCause(message), message), true);
});

test('falls back to the first line, and needs no details for a one-line message', () => {
  assert.equal(failureCause('Can’t read the commit'), 'Can’t read the commit');
  assert.equal(hasMoreThan('Can’t read the commit', 'Can’t read the commit\n'), false);
  assert.equal(failureCause('\nfirst\nsecond'), 'first');
});
