import { probe } from '../probe.ts';
import { basename } from 'node:path';
import * as vscode from 'vscode';
import type { DaemonClient } from '../daemon/client.ts';
import { daemonFor } from '../daemon/connection.ts';
import { commitFiles, splitCommitDiff } from '../daemon/diff.ts';
import { readRows, toRowData } from '../daemon/history.ts';
import { contentsToFetch, searchesContents, searchHistory } from '../daemon/search.ts';
import { computeDiff, type FileDiff } from '../diff/fileDiff.ts';
import { parseCommitObject } from '../git/commitObject.ts';
import type { Commit } from '../git/log.ts';
import { GraphBuilder } from '../history/graph.ts';
import type {
  CommitDetails,
  HistoryHostMessage,
  HistoryWebviewMessage,
  Person,
  RefLabel,
  RowData,
  Scope,
  SearchState,
} from '../history/protocol.ts';
import { editableChain, refLabels } from '../history/rows.ts';
import type { Repository, Section } from '../state/repository.ts';
import { openCommitChanges, openCommitFile } from './commitChanges.ts';
import { showError } from './stageCommands.ts';

/** 1 回に読み込むコミットの数 */
const PAGE = 200;

/** mailmap で置き換えて記録と違えば、記録されたもの */
function recordedIfMapped(recorded: Person, mapped: Person): Person | undefined {
  return recorded.name !== mapped.name || recorded.email !== mapped.email
    ? { name: recorded.name, email: recorded.email }
    : undefined;
}

/** daemon の答えの `{name, email}`（なければ undefined） */
function person(value: unknown): Person | undefined {
  const v = value as { name?: unknown; email?: unknown } | null | undefined;
  return v && typeof v.name === 'string' ? { name: v.name, email: String(v.email ?? '') } : undefined;
}
/** 編集できるかを調べる範囲 */
const EDITABLE_LIMIT = 500;
/** 詳細を覚えておくコミットの数 */
const DETAILS_CACHE = 100;
/** 差分を覚えておくファイルの数 */
const DIFF_CACHE = 200;

/** 共有の daemon の History: そのつながりと History の番号と行の数 */
interface DaemonHistory {
  client: DaemonClient;
  history: unknown;
  rows: number;
}

/** 昇順の `rows` で、`row` 以上の最初の位置（なければ最後） */
function lowerBound(rows: readonly number[], row: number): number {
  let lo = 0;
  let hi = rows.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (rows[mid]! < row) lo = mid + 1;
    else hi = mid;
  }
  return Math.min(lo, Math.max(0, rows.length - 1));
}

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

/** partial clone で中身を探す前に、取る中身の数を出して断る。上限を超えるなら探さず、絞るよう頼む */
async function confirmContentsSearch(count: number, more: boolean): Promise<boolean> {
  const n = count.toLocaleString();
  if (more) {
    await vscode.window.showWarningMessage('Too Many Contents to Fetch', {
      modal: true,
      detail: `This search needs more than ${n} file contents not fetched yet. Narrow it with path:, after: or before:.`,
    });
    return false;
  }
  const answer = await vscode.window.showWarningMessage(
    'Search the Contents?',
    { modal: true, detail: `This search fetches ${n} file contents not fetched yet.` },
    'Search',
  );
  return answer === 'Search';
}

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
  /** 読み込んだコミット（daemon の History は飛び飛びに読むので、ID から） */
  private readonly known = new Map<string, Commit>();
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
  /** 最後に始めた読み直し */
  private resetting: Promise<void> = Promise.resolve();
  /** 一度取ったコミットの詳細。コミットの内容は変わらないので、ref のラベル以外はそのまま使える */
  private readonly details = new Map<string, Omit<CommitDetails, 'refs'>>();
  /** 一度求めたファイルの差分。キーは `oid:path` */
  private readonly diffs = new Map<string, FileDiff>();
  /** 共有の daemon で History を読むとき（設定 `amonhen.daemon.path`）、出している History */
  private daemon: DaemonHistory | undefined;
  /** 一致だけの History を出しているとき、もとの History */
  private base: DaemonHistory | undefined;
  /** 今の検索。`rows` は `history`（もとの History）の行 */
  private search: { query: string; history: unknown; rows: number[] } | undefined;
  private searchAbort: AbortController | undefined;
  /** 探している途中の検索の文（History を読み直したら、終わっていなくても探し直す） */
  private searching: string | undefined;
  /** 一致だけを出す切り替え（最後に頼まれたもの） */
  private matchingOnly = false;

  constructor(post: (message: HistoryHostMessage) => void) {
    this.post = post;
  }

  /**
   * サイドバーから History を開いたときの範囲。ref を選んだならその ref、そうでなければ
   * ref の履歴を出していたときだけ現在のブランチに戻す（All Branches は保つ）
   */
  showScope(scope: Scope | undefined): void {
    const next = scope ?? (this.scope.startsWith('ref:') ? 'head' : this.scope);
    if (next !== this.scope) {
      this.scope = next;
      void this.reset();
    }
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
    return this.known.get(oid);
  }

  /** 編集できる範囲での位置（新しい順） */
  chainIndex(oid: string): number | undefined {
    return this.chain.get(oid);
  }

  /**
   * ブランチやタグ、HEAD が動いたときと、mailmap が変わったかもしれないとき（覚えた詳細も捨てる）だけ読み直す。
   * stage などでは読み直さない
   */
  private onRepositoryChange(changed: ReadonlySet<Section>): void {
    const headOid = this.repo?.current.status?.branch.oid;
    const refsChanged = changed.has('branches') || changed.has('remotes') || changed.has('tags');
    if (changed.has('mailmap')) {
      this.details.clear();
    }
    // status をまだ読んでいなければ、HEAD が動いたかは分からない（読み直さない）
    const headMoved = headOid !== undefined && headOid !== this.headOid;
    if (refsChanged || changed.has('mailmap') || headMoved) {
      clearTimeout(this.resetTimer);
      this.resetTimer = setTimeout(() => {
        this.resetTimer = undefined;
        void this.reset();
      }, 100);
    }
  }

  async handle(message: HistoryWebviewMessage): Promise<void> {
    switch (message.type) {
      case 'more':
        await this.loadMore();
        break;
      case 'range':
        if (this.daemon) {
          await this.loadRange(this.daemon, message.start, message.count);
        }
        break;
      case 'search':
        await this.runSearch(message.query.trim(), message.matchingOnly, message.selected);
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
        // History にないコミット（Reflog の、どのブランチからも辿れないもの）は、出した詳細から
        const details = this.details.get(message.oid);
        const commit =
          this.commit(message.oid) ??
          (details && {
            oid: details.oid,
            parents: details.parents,
            author: details.author.name,
            email: details.author.email,
            date: details.author.date,
            subject: details.message.split('\n', 1)[0] ?? '',
          });
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
    if (!details && this.daemon) {
      details = await this.daemonDetails(this.daemon.client, oid);
      this.details.set(oid, details);
    }
    if (!details) {
      const obj = await repo.backend.object(oid);
      if (!obj) {
        return;
      }
      const commit = parseCommitObject(obj.data.toString('utf8'));
      const [files, [author, committer]] = await Promise.all([
        repo.backend.commitFiles(oid, commit.parents[0]),
        repo.backend.mapPeople([commit.author, commit.committer]),
      ]);
      details = {
        oid,
        parents: commit.parents,
        author: { ...commit.author, ...author },
        committer: { ...commit.committer, ...committer },
        recordedAuthor: recordedIfMapped(commit.author, author!),
        recordedCommitter: recordedIfMapped(commit.committer, committer!),
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

  /** 共有の daemon で、コミットの詳細と全ファイルの差分を読む（差分は覚えておき、ファイルを選んだときに送る） */
  private async daemonDetails(client: DaemonClient, oid: string): Promise<Omit<CommitDetails, 'refs'>> {
    const [info, diff] = await Promise.all([
      client.call({ op: 'run', command: { op: 'commitInfo', oid } }),
      client.call({ op: 'commitDiff', id: oid }),
    ]);
    const i = info.json['info'] as Record<string, unknown>;
    if (info.json['ok'] !== true || !i) {
      throw new Error(String(info.json['error'] ?? 'コミットを読めません'));
    }
    for (const [path, d] of splitCommitDiff(diff)) {
      this.diffs.set(`${oid}:${path}`, d);
    }
    while (this.diffs.size > DIFF_CACHE) {
      this.diffs.delete(this.diffs.keys().next().value!);
    }
    return {
      oid,
      parents: i['parents'] as string[],
      author: { name: String(i['author']), email: String(i['authorEmail']), date: Number(i['authorTime']) },
      committer: { name: String(i['committer']), email: String(i['committerEmail']), date: Number(i['committerTime']) },
      recordedAuthor: person(i['recordedAuthor']),
      recordedCommitter: person(i['recordedCommitter']),
      message: String(i['message']),
      files: commitFiles(diff),
    };
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

  /**
   * oid のコミットを選んで見せる（Reflog の Show in History）。今の範囲になければ All Branches にして探す。
   * 共有の daemon の History だけ。見つかったか返す（一致だけの History で、一致しなければ見つからない）
   */
  async reveal(oid: string): Promise<boolean> {
    const rowOf = async (): Promise<number | undefined> => {
      const shown = this.daemon;
      if (!shown) return undefined;
      const row = (await shown.client.call({ op: 'historyRowOf', history: shown.history, id: oid })).json['row'];
      return typeof row === 'number' ? row : undefined;
    };
    await this.settled();
    let row = await rowOf();
    if (row === undefined && this.scope !== 'all') {
      this.scope = 'all';
      void this.reset();
      await this.settled();
      row = await rowOf();
    }
    if (row === undefined) return false;
    this.post({ type: 'reveal', row });
    return true;
  }

  /**
   * 読み直しが終わるまで待つ。HEAD やブランチが動いた直後で読み直しを待たせていれば、今すぐ読み直す。
   * 待つ間に次の読み直しが始まれば、それも待つ
   */
  private async settled(): Promise<void> {
    if (this.resetTimer !== undefined) {
      clearTimeout(this.resetTimer);
      this.resetTimer = undefined;
      void this.reset();
    }
    let generation: number;
    do {
      generation = this.generation;
      await this.resetting.catch(() => undefined);
    } while (generation !== this.generation);
  }

  /** 最初から読み直す */
  reset(): Promise<void> {
    this.resetting = this.reload();
    return this.resetting;
  }

  private async reload(): Promise<void> {
    const generation = ++this.generation;
    probe(`history reload ${generation} start`);
    this.commits = [];
    this.known.clear();
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
      probe(`history reload ${generation} want tags`);
      const head = await repo.backend.head();
      probe(`history reload ${generation} head`);
      const state = repo.current;
      this.headOid = head.oid;
      this.labels = refLabels(head, state.branches ?? [], state.remoteBranches ?? [], state.tags ?? []);
      this.chain = head.oid
        ? editableChain(await repo.backend.log(['--first-parent', 'HEAD'], 0, EDITABLE_LIMIT))
        : new Map();
      probe(`history reload ${generation} chain`);
    } catch (err) {
      showError(err);
    }
    this.searchAbort?.abort();
    for (const previous of [this.daemon, this.base]) {
      void previous?.client.call({ op: 'historyRelease', history: previous.history }).catch(() => undefined);
    }
    this.daemon = undefined;
    this.base = undefined;
    try {
      const opened = this.headOid ? await this.openDaemonHistory(repo.root) : undefined;
      probe(`history reload ${generation} opened (current ${this.generation})`);
      if (generation !== this.generation) {
        return;
      }
      this.daemon = opened;
    } catch (err) {
      // daemon を使えなければ、git で読む
      showError(err);
    }
    if (generation !== this.generation) {
      return;
    }
    this.post({ type: 'searchable', searchable: this.daemon !== undefined });
    await this.loadMore();
    // 探していたら（途中でも）、新しい History で探し直す
    const query = this.searching ?? this.search?.query;
    if (query && this.daemon && generation === this.generation) {
      await this.runSearch(query, this.matchingOnly, 0);
    }
  }

  /**
   * 探して、一致を Webview に送る。`matchingOnly` なら一致だけの History に切り替える。`selected` は Webview で
   * 選んでいる行で、History を切り替えるときは同じコミット（なければ近いもの）を選んだままにする。空なら検索をやめる
   */
  private async runSearch(query: string, matchingOnly: boolean, selected: number): Promise<void> {
    this.searchAbort?.abort();
    const abort = new AbortController();
    this.searchAbort = abort;
    this.searching = query || undefined;
    this.matchingOnly = matchingOnly;
    try {
      await this.searchOnce(query, matchingOnly, selected, abort);
    } finally {
      if (this.searchAbort === abort && !abort.signal.aborted) {
        this.searching = undefined;
      }
    }
  }

  private async searchOnce(
    query: string,
    matchingOnly: boolean,
    selected: number,
    abort: AbortController,
  ): Promise<void> {
    const shown = this.daemon;
    if (!shown) {
      return;
    }
    const base = this.base ?? shown;
    // もとの History での、選んでいる行
    const baseSelected = this.base ? (this.search?.rows[selected] ?? 0) : selected;
    const post = (state: Partial<SearchState>) =>
      this.post({
        type: 'search',
        state: { query, rows: [], searching: false, done: 0, matchingOnly: false, ...state },
      });
    if (!query) {
      this.search = undefined;
      await this.showMatching(base, undefined, baseSelected, abort.signal);
      post({});
      return;
    }
    let rows: number[];
    if (this.search?.query === query && this.search.history === base.history) {
      // 一致だけを出す切り替えだけなら、探し直さない
      rows = this.search.rows;
    } else {
      try {
        if (searchesContents(query)) {
          post({ searching: true });
          const need = await contentsToFetch(base.client, base.history, query, abort.signal);
          if (abort.signal.aborted) {
            return;
          }
          if (need.count > 0 && !(await confirmContentsSearch(need.count, need.more))) {
            if (this.searchAbort === abort) {
              await this.runSearch('', false, selected);
            }
            return;
          }
          if (abort.signal.aborted) {
            return;
          }
        }
        post({ searching: true });
        rows = await searchHistory(base.client, base.history, query, abort.signal, (partial, done) =>
          post({ rows: this.base ? [] : partial, searching: true, done, matchingOnly: this.base !== undefined }),
        );
      } catch (err) {
        if (!abort.signal.aborted) {
          post({ error: errorMessage(err) });
        }
        return;
      }
    }
    if (abort.signal.aborted) {
      return;
    }
    this.search = { query, history: base.history, rows };
    try {
      await this.showMatching(base, matchingOnly && rows.length > 0 ? rows : undefined, baseSelected, abort.signal);
    } catch (err) {
      post({ error: errorMessage(err) });
      return;
    }
    if (!abort.signal.aborted) {
      post({ rows, matchingOnly: this.base !== undefined });
    }
  }

  /**
   * `rows` があれば、`base` のその行だけの History（一致だけ）を、なければ `base` を出す。`selected` は `base` での
   * 選んでいる行
   */
  private async showMatching(
    base: DaemonHistory,
    rows: number[] | undefined,
    selected: number,
    signal: AbortSignal,
  ): Promise<void> {
    const filtered = this.base ? this.daemon : undefined;
    let next = base;
    let select = selected;
    if (rows) {
      const reply = await base.client.call({ op: 'historyMatching', history: base.history, rows });
      // 待つ間に読み直しや次の検索が始まっていれば、作った History は使わない（今の History を取り違える）
      if (signal.aborted) {
        void base.client.call({ op: 'historyRelease', history: reply.json['history'] }).catch(() => undefined);
        return;
      }
      next = { client: base.client, history: reply.json['history'], rows: Number(reply.json['rows']) };
      select = lowerBound(rows, selected);
    } else if (!filtered) {
      return;
    }
    if (filtered) {
      void filtered.client.call({ op: 'historyRelease', history: filtered.history }).catch(() => undefined);
    }
    this.base = rows ? base : undefined;
    this.daemon = next;
    const repo = this.repo;
    this.post({ type: 'reset', title: repo ? basename(repo.root) : '', scope: this.scope, select });
    const start = Math.floor(select / PAGE) * PAGE;
    await this.loadRange(next, start, PAGE);
  }

  /** 共有の daemon で、今の範囲の History を開く（daemon を使わない設定なら undefined） */
  private async openDaemonHistory(root: string): Promise<DaemonHistory | undefined> {
    const client = await daemonFor(root);
    probe('history daemonFor');
    if (!client) {
      return undefined;
    }
    let tips: string[] | undefined;
    if (this.scope === 'head') {
      tips = [this.headOid!];
    } else if (this.scope.startsWith('ref:')) {
      const resolved = await client.call({ op: 'run', command: { op: 'resolveCommit', rev: this.scope.slice('ref:'.length) } });
      tips = [String(resolved.json['text'])];
    }
    probe('history open ask');
    const history = await client.call(tips ? { op: 'history', tips } : { op: 'history' });
    probe('history open got');
    return { client, history: history.json['history'], rows: Number(history.json['rows']) };
  }

  /**
   * 共有の daemon の History の、start から count 行を送る。行の数を知っているので、Webview は見えているところだけを
   * 飛び飛びに頼む（離れた一致へもすぐ移れる）
   */
  private async loadRange(daemon: DaemonHistory, start: number, count: number): Promise<void> {
    let rows: RowData[] = [];
    try {
      probe(`history rows ${start}+${count} ask`);
      const reply = await daemon.client.call({ op: 'historyRows', history: daemon.history, start, count });
      probe(`history rows ${start}+${count} got (same ${daemon === this.daemon})`);
      if (daemon !== this.daemon) {
        return;
      }
      const page = readRows(reply);
      for (const r of page) {
        this.known.set(r.oid, { oid: r.oid, parents: r.parentOids, author: r.author, email: '', date: r.date, subject: r.subject });
      }
      rows = toRowData(page, this.labels, this.chain);
    } catch (err) {
      if (daemon !== this.daemon) {
        return;
      }
      showError(err);
    }
    this.post({ type: 'rows', start, rows, done: true, total: daemon.rows });
  }

  private async loadMore(): Promise<void> {
    const repo = this.repo;
    if (!repo || this.loading || this.done) {
      return;
    }
    if (this.daemon) {
      this.done = true;
      return this.loadRange(this.daemon, 0, PAGE);
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
    for (const c of page) {
      this.known.set(c.oid, c);
    }
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
    this.searchAbort?.abort();
    this.repoListener?.dispose();
  }
}
