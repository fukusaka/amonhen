/** サブモジュールの更新と、別のウィンドウで開く */
import { join } from 'node:path';
import * as vscode from 'vscode';
import type { RepositoryManager } from '../state/manager.ts';
import type { Node } from './sidebar.ts';
import { showError } from './stageCommands.ts';

export function registerSubmoduleCommands(manager: RepositoryManager): vscode.Disposable {
  return vscode.Disposable.from(
    vscode.commands.registerCommand('amonhen.submodule.update', async (node?: Node) => {
      const repo = manager.current;
      if (!repo || node?.kind !== 'submodule') return;
      try {
        await vscode.window.withProgress(
          { location: vscode.ProgressLocation.Notification, title: `Update ${node.submodule.path}` },
          () => repo.backend.updateSubmodules([node.submodule.path]),
        );
      } catch (err) {
        showError(err);
      } finally {
        await repo.refresh();
      }
    }),
    vscode.commands.registerCommand('amonhen.submodule.open', async (node?: Node) => {
      const repo = manager.current;
      if (!repo || node?.kind !== 'submodule') return;
      await vscode.commands.executeCommand('vscode.openFolder', vscode.Uri.file(join(repo.root, node.submodule.path)), {
        forceNewWindow: true,
      });
    }),
  );
}
