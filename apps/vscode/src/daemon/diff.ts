/**
 * daemon の差分（`workingDiff`・`commitDiff` の答え）を、メインビューの差分（`FileDiff`）にする。vscode に依存しない。
 * 行単位の stage・破棄は、選んだ行を daemon の差分の行の番号にして頼む
 */
import type { Eol } from '../git/eol.ts';
import { createHash } from 'node:crypto';
import type { DiffHunk, DiffLine, FileDiff, SelectedLine } from '../diff/fileDiff.ts';
import type { FileChange } from '../git/log.ts';
import type { Reply } from './client.ts';

const ADDED = 1;
const REMOVED = 2;
const HUNK = 3;
const FILE = 8;
/** `flags`: HEAD → 作業ツリーの差分で、この行の変更が index に入っている */
const STAGED = 1;

/** 差分の 1 行（daemon の差分の行の番号の順） */
export interface Row {
  kind: number;
  /** `STAGED`（1）など */
  flags: number;
  old: number;
  new: number;
  text: string;
}

/** daemon の差分と、行の番号で頼むための情報 */
export interface DaemonDiff {
  diff: FileDiff;
  /** `workingDiff` の番号（`stageRows`・`discardRows`・`diffSyntax` で使う） */
  handle: unknown;
  rows: Row[];
  /** daemon の差分の行の番号 → その行（構文の色を付ける先） */
  lines: Map<number, DiffLine>;
}

const decoder = new TextDecoder();

/** 差分の行のレコード（rows・textLengths・text）を読む */
export function readRowsOf(reply: Reply): Row[] {
  const raw = reply.u32('rows');
  const lengths = reply.u32('textLengths');
  const text = reply.bytes('text');
  const rows: Row[] = [];
  for (let i = 0; i < lengths.length; i++) {
    const off = raw[i * 4 + 3]!;
    rows.push({
      kind: raw[i * 4]! & 0xff,
      flags: (raw[i * 4]! >> 8) & 0xff,
      old: raw[i * 4 + 1]!,
      new: raw[i * 4 + 2]!,
      text: decoder.decode(text.subarray(off, off + lengths[i]!)),
    });
  }
  return rows;
}

/** 行を hunk に分けた差分。version は中身から作る（表示した差分と今の差分が同じかを確かめる） */
/**
 * index があれば、daemon の差分の行の番号（indices[i] が rows[i] の番号。なければ i）→ 作った行を入れる
 */
export function toFileDiff(
  path: string,
  rows: readonly Row[],
  binary: boolean,
  combined = false,
  index?: Map<number, DiffLine>,
  indices?: readonly number[],
): FileDiff {
  const version = createHash('sha1')
    .update(path)
    .update('\0')
    .update(rows.map((r) => `${r.kind}:${r.old}:${r.new}:${r.text}`).join('\n'))
    .digest('hex');
  if (binary) {
    return { path, version, hunks: [], binary: true };
  }
  const hunks: DiffHunk[] = [];
  for (const [k, r] of rows.entries()) {
    if (r.kind === HUNK) {
      hunks.push({ header: r.text, lines: [] });
      continue;
    }
    const hunk = hunks.at(-1);
    if (!hunk) {
      continue;
    }
    const staged = combined ? { staged: (r.flags & STAGED) !== 0 } : {};
    const line: DiffLine =
      r.kind === ADDED
        ? { kind: '+', new: r.new, text: r.text, ...staged }
        : r.kind === REMOVED
          ? { kind: '-', old: r.old, text: r.text, ...staged }
          : { kind: ' ', old: r.old, new: r.new, text: r.text };
    hunk.lines.push(line);
    index?.set(indices?.[k] ?? k, line);
  }
  return { path, version, hunks, ...(combined ? { combined: true } : {}) };
}

/** `workingDiff` の答え */
export function readWorkingDiff(reply: Reply): DaemonDiff {
  const rows = readRowsOf(reply);
  const path = String(reply.json['path']);
  const lines = new Map<number, DiffLine>();
  const diff = toFileDiff(path, rows, reply.json['binary'] === true, reply.json['mode'] === 'combined', lines);
  const le = reply.json['lineEndings'] as Record<string, unknown> | null | undefined;
  if (le) {
    diff.lineEndings = {
      index: le['index'] as Eol,
      worktree: le['worktree'] as Eol,
      checkout: le['checkout'] as Eol,
      rule: String(le['rule'] ?? ''),
      notable: le['notable'] === true,
      warning: le['warning'] === true,
    };
  }
  return { diff, handle: reply.json['diff'], rows, lines };
}

/** UTF-8 のバイト位置を、text の中の文字（UTF-16）の位置に */
export function charIndex(text: string, byte: number): number {
  let bytes = 0;
  let i = 0;
  while (i < text.length && bytes < byte) {
    const code = text.codePointAt(i)!;
    bytes += code < 0x80 ? 1 : code < 0x800 ? 2 : code < 0x10000 ? 3 : 4;
    i += code >= 0x10000 ? 2 : 1;
  }
  return i;
}

/**
 * `diffSyntax` の答え（row・start・end・kind の u32 × 4。位置は行の文字列の UTF-8 のバイト）を、lines の行に付ける。
 * 色の付いた行を返す
 */
export function applySyntax(reply: Reply, lines: ReadonlyMap<number, DiffLine>): Set<DiffLine> {
  const spans = reply.u32('spans');
  const touched = new Set<DiffLine>();
  for (let i = 0; i + 3 < spans.length; i += 4) {
    const line = lines.get(spans[i]!);
    if (!line) continue;
    const start = charIndex(line.text, spans[i + 1]!);
    const end = charIndex(line.text, spans[i + 2]!);
    if (end <= start) continue;
    if (!touched.has(line)) line.syntax = [];
    line.syntax!.push([start, end, spans[i + 3]!]);
    touched.add(line);
  }
  return touched;
}

/** `textSyntax` の答え（位置は文の先頭からの UTF-16）を、lines（文を \n で分けたもの）の行ごとに */
export function textSyntaxByLine(reply: Reply, lines: readonly string[]): [number, number, number][][] {
  const spans = reply.u32('spans');
  const starts: number[] = [];
  let at = 0;
  for (const line of lines) {
    starts.push(at);
    at += line.length + 1;
  }
  const out: [number, number, number][][] = lines.map(() => []);
  let row = 0;
  for (let i = 0; i + 3 < spans.length; i += 4) {
    let start = spans[i + 1]!;
    const end = spans[i + 2]!;
    const kind = spans[i + 3]!;
    // トークンは位置の順。行をまたぐもの（複数行のコメントなど）は行ごとに分ける
    while (row + 1 < starts.length && starts[row + 1]! <= start) row++;
    let r = row;
    while (r < lines.length && start < end) {
      const lineEnd = starts[r]! + lines[r]!.length;
      const e = Math.min(end, lineEnd);
      if (e > start) out[r]!.push([start - starts[r]!, e - starts[r]!, kind]);
      r++;
      start = starts[r] ?? end;
    }
  }
  return out;
}

/** 変更の行（追加・削除）の番号 */
export function changedRows(rows: readonly Row[]): number[] {
  return rows.flatMap((r, i) => (r.kind === ADDED || r.kind === REMOVED ? [i] : []));
}

/** 選んだ行を、daemon の差分の行の番号に */
export function selectedRows(rows: readonly Row[], lines: readonly SelectedLine[]): number[] {
  const out: number[] = [];
  rows.forEach((r, i) => {
    const hit = lines.some((l) =>
      l.kind === '-' ? r.kind === REMOVED && r.old === l.old : r.kind === ADDED && r.new === l.new,
    );
    if (hit) {
      out.push(i);
    }
  });
  return out;
}

/** `commitDiff` の答えを、ファイルごとの差分に（見出しの行で分ける） */
export function splitCommitDiff(reply: Reply, index?: Map<number, DiffLine>): Map<string, FileDiff> {
  const files = (reply.json['files'] as { path: string; binary: boolean }[] | undefined) ?? [];
  const parts: { rows: Row[]; indices: number[] }[] = [];
  for (const [i, r] of readRowsOf(reply).entries()) {
    if (r.kind === FILE) {
      parts.push({ rows: [], indices: [] });
    } else {
      parts.at(-1)?.rows.push(r);
      parts.at(-1)?.indices.push(i);
    }
  }
  const out = new Map<string, FileDiff>();
  files.forEach((f, i) =>
    out.set(f.path, toFileDiff(f.path, parts[i]?.rows ?? [], f.binary, false, index, parts[i]?.indices)),
  );
  return out;
}

/** `commitDiff` の答えの、変わったファイル（`git diff-tree` と同じ形、行数つき） */
export function commitFiles(reply: Reply): FileChange[] {
  const letters: Record<string, string> = { added: 'A', deleted: 'D', modified: 'M', renamed: 'R', copied: 'C' };
  const changes = (reply.json['changes'] as { kind: string; path: string; oldPath: string | null }[] | undefined) ?? [];
  const counts = new Map(
    ((reply.json['files'] as { path: string; added?: number; removed?: number }[] | undefined) ?? []).map((f) => [f.path, f]),
  );
  return changes.map((c) => {
    const count = counts.get(c.path);
    return {
      status: letters[c.kind] ?? 'M',
      path: c.path,
      ...(c.oldPath && c.oldPath !== c.path ? { origPath: c.oldPath } : {}),
      ...(typeof count?.added === 'number' && typeof count.removed === 'number'
        ? { added: count.added, removed: count.removed }
        : {}),
    };
  });
}
