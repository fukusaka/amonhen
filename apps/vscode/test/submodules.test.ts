/** submodule の行に出すこと */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { folderFromUrl, submoduleMarks, submoduleTooltip, type SubmoduleFact } from '../src/git/submodules.ts';

const base: SubmoduleFact = {
  path: 'lib',
  name: 'lib',
  url: 'https://example.com/lib.git',
  branch: null,
  initialized: true,
  moved: false,
  changed: false,
  conflict: false,
};

test('submoduleMarks shows what differs, as the app does', () => {
  assert.deepEqual(submoduleMarks(base), []);
  assert.deepEqual(submoduleMarks({ ...base, initialized: false }), ['not checked out']);
  assert.deepEqual(submoduleMarks({ ...base, moved: true, changed: true }), ['another commit', 'changes inside']);
  assert.deepEqual(submoduleMarks({ ...base, moved: true, conflict: true }), ['in conflict']);
});

test('submoduleTooltip has the URL and the branch it follows', () => {
  assert.equal(submoduleTooltip(base), 'lib\nURL: https://example.com/lib.git\nFollows the recorded commit');
  assert.equal(submoduleTooltip({ ...base, url: null, branch: 'main' }), 'lib\nFollows main');
});

test('folderFromUrl takes the last part without .git, as git submodule add does', () => {
  assert.equal(folderFromUrl('https://example.com/team/library.git'), 'library');
  assert.equal(folderFromUrl('git@example.com:team/library.git/'), 'library');
  assert.equal(folderFromUrl('../shared'), 'shared');
  assert.equal(folderFromUrl('C:\\repos\\tool'), 'tool');
});
