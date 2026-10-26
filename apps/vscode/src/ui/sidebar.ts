/**
 * サイドバーのナビ。Working Copy と History の入口、途中で止まっている merge / rebase、
 * ブランチ・リモート・タグ・stash・サブモジュールを出す。中身はメインビューに出す。
 */
import * as vscode from 'vscode';
import type { Sequence } from '../git/backend.ts';
import type { Ref, Remote, Stash } from '../git/refs.ts';
import { splitRemoteRef } from '../git/refs.ts';
import { describeSubmodule } from '../git/status.ts';
import type { RepositoryManager } from '../state/manager.ts';
import { shortName } from '../state/operations.ts';
import type { LazySection, Repository, Submodule } from '../state/repository.ts';
import type { ViewName } from '../view/protocol.ts';

type RefSection = 'branches' | 'remotes' | 'tags' | 'stashes' | 'submodules';

interface Folder {
  folders: Map<string, Folder>;
  leaves: { name: string; ref: Ref }[];
}

export type Node =
  | { kind: 'view'; view: ViewName }
  | { kind: 'sequence'; sequence: Sequence; progress?: { current: number; total: number } }
  | { kind: 'section'; section: RefSection }
  | { kind: 'folder'; id: string; label: string; folder: Folder; remote?: string }
  | { kind: 'branch'; ref: Ref; label: string; remote?: string }
  | { kind: 'remote'; remote: Remote; folder: Folder }
  | { kind: 'tag'; ref: Ref }
  | { kind: 'stash'; stash: Stash }
  | { kind: 'submodule'; submodule: Submodule; state?: string };

const SECTION_LABELS: Record<RefSection, string> = {
  branches: 'Branches',
  remotes: 'Remotes',
  tags: 'Tags',
  stashes: 'Stashes',
  submodules: 'Submodules',
};

const LAZY: ReadonlySet<RefSection> = new Set<LazySection>(['tags', 'stashes', 'submodules']);

export class SidebarProvider implements vscode.TreeDataProvider<Node>, vscode.Disposable {
  private readonly manager: RepositoryManager;
  private repoListener: vscode.Disposable | undefined;
  private readonly disposables: vscode.Disposable[] = [];
  /** 更新通知で同じノードを指せるよう、セクションのノードは使い回す */
  private readonly sections = new Map<RefSection, Node>(
    (Object.keys(SECTION_LABELS) as RefSection[]).map((s) => [s, { kind: 'section', section: s }]),
  );

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
    this.updateContext(repository);
    this.repoListener = repository?.onDidChange((changed) => {
      if (changed.has('branches')) this.updateContext(repository);
      // Working Copy の変化は、入口の数と途中の merge / rebase に出る
      if (changed.has('workingCopy')) {
        this._onDidChangeTreeData.fire(undefined);
        return;
      }
      for (const section of changed) {
        const node = this.sections.get(section as RefSection);
        if (node) this._onDidChangeTreeData.fire(node);
      }
    });
    this._onDidChangeTreeData.fire(undefined);
  }

  /** 今のブランチに upstream が無ければ、ナビの上の Push を Publish にする */
  private updateContext(repository: Repository | undefined): void {
    const head = repository?.current.branches?.find((b) => b.isHead);
    void vscode.commands.executeCommand('setContext', 'amonhen.unpublished', !!head && (!head.upstream || !!head.gone));
  }

  /** 遅延セクションが展開されたら取得を始める */
  onDidExpand(node: Node): void {
    if (node.kind === 'section' && LAZY.has(node.section)) {
      void this.repo?.want(node.section as LazySection);
    }
  }

  getChildren(node?: Node): Node[] {
    const repo = this.repo;
    if (!repo) {
      return [];
    }
    const state = repo.current;
    if (!node) {
      const nodes: Node[] = [
        { kind: 'view', view: 'workingCopy' },
        { kind: 'view', view: 'history' },
      ];
      if (state.sequence) nodes.push({ kind: 'sequence', sequence: state.sequence, progress: state.progress });
      return nodes.concat([...this.sections.values()]);
    }
    switch (node.kind) {
      case 'section':
        return this.sectionChildren(repo, node.section);
      case 'folder':
        return folderChildren(node.folder, node.id, node.remote);
      case 'remote':
        return folderChildren(node.folder, `remote:${node.remote.name}`, node.remote.name);
      default:
        return [];
    }
  }

  private sectionChildren(repo: Repository, section: RefSection): Node[] {
    const state = repo.current;
    switch (section) {
      case 'branches':
        return folderChildren(
          buildFolder(state.branches ?? [], (r) => shortName(r.name)),
          'branches',
        );
      case 'remotes': {
        const remotes = state.remotes ?? [];
        const names = remotes.map((r) => r.name);
        const byRemote = new Map<string, Ref[]>(names.map((n) => [n, []]));
        for (const ref of state.remoteBranches ?? []) {
          // `origin/HEAD` は出さない
          if (ref.symref) continue;
          const split = splitRemoteRef(ref.name, names);
          if (split) byRemote.get(split.remote)?.push(ref);
        }
        return remotes.map((remote) => ({
          kind: 'remote',
          remote,
          folder: buildFolder(byRemote.get(remote.name) ?? [], (r) => r.name.slice(`refs/remotes/${remote.name}/`.length)),
        }));
      }
      case 'tags':
        return [...(state.tags ?? [])].sort((a, b) => b.date - a.date).map((ref) => ({ kind: 'tag', ref }));
      case 'stashes':
        return (state.stashes ?? []).map((stash) => ({ kind: 'stash', stash }));
      case 'submodules': {
        const entries = new Map((state.status?.entries ?? []).map((e) => [e.path, e]));
        return (state.submodules ?? []).map((submodule) => ({
          kind: 'submodule',
          submodule,
          state: describeSubmodule(entries.get(submodule.path)),
        }));
      }
    }
  }

  getTreeItem(node: Node): vscode.TreeItem {
    const state = this.repo?.current;
    switch (node.kind) {
      case 'view': {
        if (node.view === 'workingCopy') {
          const item = new vscode.TreeItem('Working Copy');
          item.id = 'view:workingCopy';
          const count = state?.status?.entries.filter((e) => e.kind !== 'ignored').length ?? 0;
          item.description = count > 0 ? `${count} changed` : 'clean';
          item.iconPath = new vscode.ThemeIcon('diff');
          item.command = { command: 'amonhen.showWorkingCopy', title: 'Show Working Copy' };
          return item;
        }
        const item = new vscode.TreeItem('History');
        item.id = 'view:history';
        item.description = state?.status?.branch.head ?? 'detached HEAD';
        item.iconPath = new vscode.ThemeIcon('history');
        item.command = { command: 'amonhen.showHistory', title: 'Show History' };
        return item;
      }
      case 'sequence': {
        const name = node.sequence[0]!.toUpperCase() + node.sequence.slice(1);
        const step = node.progress ? ` (${node.progress.current}/${node.progress.total})` : '';
        const item = new vscode.TreeItem(`${name} in progress${step}`);
        item.id = 'sequence';
        item.description = '解決・コミットしてから Continue';
        item.iconPath = new vscode.ThemeIcon('debug-pause', new vscode.ThemeColor('notificationsWarningIcon.foreground'));
        item.contextValue = `sequence.${node.sequence}`;
        item.command = { command: 'amonhen.showWorkingCopy', title: 'Show Working Copy' };
        return item;
      }
      case 'section': {
        const item = new vscode.TreeItem(
          SECTION_LABELS[node.section],
          node.section === 'branches'
            ? vscode.TreeItemCollapsibleState.Expanded
            : vscode.TreeItemCollapsibleState.Collapsed,
        );
        item.id = `section:${node.section}`;
        item.contextValue = `section.${node.section}`;
        if (node.section === 'stashes' && state?.status?.stashCount) {
          item.description = String(state.status.stashCount);
        }
        return item;
      }
      case 'folder': {
        const item = new vscode.TreeItem(node.label, vscode.TreeItemCollapsibleState.Collapsed);
        item.id = node.id;
        item.iconPath = vscode.ThemeIcon.Folder;
        return item;
      }
      case 'branch':
        return branchItem(node);
      case 'remote': {
        const item = new vscode.TreeItem(node.remote.name, vscode.TreeItemCollapsibleState.Collapsed);
        item.id = `remote:${node.remote.name}`;
        item.description = node.remote.fetchUrl;
        item.iconPath = new vscode.ThemeIcon('cloud');
        item.contextValue = 'remote';
        return item;
      }
      case 'tag': {
        const name = shortName(node.ref.name);
        const item = new vscode.TreeItem(name);
        item.id = node.ref.name;
        item.description = node.ref.subject;
        item.tooltip = `${name}\n${(node.ref.peeled ?? node.ref.oid).slice(0, 8)}  ${node.ref.subject}`;
        item.iconPath = new vscode.ThemeIcon('tag');
        item.contextValue = 'tag';
        item.command = { command: 'amonhen.showRef', title: 'Show History', arguments: [node.ref.name] };
        return item;
      }
      case 'stash': {
        const item = new vscode.TreeItem(node.stash.name);
        item.id = `stash:${node.stash.oid}`;
        item.description = node.stash.subject;
        item.tooltip = `${node.stash.name}\n${node.stash.subject}`;
        item.iconPath = new vscode.ThemeIcon('archive');
        item.contextValue = 'stash';
        item.command = { command: 'amonhen.stash.show', title: 'Show Stash', arguments: [node] };
        return item;
      }
      case 'submodule': {
        const item = new vscode.TreeItem(node.submodule.path);
        item.id = `submodule:${node.submodule.path}`;
        item.description = node.submodule.initialized ? node.state : 'not initialized';
        item.iconPath = new vscode.ThemeIcon('file-submodule');
        item.contextValue = 'submodule';
        return item;
      }
    }
  }

  dispose(): void {
    this.repoListener?.dispose();
    vscode.Disposable.from(...this.disposables).dispose();
    this._onDidChangeTreeData.dispose();
  }
}

function branchItem(node: Extract<Node, { kind: 'branch' }>): vscode.TreeItem {
  const ref = node.ref;
  const item = new vscode.TreeItem(node.label);
  item.id = node.remote ? `remoteBranch:${ref.name}` : ref.name;
  const parts: string[] = [];
  if (ref.ahead) parts.push(`↑${ref.ahead}`);
  if (ref.behind) parts.push(`↓${ref.behind}`);
  if (ref.gone) parts.push('gone');
  item.description = parts.join(' ') || undefined;
  item.iconPath = new vscode.ThemeIcon(ref.isHead ? 'check' : 'git-branch');
  const lines = [shortName(ref.name)];
  if (ref.upstream) lines.push(`upstream: ${ref.upstream.replace(/^refs\/remotes\//, '')}`);
  lines.push(`${ref.oid.slice(0, 8)}  ${ref.subject}`);
  item.tooltip = lines.join('\n');
  item.contextValue = node.remote ? 'remoteBranch' : (ref.isHead ? 'branch.current' : 'branch') + syncState(ref);
  // 選ぶと、そのブランチの履歴をメインビューに出す
  item.command = { command: 'amonhen.showRef', title: 'Show History', arguments: [ref.name] };
  return item;
}

/**
 * ローカルブランチの upstream との関係（右クリックの Publish・Push・Fast-Forward の出し分け）。`;unpublished` は upstream が
 * 無いか消えている、`;ahead`・`;behind` は片側にだけ進んでいる。分かれているか同じなら空
 */
function syncState(ref: Ref): string {
  if (!ref.upstream || ref.gone) return ';unpublished';
  if ((ref.ahead ?? 0) > 0 && !ref.behind) return ';ahead';
  if ((ref.behind ?? 0) > 0 && !ref.ahead) return ';behind';
  return '';
}

function buildFolder(refs: readonly Ref[], path: (r: Ref) => string): Folder {
  const root: Folder = { folders: new Map(), leaves: [] };
  for (const ref of refs) {
    const parts = path(ref).split('/');
    const name = parts.pop()!;
    let folder = root;
    for (const part of parts) {
      let next = folder.folders.get(part);
      if (!next) {
        next = { folders: new Map(), leaves: [] };
        folder.folders.set(part, next);
      }
      folder = next;
    }
    folder.leaves.push({ name, ref });
  }
  return root;
}

function folderChildren(folder: Folder, id: string, remote?: string): Node[] {
  const folders: Node[] = [...folder.folders.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([label, f]) => ({ kind: 'folder', id: `${id}/${label}`, label, folder: f, remote }));
  const leaves: Node[] = [...folder.leaves]
    .sort((a, b) => a.name.localeCompare(b.name))
    .map((l) => ({ kind: 'branch', ref: l.ref, label: l.name, remote }));
  return folders.concat(leaves);
}
