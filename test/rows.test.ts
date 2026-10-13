import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { Commit } from '../src/git/log.ts';
import type { Ref } from '../src/git/refs.ts';
import { editableChain, refLabels } from '../src/history/rows.ts';

const ref = (name: string, oid: string, extra: Partial<Ref> = {}): Ref => ({
  name,
  oid,
  isHead: false,
  date: 0,
  subject: '',
  ...extra,
});

test('refLabels puts the current branch first and skips symbolic remote refs', () => {
  const labels = refLabels(
    { branch: 'refs/heads/main', oid: 'a' },
    [ref('refs/heads/feature', 'a'), ref('refs/heads/main', 'a')],
    [ref('refs/remotes/origin/main', 'a'), ref('refs/remotes/origin/HEAD', 'a', { symref: 'refs/remotes/origin/main' })],
    [ref('refs/tags/v1', 't', { peeled: 'a' })],
  );
  assert.deepEqual(labels.get('a'), [
    { name: 'main', kind: 'branch', current: true },
    { name: 'feature', kind: 'branch' },
    { name: 'origin/main', kind: 'remote' },
    { name: 'v1', kind: 'tag' },
  ]);
});

test('refLabels shows HEAD when detached', () => {
  assert.deepEqual(refLabels({ oid: 'x' }, [], [], []).get('x'), [{ name: 'HEAD', kind: 'head', current: true }]);
});

test('editableChain stops at the first merge', () => {
  const c = (oid: string, parents: string[]): Commit => ({ oid, parents, author: '', email: '', date: 0, subject: '' });
  assert.deepEqual([...editableChain([c('d', ['c']), c('c', ['b', 'x']), c('b', ['a'])])], [['d', 0]]);
  assert.deepEqual([...editableChain([c('b', ['a']), c('a', [])])], [
    ['b', 0],
    ['a', 1],
  ]);
});
