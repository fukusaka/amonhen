/**
 * サイドバーのブランチの並べ方のもと。共有の daemon のコアの `branchGroups`（取り込み済みか・積んだ土台・
 * お気に入り・アーカイブ）を読み、ブランチが動くたびに読み直す
 */
import * as vscode from 'vscode';
import { daemonFor } from '../daemon/connection.ts';
import type { BranchGroupsInfo } from '../git/branchGroups.ts';
import type { Repository, Section } from '../state/repository.ts';

export class BranchFacts implements vscode.Disposable {
  private repo: Repository | undefined;
  private repoListener: vscode.Disposable | undefined;
  private timer: NodeJS.Timeout | undefined;
  /** 読むたびに増やし、古い答えを捨てる */
  private generation = 0;
  private current: BranchGroupsInfo | undefined;

  private readonly _onDidChange = new vscode.EventEmitter<void>();
  readonly onDidChange = this._onDidChange.event;

  /** まだ読んでいなければ undefined（そのときは名前のフォルダだけで出す） */
  get info(): BranchGroupsInfo | undefined {
    return this.current;
  }

  setRepository(repo: Repository | undefined): void {
    if (repo === this.repo) return;
    this.repoListener?.dispose();
    this.repo = repo;
    this.generation++;
    this.current = undefined;
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
      const reply = (await client.call({ op: 'run', command: { op: 'branchGroups' } })).json;
      if (generation !== this.generation || reply['ok'] !== true) return;
      this.current = reply['info'] as unknown as BranchGroupsInfo;
      this._onDidChange.fire();
    } catch {
      // 読めなければ名前のフォルダだけで出す
    }
  }

  dispose(): void {
    clearTimeout(this.timer);
    this.repoListener?.dispose();
    this._onDidChange.dispose();
  }
}
