/** メインビューの Working Copy と拡張ホストの間のメッセージ */
import type { FileDiff, SelectedLine } from '../diff/fileDiff.ts';
import type { Change, ChangeGroup } from '../state/changes.ts';
import type { MessageParts } from './commitMessage.ts';
import type { FileLayout } from './fileList.ts';

export type WorkingCopyHostMessage =
  | { type: 'wcFiles'; files: Change[]; branch?: string; lastMessage?: string; layout: FileLayout }
  /** 選んでいるファイルの差分。group は今そのファイルがあるグループ */
  | { type: 'wcDiff'; group: ChangeGroup; diff: FileDiff }
  | { type: 'wcDiff'; group?: undefined; diff: undefined }
  /** リポジトリを開いたときのコミット欄の中身（下書き、なければテンプレート） */
  | ({ type: 'wcMessage' } & MessageParts)
  | { type: 'wcCommitted' };

export type WorkingCopyWebviewMessage =
  | { type: 'wcSelect'; group: ChangeGroup; path: string }
  /** ファイル一覧の見せ方を変えた（次に開くときのために覚える） */
  | { type: 'wcLayout'; layout: FileLayout }
  /** コミット欄を書き換えた（下書きとして覚える） */
  | ({ type: 'wcDraft' } & MessageParts)
  | { type: 'wcStageFiles'; op: 'stage' | 'unstage'; files: { group: ChangeGroup; path: string }[] }
  | { type: 'wcStageLines'; group: ChangeGroup; path: string; version: string; lines: SelectedLine[] }
  /** Changes（unstaged）のファイルを丸ごと破棄する */
  | { type: 'wcDiscardFiles'; paths: string[] }
  /** Changes のファイルの、選んだ行だけを破棄する */
  | { type: 'wcDiscardLines'; path: string; version: string; lines: SelectedLine[] }
  | { type: 'wcCommit'; message: string; amend: boolean }
  | { type: 'wcOpenFile'; path: string }
  | { type: 'wcResolve'; path: string; action: 'merge' | 'current' | 'incoming' };
