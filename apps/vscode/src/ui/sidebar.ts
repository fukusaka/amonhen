/**
 * サイドバーのナビ。Working Copy・History・Reflog の入口、途中で止まっている merge / rebase、
 * ブランチ・リモート・タグ・stash・サブモジュールを出す。中身はメインビューに出す。
 */
import { daemonFor } from '../daemon/connection.ts';
import { conflictStates } from '../state/conflicts.ts';
import { sequenceDetail, sequenceTitle, withoutMarkersNote, type SequenceInfo } from '../state/sequenceText.ts';
import * as vscode from 'vscode';
import { sharedIcon } from './sharedIcons.ts';
import type { Sequence } from '../git/backend.ts';
import type { Ref, Remote, Stash } from '../git/refs.ts';
import { splitRemoteRef } from '../git/refs.ts';
import { byName, refTree, type RefTree } from '../git/refTree.ts';
import { arrangeBranches, type Arranged, type BranchGroupsInfo } from '../git/branchGroups.ts';
import { signatureExplanation, signatureIcon, signatureLabel, signatureStatus, type SignatureStatus } from '../git/signature.ts';
import { stashBaseText, stashBranch, stashDescription } from '../git/stash.ts';
import { describeSubmodule } from '../git/status.ts';
import { submoduleMarks, submoduleTooltip } from '../git/submodules.ts';
import { subtreeTooltip, type Subtree } from '../git/subtrees.ts';
import { tagMark, tagMarkLabel, tagStateText, tagTargets } from '../git/tagRemotes.ts';
import { samePath, worktreeLayout, worktreeName, type Worktree } from '../git/worktrees.ts';
import type { RepositoryManager } from '../state/manager.ts';
import { shortName } from '../state/operations.ts';
import type { LazySection, Repository, Submodule } from '../state/repository.ts';
import type { ViewName } from '../view/protocol.ts';
import { BranchFacts } from './branchFacts.ts';
import { StashFacts } from './stashFacts.ts';
import { SubmoduleFacts } from './submoduleFacts.ts';
import { SubtreeFacts } from './subtreeFacts.ts';
import { TagMarks } from './tagMarks.ts';
import { WorktreeList } from './worktreeList.ts';

/**
 * 途中の操作の行の名前（進み具合を除く）。知らせからこの行を指す（知らせは VSCode が 12 秒ほどで通知センターに
 * 移すが、この行にはいつでも Continue・Abort がある）
 */
export function sequenceRowName(sequence: Sequence): string {
  return sequence === 'am' ? 'Applying patches' : `${sequence[0]!.toUpperCase()}${sequence.slice(1)} in progress`;
}

type RefSection = 'branches' | 'remotes' | 'tags' | 'stashes' | 'submodules' | 'subtrees';

interface Folder {
  folders: Map<string, Folder>;
  leaves: { name: string; ref: Ref }[];
}

export type Node =
  | { kind: 'view'; view: ViewName }
  | { kind: 'sequence'; sequence: Sequence; progress?: { current: number; total: number } }
  /** コンフリクトのあるファイル（あるときだけ。押すと最初のファイルをマージエディタで） */
  | { kind: 'conflicts'; count: number }
  | { kind: 'section'; section: RefSection }
  | { kind: 'folder'; id: string; label: string; folder: Folder; remote?: string }
  /**
   * ブランチの行。`stack` はその上に積んだブランチ（入れ子に出す）、`pinned` はお気に入り、`stackedOn` は土台
   * （お気に入りの行の右に書く）
   */
  | { kind: 'branch'; ref: Ref; label: string; remote?: string; stack?: Arranged[]; pinned?: boolean; stackedOn?: string }
  /** 動いているブランチの `/` のフォルダ（中でいちばん新しいブランチの位置に出る） */
  | { kind: 'branchFolder'; id: string; label: string; items: { path: string; arranged: Arranged }[] }
  /** Inactive・Archived（畳んでおく） */
  | { kind: 'branchGroup'; key: 'inactive' | 'archived'; items: Arranged[] }
  | { kind: 'remote'; remote: Remote; folder: Folder }
  /** タグの行。`label` はフォルダの中の名前（なければ名前のまま） */
  | { kind: 'tag'; ref: Ref; label?: string }
  /** タグの `/` のフォルダ（アプリと同じく名前の順で、フォルダは中の最初の名前の位置） */
  | { kind: 'tagFolder'; id: string; label: string; children: RefTree<Ref>[] }
  | { kind: 'stash'; stash: Stash }
  | { kind: 'submodule'; submodule: Submodule; state?: string }
  | { kind: 'subtree'; subtree: Subtree }
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
  subtrees: 'Subtrees',
};

const LAZY: ReadonlySet<RefSection> = new Set<LazySection>(['tags', 'stashes', 'submodules']);

export class SidebarProvider implements vscode.TreeDataProvider<Node>, vscode.Disposable {
  private readonly manager: RepositoryManager;
  private repoListener: vscode.Disposable | undefined;
  private readonly disposables: vscode.Disposable[] = [];
  /** 共有の daemon のエンジンを使えるか（Reflog、タグの印とメニューはエンジンだけ） */
  private engine = false;
  /** Range Diff を開いている（その間だけ行を出す） */
  private rangeDiffShown = false;
  /** タグのリモートでの様子と署名 */
  readonly tagMarks = new TagMarks();
  /** リポジトリの worktree（2 つ以上あるときだけ Working Copy の下に出す） */
  readonly worktrees = new WorktreeList();
  /** ローカルブランチの並べ方のもと（取り込み済み・積んだ土台・お気に入り・アーカイブ） */
  readonly branchFacts = new BranchFacts();
  /** stash の土台と untracked（エンジンだけ） */
  readonly stashFacts = new StashFacts();
  readonly submoduleFacts = new SubmoduleFacts();
  readonly subtreeFacts = new SubtreeFacts();
  /** Branches・Remotes・Tags の絞り込みの言葉（小文字。どれも名前に含むものだけ出す。空なら絞り込まない） */
  private filterWords: string[] = [];
  private readonly _onDidChangeFilter = new vscode.EventEmitter<string>();
  /** 絞り込みを変えた（空文字なら絞り込みをやめた） */
  readonly onDidChangeFilter = this._onDidChangeFilter.event;
  /** 開いている遅延セクション */
  private readonly expanded = new Set<LazySection>();
  /** 途中の merge・rebase などの中身（コアの `sequence`。エンジンだけ）。行の題と説明をアプリの帯と同じにする */
  private sequence: { root: string; info: SequenceInfo } | undefined;
  /** マーカーの残っていないコンフリクトの数（途中の行に添え、まとめて解決済みにするボタンを出す） */
  private withoutMarkers: { root: string; count: number } | undefined;
  /** sparse checkout のリポジトリ（Working Copy の行に印を付ける） */
  private readonly sparse = new Set<string>();
  /** 最後に出した節（変われば、いちばん上から描き直す） */
  private lastShown = '';
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
      // コンフリクトのファイルを保存したら、マーカーの残っていない数を数え直す（status は UU のまま変わらない）
      vscode.workspace.onDidSaveTextDocument(() => {
        const repo = this.repo;
        if (repo?.current.sequence && repo.current.status?.entries.some((e) => e.kind === 'unmerged')) void this.countWithoutMarkers(repo);
      }),
      this.tagMarks,
      this.tagMarks.onDidChange(() => this._onDidChangeTreeData.fire(this.sections.get('tags'))),
      this.worktrees,
      // Working Copy の行（子が変わる）とブランチの印
      this.worktrees.onDidChange(() => this._onDidChangeTreeData.fire(undefined)),
      this.branchFacts,
      this.stashFacts,
      this.stashFacts.onDidChange(() => this._onDidChangeTreeData.fire(this.sections.get('stashes'))),
      this.submoduleFacts,
      this.submoduleFacts.onDidChange(() => this._onDidChangeTreeData.fire(this.sections.get('submodules'))),
      this.subtreeFacts,
      // Subtrees は git subtree があるときだけ出すので、いちばん上から描き直す
      this.subtreeFacts.onDidChange(() => this._onDidChangeTreeData.fire(undefined)),
      this.branchFacts.onDidChange(() => this._onDidChangeTreeData.fire(this.sections.get('branches'))),
    );
    this.watch(manager.current);
  }

  private get repo(): Repository | undefined {
    return this.manager.current;
  }

  /** 途中の操作の中身を読む（エンジンがあって、何かの途中のときだけ）。変われば描き直す */
  private async loadSequence(repository: Repository): Promise<void> {
    const kind = repository.current.sequence;
    if (!this.engine || !kind) {
      if (this.sequence) {
        this.sequence = undefined;
        this._onDidChangeTreeData.fire(undefined);
      }
      return;
    }
    try {
      const client = await daemonFor(repository.root);
      if (!client) return;
      const reply = (await client.call({ op: 'run', command: { op: 'sequence' } })).json;
      const info = reply['info'] as SequenceInfo | undefined;
      if (repository !== this.repo) return;
      const next = info ? { root: repository.root, info } : undefined;
      if (JSON.stringify(next) === JSON.stringify(this.sequence)) return;
      this.sequence = next;
      this._onDidChangeTreeData.fire(undefined);
    } catch {
      // 古いエンジン。これまでの言い方のまま
    }
  }

  private watch(repository: Repository | undefined): void {
    this.repoListener?.dispose();
    // 開いたままの遅延セクションは、切り替えた先のリポジトリでも読む（開き直すまで空のままにしない）
    for (const section of this.expanded) void repository?.want(section);
    this.updateContext(repository);
    this.tagMarks.setRepository(this.engine ? repository : undefined);
    this.worktrees.setRepository(this.engine ? repository : undefined);
    this.branchFacts.setRepository(this.engine ? repository : undefined);
    this.stashFacts.setRepository(this.engine ? repository : undefined);
    this.submoduleFacts.setRepository(this.engine ? repository : undefined);
    this.subtreeFacts.setRepository(this.engine ? repository : undefined);
    if (repository) void this.checkSparse(repository);
    if (repository) void this.loadSequence(repository);
    if (repository) void this.countWithoutMarkers(repository);
    this.repoListener = repository?.onDidChange((changed) => {
      if (changed.has('workingCopy')) void this.loadSequence(repository);
      if (changed.has('workingCopy')) void this.countWithoutMarkers(repository);
      if (changed.has('branches') || changed.has('remotes') || changed.has('workingCopy')) this.updateContext(repository);
      // sparse checkout にしたかは設定（core.sparseCheckout。worktree の config.worktree のこともある）に出る
      if (changed.has('config') || changed.has('workingCopy')) void this.checkSparse(repository);
      // Working Copy の変化は、入口の数と途中の merge / rebase に出る
      // Stashes の節は、stash があるかで出たり消えたりする
      if (changed.has('workingCopy') || changed.has('stashes')) {
        this._onDidChangeTreeData.fire(undefined);
        return;
      }
      // 節は中身があるときだけ出すので、出す節が変われば、いちばん上から描き直す
      if (this.shownKey(repository) !== this.lastShown) {
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

  /**
   * コンフリクトのファイルのうち、マーカーの残っていないものを数える（作業ツリーのファイルを読むだけ。エンジンがなくても）。
   * 変わっていれば描き直す。status が同じままでも、ファイルを保存したら数え直す
   */
  async countWithoutMarkers(repository: Repository): Promise<void> {
    const status = repository.current.status;
    const unmerged = status?.entries.some((e) => e.kind === 'unmerged') ?? false;
    const states = status && unmerged ? await conflictStates(repository.root, repository.backend.dirs.gitDir, status.entries).catch(() => undefined) : undefined;
    if (repository !== this.repo) return;
    const count = states ? [...states.values()].filter((state) => state.markers === 0).length : 0;
    const next = count > 0 ? { root: repository.root, count } : undefined;
    if (next?.root === this.withoutMarkers?.root && next?.count === this.withoutMarkers?.count) return;
    this.withoutMarkers = next;
    if (repository.current.sequence) this._onDidChangeTreeData.fire(undefined);
  }

  /** sparse checkout かを読み、変わっていれば Working Copy の行を描き直す */
  private async checkSparse(repository: Repository): Promise<void> {
    const value = await repository.backend.config('core.sparseCheckout').catch(() => undefined);
    const on = ['true', 'yes', 'on', '1'].includes((value ?? '').trim().toLowerCase());
    if (on === this.sparse.has(repository.root)) return;
    if (on) this.sparse.add(repository.root);
    else this.sparse.delete(repository.root);
    if (repository === this.repo) this._onDidChangeTreeData.fire(undefined);
  }

  /** 絞り込みの文字列（空白で区切った言葉。大文字小文字は区別しない） */
  get filter(): string {
    return this.filterWords.join(' ');
  }

  setFilter(text: string): void {
    this.filterWords = text.toLowerCase().split(/\s+/).filter((w) => w);
    void vscode.commands.executeCommand('setContext', 'amonhen.refsFiltered', this.filterWords.length > 0);
    // タグは開くまで読まないので、絞り込むなら読む
    if (this.filterWords.length > 0) void this.repo?.want('tags');
    this._onDidChangeFilter.fire(this.filter);
    // 見出しの id が変わる（開いて出す）ので、全体を描き直す
    this._onDidChangeTreeData.fire(undefined);
  }

  private matches(name: string): boolean {
    const lower = name.toLowerCase();
    return this.filterWords.every((w) => lower.includes(w));
  }

  /** 開くための親（いちばん上の行しか開かないので、どれも undefined） */
  getParent(): Node | undefined {
    return undefined;
  }

  /** Range Diff を開いた・閉じた */
  setRangeDiffShown(shown: boolean): void {
    if (shown !== this.rangeDiffShown) {
      this.rangeDiffShown = shown;
      this._onDidChangeTreeData.fire(undefined);
    }
  }

  /** エンジンを使えるかが分かった・変わった */
  setEngine(engine: boolean): void {
    if (engine !== this.engine) {
      this.engine = engine;
      this.tagMarks.setRepository(engine ? this.repo : undefined);
      this.worktrees.setRepository(engine ? this.repo : undefined);
      this.branchFacts.setRepository(engine ? this.repo : undefined);
      this.stashFacts.setRepository(engine ? this.repo : undefined);
      this.submoduleFacts.setRepository(engine ? this.repo : undefined);
      this.subtreeFacts.setRepository(engine ? this.repo : undefined);
      this._onDidChangeTreeData.fire(undefined);
    }
  }

  /**
   * 今のブランチに upstream が無ければ、ナビの上の Push を Publish にする。リモートがなければ Publish の先がないので、
   * ブランチの行とコマンドパレットの Publish を出さない（`amonhen.hasRemote`）。途中の操作（merge・rebase など）の
   * 種類は `amonhen.sequence` に置き、Continue・Abort・Skip はその間だけ、Pull・Merge…・Rebase…・Apply Patch… は
   * その間は出さない（アプリのメニューと同じ）
   */
  private updateContext(repository: Repository | undefined): void {
    const head = repository?.current.branches?.find((b) => b.isHead);
    void vscode.commands.executeCommand('setContext', 'amonhen.unpublished', !!head && (!head.upstream || !!head.gone));
    void vscode.commands.executeCommand('setContext', 'amonhen.hasRemote', (repository?.current.remotes ?? []).length > 0);
    void vscode.commands.executeCommand('setContext', 'amonhen.sequence', repository?.current.sequence ?? '');
  }

  /** 遅延セクションが展開されたら取得を始める */
  onDidExpand(node: Node): void {
    if (node.kind === 'section' && LAZY.has(node.section)) {
      this.expanded.add(node.section as LazySection);
      void this.repo?.want(node.section as LazySection);
    }
  }

  onDidCollapse(node: Node): void {
    if (node.kind === 'section') this.expanded.delete(node.section as LazySection);
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
      // Conflicts はコンフリクトがあるときだけ、数を添えて（アプリと同じく History の下）
      const conflicts = (state.status?.entries ?? []).filter((e) => e.kind === 'unmerged').length;
      if (conflicts > 0) nodes.push({ kind: 'conflicts', count: conflicts });
      if (state.sequence) nodes.push({ kind: 'sequence', sequence: state.sequence, progress: state.progress });
      const shown = this.shownSections(repo);
      this.lastShown = shown.join(',');
      nodes.push(...shown.map((s) => this.sections.get(s)!));
      // Range Diff は開いている間だけ、Reflog はいつもいちばん下（アプリと同じ）
      if (this.rangeDiffShown) nodes.push({ kind: 'view', view: 'rangeDiff' });
      if (this.engine) nodes.push({ kind: 'view', view: 'reflog' }, { kind: 'view', view: 'settings' });
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
      case 'branch':
        return (node.stack ?? []).map((a) => this.arrangedNode(repo, a, a.name)).filter((n) => n !== undefined);
      case 'branchFolder':
        return arrangedFolders(node.items, node.id, (a, label) => this.arrangedNode(repo, a, label));
      case 'branchGroup':
        return node.items.map((a) => this.arrangedNode(repo, a, a.name)).filter((n) => n !== undefined);
      case 'tagFolder':
        return tagNodes(node.children);
      default:
        return [];
    }
  }

  /**
   * 出す節。アプリと同じく、中身のないものは出さない。絞り込み中は、合うものがある Branches・Remotes・Tags と
   * Stashes だけ（Submodules・Subtrees は絞り込まない）
   */
  private shownSections(repo: Repository): RefSection[] {
    const state = repo.current;
    // Stashes・Submodules は中身があるかを知るために読んでおく（git stash list と .gitmodules は軽い）。status が
    // stash の数を言うのは git 2.35 から
    if (!state.stashes) void repo.want('stashes');
    if (!state.submodules) void repo.want('submodules');
    const filtering = this.filterWords.length > 0;
    const named = (refs: readonly Ref[] | undefined) =>
      filtering ? (refs ?? []).some((r) => this.matches(shortName(r.name))) : (refs?.length ?? 0) > 0;
    const shown: Record<RefSection, boolean> = {
      branches: named(state.branches),
      remotes: named(remoteRefs(state.remoteBranches, state.remotes).map((r) => r.ref)),
      // タグの一覧は開くまで読まない（絞り込めば読む）ので、それまでは 1 つでもあるかで決める
      tags: state.tags ? named(state.tags) : !filtering && !!state.hasTags,
      stashes: Math.max(state.status?.stashCount ?? 0, state.stashes?.length ?? 0) > 0,
      submodules: !filtering && (state.submodules?.length ?? 0) > 0,
      // Subtrees はこの機械の git に git subtree があるときだけ（アプリと同じ。エンジンで調べる）
      subtrees: !filtering && this.engine && this.subtreeFacts.available,
    };
    return (Object.keys(SECTION_LABELS) as RefSection[]).filter((s) => shown[s]);
  }

  private shownKey(repo: Repository): string {
    return this.shownSections(repo).join(',');
  }

  private sectionChildren(repo: Repository, section: RefSection): Node[] {
    const state = repo.current;
    switch (section) {
      case 'branches':
        // 絞り込んでいれば、合うものだけを平らに（フォルダや分け方に隠さない）
        if (this.filterWords.length > 0) {
          return (state.branches ?? [])
            .filter((r) => this.matches(shortName(r.name)))
            .sort((a, b) => byName(a.name, b.name))
            .map((ref) => ({ kind: 'branch', ref, label: shortName(ref.name) }));
        }
        return this.branchChildren(repo);
      case 'remotes': {
        // アプリと同じく、リモートのブランチから組む（ブランチのないリモートは出さない。名前・URL などは Repository
        // Settings の Remotes で）。設定のないリモートの名前の下の ref（Fetch from Bundle が取り込んだものなど）も、
        // その名前で並べる
        const refs = remoteRefs(state.remoteBranches, state.remotes);
        // 絞り込んでいれば、合うものだけを名前のまま（origin/main）平らに
        if (this.filterWords.length > 0) {
          return refs
            .filter(({ ref }) => this.matches(shortName(ref.name)))
            .sort((a, b) => byName(a.ref.name, b.ref.name))
            .map(({ ref, remote }) => ({ kind: 'branch', ref, label: shortName(ref.name), remote }));
        }
        const configured = new Map((state.remotes ?? []).map((r) => [r.name, r]));
        const byRemote = new Map<string, Ref[]>();
        for (const { ref, remote } of refs) {
          if (!byRemote.has(remote)) byRemote.set(remote, []);
          byRemote.get(remote)!.push(ref);
        }
        return [...byRemote.keys()]
          .sort((a, b) => Number(!configured.has(a)) - Number(!configured.has(b)) || byName(a, b))
          .map((name) => ({
            kind: 'remote',
            remote: configured.get(name) ?? { name, unconfigured: true },
            folder: buildFolder(byRemote.get(name)!, (r) => r.name.slice(`refs/remotes/${name}/`.length)),
          }));
      }
      case 'tags': {
        if (this.engine) this.tagMarks.want();
        const tags = state.tags ?? [];
        // 絞り込んでいれば、合うものだけを名前のまま平らに
        if (this.filterWords.length > 0) {
          return tags
            .filter((t) => this.matches(shortName(t.name)))
            .sort((a, b) => byName(a.name, b.name))
            .map((ref) => ({ kind: 'tag', ref }));
        }
        // release/1.0 のような名前はフォルダにまとめる（アプリと同じく名前の順）
        return tagNodes(refTree(tags.map((ref) => ({ name: shortName(ref.name), value: ref }))));
      }
      case 'stashes':
        return (state.stashes ?? []).map((stash) => ({ kind: 'stash', stash }));
      case 'subtrees':
        return this.subtreeFacts.subtrees.map((subtree) => ({ kind: 'subtree', subtree }));
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
          item.iconPath = sharedIcon('working-copy') ?? new vscode.ThemeIcon('diff');
          // sparse checkout なら、一部のフォルダだけだと添える（アプリの Working Copy の行のフォルダの印）
          if (repo && this.sparse.has(repo.root)) {
            item.description += ' · some folders';
            item.tooltip = 'Only some folders are checked out (sparse checkout). Repository Settings > Fetching and Folders changes them.';
          }
          item.command = { command: 'amonhen.showWorkingCopy', title: 'Show Working Copy' };
          item.contextValue = 'view.workingCopy';
          // worktree が 2 つ以上あれば、その並びを下に出す（書き込む先は ✎）
          if (repo && this.worktreeLayout(repo).shown.length > 0) {
            item.collapsibleState = vscode.TreeItemCollapsibleState.Expanded;
            item.tooltip = `Working Copy of ✎ ${repo.root}`;
          }
          return item;
        }
        if (node.view === 'settings') {
          const item = new vscode.TreeItem('Repository Settings');
          item.id = 'view:settings';
          item.tooltip = 'This repository’s git settings (author, operations, remotes…)';
          item.iconPath = sharedIcon('settings') ?? new vscode.ThemeIcon('settings-gear');
          item.command = { command: 'amonhen.showRepositorySettings', title: 'Show Repository Settings' };
          return item;
        }
        if (node.view === 'rangeDiff') {
          const item = new vscode.TreeItem('Range Diff');
          item.id = 'view:rangeDiff';
          item.tooltip = 'Two versions of the same commits, compared (git range-diff)';
          item.iconPath = sharedIcon('range-diff') ?? new vscode.ThemeIcon('diff-multiple');
          item.command = { command: 'amonhen.showRangeDiff', title: 'Show Range Diff' };
          item.contextValue = 'view.rangeDiff';
          return item;
        }
        if (node.view === 'reflog') {
          const item = new vscode.TreeItem('Reflog');
          item.id = 'view:reflog';
          item.tooltip = 'Every place HEAD has been, including commits no branch reaches any more';
          item.iconPath = sharedIcon('reflog') ?? new vscode.ThemeIcon('list-ordered');
          item.command = { command: 'amonhen.showReflog', title: 'Show Reflog' };
          return item;
        }
        const item = new vscode.TreeItem('History');
        item.id = 'view:history';
        item.description = state?.status?.branch.head ?? 'detached HEAD';
        item.iconPath = sharedIcon('history') ?? new vscode.ThemeIcon('history');
        item.command = { command: 'amonhen.showHistory', title: 'Show History' };
        return item;
      }
      case 'conflicts': {
        const item = new vscode.TreeItem('Conflicts');
        item.id = 'view:conflicts';
        item.description = String(node.count);
        item.tooltip = `${node.count} ${node.count === 1 ? 'file has' : 'files have'} conflicts. Click to open the first in the merge editor.`;
        item.iconPath = sharedIcon('conflict', 'warning') ?? new vscode.ThemeIcon('warning', new vscode.ThemeColor('list.warningForeground'));
        item.command = { command: 'amonhen.openFirstConflict', title: 'Open the First Conflict' };
        item.contextValue = 'view.conflicts';
        return item;
      }
      case 'sequence': {
        const step = node.progress ? ` (${node.progress.current}/${node.progress.total})` : '';
        const am = node.sequence === 'am' ? state?.am : undefined;
        const conflicts = (state?.status?.entries ?? []).filter((e) => e.kind === 'unmerged').length;
        // マーカーの残っていないコンフリクト（rerere やほかのエディタで解決した）。数を添え、まとめて解決済みにできる
        const withoutMarkers = conflicts > 0 && this.withoutMarkers?.root === this.repo?.root ? (this.withoutMarkers?.count ?? 0) : 0;
        // エンジンがあれば、アプリの帯と同じ: 何をどこへしている途中か（何手目か）と、どこで・なぜ止まり、次に何をするか
        const info = this.sequence && this.sequence.root === this.repo?.root && this.sequence.info.kind === node.sequence ? this.sequence.info : undefined;
        const item = new vscode.TreeItem(info ? sequenceTitle(info) : `${sequenceRowName(node.sequence)}${step}`);
        item.id = 'sequence';
        item.description = info
          ? sequenceDetail(info, conflicts, withoutMarkers)
          : `Resolve and commit, then Continue${withoutMarkersNote(withoutMarkers)}`;
        if (info) item.tooltip = `${sequenceTitle(info)}\n${sequenceDetail(info, conflicts, withoutMarkers)}`;
        item.iconPath = new vscode.ThemeIcon('debug-pause', new vscode.ThemeColor('notificationsWarningIcon.foreground'));
        item.contextValue = `sequence.${node.sequence}`;
        if (am) {
          // 止まったパッチと、なぜ止まったか（当たらない・空・コンフリクト）。エンジンの中身があれば、その言い方を残す
          const shown = { description: item.description, tooltip: item.tooltip };
          const conflicts = state?.status?.entries.some((e) => e.kind === 'unmerged') ?? false;
          const subject = am.subject ? `“${am.subject}”` : 'The patch';
          item.tooltip = am.subject;
          if (am.empty) {
            item.description = `${subject} has no changes. Skip it.`;
          } else if (conflicts || am.retried) {
            item.description = `Resolve and stage the conflicts in ${subject}, then Continue`;
          } else if (am.threeWay) {
            item.description = `${subject} doesn’t apply. Apply it by hand and Continue, or Skip`;
          } else {
            item.description = `${subject} doesn’t apply. Retry with a 3-way merge, apply it by hand and Continue, or Skip`;
            item.contextValue = 'sequence.am.retry';
          }
          if (info) Object.assign(item, shown);
        }
        // Mark N Without Markers Resolved（アプリの帯のボタン）。ボタンの題は決まっているので、数はツールチップに
        if (withoutMarkers > 0) {
          item.contextValue += ';unmarked';
          item.tooltip = `${typeof item.tooltip === 'string' ? `${item.tooltip}\n` : ''}${withoutMarkers} without conflict markers left (rerere or another editor resolved them)`;
        }
        item.command = { command: 'amonhen.showWorkingCopy', title: 'Show Working Copy' };
        return item;
      }
      case 'section': {
        // 絞り込んでいる間は Branches・Remotes・Tags を開いて出す（id を変えると、畳んでいたものも開く）
        const filtered = this.filterWords.length > 0 && ['branches', 'remotes', 'tags'].includes(node.section);
        const item = new vscode.TreeItem(
          SECTION_LABELS[node.section],
          node.section === 'branches' || filtered
            ? vscode.TreeItemCollapsibleState.Expanded
            : vscode.TreeItemCollapsibleState.Collapsed,
        );
        item.id = `section:${node.section}${filtered ? ':filtered' : ''}`;
        item.contextValue = `section.${node.section}`;
        if (node.section === 'stashes' && state?.status?.stashCount) {
          item.description = String(state.status.stashCount);
        }
        return item;
      }
      case 'folder':
      case 'branchFolder':
      case 'tagFolder': {
        const item = new vscode.TreeItem(node.label, vscode.TreeItemCollapsibleState.Collapsed);
        item.id = node.id;
        item.iconPath = vscode.ThemeIcon.Folder;
        return item;
      }
      case 'branchGroup': {
        const item = new vscode.TreeItem(node.key === 'inactive' ? 'Inactive' : 'Archived', vscode.TreeItemCollapsibleState.Collapsed);
        item.id = `branches:${node.key}`;
        item.description = String(node.items.length);
        item.tooltip =
          node.key === 'inactive'
            ? 'Branches with no new commit for 90 days'
            : 'Branches you archived from their menu. Archiving is kept in git’s config, so it follows a rename.';
        item.iconPath = new vscode.ThemeIcon(node.key === 'inactive' ? 'history' : 'archive');
        return item;
      }
      case 'branch': {
        const other =
          !node.remote && repo && !node.ref.isHead
            ? this.worktrees.checkedOutElsewhere(shortName(node.ref.name), repo.root)
            : undefined;
        const facts = node.remote ? undefined : this.branchFacts.info;
        // worktree が 2 つ以上なら、今のブランチ（書き込む先のもの）に ✎ を添える
        const editing = !node.remote && node.ref.isHead && this.engine && this.worktrees.worktrees.length > 1;
        return branchItem(node, other, facts && branchFlags(facts, shortName(node.ref.name)), editing);
      }
      case 'worktree':
        return this.worktreeItem(node.worktree);
      case 'cleanWorktrees': {
        const item = new vscode.TreeItem('No changes', vscode.TreeItemCollapsibleState.Collapsed);
        item.id = 'worktrees:clean';
        item.description = String(node.worktrees.length);
        item.tooltip = 'Worktrees without changes';
        item.iconPath = sharedIcon('clean') ?? new vscode.ThemeIcon('check-all');
        return item;
      }
      case 'remote': {
        // 絞り込み中はリモートの行を出さない（合うブランチを Remotes の下に平らに並べる）
        const item = new vscode.TreeItem(node.remote.name, vscode.TreeItemCollapsibleState.Collapsed);
        item.id = `remote:${node.remote.name}`;
        item.description = node.remote.unconfigured ? 'not a configured remote' : node.remote.fetchUrl;
        item.iconPath = sharedIcon(node.remote.unconfigured ? 'folder' : 'remote') ?? new vscode.ThemeIcon(node.remote.unconfigured ? 'folder' : 'cloud');
        // 設定のないものには、リモートの操作（fetch・名前・URL・削除）を出さない
        item.contextValue = node.remote.unconfigured ? 'remote.unconfigured' : 'remote';
        return item;
      }
      case 'tag':
        return this.tagItem(node.ref, node.label);
      case 'stash': {
        const item = new vscode.TreeItem(node.stash.name);
        item.id = `stash:${node.stash.oid}`;
        // アプリの Stashes の行と同じく、作ったときのブランチ・土台と HEAD の関係・いつ・untracked・土台の短い ID を添える
        const fact = this.engine ? this.stashFacts.get(node.stash.oid) : undefined;
        const branch = stashBranch(node.stash.subject);
        const moved = fact && stashBaseText(fact.ahead, fact.behind);
        item.description = stashDescription(node.stash, fact);
        item.tooltip = [
          node.stash.name,
          node.stash.subject,
          branch ? `Made on ${branch}` : undefined,
          fact?.base ? `Base: ${fact.base.slice(0, 8)}${moved ? ` (${moved})` : ' (HEAD is still there)'}` : undefined,
          fact?.untracked ? 'Includes untracked files' : undefined,
        ]
          .filter(Boolean)
          .join('\n');
        item.iconPath = sharedIcon('stashes') ?? new vscode.ThemeIcon('archive');
        item.contextValue = 'stash';
        item.command = { command: 'amonhen.stash.show', title: 'Show Stash', arguments: [node] };
        return item;
      }
      case 'submodule': {
        const item = new vscode.TreeItem(node.submodule.path);
        item.id = `submodule:${node.submodule.path}`;
        // エンジンがあれば、アプリと同じ印（取り出していない・違うコミット・中の変更・コンフリクト）と URL・追うブランチ
        const fact = this.engine ? this.submoduleFacts.get(node.submodule.path) : undefined;
        if (fact) {
          item.description = submoduleMarks(fact).join(' · ') || undefined;
          item.tooltip = submoduleTooltip(fact);
        } else {
          item.description = node.submodule.initialized ? node.state : 'not initialized';
        }
        const off = fact ? !fact.initialized : !node.submodule.initialized;
        item.iconPath =
          sharedIcon(fact?.conflict ? 'conflict' : off ? 'not-checked-out' : 'submodule') ??
          new vscode.ThemeIcon(
            fact?.conflict ? 'warning' : 'file-submodule',
            off ? new vscode.ThemeColor('disabledForeground') : undefined,
          );
        // `;checkedOut` は取り出してある（Open in New Window はそのときだけ）
        item.contextValue = `submodule${fact?.ownGitDir ? ';ownGitDir' : ''}${off ? '' : ';checkedOut'}`;
        return item;
      }
      case 'subtree': {
        const item = new vscode.TreeItem(node.subtree.dir);
        item.id = `subtree:${node.subtree.dir}`;
        item.description = node.subtree.squashed ? 'squashed' : undefined;
        item.tooltip = subtreeTooltip(node.subtree);
        item.iconPath = sharedIcon('subtree') ?? new vscode.ThemeIcon('folder-library');
        item.contextValue = 'subtree';
        return item;
      }
    }
  }

  /**
   * Branches の中身。エンジンで分け方が分かっていれば、アプリと同じく上から お気に入り（★）・今のブランチ（その
   * 積み重ねの根元ごと）と既定のブランチ・動いているブランチ（新しい順、`/` はフォルダ）・Inactive・Archived。
   * 分からなければ名前のフォルダだけ
   */
  private branchChildren(repo: Repository): Node[] {
    const branches = repo.current.branches ?? [];
    const info = this.engine ? this.branchFacts.info : undefined;
    if (!info) return folderChildren(buildFolder(branches, (r) => shortName(r.name)), 'branches');
    const head = branches.find((b) => b.isHead);
    const a = arrangeBranches(
      branches.map((b) => shortName(b.name)),
      head && shortName(head.name),
      info,
      Math.floor(Date.now() / 1000),
      (name) => !!this.worktrees.checkedOutElsewhere(name, repo.root),
    );
    const row = (x: Arranged, pinned = false) => this.arrangedNode(repo, x, x.name, pinned);
    const nodes: (Node | undefined)[] = [
      ...a.pinned.map((x) => row(x, true)),
      ...a.top.map((x) => row(x)),
      ...arrangedFolders(
        a.active.map((arranged) => ({ path: arranged.name, arranged })),
        'branches',
        (x, label) => this.arrangedNode(repo, x, label),
      ),
    ];
    if (a.inactive.length > 0) nodes.push({ kind: 'branchGroup', key: 'inactive', items: a.inactive });
    if (a.archived.length > 0) nodes.push({ kind: 'branchGroup', key: 'archived', items: a.archived });
    return nodes.filter((n) => n !== undefined);
  }

  private arrangedNode(repo: Repository, a: Arranged, label: string, pinned = false): Node | undefined {
    const ref = repo.current.branches?.find((b) => b.name === `refs/heads/${a.name}`);
    if (!ref) return undefined;
    return {
      kind: 'branch',
      ref,
      label,
      ...(a.stack.length > 0 ? { stack: a.stack } : {}),
      ...(pinned ? { pinned } : {}),
      ...(a.stackedOn ? { stackedOn: a.stackedOn } : {}),
    };
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
        ? (sharedIcon('conflict', 'warning') ?? new vscode.ThemeIcon('warning', new vscode.ThemeColor('list.warningForeground')))
        : editing
          ? (sharedIcon('edited-worktree') ?? new vscode.ThemeIcon('edit'))
          : w.main
            ? (sharedIcon('main-worktree') ?? new vscode.ThemeIcon('home'))
            : (sharedIcon('branch', 'green') ?? new vscode.ThemeIcon('repo-forked', new vscode.ThemeColor('charts.green')));
    item.tooltip = [
      w.path,
      w.branch ? `Branch: ${w.branch}` : 'detached HEAD',
      editing ? 'Editing this worktree' : w.prunable === null ? 'Click to edit this worktree' : undefined,
      w.locked !== null ? (w.locked ? `Locked: ${w.locked}` : 'Locked') : undefined,
      w.prunable !== null ? `Its folder is gone (Prune removes it): ${w.prunable}` : undefined,
    ]
      .filter((l) => l !== undefined)
      .join('\n');
    item.contextValue = `worktree${w.main ? ';main' : ''}${editing ? ';editing' : ''}${w.prunable !== null ? ';prunable' : ''}${w.locked !== null ? ';locked' : ''}`;
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
  private tagItem(ref: Ref, label?: string): vscode.TreeItem {
    const name = shortName(ref.name);
    const item = new vscode.TreeItem(label ?? name);
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
      const shared = status.kind === 'pending' ? sharedIcon('tag') : signatureSharedIcon(status);
      item.iconPath = shared ?? new vscode.ThemeIcon(status.kind === 'pending' ? 'tag' : icon.icon, color);
      const explanation = escape(signatureExplanation(status, true));
      tooltip.appendMarkdown(`\n\n$(${icon.icon}) **${signatureLabel(status)}** (${signed.signed}) — ${explanation}`);
      const signer = signed.signature?.signer || signed.signature?.key;
      if (signer) tooltip.appendMarkdown(`\n\nSigned by ${escape(signer)}`);
    } else {
      item.iconPath = sharedIcon('tag') ?? new vscode.ThemeIcon('tag');
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
    this._onDidChangeFilter.dispose();
  }
}

/** 分け方から分かる、行の印とメニューの出し分け */
interface BranchFlags {
  merged: boolean;
  /** プルリクエストで取り込んだ（squash・rebase）。`merged` も立つ */
  pullRequest?: number;
  archived: boolean;
}

function branchFlags(info: BranchGroupsInfo, name: string): BranchFlags | undefined {
  const f = info.branches.find((b) => b.name === name);
  if (!f) return undefined;
  // 既定のブランチと、その先端と同じだけのブランチは、取り込み済みと言わない
  const pullRequest = name !== info.base ? (f.pullRequest ?? undefined) : undefined;
  return {
    merged: (f.merged && !f.atBase && name !== info.base) || pullRequest !== undefined,
    ...(pullRequest !== undefined ? { pullRequest } : {}),
    archived: f.archived,
  };
}

/** `other` はこのブランチを checkout している、書き込む先以外の worktree */
function branchItem(node: Extract<Node, { kind: 'branch' }>, other?: Worktree, flags?: BranchFlags, editing = false): vscode.TreeItem {
  const ref = node.ref;
  const item = new vscode.TreeItem(
    node.label,
    node.stack?.length ? vscode.TreeItemCollapsibleState.Expanded : vscode.TreeItemCollapsibleState.None,
  );
  item.id = node.remote ? `remoteBranch:${ref.name}` : ref.name;
  // 今のブランチは取り込み済みと言わない（アプリと同じ）
  const merged = !!flags?.merged && !ref.isHead;
  const parts: string[] = [];
  if (other) {
    // ほかの worktree が checkout している（git はここでは checkout させない）。アプリと同じく、行には印だけを出し
    // （⌂ はメインの作業ツリー、⑂ は追加した worktree）、worktree の名前はツールチップに（ブランチ名を隠さない）
    parts.push(other.main ? '⌂' : '⑂');
  } else {
    if (editing) parts.push('✎');
    if (ref.ahead) parts.push(`↑${ref.ahead}`);
    if (ref.behind) parts.push(`↓${ref.behind}`);
    if (ref.gone) parts.push('gone');
    if (merged) parts.push('merged');
    // お気に入りは積んだブランチでも平らに出すので、土台を書く
    if (node.pinned && node.stackedOn) parts.push(`↳ ${node.stackedOn}`);
  }
  item.description = parts.join(' ') || undefined;
  // どの FE でも同じ形のアイコン（今のブランチ・お気に入り・ブランチ。ほかの worktree が checkout していれば緑）。
  const elsewhere = !!other && !other.main;
  item.iconPath =
    sharedIcon(ref.isHead ? 'current-branch' : node.pinned ? 'favorite' : 'branch', elsewhere && 'green') ??
    new vscode.ThemeIcon(
      ref.isHead ? 'check' : node.pinned ? 'star-full' : 'git-branch',
      elsewhere ? new vscode.ThemeColor('charts.green') : undefined,
    );
  const lines = [shortName(ref.name)];
  if (ref.upstream) lines.push(`upstream: ${ref.upstream.replace(/^refs\/remotes\//, '')}`);
  lines.push(`${ref.oid.slice(0, 8)}  ${ref.subject}`);
  if (other) lines.push(`Checked out in the worktree “${worktreeName(other)}” (${other.path})`);
  if (merged) lines.push(flags!.pullRequest !== undefined ? `Merged by pull request #${flags!.pullRequest}` : 'Merged into the default branch');
  item.tooltip = lines.join('\n');
  // `;elsewhere` はほかの worktree が checkout している（Checkout・Fast-Forward の代わりに Open Worktree）
  item.contextValue = node.remote
    ? 'remoteBranch'
    : (ref.isHead ? 'branch.current' : 'branch') +
      syncState(ref) +
      (node.pinned ? ';pinned' : '') +
      (flags?.archived ? ';archived' : '') +
      (other ? ';elsewhere' : '');
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

/**
 * 並べた順（新しい順）の `items` を、名前の `/` でフォルダに分ける。フォルダは中でいちばん新しいブランチの位置に出る。
 * `node` は行（ブランチがなければ undefined）
 */
function arrangedFolders(
  items: readonly { path: string; arranged: Arranged }[],
  id: string,
  node: (a: Arranged, label: string) => Node | undefined,
): Node[] {
  const out: (Node | { pending: string; items: { path: string; arranged: Arranged }[] })[] = [];
  const folders = new Map<string, { pending: string; items: { path: string; arranged: Arranged }[] }>();
  for (const item of items) {
    const slash = item.path.indexOf('/');
    if (slash < 0) {
      const n = node(item.arranged, item.path);
      if (n) out.push(n);
      continue;
    }
    const name = item.path.slice(0, slash);
    let folder = folders.get(name);
    if (!folder) {
      folder = { pending: name, items: [] };
      folders.set(name, folder);
      out.push(folder);
    }
    folder.items.push({ path: item.path.slice(slash + 1), arranged: item.arranged });
  }
  return out.map((o): Node =>
    'pending' in o ? { kind: 'branchFolder', id: `${id}/${o.pending}`, label: o.pending, items: o.items } : o,
  );
}

/** 並べるリモートのブランチと、そのリモート（`origin/HEAD` は出さない） */
function remoteRefs(refs: readonly Ref[] | undefined, remotes: readonly Remote[] | undefined): { ref: Ref; remote: string }[] {
  const names = (remotes ?? []).map((r) => r.name);
  return (refs ?? []).flatMap((ref) => {
    const split = ref.symref ? undefined : splitRemoteRef(ref.name, names);
    return split ? [{ ref, remote: split.remote }] : [];
  });
}

function tagNodes(tree: readonly RefTree<Ref>[]): Node[] {
  return tree.map((t): Node =>
    t.kind === 'folder'
      ? { kind: 'tagFolder', id: `tags/${t.path}`, label: t.label, children: t.children }
      : { kind: 'tag', ref: t.value, label: t.label },
  );
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

/** 署名の状態の、どの FE でも同じ形のアイコン（色は状態の色）。 */
function signatureSharedIcon(status: SignatureStatus): ReturnType<typeof sharedIcon> {
  switch (status.kind) {
    case 'verified':
      return sharedIcon('signed', 'green');
    case 'untrusted':
      return sharedIcon('trust-unknown', 'yellow');
    case 'signerMismatch':
      return sharedIcon('signer-mismatch', 'yellow');
    case 'expired':
      return sharedIcon('expired-key', 'orange');
    case 'revoked':
      return sharedIcon('revoked-key', 'red');
    case 'bad':
      return sharedIcon('bad-signature', 'red');
    case 'noKey':
      return sharedIcon('missing-key');
    case 'unsupported':
      return sharedIcon('cannot-verify');
    case 'pending':
      return undefined;
  }
}
