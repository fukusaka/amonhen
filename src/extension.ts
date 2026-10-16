import * as vscode from 'vscode';
import { disposeDaemonClients } from './daemon/connection.ts';
import { RepositoryManager } from './state/manager.ts';
import { registerBranchCommands } from './ui/branchCommands.ts';
import { registerContentProvider } from './ui/content.ts';
import { SidebarDragAndDrop } from './ui/dragDrop.ts';
import { registerFetchCommand } from './ui/fetchCommand.ts';
import { registerHistoryCommands } from './ui/historyCommands.ts';
import { MainPanel } from './ui/mainPanel.ts';
import { registerNavCommands } from './ui/navCommands.ts';
import { registerSequenceCommands } from './ui/sequenceCommands.ts';
import { SidebarProvider } from './ui/sidebar.ts';
import { registerStashCommands } from './ui/stashCommands.ts';
import { registerSubmoduleCommands } from './ui/submoduleCommands.ts';
import { registerSyncCommands } from './ui/syncCommands.ts';
import { registerUndoCommand } from './ui/undoCommand.ts';

/** 戻り値は結合テストから中身を確かめるためのもの */
export async function activate(
  context: vscode.ExtensionContext,
): Promise<{ manager: RepositoryManager; sidebar: SidebarProvider; dragAndDrop: SidebarDragAndDrop }> {
  const start = performance.now();
  const log = vscode.window.createOutputChannel('Amon Hen', { log: true });
  const manager = new RepositoryManager(log);
  const sidebar = new SidebarProvider(manager);
  const dragAndDrop = new SidebarDragAndDrop(manager);
  const view = vscode.window.createTreeView('amonhen.sidebar', {
    treeDataProvider: sidebar,
    dragAndDropController: dragAndDrop,
  });
  const show = (target?: 'workingCopy' | 'history', ref?: string) =>
    MainPanel.show(context.extensionUri, manager, target, ref);

  const updateDescription = () => {
    view.description = manager.repositories.length > 1 ? manager.current?.root.split(/[\\/]/).pop() : undefined;
  };

  context.subscriptions.push(
    log,
    manager,
    sidebar,
    view,
    view.onDidExpandElement((e) => sidebar.onDidExpand(e.element)),
    // アクティビティバーの Amon Hen を開いたら、メインビューも開く
    view.onDidChangeVisibility((e) => e.visible && manager.current && show()),
    manager.onDidChangeCurrent(updateDescription),
    registerContentProvider(manager),
    vscode.commands.registerCommand('amonhen.refresh', () => manager.current?.refresh()),
    vscode.commands.registerCommand('amonhen.selectRepository', () => manager.pick()),
    vscode.commands.registerCommand('amonhen.open', () => show()),
    vscode.commands.registerCommand('amonhen.showWorkingCopy', () => show('workingCopy')),
    vscode.commands.registerCommand('amonhen.showHistory', () => show('history')),
    vscode.commands.registerCommand('amonhen.showRef', (ref: string) => show('history', ref)),
    registerUndoCommand(manager),
    registerFetchCommand(manager),
    registerSyncCommands(manager),
    registerSequenceCommands(manager),
    registerHistoryCommands(),
    registerNavCommands(manager),
    registerBranchCommands(manager),
    registerStashCommands(manager),
    registerSubmoduleCommands(manager),
  );

  await manager.initialize();
  updateDescription();
  log.info(`activated in ${Math.round(performance.now() - start)}ms`);
  return { manager, sidebar, dragAndDrop };
}

export function deactivate(): void {
  disposeDaemonClients();
}
