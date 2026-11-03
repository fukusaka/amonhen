/**
 * File History と Blame を開く（エクスプローラー・エディタのタブの右クリック、コマンドパレットは今のエディタの
 * ファイル、メインビューのファイルの右クリック）。ファイルを含むリポジトリに切り替えて、メインビューに出す
 */
import { realpathSync } from 'node:fs';
import { relative, sep } from 'node:path';
import * as vscode from 'vscode';
import type { RepositoryManager } from '../state/manager.ts';
import type { Repository } from '../state/repository.ts';
import type { ViewName } from '../view/protocol.ts';
import type { MainPanel } from './mainPanel.ts';

/** メインビューのファイルの右クリック（data-vscode-context）。path はリポジトリの中のパス、oid はそのコミット */
interface FileContext {
  path: string;
  oid?: string;
}

/** 右クリックで渡されたもの（エクスプローラーなら Uri、なければ今のエディタ）のリポジトリとパス */
function targetOf(
  manager: RepositoryManager,
  arg: vscode.Uri | FileContext | undefined,
): { repo: Repository; path: string } | undefined {
  if (arg && 'path' in arg && !(arg instanceof vscode.Uri)) {
    return manager.current && { repo: manager.current, path: arg.path };
  }
  const uri = arg instanceof vscode.Uri ? arg : vscode.window.activeTextEditor?.document.uri;
  return uri?.scheme === 'file' ? locate(manager, uri.fsPath) : undefined;
}

/**
 * `fsPath` を含むリポジトリ（入れ子なら深い方）と、その中のパス（/ 区切り）。Windows では大文字小文字を区別しない
 * （VSCode の Uri はドライブ文字を小文字にするが、git は大文字で返す）。見つからなければ、シンボリックリンクを
 * 解いて探し直す（git のリポジトリのパスは解いたもの。macOS の /var → /private/var など）
 */
function locate(manager: RepositoryManager, fsPath: string): { repo: Repository; path: string } | undefined {
  return within(manager, fsPath, (p) => p) ?? within(manager, fsPath, real);
}

function within(
  manager: RepositoryManager,
  fsPath: string,
  resolve: (p: string) => string,
): { repo: Repository; path: string } | undefined {
  const fold = (p: string) => (process.platform === 'win32' ? p.toLowerCase() : p);
  const filePath = resolve(fsPath);
  const file = fold(filePath);
  const found = manager.repositories
    .map((r) => ({ r, root: resolve(r.root) }))
    .filter(({ root }) => {
      const folded = fold(root);
      return file === folded || file.startsWith(folded.endsWith(sep) ? folded : folded + sep);
    })
    .sort((a, b) => b.root.length - a.root.length)[0];
  if (!found) return undefined;
  const path = relative(found.root, filePath).split(sep).join('/');
  return path ? { repo: found.r, path } : undefined;
}

function real(p: string): string {
  try {
    return realpathSync.native(p);
  } catch {
    return p;
  }
}

export function registerFileToolCommands(
  manager: RepositoryManager,
  show: (view: ViewName) => MainPanel,
): vscode.Disposable {
  return vscode.Disposable.from(
    vscode.commands.registerCommand('amonhen.fileHistory', async (arg?: vscode.Uri | FileContext) => {
      const target = targetOf(manager, arg);
      if (!target) {
        void vscode.window.showInformationMessage('File History: choose a file in a Git repository');
        return;
      }
      manager.select(target.repo);
      await show('fileHistory').fileHistory.show(target.path);
    }),
    // コミットの行（History の詳細・File History・Blame）からなら、そのコミットの内容。ほかは作業ツリーの内容
    vscode.commands.registerCommand('amonhen.blame', async (arg?: vscode.Uri | FileContext) => {
      const target = targetOf(manager, arg);
      if (!target) {
        void vscode.window.showInformationMessage('Blame: choose a file in a Git repository');
        return;
      }
      const rev = arg && !(arg instanceof vscode.Uri) ? arg.oid : undefined;
      manager.select(target.repo);
      await show('blame').blame.show(target.path, rev);
    }),
    vscode.commands.registerCommand('amonhen.fileHistory.copyPath', async (ctx?: FileContext) => {
      if (!ctx?.path) return;
      await vscode.env.clipboard.writeText(ctx.path);
      vscode.window.setStatusBarMessage(`$(copy) ${ctx.path}`, 3000);
    }),
  );
}
