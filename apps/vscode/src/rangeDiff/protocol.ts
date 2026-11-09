/** メインビューの Range Diff と拡張ホストの間のメッセージ */
import type { RangePair } from '../git/rangeDiff.ts';

export type RangeDiffHostMessage =
  /**
   * 比べた結果。`pairs` が null なら比べている（探している）途中。`summary` は何と何を比べたか（前の版なら reflog
   * の行）、`time` は前の版にした時刻（unix 秒）、`error` は比べられなかったわけ
   */
  | { type: 'rangeDiff'; ranges: string[]; summary: string; time?: number; pairs: RangePair[] | null; error?: string };

export type RangeDiffWebviewMessage =
  /** Range Diff を出した（今の結果を送り直してもらう） */
  | { type: 'rangeDiffLoad' }
  /** 入力欄の範囲で比べる */
  | { type: 'rangeDiffCompare'; ranges: string[] }
  /** 閉じる（サイドバーの行を消す） */
  | { type: 'rangeDiffClose' };
