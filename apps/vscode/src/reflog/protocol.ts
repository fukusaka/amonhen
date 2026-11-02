/** メインビューの Reflog と拡張ホストの間のメッセージ */

/** reflog の 1 行（新しい順。コアの `reflog` の答え） */
export interface ReflogEntry {
  /** `HEAD@{0}` */
  selector: string;
  /** 動いた先のコミット */
  oid: string;
  /** git の reflog の行（`commit: …`、`checkout: moving from a to b`、`reset: moving to …`） */
  message: string;
  /** そのコミットの件名（読めなければ空） */
  subject: string;
  /** unix 秒 */
  time: number;
  /** ref から辿れる（History にある） */
  reachable: boolean;
}

export type ReflogHostMessage =
  /** 読み直した reflog。`error` は読めなかったわけ */
  | { type: 'reflog'; entries: ReflogEntry[]; error?: string };

export type ReflogWebviewMessage =
  /** Reflog を出した（読み直してもらう） */
  | { type: 'reflogLoad' }
  /** 一覧の上のボタン */
  | { type: 'reflogAction'; action: 'newBranch' | 'checkout'; oid: string; selector: string };
