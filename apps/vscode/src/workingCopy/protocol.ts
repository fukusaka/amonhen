/** メインビューの Working Copy と拡張ホストの間のメッセージ */
import type { FileDiff, SelectedLine } from '../diff/fileDiff.ts';
import type { Change, ChangeGroup } from '../state/changes.ts';
import type { MessageParts } from './commitMessage.ts';
import type { FileLayout } from './fileList.ts';

export interface FileStat {
  path: string;
  added: number | null;
  removed: number | null;
}

export type WorkingCopyHostMessage =
  | { type: 'wcFiles'; files: Change[]; branch?: string; lastMessage?: string; layout: FileLayout }
  /** 選んでいるファイルの差分。group は今そのファイルがあるグループ */
  | { type: 'wcDiff'; group: ChangeGroup; diff: FileDiff }
  | { type: 'wcDiff'; group?: undefined; diff: undefined }
  /** リポジトリを開いたときのコミット欄の中身（下書き、なければテンプレート） */
  | ({ type: 'wcMessage' } & MessageParts)
  /** 最近のコミットのメッセージ（新しい順。⌥↑・⌥↓ で呼び出す） */
  | { type: 'wcRecent'; messages: string[] }
  /** 選んだメッセージをコミット欄に入れる */
  | { type: 'wcUseMessage'; message: string }
  /** 件名に型を付ける */
  | { type: 'wcInsertType'; commitType: string }
  /** 本文の末尾にトレーラーを足す */
  | { type: 'wcInsertTrailer'; line: string }
  /** Amend を入れて、直前のメッセージを入れる */
  | { type: 'wcStartAmend' }
  /** `fixup! 件名`・`squash! 件名` のコミットを書き始める */
  | { type: 'wcStartFixup'; kind: 'fixup' | 'squash'; subject: string }
  /** コミットの hooks の出力や、失敗の理由 */
  | { type: 'wcCommitOutput'; title: string; text: string }
  /** ファイルごとの追加・削除の行数（HEAD と作業ツリー。エンジンのあるときだけ） */
  | { type: 'wcStats'; stats: FileStat[] }
  /** 一部のフォルダだけ取り出しているときの知らせ（ふつうは null） */
  | { type: 'wcSparse'; sparse: { cone: boolean; folders: string[] } | null }
  | { type: 'wcCommitted' };

export type WorkingCopyWebviewMessage =
  | { type: 'wcSelect'; group: ChangeGroup; path: string }
  /** ファイル一覧の見せ方を変えた（次に開くときのために覚える） */
  | { type: 'wcLayout'; layout: FileLayout }
  /** コミット欄を書き換えた（下書きとして覚える） */
  | ({ type: 'wcDraft' } & MessageParts)
  /** 最近のメッセージの一覧を出す */
  | { type: 'wcPickRecent' }
  /** 取り出すフォルダを変える画面を開く */
  | { type: 'wcShowFolders' }
  /** 型やトレーラーの一覧を出す */
  | { type: 'wcPickInsert' }
  | { type: 'wcStageFiles'; op: 'stage' | 'unstage'; files: { group: ChangeGroup; path: string }[] }
  /** stage は HEAD → 作業ツリーの差分（combined）で、入れるか（true）外すか（false） */
  | { type: 'wcStageLines'; group: ChangeGroup; path: string; version: string; lines: SelectedLine[]; stage?: boolean }
  /** Changes（unstaged）のファイルを丸ごと破棄する */
  | { type: 'wcDiscardFiles'; paths: string[] }
  /** Changes のファイルの、選んだ行だけを破棄する */
  | { type: 'wcDiscardLines'; path: string; version: string; lines: SelectedLine[] }
  /** コミットする。push なら続けて push する（上流がなければ Publish） */
  | { type: 'wcCommit'; message: string; amend: boolean; push?: boolean }
  /** Commit の続きの一覧を出す */
  | { type: 'wcCommitMenu'; canCommit: boolean; message: string; amend: boolean }
  | { type: 'wcOpenFile'; path: string }
  | { type: 'wcResolve'; path: string; action: 'merge' | 'current' | 'incoming' };
