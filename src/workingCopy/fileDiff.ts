/**
 * Working Copy の 1 ファイルの差分を、Webview に送る形に組み立てる。vscode に依存しない。
 * 行の計算は latin1 で行い（バイトと 1 対 1 で内容を壊さない）、表示する文字列だけ UTF-8 で読む。
 */
import { createHash } from 'node:crypto';
import { buildHunks } from '../diff/hunks.ts';
import { applyLineSets, diffLines, splitLines, type LineChange } from '../diff/lineDiff.ts';
import type { ChangeGroup } from '../state/changes.ts';
import type { DiffHunk, FileDiff, SelectedLine } from './protocol.ts';

/** これより大きいファイルは差分を出さない */
const MAX_BYTES = 4 * 1024 * 1024;

export interface ComputedDiff {
  diff: FileDiff;
  oldLines: string[];
  newLines: string[];
  changes: LineChange[];
}

/**
 * old と new の差分。Changes では old が index、new が作業中のファイル。Staged では old が HEAD、new が index。
 * 無い側は undefined
 */
export function computeDiff(
  group: ChangeGroup,
  path: string,
  oldData: Buffer | undefined,
  newData: Buffer | undefined,
): ComputedDiff {
  const o = oldData ?? Buffer.alloc(0);
  const n = newData ?? Buffer.alloc(0);
  const version = createHash('sha1').update(o).update('\0').update(n).digest('hex');
  const base = { group, path, version };
  if (o.length + n.length > MAX_BYTES) {
    return { diff: { ...base, hunks: [], tooLarge: true }, oldLines: [], newLines: [], changes: [] };
  }
  if (isBinary(o) || isBinary(n)) {
    return { diff: { ...base, hunks: [], binary: true }, oldLines: [], newLines: [], changes: [] };
  }
  const oldLines = splitLines(o.toString('latin1'));
  const newLines = splitLines(n.toString('latin1'));
  const changes = diffLines(oldLines, newLines);
  const hunks: DiffHunk[] = buildHunks(oldLines.length, newLines.length, changes).map((h) => ({
    header: `@@ -${h.baseStart + 1},${h.baseCount} +${h.targetStart + 1},${h.targetCount} @@`,
    lines: h.lines.map((l) => {
      const text = display(l.kind === '+' ? newLines[l.target!]! : oldLines[l.base!]!);
      return l.kind === ' '
        ? { kind: l.kind, old: l.base! + 1, new: l.target! + 1, text }
        : l.kind === '-'
          ? { kind: l.kind, old: l.base! + 1, text }
          : { kind: l.kind, new: l.target! + 1, text };
    }),
  }));
  return { diff: { ...base, hunks }, oldLines, newLines, changes };
}

/**
 * 選んだ行だけを当てた、新しい index の内容（latin1）。
 * - stage（Changes）: index（old）に、作業中のファイル（new）との違いのうち選んだ行を当てる
 * - unstage（Staged）: index（new）を、HEAD（old）に向けて選んだ行だけ戻す
 */
export function applyDiffSelection(computed: ComputedDiff, op: 'stage' | 'unstage', lines: readonly SelectedLine[]): string {
  const { oldLines, newLines, changes } = computed;
  const removed = new Set(lines.flatMap((l) => (l.kind === '-' ? [l.old - 1] : [])));
  const added = new Set(lines.flatMap((l) => (l.kind === '+' ? [l.new - 1] : [])));
  if (op === 'stage') {
    return applyLineSets(oldLines, newLines, changes, removed, added);
  }
  // index から見て HEAD へ戻す。追加した行（+）を消し、消した行（-）を戻す
  const reversed = changes.map((c) => ({
    baseStart: c.targetStart,
    baseEnd: c.targetEnd,
    targetStart: c.baseStart,
    targetEnd: c.baseEnd,
  }));
  return applyLineSets(newLines, oldLines, reversed, added, removed, true);
}

function isBinary(data: Buffer): boolean {
  return data.subarray(0, 8000).includes(0);
}

function display(line: string): string {
  return Buffer.from(line.replace(/\r?\n$/, ''), 'latin1').toString('utf8');
}
