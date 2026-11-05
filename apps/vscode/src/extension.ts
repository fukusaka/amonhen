import * as vscode from 'vscode';
import { setDaemonLanguage } from './daemon/client.ts';
import { daemonPath, disposeDaemonClients, initDaemon } from './daemon/connection.ts';
import { RepositoryManager } from './state/manager.ts';
import { registerBranchArrangeCommands } from './ui/branchArrangeCommands.ts';
import { registerBranchCommands } from './ui/branchCommands.ts';
import { registerContentProvider } from './ui/content.ts';
import { registerRangeDiffCommands } from './ui/rangeDiff.ts';
import { SidebarDragAndDrop } from './ui/dragDrop.ts';
import { registerFetchCommand } from './ui/fetchCommand.ts';
import { registerFileToolCommands } from './ui/fileToolCommands.ts';
import { registerFormCommands } from './ui/formCommands.ts';
import { registerHistoryCommands } from './ui/historyCommands.ts';
import { MainPanel } from './ui/mainPanel.ts';
import { registerMergeBranchesCommand } from './ui/mergeBranches.ts';
import { registerNavCommands } from './ui/navCommands.ts';
import { registerPatchCommands } from './ui/patchCommands.ts';
import { registerReflogCommands } from './ui/reflogCommands.ts';
import { registerSequenceCommands } from './ui/sequenceCommands.ts';
import { SidebarProvider } from './ui/sidebar.ts';
import { registerStashCommands } from './ui/stashCommands.ts';
import { registerRestoreCommands } from './ui/restoreCommands.ts';
import { registerSubmoduleCommands } from './ui/submoduleCommands.ts';
import { registerSubtreeCommands } from './ui/subtreeCommands.ts';
import { registerSyncCommands } from './ui/syncCommands.ts';
import { registerTagCommands } from './ui/tagCommands.ts';
import { registerUndoCommand } from './ui/undoCommand.ts';
import { registerWorktreeCommands } from './ui/worktreeCommands.ts';
import type { ViewName } from './view/protocol.ts';

/** 戻り値は結合テストから中身を確かめるためのもの */
export async function activate(
  context: vscode.ExtensionContext,
): Promise<{
  manager: RepositoryManager;
  sidebar: SidebarProvider;
  dragAndDrop: SidebarDragAndDrop;
  /** 開いているメインビュー（e2e のテストが Webview の代わりにメッセージを入れる） */
  panel: () => MainPanel | undefined;
}> {
  const start = performance.now();
  const log = vscode.window.createOutputChannel('Amon Hen', { log: true });
  // daemon が作るエラーの文を、VSCode の表示言語にする
  setDaemonLanguage(vscode.env.language);
  // 同梱のエンジンを置き場所に置く（裏で。使うときに待つ）
  initDaemon(context.extensionPath, log);
  const manager = new RepositoryManager(log);
  const sidebar = new SidebarProvider(manager);
  const dragAndDrop = new SidebarDragAndDrop(manager);
  const view = vscode.window.createTreeView('amonhen.sidebar', {
    treeDataProvider: sidebar,
    dragAndDropController: dragAndDrop,
    // ブランチ・タグをいくつも選んで、まとめて merge・削除・お気に入り・アーカイブする
    canSelectMany: true,
  });
  const show = (target?: ViewName, ref?: string) =>
    MainPanel.show(context.extensionUri, manager, target, ref);

  const updateEngine = async () => {
    const engine = !!(await daemonPath().catch(() => undefined));
    sidebar.setEngine(engine);
    void vscode.commands.executeCommand('setContext', 'amonhen.engine', engine);
  };
  const updateDescription = () => {
    view.description = manager.repositories.length > 1 ? manager.current?.root.split(/[\\/]/).pop() : undefined;
  };

  context.subscriptions.push(
    log,
    manager,
    sidebar,
    view,
    view.onDidExpandElement((e) => sidebar.onDidExpand(e.element)),
    view.onDidCollapseElement((e) => sidebar.onDidCollapse(e.element)),
    // Branches・Remotes・Tags の絞り込み（アプリのサイドバーの上の欄と同じ）。何で絞っているかを上に出す
    sidebar.onDidChangeFilter((text) => (view.message = text ? `Filter: ${text}` : undefined)),
    vscode.commands.registerCommand('amonhen.filterRefs', async () => {
      const text = await vscode.window.showInputBox({
        title: 'Filter Branches, Remotes and Tags',
        prompt: 'Shows those whose names contain all the words (empty to show all)',
        value: sidebar.filter,
      });
      if (text !== undefined) sidebar.setFilter(text);
    }),
    vscode.commands.registerCommand('amonhen.clearRefsFilter', () => sidebar.setFilter('')),
    // アクティビティバーの Amon Hen を開いたら、メインビューも開く
    view.onDidChangeVisibility((e) => e.visible && manager.current && show()),
    manager.onDidChangeCurrent(updateDescription),
    registerContentProvider(manager),
    vscode.commands.registerCommand('amonhen.refresh', () => manager.current?.refresh()),
    vscode.commands.registerCommand('amonhen.selectRepository', () => manager.pick()),
    vscode.commands.registerCommand('amonhen.open', () => show()),
    vscode.commands.registerCommand('amonhen.showWorkingCopy', () => show('workingCopy')),
    vscode.commands.registerCommand('amonhen.showHistory', () => show('history')),
    vscode.commands.registerCommand('amonhen.showReflog', () => show('reflog')),
    vscode.commands.registerCommand('amonhen.showRepositorySettings', () => show('settings')),
    vscode.commands.registerCommand('amonhen.showRef', (ref: string) => show('history', ref)),
    registerUndoCommand(manager),
    registerFetchCommand(manager, () => sidebar.tagMarks.reload()),
    registerSyncCommands(manager),
    registerSequenceCommands(manager),
    registerHistoryCommands(),
    registerPatchCommands(manager),
    registerNavCommands(manager),
    registerMergeBranchesCommand(manager),
    registerBranchCommands(manager),
    registerStashCommands(manager),
    registerRestoreCommands(manager),
    registerSubmoduleCommands(manager, sidebar),
    registerSubtreeCommands(manager, sidebar),
    registerRangeDiffCommands(manager),
    registerReflogCommands(manager),
    registerTagCommands(manager, sidebar),
    registerWorktreeCommands(manager, sidebar),
    registerBranchArrangeCommands(manager, sidebar),
    registerFormCommands(manager),
    registerFileToolCommands(manager, (target) => show(target)),
    // エンジンを使えるときだけ出すもの（Reflog・Reset、タグとリモートのメニュー、worktree）
    vscode.workspace.onDidChangeConfiguration((e) => e.affectsConfiguration('amonhen.daemon') && void updateEngine()),
  );
  void updateEngine();

  await manager.initialize();
  updateDescription();
  log.info(`activated in ${Math.round(performance.now() - start)}ms`);
  return { manager, sidebar, dragAndDrop, panel: () => MainPanel.active };
}

export function deactivate(): void {
  disposeDaemonClients();
}
