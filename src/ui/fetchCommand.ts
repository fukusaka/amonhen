import * as vscode from 'vscode';
import type { RepositoryManager } from '../state/manager.ts';
import { showError } from './stageCommands.ts';

/** すべてのリモートから fetch し、消えたリモートブランチも片付ける */
export function registerFetchCommand(manager: RepositoryManager): vscode.Disposable {
  return vscode.commands.registerCommand('amonhen.fetch', async () => {
    const repo = manager.current;
    if (!repo) {
      return;
    }
    try {
      await vscode.window.withProgress({ location: vscode.ProgressLocation.Window, title: 'Fetch' }, () =>
        repo.backend.fetch(),
      );
    } catch (err) {
      showError(err);
    } finally {
      await repo.refresh();
    }
  });
}
