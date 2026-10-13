import assert from 'node:assert/strict';
import { test } from 'node:test';
import { parseRefs, parseRemotes, parseStashes, parseSubmoduleConfig, splitRemoteRef } from '../src/git/refs.ts';

test('parseRefs: tracking info and annotated tags', () => {
  const rec = (...f: string[]) => `\x1e${f.join('\0')}\n`;
  const out =
    rec('refs/heads/main', 'a1', '', 'refs/remotes/origin/main', 'ahead 1, behind 2', '*', '', '100', 'subject one') +
    rec('refs/heads/old', 'a2', '', 'refs/remotes/origin/old', 'gone', ' ', '', '200', '') +
    rec('refs/tags/v1', 't1', 'c1', '', '', ' ', '', '300', 'release') +
    rec('refs/remotes/origin/HEAD', 'a1', '', '', '', ' ', 'refs/remotes/origin/main', '100', 'x');
  const [main, old, tag, head] = parseRefs(out);
  assert.deepEqual(main, {
    name: 'refs/heads/main',
    oid: 'a1',
    upstream: 'refs/remotes/origin/main',
    ahead: 1,
    behind: 2,
    isHead: true,
    date: 100,
    subject: 'subject one',
  });
  assert.equal(old?.gone, true);
  assert.equal(tag?.peeled, 'c1');
  assert.equal(head?.symref, 'refs/remotes/origin/main');
});

test('parseRemotes', () => {
  const out = 'origin\tgit@example.com:a/b.git (fetch)\norigin\tgit@example.com:a/b.git (push)\nup\thttps://x (fetch)\n';
  assert.deepEqual(parseRemotes(out), [
    { name: 'origin', fetchUrl: 'git@example.com:a/b.git', pushUrl: 'git@example.com:a/b.git' },
    { name: 'up', fetchUrl: 'https://x' },
  ]);
});

test('splitRemoteRef: remote names containing a slash', () => {
  assert.deepEqual(splitRemoteRef('refs/remotes/team/a/feature/x', ['team', 'team/a']), {
    remote: 'team/a',
    branch: 'feature/x',
  });
  assert.deepEqual(splitRemoteRef('refs/remotes/gone/main', ['origin']), { remote: 'gone', branch: 'main' });
});

test('parseStashes', () => {
  const rec = (...f: string[]) => `\x1e${f.join('\0')}\n`;
  const out = rec('stash@{0}', 'abc', '100', 'WIP on main: x') + rec('stash@{1}', 'def', '050', 'On dev: y');
  assert.deepEqual(parseStashes(out), [
    { name: 'stash@{0}', oid: 'abc', date: 100, subject: 'WIP on main: x' },
    { name: 'stash@{1}', oid: 'def', date: 50, subject: 'On dev: y' },
  ]);
});

test('parseSubmoduleConfig', () => {
  const out = 'submodule.lib/a.path\nvendor/a\0submodule.b.path\nb\0';
  assert.deepEqual(parseSubmoduleConfig(out), [
    { name: 'lib/a', path: 'vendor/a' },
    { name: 'b', path: 'b' },
  ]);
});
