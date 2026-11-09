/** メインビューの Blame と拡張ホストの間のメッセージ */

/** 行を最後に変えたコミット（コアの `blame` の答え）。まだコミットしていない行は oid に 0 が並ぶ */
export interface BlameCommit {
  oid: string;
  author: string;
  /** unix 秒 */
  time: number;
  summary: string;
}

export type BlameHostMessage =
  /** 読んだ blame。`lines` は各行の commits の添字（c）と内容（t）。`rev` がなければ作業ツリーの内容。`error` は読めなかったわけ */
  | {
      type: 'blame';
      path: string;
      rev?: string;
      commits: BlameCommit[];
      lines: { c: number; t: string }[];
      loading: boolean;
      error?: string;
    }
  /** 行ごとの構文の色（[始め, 終わり, 種類]。後から送る） */
  | { type: 'blameSyntax'; path: string; rev?: string; syntax: [number, number, number][][] };

export type BlameWebviewMessage =
  /** Blame を出した（読み直してもらう） */
  | { type: 'blameLoad' }
  /** そのコミットを History で選ぶ（ダブルクリック） */
  | { type: 'blameReveal'; oid: string };
