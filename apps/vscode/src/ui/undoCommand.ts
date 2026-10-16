import * as vscode from 'vscode';
import type { RepositoryManager } from '../state/manager.ts';
import { showError } from './stageCommands.ts';

export function registerUndoCommand(manager: RepositoryManager): vscode.Disposable {
  return vscode.commands.registerCommand('amonhen.undo', () => undo(manager));
}

async function undo(manager: RepositoryManager): Promise<void> {
  const repo = manager.current;
  if (!repo) {
    return;
  }
  try {
    const entry = await vscode.window.withProgress({ location: vscode.ProgressLocation.Window, title: 'Undo' }, () =>
      repo.undo.undo(),
    );
    if (entry) {
      vscode.window.setStatusBarMessage(`$(discard) Undo: ${entry.label}`, 5000);
    } else {
      void vscode.window.showInformationMessage('Undo できる操作はありません');
    }
  } catch (err) {
    showError(err);
  } finally {
    await repo.refresh();
  }
}
