import assert from 'node:assert/strict';
import { test } from 'node:test';
import { remoteKind } from '../src/sync/remoteKind.ts';

test('a remote URL is named by the service, path or SSH host it points to', () => {
  assert.equal(remoteKind('git@github.com:owner/repo.git'), 'GitHub');
  assert.equal(remoteKind('https://gitlab.example.com/a/b.git'), 'GitLab');
  assert.equal(remoteKind('/srv/git/repo.git'), 'A path');
  assert.equal(remoteKind('../other'), 'A path');
  assert.equal(remoteKind('file:///srv/repo'), 'A path');
  assert.equal(remoteKind('C:\\repos\\x'), 'A path');
  assert.equal(remoteKind('ssh://git@example.org:2222/x.git'), 'SSH host example.org');
  assert.equal(remoteKind('git+ssh://example.org/x.git'), 'SSH host example.org');
  assert.equal(remoteKind('me@build:repos/x.git'), 'SSH host build');
  assert.equal(remoteKind('https://git.example.net/x.git'), 'git.example.net');
  assert.equal(remoteKind(''), '');
  assert.equal(remoteKind('nonsense'), 'Other');
});
