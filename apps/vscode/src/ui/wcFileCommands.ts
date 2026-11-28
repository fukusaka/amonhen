/**
 * Working Copy のファイルの右クリック（package.json の webview/context、wcFile）のうち、一覧の操作と同じもの:
 * Stage・Unstage、Discard…・Delete File…（untracked）、Open File、OS のファイルマネージャーで見せる（アプリの
 * ファイルの右クリックと同じ並び）。差分の右クリック（wcDiff）も
 */
import { join } from 'node:path';
import * as vscode from 'vscode';
import type { RepositoryManager } from '../state/manager.ts';
import type { DiffMenuAction } from '../workingCopy/protocol.ts';
import { MainPanel } from './mainPanel.ts';

/** 差分の右クリック（wcDiff）の項目。コマンド ID は `amonhen.wcDiff.<項目>` */
const DIFF_ACTIONS: DiffMenuAction[] = [
  'gapDown',
  'gapUp',
  'gapAll',
  'gapCollapse',
  'stageSelected',
  'unstageSelected',
  'discardSelected',
  'stageHunk',
  'unstageHunk',
  'discardHunk',
  'stageLine',
  'unstageLine',
  'discardLine',
];

/** 差分の行・隙間の行の data-vscode-context */
interface WcDiffContext {
  gap?: number;
  hunk?: number;
  line?: string;
}

/** ファイルの行の data-vscode-context */
interface WcFileContext {
  path?: string;
}

export function registerWcFileCommands(manager: RepositoryManager): vscode.Disposable {
  // Stage は作業ツリーの側、Unstage は index の側の変更（Mixed の行は両方あることがある）
  const stage = (op: 'stage' | 'unstage') => async (ctx?: WcFileContext) => {
    if (!ctx?.path) return;
    const group = op === 'stage' ? 'unstaged' : 'staged';
    await MainPanel.active?.workingCopy.handle({ type: 'wcStageFiles', op, files: [{ group, path: ctx.path }] });
  };
  // 確かめてから捨てる（untracked なら Delete と聞く。どちらも Undo で戻せる）
  const discard = async (ctx?: WcFileContext) => {
    if (!ctx?.path) return;
    await MainPanel.active?.workingCopy.handle({ type: 'wcDiscardFiles', paths: [ctx.path] });
  };
  const uriOf = (ctx?: WcFileContext) => {
    const repo = manager.current;
    return repo && ctx?.path ? vscode.Uri.file(join(repo.root, ctx.path)) : undefined;
  };
  const reveal = async (ctx?: WcFileContext) => {
    const uri = uriOf(ctx);
    if (uri) await vscode.commands.executeCommand('revealFileInOS', uri);
  };
  return vscode.Disposable.from(
    vscode.commands.registerCommand('amonhen.wcFile.stage', stage('stage')),
    vscode.commands.registerCommand('amonhen.wcFile.unstage', stage('unstage')),
    vscode.commands.registerCommand('amonhen.wcFile.discard', discard),
    vscode.commands.registerCommand('amonhen.wcFile.delete', discard),
    vscode.commands.registerCommand('amonhen.wcFile.open', async (ctx?: WcFileContext) => {
      const uri = uriOf(ctx);
      if (uri) await vscode.commands.executeCommand('vscode.open', uri, { viewColumn: vscode.ViewColumn.Beside });
    }),
    // 名前は OS ごとの VSCode の言い方（Finder・File Explorer・Open Containing Folder）
    vscode.commands.registerCommand('amonhen.wcFile.revealInFinder', reveal),
    vscode.commands.registerCommand('amonhen.wcFile.revealInExplorer', reveal),
    vscode.commands.registerCommand('amonhen.wcFile.openContainingFolder', reveal),
    // 差分の右クリック: 隙間の開く・畳むと、選んだ行・その hunk・その行の Stage・Unstage・Discard（Webview がする）
    ...DIFF_ACTIONS.map((action) =>
      vscode.commands.registerCommand(`amonhen.wcDiff.${action}`, (ctx?: WcDiffContext) =>
        MainPanel.active?.workingCopy.diffAction(action, { gap: ctx?.gap, hunk: ctx?.hunk, line: ctx?.line }),
      ),
    ),
  );
}
