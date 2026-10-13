import assert from 'node:assert/strict';
import { test } from 'node:test';
import { parseCommitObject } from '../src/git/commitObject.ts';

test('parseCommitObject with a signature header and a body', () => {
  const text = [
    'tree t1',
    'parent p1',
    'parent p2',
    'author Ann Lee <ann@example.com> 1700000000 +0900',
    'committer Bob <bob@example.com> 1700000100 -0500',
    'gpgsig -----BEGIN PGP SIGNATURE-----',
    ' ',
    ' abc',
    ' -----END PGP SIGNATURE-----',
    '',
    'Subject line',
    '',
    'Body text.',
    '',
  ].join('\n');
  assert.deepEqual(parseCommitObject(text), {
    tree: 't1',
    parents: ['p1', 'p2'],
    author: { name: 'Ann Lee', email: 'ann@example.com', date: 1700000000 },
    committer: { name: 'Bob', email: 'bob@example.com', date: 1700000100 },
    message: 'Subject line\n\nBody text.\n',
  });
});
