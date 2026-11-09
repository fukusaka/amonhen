import assert from 'node:assert/strict';
import { test } from 'node:test';
import { pushWarnings } from '../src/sync/pushPlan.ts';

test('warns about pushing to a default branch, unfinished commits, secrets and binaries', () => {
  assert.deepEqual(pushWarnings('origin/topic', ['Add the parser'], []), []);
  assert.deepEqual(
    pushWarnings(
      'origin/main',
      ['fixup! Add the parser', 'WIP: tests', 'Add docs'],
      [
        { path: 'config/.env.local', binary: false },
        { path: 'keys/server.pem', binary: false },
        { path: 'logo.png', binary: true },
      ],
    ),
    [
      'This pushes straight to origin/main.',
      '2 commits look unfinished (fixup!, squash! or WIP).',
      'Adds files that may hold secrets: config/.env.local, keys/server.pem',
      'Adds 1 binary file.',
    ],
  );
});
