/**
 * まだ stage していない変更の破棄。どれも Undo で戻せるように、破棄する前の内容を記録する。vscode に依存しない
 */
import { rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { applyDiffSelection, type ComputedDiff, type SelectedLine } from '../diff/fileDiff.ts';
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
  // 作業中のファイル（new）を index（old）に向けて、選んだ行だけ戻す。unstage と同じ計算
  const next = applyDiffSelection(computed, 'unstage', lines);
  if (next === computed.newLines.join('')) {
    return;
  }
  await undo.recordFiles(`Discard lines in ${path}`, [path], () =>
    writeFile(join(backend.root, path), Buffer.from(next, 'latin1')),
  );
}
