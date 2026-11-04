/**
 * サイドバーの submodule の行のもと。共有の daemon のコアの `submodules`（URL・追うブランチ・取り出したか・記録と
 * 違うコミットか・中の変更・コンフリクト）を読み、submodule や作業ツリーが変わるたびに読み直す
 */
import * as vscode from 'vscode';
import { daemonFor } from '../daemon/connection.ts';
import type { SubmoduleFact } from '../git/submodules.ts';
import type { Repository, Section } from '../state/repository.ts';

export class SubmoduleFacts implements vscode.Disposable {
  private repo: Repository | undefined;
  private repoListener: vscode.Disposable | undefined;
  private timer: NodeJS.Timeout | undefined;
  /** 読むたびに増やし、古い答えを捨てる */
  private generation = 0;
  /** 場所 → 様子 */
  private readonly facts = new Map<string, SubmoduleFact>();

  private readonly _onDidChange = new vscode.EventEmitter<void>();
  readonly onDidChange = this._onDidChange.event;

  get(path: string): SubmoduleFact | undefined {
    return this.facts.get(path);
  }

  setRepository(repo: Repository | undefined): void {
    if (repo === this.repo) return;
    this.repoListener?.dispose();
    this.repo = repo;
    this.generation++;
    this.facts.clear();
    this.repoListener = repo?.onDidChange((changed: ReadonlySet<Section>) => {
      if (changed.has('submodules') || changed.has('workingCopy')) this.schedule();
    });
    if (repo) this.schedule();
  }

  /** 今すぐ読み直す（submodule を変えた後） */
  reload(): Promise<void> {
    clearTimeout(this.timer);
    return this.load();
  }

  private schedule(): void {
    clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      this.timer = undefined;
      void this.load();
    }, 200);
  }

  private async load(): Promise<void> {
    const generation = ++this.generation;
    const repo = this.repo;
    const client = repo ? await daemonFor(repo.root).catch(() => undefined) : undefined;
    if (!repo || !client || generation !== this.generation) return;
    try {
      const reply = (await client.call({ op: 'run', command: { op: 'submodules' } })).json;
      if (reply['ok'] !== true || generation !== this.generation) return;
      const list = (reply['info'] as { submodules?: SubmoduleFact[] } | undefined)?.submodules ?? [];
      this.facts.clear();
      for (const f of list) this.facts.set(f.path, f);
      this._onDidChange.fire();
    } catch {
      // 読めなければ git の status の様子だけを出す
    }
  }

  dispose(): void {
    clearTimeout(this.timer);
    this.repoListener?.dispose();
    this._onDidChange.dispose();
  }
}
