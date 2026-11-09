import assert from 'node:assert/strict';
import { test } from 'node:test';
import { pruneKey, pruneLine } from '../src/sync/fetchPlan.ts';

test('prunes as git does: the remote’s own setting first, then fetch.prune', () => {
  const config = new Map([
    ['fetch.prune', 'true'],
    ['remote.fork.prune', 'false'],
  ]);
  assert.equal(pruneKey('origin', config), 'fetch.prune');
  assert.equal(pruneKey('fork', config), undefined);
  assert.equal(pruneKey('origin', new Map()), undefined);
  assert.equal(pruneKey('origin', new Map([['remote.origin.prune', 'yes']])), 'remote.origin.prune');
});

test('says which remotes prune', () => {
  const config = new Map([['remote.origin.prune', 'true']]);
  assert.equal(pruneLine(['origin'], config), 'Remote branches deleted there are removed here too (remote.origin.prune).');
  assert.equal(
    pruneLine(['origin', 'fork'], config),
    'Remote branches deleted on origin are removed here too (remote.origin.prune).',
  );
  assert.equal(pruneLine(['fork'], config), '');
});
