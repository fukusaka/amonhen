/**
 * git 実行層のインターフェース。状態管理層と UI 層はこれだけに依存する。
 * 計測して足りない箇所が出たら、その部分だけ別実装（WASM やネイティブ）に差し替える。
 */
import { access, readFile, realpath, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { CatFileBatch, type GitObject } from './catFile.ts';
import { ATTRIBUTES, lineEndingsOf, parseCheckAttr, parseEolConfig, toGitForm, type LineEndings } from './eol.ts';
import { GitError, shellQuote, type GitExecutor } from './exec.ts';
import { findPreviousVersion, parseRangeDiff, type PreviousVersion, type RangePair, type ReflogLine } from './rangeDiff.ts';
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

/** 途中で止まっている操作（am はメールのパッチを当てている途中） */
export type Sequence = 'merge' | 'rebase' | 'cherry-pick' | 'revert' | 'am';

/** `git am` が止まっているパッチ */
export interface AmState {
  subject?: string;
  /** 3-way で当てようとしたか（していなければ、当て直せる） */
  threeWay: boolean;
  /** diff のないパッチ（送り状など）で止まった */
  empty: boolean;
  /** このパッチを 3-way で当て直したところ（残っているのはコンフリクトか、その解決） */
  retried: boolean;
}

/** `git format-patch` の選び方。ほかのこと（番号の付け方・宛先・署名など）は git の `format.*` に任せる */
export interface FormatPatchOptions {
  coverLetter?: boolean;
  reroll?: number;
}

/** `git apply` の当て方。既定は git と同じく作業ツリーだけ */
export interface ApplyPatchOptions {
  /** index にも当てる（`--index`） */
  index?: boolean;
  /** 当たらなければ 3-way merge にしてコンフリクトを残す（`--3way`） */
  threeWay?: boolean;
}

/**
 * `git am` が止まったパッチを 3-way で当て直したときに、その番号（`rebase-apply/next`）を書いておく（コアと同じ）。
 * am が終わるか中止されると git が rebase-apply ごと消すので、次の am に残らない
 */
const AM_RETRIED = join('rebase-apply', 'amonhen-retried');

/** merge や rebase がコンフリクトで止まったら conflict */
export type MergeResult = 'done' | 'conflict';

/** 1 つずつの merge の結果。コンフリクトで止まったら、まだ merge していないもの */
export interface OneByOneResult {
  result: MergeResult;
  remaining: string[];
  /** 途中で失敗した（済んだ分は Undo に記録してから知らせる） */
  error?: unknown;
}

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
  /** start からブランチを作る。checkout なら切り替える（作業中の変更は持っていく） */
  createBranch(name: string, start: string, checkout: boolean): Promise<void>;
  renameBranch(from: string, to: string): Promise<void>;
  /** target にタグを付ける。message があれば annotated tag */
  createTag(name: string, target: string, message?: string): Promise<void>;
  /** ブランチ名として使えるか。使えなければ git の説明を返す */
  checkBranchName(name: string): Promise<string | undefined>;
  /** コミットを現在のブランチに当てる。merge コミットなら最初の親との差分を当てる */
  cherryPick(oid: string, isMerge: boolean): Promise<RebaseResult>;
  /** コミットを打ち消すコミットを作る。merge コミットなら最初の親の側に戻す */
  revert(oid: string, isMerge: boolean): Promise<RebaseResult>;
  /** サブモジュールを初期化し、記録されているコミットに合わせる */
  updateSubmodules(paths: readonly string[]): Promise<void>;
  /** 作業中の変更を stash に保存する */
  stashPush(opts: { message?: string; includeUntracked?: boolean }): Promise<void>;
  /** stash を当てる。pop なら当てたあとで消す。コンフリクトしたら conflict（pop でも stash は残る） */
  stashApply(name: string, pop: boolean): Promise<MergeResult>;
  stashDrop(name: string): Promise<void>;
  /** 消した stash を戻す */
  stashStore(oid: string, message: string): Promise<void>;
  /** ファイル単位で stage する。削除も含む */
  stage(paths: readonly string[]): Promise<void>;
  /** ファイル単位で unstage する。HEAD がまだ無くてもよい */
  unstage(paths: readonly string[]): Promise<void>;
  /** index のエントリを指定した内容で置き換える（行単位の stage に使う） */
  writeIndex(path: string, mode: string, content: Buffer): Promise<void>;
  /**
   * 作業中のファイルを、git に入れる形で（改行の変換をかけたもの。index の blob とそのまま比べられる）。なければ undefined。
   * index はその path の index の blob（自動の改行の変換は、index に CR があればしない）
   */
  worktreeFile(path: string, index: Buffer | undefined): Promise<Buffer | undefined>;
  /** index の path の blob を、作業中のファイルに書く形で（checkout と同じ変換） */
  indexInWorktreeForm(path: string): Promise<Buffer>;
  /** 作業中のファイルの改行（index はその path の index の blob。なければ空）。ファイルがなければ undefined */
  lineEndings(path: string, index: Buffer): Promise<LineEndings | undefined>;
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
  /** 複数の ref を、すべて書き換えるか何も変えないかのどちらかで書き換える */
  updateRefs(updates: readonly { name: string; value: string | null; expected: string | null }[]): Promise<void>;
  /** 現在のブランチ（detached なら HEAD）を oid に戻す。keep は作業中の変更を残し、衝突するなら失敗する */
  reset(mode: 'soft' | 'mixed' | 'keep', oid: string): Promise<void>;
  /** branch に切り替える。branch が無ければ oid で detached HEAD にする */
  switchTo(target: HeadState & { oid: string }): Promise<void>;
  isAncestor(ancestor: string, descendant: string): Promise<boolean>;
  /** 同じ一連のコミットの 2 つの版を比べる（git range-diff。ranges は git と同じ形） */
  rangeDiff(ranges: readonly string[]): Promise<RangePair[]>;
  /** ローカルブランチ branch（完全な名前。省略時は今のブランチ）の、最後に書き換える前の版（reflog から） */
  previousVersion(branch?: string): Promise<PreviousVersion | undefined>;
  /**
   * 現在のブランチに rev を merge する。いくつも渡せば 1 つのコミットにまとめる（octopus。`git merge a b c`）。
   * octopus が途中で断ったら、作業ツリーと index を始める前に戻してから失敗する
   */
  merge(rev: string | readonly string[]): Promise<MergeResult>;
  /**
   * revs を 1 つずつ現在のブランチに merge する（それぞれが merge コミットになる）。コンフリクトで止まったら、
   * まだのものを返す
   */
  mergeOneByOne(revs: readonly string[]): Promise<OneByOneResult>;
  /**
   * revs を現在のブランチに merge したらコンフリクトするファイルを、revs の順に。いくつもあれば octopus と同じく、
   * 前のものまでを合わせた結果に重ねて調べる。作業ツリーと index には触らない（`git merge-tree --write-tree`）。
   * git が古くて調べられなければ undefined
   */
  predictMerge(revs: readonly string[]): Promise<string[][] | undefined>;
  /** branch（省略時は現在のブランチ）を onto の上に rebase する。branch を指定すると checkout される */
  rebase(onto: string, branch?: string): Promise<MergeResult>;
  sequenceInProgress(): Promise<Sequence | undefined>;
  abort(sequence: Sequence): Promise<void>;
  /** コンフリクトの解決や break のあとで続ける */
  continueSequence(sequence: Sequence): Promise<RebaseResult>;
  /** rebase などで、今のコミットを飛ばして続ける */
  skipSequence(sequence: Sequence): Promise<RebaseResult>;
  /** rebase・am の進み具合。どちらの途中でもなければ undefined */
  rebaseProgress(): Promise<{ current: number; total: number } | undefined>;
  /** am の途中なら、止まっているパッチのこと */
  amState(): Promise<AmState | undefined>;
  /**
   * コミット oids（古い順）をパッチにする（`git format-patch`。マージは git と同じく飛ばす）。dir があればそのフォルダに
   * 1 コミット 1 ファイル（git が付けた名前）、file があればそのファイルに 1 つの mbox。書いたファイルのパスを返す
   */
  formatPatch(oids: readonly string[], options: FormatPatchOptions, out: { dir?: string; file?: string }): Promise<string[]>;
  /** メールの形のパッチを今のブランチにコミットとして当てる（`git am`）。threeWay が undefined なら am.threeWay に任せる */
  am(paths: readonly string[], threeWay: boolean | undefined): Promise<RebaseResult>;
  /** `git am` が当てられずに止まったパッチを 3-way merge で当て直す。きれいに当たれば続ける */
  amThreeWay(): Promise<RebaseResult>;
  /** diff を当てる（`git apply`）。3-way でコンフリクトが残れば conflict */
  applyPatch(paths: readonly string[], options: ApplyPatchOptions): Promise<MergeResult>;
  /** oid から HEAD までを first-parent で辿ったコミット（古い順、oid を含む）。oid が HEAD の祖先でなければ undefined */
  firstParentsToHead(oid: string): Promise<string[] | undefined>;
  /** パッチがそのまま当たるか（`git apply --check`。index なら index にも）。当たらなければ git の答え */
  checkPatch(paths: readonly string[], index: boolean): Promise<string | undefined>;
  /**
   * コンフリクトしたファイルを、現在の側（HEAD）か取り込む側の内容で解決して stage する。
   * 選んだ側で削除されていれば、削除として解決する。conflict は status の XY
   */
  resolveConflicts(files: readonly { path: string; conflict: string }[], side: 'current' | 'incoming'): Promise<void>;
  /**
   * todo ファイルの内容で非対話の rebase をする。base が null なら --root。messagePath は、todo の reword・squash で
   * git が開くエディタに書かせるメッセージのファイル
   */
  rebaseTodo(base: string | null, todoPath: string, messagePath?: string | null): Promise<RebaseResult>;
  /** ローカルブランチ branch を remote の remoteBranch へ push する */
  push(remote: string, branch: string, remoteBranch: string, opts?: PushOptions): Promise<void>;
  /**
   * チェックアウトしていないローカルブランチ branch（`refs/heads/...`）を、現在の値が expected のときだけ value へ進める。
   * ほかの worktree でチェックアウトしていれば断る（その作業ツリーとずれる）
   */
  fastForward(branch: string, value: string, expected: string): Promise<void>;
  /** git の設定の値（`git config --get`）。無ければ undefined */
  config(key: string): Promise<string | undefined>;
  /** ローカルブランチ branch（短い名前）の upstream を upstream（`refs/remotes/...`）にする */
  setUpstream(branch: string, upstream: string): Promise<void>;
  fetch(): Promise<void>;
  /** revs から辿れるコミットを、トポロジカル順で skip 件飛ばして max 件 */
  log(revs: readonly string[], skip: number, max: number): Promise<Commit[]>;
  /** コミットで変わったファイル。parent が無ければ root コミットとして扱う */
  commitFiles(oid: string, parent: string | undefined): Promise<FileChange[]>;
  /** git の `mailmap.file`（絶対パス。なければ undefined） */
  mailmapFile(): Promise<string | undefined>;
  /** 名前とメールを mailmap で置き換えたもの（log.mailmap が false なら記録のまま）。同じ順 */
  mapPeople(people: readonly { name: string; email: string }[]): Promise<{ name: string; email: string }[]>;
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
/**
 * ほかの操作の途中で始めた操作を git が断ったとき（断りの文は操作と git の版でまちまち）。止まったのではないので、
 * 何の途中かを言って失敗する（コアと同じ文）
 */
function inProgress(sequence: Sequence): Error {
  return new Error(`${sequence} の途中です。Continue か Abort で終えてから、もう一度してください`);
}

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
    // `--git-dir`・`--git-common-dir` は cwd からの相対で返ることがあるので、cwd の実際の場所から解決する
    // （`--path-format=absolute` は git 2.31 から）
    const out = await git.text(['rev-parse', '--show-toplevel', '--git-dir', '--git-common-dir'], { cwd, readOnly: true });
    const base = await realpath(cwd);
    // Windows の git は `C:/…` と / で区切って返すので、その OS の形にそろえる（VSCode のパスと比べられるように）。
    const [root = '', gitDir = '', commonDir = ''] = out.split('\n').map((p) => (p ? resolve(base, p) : p));
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

  async createBranch(name: string, start: string, checkout: boolean): Promise<void> {
    const args = checkout ? ['switch', '--create', name, start] : ['branch', '--', name, start];
    await this.git.exec(args, { cwd: this.root });
  }

  async renameBranch(from: string, to: string): Promise<void> {
    await this.git.exec(['branch', '--move', '--', from, to], { cwd: this.root });
  }

  async createTag(name: string, target: string, message?: string): Promise<void> {
    const args = message ? ['tag', '--annotate', '--file=-', '--', name, target] : ['tag', '--', name, target];
    await this.git.exec(args, { cwd: this.root, input: message });
  }

  async checkBranchName(name: string): Promise<string | undefined> {
    try {
      await this.read(['check-ref-format', '--branch', name]);
      return undefined;
    } catch (err) {
      return err instanceof GitError ? err.message : String(err);
    }
  }

  cherryPick(oid: string, isMerge: boolean): Promise<RebaseResult> {
    return this.stoppable(isMerge ? ['cherry-pick', '-m', '1', oid] : ['cherry-pick', oid]);
  }

  revert(oid: string, isMerge: boolean): Promise<RebaseResult> {
    return this.stoppable(isMerge ? ['revert', '--no-edit', '-m', '1', oid] : ['revert', '--no-edit', oid]);
  }

  async updateSubmodules(paths: readonly string[]): Promise<void> {
    await this.write(['submodule', 'update', '--init', '--', ...paths]);
  }

  async stashPush(opts: { message?: string; includeUntracked?: boolean }): Promise<void> {
    const args = ['stash', 'push'];
    if (opts.includeUntracked) args.push('--include-untracked');
    if (opts.message) args.push('--message', opts.message);
    await this.git.exec(args, { cwd: this.root });
  }

  async stashApply(name: string, pop: boolean): Promise<MergeResult> {
    // stage していた状態も戻す。--index で当てられなければ、作業中のファイルにだけ当てる
    for (const args of [
      ['stash', pop ? 'pop' : 'apply', '--index', name],
      ['stash', pop ? 'pop' : 'apply', name],
    ]) {
      try {
        await this.git.exec(args, { cwd: this.root });
        return 'done';
      } catch (err) {
        const status = await this.status();
        if (status.entries.some((e) => e.kind === 'unmerged')) {
          return 'conflict';
        }
        // stage していた変更を index に戻せないときは「Try without --index」と言われる
        if (args.includes('--index') && err instanceof GitError && /without --index/.test(err.stderr)) {
          continue;
        }
        throw err;
      }
    }
    return 'done';
  }

  async stashDrop(name: string): Promise<void> {
    await this.git.exec(['stash', 'drop', name], { cwd: this.root });
  }

  async stashStore(oid: string, message: string): Promise<void> {
    await this.git.exec(['stash', 'store', '--message', message, oid], { cwd: this.root });
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

  async worktreeFile(path: string, index: Buffer | undefined): Promise<Buffer | undefined> {
    const data = await readFile(join(this.root, path)).catch(() => undefined);
    // 改行の変換は CR を取り除くだけなので、CR がなければ git に聞くまでもない
    if (!data?.includes(0x0d)) {
      return data;
    }
    const [attrs, autocrlf] = await Promise.all([
      this.read(['check-attr', '-z', ...ATTRIBUTES, '--', path]).then(parseCheckAttr),
      this.read(['config', '--get', 'core.autocrlf'], [0, 1]).then((s) => s.trim() || undefined),
    ]);
    return toGitForm(data, attrs, autocrlf, index);
  }

  async lineEndings(path: string, index: Buffer): Promise<LineEndings | undefined> {
    const raw = await readFile(join(this.root, path)).catch(() => undefined);
    if (!raw) {
      return undefined;
    }
    const [attrs, config] = await Promise.all([
      this.read(['check-attr', '-z', ...ATTRIBUTES, '--', path]).then(parseCheckAttr),
      this.read(['config', '--get-regexp', '^core\\.(autocrlf|eol)$'], [0, 1]).then(parseEolConfig),
    ]);
    return lineEndingsOf(raw, index, attrs, config);
  }

  indexInWorktreeForm(path: string): Promise<Buffer> {
    return this.git.exec(['cat-file', '--filters', `:${path}`], { cwd: this.root, readOnly: true });
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

  async updateRefs(updates: readonly { name: string; value: string | null; expected: string | null }[]): Promise<void> {
    // update-ref --stdin は全部を 1 つのトランザクションで書く。「まだ存在しないこと」は 0 の oid で表す
    const zero = '0'.repeat(40);
    const input = updates
      .map(({ name, value, expected }) =>
        value === null ? `delete ${name} ${expected ?? zero}\n` : `update ${name} ${value} ${expected ?? zero}\n`,
      )
      .join('');
    await this.git.exec(['update-ref', '--stdin'], { cwd: this.root, input });
  }

  async reset(mode: 'soft' | 'mixed' | 'keep', oid: string): Promise<void> {
    // keep は index の stat を見て、中身が同じでも書き直しただけのファイル（Undo で戻したものなど）を「変更あり」として
    // 断る。先に index を作業ツリーに合わせておく（変わったファイルが残っていれば 1 を返すが、それは構わない）
    if (mode === 'keep') {
      await this.git.exec(['update-index', '-q', '--refresh'], { cwd: this.root, okCodes: [1] });
    }
    await this.git.exec(['reset', `--${mode}`, oid], { cwd: this.root });
  }

  async switchTo(target: HeadState & { oid: string }): Promise<void> {
    const args = target.branch
      ? ['switch', target.branch.replace(/^refs\/heads\//, '')]
      : ['switch', '--detach', target.oid];
    await this.git.exec(args, { cwd: this.root });
  }

  async rangeDiff(ranges: readonly string[]): Promise<RangePair[]> {
    // 範囲を options と取り違えないように（`--end-of-options` は git 2.24 から）
    const bad = ranges.find((r) => !r || r.startsWith('-'));
    if (bad !== undefined) throw new Error(`Not a range: ${bad}`);
    const pairs = parseRangeDiff(await this.read(['range-diff', '--no-color', '--abbrev=40', ...ranges]));
    // `--abbrev=40` を見出しの ID に使わない古い git もあるので、短いものは完全な形に直す
    const short = [...new Set(pairs.flatMap((p) => [p.old, p.new]).flatMap((c) => (c && c.oid.length < 40 ? [c.oid] : [])))];
    if (short.length > 0) {
      const full = (await this.read(['rev-parse', ...short.map((s) => `${s}^{commit}`)])).trim().split('\n');
      const map = new Map(short.map((s, i) => [s, full[i] ?? s]));
      for (const c of pairs.flatMap((p) => [p.old, p.new])) {
        if (c) c.oid = map.get(c.oid) ?? c.oid;
      }
    }
    return pairs;
  }

  async previousVersion(branch?: string): Promise<PreviousVersion | undefined> {
    const ref = branch ?? (await this.head()).branch;
    if (!ref) throw new Error('HEAD isn’t on a branch');
    let entries: ReflogLine[];
    try {
      entries = await this.reflogLines(ref);
    } catch {
      return undefined;
    }
    return findPreviousVersion(
      ref.replace(/^refs\/heads\//, ''),
      entries,
      (a, d) => this.isAncestor(a, d),
      async (rev) => (await this.read(['rev-parse', '--verify', '--quiet', `${rev}^{commit}`], [0, 1])).trim() || undefined,
      () => this.reflogLines('HEAD').catch(() => []),
    );
  }

  private async reflogLines(ref: string): Promise<ReflogLine[]> {
    const out = await this.read(['log', '-g', '-n', '500', '--format=%H%x00%gs', ref, '--']);
    const rows = out.split('\n').filter(Boolean).map((l) => l.split('\0'));
    return rows.map(([oid = '', message = ''], i) => ({ new: oid, old: rows[i + 1]?.[0] ?? null, message }));
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

  async merge(rev: string | readonly string[]): Promise<MergeResult> {
    const revs = typeof rev === 'string' ? [rev] : rev;
    if (revs.length < 2) {
      return this.sequence(['merge', '--no-edit', ...revs]);
    }
    // octopus は、最後のもの以外でコンフリクトすると MERGE_HEAD を残さずに断る。git 2.37 より前は、そこまでに
    // 合わせた変更とコンフリクトを作業ツリーと index に残す（merge --abort も効かない）ので、新しい git と同じく
    // 元に戻す（作業中の変更は残る）。始める前に staged の変更があれば git は何もせずに断るので触らない
    const staged = await this.indexDiffersFromHead();
    try {
      return await this.sequence(['merge', '--no-edit', ...revs]);
    } catch (err) {
      if (!staged && (await this.indexDiffersFromHead())) {
        await this.git.exec(['reset', '--merge'], { cwd: this.root });
      }
      throw err;
    }
  }

  async mergeOneByOne(revs: readonly string[]): Promise<OneByOneResult> {
    for (const [i, rev] of revs.entries()) {
      try {
        if ((await this.merge(rev)) === 'conflict') {
          return { result: 'conflict', remaining: revs.slice(i + 1) };
        }
      } catch (error) {
        return { result: 'done', remaining: revs.slice(i), error };
      }
    }
    return { result: 'done', remaining: [] };
  }

  async predictMerge(revs: readonly string[]): Promise<string[][] | undefined> {
    const steps: string[][] = [];
    let base = 'HEAD';
    for (const [i, rev] of revs.entries()) {
      let out: string;
      try {
        out = await this.read(['merge-tree', '--write-tree', '--name-only', '--no-messages', '-z', base, rev], [0, 1]);
      } catch (err) {
        // `--write-tree` は git 2.38 から。古い git は使い方を出して 129 で終わる
        if (err instanceof GitError && err.exitCode === 129) return undefined;
        throw err;
      }
      // 出力は 木の ID、コンフリクトするファイル（NUL 区切り）
      const [tree = '', ...files] = out.split('\0');
      steps.push(files.filter(Boolean));
      if (i + 1 < revs.length) {
        // 次のものは、ここまでを合わせた結果に重ねる（octopus と同じ順）。結果は仮のコミットにする（ref は作らない）。
        // 利用者の名前や署名の設定によらないよう、名前を決めて署名しない
        const ident = { NAME: 'Amon Hen', EMAIL: 'amonhen@localhost' };
        const env = Object.fromEntries(
          ['AUTHOR', 'COMMITTER'].flatMap((who) => Object.entries(ident).map(([k, v]) => [`GIT_${who}_${k}`, v])),
        );
        const commit = await this.git.text(
          ['commit-tree', '--no-gpg-sign', '-p', base, '-p', rev, '-m', 'Amon Hen merge forecast', tree],
          { cwd: this.root, readOnly: true, env },
        );
        base = commit.trim();
      }
    }
    return steps;
  }

  /** index が HEAD と違う（staged の変更かコンフリクトがある）。まだコミットがなければ true */
  private async indexDiffersFromHead(): Promise<boolean> {
    if (await this.hasUnmerged()) return true;
    try {
      await this.read(['diff-index', '--cached', '--quiet', 'HEAD', '--']);
      return false;
    } catch {
      return true;
    }
  }

  rebase(onto: string, branch?: string): Promise<MergeResult> {
    return this.sequence(branch ? ['rebase', onto, branch] : ['rebase', onto]);
  }

  /** 失敗したとき、途中で止まっていればコンフリクトとして扱う */
  private async sequence(args: readonly string[]): Promise<MergeResult> {
    const busy = await this.sequenceInProgress();
    try {
      await this.git.exec(args, { cwd: this.root });
      return 'done';
    } catch (err) {
      if (busy) {
        throw inProgress(busy);
      }
      if (await this.sequenceInProgress()) {
        return 'conflict';
      }
      throw err;
    }
  }

  async sequenceInProgress(): Promise<Sequence | undefined> {
    const d = this.dirs.gitDir;
    // `git am` も rebase-apply を使う。am のときだけ applying がある
    if (await exists(join(d, 'rebase-apply', 'applying'))) return 'am';
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
    return this.stoppable([sequence, '--continue'], undefined, true);
  }

  skipSequence(sequence: Sequence): Promise<RebaseResult> {
    return this.stoppable([sequence, '--skip'], undefined, true);
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

  async amState(): Promise<AmState | undefined> {
    const dir = join(this.dirs.gitDir, 'rebase-apply');
    if (!(await exists(join(dir, 'applying')))) return undefined;
    const read = (path: string): Promise<string> => readFile(path, 'utf8').then((t) => t.trim(), () => '');
    const [info, message, threeWay, patch, next, retried] = await Promise.all([
      read(join(dir, 'info')),
      read(join(dir, 'final-commit')),
      read(join(dir, 'threeway')),
      read(join(dir, 'patch')),
      read(join(dir, 'next')),
      read(join(this.dirs.gitDir, AM_RETRIED)),
    ]);
    const subject = /^Subject: (.*)$/m.exec(info)?.[1] ?? (message.split('\n')[0] || undefined);
    return { subject, threeWay: threeWay === 't', empty: patch === '', retried: retried !== '' && retried === next };
  }

  async formatPatch(
    oids: readonly string[],
    options: FormatPatchOptions,
    out: { dir?: string; file?: string },
  ): Promise<string[]> {
    if (oids.length === 0) return [];
    // format-patch は受け取った並びの逆の順に書き出す。日付で並べ替えさせず（--no-walk=unsorted）、逆にして渡す。数を
    // 渡さないと、コミットが 1 つのときに `<rev>..HEAD` の意味になる
    const args = ['format-patch', '--no-walk=unsorted', `--max-count=${oids.length}`];
    if (options.coverLetter) args.push('--cover-letter');
    if (options.reroll) args.push(`--reroll-count=${options.reroll}`);
    const revs = [...oids].reverse();
    if (out.file) {
      const content = await this.git.exec([...args, '--stdout', ...revs], { cwd: this.root, readOnly: true });
      await writeFile(out.file, content);
      return [out.file];
    }
    if (!out.dir) throw new Error('パッチを書き出すフォルダかファイルを選んでください');
    const written = await this.read([...args, '--output-directory', out.dir, ...revs]);
    return written.split('\n').filter(Boolean).map((p) => resolve(this.root, p));
  }

  am(paths: readonly string[], threeWay: boolean | undefined): Promise<RebaseResult> {
    const args = ['am'];
    if (threeWay !== undefined) args.push(threeWay ? '--3way' : '--no-3way');
    return this.stoppable([...args, '--', ...paths]);
  }

  async amThreeWay(): Promise<RebaseResult> {
    // `git am -3` と同じく、止まったパッチ（rebase-apply/patch）を index の blob を使って 3-way で当てる
    try {
      await this.git.exec(['apply', '--3way', join(this.dirs.gitDir, 'rebase-apply', 'patch')], { cwd: this.root });
    } catch (err) {
      if (!(await this.hasUnmerged())) throw err;
      const next = await readFile(join(this.dirs.gitDir, 'rebase-apply', 'next'), 'utf8').catch(() => '');
      await writeFile(join(this.dirs.gitDir, AM_RETRIED), next);
      return 'conflict';
    }
    return this.continueSequence('am');
  }

  async applyPatch(paths: readonly string[], options: ApplyPatchOptions): Promise<MergeResult> {
    const args = ['apply'];
    if (options.threeWay) args.push('--3way');
    else if (options.index) args.push('--index');
    try {
      await this.git.exec([...args, '--', ...paths], { cwd: this.root });
    } catch (err) {
      // 3-way でコンフリクトを残して当てた（終了コードは 1）
      if (options.threeWay && (await this.hasUnmerged())) return 'conflict';
      throw err;
    }
    return 'done';
  }

  async firstParentsToHead(oid: string): Promise<string[] | undefined> {
    if (!(await this.isAncestor(oid, 'HEAD'))) return undefined;
    const later = (await this.read(['rev-list', '--first-parent', '--reverse', `${oid}..HEAD`])).split('\n').filter(Boolean);
    return [oid, ...later];
  }

  async checkPatch(paths: readonly string[], index: boolean): Promise<string | undefined> {
    try {
      await this.read(['apply', '--check', ...(index ? ['--index'] : []), '--', ...paths]);
      return undefined;
    } catch (err) {
      return err instanceof Error ? err.message : String(err);
    }
  }

  private async hasUnmerged(): Promise<boolean> {
    return (await this.read(['ls-files', '--unmerged', '-z'])) !== '';
  }

  async rebaseTodo(base: string | null, todoPath: string, messagePath?: string | null): Promise<RebaseResult> {
    // git は `$GIT_SEQUENCE_EDITOR <todo>` を sh で実行するので、用意した todo を上書きコピーさせる。reword・squash の
    // メッセージも同じく、`$GIT_EDITOR <file>` でファイルを上書きコピーさせる
    const env: Record<string, string> = { GIT_SEQUENCE_EDITOR: `cp ${shellQuote(todoPath)}` };
    const config: string[] = [];
    if (messagePath) {
      env.GIT_EDITOR = `cp ${shellQuote(messagePath)}`;
      // エディタで書いたメッセージは、git の既定では # で始まる行を除く。コミット（-F）と同じく、書いたとおりに
      // 残す。commit.cleanup を設定していれば、それに従う
      const cleanup = await this.config('commit.cleanup');
      if (cleanup === undefined || cleanup === 'default') {
        config.push('-c', 'commit.cleanup=whitespace');
      }
    }
    return this.stoppable([...config, 'rebase', '--interactive', '--autostash', '--no-autosquash', base ?? '--root'], env);
  }

  /**
   * 成功しても途中で止まっていれば stopped、失敗して止まっていれば conflict。resuming でなければ（始める操作なら）、
   * 始める前から途中だった操作を、新しい操作のコンフリクトと取り違えない
   */
  private async stoppable(args: readonly string[], env?: Record<string, string>, resuming = false): Promise<RebaseResult> {
    const busy = resuming ? undefined : await this.sequenceInProgress();
    try {
      await this.git.exec(args, { cwd: this.root, env });
    } catch (err) {
      if (busy) {
        throw inProgress(busy);
      }
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

  async fastForward(branch: string, value: string, expected: string): Promise<void> {
    const list = await this.read(['worktree', 'list', '--porcelain']);
    const elsewhere = list.split('\n\n').find((w) => w.split('\n').includes(`branch ${branch}`));
    if (elsewhere) {
      const path = (elsewhere.split('\n')[0] ?? '').replace(/^worktree /, '');
      throw new Error(`${branch.replace(/^refs\/heads\//, '')} は worktree ${path} でチェックアウトしています。そちらで取り込んでください`);
    }
    await this.updateRef(branch, value, expected);
  }

  async config(key: string): Promise<string | undefined> {
    return (await this.read(['config', '--get', key], [0, 1])).trim() || undefined;
  }

  async setUpstream(branch: string, upstream: string): Promise<void> {
    await this.git.exec(['branch', `--set-upstream-to=${upstream}`, branch], { cwd: this.root });
  }

  async fetch(): Promise<void> {
    await this.git.exec(['fetch', '--all', '--prune'], { cwd: this.root });
  }

  async log(revs: readonly string[], skip: number, max: number): Promise<Commit[]> {
    const [out, mailmap] = await Promise.all([this.read(logArgs(revs, skip, max)), this.usesMailmap()]);
    return parseLog(out, mailmap);
  }

  async mailmapFile(): Promise<string | undefined> {
    // `--path` で `~` を展開してもらう。相対パスは git と同じく作業ツリーから
    const path = (await this.read(['config', '--path', '--get', 'mailmap.file'], [0, 1])).trim();
    return path ? resolve(this.root, path) : undefined;
  }

  /** 作者とコミッターを mailmap で置き換えるか（git の log.mailmap。既定は true） */
  async usesMailmap(): Promise<boolean> {
    const value = (await this.read(['config', '--type=bool', '--get', 'log.mailmap'], [0, 1])).trim();
    return value !== 'false';
  }

  /**
   * 名前とメールを mailmap で置き換える（`git check-mailmap`。log.mailmap が false なら置き換えない）。
   * 置き換えたものを同じ順に返す
   */
  async mapPeople(people: readonly { name: string; email: string }[]): Promise<{ name: string; email: string }[]> {
    if (people.length === 0 || !(await this.usesMailmap())) {
      return people.map(({ name, email }) => ({ name, email }));
    }
    const out = await this.read(['check-mailmap', ...people.map((p) => `${p.name} <${p.email}>`)]);
    const lines = out.split('\n');
    return people.map((p, i) => {
      const m = /^(.*) <(.*)>$/.exec(lines[i] ?? '');
      return m ? { name: m[1]!, email: m[2]! } : { name: p.name, email: p.email };
    });
  }

  async commitFiles(oid: string, parent: string | undefined): Promise<FileChange[]> {
    return parseNameStatus(await this.read(diffTreeArgs(oid, parent)));
  }

  /** パスに `*` などが含まれても pathspec として解釈させない */
  private write(args: readonly string[]): Promise<Buffer> {
    return this.git.exec(['--literal-pathspecs', ...args], { cwd: this.root });
  }

  /** 常駐の cat-file を終わらせる。返す Promise は終わったら解決する（作業ディレクトリを消す前に待つ） */
  dispose(): Promise<void> {
    return this.catFile.dispose();
  }
}
