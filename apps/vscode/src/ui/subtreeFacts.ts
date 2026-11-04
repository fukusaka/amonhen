/**
 * サイドバーの Subtrees のもと。共有の daemon のコアの `subtrees`（この機械の git に git subtree があるか、取り込んだ
 * フォルダと覚えている相手）を読み、HEAD やブランチが動くたびに読み直す（コアは読んだ HEAD を覚え、増えた分だけ読む）。
 * `amonhen.subtrees` のコンテキストで、メニューとコマンドパレットに出すかを決める
 */
import * as vscode from 'vscode';
import { daemonFor } from '../daemon/connection.ts';
import type { Subtree, SubtreesInfo } from '../git/subtrees.ts';
import type { Repository, Section } from '../state/repository.ts';

export class SubtreeFacts implements vscode.Disposable {
  private repo: Repository | undefined;
  private repoListener: vscode.Disposable | undefined;
  private timer: NodeJS.Timeout | undefined;
  /** 読むたびに増やし、古い答えを捨てる */
  private generation = 0;
  private info: SubtreesInfo = { available: false, subtrees: [] };

  private readonly _onDidChange = new vscode.EventEmitter<void>();
  readonly onDidChange = this._onDidChange.event;

  get available(): boolean {
    return this.info.available;
  }

  get subtrees(): readonly Subtree[] {
    return this.info.subtrees;
  }

  get(dir: string): Subtree | undefined {
    return this.info.subtrees.find((s) => s.dir === dir);
  }

  setRepository(repo: Repository | undefined): void {
    if (repo === this.repo) return;
    this.repoListener?.dispose();
    this.repo = repo;
    this.generation++;
    this.set({ available: false, subtrees: [] });
    this.repoListener = repo?.onDidChange((changed: ReadonlySet<Section>) => {
      if (changed.has('branches')) this.schedule();
    });
    if (repo) this.schedule();
  }

  /** 今すぐ読み直す（subtree を変えた後） */
  reload(): Promise<void> {
    clearTimeout(this.timer);
    return this.load();
  }

  private schedule(): void {
    clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      this.timer = undefined;
      void this.load();
    }, 300);
  }

  private set(info: SubtreesInfo): void {
    this.info = info;
    void vscode.commands.executeCommand('setContext', 'amonhen.subtrees', info.available);
    this._onDidChange.fire();
  }

  private async load(): Promise<void> {
    const generation = ++this.generation;
    const repo = this.repo;
    const client = repo ? await daemonFor(repo.root).catch(() => undefined) : undefined;
    if (!repo || !client || generation !== this.generation) return;
    try {
      const reply = (await client.call({ op: 'run', command: { op: 'subtrees' } })).json;
      if (reply['ok'] !== true || generation !== this.generation) return;
      const info = reply['info'] as Partial<SubtreesInfo> | undefined;
      this.set({ available: info?.available === true, subtrees: info?.subtrees ?? [] });
    } catch {
      // 読めなければ出さない
    }
  }

  dispose(): void {
    clearTimeout(this.timer);
    this.repoListener?.dispose();
    this._onDidChange.dispose();
  }
}
