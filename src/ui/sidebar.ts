/**
 * サイドバーの Changes ビュー。メインビューを開かなくても変更を確かめられるよう、
 * 途中で止まっている merge / rebase と、Conflicts / Staged / Changes のファイルだけを出す。
 * ブランチや履歴はメインビューで扱う。
 */
import { basename, dirname, join } from 'node:path';
import * as vscode from 'vscode';
import type { Sequence } from '../git/backend.ts';
import { conflictSides } from '../git/status.ts';
import { groupChanges, type Change, type ChangeGroup } from '../state/changes.ts';
import type { RepositoryManager } from '../state/manager.ts';
import type { Repository } from '../state/repository.ts';
import { decorationUri } from './uri.ts';

export type Node =
  | { kind: 'sequence'; sequence: Sequence; progress?: { current: number; total: number } }
  | { kind: 'group'; group: ChangeGroup; changes: Change[] }
  | { kind: 'change'; change: Change };

const GROUP_LABELS: Record<ChangeGroup, string> = {
  conflicts: 'Conflicts',
  staged: 'Staged',
  unstaged: 'Changes',
};

export class SidebarProvider implements vscode.TreeDataProvider<Node>, vscode.Disposable {
  private readonly manager: RepositoryManager;
  private repoListener: vscode.Disposable | undefined;
  private readonly disposables: vscode.Disposable[] = [];

  private readonly _onDidChangeTreeData = new vscode.EventEmitter<Node | undefined>();
  readonly onDidChangeTreeData = this._onDidChangeTreeData.event;

  constructor(manager: RepositoryManager) {
    this.manager = manager;
    this.disposables.push(manager.onDidChangeCurrent((r) => this.watch(r)));
    this.watch(manager.current);
  }

  private get repo(): Repository | undefined {
    return this.manager.current;
  }

  private watch(repository: Repository | undefined): void {
    this.repoListener?.dispose();
    this.repoListener = repository?.onDidChange((changed) => {
      if (changed.has('workingCopy')) {
        this._onDidChangeTreeData.fire(undefined);
      }
    });
    this._onDidChangeTreeData.fire(undefined);
  }

  getChildren(node?: Node): Node[] {
    const state = this.repo?.current;
    if (!state) {
      return [];
    }
    if (node) {
      return node.kind === 'group' ? node.changes.map((change) => ({ kind: 'change', change })) : [];
    }
    const nodes: Node[] = state.sequence
      ? [{ kind: 'sequence', sequence: state.sequence, progress: state.progress }]
      : [];
    return nodes.concat(
      groupChanges(state.status?.entries ?? [])
        .filter(([, changes]) => changes.length > 0)
        .map(([group, changes]) => ({ kind: 'group', group, changes })),
    );
  }

  getTreeItem(node: Node): vscode.TreeItem {
    switch (node.kind) {
      case 'sequence': {
        const name = node.sequence[0]!.toUpperCase() + node.sequence.slice(1);
        const step = node.progress ? ` (${node.progress.current}/${node.progress.total})` : '';
        const item = new vscode.TreeItem(`${name} in progress${step}`);
        item.id = 'sequence';
        item.description = '解決・コミットしてから Continue';
        item.iconPath = new vscode.ThemeIcon(
          'debug-pause',
          new vscode.ThemeColor('notificationsWarningIcon.foreground'),
        );
        item.contextValue = `sequence.${node.sequence}`;
        return item;
      }
      case 'group': {
        const item = new vscode.TreeItem(GROUP_LABELS[node.group], vscode.TreeItemCollapsibleState.Expanded);
        item.id = `group:${node.group}`;
        item.description = String(node.changes.length);
        item.contextValue = `group.${node.group}`;
        return item;
      }
      case 'change':
        return changeItem(node.change, this.repo?.root ?? '');
    }
  }

  dispose(): void {
    this.repoListener?.dispose();
    vscode.Disposable.from(...this.disposables).dispose();
    this._onDidChangeTreeData.dispose();
  }
}

function changeItem(change: Change, root: string): vscode.TreeItem {
  const abs = join(root, change.path);
  const item = new vscode.TreeItem(basename(change.path));
  item.id = `change:${change.group}:${change.path}`;
  const dir = dirname(change.path);
  const parts = [dir === '.' ? '' : dir];
  if (change.conflict) parts.unshift(conflictSides(change.conflict).label);
  item.description = parts.filter(Boolean).join('  ') || undefined;
  // ファイルアイコンとバッジのため
  item.resourceUri = decorationUri(abs, change.letter);
  item.tooltip = change.origPath ? `${change.origPath} → ${change.path}` : change.path;
  item.contextValue = `change.${change.group}`;
  item.command = { command: 'amonhen.openChange', title: 'Open Diff', arguments: [root, change] };
  return item;
}
