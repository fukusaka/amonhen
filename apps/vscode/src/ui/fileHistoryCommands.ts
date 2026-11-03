/**
 * File History を開く（エクスプローラー・エディタのタブと右クリック、コマンドパレットは今のエディタのファイル、
 * メインビューのファイルの右クリック）。ファイルを含むリポジトリに切り替えて、メインビューの File History に出す
 */
import { relative, sep } from 'node:path';
import * as vscode from 'vscode';
import type { RepositoryManager } from '../state/manager.ts';
import type { Repository } from '../state/repository.ts';
import type { ViewName } from '../view/protocol.ts';
import type { MainPanel } from './mainPanel.ts';

/** メインビューのファイルの右クリック（data-vscode-context）。path はリポジトリの中のパス */
interface FileContext {
  path: string;
  oid?: string;
}

/** `fsPath` を含むリポジトリ（入れ子なら深い方）と、その中のパス（/ 区切り） */
function locate(manager: RepositoryManager, fsPath: string): { repo: Repository; path: string } | undefined {
  const repo = manager.repositories
    .filter((r) => fsPath === r.root || fsPath.startsWith(r.root.endsWith(sep) ? r.root : r.root + sep))
    .sort((a, b) => b.root.length - a.root.length)[0];
  if (!repo) return undefined;
  const path = relative(repo.root, fsPath).split(sep).join('/');
  return path ? { repo, path } : undefined;
}

export function registerFileHistoryCommands(
  manager: RepositoryManager,
  show: (view: ViewName) => MainPanel,
): vscode.Disposable {
  return vscode.Disposable.from(
    vscode.commands.registerCommand('amonhen.fileHistory', async (arg?: vscode.Uri | FileContext) => {
      let target: { repo: Repository; path: string } | undefined;
      if (arg && 'path' in arg && !(arg instanceof vscode.Uri)) {
        target = manager.current && { repo: manager.current, path: arg.path };
      } else {
        const uri = arg instanceof vscode.Uri ? arg : vscode.window.activeTextEditor?.document.uri;
        target = uri?.scheme === 'file' ? locate(manager, uri.fsPath) : undefined;
      }
      if (!target) {
        void vscode.window.showInformationMessage('File History: choose a file in a Git repository');
        return;
      }
      manager.select(target.repo);
      await show('fileHistory').fileHistory.show(target.path);
    }),
    vscode.commands.registerCommand('amonhen.fileHistory.copyPath', async (ctx?: FileContext) => {
      if (!ctx?.path) return;
      await vscode.env.clipboard.writeText(ctx.path);
      vscode.window.setStatusBarMessage(`$(copy) ${ctx.path}`, 3000);
    }),
  );
}
