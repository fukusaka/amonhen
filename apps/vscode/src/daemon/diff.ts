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
  /** `workingDiff` の番号（`stageRows`・`discardRows` で使う） */
  handle: unknown;
  rows: Row[];
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
export function toFileDiff(path: string, rows: readonly Row[], binary: boolean, combined = false): FileDiff {
  const version = createHash('sha1')
    .update(path)
    .update('\0')
    .update(rows.map((r) => `${r.kind}:${r.old}:${r.new}:${r.text}`).join('\n'))
    .digest('hex');
  if (binary) {
    return { path, version, hunks: [], binary: true };
  }
  const hunks: DiffHunk[] = [];
  for (const r of rows) {
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
  }
  return { path, version, hunks, ...(combined ? { combined: true } : {}) };
}

/** `workingDiff` の答え */
export function readWorkingDiff(reply: Reply): DaemonDiff {
  const rows = readRowsOf(reply);
  const path = String(reply.json['path']);
  const diff = toFileDiff(path, rows, reply.json['binary'] === true, reply.json['mode'] === 'combined');
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
  return { diff, handle: reply.json['diff'], rows };
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
export function splitCommitDiff(reply: Reply): Map<string, FileDiff> {
  const files = (reply.json['files'] as { path: string; binary: boolean }[] | undefined) ?? [];
  const parts: Row[][] = [];
  for (const r of readRowsOf(reply)) {
    if (r.kind === FILE) {
      parts.push([]);
    } else {
      parts.at(-1)?.push(r);
    }
  }
  const out = new Map<string, FileDiff>();
  files.forEach((f, i) => out.set(f.path, toFileDiff(f.path, parts[i] ?? [], f.binary)));
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
