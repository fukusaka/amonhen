/**
 * git 実行層のインターフェース。状態管理層と UI 層はこれだけに依存する。
 * 計測して足りない箇所が出たら、その部分だけ別実装（WASM やネイティブ）に差し替える。
 */
import { access } from 'node:fs/promises';
import { join } from 'node:path';
import { CatFileBatch, type GitObject } from './catFile.ts';
import type { GitExecutor } from './exec.ts';
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
import { parseStatus, STATUS_ARGS, type StatusResult } from './status.ts';

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
  dispose(): void;
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
    this.catFile = new CatFileBatch(git.path, root);
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

  dispose(): void {
    this.catFile.dispose();
  }
}
