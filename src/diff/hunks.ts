/** 行の差分を、前後に文脈の行を付けた hunk にまとめる。表示用 */
import type { LineChange } from './lineDiff.ts';

export interface HunkLine {
  kind: ' ' | '-' | '+';
  /** base の行（0 始まり）。追加行には無い */
  base?: number;
  /** target の行（0 始まり）。削除行には無い */
  target?: number;
}

export interface Hunk {
  baseStart: number;
  baseCount: number;
  targetStart: number;
  targetCount: number;
  lines: HunkLine[];
}

/** 変更どうしの間が context * 2 行以下なら、1 つの hunk にまとめる */
export function buildHunks(
  baseLength: number,
  targetLength: number,
  changes: readonly LineChange[],
  context = 3,
): Hunk[] {
  const hunks: Hunk[] = [];
  let i = 0;
  while (i < changes.length) {
    // まとめる変更の範囲 [i, j)
    let j = i + 1;
    while (j < changes.length && changes[j]!.baseStart - changes[j - 1]!.baseEnd <= context * 2) {
      j++;
    }
    const first = changes[i]!;
    const last = changes[j - 1]!;
    const lead = Math.min(context, first.baseStart);
    const trail = Math.min(context, baseLength - last.baseEnd, targetLength - last.targetEnd);
    const lines: HunkLine[] = [];
    let b = first.baseStart - lead;
    let t = first.targetStart - lead;
    for (let k = i; k < j; k++) {
      const c = changes[k]!;
      for (; b < c.baseStart; b++, t++) lines.push({ kind: ' ', base: b, target: t });
      for (; b < c.baseEnd; b++) lines.push({ kind: '-', base: b });
      for (; t < c.targetEnd; t++) lines.push({ kind: '+', target: t });
    }
    for (let k = 0; k < trail; k++, b++, t++) lines.push({ kind: ' ', base: b, target: t });
    hunks.push({
      baseStart: first.baseStart - lead,
      baseCount: b - (first.baseStart - lead),
      targetStart: first.targetStart - lead,
      targetCount: t - (first.targetStart - lead),
      lines,
    });
    i = j;
  }
  return hunks;
}
