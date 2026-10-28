import assert from 'node:assert/strict';
import { test } from 'node:test';
import { parseLog, parseNameStatus } from '../src/git/log.ts';

test('parseLog', () => {
  const rec = (...f: string[]) => `\x1e${f.join('\0')}\n`;
  const out =
    rec('c2', 'p1 p2', 'Ann', 'a@x', 'Ann', 'a@x', '200', 'merge: x y') +
    rec('p1', '', 'Bob', 'b@x', 'Robert', 'robert@x', '100', '');
  assert.deepEqual(parseLog(out), [
    { oid: 'c2', parents: ['p1', 'p2'], author: 'Ann', email: 'a@x', date: 200, subject: 'merge: x y' },
    { oid: 'p1', parents: [], author: 'Robert', email: 'robert@x', date: 100, subject: '' },
  ]);
  // log.mailmap が false なら記録のまま
  assert.equal(parseLog(out, false)[1]!.author, 'Bob');
});

test('parseNameStatus with renames', () => {
  const out = ['M', 'a b.txt', 'R087', 'old.txt', 'new.txt', 'A', 'n', 'D', 'd', ''].join('\0');
  assert.deepEqual(parseNameStatus(out), [
    { status: 'M', path: 'a b.txt' },
    { status: 'R', origPath: 'old.txt', path: 'new.txt' },
    { status: 'A', path: 'n' },
    { status: 'D', path: 'd' },
  ]);
});
