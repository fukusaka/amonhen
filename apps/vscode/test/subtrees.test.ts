/** subtree の行に出すこと */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { defaultSquash, subtreeTooltip, type Subtree } from '../src/git/subtrees.ts';

const base: Subtree = { dir: 'vendor/lib', squashed: false, url: null, ref: null, pushRef: null, squash: null };

test('subtreeTooltip has the repository and branch used last and how the last import was made', () => {
  assert.equal(subtreeTooltip(base), 'vendor/lib\nNot pulled from here yet\nThe last import kept the history');
  assert.equal(
    subtreeTooltip({ ...base, url: 'https://example.com/lib.git', ref: 'main', pushRef: 'from-app', squashed: true }),
    'vendor/lib\nFrom https://example.com/lib.git (main)\nPushes to from-app\nThe last import was squashed',
  );
  assert.equal(subtreeTooltip({ ...base, url: 'u', ref: 'main', pushRef: 'main' }), 'vendor/lib\nFrom u (main)\nThe last import kept the history');
});

test('defaultSquash is the choice made last, or off as in git', () => {
  assert.equal(defaultSquash(undefined), false);
  assert.equal(defaultSquash(base), false);
  assert.equal(defaultSquash({ squash: true }), true);
});
