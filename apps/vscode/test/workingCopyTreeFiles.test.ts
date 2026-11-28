import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { TreeEntry } from '../src/history/protocol.ts';
import type { Change } from '../src/state/changes.ts';
import { flatOrder, folderChildren, statusOverlay, type TreeNode } from '../src/workingCopy/treeFiles.ts';

const staged = (path: string, letter = 'M', origPath?: string): Change => ({ group: 'staged', path, letter, ...(origPath ? { origPath } : {}) });
const unstaged = (path: string, letter = 'M'): Change => ({ group: 'unstaged', path, letter });
const conflict = (path: string): Change => ({ group: 'conflicts', path, letter: '!', conflict: 'UU' });
const file = (name: string): TreeEntry => ({ name, kind: 'file' });
const folder = (name: string): TreeEntry => ({ name, kind: 'folder' });

/** 木の行を [パス、フォルダか、印、消したか、変更を含むか] に */
const shape = (nodes: TreeNode[]) => nodes.map((n) => [n.path, n.folder, n.mark?.letter ?? '', n.mark?.deleted ?? false, n.changes]);

test('changed files get the list’s marks and folders with changes are marked', () => {
  const overlay = statusOverlay([unstaged('src/a.ts'), staged('src/b.ts', 'A'), unstaged('README.md')]);
  assert.deepEqual([...overlay.folders].sort(), ['', 'src']);
  const top = folderChildren('', [file('README.md'), folder('src'), folder('docs'), file('LICENSE')], overlay);
  // フォルダが先、名前の順（大文字と小文字は区別しない）
  assert.deepEqual(shape(top), [
    ['docs', true, '', false, false],
    ['src', true, '', false, true],
    ['LICENSE', false, '', false, false],
    ['README.md', false, 'M', false, true],
  ]);
  const src = folderChildren('src', [file('a.ts'), file('c.ts')], overlay);
  // stage して足したファイルは HEAD の木にないので足す
  assert.deepEqual(shape(src), [
    ['src/a.ts', false, 'M', false, true],
    ['src/b.ts', false, 'A', false, true],
    ['src/c.ts', false, '', false, false],
  ]);
  assert.equal(src.find((n) => n.path === 'src/b.ts')!.inHead, false);
});

test('untracked files and folders that aren’t in HEAD are added', () => {
  const overlay = statusOverlay([unstaged('new/deep/x.txt', 'U'), unstaged('top.txt', 'U')]);
  assert.deepEqual([...overlay.folders].sort(), ['', 'new', 'new/deep']);
  const top = folderChildren('', [file('a.txt')], overlay);
  assert.deepEqual(shape(top), [
    ['new', true, '', false, true],
    ['a.txt', false, '', false, false],
    ['top.txt', false, 'U', false, true],
  ]);
  // HEAD にないフォルダは HEAD から読まない
  assert.equal(top[0]!.inHead, false);
  assert.deepEqual(shape(folderChildren('new', [], overlay)), [['new/deep', true, '', false, true]]);
  assert.deepEqual(shape(folderChildren('new/deep', [], overlay)), [['new/deep/x.txt', false, 'U', false, true]]);
});

test('deleted files stay struck through, staged or not, unless the name is back', () => {
  const overlay = statusOverlay([unstaged('gone.txt', 'D'), staged('removed.txt', 'D'), staged('back.txt', 'D'), unstaged('back.txt', 'U')]);
  const top = folderChildren('', [file('gone.txt'), file('back.txt')], overlay);
  assert.deepEqual(shape(top), [
    ['back.txt', false, 'D', false, true],
    ['gone.txt', false, 'D', true, true],
    ['removed.txt', false, 'D', true, true],
  ]);
});

test('a rename shows the new name with R and the old one struck through', () => {
  const overlay = statusOverlay([staged('lib/new.ts', 'R', 'old/old.ts')]);
  assert.deepEqual([...overlay.folders].sort(), ['', 'lib', 'old']);
  assert.deepEqual(shape(folderChildren('old', [file('old.ts')], overlay)), [['old/old.ts', false, 'D', true, true]]);
  assert.deepEqual(shape(folderChildren('lib', [], overlay)), [['lib/new.ts', false, 'R', false, true]]);
  // 前の名前に別の変更（同じ名前で足し直した）があれば、そちらの印
  const again = statusOverlay([staged('b.ts', 'R', 'a.ts'), unstaged('a.ts', 'U')]);
  assert.deepEqual(again.marks.get('a.ts'), { letter: 'U', conflict: false, deleted: false });
});

test('a conflicted file gets ⚠ instead of a letter', () => {
  const overlay = statusOverlay([conflict('src/merge.ts'), staged('src/merge.ts')]);
  assert.deepEqual(overlay.marks.get('src/merge.ts'), { letter: '⚠', conflict: true, deleted: false });
  assert.ok(overlay.folders.has('src'));
});

test('a partly staged file is one node with the file’s mark', () => {
  const overlay = statusOverlay([staged('a.ts', 'A'), unstaged('a.ts', 'M')]);
  assert.deepEqual(shape(folderChildren('', [], overlay)), [['a.ts', false, 'A', false, true]]);
});

test('submodules are marked and the list and file name orders follow the app', () => {
  const overlay = statusOverlay([]);
  const nodes = folderChildren('', [{ name: 'vendor', kind: 'submodule' }], overlay);
  assert.equal(nodes[0]!.submodule, true);
  assert.equal(nodes[0]!.folder, false);
  const files = ['b/readme.md', 'a/Zeta.ts', 'a/readme.md'].map((path): TreeNode => ({
    name: path.slice(path.lastIndexOf('/') + 1),
    path,
    folder: false,
    submodule: false,
    inHead: true,
    changes: false,
  }));
  assert.deepEqual(flatOrder(files, 'list').map((n) => n.path), ['a/readme.md', 'a/Zeta.ts', 'b/readme.md']);
  assert.deepEqual(flatOrder(files, 'flat').map((n) => n.path), ['a/readme.md', 'b/readme.md', 'a/Zeta.ts']);
});
