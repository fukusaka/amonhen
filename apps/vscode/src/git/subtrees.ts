/**
 * subtree（コアの `subtrees`）と、サイドバーの行のツールチップ（アプリのサイドバーと同じ決まり）。vscode に依存しない
 */

/** コアの `subtrees` の 1 件 */
export interface Subtree {
  /** 取り込んだフォルダ */
  dir: string;
  /** 最後に記録した取り込みが squash だった */
  squashed: boolean;
  /** 覚えている Pull の相手（Push も同じ URL） */
  url: string | null;
  ref: string | null;
  /** 覚えている Push の先の ref */
  pushRef: string | null;
  /** 前に選んだ squash（なければ git の既定のオフ） */
  squash: boolean | null;
}

export interface SubtreesInfo {
  /** この機械の git に `git subtree` がある */
  available: boolean;
  subtrees: Subtree[];
}

/** ツールチップ: 最後に使ったリポジトリとブランチ、最後の取り込みが squash だったか */
export function subtreeTooltip(s: Subtree): string {
  return [
    s.dir,
    s.url ? `From ${s.url}${s.ref ? ` (${s.ref})` : ''}` : 'Not pulled from here yet',
    s.pushRef && s.pushRef !== s.ref ? `Pushes to ${s.pushRef}` : undefined,
    s.squashed ? 'The last import was squashed' : 'The last import kept the history',
  ]
    .filter(Boolean)
    .join('\n');
}

/** Pull の squash の既定: 前に選んだもの、なければ git の既定のオフ */
export function defaultSquash(s: Pick<Subtree, 'squash'> | undefined): boolean {
  return s?.squash ?? false;
}
