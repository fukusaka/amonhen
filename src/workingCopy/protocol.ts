/** メインビューの Working Copy と拡張ホストの間のメッセージ */
import type { Change, ChangeGroup } from '../state/changes.ts';

export interface DiffLine {
  kind: ' ' | '-' | '+';
  /** 表示用の行番号（1 始まり）。古い側 */
  old?: number;
  /** 表示用の行番号（1 始まり）。新しい側 */
  new?: number;
  text: string;
}

export interface DiffHunk {
  header: string;
  lines: DiffLine[];
}

export interface FileDiff {
  group: ChangeGroup;
  path: string;
  /** 両側の内容から作る。行単位の stage のときに、表示している差分と今の内容が同じかを確かめる */
  version: string;
  hunks: DiffHunk[];
  binary?: boolean;
  tooLarge?: boolean;
}

/** 選んだ行。kind と、その側の行番号 */
export type SelectedLine = { kind: '-'; old: number } | { kind: '+'; new: number };

export type WorkingCopyHostMessage =
  | { type: 'wcFiles'; files: Change[]; branch?: string; lastMessage?: string }
  | { type: 'wcDiff'; diff: FileDiff | undefined }
  | { type: 'wcCommitted' };

export type WorkingCopyWebviewMessage =
  | { type: 'wcSelect'; group: ChangeGroup; path: string }
  | { type: 'wcStageFiles'; op: 'stage' | 'unstage'; files: { group: ChangeGroup; path: string }[] }
  | { type: 'wcStageLines'; group: ChangeGroup; path: string; version: string; lines: SelectedLine[] }
  | { type: 'wcCommit'; message: string; amend: boolean }
  | { type: 'wcOpenFile'; path: string }
  | { type: 'wcResolve'; path: string; action: 'merge' | 'current' | 'incoming' };
