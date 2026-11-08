import * as vscode from 'vscode';
import { GitError } from '../git/exec.ts';
import type { Repository } from '../state/repository.ts';

function stagedCount(repo: Repository): number {
  return (repo.current.status?.entries ?? []).filter(
    (e) => (e.kind === 'changed' || e.kind === 'renamed') && e.index !== '.',
  ).length;
}

/** コミットできる状態か。できなければ理由を出して false */
function canCommit(repo: Repository, amend: boolean): boolean {
  const entries = repo.current.status?.entries ?? [];
  if (entries.some((e) => e.kind === 'unmerged')) {
    void vscode.window.showWarningMessage('コンフリクトを解決してからコミットしてください');
    return false;
  }
  if (!amend && stagedCount(repo) === 0) {
    void vscode.window.showWarningMessage('stage されている変更がありません');
    return false;
  }
  return true;
}

/** コミットの結果。`output` は hooks の出力、`error` は失敗の理由（コミットできる状態でなければどちらもない） */
export interface CommitResult {
  committed: boolean;
  output?: string;
  error?: string;
}

/**
 * stage した変更をコミットする。amend なら直前のコミットを置き換える。
 * Undo すると、コミットした変更は stage された状態に戻る
 */
export async function commitStaged(repo: Repository, message: string, amend: boolean): Promise<CommitResult> {
  if (!canCommit(repo, amend)) {
    return { committed: false };
  }
  try {
    const branch = (await repo.backend.head()).branch;
    const subject = message.split('\n')[0]!;
    const output = await vscode.window.withProgress({ location: vscode.ProgressLocation.Window, title: 'Commit' }, () =>
      repo.undo.record(`${amend ? 'Amend' : 'Commit'} "${subject}"`, branch ? [branch] : [], 'soft', () =>
        repo.backend.commit(message, { amend }),
      ),
    );
    return { committed: true, output };
  } catch (err) {
    return { committed: false, error: err instanceof GitError ? err.stderr.trim() || err.message : String(err) };
  } finally {
    await repo.refresh();
  }
}
