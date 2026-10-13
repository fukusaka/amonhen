import * as vscode from 'vscode';
import { RepositoryManager } from './state/manager.ts';
import { registerCommitCommand } from './ui/commit.ts';
import { registerContentProvider } from './ui/content.ts';
import { ChangeDecorationProvider } from './ui/decorations.ts';
import { openChangeDiff, trackActiveDiff } from './ui/diff.ts';
import type { Change } from './state/changes.ts';
import { SidebarProvider } from './ui/sidebar.ts';
import { registerFetchCommand } from './ui/fetchCommand.ts';
import { registerHistoryCommands } from './ui/historyCommands.ts';
import { MainPanel } from './ui/mainPanel.ts';
import { registerNavCommands } from './ui/navCommands.ts';
import { registerSequenceCommands } from './ui/sequenceCommands.ts';
import { registerStageCommands } from './ui/stageCommands.ts';
import { registerSyncCommands } from './ui/syncCommands.ts';
import { registerUndoCommand } from './ui/undoCommand.ts';

/** 戻り値は結合テストから中身を確かめるためのもの */
export async function activate(
  context: vscode.ExtensionContext,
): Promise<{ manager: RepositoryManager; sidebar: SidebarProvider }> {
  const start = performance.now();
  const log = vscode.window.createOutputChannel('Amon Hen', { log: true });
  const manager = new RepositoryManager(log);
  const sidebar = new SidebarProvider(manager);
  const view = vscode.window.createTreeView('amonhen.sidebar', { treeDataProvider: sidebar, canSelectMany: true });
  const openMain = () => MainPanel.show(context.extensionUri, manager);

  const updateDescription = () => {
    view.description = manager.repositories.length > 1 ? manager.current?.root.split(/[\\/]/).pop() : undefined;
  };

  context.subscriptions.push(
    log,
    manager,
    sidebar,
    view,
    // アクティビティバーの Amon Hen を開いたら、メインビューも開く
    view.onDidChangeVisibility((e) => e.visible && manager.current && openMain()),
    manager.onDidChangeCurrent(updateDescription),
    vscode.window.registerFileDecorationProvider(new ChangeDecorationProvider()),
    registerContentProvider(manager),
    vscode.commands.registerCommand('amonhen.refresh', () => manager.current?.refresh()),
    vscode.commands.registerCommand('amonhen.selectRepository', () => manager.pick()),
    vscode.commands.registerCommand('amonhen.openChange', (root: string, change: Change) =>
      openChangeDiff(root, change, manager.get(root)?.current.sequence),
    ),
    trackActiveDiff(),
    registerStageCommands(manager),
    registerCommitCommand(manager),
    registerUndoCommand(manager),
    registerFetchCommand(manager),
    registerSyncCommands(manager),
    registerSequenceCommands(manager),
    registerHistoryCommands(),
    registerNavCommands(),
    vscode.commands.registerCommand('amonhen.open', openMain),
    vscode.commands.registerCommand('amonhen.showHistory', () =>
      MainPanel.show(context.extensionUri, manager, 'history'),
    ),
  );

  await manager.initialize();
  updateDescription();
  log.info(`activated in ${Math.round(performance.now() - start)}ms`);
  return { manager, sidebar };
}

export function deactivate(): void {}
