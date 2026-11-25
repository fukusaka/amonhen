/** タグのフォルダ（アプリのサイドバーと同じく、名前の順でフォルダは最初の名前の位置） */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { refTree, type RefTree } from '../src/git/refTree.ts';

const shape = (nodes: RefTree<string>[]): unknown[] =>
  nodes.map((n) => (n.kind === 'folder' ? { [`${n.label}/`]: shape(n.children) } : n.value));

test('refTree sorts by name and puts each folder where its first name sorts', () => {
  const names = ['v2.0', 'release/1.1', 'a', 'release/1.0', 'release/old/0.9', 'v1.10', 'v1.2'];
  const tree = refTree(names.map((name) => ({ name, value: name })));
  assert.deepEqual(shape(tree), [
    'a',
    { 'release/': ['release/1.0', 'release/1.1', { 'old/': ['release/old/0.9'] }] },
    // 名前の順（数としてではない）
    'v1.10',
    'v1.2',
    'v2.0',
  ]);
  const release = tree[1] as Extract<RefTree<string>, { kind: 'folder' }>;
  assert.equal(release.path, 'release');
  assert.equal((release.children[2] as Extract<RefTree<string>, { kind: 'folder' }>).path, 'release/old');
  assert.equal(release.children[0]!.label, '1.0');
});
