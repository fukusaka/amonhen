import assert from 'node:assert/strict';
import { test } from 'node:test';
import { lengthLevel, longestLine, messageLength, splitMessage, templateMessage } from '../src/workingCopy/commitMessage.ts';

test('the length counts characters and marks 50 and 72', () => {
  assert.equal(messageLength('日本語😀'), 4);
  assert.equal(lengthLevel(50), 'ok');
  assert.equal(lengthLevel(51), 'long');
  assert.equal(lengthLevel(72), 'long');
  assert.equal(lengthLevel(73), 'over');
  assert.equal(longestLine(''), 0);
  assert.equal(longestLine('ab\nabcd\na'), 4);
});

test('a message splits into its summary and body', () => {
  assert.deepEqual(splitMessage('one line\n'), { summary: 'one line', body: '' });
  assert.deepEqual(splitMessage('subject\n\nbody\n\nmore\n'), { summary: 'subject', body: 'body\n\nmore' });
  assert.deepEqual(splitMessage('subject\nbody'), { summary: 'subject', body: 'body' });
  assert.deepEqual(splitMessage(''), { summary: '', body: '' });
});

test('a template loses its comment lines', () => {
  assert.deepEqual(templateMessage('# say why\nfeat: \n\n# more\nDetails\r\n'), { summary: 'feat:', body: 'Details' });
  assert.deepEqual(templateMessage('# only comments\n'), { summary: '', body: '' });
});
