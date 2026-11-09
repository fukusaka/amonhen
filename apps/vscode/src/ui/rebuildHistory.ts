/**
 * Rebuild History（アプリの Refresh の ▾）: 共有の daemon の History の永続キャッシュを捨てて作り直し、開いている
 * History を読み直す。キャッシュがずれたと思えるときのためのもので、ふだんは要らない
 */
import * as vscode from 'vscode';
import { daemonFor } from '../daemon/connection.ts';
import type { RepositoryManager } from '../state/manager.ts';
import { MainPanel } from './mainPanel.ts';
import { showError } from './stageCommands.ts';

export function registerRebuildHistory(manager: RepositoryManager): vscode.Disposable {
  return vscode.commands.registerCommand('amonhen.rebuildHistory', async () => {
    const repo = manager.current;
    if (!repo) return;
    try {
      const client = await daemonFor(repo.root);
      if (!client) throw new Error('Rebuild History needs the Amon Hen engine (amonhen.daemon.enabled)');
      await vscode.window.withProgress({ location: vscode.ProgressLocation.Notification, title: 'Rebuilding History' }, async () => {
        await client.call({ op: 'history', rebuild: true });
        await repo.refresh();
        await MainPanel.active?.history.reset();
      });
      vscode.window.setStatusBarMessage('$(check) Rebuilt History', 5000);
    } catch (err) {
      showError(err);
    }
  });
}
