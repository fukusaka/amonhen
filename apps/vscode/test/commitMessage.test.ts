import assert from 'node:assert/strict';
import { test } from 'node:test';
import { lengthLevel, longestLine, messageLength } from '../src/workingCopy/commitMessage.ts';

test('the length counts characters and marks 50 and 72', () => {
  assert.equal(messageLength('日本語😀'), 4);
  assert.equal(lengthLevel(50), 'ok');
  assert.equal(lengthLevel(51), 'long');
  assert.equal(lengthLevel(72), 'long');
  assert.equal(lengthLevel(73), 'over');
  assert.equal(longestLine(''), 0);
  assert.equal(longestLine('ab\nabcd\na'), 4);
});
