import assert from 'node:assert/strict';
import { test } from 'node:test';
import { ignorePatterns, withIgnorePattern } from '../src/workingCopy/ignore.ts';

test('a file offers its path, and its extension when it has one', () => {
  assert.deepEqual(ignorePatterns('src/a.log'), ['src/a.log', '*.log']);
  assert.deepEqual(ignorePatterns('Makefile'), ['Makefile']);
  assert.deepEqual(ignorePatterns('dir.d/.env'), ['dir.d/.env']);
  assert.deepEqual(ignorePatterns('trailing.'), ['trailing.']);
});

test('a pattern goes last, once', () => {
  assert.equal(withIgnorePattern('', '*.log'), '*.log\n');
  assert.equal(withIgnorePattern('a', '*.log'), 'a\n*.log\n');
  assert.equal(withIgnorePattern('a\n', '*.log'), 'a\n*.log\n');
  assert.equal(withIgnorePattern('a\r\n *.log \r\n', '*.log'), undefined);
});
