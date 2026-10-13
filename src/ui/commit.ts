import * as vscode from 'vscode';
import type { RepositoryManager } from '../state/manager.ts';
import { showError } from './stageCommands.ts';

export function registerCommitCommand(manager: RepositoryManager): vscode.Disposable {
  return vscode.commands.registerCommand('amonhen.commit', () => commit(manager));
}

async function commit(manager: RepositoryManager): Promise<void> {
  const repo = manager.current;
  const status = repo?.current.status;
  if (!repo || !status) {
    return;
  }
  if (status.entries.some((e) => e.kind === 'unmerged')) {
    void vscode.window.showWarningMessage('コンフリクトを解決してからコミットしてください');
    return;
  }
  const staged = status.entries.filter((e) => (e.kind === 'changed' || e.kind === 'renamed') && e.index !== '.');
  if (staged.length === 0) {
    void vscode.window.showWarningMessage('stage されている変更がありません');
    return;
  }
  const message = await vscode.window.showInputBox({
    title: `Commit ${staged.length} file${staged.length > 1 ? 's' : ''} to ${status.branch.head ?? 'detached HEAD'}`,
    placeHolder: 'Commit message',
    validateInput: (v) => (v.trim() ? undefined : 'メッセージを入力してください'),
  });
  if (!message) {
    return;
  }
  try {
    await vscode.window.withProgress({ location: vscode.ProgressLocation.Window, title: 'Commit' }, () =>
      repo.backend.commit(message),
    );
  } catch (err) {
    showError(err);
  } finally {
    await repo.refresh();
  }
}
