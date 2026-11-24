import assert from 'node:assert/strict';
import { test } from 'node:test';
import { highlightTerms, linkPieces, splitTrailers } from '../src/history/messageParts.ts';

test('trailers in the last paragraph are split from the body', () => {
  assert.deepEqual(splitTrailers('Body line.\n\nSigned-off-by: A <a@x>\nCo-Authored-By: B <b@x>\n'), {
    body: 'Body line.',
    trailers: [
      { key: 'Signed-off-by', value: 'A <a@x>' },
      { key: 'Co-Authored-By', value: 'B <b@x>' },
    ],
  });
  assert.deepEqual(splitTrailers('Signed-off-by: A <a@x>'), { body: '', trailers: [{ key: 'Signed-off-by', value: 'A <a@x>' }] });
  // 最後の段落に地の文が混ざれば、トレーラーとみなさない
  assert.deepEqual(splitTrailers('Body.\n\nSee: this\nand more'), { body: 'Body.\n\nSee: this\nand more', trailers: [] });
});

test('search terms to highlight leave out field searches', () => {
  assert.deepEqual(highlightTerms('fix author:me path:src "two words" OR subject:crash'), ['fix', 'two words', 'crash']);
  assert.deepEqual(highlightTerms('foo:bar'), ['foo:bar']);
});

test('URLs and commit IDs become links', () => {
  assert.deepEqual(linkPieces('See https://example.com/a, fixes 1234abc.'), [
    { text: 'See ' },
    { url: 'https://example.com/a' },
    { text: ', fixes ' },
    { commit: '1234abc' },
    { text: '.' },
  ]);
  assert.deepEqual(linkPieces('deadbeefx and /abcdef12'), [{ text: 'deadbeefx and /abcdef12' }]);
});
