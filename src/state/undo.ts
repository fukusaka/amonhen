/**
 * 履歴を書き換える操作の Undo。
 * 操作の前後で HEAD と関係する ref を記録し、`<gitDir>/amonhen/undo.json` に残す。
 * Undo するときは、今の状態が記録した「操作後」と一致することを確かめてから戻す。
 */
import { createHash } from 'node:crypto';
import { chmod, mkdir, readFile, rm, stat, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import type { HeadState, RepositoryBackend } from '../git/backend.ts';

export interface RefSnapshot {
  head: HeadState;
  /** 完全な ref 名 → oid。存在しなければ null */
  refs: Record<string, string | null>;
}

/** 作業中のファイルの、操作の前後の内容（変更の破棄を Undo で戻すため） */
export interface FileBackup {
  /** リポジトリのルートからの相対パス */
  path: string;
  /** 操作の前の内容を保存した blob。ファイルが無ければ null */
  before: string | null;
  /** 操作の前のファイルのモード */
  mode?: number;
  /** 操作の後の内容の sha1。ファイルが無ければ null。Undo の前に、ほかで変わっていないかを確かめる */
  after: string | null;
}

export interface UndoEntry {
  label: string;
  time: number;
  /** 現在のブランチを戻すときの reset の種類。commit は soft、merge や rebase は keep */
  mode: 'soft' | 'keep';
  before: RefSnapshot;
  after: RefSnapshot;
  /** あれば、ref ではなく作業中のファイルを戻す操作 */
  files?: FileBackup[];
  /** あれば、消した stash を戻す操作 */
  stash?: { oid: string; message: string };
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
  private readonly pendingFile: string;
  private entries: UndoEntry[] | undefined;
  /** undefined はまだ読んでいない、null は無い */
  private pending: Omit<UndoEntry, 'after'> | null | undefined;

  constructor(backend: RepositoryBackend) {
    this.backend = backend;
    this.file = join(backend.dirs.gitDir, 'amonhen', 'undo.json');
    this.pendingFile = join(backend.dirs.gitDir, 'amonhen', 'undo-pending.json');
  }

  /** 次に Undo される操作 */
  async peek(): Promise<UndoEntry | undefined> {
    return (await this.load()).at(-1);
  }

  /**
   * fn を実行し、成功して ref が動いたら記録する。fn が失敗したときは記録しない。
   * - skip が true を返したら記録しない（コンフリクトで止まったときなど。戻すのは abort の役目）
   * - pending が true を返したら、操作の前の状態だけを保留しておき、settlePending で確定する
   *   （edit や split で rebase が途中で止まったとき）
   */
  async record<T>(
    label: string,
    refs: readonly string[],
    mode: UndoEntry['mode'],
    fn: () => Promise<T>,
    opts: { skip?: (result: T) => boolean; pending?: (result: T) => boolean } = {},
  ): Promise<T> {
    const before = await this.snapshot(refs);
    const result = await fn();
    if (opts.pending?.(result)) {
      this.pending = { label, time: Date.now(), mode, before };
      await this.savePending();
      return result;
    }
    if (opts.skip?.(result)) {
      return result;
    }
    await this.push({ label, time: Date.now(), mode, before }, await this.snapshot(refs));
    return result;
  }

  /**
   * 作業中のファイルを書き換える操作（変更の破棄）を実行し、変わったファイルの前の内容を記録する。
   * paths はリポジトリのルートからの相対パス
   */
  async recordFiles<T>(label: string, paths: readonly string[], fn: () => Promise<T>): Promise<T> {
    const root = this.backend.root;
    const before = await Promise.all(
      paths.map(async (path) => {
        const data = await readFile(join(root, path)).catch(() => undefined);
        const mode = data ? (await stat(join(root, path))).mode : undefined;
        return { path, data, mode, blob: data ? await this.backend.writeBlob(data) : null };
      }),
    );
    const result = await fn();
    const files: FileBackup[] = [];
    for (const b of before) {
      const after = await contentHash(join(root, b.path));
      if (after !== (b.data ? sha1(b.data) : null)) {
        files.push({ path: b.path, before: b.blob, mode: b.mode, after });
      }
    }
    if (files.length > 0) {
      const snapshot = await this.snapshot([]);
      const entries = await this.load();
      entries.push({ label, time: Date.now(), mode: 'keep', before: snapshot, after: snapshot, files });
      entries.splice(0, Math.max(0, entries.length - MAX_ENTRIES));
      await this.save();
    }
    return result;
  }

  /** stash を消し、Undo で戻せるように記録する */
  async recordStashDrop(name: string, oid: string, message: string): Promise<void> {
    await this.backend.stashDrop(name);
    const snapshot = await this.snapshot([]);
    const entries = await this.load();
    entries.push({
      label: `Drop ${message}`,
      time: Date.now(),
      mode: 'keep',
      before: snapshot,
      after: snapshot,
      stash: { oid, message },
    });
    entries.splice(0, Math.max(0, entries.length - MAX_ENTRIES));
    await this.save();
  }

  /** ファイルを操作の前の内容に戻す。ほかで書き換えられていたら何もしない */
  private async undoFiles(entry: UndoEntry, files: readonly FileBackup[]): Promise<void> {
    const root = this.backend.root;
    for (const f of files) {
      if ((await contentHash(join(root, f.path))) !== f.after) {
        throw new UndoConflictError(entry);
      }
    }
    for (const f of files) {
      const abs = join(root, f.path);
      if (f.before === null) {
        await rm(abs, { force: true });
        continue;
      }
      const obj = await this.backend.object(f.before);
      if (!obj) {
        throw new Error(`${f.path} の元の内容が見つかりません`);
      }
      await mkdir(dirname(abs), { recursive: true });
      await writeFile(abs, obj.data);
      if (f.mode !== undefined) await chmod(abs, f.mode & 0o777);
    }
  }

  /**
   * 保留している操作があり、rebase などがもう終わっていれば確定する。
   * abort した場合は操作の前と同じ状態なので、記録は残らない。確定したら true
   */
  async settlePending(): Promise<boolean> {
    const pending = await this.loadPending();
    if (!pending || (await this.backend.sequenceInProgress())) {
      return false;
    }
    await this.push(pending, await this.snapshot(Object.keys(pending.before.refs)));
    this.pending = null;
    await rm(this.pendingFile, { force: true });
    return true;
  }

  private async push(entry: Omit<UndoEntry, 'after'>, after: RefSnapshot): Promise<void> {
    if (sameSnapshot(entry.before, after)) {
      return;
    }
    const entries = await this.load();
    entries.push({ ...entry, after });
    entries.splice(0, Math.max(0, entries.length - MAX_ENTRIES));
    await this.save();
  }

  /** 最後の操作を戻す。戻したものを返す */
  async undo(): Promise<UndoEntry | undefined> {
    const entries = await this.load();
    const entry = entries.at(-1);
    if (!entry) {
      return undefined;
    }
    if (entry.stash) {
      await this.backend.stashStore(entry.stash.oid, entry.stash.message);
      entries.pop();
      await this.save();
      return entry;
    }
    if (entry.files) {
      await this.undoFiles(entry, entry.files);
      entries.pop();
      await this.save();
      return entry;
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

  private async loadPending(): Promise<Omit<UndoEntry, 'after'> | null> {
    if (this.pending === undefined) {
      try {
        this.pending = JSON.parse(await readFile(this.pendingFile, 'utf8')) as Omit<UndoEntry, 'after'>;
      } catch {
        this.pending = null;
      }
    }
    return this.pending;
  }

  private async savePending(): Promise<void> {
    await mkdir(dirname(this.pendingFile), { recursive: true });
    await writeFile(this.pendingFile, JSON.stringify(this.pending, null, 2));
  }

  private async save(): Promise<void> {
    await mkdir(dirname(this.file), { recursive: true });
    await writeFile(this.file, JSON.stringify(this.entries ?? [], null, 2));
  }
}

function sha1(data: Buffer): string {
  return createHash('sha1').update(data).digest('hex');
}

/** ファイルの内容の sha1。無ければ null */
async function contentHash(path: string): Promise<string | null> {
  const data = await readFile(path).catch(() => undefined);
  return data ? sha1(data) : null;
}

function sameSnapshot(a: RefSnapshot, b: RefSnapshot): boolean {
  if (a.head.branch !== b.head.branch || a.head.oid !== b.head.oid) {
    return false;
  }
  const names = new Set([...Object.keys(a.refs), ...Object.keys(b.refs)]);
  return [...names].every((n) => (a.refs[n] ?? null) === (b.refs[n] ?? null));
}
