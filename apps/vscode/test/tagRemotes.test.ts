/** タグのリモートでの様子から決まるメニューと印（アプリのサイドバーと同じ決まり） */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { tagMark, tagMarkLabel, tagStateText, tagTargets, type TagRemotes } from '../src/git/tagRemotes.ts';

const info: TagRemotes = {
  remotes: [
    { name: 'origin', checked: 1_700_000_000 },
    { name: 'fork', checked: 1_700_000_000 },
    { name: 'new', checked: null },
  ],
  tags: {
    'v1.0': { origin: 'same', fork: 'same' },
    'v1.1': { origin: 'same', fork: 'missing' },
    'v2.0': { origin: 'different', fork: 'missing' },
    local: { origin: 'missing', fork: 'missing' },
  },
};

test('tagTargets offers Push where it is missing, Replace where it differs, and Delete where it is', () => {
  // まだ読んでいないリモート（new）には、Push と削除の両方を出す
  assert.deepEqual(tagTargets(info, 'v1.0'), { push: ['new'], replace: [], onRemote: ['origin', 'fork', 'new'] });
  assert.deepEqual(tagTargets(info, 'v1.1'), { push: ['fork', 'new'], replace: [], onRemote: ['origin', 'new'] });
  assert.deepEqual(tagTargets(info, 'v2.0'), { push: ['fork', 'new'], replace: ['origin'], onRemote: ['origin', 'new'] });
  // 読んだ後にできたタグ（様子がない）は、どこにもないのと同じ
  assert.deepEqual(tagTargets(info, 'later'), { push: ['origin', 'fork', 'new'], replace: [], onRemote: ['new'] });
});

test('tagMark marks a different tag first, then a tag on no remote that was read', () => {
  assert.equal(tagMark(info, 'v1.0'), undefined);
  assert.equal(tagMark(info, 'v1.1'), undefined);
  const different = tagMark(info, 'v2.0')!;
  assert.deepEqual(different, { kind: 'different', remotes: ['origin'] });
  assert.equal(tagMarkLabel(different), 'different on origin');
  assert.deepEqual(tagMark(info, 'local'), { kind: 'missing' });
  assert.equal(tagMarkLabel({ kind: 'missing' }), 'not on any remote');
  // どのリモートもまだ読んでいなければ、印は出さない
  assert.equal(tagMark({ remotes: [{ name: 'origin', checked: null }], tags: {} }, 'local'), undefined);
});

test('tagStateText says what each remote has', () => {
  assert.equal(tagStateText(info, 'v1.0', 'origin'), 'origin: has it');
  assert.equal(tagStateText(info, 'v2.0', 'origin'), 'origin: has a different tag with this name');
  assert.equal(tagStateText(info, 'v2.0', 'fork'), 'fork: doesn’t have it');
  assert.equal(tagStateText(info, 'v2.0', 'new'), 'new: not checked yet (Fetch reads it)');
});
