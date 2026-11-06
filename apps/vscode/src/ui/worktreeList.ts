/**
 * サイドバーの worktree のもと。共有の daemon のコアで worktree の一覧（`worktrees`）と、それぞれの変更の数
 * （`worktreeChanges`。変わっていない worktree は前の数を返すので軽い）を読み、今のリポジトリが変わるたびに
 * 読み直す
 */
import * as vscode from 'vscode';
import { daemonFor } from '../daemon/connection.ts';
import { canonical, type Worktree } from '../git/worktrees.ts';
import type { Repository, Section } from '../state/repository.ts';

export class WorktreeList implements vscode.Disposable {
  private repo: Repository | undefined;
  private repoListener: vscode.Disposable | undefined;
  private timer: NodeJS.Timeout | undefined;
  /** 読むたびに増やし、古い答えを捨てる */
  private generation = 0;
  private list: Worktree[] = [];
  /** 揃えたパス → 変更のあるファイルの数 */
  private readonly counts = new Map<string, number>();

  private readonly _onDidChange = new vscode.EventEmitter<void>();
  readonly onDidChange = this._onDidChange.event;
  /** ほかの worktree の変更は今のリポジトリの知らせには出ないので、窓に戻ってきたときにも数え直す */
  private readonly focusListener = vscode.window.onDidChangeWindowState((e) => e.focused && this.repo && this.schedule(0));

  get worktrees(): readonly Worktree[] {
    return this.list;
  }

  changes(w: Worktree): number {
    return this.counts.get(canonical(w.path)) ?? 0;
  }

  /**
   * 今の変更の数（覚えている数ではなく、数え直す）。覚えている数は古いことがある（変更のない worktree は HEAD か
   * index が変わるまで数え直さないので、後から足したファイルは数に入らない）。数えられなければ覚えている数
   */
  async freshChanges(w: Worktree): Promise<number> {
    const client = this.repo && (await daemonFor(this.repo.root).catch(() => undefined));
    if (!client) return this.changes(w);
    try {
      const reply = (await client.call({ op: 'run', command: { op: 'worktreeChanges', paths: [w.path], fresh: true } })).json;
      const count = (reply['info'] as { worktrees?: { count: number | null }[] } | undefined)?.worktrees?.[0]?.count;
      return count ?? this.changes(w);
    } catch {
      return this.changes(w);
    }
  }

  /** `branch` を checkout している、`except`（書き込む先）以外の worktree */
  checkedOutElsewhere(branch: string, except: string): Worktree | undefined {
    if (this.list.length < 2) return undefined;
    const editing = canonical(except);
    return this.list.find((w) => w.branch === branch && canonical(w.path) !== editing);
  }

  setRepository(repo: Repository | undefined): void {
    if (repo === this.repo) return;
    this.repoListener?.dispose();
    this.repo = repo;
    this.generation++;
    this.repoListener = repo?.onDidChange((changed) => this.onRepositoryChange(changed));
    this.schedule(0);
  }

  /** 今すぐ読み直す（worktree を足した・消した後） */
  async reload(): Promise<void> {
    clearTimeout(this.timer);
    this.timer = undefined;
    await this.load();
  }

  private onRepositoryChange(changed: ReadonlySet<Section>): void {
    // 作業中の変更は数に、ブランチの checkout は一覧に出る
    if (changed.has('workingCopy') || changed.has('branches')) this.schedule(300);
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
    if (generation !== this.generation) return;
    if (!repo || !client) {
      this.set([], []);
      return;
    }
    try {
      const reply = (await client.call({ op: 'run', command: { op: 'worktrees' } })).json;
      if (generation !== this.generation || reply['ok'] !== true) return;
      const list = ((reply['info'] as { worktrees?: Worktree[] } | undefined)?.worktrees ?? []).filter((w) => !w.bare);
      let counts: { path: string; count: number }[] = [];
      const paths = list.filter((w) => w.prunable === null).map((w) => w.path);
      if (list.length > 1 && paths.length > 0) {
        const changed = (await client.call({ op: 'run', command: { op: 'worktreeChanges', paths } })).json;
        if (generation !== this.generation) return;
        counts = (changed['info'] as { worktrees?: { path: string; count: number }[] } | undefined)?.worktrees ?? [];
      }
      this.set(list, counts);
    } catch {
      // worktree は出せなくてもよい（読めなければ出さない）
    }
  }

  private set(list: Worktree[], counts: { path: string; count: number }[]): void {
    const next = new Map(counts.map((c) => [canonical(c.path), c.count]));
    const same =
      JSON.stringify(list) === JSON.stringify(this.list) &&
      next.size === this.counts.size &&
      [...next].every(([k, v]) => this.counts.get(k) === v);
    if (same) return;
    this.list = list;
    this.counts.clear();
    for (const [k, v] of next) this.counts.set(k, v);
    this._onDidChange.fire();
  }

  dispose(): void {
    clearTimeout(this.timer);
    this.repoListener?.dispose();
    this.focusListener.dispose();
    this._onDidChange.dispose();
  }
}
