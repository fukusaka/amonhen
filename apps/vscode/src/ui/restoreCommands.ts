/**
 * 別のコミットの版に戻す（アプリの History のファイルの Restore This Version・Restore the Version Before）。作業
 * ツリーのファイルをその版にし（その版になければ消す）、選べば index にも入れる。共有の daemon のコアで行い、作業
 * ツリーのファイルは Undo で戻る
 */
import * as vscode from 'vscode';
import type { RepositoryManager } from '../state/manager.ts';
import { runEngine } from './engine.ts';
import { showError } from './stageCommands.ts';

/** History の詳細のファイルの右クリック（data-vscode-context） */
interface CommitFileContext {
  oid?: string;
  path?: string;
}

export function registerRestoreCommands(manager: RepositoryManager): vscode.Disposable {
  const restore = (id: string, before: boolean) =>
    vscode.commands.registerCommand(id, async (ctx?: CommitFileContext) => {
      const repo = manager.current;
      if (!repo || !ctx?.oid || !ctx.path) return;
      const rev = before ? `${ctx.oid}^` : ctx.oid;
      const which = before ? `the version before ${ctx.oid.slice(0, 8)}` : `its version in ${ctx.oid.slice(0, 8)}`;
      const answer = await vscode.window.showWarningMessage(
        `Restore ${ctx.path} to ${which}?`,
        {
          modal: true,
          detail:
            'Replaces the file in the working tree, or deletes it if that version has none. Undo puts the working tree’s file back. Restore and Stage also puts it in the index (Undo doesn’t take that back).',
        },
        'Restore',
        'Restore and Stage',
      );
      if (answer !== 'Restore' && answer !== 'Restore and Stage') return;
      try {
        await runEngine(repo, { op: 'restoreFiles', rev, paths: [ctx.path], staged: answer === 'Restore and Stage' }, `Restore ${ctx.path}`);
        vscode.window.setStatusBarMessage(`$(history) Restored ${ctx.path}`, 5000);
      } catch (err) {
        showError(err);
      } finally {
        await repo.refresh();
      }
    });
  return vscode.Disposable.from(
    restore('amonhen.restoreFile.thisVersion', false),
    restore('amonhen.restoreFile.versionBefore', true),
  );
}
