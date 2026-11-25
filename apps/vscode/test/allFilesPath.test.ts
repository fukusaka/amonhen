import assert from 'node:assert/strict';
import { test } from 'node:test';
import { resolvePath } from '../src/history/repoPath.ts';

test('a Markdown link is resolved from the file it is in', () => {
  assert.equal(resolvePath('docs/usage.md', 'design.md'), 'docs/design.md');
  assert.equal(resolvePath('docs/ja/usage.md', '../design.md'), 'docs/design.md');
  assert.equal(resolvePath('docs/usage.md', '/README.md'), 'README.md');
  assert.equal(resolvePath('docs/usage.md', './img/a%20b.png'), 'docs/img/a b.png');
  assert.equal(resolvePath('README.md', '../x.md'), undefined);
});
