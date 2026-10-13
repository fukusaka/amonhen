/** メインビューの Working Copy と拡張ホストの間のメッセージ */
import type { FileDiff, SelectedLine } from '../diff/fileDiff.ts';
import type { Change, ChangeGroup } from '../state/changes.ts';

export type WorkingCopyHostMessage =
  | { type: 'wcFiles'; files: Change[]; branch?: string; lastMessage?: string }
  /** 選んでいるファイルの差分。group は今そのファイルがあるグループ */
  | { type: 'wcDiff'; group: ChangeGroup; diff: FileDiff }
  | { type: 'wcDiff'; group?: undefined; diff: undefined }
  | { type: 'wcCommitted' };

export type WorkingCopyWebviewMessage =
  | { type: 'wcSelect'; group: ChangeGroup; path: string }
  | { type: 'wcStageFiles'; op: 'stage' | 'unstage'; files: { group: ChangeGroup; path: string }[] }
  | { type: 'wcStageLines'; group: ChangeGroup; path: string; version: string; lines: SelectedLine[] }
  | { type: 'wcCommit'; message: string; amend: boolean }
  | { type: 'wcOpenFile'; path: string }
  | { type: 'wcResolve'; path: string; action: 'merge' | 'current' | 'incoming' };
