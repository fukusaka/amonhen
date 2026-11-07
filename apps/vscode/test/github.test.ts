import assert from 'node:assert/strict';
import { test } from 'node:test';
import { gitHubSlug, repositorySlug } from '../src/git/github.ts';

test('gitHubSlug reads HTTPS, scp-like and ssh URLs', () => {
  assert.equal(gitHubSlug('https://github.com/o/r.git'), 'o/r');
  assert.equal(gitHubSlug('git@github.com:o/r.git'), 'o/r');
  assert.equal(gitHubSlug('ssh://git@github.com/o/r'), 'o/r');
  assert.equal(gitHubSlug('https://user@github.com:443/o/r'), 'o/r');
  assert.equal(gitHubSlug('https://github.example.com/team/o/r.git'), 'github.example.com/o/r');
});

test('gitHubSlug leaves other hosts and paths alone', () => {
  assert.equal(gitHubSlug('https://gitlab.com/o/r.git'), undefined);
  assert.equal(gitHubSlug('/srv/repos/r.git'), undefined);
  assert.equal(gitHubSlug('https://github.com/o'), undefined);
});

test('repositorySlug prefers origin', () => {
  assert.equal(
    repositorySlug([
      { name: 'fork', fetchUrl: 'https://github.com/me/r.git' },
      { name: 'origin', fetchUrl: 'git@github.com:team/r.git' },
    ]),
    'team/r',
  );
  assert.equal(repositorySlug([{ name: 'local', fetchUrl: '/tmp/r' }]), undefined);
  assert.equal(repositorySlug([]), undefined);
});
