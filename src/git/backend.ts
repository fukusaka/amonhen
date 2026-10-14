/**
 * git 実行層のインターフェース。状態管理層と UI 層はこれだけに依存する。
 * 計測して足りない箇所が出たら、その部分だけ別実装（WASM やネイティブ）に差し替える。
 */
import { access, readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { CatFileBatch, type GitObject } from './catFile.ts';
import { GitError, shellQuote, type GitExecutor } from './exec.ts';
import { diffTreeArgs, logArgs, parseLog, parseNameStatus, type Commit, type FileChange } from './log.ts';
import {
  forEachRefArgs,
  parseRefs,
  parseRemotes,
  parseStashes,
  parseSubmoduleConfig,
  REMOTE_ARGS,
  STASH_ARGS,
  SUBMODULE_CONFIG_ARGS,
  type Ref,
  type Remote,
  type Stash,
  type SubmoduleConfig,
} from './refs.ts';
import type { GitDirs } from './stamp.ts';
import { conflictSides, parseStatus, STATUS_ARGS, type StatusResult } from './status.ts';

export interface HeadState {
  /** `refs/heads/main` など。detached HEAD のときは undefined */
  branch?: string;
  /** まだコミットが無いときは undefined */
  oid?: string;
}

/** 途中で止まっている操作 */
export type Sequence = 'merge' | 'rebase' | 'cherry-pick' | 'revert';

/** merge や rebase がコンフリクトで止まったら conflict */
export type MergeResult = 'done' | 'conflict';

/** todo の break で止まったら stopped */
export type RebaseResult = MergeResult | 'stopped';

export interface PushOptions {
  /** `--force-with-lease`。rebase したブランチを上書きするとき */
  force?: boolean;
  /** upstream に設定する */
  setUpstream?: boolean;
}

export interface RepositoryBackend {
  readonly root: string;
  readonly dirs: GitDirs;
  status(): Promise<StatusResult>;
  /** `refs/heads` などのパターンに一致する ref */
  refs(patterns: readonly string[]): Promise<Ref[]>;
  remotes(): Promise<Remote[]>;
  stashes(): Promise<Stash[]>;
  submodules(): Promise<SubmoduleConfig[]>;
  object(rev: string): Promise<GitObject | undefined>;
  switchBranch(name: string): Promise<void>;
  /** リモートブランチ（`origin/x`）から、同じ名前で追跡するローカルブランチを作って切り替える */
  trackBranch(remoteBranch: string): Promise<void>;
  /** ローカルブランチを削除する。force でなければ merge されていないと失敗する */
  deleteBranch(name: string, force: boolean): Promise<void>;
  /** ファイル単位で stage する。削除も含む */
  stage(paths: readonly string[]): Promise<void>;
  /** ファイル単位で unstage する。HEAD がまだ無くてもよい */
  unstage(paths: readonly string[]): Promise<void>;
  /** index のエントリを指定した内容で置き換える（行単位の stage に使う） */
  writeIndex(path: string, mode: string, content: Buffer): Promise<void>;
  /** 内容を blob として保存し、oid を返す（破棄した内容を Undo で戻すために残す） */
  writeBlob(content: Buffer): Promise<string>;
  /** 作業中のファイルを index の内容に戻す（stage していない変更を破棄する） */
  restoreWorktree(paths: readonly string[]): Promise<void>;
  /** amend なら直前のコミットを置き換える（stage したものと message で） */
  commit(message: string, opts?: { amend?: boolean }): Promise<void>;
  head(): Promise<HeadState>;
  /** 完全な名前の ref の現在の値。存在しなければ null */
  resolveRefs(names: readonly string[]): Promise<Map<string, string | null>>;
  /** 現在の値が expected のときだけ書き換える。value が null なら削除する */
  updateRef(name: string, value: string | null, expected: string | null): Promise<void>;
  /** 現在のブランチ（detached なら HEAD）を oid に戻す。keep は作業中の変更を残し、衝突するなら失敗する */
  reset(mode: 'soft' | 'keep', oid: string): Promise<void>;
  /** branch に切り替える。branch が無ければ oid で detached HEAD にする */
  switchTo(target: HeadState & { oid: string }): Promise<void>;
  isAncestor(ancestor: string, descendant: string): Promise<boolean>;
  /** 現在のブランチに rev を merge する */
  merge(rev: string): Promise<MergeResult>;
  /** branch（省略時は現在のブランチ）を onto の上に rebase する。branch を指定すると checkout される */
  rebase(onto: string, branch?: string): Promise<MergeResult>;
  sequenceInProgress(): Promise<Sequence | undefined>;
  abort(sequence: Sequence): Promise<void>;
  /** コンフリクトの解決や break のあとで続ける */
  continueSequence(sequence: Sequence): Promise<RebaseResult>;
  /** rebase などで、今のコミットを飛ばして続ける */
  skipSequence(sequence: Sequence): Promise<RebaseResult>;
  /** rebase の進み具合。rebase 中でなければ undefined */
  rebaseProgress(): Promise<{ current: number; total: number } | undefined>;
  /**
   * コンフリクトしたファイルを、現在の側（HEAD）か取り込む側の内容で解決して stage する。
   * 選んだ側で削除されていれば、削除として解決する。conflict は status の XY
   */
  resolveConflicts(files: readonly { path: string; conflict: string }[], side: 'current' | 'incoming'): Promise<void>;
  /** todo ファイルの内容で非対話の rebase をする。base が null なら --root */
  rebaseTodo(base: string | null, todoPath: string): Promise<RebaseResult>;
  /** ローカルブランチ branch を remote の remoteBranch へ push する */
  push(remote: string, branch: string, remoteBranch: string, opts?: PushOptions): Promise<void>;
  fetch(): Promise<void>;
  /** revs から辿れるコミットを、トポロジカル順で skip 件飛ばして max 件 */
  log(revs: readonly string[], skip: number, max: number): Promise<Commit[]>;
  /** コミットで変わったファイル。parent が無ければ root コミットとして扱う */
  commitFiles(oid: string, parent: string | undefined): Promise<FileChange[]>;
  dispose(): void;
}

async function exists(path: string): Promise<boolean> {
  try {
    await access(path);
    return true;
  } catch {
    return false;
  }
}

/** `git` を spawn する実装 */
export class CliBackend implements RepositoryBackend {
  readonly root: string;
  readonly dirs: GitDirs;
  private readonly git: GitExecutor;
  private readonly catFile: CatFileBatch;

  private constructor(git: GitExecutor, root: string, dirs: GitDirs) {
    this.git = git;
    this.root = root;
    this.dirs = dirs;
    this.catFile = new CatFileBatch(git.path, root, join(dirs.gitDir, 'index'));
  }

  static async open(git: GitExecutor, cwd: string): Promise<CliBackend> {
    const out = await git.text(
      ['rev-parse', '--path-format=absolute', '--show-toplevel', '--git-dir', '--git-common-dir'],
      { cwd, readOnly: true },
    );
    const [root = '', gitDir = '', commonDir = ''] = out.split('\n');
    return new CliBackend(git, root, { gitDir, commonDir });
  }

  private read(args: readonly string[], okCodes?: readonly number[]): Promise<string> {
    return this.git.text(args, { cwd: this.root, readOnly: true, okCodes });
  }

  async status(): Promise<StatusResult> {
    return parseStatus(await this.read(STATUS_ARGS));
  }

  async refs(patterns: readonly string[]): Promise<Ref[]> {
    return parseRefs(await this.read(forEachRefArgs(patterns)));
  }

  async remotes(): Promise<Remote[]> {
    return parseRemotes(await this.read(REMOTE_ARGS));
  }

  async stashes(): Promise<Stash[]> {
    return parseStashes(await this.read(STASH_ARGS));
  }

  async submodules(): Promise<SubmoduleConfig[]> {
    try {
      await access(join(this.root, '.gitmodules'));
    } catch {
      return [];
    }
    // 該当するキーが無いと終了コード 1
    return parseSubmoduleConfig(await this.read(SUBMODULE_CONFIG_ARGS, [1]));
  }

  object(rev: string): Promise<GitObject | undefined> {
    return this.catFile.contents(rev);
  }

  async switchBranch(name: string): Promise<void> {
    // branch 名は `-` で始められないので、オプションと誤認されることはない
    await this.git.exec(['switch', name], { cwd: this.root });
  }

  async trackBranch(remoteBranch: string): Promise<void> {
    await this.git.exec(['switch', '--track', remoteBranch], { cwd: this.root });
  }

  async deleteBranch(name: string, force: boolean): Promise<void> {
    await this.git.exec(['branch', force ? '-D' : '-d', '--', name], { cwd: this.root });
  }

  async stage(paths: readonly string[]): Promise<void> {
    await this.write(['add', '-A', '--', ...paths]);
  }

  async unstage(paths: readonly string[]): Promise<void> {
    await this.write(['reset', '-q', '--', ...paths]);
  }

  async writeIndex(path: string, mode: string, content: Buffer): Promise<void> {
    // 内容は index から作ったものなので、clean フィルターは通さない
    const oid = await this.writeBlob(content);
    await this.write(['update-index', '--add', '--cacheinfo', `${mode},${oid},${path}`]);
  }

  async writeBlob(content: Buffer): Promise<string> {
    return (
      await this.git.text(['hash-object', '-w', '--no-filters', '--stdin'], { cwd: this.root, input: content })
    ).trim();
  }

  async restoreWorktree(paths: readonly string[]): Promise<void> {
    await this.write(['checkout', '--', ...paths]);
  }

  async commit(message: string, opts: { amend?: boolean } = {}): Promise<void> {
    const args = opts.amend ? ['commit', '--amend', '--file=-'] : ['commit', '--file=-'];
    await this.git.exec(args, { cwd: this.root, input: message });
  }

  async head(): Promise<HeadState> {
    const [branch, oid] = await Promise.all([
      this.read(['symbolic-ref', '-q', 'HEAD'], [1]),
      this.read(['rev-parse', '-q', '--verify', 'HEAD'], [1]),
    ]);
    const head: HeadState = {};
    if (branch.trim()) head.branch = branch.trim();
    if (oid.trim()) head.oid = oid.trim();
    return head;
  }

  async resolveRefs(names: readonly string[]): Promise<Map<string, string | null>> {
    const result = new Map<string, string | null>(names.map((n) => [n, null]));
    if (names.length === 0) {
      return result;
    }
    // for-each-ref のパターンは前方一致（refs/heads/a で refs/heads/a/b も一致）なので、完全一致だけ拾う
    const out = await this.read(['for-each-ref', '--format=%(refname)%00%(objectname)', ...names]);
    for (const line of out.split('\n')) {
      const [name = '', oid = ''] = line.split('\0');
      if (result.has(name)) {
        result.set(name, oid);
      }
    }
    return result;
  }

  async updateRef(name: string, value: string | null, expected: string | null): Promise<void> {
    const args =
      value === null
        ? ['update-ref', '-d', name, expected ?? '']
        : // expected が空文字なら「まだ存在しないこと」を条件にする
          ['update-ref', name, value, expected ?? ''];
    await this.git.exec(args, { cwd: this.root });
  }

  async reset(mode: 'soft' | 'keep', oid: string): Promise<void> {
    await this.git.exec(['reset', `--${mode}`, oid], { cwd: this.root });
  }

  async switchTo(target: HeadState & { oid: string }): Promise<void> {
    const args = target.branch
      ? ['switch', target.branch.replace(/^refs\/heads\//, '')]
      : ['switch', '--detach', target.oid];
    await this.git.exec(args, { cwd: this.root });
  }

  async isAncestor(ancestor: string, descendant: string): Promise<boolean> {
    try {
      await this.read(['merge-base', '--is-ancestor', ancestor, descendant]);
      return true;
    } catch (err) {
      if (err instanceof GitError && err.exitCode === 1) {
        return false;
      }
      throw err;
    }
  }

  merge(rev: string): Promise<MergeResult> {
    return this.sequence(['merge', '--no-edit', rev]);
  }

  rebase(onto: string, branch?: string): Promise<MergeResult> {
    return this.sequence(branch ? ['rebase', onto, branch] : ['rebase', onto]);
  }

  /** 失敗したとき、途中で止まっていればコンフリクトとして扱う */
  private async sequence(args: readonly string[]): Promise<MergeResult> {
    try {
      await this.git.exec(args, { cwd: this.root });
      return 'done';
    } catch (err) {
      if (await this.sequenceInProgress()) {
        return 'conflict';
      }
      throw err;
    }
  }

  async sequenceInProgress(): Promise<Sequence | undefined> {
    const d = this.dirs.gitDir;
    if ((await exists(join(d, 'rebase-merge'))) || (await exists(join(d, 'rebase-apply')))) return 'rebase';
    if (await exists(join(d, 'MERGE_HEAD'))) return 'merge';
    if (await exists(join(d, 'CHERRY_PICK_HEAD'))) return 'cherry-pick';
    if (await exists(join(d, 'REVERT_HEAD'))) return 'revert';
    return undefined;
  }

  async abort(sequence: Sequence): Promise<void> {
    await this.git.exec([sequence, '--abort'], { cwd: this.root });
  }

  continueSequence(sequence: Sequence): Promise<RebaseResult> {
    return this.stoppable([sequence, '--continue']);
  }

  skipSequence(sequence: Sequence): Promise<RebaseResult> {
    return this.stoppable([sequence, '--skip']);
  }

  async rebaseProgress(): Promise<{ current: number; total: number } | undefined> {
    for (const dir of ['rebase-merge', 'rebase-apply']) {
      const base = join(this.dirs.gitDir, dir);
      try {
        const [current, total] = await Promise.all(
          dir === 'rebase-merge'
            ? [readFile(join(base, 'msgnum'), 'utf8'), readFile(join(base, 'end'), 'utf8')]
            : [readFile(join(base, 'next'), 'utf8'), readFile(join(base, 'last'), 'utf8')],
        );
        return { current: Number(current.trim()), total: Number(total.trim()) };
      } catch {
        // この形式の rebase ではない
      }
    }
    return undefined;
  }

  async resolveConflicts(
    files: readonly { path: string; conflict: string }[],
    side: 'current' | 'incoming',
  ): Promise<void> {
    const keep: string[] = [];
    const remove: string[] = [];
    for (const f of files) {
      const sides = conflictSides(f.conflict);
      (side === 'current' ? sides.current : sides.incoming) ? keep.push(f.path) : remove.push(f.path);
    }
    if (keep.length > 0) {
      await this.write(['checkout', side === 'current' ? '--ours' : '--theirs', '--', ...keep]);
      await this.write(['add', '--', ...keep]);
    }
    if (remove.length > 0) {
      await this.write(['rm', '--quiet', '--', ...remove]);
    }
  }

  rebaseTodo(base: string | null, todoPath: string): Promise<RebaseResult> {
    // git は `$GIT_SEQUENCE_EDITOR <todo>` を sh で実行するので、用意した todo を上書きコピーさせる
    return this.stoppable(['rebase', '--interactive', '--autostash', '--no-autosquash', base ?? '--root'], {
      GIT_SEQUENCE_EDITOR: `cp ${shellQuote(todoPath)}`,
    });
  }

  /** 成功しても途中で止まっていれば stopped、失敗して止まっていれば conflict */
  private async stoppable(args: readonly string[], env?: Record<string, string>): Promise<RebaseResult> {
    try {
      await this.git.exec(args, { cwd: this.root, env });
    } catch (err) {
      if (await this.sequenceInProgress()) {
        return 'conflict';
      }
      throw err;
    }
    return (await this.sequenceInProgress()) ? 'stopped' : 'done';
  }

  async push(remote: string, branch: string, remoteBranch: string, opts: PushOptions = {}): Promise<void> {
    const args = ['push'];
    if (opts.force) args.push('--force-with-lease');
    if (opts.setUpstream) args.push('--set-upstream');
    args.push(remote, `refs/heads/${branch}:refs/heads/${remoteBranch}`);
    await this.git.exec(args, { cwd: this.root });
  }

  async fetch(): Promise<void> {
    await this.git.exec(['fetch', '--all', '--prune'], { cwd: this.root });
  }

  async log(revs: readonly string[], skip: number, max: number): Promise<Commit[]> {
    return parseLog(await this.read(logArgs(revs, skip, max)));
  }

  async commitFiles(oid: string, parent: string | undefined): Promise<FileChange[]> {
    return parseNameStatus(await this.read(diffTreeArgs(oid, parent)));
  }

  /** パスに `*` などが含まれても pathspec として解釈させない */
  private write(args: readonly string[]): Promise<Buffer> {
    return this.git.exec(['--literal-pathspecs', ...args], { cwd: this.root });
  }

  dispose(): void {
    this.catFile.dispose();
  }
}
