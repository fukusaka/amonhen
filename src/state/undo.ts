/**
 * 履歴を書き換える操作の Undo。
 * 操作の前後で HEAD と関係する ref を記録し、`<gitDir>/amonhen/undo.json` に残す。
 * Undo するときは、今の状態が記録した「操作後」と一致することを確かめてから戻す。
 */
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import type { HeadState, RepositoryBackend } from '../git/backend.ts';

export interface RefSnapshot {
  head: HeadState;
  /** 完全な ref 名 → oid。存在しなければ null */
  refs: Record<string, string | null>;
}

export interface UndoEntry {
  label: string;
  time: number;
  /** 現在のブランチを戻すときの reset の種類。commit は soft、merge や rebase は keep */
  mode: 'soft' | 'keep';
  before: RefSnapshot;
  after: RefSnapshot;
}

/** 記録した後に、ほかの操作でブランチや HEAD が動いている */
export class UndoConflictError extends Error {
  readonly entry: UndoEntry;

  constructor(entry: UndoEntry) {
    super(`「${entry.label}」のあとにブランチや HEAD が変わっているため、Undo できません`);
    this.name = 'UndoConflictError';
    this.entry = entry;
  }
}

const MAX_ENTRIES = 50;

export class UndoLog {
  private readonly backend: RepositoryBackend;
  private readonly file: string;
  private entries: UndoEntry[] | undefined;

  constructor(backend: RepositoryBackend) {
    this.backend = backend;
    this.file = join(backend.dirs.gitDir, 'amonhen', 'undo.json');
  }

  /** 次に Undo される操作 */
  async peek(): Promise<UndoEntry | undefined> {
    return (await this.load()).at(-1);
  }

  /**
   * fn を実行し、成功して ref が動いたら記録する。
   * fn が失敗したときと、skip が true を返したとき（コンフリクトで止まったときなど）は記録しない。
   */
  async record<T>(
    label: string,
    refs: readonly string[],
    mode: UndoEntry['mode'],
    fn: () => Promise<T>,
    skip?: (result: T) => boolean,
  ): Promise<T> {
    const before = await this.snapshot(refs);
    const result = await fn();
    if (skip?.(result)) {
      return result;
    }
    const after = await this.snapshot(refs);
    if (!sameSnapshot(before, after)) {
      const entries = await this.load();
      entries.push({ label, time: Date.now(), mode, before, after });
      entries.splice(0, Math.max(0, entries.length - MAX_ENTRIES));
      await this.save();
    }
    return result;
  }

  /** 最後の操作を戻す。戻したものを返す */
  async undo(): Promise<UndoEntry | undefined> {
    const entries = await this.load();
    const entry = entries.at(-1);
    if (!entry) {
      return undefined;
    }
    const { before, after, mode } = entry;
    if (!sameSnapshot(await this.snapshot(Object.keys(after.refs)), after)) {
      throw new UndoConflictError(entry);
    }

    const done = new Set<string>();
    // 1. いま checkout しているブランチ（または detached HEAD）を、作業中の変更を残したまま戻す
    if (after.head.branch) {
      // 操作の中で作ったブランチ（old が null）は、手順 2 で離れてから手順 3 で消す
      const old = before.refs[after.head.branch];
      if (old) {
        if (old !== after.refs[after.head.branch]) {
          await this.backend.reset(mode, old);
        }
        done.add(after.head.branch);
      }
    } else if (!before.head.branch && before.head.oid && before.head.oid !== after.head.oid) {
      await this.backend.reset(mode, before.head.oid);
    }
    // 2. 操作の中で別のブランチに切り替えていたら、元に戻る
    if (before.head.oid && before.head.branch !== after.head.branch) {
      await this.backend.switchTo({ ...before.head, oid: before.head.oid });
    }
    // 3. 残りの ref を戻す
    for (const [name, value] of Object.entries(before.refs)) {
      const current = after.refs[name] ?? null;
      if (done.has(name) || value === current) {
        continue;
      }
      if (name === before.head.branch && value) {
        // checkout 中のブランチは update-ref だと作業ツリーとずれるので reset で戻す
        await this.backend.reset(mode, value);
      } else {
        await this.backend.updateRef(name, value, current);
      }
    }

    entries.pop();
    await this.save();
    return entry;
  }

  private async snapshot(refs: readonly string[]): Promise<RefSnapshot> {
    const [head, values] = await Promise.all([this.backend.head(), this.backend.resolveRefs(refs)]);
    return { head, refs: Object.fromEntries(values) };
  }

  private async load(): Promise<UndoEntry[]> {
    if (!this.entries) {
      try {
        this.entries = JSON.parse(await readFile(this.file, 'utf8')) as UndoEntry[];
      } catch {
        this.entries = [];
      }
    }
    return this.entries;
  }

  private async save(): Promise<void> {
    await mkdir(dirname(this.file), { recursive: true });
    await writeFile(this.file, JSON.stringify(this.entries ?? [], null, 2));
  }
}

function sameSnapshot(a: RefSnapshot, b: RefSnapshot): boolean {
  if (a.head.branch !== b.head.branch || a.head.oid !== b.head.oid) {
    return false;
  }
  const names = new Set([...Object.keys(a.refs), ...Object.keys(b.refs)]);
  return [...names].every((n) => (a.refs[n] ?? null) === (b.refs[n] ?? null));
}
