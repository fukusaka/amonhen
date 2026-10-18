/**
 * まだ stage していない変更の破棄。どれも Undo で戻せるように、破棄する前の内容を記録する。vscode に依存しない
 */
import { readFile, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { applyDiffSelection, type ComputedDiff, type SelectedLine } from '../diff/fileDiff.ts';
import { splitLines } from '../diff/lineDiff.ts';
import type { RepositoryBackend } from '../git/backend.ts';
import type { Change } from './changes.ts';
import type { UndoLog } from './undo.ts';

function label(paths: readonly string[]): string {
  return paths.length === 1 ? `Discard ${paths[0]}` : `Discard ${paths.length} files`;
}

/**
 * ファイルごと破棄する。追跡しているファイルは index の内容に戻し、untracked のファイルは消す。
 * changes は Changes（unstaged）のもの
 */
export async function discardFiles(
  backend: RepositoryBackend,
  undo: UndoLog,
  changes: readonly Change[],
): Promise<void> {
  const tracked = changes.filter((c) => c.letter !== 'U').map((c) => c.path);
  const untracked = changes.filter((c) => c.letter === 'U').map((c) => c.path);
  const paths = [...tracked, ...untracked];
  if (paths.length === 0) {
    return;
  }
  await undo.recordFiles(label(paths), paths, async () => {
    if (tracked.length > 0) {
      await backend.restoreWorktree(tracked);
    }
    for (const path of untracked) {
      await rm(join(backend.root, path), { force: true });
    }
  });
}

/**
 * 選んだ行だけを破棄する。computed は index（old）と作業中のファイル（new）の差分。
 * 選んだ追加行を消し、選んだ削除行を戻した内容を、作業中のファイルに書く
 */
export async function discardLines(
  backend: RepositoryBackend,
  undo: UndoLog,
  path: string,
  computed: ComputedDiff,
  lines: readonly SelectedLine[],
): Promise<void> {
  // 差分は git に入れる形（改行の変換をかけたもの）なので、作業中のファイルの形で組み立てる。残す行は作業中のファイルの
  // 行を改行ごとそのまま使い、戻す行は index の内容を checkout と同じ形にしたものを使う（選んだ行のほかは変えない）
  const [raw, restore] = await Promise.all([
    readFile(join(backend.root, path)),
    computed.oldLines.length > 0 ? backend.indexInWorktreeForm(path) : Promise.resolve(Buffer.alloc(0)),
  ]);
  const newLines = splitLines(raw.toString('latin1'));
  const oldLines = splitLines(restore.toString('latin1'));
  if (!sameLines(newLines, computed.newLines) || !sameLines(oldLines, computed.oldLines)) {
    throw new Error(`${path} は、作業中のファイルの形と git に入れる形が行ごとに対応しないため、行単位では破棄できません`);
  }
  // 作業中のファイル（new）を index（old）に向けて、選んだ行だけ戻す。unstage と同じ計算
  const next = applyDiffSelection({ ...computed, oldLines, newLines }, 'unstage', lines);
  if (next === raw.toString('latin1')) {
    return;
  }
  await undo.recordFiles(`Discard lines in ${path}`, [path], () =>
    writeFile(join(backend.root, path), Buffer.from(next, 'latin1')),
  );
}

/** 改行のほかは同じ行の並びか */
function sameLines(a: readonly string[], b: readonly string[]): boolean {
  const body = (line: string) => line.replace(/\r?\n$/, '');
  return a.length === b.length && a.every((line, i) => body(line) === body(b[i]!));
}
