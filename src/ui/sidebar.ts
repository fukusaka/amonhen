import { basename, dirname, join } from 'node:path';
import * as vscode from 'vscode';
import type { Ref, Remote, Stash } from '../git/refs.ts';
import { splitRemoteRef } from '../git/refs.ts';
import type { Sequence } from '../git/backend.ts';
import { conflictSides, type StatusEntry } from '../git/status.ts';
import type { RepositoryManager } from '../state/manager.ts';
import { shortName } from '../state/operations.ts';
import type { LazySection, Repository, Section, Submodule } from '../state/repository.ts';
import { decorationUri } from './uri.ts';

export type ChangeGroup = 'conflicts' | 'staged' | 'unstaged';

export interface Change {
  group: ChangeGroup;
  path: string;
  origPath?: string;
  /** バッジに出す 1 文字 */
  letter: string;
  /** コンフリクトしているときの XY（UU、DU など） */
  conflict?: string;
}

interface Folder<T> {
  folders: Map<string, Folder<T>>;
  leaves: { name: string; item: T }[];
}

export type Node =
  | { kind: 'section'; section: Section }
  | { kind: 'group'; group: ChangeGroup; changes: Change[] }
  | { kind: 'change'; change: Change }
  | { kind: 'folder'; id: string; label: string; folder: Folder<Ref>; remote?: string }
  | { kind: 'branch'; ref: Ref; label: string; remote?: string }
  | { kind: 'detached'; oid: string }
  | { kind: 'remote'; remote: Remote; folder: Folder<Ref> }
  | { kind: 'tag'; ref: Ref }
  | { kind: 'stash'; stash: Stash }
  | { kind: 'submodule'; submodule: Submodule; state?: string }
  | { kind: 'sequence'; sequence: Sequence; progress?: { current: number; total: number } };

const SECTION_LABELS: Record<Section, string> = {
  workingCopy: 'Working Copy',
  branches: 'Branches',
  remotes: 'Remotes',
  tags: 'Tags',
  stashes: 'Stashes',
  submodules: 'Submodules',
};

const LAZY: ReadonlySet<Section> = new Set<LazySection>(['tags', 'stashes', 'submodules']);

const GROUP_LABELS: Record<ChangeGroup, string> = {
  conflicts: 'Conflicts',
  staged: 'Staged',
  unstaged: 'Changes',
};

export class SidebarProvider implements vscode.TreeDataProvider<Node>, vscode.Disposable {
  private readonly manager: RepositoryManager;
  /** 更新通知で同じノードを指せるよう、セクションのノードは使い回す */
  private readonly sections = new Map<Section, Node>(
    (Object.keys(SECTION_LABELS) as Section[]).map((s) => [s, { kind: 'section', section: s }]),
  );
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
      // 変わったセクションだけ描き直す
      for (const section of changed) {
        this._onDidChangeTreeData.fire(this.sections.get(section));
      }
    });
    this._onDidChangeTreeData.fire(undefined);
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
    if (!node) {
      return [...this.sections.values()];
    }
    switch (node.kind) {
      case 'section':
        return this.sectionChildren(repo, node.section);
      case 'group':
        return node.changes.map((change) => ({ kind: 'change', change }));
      case 'folder':
        return folderChildren(node.folder, node.id, node.remote);
      case 'remote':
        return folderChildren(node.folder, `remote:${node.remote.name}`, node.remote.name);
      default:
        return [];
    }
  }

  private sectionChildren(repo: Repository, section: Section): Node[] {
    const state = repo.current;
    switch (section) {
      case 'workingCopy': {
        const nodes: Node[] = state.sequence
          ? [{ kind: 'sequence', sequence: state.sequence, progress: state.progress }]
          : [];
        return nodes.concat(
          groupChanges(state.status?.entries ?? [])
            .filter(([, changes]) => changes.length > 0)
            .map(([group, changes]) => ({ kind: 'group', group, changes })),
        );
      }
      case 'branches': {
        const nodes: Node[] = [];
        const status = state.status;
        if (status && !status.branch.head && status.branch.oid) {
          nodes.push({ kind: 'detached', oid: status.branch.oid });
        }
        const folder = buildFolder(state.branches ?? [], (r) => r.name.slice('refs/heads/'.length));
        return nodes.concat(folderChildren(folder, 'branches'));
      }
      case 'remotes': {
        const remotes = state.remotes ?? [];
        const names = remotes.map((r) => r.name);
        const byRemote = new Map<string, Ref[]>(names.map((n) => [n, []]));
        for (const ref of state.remoteBranches ?? []) {
          // `origin/HEAD` は表示しない
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
    const repo = this.repo;
    switch (node.kind) {
      case 'section':
        return this.sectionItem(node.section, repo);
      case 'group': {
        const item = new vscode.TreeItem(GROUP_LABELS[node.group], vscode.TreeItemCollapsibleState.Expanded);
        item.id = `group:${node.group}`;
        item.description = String(node.changes.length);
        item.contextValue = `group.${node.group}`;
        return item;
      }
      case 'change':
        return changeItem(node.change, repo?.root ?? '');
      case 'folder': {
        const item = new vscode.TreeItem(node.label, vscode.TreeItemCollapsibleState.Collapsed);
        item.id = node.id;
        item.iconPath = vscode.ThemeIcon.Folder;
        return item;
      }
      case 'branch':
        return branchItem(node);
      case 'detached': {
        const item = new vscode.TreeItem('HEAD');
        item.id = 'detached';
        item.description = `detached at ${node.oid.slice(0, 8)}`;
        item.iconPath = new vscode.ThemeIcon('check');
        return item;
      }
      case 'remote': {
        const item = new vscode.TreeItem(node.remote.name, vscode.TreeItemCollapsibleState.Collapsed);
        item.id = `remote:${node.remote.name}`;
        item.description = node.remote.fetchUrl;
        item.iconPath = new vscode.ThemeIcon('cloud');
        item.contextValue = 'remote';
        return item;
      }
      case 'tag': {
        const name = node.ref.name.slice('refs/tags/'.length);
        const item = new vscode.TreeItem(name);
        item.id = node.ref.name;
        item.description = node.ref.subject;
        item.tooltip = `${name}\n${(node.ref.peeled ?? node.ref.oid).slice(0, 8)}  ${node.ref.subject}`;
        item.iconPath = new vscode.ThemeIcon('tag');
        item.contextValue = 'tag';
        return item;
      }
      case 'stash': {
        const item = new vscode.TreeItem(node.stash.name);
        item.id = `stash:${node.stash.oid}`;
        item.description = node.stash.subject;
        item.tooltip = `${node.stash.name}\n${node.stash.subject}`;
        item.iconPath = new vscode.ThemeIcon('archive');
        item.contextValue = 'stash';
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

  private sectionItem(section: Section, repo: Repository | undefined): vscode.TreeItem {
    const expanded = section === 'workingCopy' || section === 'branches';
    const item = new vscode.TreeItem(
      SECTION_LABELS[section],
      expanded ? vscode.TreeItemCollapsibleState.Expanded : vscode.TreeItemCollapsibleState.Collapsed,
    );
    item.id = `section:${section}`;
    item.contextValue = `section.${section}`;
    const state = repo?.current;
    if (section === 'workingCopy') {
      const count = state?.status?.entries.filter((e) => e.kind !== 'ignored').length ?? 0;
      item.description = count > 0 ? `${count} changed` : 'clean';
    } else if (section === 'branches') {
      item.description = state?.status?.branch.head;
    } else if (section === 'stashes' && state?.status?.stashCount) {
      item.description = String(state.status.stashCount);
    }
    return item;
  }

  dispose(): void {
    this.repoListener?.dispose();
    vscode.Disposable.from(...this.disposables).dispose();
    this._onDidChangeTreeData.dispose();
  }
}

/** status のエントリを Conflicts / Staged / Changes に振り分ける。1 つのファイルが Staged と Changes の両方に出ることもある */
export function groupChanges(entries: readonly StatusEntry[]): [ChangeGroup, Change[]][] {
  const conflicts: Change[] = [];
  const staged: Change[] = [];
  const unstaged: Change[] = [];
  for (const e of entries) {
    switch (e.kind) {
      case 'unmerged':
        conflicts.push({ group: 'conflicts', path: e.path, letter: '!', conflict: e.index + e.worktree });
        break;
      case 'untracked':
        unstaged.push({ group: 'unstaged', path: e.path, letter: 'U' });
        break;
      case 'changed':
      case 'renamed':
        if (e.index !== '.') {
          staged.push({ group: 'staged', path: e.path, origPath: e.origPath, letter: e.index });
        }
        if (e.worktree !== '.') {
          unstaged.push({ group: 'unstaged', path: e.path, letter: e.worktree });
        }
        break;
    }
  }
  return [
    ['conflicts', conflicts],
    ['staged', staged],
    ['unstaged', unstaged],
  ];
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
  const name = shortName(ref.name);
  const lines = [name];
  if (ref.upstream) lines.push(`upstream: ${ref.upstream.replace(/^refs\/remotes\//, '')}`);
  lines.push(`${ref.oid.slice(0, 8)}  ${ref.subject}`);
  item.tooltip = lines.join('\n');
  item.contextValue = node.remote ? 'remoteBranch' : ref.isHead ? 'branch.current' : 'branch';
  return item;
}

function buildFolder(refs: readonly Ref[], path: (r: Ref) => string): Folder<Ref> {
  const root: Folder<Ref> = { folders: new Map(), leaves: [] };
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
    folder.leaves.push({ name, item: ref });
  }
  return root;
}

function folderChildren(folder: Folder<Ref>, id: string, remote?: string): Node[] {
  const folders: Node[] = [...folder.folders.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([label, f]) => ({ kind: 'folder', id: `${id}/${label}`, label, folder: f, remote }));
  const leaves: Node[] = [...folder.leaves]
    .sort((a, b) => a.name.localeCompare(b.name))
    .map((l) => ({ kind: 'branch', ref: l.item, label: l.name, remote }));
  return folders.concat(leaves);
}

function describeSubmodule(entry: StatusEntry | undefined): string | undefined {
  const s = entry?.submodule;
  if (!s || s[0] !== 'S') {
    return undefined;
  }
  const parts: string[] = [];
  if (s[1] === 'C') parts.push('new commits');
  if (s[2] === 'M') parts.push('modified');
  if (s[3] === 'U') parts.push('untracked');
  return parts.join(', ') || undefined;
}
