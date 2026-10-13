import { access } from 'node:fs/promises';
import { join } from 'node:path';
import * as vscode from 'vscode';
import type { RepositoryBackend } from '../git/backend.ts';
import type { Ref, Remote, Stash } from '../git/refs.ts';
import { configStamp, gitmodulesStamp, indexStamp, refsStamp, stashStamp } from '../git/stamp.ts';
import type { StatusEntry, StatusResult } from '../git/status.ts';
import { UndoLog } from './undo.ts';

export type Section = 'workingCopy' | 'branches' | 'remotes' | 'tags' | 'stashes' | 'submodules';

/** 展開されるまで取得しないセクション */
export type LazySection = 'tags' | 'stashes' | 'submodules';

export interface Submodule {
  name: string;
  path: string;
  initialized: boolean;
}

export interface RepositoryState {
  status?: StatusResult;
  /** `refs/heads` */
  branches?: Ref[];
  /** `refs/remotes` */
  remoteBranches?: Ref[];
  remotes?: Remote[];
  tags?: Ref[];
  stashes?: Stash[];
  submodules?: Submodule[];
}

interface Stamps {
  refs?: string;
  /** HEAD・index のファイル内容が変わりうるもの（ref と index） */
  contents?: string;
  tags?: string;
  config?: string;
  stash?: string;
  gitmodules?: string;
}

/**
 * 1 つのリポジトリの ref・status のキャッシュ。
 * 変化は `.git` の更新時刻で検知し、変わった部分だけ取り直す。
 */
export class Repository implements vscode.Disposable {
  readonly backend: RepositoryBackend;
  readonly undo: UndoLog;
  private readonly log: vscode.LogOutputChannel;
  private readonly state: RepositoryState = {};
  /** 最後に取得した時点の更新時刻 */
  private readonly stamps: Stamps = {};
  private readonly wanted = new Set<LazySection>();
  private running: Promise<void> | undefined;
  /** patchStatus のたびに増やす。それより前に始めた status の取得結果は古いので捨てる */
  private patchGeneration = 0;
  private queued = false;

  private readonly _onDidChange = new vscode.EventEmitter<ReadonlySet<Section>>();
  /** 変化したセクションを通知する */
  readonly onDidChange = this._onDidChange.event;

  private readonly _onDidChangeContents = new vscode.EventEmitter<void>();
  /**
   * HEAD や index にあるファイルの内容が変わったかもしれない。
   * status の表示が同じままでも（M のまま別の行を stage したなど）通知する。
   */
  readonly onDidChangeContents = this._onDidChangeContents.event;

  constructor(backend: RepositoryBackend, log: vscode.LogOutputChannel) {
    this.backend = backend;
    this.undo = new UndoLog(backend);
    this.log = log;
  }

  get root(): string {
    return this.backend.root;
  }

  get current(): Readonly<RepositoryState> {
    return this.state;
  }

  /**
   * 取り直す。実行中に呼ばれたら、終わってからもう 1 回だけ実行する。
   * 返す Promise はその再実行まで含めて完了する。
   */
  refresh(): Promise<void> {
    if (this.running) {
      this.queued = true;
      return this.running;
    }
    this.running = (async () => {
      try {
        do {
          this.queued = false;
          await this.load();
        } while (this.queued);
      } finally {
        this.running = undefined;
      }
    })();
    return this.running;
  }

  /** 遅延セクションが展開されたときに呼ぶ。未取得なら取得する */
  async want(section: LazySection): Promise<void> {
    if (this.wanted.has(section)) {
      return;
    }
    this.wanted.add(section);
    const changed = new Set<Section>();
    try {
      await this.loadLazy(section, changed);
    } catch (err) {
      this.log.error(`failed to load ${section}:`, err);
    }
    this.fire(changed);
  }

  private async load(): Promise<void> {
    const start = performance.now();
    const changed = new Set<Section>();
    const d = this.backend.dirs;
    const [refs, config, index] = await Promise.all([refsStamp(d), configStamp(d), indexStamp(d)]);
    const contents = `${refs}|${index}`;
    const contentsChanged = this.stamps.contents !== undefined && contents !== this.stamps.contents;
    this.stamps.contents = contents;

    const tasks: Promise<void>[] = [
      // worktree の変化は検知できないので status は毎回取る
      this.statusSince(this.patchGeneration).then((status) => {
        if (status && this.replace('status', status)) {
          changed.add('workingCopy');
          if (this.wanted.has('submodules')) {
            changed.add('submodules');
          }
        }
      }),
    ];
    // upstream の設定が変わると ahead/behind も変わる
    if (refs !== this.stamps.refs || config !== this.stamps.config) {
      tasks.push(
        this.backend.refs(['refs/heads', 'refs/remotes']).then((all) => {
          const branches = all.filter((r) => r.name.startsWith('refs/heads/'));
          const remoteBranches = all.filter((r) => r.name.startsWith('refs/remotes/'));
          if (this.replace('branches', branches)) changed.add('branches');
          if (this.replace('remoteBranches', remoteBranches)) changed.add('remotes');
          this.stamps.refs = refs;
        }),
      );
    }
    if (config !== this.stamps.config) {
      tasks.push(
        this.backend.remotes().then((remotes) => {
          if (this.replace('remotes', remotes)) changed.add('remotes');
          this.stamps.config = config;
        }),
      );
    }
    for (const section of this.wanted) {
      tasks.push(this.loadLazy(section, changed, refs));
    }

    const results = await Promise.allSettled(tasks);
    for (const r of results) {
      if (r.status === 'rejected') {
        this.log.error('refresh failed:', r.reason);
      }
    }
    this.log.debug(`refresh ${Math.round(performance.now() - start)}ms changed=[${[...changed].join(',')}]`);
    this.fire(changed);
    if (contentsChanged) {
      this._onDidChangeContents.fire();
    }
  }

  private async loadLazy(section: LazySection, changed: Set<Section>, refs?: string): Promise<void> {
    switch (section) {
      case 'tags': {
        refs ??= await refsStamp(this.backend.dirs);
        if (this.state.tags && refs === this.stamps.tags) {
          return;
        }
        const tags = await this.backend.refs(['refs/tags']);
        if (this.replace('tags', tags)) changed.add('tags');
        this.stamps.tags = refs;
        return;
      }
      case 'stashes': {
        const stamp = await stashStamp(this.backend.dirs);
        if (this.state.stashes && stamp === this.stamps.stash) {
          return;
        }
        const stashes = await this.backend.stashes();
        if (this.replace('stashes', stashes)) changed.add('stashes');
        this.stamps.stash = stamp;
        return;
      }
      case 'submodules': {
        const stamp = await gitmodulesStamp(this.root);
        if (this.state.submodules && stamp === this.stamps.gitmodules) {
          return;
        }
        const configs = await this.backend.submodules();
        const submodules = await Promise.all(
          configs.map(async (c) => ({ ...c, initialized: await exists(join(this.root, c.path, '.git')) })),
        );
        if (this.replace('submodules', submodules)) changed.add('submodules');
        this.stamps.gitmodules = stamp;
        return;
      }
    }
  }

  /** 値が変わっていれば差し替えて true を返す */
  private replace<K extends keyof RepositoryState>(key: K, value: RepositoryState[K]): boolean {
    if (JSON.stringify(this.state[key]) === JSON.stringify(value)) {
      return false;
    }
    this.state[key] = value;
    return true;
  }

  /**
   * git の結果を待たずに status を書き換えて画面に反映する（楽観的更新）。
   * 操作のあとに refresh すれば正しい値に置き換わる。
   */
  patchStatus(fn: (entries: readonly StatusEntry[]) => StatusEntry[]): void {
    const status = this.state.status;
    if (!status) {
      return;
    }
    this.patchGeneration++;
    this.state.status = { ...status, entries: fn(status.entries) };
    this.fire(new Set(['workingCopy']));
  }

  private async statusSince(generation: number): Promise<StatusResult | undefined> {
    const status = await this.backend.status();
    return generation === this.patchGeneration ? status : undefined;
  }

  private fire(changed: Set<Section>): void {
    if (changed.size > 0) {
      this._onDidChange.fire(changed);
    }
  }

  dispose(): void {
    this._onDidChange.dispose();
    this._onDidChangeContents.dispose();
    this.backend.dispose();
  }
}

async function exists(path: string): Promise<boolean> {
  try {
    await access(path);
    return true;
  } catch {
    return false;
  }
}
