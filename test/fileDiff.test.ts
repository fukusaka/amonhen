import assert from 'node:assert/strict';
import { test } from 'node:test';
import { applyDiffSelection, computeDiff } from '../src/workingCopy/fileDiff.ts';

const B = (s: string) => Buffer.from(s, 'utf8');

test('computeDiff numbers lines from 1 and decodes UTF-8 for display', () => {
  const { diff } = computeDiff('unstaged', 'a.txt', B('一\n二\n三\n'), B('一\nに\n三\n'));
  assert.equal(diff.hunks.length, 1);
  assert.equal(diff.hunks[0]!.header, '@@ -1,3 +1,3 @@');
  assert.deepEqual(diff.hunks[0]!.lines, [
    { kind: ' ', old: 1, new: 1, text: '一' },
    { kind: '-', old: 2, text: '二' },
    { kind: '+', new: 2, text: 'に' },
    { kind: ' ', old: 3, new: 3, text: '三' },
  ]);
});

test('binary and new files', () => {
  assert.equal(computeDiff('unstaged', 'b', undefined, Buffer.from([0, 1, 2])).diff.binary, true);
  const added = computeDiff('unstaged', 'n', undefined, B('x\ny\n'));
  assert.deepEqual(
    added.diff.hunks[0]!.lines.map((l) => l.kind),
    ['+', '+'],
  );
});

test('the version changes with either side', () => {
  const a = computeDiff('unstaged', 'a', B('1\n'), B('2\n')).diff.version;
  assert.equal(computeDiff('unstaged', 'a', B('1\n'), B('2\n')).diff.version, a);
  assert.notEqual(computeDiff('unstaged', 'a', B('1\n'), B('3\n')).diff.version, a);
});

test('stage selected lines into the index', () => {
  // index（old）→ 作業中（new）
  const c = computeDiff('unstaged', 'a', B('keep\nold1\nold2\nend\n'), B('keep\nnew1\nnew2\nend\n'));
  assert.equal(
    Buffer.from(applyDiffSelection(c, 'stage', [{ kind: '-', old: 2 }, { kind: '+', new: 2 }]), 'latin1').toString(),
    'keep\nold2\nnew1\nend\n',
  );
});

test('unstage selected lines back towards HEAD', () => {
  // HEAD（old）→ index（new）
  const c = computeDiff('staged', 'a', B('keep\nold1\nend\n'), B('keep\nnew1\nnew2\nend\n'));
  // new2 の追加だけを取り消す
  assert.equal(applyDiffSelection(c, 'unstage', [{ kind: '+', new: 3 }]), 'keep\nnew1\nend\n');
  // old1 の削除だけを取り消す（old1 が戻る）
  assert.equal(applyDiffSelection(c, 'unstage', [{ kind: '-', old: 2 }]), 'keep\nold1\nnew1\nnew2\nend\n');
});
