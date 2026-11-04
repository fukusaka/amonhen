/**
 * サイドバーの stash の行のもと。共有の daemon のコアの `stashBases`（土台と、今の HEAD との離れ具合）と、stash の
 * コミットの親の数（3 つめの親があれば untracked のファイルも入っている）を読み、stash や HEAD が動くたびに読み直す
 */
import * as vscode from 'vscode';
import { daemonFor } from '../daemon/connection.ts';
import { parseCommitObject } from '../git/commitObject.ts';
import type { Repository, Section } from '../state/repository.ts';

export interface StashFact {
  base?: string;
  ahead: number;
  behind: number;
  untracked: boolean;
}

export class StashFacts implements vscode.Disposable {
  private repo: Repository | undefined;
  private repoListener: vscode.Disposable | undefined;
  private timer: NodeJS.Timeout | undefined;
  /** 読むたびに増やし、古い答えを捨てる */
  private generation = 0;
  /** stash のコミット → 様子 */
  private readonly facts = new Map<string, StashFact>();

  private readonly _onDidChange = new vscode.EventEmitter<void>();
  readonly onDidChange = this._onDidChange.event;

  get(oid: string): StashFact | undefined {
    return this.facts.get(oid);
  }

  setRepository(repo: Repository | undefined): void {
    if (repo === this.repo) return;
    this.repoListener?.dispose();
    this.repo = repo;
    this.generation++;
    this.facts.clear();
    this.repoListener = repo?.onDidChange((changed: ReadonlySet<Section>) => {
      if (changed.has('stashes') || changed.has('branches')) this.schedule();
    });
    if (repo) this.schedule();
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
    const stashes = repo?.current.stashes;
    const client = repo && stashes?.length ? await daemonFor(repo.root).catch(() => undefined) : undefined;
    if (!repo || !stashes || !client || generation !== this.generation) return;
    try {
      await client.call({ op: 'refs' });
      const reply = (await client.call({ op: 'run', command: { op: 'stashBases' } })).json;
      const bases = new Map(
        ((reply['info'] as { stashes?: { name: string; base: string; ahead: number; behind: number }[] } | undefined)?.stashes ?? []).map(
          (b) => [b.name, b],
        ),
      );
      const next = new Map<string, StashFact>();
      for (const stash of stashes) {
        const object = await repo.backend.object(stash.oid);
        const parents = object ? parseCommitObject(object.data.toString('utf8')).parents : [];
        const base = bases.get(stash.name);
        next.set(stash.oid, {
          ...(base ? { base: base.base } : {}),
          ahead: base?.ahead ?? 0,
          behind: base?.behind ?? 0,
          untracked: parents.length > 2,
        });
      }
      if (generation !== this.generation) return;
      this.facts.clear();
      for (const [k, v] of next) this.facts.set(k, v);
      this._onDidChange.fire();
    } catch {
      // 読めなければ名前と件名だけを出す
    }
  }

  dispose(): void {
    clearTimeout(this.timer);
    this.repoListener?.dispose();
    this._onDidChange.dispose();
  }
}
