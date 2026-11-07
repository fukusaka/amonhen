import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  addTrailer,
  fixupSubjects,
  lengthLevel,
  longestLine,
  messageLength,
  splitMessage,
  templateMessage,
  withType,
} from '../src/workingCopy/commitMessage.ts';

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

test('a type replaces the one already there', () => {
  assert.equal(withType('add a thing', 'feat'), 'feat: add a thing');
  assert.equal(withType('fix(ui)!: add a thing', 'docs'), 'docs: add a thing');
  assert.equal(withType('', 'ci'), 'ci: ');
  assert.equal(withType('Note: keep this', 'fix'), 'fix: Note: keep this');
});

test('a trailer goes last, once', () => {
  const me = 'Signed-off-by: A <a@example.com>';
  const other = 'Co-authored-by: B <b@example.com>';
  assert.equal(addTrailer('', me), me);
  assert.equal(addTrailer('Body text\n', me), `Body text\n\n${me}`);
  assert.equal(addTrailer(`Body\n\n${me}`, other), `Body\n\n${me}\n${other}`);
  assert.equal(addTrailer(`Body\n\n${me}`, me), `Body\n\n${me}`);
});

test('fixup targets skip fixups, squashes and repeats, and stop at 15', () => {
  assert.deepEqual(fixupSubjects(['Two\n\nbody', 'fixup! One', 'squash! One', 'One', 'Two', '']), ['Two', 'One']);
  const many = Array.from({ length: 20 }, (_, i) => `c${i}`);
  assert.deepEqual(fixupSubjects(many), many.slice(0, 15));
});
