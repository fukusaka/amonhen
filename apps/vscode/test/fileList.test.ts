import assert from 'node:assert/strict';
import { test } from 'node:test';
import { filesUnder, filterByPath, makeFileTree } from '../src/workingCopy/fileList.ts';

const items = (...paths: string[]) => paths.map((path) => ({ path }));

test('the filter matches anywhere in the path, ignoring case and outer spaces', () => {
  const list = items('src/Main.ts', 'docs/readme.md', 'main.c');
  assert.deepEqual(
    filterByPath(list, ' MAIN ').map((f) => f.path),
    ['src/Main.ts', 'main.c'],
  );
  assert.deepEqual(
    filterByPath(list, 'src/').map((f) => f.path),
    ['src/Main.ts'],
  );
  assert.equal(filterByPath(list, '  ').length, 3);
  assert.equal(filterByPath(list, 'nothing').length, 0);
});

test('the tree puts files under sorted directories', () => {
  const tree = makeFileTree(items('z.txt', 'b/two.ts', 'a/one.ts', 'a/x/three.ts'));
  assert.deepEqual(
    tree.files.map((f) => f.path),
    ['z.txt'],
  );
  assert.deepEqual(
    tree.dirs.map((d) => [d.name, d.path]),
    [
      ['a', 'a'],
      ['b', 'b'],
    ],
  );
  assert.deepEqual(
    tree.dirs[0]!.dirs.map((d) => d.path),
    ['a/x'],
  );
});

test('a directory holding only one directory joins it, keeping the full path', () => {
  const tree = makeFileTree(items('a/b/c/one.ts', 'a/b/c/two.ts', 'a/b/d/three.ts'));
  assert.equal(tree.dirs.length, 1);
  const ab = tree.dirs[0]!;
  assert.equal(ab.name, 'a/b');
  assert.equal(ab.path, 'a/b');
  assert.deepEqual(
    ab.dirs.map((d) => d.name),
    ['c', 'd'],
  );
  const deep = makeFileTree(items('a/b/c/one.ts')).dirs[0]!;
  assert.equal(deep.name, 'a/b/c');
  assert.equal(deep.path, 'a/b/c');
});

test('filesUnder reaches every file below a directory', () => {
  const tree = makeFileTree(items('a/one.ts', 'a/x/two.ts', 'b/three.ts'));
  assert.deepEqual(
    filesUnder(tree.dirs[0]!).map((f) => f.path),
    ['a/x/two.ts', 'a/one.ts'],
  );
  assert.equal(filesUnder(tree).length, 3);
});
