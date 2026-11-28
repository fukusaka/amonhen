import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { FileDiff } from '../src/diff/fileDiff.ts';
import { firstOpen, isGenerated, lineCounts, openTogether, renamedPath } from '../src/history/stackedFiles.ts';

test('lockfiles and minified files are generated, by their name only', () => {
  assert.ok(isGenerated('Cargo.lock'));
  assert.ok(isGenerated('web/package-lock.json'));
  assert.ok(isGenerated('dist/app.min.js'));
  assert.ok(!isGenerated('src/Cargo.lock.rs'));
  assert.ok(!isGenerated('lock/main.rs'));
});

test('line counts add up the added and removed lines of every hunk', () => {
  const line = (kind: '+' | '-' | ' ') => ({ kind, text: '', old: 1, new: 1 });
  const diff = {
    path: 'a',
    version: '',
    hunks: [
      { header: '', lines: [line('+'), line('-'), line(' ')] },
      { header: '', lines: [line('+'), line('+')] },
    ],
  } as unknown as FileDiff;
  assert.deepEqual(lineCounts(diff), { added: 3, removed: 1 });
});

test('renamed paths factor out what stays the same, as git diff --stat does', () => {
  assert.equal(renamedPath('src/old.rs', 'src/new.rs'), 'src/{old.rs → new.rs}');
  assert.equal(renamedPath('a/x/f.txt', 'a/y/f.txt'), 'a/{x → y}/f.txt');
  assert.equal(renamedPath('old.txt', 'new.txt'), '{old.txt → new.txt}');
  assert.equal(renamedPath('lib/f.rs', 'src/lib/f.rs'), '{lib → src/lib}/f.rs');
});

test('a commit opens its first files up to Files to open together, leaving the heavy ones folded', () => {
  const files = [false, true, false, false, false].map((heavy) => ({ heavy }));
  assert.deepEqual(firstOpen(files, 3), [0, 2]);
  assert.deepEqual(firstOpen(files, 20), [0, 2, 3, 4]);
  assert.deepEqual(firstOpen([], 5), []);
});

test('Files to open together takes the app’s choices, otherwise 20', () => {
  assert.equal(openTogether(5), 5);
  assert.equal(openTogether(100), 100);
  assert.equal(openTogether(7), 20);
  assert.equal(openTogether(undefined), 20);
  assert.equal(openTogether('10'), 20);
});
