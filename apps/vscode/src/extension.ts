import * as vscode from 'vscode';
import { groupChanges } from './state/changes.ts';
import { askpass, setAskpass } from './git/askpass.ts';
import { setFsmonitor } from './git/fsmonitor.ts';
import { startAskpass } from './ui/askpass.ts';
import { openConflict } from './ui/diff.ts';
import { setDaemonLanguage } from './daemon/client.ts';
import { daemonPath, disposeDaemonClients, initDaemon } from './daemon/connection.ts';
import { RepositoryManager } from './state/manager.ts';
import { registerBranchArrangeCommands } from './ui/branchArrangeCommands.ts';
import { registerBranchCommands } from './ui/branchCommands.ts';
import { registerContentProvider } from './ui/content.ts';
import { registerRangeDiffCommands } from './ui/rangeDiff.ts';
import { onDidChangeRangeDiffShown } from './ui/rangeDiffController.ts';
import { SidebarDragAndDrop } from './ui/dragDrop.ts';
import { registerFetchCommand } from './ui/fetchCommand.ts';
import { registerFileToolCommands } from './ui/fileToolCommands.ts';
import { refsScope } from './history/scope.ts';
import { registerIgnoreCommand } from './ui/ignoreCommand.ts';
import { initPreferences } from './ui/preferences.ts';
import { registerFormCommands } from './ui/formCommands.ts';
import { registerHistoryCommands } from './ui/historyCommands.ts';
import { MainPanel } from './ui/mainPanel.ts';
import { registerMergeBranchesCommand } from './ui/mergeBranches.ts';
import { registerNavCommands } from './ui/navCommands.ts';
import { registerBundleArchiveCommands } from './ui/bundleArchiveCommands.ts';
import { registerPatchCommands } from './ui/patchCommands.ts';
import { registerReflogCommands } from './ui/reflogCommands.ts';
import { registerSequenceCommands } from './ui/sequenceCommands.ts';
import { SidebarProvider } from './ui/sidebar.ts';
import { registerStashCommands } from './ui/stashCommands.ts';
import { registerBranchCleanupCommands } from './ui/branchCleanupCommands.ts';
import { registerIgnoredCommands } from './ui/ignoredCommands.ts';
import { registerOrphanCommands } from './ui/orphanCommands.ts';
import { registerRerereCommands } from './ui/rerereCommands.ts';
import { registerRestoreCommands } from './ui/restoreCommands.ts';
import { registerSubmoduleCommands } from './ui/submoduleCommands.ts';
import { registerSubtreeCommands } from './ui/subtreeCommands.ts';
import { registerSyncCommands } from './ui/syncCommands.ts';
import { registerBranchSwitcher } from './ui/branchSwitcher.ts';
import { registerRebuildHistory } from './ui/rebuildHistory.ts';
import { registerTagCommands } from './ui/tagCommands.ts';
import { registerUndoCommand } from './ui/undoCommand.ts';
import { registerWorktreeCommands } from './ui/worktreeCommands.ts';
import type { ViewName } from './view/protocol.ts';

/** 戻り値は結合テストから中身を確かめるためのもの */
/** サイドバーのブランチやタグの行 */
interface RefNode {
  ref?: { name: string };
}

export async function activate(
  context: vscode.ExtensionContext,
): Promise<{
  manager: RepositoryManager;
  sidebar: SidebarProvider;
  dragAndDrop: SidebarDragAndDrop;
  /** 開いているメインビュー（e2e のテストが Webview の代わりにメッセージを入れる） */
  panel: () => MainPanel | undefined;
  /** git と ssh に渡す askpass のスクリプト */
  askpass: () => string | undefined;
}> {
  initPreferences(context.globalState);
  const start = performance.now();
  const log = vscode.window.createOutputChannel('Amon Hen', { log: true });
  // git と ssh が資格情報を尋ねたら、VSCode の入力欄で尋ねる（daemon につなぐ前に置く）
  try {
    const helper = await startAskpass();
    setAskpass(helper.path);
    context.subscriptions.push(helper, { dispose: () => setAskpass(undefined) });
  } catch (err) {
    log.warn(`No password prompt for git: ${err instanceof Error ? err.message : String(err)}`);
  }
  // daemon が作るエラーの文を、VSCode の表示言語にする
  setDaemonLanguage(vscode.env.language);
  // 同梱のエンジンを置き場所に置く（裏で。使うときに待つ）
  initDaemon(context.extensionPath, log);
  // status で git の fsmonitor を使うか。Linux・BSD の Watchman の hook はエンジン（置き終えてから）
  const updateFsmonitor = async () => {
    const enabled = vscode.workspace.getConfiguration('amonhen').get<boolean>('fsmonitor', true);
    setFsmonitor({ enabled });
    setFsmonitor({ enabled, engine: await daemonPath().catch(() => undefined) });
  };
  void updateFsmonitor();
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
    // サイドバーの Conflicts: 最初のコンフリクトのファイルをマージエディタで開く（アプリと同じ）
    vscode.commands.registerCommand('amonhen.openFirstConflict', async () => {
      const repo = manager.current;
      const first = repo && groupChanges(repo.current.status?.entries ?? []).find(([group]) => group === 'conflicts')?.[1][0];
      if (repo && first) await openConflict(repo.root, first, repo.current.sequence);
    }),
    registerIgnoreCommand(manager),
    vscode.commands.registerCommand('amonhen.selectRepository', () => manager.pick()),
    vscode.commands.registerCommand('amonhen.open', () => show()),
    vscode.commands.registerCommand('amonhen.showWorkingCopy', () => show('workingCopy')),
    vscode.commands.registerCommand('amonhen.showHistory', () => show('history')),
    vscode.commands.registerCommand('amonhen.showReflog', () => show('reflog')),
    vscode.commands.registerCommand('amonhen.showRepositorySettings', () => show('settings')),
    vscode.commands.registerCommand('amonhen.showDaemon', () => show('daemon')),
    vscode.commands.registerCommand('amonhen.showRef', (ref: string) => show('history', ref)),
    // サイドバーでいくつも選んだブランチやタグの History
    vscode.commands.registerCommand('amonhen.showRefs', (node?: RefNode, selected?: RefNode[]) => {
      const names = (selected && selected.length > 0 ? selected : node ? [node] : []).flatMap((n) => n.ref?.name ?? []);
      if (names.length > 0) void show('history').history.showScope(refsScope(names));
    }),
    registerUndoCommand(manager),
    registerFetchCommand(manager, () => sidebar.tagMarks.reload()),
    registerSyncCommands(manager),
    registerSequenceCommands(manager),
    registerHistoryCommands(),
    registerPatchCommands(manager),
    registerBundleArchiveCommands(manager),
    registerNavCommands(manager, (repo, name) => sidebar.branchFacts.pullRequestOf(repo, name)),
    registerMergeBranchesCommand(manager),
    registerBranchCommands(manager),
    registerStashCommands(manager),
    registerIgnoredCommands(manager),
    registerBranchCleanupCommands(manager),
    registerOrphanCommands(manager),
    registerRerereCommands(manager),
    registerRestoreCommands(manager),
    registerSubmoduleCommands(manager, sidebar),
    registerSubtreeCommands(manager, sidebar),
    registerRangeDiffCommands(manager, show),
    onDidChangeRangeDiffShown((shown) => sidebar.setRangeDiffShown(shown)),
    registerReflogCommands(manager),
    registerTagCommands(manager, sidebar),
    registerWorktreeCommands(manager, sidebar),
    registerBranchSwitcher(manager, sidebar),
    registerRebuildHistory(manager),
    registerBranchArrangeCommands(manager, sidebar),
    registerFormCommands(manager),
    registerFileToolCommands(manager, (target) => show(target)),
    // エンジンを使えるときだけ出すもの（Reflog・Reset、タグとリモートのメニュー、worktree）
    vscode.workspace.onDidChangeConfiguration((e) => e.affectsConfiguration('amonhen.daemon') && void updateEngine()),
    vscode.workspace.onDidChangeConfiguration(
      (e) => (e.affectsConfiguration('amonhen.fsmonitor') || e.affectsConfiguration('amonhen.daemon')) && void updateFsmonitor(),
    ),
  );
  void updateEngine();

  await manager.initialize();
  updateDescription();
  log.info(`activated in ${Math.round(performance.now() - start)}ms`);
  return { manager, sidebar, dragAndDrop, panel: () => MainPanel.active, askpass: () => askpass() };
}

export function deactivate(): void {
  disposeDaemonClients();
}
