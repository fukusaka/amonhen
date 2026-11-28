import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { Change } from '../src/state/changes.ts';
import {
  adoptSide,
  firstRow,
  groupCheck,
  keyText,
  mixedRows,
  modeOf,
  parseKey,
  reselectRows,
  reselectTarget,
  rowCheck,
  sectionRows,
  sideNoteFor,
  sideNoteText,
  toggleRequest,
  type RowKey,
} from '../src/workingCopy/sides.ts';

const staged = (path: string, letter = 'M', origPath?: string): Change => ({ group: 'staged', path, letter, ...(origPath ? { origPath } : {}) });
const unstaged = (path: string, letter = 'M'): Change => ({ group: 'unstaged', path, letter });
const conflict = (path: string): Change => ({ group: 'conflicts', path, letter: '!', conflict: 'UU' });

test('All reads the combined diff, the other sides their own', () => {
  assert.equal(modeOf('all'), 'combined');
  assert.equal(modeOf('staged'), 'staged');
  assert.equal(modeOf('unstaged'), 'unstaged');
});

test('a side with nothing in the file, and All while whitespace is ignored, say why instead of a diff', () => {
  const both = { staged: true, unstaged: true };
  assert.equal(sideNoteFor('unstaged', { staged: true, unstaged: false }, false), 'noUnstaged');
  assert.equal(sideNoteFor('staged', { staged: false, unstaged: true }, false), 'noStaged');
  assert.equal(sideNoteFor('staged', both, true), undefined, 'Staged ignores whitespace');
  assert.equal(sideNoteFor('all', both, true), 'allWhitespace');
  assert.equal(sideNoteFor('all', { staged: false, unstaged: true }, false), undefined);
  assert.equal(sideNoteText('noStaged'), 'No staged changes in this file');
  assert.match(sideNoteText('allUnavailable'), /See Unstaged and Staged\.$/);
  assert.match(sideNoteText('allWhitespace'), /whitespace is ignored/);
});

test('Mixed has one row per file, conflicts first, with the check from the index', () => {
  const rows = mixedRows([
    conflict('c.txt'),
    staged('b.txt', 'A'),
    staged('a.txt'),
    unstaged('a.txt'),
    unstaged('z.txt', 'U'),
    staged('n.txt', 'R', 'o.txt'),
  ]);
  assert.deepEqual(
    rows.map((r) => [r.path, r.section, r.letter, rowCheck(r)]),
    [
      ['c.txt', 'conflicts', '!', undefined],
      ['a.txt', null, 'M', 'mixed'],
      ['b.txt', null, 'A', 'on'],
      ['n.txt', null, 'R', 'on'],
      ['z.txt', null, 'U', 'off'],
    ],
  );
  assert.equal(rows[3]!.origPath, 'o.txt');
  // 追加して作業ツリーでも変えたファイルは、追加の印
  assert.equal(mixedRows([staged('x', 'A'), unstaged('x')])[0]!.letter, 'A');
});

test('the sections keep a partly staged file in both, each checked by its side', () => {
  const files = [staged('a.txt'), unstaged('a.txt'), unstaged('b.txt')];
  assert.deepEqual(
    sectionRows(files, 'staged').map((r) => [r.path, rowCheck(r), r.staged?.group, r.unstaged]),
    [['a.txt', 'on', 'staged', undefined]],
  );
  assert.deepEqual(
    sectionRows(files, 'unstaged').map((r) => [r.path, rowCheck(r)]),
    [
      ['a.txt', 'off'],
      ['b.txt', 'off'],
    ],
  );
});

test('a check over several rows is mixed unless they agree, and toggling includes all unless all are in', () => {
  const rows = mixedRows([staged('a'), unstaged('b'), staged('c'), unstaged('c'), conflict('d')]);
  assert.equal(groupCheck(rows), 'mixed');
  assert.equal(groupCheck(rows.filter((r) => r.path === 'a')), 'on');
  assert.equal(groupCheck(rows.filter((r) => r.path === 'd')), undefined);
  const include = toggleRequest(rows)!;
  assert.equal(include.op, 'stage');
  assert.deepEqual(
    include.files.map((f) => f.path),
    ['b', 'c'],
  );
  const exclude = toggleRequest(rows.filter((r) => r.path === 'a'))!;
  assert.equal(exclude.op, 'unstage');
  assert.deepEqual(
    exclude.files.map((f) => [f.group, f.path]),
    [['staged', 'a']],
  );
  assert.equal(toggleRequest(rows.filter((r) => r.path === 'd')), undefined, 'conflicts have no check');
});

test('row keys keep the path and the section', () => {
  for (const key of [
    { path: 'a/b.txt', section: null },
    { path: 'x', section: 'staged' },
  ] as RowKey[]) {
    assert.deepEqual(parseKey(keyText(key)), key);
  }
});

const k = (path: string, section: RowKey['section'] = null): RowKey => ({ path, section });

test('reselecting keeps the same rows, and follows a file that moved to the other section', () => {
  const rows = [k('a', 'staged'), k('b', 'staged'), k('a', 'unstaged'), k('c', 'unstaged')];
  // a の Unstaged の行はある（選んだ節のまま）、b は Unstaged から Staged へ移った
  assert.deepEqual(reselectRows([k('a', 'unstaged'), k('b', 'unstaged')], rows), [1, 2]);
  // 消えたファイルは外す
  assert.deepEqual(reselectRows([k('gone', 'unstaged')], rows), []);
  // Mixed の行から節へ切り替えると、どちらの節の行も
  assert.deepEqual(reselectRows([k('a')], rows), [0, 2]);
});

test('the row to show is the one shown before, then the same file on the side shown, then the rest', () => {
  const rows = [k('c', 'conflicts'), k('a', 'staged'), k('a', 'unstaged'), k('b', 'unstaged')];
  assert.equal(reselectTarget(k('a', 'unstaged'), rows, [2], 'unstaged'), 2);
  // Mixed から節へ: 見る側の節の行
  assert.equal(reselectTarget(k('a'), rows, [1, 2], 'staged'), 1);
  // 消えた: 選んでいたほかの行、なければ見る側の最初の行（コンフリクトは先のまま）
  assert.equal(reselectTarget(k('x', 'unstaged'), rows, [3], 'unstaged'), 3);
  assert.equal(reselectTarget(undefined, rows, [], 'staged'), 0);
  assert.equal(firstRow([k('a', 'staged'), k('b', 'unstaged')], 'unstaged'), 1);
  assert.equal(firstRow([k('a', 'staged')], 'unstaged'), 0);
  assert.equal(firstRow([], 'all'), undefined);
});

test('choosing a section row adopts its side; Mixed and conflict rows keep the side', () => {
  assert.equal(adoptSide(k('a', 'staged'), 'unstaged'), 'staged');
  assert.equal(adoptSide(k('a', 'unstaged'), 'all'), 'unstaged');
  assert.equal(adoptSide(k('a'), 'all'), 'all');
  assert.equal(adoptSide(k('a', 'conflicts'), 'staged'), 'staged');
});
