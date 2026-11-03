/** メインビューの File History と拡張ホストの間のメッセージ */

/** ファイルを変えたコミット（新しい順。コアの `fileHistory` の答え） */
export interface FileHistoryEntry {
  oid: string;
  subject: string;
  /** mailmap の名前（あれば） */
  author: string;
  /** unix 秒 */
  time: number;
  /** そのコミットの時点のパス（名前を変えていれば今と違う） */
  path: string;
}

export type FileHistoryHostMessage =
  /** 読んだ履歴。`complete` でなければ、まだ先を読んでいる。`error` は読めなかったわけ */
  | {
      type: 'fileHistory';
      path: string;
      entries: FileHistoryEntry[];
      complete: boolean;
      merges: boolean;
      error?: string;
    };

export type FileHistoryWebviewMessage =
  /** File History を出した、マージも出す・出さないを変えた（読み直してもらう）。`merges` はページのチェック */
  | { type: 'fileHistoryLoad'; merges: boolean }
  /** そのコミットを History で選ぶ（ダブルクリック・Enter） */
  | { type: 'fileHistoryReveal'; oid: string };
