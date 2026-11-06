/**
 * rerere が当てた解決を忘れ、コンフリクトのある形に戻す（アプリのマージエディタの Forget Resolution。VSCode では
 * Working Copy のコンフリクトのファイルの右クリック）。共有の daemon のコアで行い、作業ツリーのファイルは Undo で戻る
 */
import * as vscode from 'vscode';
import type { RepositoryManager } from '../state/manager.ts';
import { runEngine } from './engine.ts';
import { showError } from './stageCommands.ts';

export function registerRerereCommands(manager: RepositoryManager): vscode.Disposable {
  return vscode.commands.registerCommand('amonhen.forgetResolution', async (ctx?: { path?: string }) => {
    const repo = manager.current;
    if (!repo || !ctx?.path) return;
    try {
      await runEngine(repo, { op: 'forgetResolution', path: ctx.path }, `Forget the resolution of ${ctx.path}`);
      vscode.window.setStatusBarMessage(`$(check) ${ctx.path} is in conflict again`, 5000);
    } catch (err) {
      showError(err);
    } finally {
      await repo.refresh();
    }
  });
}
