/**
 * Working Copy の、変更のない所（隙間）をたたんだ差分の決まり（vscode にも DOM にも依存しない）: 隙間の開き方、
 * 隙間の行の札とボタンの置き方、hunk の操作の名前、変更のかたまり。アプリ（macOS の DiffViewController の
 * gapLayout・GapFit・stageActions）と同じ
 */
import type { DiffGap, DiffHunk, DiffLine } from '../diff/fileDiff.ts';

/** 隙間の開き方（`workingDiff` の open の 1 つ）。上（前の変更の直後）と下（次の変更の直前）から見せる行数 */
export interface GapOpen {
  gap: number;
  top: number;
  bottom: number;
}

/** 1 度に開く行数 */
export const GAP_STEP = 20;
/** すべて開くときの行数（コアは u32 に収める） */
export const GAP_ALL = 0xffffffff;

/** 隙間の行の開く・畳む */
export type GapAction = 'down' | 'up' | 'all' | 'collapse';

/** 隙間で出せる開く・畳む（行を選んでいれば出さない）。先頭の隙間に ↓、末尾の隙間に ↑ はない */
export function gapActions(gap: DiffGap, selected = false): { action: GapAction; label: string }[] {
  if (selected) return [];
  const out: { action: GapAction; label: string }[] = [];
  if (gap.hidden > GAP_STEP) {
    if (!gap.leading) out.push({ action: 'down', label: `↓ ${GAP_STEP} lines` });
    if (!gap.trailing) out.push({ action: 'up', label: `↑ ${GAP_STEP} lines` });
  }
  if (gap.hidden > 0) out.push({ action: 'all', label: 'All' });
  if (gap.shown > 0) out.push({ action: 'collapse', label: 'Collapse' });
  return out;
}

/** 隙間 gap の開き方を action で変えた、新しい開き方（ほかの隙間はそのまま） */
export function openGap(open: readonly GapOpen[], gap: number, action: GapAction): GapOpen[] {
  const rest = open.filter((g) => g.gap !== gap);
  if (action === 'collapse') return rest;
  const now = open.find((g) => g.gap === gap) ?? { gap, top: 0, bottom: 0 };
  const grow = (n: number) => Math.min(GAP_ALL, n + GAP_STEP);
  // すべて開くときは、隙間の行を開いた行の上に残す（そこから畳める）
  const next =
    action === 'down' ? { ...now, top: grow(now.top) } : action === 'up' ? { ...now, bottom: grow(now.bottom) } : { gap, top: 0, bottom: GAP_ALL };
  return [...rest, next].sort((a, b) => a.gap - b.gap);
}

/** 隙間の行のある hunk の、隙間 */
function gapsOf(hunks: readonly DiffHunk[]): DiffGap[] {
  return hunks.flatMap((h) => (h.gap ? [h.gap] : []));
}

/** ファイル全体を見せている（隙間があって、どれも隠していない） */
export function showsWholeFile(hunks: readonly DiffHunk[]): boolean {
  const gaps = gapsOf(hunks);
  return gaps.some((g) => g.shown > 0) && gaps.every((g) => g.hidden === 0);
}

/** Show Whole File: すべての隙間を開く（見せていれば、すべて畳む） */
export function toggleWholeFile(hunks: readonly DiffHunk[]): GapOpen[] {
  if (showsWholeFile(hunks)) return [];
  return gapsOf(hunks).map((g) => ({ gap: g.index, top: 0, bottom: GAP_ALL }));
}

/** 隙間の行の札の文字（行を選んでいれば選んだ数、隠していれば隠した数、開いていれば見せている数） */
export function pillText(gap: DiffGap, selected: number): { text: string; short?: string } {
  if (selected > 0) return { text: `${selected} line${selected > 1 ? 's' : ''} selected` };
  if (gap.hidden > 0) return { text: `⋯  ${gap.hidden} unchanged line${gap.hidden > 1 ? 's' : ''}`, short: `⋯ ${gap.hidden}` };
  if (gap.shown > 0) return { text: `${gap.shown} unchanged line${gap.shown > 1 ? 's' : ''} shown` };
  return { text: '' };
}

/** 札とボタンの置き方の結果 */
export interface GapFit {
  /** 札の左端（出さなければ undefined） */
  pillX?: number;
  /** 短い札にした */
  short: boolean;
  /** 並べたボタン（chips の中の位置と左端） */
  chips: { index: number; x: number }[];
  /** 並べられなかったボタンがある（札を出さないときは、ボタンがあれば true） */
  overflows: boolean;
}

const MARGIN = 8;
const SPACE = 6;

/**
 * 隙間の行の札とボタンの置き方（行の中の座標）。札は自分の幅で中央に置き、右の hunk の操作（right から左）に
 * 8 より近づくなら左へずらす（left まで）。それでも入らなければ短い札、それも入らなければ札を出さない。
 * ボタンは札の右に入るものだけ並べる
 */
export function placeGap(place: {
  left: number;
  center: number;
  right: number;
  pill: number;
  shortPill?: number;
  chips: readonly number[];
}): GapFit {
  const { left, center, right, pill, shortPill, chips } = place;
  const limit = right - MARGIN;
  const fit = (width: number) => {
    const x = Math.min(Math.max(left, center - width / 2), limit - width);
    return x >= left ? x : undefined;
  };
  let short = false;
  let pillX = fit(pill);
  if (pillX === undefined && shortPill !== undefined) {
    pillX = fit(shortPill);
    short = pillX !== undefined;
  }
  if (pillX === undefined) return { short: false, chips: [], overflows: chips.length > 0 };
  let x = pillX + (short ? shortPill! : pill) + SPACE;
  const placed: { index: number; x: number }[] = [];
  chips.forEach((width, index) => {
    if (x + width < limit) {
      placed.push({ index, x });
      x += width + SPACE;
    }
  });
  return { pillX, short, chips: placed, overflows: placed.length < chips.length };
}

/** 差分の側（hunk の操作の名前を決める） */
export type FoldedSide = 'unstaged' | 'staged' | 'all';
export type StageOp = 'stage' | 'unstage' | 'discard';

/**
 * 変更の行 lines に効く操作と名前（lines なら「Lines」、でなければ「Hunk」）。Unstaged は Stage と Discard、Staged は
 * Unstage、All はすべて入っていれば Unstage、でなければ Stage（アプリと同じく、All では Discard を出さない）
 */
export function stageActions(side: FoldedSide, lines: readonly DiffLine[], noun: 'Lines' | 'Hunk'): { op: StageOp; label: string }[] {
  if (lines.length === 0) return [];
  if (side === 'all') {
    return lines.every((l) => l.staged) ? [{ op: 'unstage', label: `Unstage ${noun}` }] : [{ op: 'stage', label: `Stage ${noun}` }];
  }
  if (side === 'staged') return [{ op: 'unstage', label: `Unstage ${noun}` }];
  return [
    { op: 'stage', label: `Stage ${noun}` },
    { op: 'discard', label: `Discard ${noun}` },
  ];
}

/** 変更のかたまり（続いた追加・削除の行）。hunk をまたがない */
export function changeBlocks(hunks: readonly DiffHunk[]): DiffLine[][] {
  const blocks: DiffLine[][] = [];
  for (const hunk of hunks) {
    let block: DiffLine[] = [];
    for (const line of hunk.lines) {
      if (line.kind === ' ') {
        if (block.length > 0) blocks.push(block);
        block = [];
      } else {
        block.push(line);
      }
    }
    if (block.length > 0) blocks.push(block);
  }
  return blocks;
}

/** 開き方を覚える鍵（ファイルと側ごと。拡張が動いている間だけ） */
export function gapKey(root: string, side: FoldedSide, path: string): string {
  return `${root}\0${side}\0${path}`;
}
