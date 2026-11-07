/**
 * サイドバーのブランチの並べ方のもと。共有の daemon のコアの `branchGroups`（取り込み済みか・積んだ土台・
 * お気に入り・アーカイブ）を読み、ブランチが動くたびに読み直す。マージされたプルリクエストも gh で読んで渡す
 * （squash・rebase で取り込んだブランチの印）
 */
import * as vscode from 'vscode';
import type { Json } from '../daemon/client.ts';
import { daemonFor } from '../daemon/connection.ts';
import type { BranchGroupsInfo } from '../git/branchGroups.ts';
import { type MergedPullRequest, mergedPullRequests, pullRequestTarget, repositorySlug } from '../git/github.ts';
import type { Repository, Section } from '../state/repository.ts';

/** マージされたプルリクエストを読み直す間隔の下限（アプリと同じ） */
const PULL_REQUESTS_INTERVAL = 30_000;

export class BranchFacts implements vscode.Disposable {
  private repo: Repository | undefined;
  private repoListener: vscode.Disposable | undefined;
  private timer: NodeJS.Timeout | undefined;
  /** 読むたびに増やし、古い答えを捨てる */
  private generation = 0;
  private current: BranchGroupsInfo | undefined;
  private pullRequests: MergedPullRequest[] = [];
  private pullRequestsReadAt = 0;
  /** ウィンドウに戻ってきたら読み直す（GitHub でマージして戻ってきたとき） */
  private readonly focusListener = vscode.window.onDidChangeWindowState((e) => e.focused && void this.readPullRequests());

  /** マージされたプルリクエストの読み方（GitHub のリポジトリでなければ空。e2e が差し替える） */
  readMerged = async (repo: Repository): Promise<MergedPullRequest[]> => {
    const slug = repositorySlug(await repo.backend.remotes());
    return slug ? mergedPullRequests(await pullRequestTarget(slug)) : [];
  };

  private readonly _onDidChange = new vscode.EventEmitter<void>();
  readonly onDidChange = this._onDidChange.event;

  /** まだ読んでいなければ undefined（そのときは名前のフォルダだけで出す） */
  get info(): BranchGroupsInfo | undefined {
    return this.current;
  }

  /** `repo` のブランチ `name` をマージしたプルリクエストの番号（squash・rebase） */
  pullRequestOf(repo: Repository, name: string): number | undefined {
    if (repo !== this.repo) return undefined;
    return this.current?.branches.find((b) => b.name === name)?.pullRequest ?? undefined;
  }

  setRepository(repo: Repository | undefined): void {
    if (repo === this.repo) return;
    this.repoListener?.dispose();
    this.repo = repo;
    this.generation++;
    this.current = undefined;
    this.pullRequests = [];
    this.pullRequestsReadAt = 0;
    this.repoListener = repo?.onDidChange((changed: ReadonlySet<Section>) => {
      if (changed.has('branches')) this.schedule(200);
    });
    if (repo) this.schedule(0);
  }

  /** 今すぐ読み直す（お気に入り・アーカイブを変えた後。git の設定なので、ブランチの知らせは来ない） */
  async reload(): Promise<void> {
    clearTimeout(this.timer);
    this.timer = undefined;
    await this.load();
  }

  /** マージされたプルリクエストを読み、変わっていたら分け方を読み直す。読めなければ前のまま（gh がないときも） */
  async readPullRequests(force = false): Promise<void> {
    const repo = this.repo;
    if (!repo || (!force && Date.now() - this.pullRequestsReadAt < PULL_REQUESTS_INTERVAL)) return;
    this.pullRequestsReadAt = Date.now();
    const list = await this.readMerged(repo).catch(() => undefined);
    if (!list || repo !== this.repo || JSON.stringify(list) === JSON.stringify(this.pullRequests)) return;
    this.pullRequests = list;
    await this.load();
  }

  private schedule(delay: number): void {
    clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      this.timer = undefined;
      void this.load();
    }, delay);
  }

  private async load(): Promise<void> {
    const generation = ++this.generation;
    const repo = this.repo;
    const client = repo && (await daemonFor(repo.root).catch(() => undefined));
    if (!repo || !client || generation !== this.generation) return;
    try {
      // daemon は ref を変わったと知るまで読み直さないので、先に読み直させる
      await client.call({ op: 'refs' });
      const reply = (await client.call({ op: 'run', command: { op: 'branchGroups', pullRequests: this.pullRequests as unknown as Json } })).json;
      if (generation !== this.generation || reply['ok'] !== true) return;
      this.current = reply['info'] as unknown as BranchGroupsInfo;
      this._onDidChange.fire();
      void this.readPullRequests();
    } catch {
      // 読めなければ名前のフォルダだけで出す
    }
  }

  dispose(): void {
    clearTimeout(this.timer);
    this.repoListener?.dispose();
    this.focusListener.dispose();
    this._onDidChange.dispose();
  }
}
