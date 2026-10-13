import { basename } from 'node:path';
import * as vscode from 'vscode';
import { computeDiff, type FileDiff } from '../diff/fileDiff.ts';
import { parseCommitObject } from '../git/commitObject.ts';
import type { Commit } from '../git/log.ts';
import { GraphBuilder } from '../history/graph.ts';
import type {
  CommitDetails,
  HistoryHostMessage,
  HistoryWebviewMessage,
  RefLabel,
  RowData,
  Scope,
} from '../history/protocol.ts';
import { editableChain, refLabels } from '../history/rows.ts';
import type { Repository, Section } from '../state/repository.ts';
import { openCommitChanges, openCommitFile } from './commitChanges.ts';
import { showError } from './stageCommands.ts';

/** 1 回に読み込むコミットの数 */
const PAGE = 200;
/** 編集できるかを調べる範囲 */
const EDITABLE_LIMIT = 500;
/** 詳細を覚えておくコミットの数 */
const DETAILS_CACHE = 100;
/** 差分を覚えておくファイルの数 */
const DIFF_CACHE = 200;

/**
 * メインビューの History の拡張ホスト側。
 * log は分割して読み込み、グラフのレーンはこちらで計算して、描画に必要な最小限の配列だけ送る。
 */
export class HistoryController implements vscode.Disposable {
  private static readonly edits = new vscode.EventEmitter<
    Extract<HistoryWebviewMessage, { type: 'saveMessage' | 'move' }>
  >();
  /** Webview でメッセージの編集や並べ替えが行われた。処理は historyCommands が受け持つ */
  static readonly onDidReceiveEdit = HistoryController.edits.event;

  /** Webview へ送る。historyCommands からも使う */
  readonly post: (message: HistoryHostMessage) => void;
  private repoListener: vscode.Disposable | undefined;
  private repo: Repository | undefined;
  private scope: Scope = 'head';

  private commits: Commit[] = [];
  private graph = new GraphBuilder();
  private labels = new Map<string, RefLabel[]>();
  /** 編集できるコミットと、その位置（新しい順） */
  private chain = new Map<string, number>();
  private done = false;
  private loading = false;
  /** reset のたびに増やし、古い読み込みの結果を捨てる */
  private generation = 0;
  private headOid: string | undefined;
  private resetTimer: NodeJS.Timeout | undefined;
  /** 一度取ったコミットの詳細。コミットの内容は変わらないので、ref のラベル以外はそのまま使える */
  private readonly details = new Map<string, Omit<CommitDetails, 'refs'>>();
  /** 一度求めたファイルの差分。キーは `oid:path` */
  private readonly diffs = new Map<string, FileDiff>();

  constructor(post: (message: HistoryHostMessage) => void) {
    this.post = post;
  }

  /** 履歴の範囲を変えて読み直す */
  setScope(scope: Scope): void {
    this.scope = scope;
    void this.reset();
  }

  /** 表示するリポジトリを切り替えて読み直す */
  setRepository(repo: Repository | undefined): void {
    this.repoListener?.dispose();
    this.repo = repo;
    this.repoListener = repo?.onDidChange((changed) => this.onRepositoryChange(changed));
    this.details.clear();
    void this.reset();
  }

  get repository(): Repository | undefined {
    return this.repo;
  }

  commit(oid: string): Commit | undefined {
    return this.commits.find((c) => c.oid === oid);
  }

  /** 編集できる範囲での位置（新しい順） */
  chainIndex(oid: string): number | undefined {
    return this.chain.get(oid);
  }

  /** ブランチやタグ、HEAD が動いたときだけ読み直す。stage などでは読み直さない */
  private onRepositoryChange(changed: ReadonlySet<Section>): void {
    const headOid = this.repo?.current.status?.branch.oid;
    const refsChanged = changed.has('branches') || changed.has('remotes') || changed.has('tags');
    if (refsChanged || headOid !== this.headOid) {
      clearTimeout(this.resetTimer);
      this.resetTimer = setTimeout(() => void this.reset(), 100);
    }
  }

  async handle(message: HistoryWebviewMessage): Promise<void> {
    switch (message.type) {
      case 'more':
        await this.loadMore();
        break;
      case 'scope':
        this.scope = message.scope;
        await this.reset();
        break;
      case 'select':
        await this.sendDetails(message.oid).catch(showError);
        break;
      case 'commitDiff':
        await this.sendCommitDiff(message.oid, message.path).catch(showError);
        break;
      case 'openFile': {
        const details = this.details.get(message.oid);
        const file = details?.files.find((f) => f.path === message.path);
        if (this.repo && details && file) {
          await openCommitFile(this.repo, details.oid, details.parents[0], file).catch(showError);
        }
        break;
      }
      case 'open': {
        const commit = this.commit(message.oid);
        if (this.repo && commit) {
          await openCommitChanges(this.repo, commit).catch(showError);
        }
        break;
      }
      case 'saveMessage':
      case 'move':
        HistoryController.edits.fire(message);
        break;
    }
  }

  private async sendDetails(oid: string): Promise<void> {
    const repo = this.repo;
    if (!repo) {
      return;
    }
    let details = this.details.get(oid);
    if (!details) {
      const obj = await repo.backend.object(oid);
      if (!obj) {
        return;
      }
      const commit = parseCommitObject(obj.data.toString('utf8'));
      const files = await repo.backend.commitFiles(oid, commit.parents[0]);
      details = {
        oid,
        parents: commit.parents,
        author: commit.author,
        committer: commit.committer,
        message: commit.message,
        files,
      };
      this.details.set(oid, details);
      if (this.details.size > DETAILS_CACHE) {
        this.details.delete(this.details.keys().next().value!);
      }
    }
    this.post({ type: 'details', details: { ...details, refs: this.labels.get(oid) ?? [] } });
  }

  private async sendCommitDiff(oid: string, path: string): Promise<void> {
    const repo = this.repo;
    const details = this.details.get(oid);
    const file = details?.files.find((f) => f.path === path);
    if (!repo || !details || !file) {
      return;
    }
    const key = `${oid}:${path}`;
    let diff = this.diffs.get(key);
    if (!diff) {
      const parent = details.parents[0];
      const object = async (rev: string) => (await repo.backend.object(rev))?.data;
      const old = file.status === 'A' || !parent ? undefined : await object(`${parent}:${file.origPath ?? file.path}`);
      const now = file.status === 'D' ? undefined : await object(`${oid}:${file.path}`);
      diff = computeDiff(path, old, now).diff;
      this.diffs.set(key, diff);
      if (this.diffs.size > DIFF_CACHE) {
        this.diffs.delete(this.diffs.keys().next().value!);
      }
    }
    this.post({ type: 'commitDiff', oid, path, diff });
  }

  /** 最初から読み直す */
  async reset(): Promise<void> {
    const generation = ++this.generation;
    this.commits = [];
    this.graph = new GraphBuilder();
    this.done = false;
    this.loading = false;
    const repo = this.repo;
    this.post({ type: 'reset', title: repo ? basename(repo.root) : '', scope: this.scope });
    if (!repo) {
      this.post({ type: 'rows', start: 0, rows: [], done: true });
      return;
    }
    try {
      await repo.want('tags');
      const head = await repo.backend.head();
      const state = repo.current;
      this.headOid = head.oid;
      this.labels = refLabels(head, state.branches ?? [], state.remoteBranches ?? [], state.tags ?? []);
      this.chain = head.oid
        ? editableChain(await repo.backend.log(['--first-parent', 'HEAD'], 0, EDITABLE_LIMIT))
        : new Map();
    } catch (err) {
      showError(err);
    }
    if (generation === this.generation) {
      await this.loadMore();
    }
  }

  private async loadMore(): Promise<void> {
    const repo = this.repo;
    if (!repo || this.loading || this.done) {
      return;
    }
    this.loading = true;
    const generation = this.generation;
    const revs =
      this.scope === 'all'
        ? ['HEAD', '--branches', '--remotes', '--tags']
        : this.scope.startsWith('ref:')
          ? [this.scope.slice('ref:'.length)]
          : ['HEAD'];
    let page: Commit[] = [];
    try {
      page = this.headOid ? await repo.backend.log(revs, this.commits.length, PAGE) : [];
    } catch (err) {
      showError(err);
    }
    if (generation !== this.generation) {
      return;
    }
    const start = this.commits.length;
    this.commits.push(...page);
    const rows: RowData[] = page.map((c) => {
      const chain = this.chain.get(c.oid);
      return {
        oid: c.oid,
        subject: c.subject,
        author: c.author,
        date: c.date,
        refs: this.labels.get(c.oid) ?? [],
        graph: this.graph.add(c.oid, c.parents),
        editable: chain !== undefined,
        chain,
        canSquash: chain !== undefined && this.chain.size > chain + 1,
      };
    });
    this.done = page.length < PAGE;
    this.loading = false;
    this.post({ type: 'rows', start, rows, done: this.done });
  }

  dispose(): void {
    clearTimeout(this.resetTimer);
    this.repoListener?.dispose();
  }
}
