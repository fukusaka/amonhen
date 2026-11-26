/** リポジトリの設定の画面のもと */
import assert from 'node:assert/strict';
import { join, resolve } from 'node:path';
import { test } from 'node:test';
import {
  BASIC_KEYS,
  ConfigView,
  configRows,
  groupConfigRows,
  keyParts,
  normalizedBool,
  pullMethod,
  pullWrites,
  remoteKind,
  type ConfigEntry,
} from '../src/settings/model.ts';
import { templateFilePath, templatePreview } from '../src/settings/templateFile.ts';

const entry = (key: string, value: string, scope: string): ConfigEntry => ({ key, value, scope, origin: `file:${scope}` });

test('ConfigView picks the value in effect, the override here and the one above', () => {
  const c = new ConfigView([entry('fetch.prune', 'false', 'global'), entry('Fetch.Prune', 'true', 'local')]);
  assert.equal(c.effective('fetch.prune')?.value, 'true');
  assert.equal(c.local('fetch.prune'), 'true');
  assert.equal(c.inherited('fetch.prune')?.value, 'false');
  assert.deepEqual(c.origin('fetch.prune'), { label: 'this repository', file: 'local' });
  assert.deepEqual(c.origin('rerere.enabled'), { label: 'git’s default' });
});

test('ConfigView gives the value at each level, the last one written there', () => {
  const c = new ConfigView([
    entry('pull.rebase', 'false', 'system'),
    entry('pull.rebase', 'true', 'global'),
    entry('Pull.Rebase', 'merges', 'global'),
    entry('pull.rebase', 'false', 'local'),
  ]);
  assert.equal(c.at('pull.rebase', 'system')?.value, 'false');
  assert.equal(c.at('pull.rebase', 'global')?.value, 'merges');
  assert.equal(c.at('pull.rebase', 'local')?.value, 'false');
  assert.equal(c.at('pull.rebase', 'worktree'), undefined);
});

test('normalizedBool accepts git’s spellings', () => {
  assert.equal(normalizedBool('Yes'), 'true');
  assert.equal(normalizedBool('off'), 'false');
  assert.equal(normalizedBool('maybe'), undefined);
});

test('pullMethod and pullWrites follow pull.ff=only first, then pull.rebase', () => {
  assert.equal(pullMethod(new ConfigView([])), 'merge');
  assert.equal(pullMethod(new ConfigView([entry('pull.rebase', 'merges', 'global')])), 'rebase');
  const ff = new ConfigView([entry('pull.rebase', 'true', 'global'), entry('pull.ff', 'only', 'local')]);
  assert.equal(pullMethod(ff), 'ffOnly');
  assert.deepEqual(pullWrites(ff, 'rebase'), [
    ['pull.ff', null],
    ['pull.rebase', 'true'],
  ]);
  assert.deepEqual(pullWrites(new ConfigView([]), 'ffOnly'), [
    ['pull.rebase', 'false'],
    ['pull.ff', 'only'],
  ]);
});

test('remoteKind says where a remote points', () => {
  assert.equal(remoteKind('git@github.com:owner/repo.git'), 'GitHub');
  assert.equal(remoteKind('/srv/git/repo.git'), 'A path');
  assert.equal(remoteKind('C:\\work\\repo'), 'A path');
  assert.equal(remoteKind('ssh://me@build.example.com/repo.git'), 'SSH host build.example.com');
  assert.equal(remoteKind('me@build.example.com:repo.git'), 'SSH host build.example.com');
  assert.equal(remoteKind('https://git.example.org/repo.git'), 'git.example.org');
  assert.equal(remoteKind(''), '');
});

test('configRows keeps every value a level has for a key, one per line', () => {
  const rows = configRows(
    [
      entry('remote.origin.fetch', '+refs/heads/*:refs/remotes/origin/*', 'local'),
      entry('core.filemode', 'false', 'global'),
      entry('remote.origin.fetch', '+refs/tags/*:refs/tags/*', 'local'),
      entry('core.filemode', 'true', 'local'),
    ],
    ['core.fileMode'],
  );
  assert.deepEqual(
    rows.map((r) => [r.key, r.effective, r.effectiveScope]),
    [
      ['remote.origin.fetch', '+refs/heads/*:refs/remotes/origin/*\n+refs/tags/*:refs/tags/*', 'local'],
      ['core.fileMode', 'true', 'local'],
    ],
  );
  assert.deepEqual(rows[1]!.values, { global: 'false', local: 'true' });
  assert.equal(rows[1]!.files['global'], 'global');
});

test('the Basic keys are the ones the app lists', () => {
  assert.ok(BASIC_KEYS.includes('user.signingkey'));
  assert.ok(BASIC_KEYS.includes('commit.template'));
  assert.ok(!(BASIC_KEYS as readonly string[]).includes('log.mailmap'));
});

test('keyParts splits at the first and last dot', () => {
  assert.deepEqual(keyParts('core.editor'), { section: 'core', name: 'editor' });
  assert.deepEqual(keyParts('url.https://example.com/.insteadOf'), { section: 'url', subsection: 'https://example.com/', name: 'insteadOf' });
});

test('groupConfigRows puts keys under sections and subsections, in the sort order', () => {
  const rows = configRows(
    [
      entry('remote.origin.url', 'b', 'local'),
      entry('core.bare', 'false', 'local'),
      entry('remote.origin.fetch', 'a', 'local'),
      entry('core.autocrlf', 'input', 'global'),
      entry('remote.upstream.url', 'c', 'local'),
    ],
    [],
  );
  const shape = (groups: ReturnType<typeof groupConfigRows>) =>
    groups.map((g) => [g.key, g.rows.map((r) => r.key), g.groups.map((s) => [s.key, s.label, s.rows.map((r) => r.key)])]);
  assert.deepEqual(shape(groupConfigRows(rows, { column: 'key', reverse: false })), [
    ['core', ['core.autocrlf', 'core.bare'], []],
    ['remote', [], [['remote.origin', 'origin', ['remote.origin.fetch', 'remote.origin.url']], ['remote.upstream', 'upstream', ['remote.upstream.url']]]],
  ]);
  assert.deepEqual(shape(groupConfigRows(rows, { column: 'key', reverse: true })), [
    ['remote', [], [['remote.upstream', 'upstream', ['remote.upstream.url']], ['remote.origin', 'origin', ['remote.origin.url', 'remote.origin.fetch']]]],
    ['core', ['core.bare', 'core.autocrlf'], []],
  ]);
  // 値で並べても、組はキーの順
  assert.deepEqual(shape(groupConfigRows(rows, { column: 'global', reverse: true }))[0]![1], ['core.autocrlf', 'core.bare']);
});

test('a template file is found as git finds it, and previewed up to 12 lines', () => {
  assert.equal(templateFilePath('~/t.txt', '/repo', '/home/me'), join('/home/me', 't.txt'));
  assert.equal(templateFilePath('t.txt', '/repo', '/home/me'), resolve('/repo', 't.txt'));
  assert.equal(templatePreview('\nsubject\n\nbody\n\n'), 'subject\n\nbody');
  const long = Array.from({ length: 14 }, (_, i) => `line ${i + 1}`).join('\n');
  assert.equal(templatePreview(long), `${long.split('\n').slice(0, 12).join('\n')}…`);
});
