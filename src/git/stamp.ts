/**
 * `.git` 内のファイルの更新時刻から「変わったかどうか」を判定する。spawn しない。
 * git は ref を lock ファイルの rename で更新するため、ref の追加・更新・削除は
 * どれも親ディレクトリの更新時刻に現れる。
 */
import { readdir, stat } from 'node:fs/promises';
import { join } from 'node:path';

export interface GitDirs {
  /** worktree ごとの git ディレクトリ（`HEAD` や `index` がある） */
  gitDir: string;
  /** worktree 間で共有される git ディレクトリ（`refs` や `config` がある） */
  commonDir: string;
}

async function fileStamp(path: string): Promise<string> {
  try {
    const s = await stat(path, { bigint: true });
    return `${s.mtimeNs}:${s.size}`;
  } catch {
    return '-';
  }
}

async function dirTreeStamp(dir: string): Promise<string> {
  let entries;
  try {
    entries = await readdir(dir, { withFileTypes: true });
  } catch {
    return '-';
  }
  const parts = await Promise.all([
    fileStamp(dir),
    ...entries.filter((e) => e.isDirectory()).map((e) => dirTreeStamp(join(dir, e.name))),
  ]);
  return parts.join(',');
}

/** branch・remote branch・tag と HEAD の変化 */
export async function refsStamp(dirs: GitDirs): Promise<string> {
  const parts = await Promise.all([
    fileStamp(join(dirs.gitDir, 'HEAD')),
    fileStamp(join(dirs.commonDir, 'packed-refs')),
    fileStamp(join(dirs.commonDir, 'reftable', 'tables.list')),
    dirTreeStamp(join(dirs.commonDir, 'refs')),
  ]);
  return parts.join('|');
}

/** index の変化。stage や commit、checkout で変わる */
export async function indexStamp(dirs: GitDirs): Promise<string> {
  return fileStamp(join(dirs.gitDir, 'index'));
}

/** remote と upstream の設定の変化 */
export async function configStamp(dirs: GitDirs): Promise<string> {
  const parts = await Promise.all([
    fileStamp(join(dirs.commonDir, 'config')),
    fileStamp(join(dirs.gitDir, 'config.worktree')),
  ]);
  return parts.join('|');
}

/** stash の変化。stash の一覧は `refs/stash` の reflog */
export async function stashStamp(dirs: GitDirs): Promise<string> {
  return fileStamp(join(dirs.commonDir, 'logs', 'refs', 'stash'));
}

/** `.gitmodules` の変化 */
export async function gitmodulesStamp(root: string): Promise<string> {
  return fileStamp(join(root, '.gitmodules'));
}
