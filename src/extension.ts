import { access } from 'node:fs/promises';
import * as vscode from 'vscode';
import { GitError } from './git/exec.ts';
import { RepositoryManager } from './state/manager.ts';
import { registerContentProvider } from './ui/content.ts';
import { ChangeDecorationProvider } from './ui/decorations.ts';
import { SidebarProvider, type Node } from './ui/sidebar.ts';

/** 戻り値は結合テストから中身を確かめるためのもの */
export async function activate(context: vscode.ExtensionContext): Promise<{ manager: RepositoryManager; sidebar: SidebarProvider }> {
  const start = performance.now();
  const log = vscode.window.createOutputChannel('Amon Hen', { log: true });
  const manager = new RepositoryManager(log);
  const sidebar = new SidebarProvider(manager);
  const view = vscode.window.createTreeView('amonhen.sidebar', { treeDataProvider: sidebar });

  const updateDescription = () => {
    view.description = manager.repositories.length > 1 ? manager.current?.root.split(/[\\/]/).pop() : undefined;
  };

  context.subscriptions.push(
    log,
    manager,
    sidebar,
    view,
    view.onDidExpandElement((e) => sidebar.onDidExpand(e.element)),
    manager.onDidChangeCurrent(updateDescription),
    vscode.window.registerFileDecorationProvider(new ChangeDecorationProvider()),
    registerContentProvider(manager),
    vscode.commands.registerCommand('amonhen.refresh', () => manager.current?.refresh()),
    vscode.commands.registerCommand('amonhen.selectRepository', () => manager.pick()),
    vscode.commands.registerCommand('amonhen.checkout', (node: Node) => checkout(manager, node)),
    vscode.commands.registerCommand('amonhen.openChange', (path: string) => openChange(path)),
  );

  await manager.initialize();
  updateDescription();
  log.info(`activated in ${Math.round(performance.now() - start)}ms`);
  return { manager, sidebar };
}

export function deactivate(): void {}

async function checkout(manager: RepositoryManager, node: Node): Promise<void> {
  const repo = manager.current;
  if (!repo || node.kind !== 'branch' || node.remote) {
    return;
  }
  const name = node.ref.name.slice('refs/heads/'.length);
  try {
    await vscode.window.withProgress(
      { location: vscode.ProgressLocation.Window, title: `Checkout ${name}` },
      () => repo.backend.switchBranch(name),
    );
  } catch (err) {
    void vscode.window.showErrorMessage(err instanceof GitError ? err.message : String(err));
  } finally {
    await repo.refresh();
  }
}

async function openChange(path: string): Promise<void> {
  try {
    await access(path);
  } catch {
    void vscode.window.showInformationMessage('ファイルは削除されています');
    return;
  }
  await vscode.commands.executeCommand('vscode.open', vscode.Uri.file(path));
}
