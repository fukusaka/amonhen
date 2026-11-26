/** 差分の中の、変わった単語の強調（コアの words.rs と同じ考え） */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { changedWords, wordHighlighter, wordPairs } from '../src/diff/words.ts';

const pick = (text: string, ranges: readonly (readonly [number, number])[] | undefined) => ranges?.map(([s, e]) => text.slice(s, e));

test('finds the changed words, several in one line', () => {
  const [removed, added] = changedWords('let x = foo(1);', 'let x = bar(1);')!;
  assert.deepEqual(removed, [[8, 11]]);
  assert.deepEqual(added, [[8, 11]]);
  const old = 'call(alpha, beta, gamma) + offset;';
  const neu = 'call(alpha, delta, gamma) + shift;';
  const [r, a] = changedWords(old, neu)!;
  assert.deepEqual(pick(old, r), ['beta', 'offset']);
  assert.deepEqual(pick(neu, a), ['delta', 'shift']);
});

test('an insertion only marks the added side', () => {
  const [r, a] = changedWords('f(a, b)', 'f(a, x, b)')!;
  assert.deepEqual(r, []);
  assert.deepEqual(pick('f(a, x, b)', a), ['x, ']);
});

test('lines that changed almost entirely or are too long are not marked', () => {
  assert.equal(changedWords('completely different', 'nothing alike here'), undefined);
  assert.equal(changedWords('a'.repeat(1001), 'b'), undefined);
  // バイトで数える（3 バイトの文字 400 個は 1000 バイトを超える）
  assert.equal(changedWords('あ'.repeat(400), 'あ'.repeat(399) + 'い'), undefined);
});

test('non-ASCII text is one word and ranges stay on character boundaries', () => {
  const old = 'name = 太郎 ok';
  const neu = 'name = 花子 ok';
  const [r, a] = changedWords(old, neu)!;
  assert.deepEqual(pick(old, r), ['太郎']);
  assert.deepEqual(pick(neu, a), ['花子']);
  // サロゲートの組の間では切らない
  const [, b] = changedWords('icon 😀 here', 'icon 😃 here')!;
  assert.deepEqual(pick('icon 😃 here', b), ['😃']);
});

test('removed and added lines pair up even when the counts differ', () => {
  assert.deepEqual([...wordPairs([' ', '-', '-', '-', '+', '+', ' ', '-', '+'])].sort((x, y) => x[0] - y[0]), [
    [1, 4],
    [2, 5],
    [4, 1],
    [5, 2],
    [7, 8],
    [8, 7],
  ]);
  const lines = [
    { kind: '-', text: 'let a = 1;' },
    { kind: '-', text: 'let b = 2;' },
    { kind: '-', text: 'let c = 3;' },
    { kind: '+', text: 'let a = 10;' },
    { kind: '+', text: 'let b = 20;' },
  ];
  const words = wordHighlighter(lines);
  assert.deepEqual(pick(lines[0]!.text, words(0)), ['1']);
  assert.deepEqual(pick(lines[3]!.text, words(3)), ['10']);
  assert.deepEqual(pick(lines[4]!.text, words(4)), ['20']);
  // 組にならない削除行は行全体の色だけ
  assert.equal(words(2), undefined);
});
