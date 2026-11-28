/**
 * Working Copy の差分で見る側（Unstaged・Staged・All）と、一覧の行（Mixed の 1 つの一覧・Staged と Unstaged の節）、
 * 読み直した後の選び直し。vscode と DOM に依存しない（アプリの WorkingCopyReselect と同じ規則）
 */
import type { Change, ChangeGroup } from '../state/changes.ts';

/** 差分で見る側（git の index → 作業ツリー・HEAD → index・HEAD → 作業ツリー）。Working Copy で 1 つ */
export type DiffSide = 'unstaged' | 'staged' | 'all';

/** 差分の帯の切り替え（アプリと同じ文言） */
export const SIDES: readonly { side: DiffSide; label: string; title: string }[] = [
  { side: 'unstaged', label: 'Unstaged', title: 'Changes in the working tree that are not staged yet' },
  { side: 'staged', label: 'Staged', title: 'Staged changes (included in the next commit)' },
  {
    side: 'all',
    label: 'All',
    title: 'Every change from HEAD to the working tree (the check on each line shows whether it’s staged)',
  },
];

/** daemon の `workingDiff` の mode */
export function modeOf(side: DiffSide): 'unstaged' | 'staged' | 'combined' {
  return side === 'all' ? 'combined' : side;
}

/** 選んだ側の差分を出さず、わけを言うとき */
export type SideNote = 'noUnstaged' | 'noStaged' | 'allWhitespace' | 'allUnavailable';

export function sideNoteText(note: SideNote): string {
  switch (note) {
    case 'noUnstaged':
      return 'No unstaged changes in this file';
    case 'noStaged':
      return 'No staged changes in this file';
    case 'allWhitespace':
      return 'All can’t be shown while whitespace is ignored. See Unstaged and Staged.';
    case 'allUnavailable':
      return 'This file’s staged and unstaged changes can’t be shown in one diff. See Unstaged and Staged.';
  }
}

/**
 * 読まずに分かるわけ: その側に変更がない、空白を無視していて All を出せない。読んでみないと分からない
 * allUnavailable（コアが断る）は返さない
 */
export function sideNoteFor(
  side: DiffSide,
  has: { staged: boolean; unstaged: boolean },
  whitespaceIgnored: boolean,
): SideNote | undefined {
  if (side === 'unstaged') return has.unstaged ? undefined : 'noUnstaged';
  if (side === 'staged') return has.staged ? undefined : 'noStaged';
  return whitespaceIgnored ? 'allWhitespace' : undefined;
}

/** 次のコミットに入れるか（☑ すべて・▣ 一部・☐ 入れない） */
export type Check = 'on' | 'mixed' | 'off';

/** 一覧の 1 行 */
export interface ListRow {
  path: string;
  /** 節分けの節（staged・unstaged）、コンフリクトは conflicts、Mixed の行は null */
  section: ChangeGroup | null;
  /** 行に出す状態の印 */
  letter: string;
  /** 元のパス（名前を変えたとき） */
  origPath?: string;
  /** index の側の変更（HEAD → index）。あれば Unstage できる */
  staged?: Change;
  /** 作業ツリーの側の変更（index → 作業ツリー）。あれば Stage・Discard できる */
  unstaged?: Change;
  /** コンフリクト */
  conflict?: Change;
}

/** 行の代わりのもの（印・マーカー・rerere・untracked などを見る） */
export function changeOf(row: ListRow): Change {
  return (row.conflict ?? row.unstaged ?? row.staged)!;
}

/** stage・unstage を頼むときのグループ（Mixed の行は、作業ツリーの側があればそちら） */
export function groupOf(row: ListRow): ChangeGroup {
  return row.section ?? (row.unstaged ? 'unstaged' : 'staged');
}

/** 行のチェック。コンフリクトは undefined（チェックの代わりに ⚠） */
export function rowCheck(row: ListRow): Check | undefined {
  switch (row.section) {
    case 'conflicts':
      return undefined;
    case 'staged':
      return 'on';
    case 'unstaged':
      return 'off';
    default:
      if (!row.staged) return 'off';
      return row.unstaged ? 'mixed' : 'on';
  }
}

/** いくつかの行をまとめたチェック（見出し・フォルダ）。コンフリクトだけなら undefined */
export function groupCheck(rows: readonly ListRow[]): Check | undefined {
  const states = new Set(rows.map(rowCheck).filter((c) => c !== undefined));
  if (states.size === 0) return undefined;
  if (states.size === 1) return [...states][0];
  return 'mixed';
}

/**
 * チェックを押したときの頼みごと: すべて入っていれば外す（unstage）、でなければ作業ツリーの側をすべて入れる（stage）。
 * 押す相手のないものは除く
 */
export function toggleRequest(rows: readonly ListRow[]): { op: 'stage' | 'unstage'; files: Change[] } | undefined {
  const targets = rows.filter((r) => rowCheck(r) !== undefined);
  if (targets.length === 0) return undefined;
  if (targets.every((r) => rowCheck(r) === 'on')) {
    return { op: 'unstage', files: dedupe(targets.flatMap((r) => r.staged ?? [])) };
  }
  return { op: 'stage', files: dedupe(targets.flatMap((r) => r.unstaged ?? [])) };
}

function dedupe(changes: Change[]): Change[] {
  const seen = new Set<string>();
  return changes.filter((c) => {
    const k = `${c.group}\0${c.path}`;
    if (seen.has(k)) return false;
    seen.add(k);
    return true;
  });
}

/** 節分けの 1 つの節の行 */
export function sectionRows(files: readonly Change[], group: ChangeGroup): ListRow[] {
  return files
    .filter((f) => f.group === group)
    .map((f): ListRow => {
      const row: ListRow = { path: f.path, section: group, letter: f.letter, origPath: f.origPath };
      if (group === 'conflicts') row.conflict = f;
      else row[group] = f;
      return row;
    });
}

/**
 * Mixed の 1 つの一覧: コンフリクトが先、あとはパスの順に 1 ファイル 1 行（一部だけ stage したファイルも 1 行）。
 * 印は index の側が追加・削除・名前の変更ならそれ、でなければ作業ツリーの側（アプリと同じく、ファイルとしての変化）
 */
export function mixedRows(files: readonly Change[]): ListRow[] {
  const conflicts = sectionRows(files, 'conflicts');
  const byPath = new Map<string, ListRow>();
  for (const f of files) {
    if (f.group === 'conflicts') continue;
    let row = byPath.get(f.path);
    if (!row) {
      row = { path: f.path, section: null, letter: f.letter };
      byPath.set(f.path, row);
    }
    if (f.group === 'staged') {
      row.staged = f;
      row.origPath = f.origPath;
    } else {
      row.unstaged = f;
    }
  }
  for (const row of byPath.values()) {
    const s = row.staged?.letter;
    row.letter = s && s !== 'M' && s !== 'T' ? s : (row.unstaged ?? row.staged)!.letter;
  }
  const rest = [...byPath.values()].sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
  return [...conflicts, ...rest];
}

/** 一覧の行を見分けるもの: パスと節（Mixed は null） */
export interface RowKey {
  path: string;
  section: ChangeGroup | null;
}

export function keyOf(row: RowKey): RowKey {
  return { path: row.path, section: row.section };
}

export function keyText(row: RowKey): string {
  return `${row.section ?? ''}\0${row.path}`;
}

export function parseKey(text: string): RowKey {
  const at = text.indexOf('\0');
  const section = text.slice(0, at);
  return { path: text.slice(at + 1), section: section ? (section as ChangeGroup) : null };
}

const sameKey = (a: RowKey, b: RowKey) => a.path === b.path && a.section === b.section;

/** 見る側 side に合う行か（Mixed とコンフリクトはいつも合う） */
function fitsSide(row: RowKey, side: DiffSide): boolean {
  return row.section === null || row.section === 'conflicts' || row.section === side;
}

/**
 * 作り直した一覧 rows で、前に選んでいた行 kept に当たる行の添字。前の行（同じパス・同じ節）があればそれ、
 * なければ（すべて stage した・外したので別の節へ移った）同じパスの別の節の行。一部だけ stage して両方の節にある
 * ファイルは、選んでいた節の行だけ
 */
export function reselectRows(kept: readonly RowKey[], rows: readonly RowKey[]): number[] {
  const present = (k: RowKey) => rows.some((r) => sameKey(r, k));
  const moved = new Set(kept.filter((k) => !present(k)).map((k) => k.path));
  return rows.flatMap((r, i) => (kept.some((k) => sameKey(k, r)) || moved.has(r.path) ? [i] : []));
}

/** 何も選んでいないときに最初に選ぶ行: 今の見る側の節の行を先に（その前のコンフリクトは先のまま）、なければ最初の行 */
export function firstRow(rows: readonly RowKey[], side: DiffSide): number | undefined {
  const i = rows.findIndex((r) => fitsSide(r, side));
  return i >= 0 ? i : rows.length > 0 ? 0 : undefined;
}

/**
 * 読み直した後に差分を出す行。差分に出していた行 shown がそのまま残っていればそれ、なくなったが同じパスの行があれば
 * （見る側の節を先に）それ、それもなければ選び直したほかの行（見る側の節を先に）、それもなければ最初の行
 */
export function reselectTarget(
  shown: RowKey | undefined,
  rows: readonly RowKey[],
  kept: readonly number[],
  side: DiffSide,
): number | undefined {
  if (shown) {
    const same = rows.findIndex((r) => sameKey(r, shown));
    if (same >= 0) return same;
    const fit = rows.findIndex((r) => r.path === shown.path && fitsSide(r, side));
    if (fit >= 0) return fit;
    const other = rows.findIndex((r) => r.path === shown.path);
    if (other >= 0) return other;
  }
  const keptFit = kept.find((i) => fitsSide(rows[i]!, side));
  if (keptFit !== undefined) return keptFit;
  if (kept.length > 0) return kept[0];
  return firstRow(rows, side);
}

/** 節分けの行を選んだら、見る側をその節にそろえる（アプリの adoptSection）。Mixed・コンフリクトは替えない */
export function adoptSide(row: RowKey, side: DiffSide): DiffSide {
  return row.section === 'staged' || row.section === 'unstaged' ? row.section : side;
}

/**
 * いくつか選んだファイルのうち、並べて差分を出すもの（アプリの showMulti と同じ）: 見る側 side に変更のあるもの
 * （All はすべて）を選んだ順に、上から limit 個まで。total はその側に変更のある数
 */
export function stackedPaths(
  paths: readonly string[],
  has: (path: string, group: 'staged' | 'unstaged') => boolean,
  side: DiffSide,
  limit: number,
): { shown: string[]; total: number } {
  const candidates = paths.filter((p) => side === 'all' || has(p, side));
  return { shown: candidates.slice(0, limit), total: candidates.length };
}

/** 並べた差分の帯の題（アプリと同じ文言）: 選んだ数と、その側に変更のある数が多くて削ったときは出した数 */
export function stackTitle(chosen: number, total: number, shown: number): string {
  const title = `${chosen} files`;
  return total > shown ? `${title}  (showing the first ${shown})` : title;
}

/** 並べた差分に何も出さないときのわけ（アプリと同じ文言） */
export function stackEmptyText(side: DiffSide): string {
  return side === 'staged' ? 'No staged changes in these files' : 'No unstaged changes in these files';
}

/** 並べる相手になる、選んだ行のパス（一覧の順、重ねない。コンフリクトは並べない） */
export function stackCandidates(rows: readonly ListRow[]): string[] {
  return [...new Set(rows.filter((r) => r.section !== 'conflicts').map((r) => r.path))];
}
