import assert from 'node:assert/strict';
import { test } from 'node:test';
import { toFileDiff, type Row } from '../src/daemon/diff.ts';
import type { DiffGap, DiffLine } from '../src/diff/fileDiff.ts';
import {
  changeBlocks,
  GAP_ALL,
  gapActions,
  openGap,
  pillText,
  placeGap,
  showsWholeFile,
  stageActions,
  toggleWholeFile,
} from '../src/workingCopy/gaps.ts';

const HUNK = 3;
const ctx = (old: number, now: number, text = `line ${now}`): Row => ({ kind: 0, flags: 0, old, new: now, text });
const add = (now: number, text: string, flags = 0): Row => ({ kind: 1, flags, old: 0, new: now, text });
const del = (old: number, text: string, flags = 0): Row => ({ kind: 2, flags, old, new: 0, text });
const gapRow = (hidden: number, shown: number, fn = ''): Row => ({ kind: HUNK, flags: 0, old: hidden, new: shown, text: fn });

test('a folded diff keeps the lines opened above the first gap row and reads the gaps', () => {
  // 頭の隙間（5 行）を上から 2 行開いた: 開いた行は隙間の行の前。間の隙間は 30 行を隠し、末尾の隙間は 4 行
  const rows: Row[] = [
    ctx(1, 1),
    ctx(2, 2),
    gapRow(3, 2, 'fn parse'),
    del(6, 'old'),
    add(6, 'new'),
    gapRow(30, 0, 'fn next'),
    add(38, 'more'),
    gapRow(4, 0),
  ];
  const index = new Map<number, DiffLine>();
  const diff = toFileDiff('a.txt', rows, false, false, index, undefined, true);
  assert.equal(diff.folded, true);
  assert.equal(diff.hunks.length, 4);
  assert.equal(diff.hunks[0]!.gap, undefined, '頭の隙間を開いた行は、隙間のない最初の hunk');
  assert.deepEqual(diff.hunks[0]!.lines.map((l) => l.text), ['line 1', 'line 2']);
  assert.deepEqual(
    diff.hunks.slice(1).map((h) => h.gap),
    [
      { index: 0, hidden: 3, shown: 2, function: 'fn parse', leading: true, trailing: false },
      { index: 1, hidden: 30, shown: 0, function: 'fn next', leading: false, trailing: false },
      { index: 2, hidden: 4, shown: 0, function: '', leading: false, trailing: true },
    ],
  );
  assert.deepEqual(diff.hunks[1]!.lines.map((l) => `${l.kind}${l.text}`), ['-old', '+new']);
  // 構文の色を付ける先: daemon の行の番号 → 行（隙間の行は除く）
  assert.equal(index.get(0)?.text, 'line 1');
  assert.equal(index.get(4)?.text, 'new');
  assert.equal(index.has(2), false);
});

test('the version stays when gaps open or close, and changes with the changed lines', () => {
  const version = (rows: Row[]) => toFileDiff('a.txt', rows, false, false, undefined, undefined, true).version;
  const closed = [gapRow(5, 0), del(6, 'old'), add(6, 'new'), gapRow(4, 0)];
  const opened = [ctx(1, 1), gapRow(4, 1), ctx(5, 5), del(6, 'old'), add(6, 'new'), ctx(7, 7), gapRow(3, 1)];
  assert.equal(version(closed), version(opened), '開いても選んだ行が消えない');
  assert.notEqual(version(closed), version([gapRow(5, 0), del(6, 'old'), add(6, 'newer'), gapRow(4, 0)]));
  // たたまない形（`@@` の見出し）は今までどおり
  const plain = toFileDiff('a.txt', [{ kind: HUNK, flags: 0, old: 0, new: 0, text: '@@ -6 +6 @@' }, del(6, 'old'), add(6, 'new')], false);
  assert.equal(plain.folded, undefined);
  assert.equal(plain.hunks[0]!.header, '@@ -6 +6 @@');
  assert.equal(plain.hunks[0]!.gap, undefined);
});

const gap = (patch: Partial<DiffGap>): DiffGap => ({ index: 1, hidden: 0, shown: 0, function: '', leading: false, trailing: false, ...patch });

test('a gap offers 20 lines down and up only when it hides more, and not toward the file’s ends', () => {
  const labels = (g: DiffGap, selected = false) => gapActions(g, selected).map((a) => a.action);
  assert.deepEqual(labels(gap({ hidden: 56 })), ['down', 'up', 'all']);
  assert.deepEqual(labels(gap({ hidden: 56, leading: true })), ['up', 'all']);
  assert.deepEqual(labels(gap({ hidden: 56, trailing: true })), ['down', 'all']);
  assert.deepEqual(labels(gap({ hidden: 20 })), ['all']);
  assert.deepEqual(labels(gap({ hidden: 36, shown: 20 })), ['down', 'up', 'all', 'collapse']);
  assert.deepEqual(labels(gap({ shown: 56 })), ['collapse']);
  assert.deepEqual(labels(gap({ hidden: 56 }), true), [], '行を選んでいれば出さない');
  assert.equal(gapActions(gap({ hidden: 56 }))[0]!.label, '↓ 20 lines');
});

test('opening a gap 20 lines at a time, all of it, or collapsing it', () => {
  let open = openGap([], 1, 'down');
  assert.deepEqual(open, [{ gap: 1, top: 20, bottom: 0 }]);
  open = openGap(open, 1, 'down');
  open = openGap(open, 1, 'up');
  assert.deepEqual(open, [{ gap: 1, top: 40, bottom: 20 }]);
  open = openGap(open, 0, 'all');
  assert.deepEqual(open, [
    { gap: 0, top: 0, bottom: GAP_ALL },
    { gap: 1, top: 40, bottom: 20 },
  ]);
  assert.deepEqual(openGap(open, 1, 'collapse'), [{ gap: 0, top: 0, bottom: GAP_ALL }]);
  assert.deepEqual(openGap([{ gap: 0, top: GAP_ALL - 5, bottom: 0 }], 0, 'down'), [{ gap: 0, top: GAP_ALL, bottom: 0 }]);
});

test('Show Whole File opens every gap, and closes them all when the whole file shows', () => {
  const hunks = (gaps: Partial<DiffGap>[]) => gaps.map((g, i) => ({ header: '', lines: [], gap: gap({ index: i, ...g }) }));
  const folded = hunks([{ hidden: 3 }, { hidden: 30 }, { hidden: 0, shown: 4 }]);
  assert.equal(showsWholeFile(folded), false);
  assert.deepEqual(toggleWholeFile(folded), [0, 1, 2].map((i) => ({ gap: i, top: 0, bottom: GAP_ALL })));
  const whole = hunks([{ shown: 3 }, { shown: 30 }, { shown: 4 }]);
  assert.equal(showsWholeFile(whole), true);
  assert.deepEqual(toggleWholeFile(whole), []);
  // 頭から変わって隙間が 0 行だけなら、全体を見せているとは言わない
  assert.equal(showsWholeFile(hunks([{}])), false);
});

test('the pill says what is hidden, shown or selected', () => {
  assert.deepEqual(pillText(gap({ hidden: 56 }), 0), { text: '⋯  56 unchanged lines', short: '⋯ 56' });
  assert.deepEqual(pillText(gap({ hidden: 0, shown: 56 }), 0), { text: '56 unchanged lines shown' });
  assert.deepEqual(pillText(gap({ hidden: 56 }), 2), { text: '2 lines selected' });
  assert.deepEqual(pillText(gap({}), 0), { text: '' });
});

test('the pill sits centered, moves left of the hunk actions, shortens, then goes', () => {
  // 広い: 中央に置き、ボタンはすべて札の右に入る
  const wide = placeGap({ left: 100, center: 500, right: 900, pill: 120, shortPill: 40, chips: [60, 60, 30] });
  assert.equal(wide.pillX, 440);
  assert.equal(wide.short, false);
  assert.deepEqual(wide.chips, [
    { index: 0, x: 566 },
    { index: 1, x: 632 },
    { index: 2, x: 698 },
  ]);
  assert.equal(wide.overflows, false);
  // hunk の操作に近づくなら左へずらす（中央のまま重ならないところまで）。入らないボタンがあればメニュー
  const shifted = placeGap({ left: 100, center: 300, right: 400, pill: 200, shortPill: 40, chips: [60, 60] });
  assert.equal(shifted.pillX, 192);
  assert.equal(shifted.overflows, true);
  assert.deepEqual(shifted.chips, []);
  // 入らなければ短い札
  const short = placeGap({ left: 100, center: 160, right: 260, pill: 200, shortPill: 40, chips: [60] });
  assert.equal(short.short, true);
  assert.equal(short.pillX, 140);
  assert.ok(short.pillX! + 40 <= 260 - 8, 'hunk の操作に重ならない');
  // それも入らなければ札を出さない（ボタンがあれば、右クリックで）
  const none = placeGap({ left: 100, center: 120, right: 130, pill: 200, shortPill: 40, chips: [60] });
  assert.equal(none.pillX, undefined);
  assert.equal(none.overflows, true);
  assert.equal(placeGap({ left: 100, center: 120, right: 130, pill: 200, chips: [] }).overflows, false);
});

test('hunk actions are named for the side and what is selected', () => {
  const line = (staged: boolean): DiffLine => ({ kind: '+', new: 1, text: 'x', staged });
  const names = (...a: Parameters<typeof stageActions>) => stageActions(...a).map((x) => x.label);
  assert.deepEqual(names('unstaged', [line(false)], 'Hunk'), ['Stage Hunk', 'Discard Hunk']);
  assert.deepEqual(names('unstaged', [line(false)], 'Lines'), ['Stage Lines', 'Discard Lines']);
  assert.deepEqual(names('staged', [line(false)], 'Hunk'), ['Unstage Hunk']);
  assert.deepEqual(names('all', [line(true), line(false)], 'Hunk'), ['Stage Hunk']);
  assert.deepEqual(names('all', [line(true), line(true)], 'Lines'), ['Unstage Lines']);
  assert.deepEqual(names('unstaged', [], 'Hunk'), []);
});

test('blocks of changes are runs of − and + lines within a hunk', () => {
  const l = (kind: ' ' | '-' | '+', n: number): DiffLine => (kind === '-' ? { kind, old: n, text: '' } : { kind, new: n, old: n, text: '' });
  const blocks = changeBlocks([
    { header: '', lines: [l(' ', 1), l('-', 2), l('+', 2), l(' ', 3), l('+', 4)] },
    { header: '', lines: [l('-', 9)] },
  ]);
  assert.deepEqual(
    blocks.map((b) => b.map((x) => `${x.kind}${x.kind === '-' ? x.old : x.new}`)),
    [['-2', '+2'], ['+4'], ['-9']],
  );
});
