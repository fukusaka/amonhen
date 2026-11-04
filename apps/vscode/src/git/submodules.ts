/**
 * submodule の様子（コアの `submodules`）と、サイドバーの行の印・ツールチップ（アプリのサイドバーと同じ決まり）。
 * vscode に依存しない
 */

/** コアの `submodules` の 1 件 */
export interface SubmoduleFact {
  path: string;
  name: string;
  url: string | null;
  /** 追うブランチ（`.gitmodules` の `branch`。なければ null） */
  branch: string | null;
  /** 取り出してある */
  initialized: boolean;
  /** 記録したコミットと違うコミットにいる */
  moved: boolean;
  /** 中に変更がある */
  changed: boolean;
  conflict: boolean;
}

/** 行の右に出す印（アプリと同じ順: 取り出していない・違うコミット・中の変更・コンフリクト） */
export function submoduleMarks(f: SubmoduleFact): string[] {
  if (f.conflict) return ['in conflict'];
  if (!f.initialized) return ['not checked out'];
  const marks: string[] = [];
  if (f.moved) marks.push('another commit');
  if (f.changed) marks.push('changes inside');
  return marks;
}

/** ツールチップ: URL と追うブランチ */
export function submoduleTooltip(f: SubmoduleFact): string {
  return [
    f.path,
    f.url ? `URL: ${f.url}` : undefined,
    f.branch ? `Follows ${f.branch}` : 'Follows the recorded commit',
  ]
    .filter(Boolean)
    .join('\n');
}

/** 足すときのフォルダの既定: URL の最後の部分から `.git` を除いたもの（git submodule add と同じ） */
export function folderFromUrl(url: string): string {
  const last = url.trim().replace(/[\\/]+$/, '').split(/[\\/:]/).pop() ?? '';
  return last.replace(/\.git$/, '');
}
