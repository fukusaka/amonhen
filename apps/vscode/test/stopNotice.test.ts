import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { RepositoryBackend } from '../src/git/backend.ts';
import { isStopped, withStopNotice } from '../src/state/stopNotice.ts';

test('a conflict, a stop, or a one-by-one merge that stopped counts as stopped', () => {
  assert.equal(isStopped('conflict'), true);
  assert.equal(isStopped('stopped'), true);
  assert.equal(isStopped({ result: 'conflict', remaining: ['b'] }), true);
  assert.equal(isStopped('done'), false);
  assert.equal(isStopped({ result: 'done', remaining: [] }), false);
  assert.equal(isStopped(undefined), false);
});

test('the backend tells when an operation stops, whoever called it', async () => {
  const inner = {
    root: '/repo',
    merge: async () => 'conflict',
    rebase: async () => 'done',
    continueSequence: async () => 'stopped',
    // 止まりうる書き込みでなければ、conflict を返しても知らせない
    predictMerge: async () => 'conflict',
  } as unknown as RepositoryBackend;
  let stops = 0;
  const backend = withStopNotice(inner, () => stops++);
  assert.equal(backend.root, '/repo');
  assert.equal(await backend.merge('topic'), 'conflict');
  assert.equal(stops, 1);
  assert.equal(await backend.rebase('main'), 'done');
  assert.equal(stops, 1);
  assert.equal(await backend.continueSequence('rebase'), 'stopped');
  assert.equal(stops, 2);
  await backend.predictMerge(['topic']);
  assert.equal(stops, 2);
});

test('the backend tells when an operation is aborted, and not when the abort fails', async () => {
  let fail = false;
  const inner = {
    abort: async () => {
      if (fail) throw new Error('no');
    },
  } as unknown as RepositoryBackend;
  let aborts = 0;
  const backend = withStopNotice(inner, () => {}, () => aborts++);
  await backend.abort('merge');
  assert.equal(aborts, 1);
  fail = true;
  await assert.rejects(backend.abort('merge'));
  assert.equal(aborts, 1);
});
