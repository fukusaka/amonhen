import * as vscode from 'vscode';
import type { Repository } from '../state/repository.ts';
import { showError } from './stageCommands.ts';

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

/**
 * stage した変更をコミットする。amend なら直前のコミットを置き換える。
 * Undo すると、コミットした変更は stage された状態に戻る。コミットしたら true
 */
export async function commitStaged(repo: Repository, message: string, amend: boolean): Promise<boolean> {
  if (!canCommit(repo, amend)) {
    return false;
  }
  try {
    const branch = (await repo.backend.head()).branch;
    const subject = message.split('\n')[0]!;
    await vscode.window.withProgress({ location: vscode.ProgressLocation.Window, title: 'Commit' }, () =>
      repo.undo.record(`${amend ? 'Amend' : 'Commit'} "${subject}"`, branch ? [branch] : [], 'soft', () =>
        repo.backend.commit(message, { amend }),
      ),
    );
    return true;
  } catch (err) {
    showError(err);
    return false;
  } finally {
    await repo.refresh();
  }
}
