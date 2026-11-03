/**
 * worktree の一覧（コアの `worktrees`）と、サイドバーの Working Copy の下への並べ方（アプリと同じ決まり）。
 * vscode に依存しない
 */
import { realpathSync } from 'node:fs';
import { basename } from 'node:path';

/** コアの `worktrees` の 1 件 */
export interface Worktree {
  path: string;
  /** HEAD のコミット。まだコミットがなければ null */
  head: string | null;
  /** checkout しているブランチ（`main`）。detached なら null */
  branch: string | null;
  detached: boolean;
  bare: boolean;
  /** ロックしている（空文字は理由なし） */
  locked: string | null;
  /** フォルダがなくなっていて、prune で片付けられる（理由） */
  prunable: string | null;
  /** 一覧の先頭（`.git` を持つ元の worktree） */
  main: boolean;
}

/** シンボリックリンク（macOS の /var → /private/var など）と大文字小文字（Windows）を揃えたパス。比べるときに使う */
export function canonical(path: string): string {
  let p = path;
  try {
    p = realpathSync.native(path);
  } catch {
    // フォルダがない（prunable）
  }
  p = p.replace(/[\\/]+$/, '');
  return process.platform === 'win32' ? p.replaceAll('/', '\\').toLowerCase() : p;
}

export function samePath(a: string, b: string): boolean {
  return canonical(a) === canonical(b);
}

/** 行に出す名前（フォルダの名前） */
export function worktreeName(w: Pick<Worktree, 'path'>): string {
  return basename(w.path.replace(/[\\/]+$/, '')) || w.path;
}

/**
 * Working Copy の下に並べる worktree: メインの作業ツリー（変更がなくてもいつも出す）、書き込む先、変更のあるもの、
 * フォルダが消えた・ロックしたもの（メインを先に、残りは名前の順。w2 は w10 より前）と、「No changes」に
 * まとめるもの（2 つ以上あるときだけまとめる）。worktree が 1 つなら出さない
 */
export function worktreeLayout(
  worktrees: readonly Worktree[],
  changes: (w: Worktree) => number,
  editing: (w: Worktree) => boolean,
): { shown: Worktree[]; clean: Worktree[] } {
  if (worktrees.length < 2) return { shown: [], clean: [] };
  const ordered = [...worktrees].sort((a, b) =>
    a.main !== b.main ? (a.main ? -1 : 1) : worktreeName(a).localeCompare(worktreeName(b), undefined, { numeric: true }),
  );
  const shown: Worktree[] = [];
  const clean: Worktree[] = [];
  for (const w of ordered) {
    const always = w.main || editing(w) || w.prunable !== null || w.locked !== null;
    (always || changes(w) > 0 ? shown : clean).push(w);
  }
  return clean.length < 2 ? { shown: ordered, clean: [] } : { shown, clean };
}
