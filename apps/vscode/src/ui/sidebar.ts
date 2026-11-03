/**
 * サイドバーのナビ。Working Copy・History・Reflog の入口、途中で止まっている merge / rebase、
 * ブランチ・リモート・タグ・stash・サブモジュールを出す。中身はメインビューに出す。
 */
import * as vscode from 'vscode';
import type { Sequence } from '../git/backend.ts';
import type { Ref, Remote, Stash } from '../git/refs.ts';
import { splitRemoteRef } from '../git/refs.ts';
import { signatureExplanation, signatureIcon, signatureLabel, signatureStatus } from '../git/signature.ts';
import { describeSubmodule } from '../git/status.ts';
import { tagMark, tagMarkLabel, tagStateText, tagTargets } from '../git/tagRemotes.ts';
import { samePath, worktreeLayout, worktreeName, type Worktree } from '../git/worktrees.ts';
import type { RepositoryManager } from '../state/manager.ts';
import { shortName } from '../state/operations.ts';
import type { LazySection, Repository, Submodule } from '../state/repository.ts';
import type { ViewName } from '../view/protocol.ts';
import { TagMarks } from './tagMarks.ts';
import { WorktreeList } from './worktreeList.ts';

/**
 * 途中の操作の行の名前（進み具合を除く）。知らせからこの行を指す（知らせは VSCode が 12 秒ほどで通知センターに
 * 移すが、この行にはいつでも Continue・Abort がある）
 */
export function sequenceRowName(sequence: Sequence): string {
  return sequence === 'am' ? 'Applying patches' : `${sequence[0]!.toUpperCase()}${sequence.slice(1)} in progress`;
}

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
  | { kind: 'submodule'; submodule: Submodule; state?: string }
  /** Working Copy の下の worktree */
  | { kind: 'worktree'; worktree: Worktree }
  /** 変更のない worktree をまとめる行 */
  | { kind: 'cleanWorktrees'; worktrees: Worktree[] };

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
  /** 共有の daemon のエンジンを使えるか（Reflog、タグの印とメニューはエンジンだけ） */
  private engine = false;
  /** タグのリモートでの様子と署名 */
  readonly tagMarks = new TagMarks();
  /** リポジトリの worktree（2 つ以上あるときだけ Working Copy の下に出す） */
  readonly worktrees = new WorktreeList();
  /** 更新通知で同じノードを指せるよう、セクションのノードは使い回す */
  private readonly sections = new Map<RefSection, Node>(
    (Object.keys(SECTION_LABELS) as RefSection[]).map((s) => [s, { kind: 'section', section: s }]),
  );

  private readonly _onDidChangeTreeData = new vscode.EventEmitter<Node | undefined>();
  readonly onDidChangeTreeData = this._onDidChangeTreeData.event;

  constructor(manager: RepositoryManager) {
    this.manager = manager;
    this.disposables.push(
      manager.onDidChangeCurrent((r) => this.watch(r)),
      this.tagMarks,
      this.tagMarks.onDidChange(() => this._onDidChangeTreeData.fire(this.sections.get('tags'))),
      this.worktrees,
      // Working Copy の行（子が変わる）とブランチの印
      this.worktrees.onDidChange(() => this._onDidChangeTreeData.fire(undefined)),
    );
    this.watch(manager.current);
  }

  private get repo(): Repository | undefined {
    return this.manager.current;
  }

  private watch(repository: Repository | undefined): void {
    this.repoListener?.dispose();
    this.updateContext(repository);
    this.tagMarks.setRepository(this.engine ? repository : undefined);
    this.worktrees.setRepository(this.engine ? repository : undefined);
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

  /** エンジンを使えるかが分かった・変わった */
  setEngine(engine: boolean): void {
    if (engine !== this.engine) {
      this.engine = engine;
      this.tagMarks.setRepository(engine ? this.repo : undefined);
      this.worktrees.setRepository(engine ? this.repo : undefined);
      this._onDidChangeTreeData.fire(undefined);
    }
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
      nodes.push(...this.sections.values());
      // Reflog はいつもいちばん下（アプリと同じ）
      if (this.engine) nodes.push({ kind: 'view', view: 'reflog' });
      return nodes;
    }
    switch (node.kind) {
      case 'section':
        return this.sectionChildren(repo, node.section);
      case 'folder':
        return folderChildren(node.folder, node.id, node.remote);
      case 'remote':
        return folderChildren(node.folder, `remote:${node.remote.name}`, node.remote.name);
      case 'view': {
        if (node.view !== 'workingCopy') return [];
        const layout = this.worktreeLayout(repo);
        const nodes: Node[] = layout.shown.map((worktree) => ({ kind: 'worktree', worktree }));
        if (layout.clean.length > 0) nodes.push({ kind: 'cleanWorktrees', worktrees: layout.clean });
        return nodes;
      }
      case 'cleanWorktrees':
        return node.worktrees.map((worktree) => ({ kind: 'worktree', worktree }));
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
        if (this.engine) this.tagMarks.want();
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
    const state = repo?.current;
    switch (node.kind) {
      case 'view': {
        if (node.view === 'workingCopy') {
          const item = new vscode.TreeItem('Working Copy');
          item.id = 'view:workingCopy';
          const count = state?.status?.entries.filter((e) => e.kind !== 'ignored').length ?? 0;
          item.description = count > 0 ? `${count} changed` : 'clean';
          item.iconPath = new vscode.ThemeIcon('diff');
          item.command = { command: 'amonhen.showWorkingCopy', title: 'Show Working Copy' };
          item.contextValue = 'view.workingCopy';
          // worktree が 2 つ以上あれば、その並びを下に出す（書き込む先は ✎）
          if (repo && this.worktreeLayout(repo).shown.length > 0) {
            item.collapsibleState = vscode.TreeItemCollapsibleState.Expanded;
            item.tooltip = `Working Copy of ✎ ${repo.root}`;
          }
          return item;
        }
        if (node.view === 'reflog') {
          const item = new vscode.TreeItem('Reflog');
          item.id = 'view:reflog';
          item.tooltip = 'Every place HEAD has been, including commits no branch reaches any more';
          item.iconPath = new vscode.ThemeIcon('list-ordered');
          item.command = { command: 'amonhen.showReflog', title: 'Show Reflog' };
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
        const step = node.progress ? ` (${node.progress.current}/${node.progress.total})` : '';
        const am = node.sequence === 'am' ? state?.am : undefined;
        const item = new vscode.TreeItem(`${sequenceRowName(node.sequence)}${step}`);
        item.id = 'sequence';
        item.description = '解決・コミットしてから Continue';
        item.iconPath = new vscode.ThemeIcon('debug-pause', new vscode.ThemeColor('notificationsWarningIcon.foreground'));
        item.contextValue = `sequence.${node.sequence}`;
        if (am) {
          // 止まったパッチと、なぜ止まったか（当たらない・空・コンフリクト）
          const conflicts = state?.status?.entries.some((e) => e.kind === 'unmerged') ?? false;
          const subject = am.subject ? `「${am.subject}」` : 'パッチ';
          item.tooltip = am.subject;
          if (am.empty) {
            item.description = `${subject}に変更がありません。Skip してください`;
          } else if (conflicts || am.retried) {
            item.description = `${subject}のコンフリクトを解決して stage してから Continue`;
          } else if (am.threeWay) {
            item.description = `${subject}が当たりません。手で当てて Continue するか Skip`;
          } else {
            item.description = `${subject}が当たりません。3-way merge で当て直すか、手で当てて Continue するか Skip`;
            item.contextValue = 'sequence.am.retry';
          }
        }
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
      case 'branch': {
        const other =
          !node.remote && repo && !node.ref.isHead
            ? this.worktrees.checkedOutElsewhere(shortName(node.ref.name), repo.root)
            : undefined;
        return branchItem(node, other);
      }
      case 'worktree':
        return this.worktreeItem(node.worktree);
      case 'cleanWorktrees': {
        const item = new vscode.TreeItem('No changes', vscode.TreeItemCollapsibleState.Collapsed);
        item.id = 'worktrees:clean';
        item.description = String(node.worktrees.length);
        item.tooltip = 'Worktrees without changes';
        item.iconPath = new vscode.ThemeIcon('check-all');
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
      case 'tag':
        return this.tagItem(node.ref);
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

  private worktreeLayout(repo: Repository): { shown: Worktree[]; clean: Worktree[] } {
    if (!this.engine) return { shown: [], clean: [] };
    return worktreeLayout(
      this.worktrees.worktrees,
      (w) => this.worktrees.changes(w),
      (w) => samePath(w.path, repo.root),
    );
  }

  /**
   * worktree の行。書き込む先（今のリポジトリ）は ✎、メインの作業ツリーは ⌂、追加した worktree は ⑂ のアイコン。
   * 選ぶと、その worktree を書き込む先にする
   */
  private worktreeItem(w: Worktree): vscode.TreeItem {
    const repo = this.repo;
    const editing = !!repo && samePath(w.path, repo.root);
    const item = new vscode.TreeItem(worktreeName(w));
    item.id = `worktree:${w.path}`;
    const changes = this.worktrees.changes(w);
    const parts = [w.branch ?? (w.head ? `detached at ${w.head.slice(0, 8)}` : 'no commits')];
    if (w.prunable !== null) parts.push('folder is gone');
    else parts.push(changes > 0 ? `${changes} changed` : 'no changes');
    if (w.locked !== null) parts.push('locked');
    item.description = parts.join(' · ');
    item.iconPath =
      w.prunable !== null
        ? new vscode.ThemeIcon('warning', new vscode.ThemeColor('list.warningForeground'))
        : editing
          ? new vscode.ThemeIcon('edit')
          : w.main
            ? new vscode.ThemeIcon('home')
            : new vscode.ThemeIcon('repo-forked', new vscode.ThemeColor('charts.green'));
    item.tooltip = [
      w.path,
      w.branch ? `Branch: ${w.branch}` : 'detached HEAD',
      editing ? 'Editing this worktree' : w.prunable === null ? 'Click to edit this worktree' : undefined,
      w.locked !== null ? (w.locked ? `Locked: ${w.locked}` : 'Locked') : undefined,
      w.prunable !== null ? `Its folder is gone (Prune removes it): ${w.prunable}` : undefined,
    ]
      .filter((l) => l !== undefined)
      .join('\n');
    item.contextValue = `worktree${w.main ? ';main' : ''}${editing ? ';editing' : ''}${w.prunable !== null ? ';prunable' : ''}`;
    if (!editing && w.prunable === null) {
      item.command = { command: 'amonhen.worktree.edit', title: 'Edit This Worktree', arguments: [{ kind: 'worktree', worktree: w }] };
    }
    return item;
  }

  /**
   * タグの行。アイコンは署名の様子（アプリの署名の印と同じ形と色）、説明の頭にリモートでの様子（どのリモートにも
   * ない・同じ名前の別のタグがある）を出し、ツールチップにリモートごとの様子と署名の説明を並べる。右クリックの
   * Push・Replace・Delete from は、出せるリモートがあるときだけ出す（contextValue の `;push` など）
   */
  private tagItem(ref: Ref): vscode.TreeItem {
    const name = shortName(ref.name);
    const item = new vscode.TreeItem(name);
    item.id = ref.name;
    const remotes = this.engine ? this.tagMarks.remotes : undefined;
    const mark = remotes && tagMark(remotes, name);
    item.description = mark ? `${tagMarkLabel(mark)} · ${ref.subject}` : ref.subject;
    const tooltip = new vscode.MarkdownString(undefined, true);
    tooltip.appendMarkdown(`**${escape(name)}**\n\n`);
    tooltip.appendMarkdown(`\`${(ref.peeled ?? ref.oid).slice(0, 8)}\` ${escape(ref.subject)}`);
    const signed = this.engine ? this.tagMarks.signature(name) : undefined;
    if (signed) {
      const status = signatureStatus(signed.signature, signed.taggerEmail ?? '', signed.signed);
      const icon = signatureIcon(status);
      const color = icon.color ? new vscode.ThemeColor(icon.color) : undefined;
      // 確かめ終わるまでは、ふつうのタグのアイコン
      item.iconPath = new vscode.ThemeIcon(status.kind === 'pending' ? 'tag' : icon.icon, color);
      const explanation = escape(signatureExplanation(status, true));
      tooltip.appendMarkdown(`\n\n$(${icon.icon}) **${signatureLabel(status)}** (${signed.signed}) — ${explanation}`);
      const signer = signed.signature?.signer || signed.signature?.key;
      if (signer) tooltip.appendMarkdown(`\n\nSigned by ${escape(signer)}`);
    } else {
      item.iconPath = new vscode.ThemeIcon('tag');
    }
    if (remotes && remotes.remotes.length > 0) {
      tooltip.appendMarkdown(`\n\n${remotes.remotes.map((r) => escape(tagStateText(remotes, name, r.name))).join('  \n')}`);
    }
    item.tooltip = tooltip;
    let context = 'tag';
    if (remotes) {
      const targets = tagTargets(remotes, name);
      if (targets.push.length > 0) context += ';push';
      if (targets.replace.length > 0) context += ';replace';
      if (targets.onRemote.length > 0) context += ';onRemote';
    }
    item.contextValue = context;
    item.command = { command: 'amonhen.showRef', title: 'Show History', arguments: [ref.name] };
    return item;
  }

  dispose(): void {
    this.repoListener?.dispose();
    vscode.Disposable.from(...this.disposables).dispose();
    this._onDidChangeTreeData.dispose();
  }
}

/** `other` はこのブランチを checkout している、書き込む先以外の worktree */
function branchItem(node: Extract<Node, { kind: 'branch' }>, other?: Worktree): vscode.TreeItem {
  const ref = node.ref;
  const item = new vscode.TreeItem(node.label);
  item.id = node.remote ? `remoteBranch:${ref.name}` : ref.name;
  const parts: string[] = [];
  if (ref.ahead) parts.push(`↑${ref.ahead}`);
  if (ref.behind) parts.push(`↓${ref.behind}`);
  if (ref.gone) parts.push('gone');
  // ほかの worktree が checkout している（⌂ はメインの作業ツリー、⑂ は追加した worktree）
  if (other) parts.push(`${other.main ? '⌂' : '⑂'} ${worktreeName(other)}`);
  item.description = parts.join(' ') || undefined;
  item.iconPath = new vscode.ThemeIcon(ref.isHead ? 'check' : 'git-branch');
  const lines = [shortName(ref.name)];
  if (ref.upstream) lines.push(`upstream: ${ref.upstream.replace(/^refs\/remotes\//, '')}`);
  lines.push(`${ref.oid.slice(0, 8)}  ${ref.subject}`);
  if (other) lines.push(`Checked out in the worktree ${other.path}`);
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

/** Markdown の記号を文字のまま出す */
function escape(text: string): string {
  return text.replace(/[\\`*_{}[\]()#+\-.!|<>$]/g, '\\$&');
}
