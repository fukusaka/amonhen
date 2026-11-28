/** メインビューの Working Copy と拡張ホストの間のメッセージ */
import type { FileDiff, SelectedLine } from '../diff/fileDiff.ts';
import type { Change, ChangeGroup } from '../state/changes.ts';
import type { MessageParts } from './commitMessage.ts';
import type { FileLayout } from './fileList.ts';
import type { DiffWhitespace } from '../history/protocol.ts';
import type { DiffSide, SideNote } from './sides.ts';
import type { GapOpen } from './gaps.ts';

/**
 * 行の Stage の形（設定 `amonhen.diff.lineStaging`。アプリと同じ）: 行を選んでから隙間の行の操作で（select）、
 * 変更の帯・行番号を押して（bar）
 */
export type LineStaging = 'select' | 'bar';

/** 差分の右クリック（package.json の webview/context、wcDiff）の項目。拡張が Webview に頼み返す */
export type DiffMenuAction =
  | 'gapDown'
  | 'gapUp'
  | 'gapAll'
  | 'gapCollapse'
  | 'stageSelected'
  | 'unstageSelected'
  | 'discardSelected'
  | 'stageHunk'
  | 'unstageHunk'
  | 'discardHunk'
  | 'stageLine'
  | 'unstageLine'
  | 'discardLine';

/** 並べた差分の 1 ファイル（group は差分の側のグループ。All なら今そのファイルがあるグループ） */
export interface StackedFile {
  group: ChangeGroup;
  diff: FileDiff;
  /** たたんだ差分の隙間の開き方 */
  open: GapOpen[];
}

export interface FileStat {
  path: string;
  added: number | null;
  removed: number | null;
}

/** Working Copy の見た目の好み（覚える） */
export interface WorkingCopyLook {
  /** 一覧を Staged と Unstaged の節に分ける（でなければ Mixed の 1 つの一覧。エンジンがなければいつも節） */
  sections: boolean;
  /** コミットの欄を件名の行だけにたたむ */
  commitCollapsed: boolean;
  /** コミットの欄の高さ（欄と差分の境目を動かしたとき。なければ中身の高さ。0 を送ると中身の高さに戻す） */
  commitHeight?: number;
}

export type WorkingCopyHostMessage =
  /** engine: エンジン（共有の daemon）で読んでいる（見る側の帯・Mixed の一覧・空白の無視はエンジンがあるときだけ） */
  | ({
      type: 'wcFiles';
      files: Change[];
      branch?: string;
      lastMessage?: string;
      layout: FileLayout;
      engine: boolean;
      lineStaging: LineStaging;
    } & WorkingCopyLook)
  /** 行の Stage の形を設定で変えた */
  | { type: 'wcLineStaging'; lineStaging: LineStaging }
  /** 差分の右クリックの項目を選んだ（gap は隙間の番号、hunk は差分の hunk の位置、line は行の lineKey） */
  | { type: 'wcDiffAction'; action: DiffMenuAction; gap?: number; hunk?: number; line?: string }
  /**
   * 選んでいるファイルの差分。group は差分の側のグループ（All なら今そのファイルがあるグループ）。whitespace は
   * エンジンで読んだときの、無視している空白（あれば帯で選べる。エンジンがなければ選べない）。side はエンジンで
   * 読んだときの見る側。open は、たたんだ差分（diff.folded）の隙間の開き方
   */
  | { type: 'wcDiff'; group: ChangeGroup; diff: FileDiff; whitespace?: DiffWhitespace; side?: DiffSide; open?: GapOpen[] }
  /** 差分を出さない。gone は変更がなくなったので選びを外したファイル */
  | { type: 'wcDiff'; group?: undefined; diff: undefined; whitespace?: undefined; side?: undefined; open?: undefined; gone?: string }
  /**
   * いくつか選んだファイルの差分を並べる（エンジンがあるとき。アプリと同じ）。paths は選んだファイル（一覧の順）、
   * total はその側に変更のある数、files は並べるもの（上から Files to open together の数まで）。note は何も
   * 並べないわけ（All で空白を無視している）
   */
  | { type: 'wcStack'; side: DiffSide; whitespace: DiffWhitespace; paths: string[]; total: number; files: StackedFile[]; note?: SideNote }
  /** 選んだ側の差分を出さず、わけを言う（その側に変更がない・All で出せない） */
  | { type: 'wcDiffNote'; group: ChangeGroup; path: string; side: DiffSide; whitespace: DiffWhitespace; note: SideNote }
  /** 節ごとの追加・削除の行数（HEAD → index と index → 作業ツリー。エンジンがあって節に分けているときだけ） */
  | { type: 'wcSplitStats'; staged: FileStat[]; unstaged: FileStat[] }
  /** 選んでいるファイルの差分を読めなかった。差分の場所に理由を出す */
  | { type: 'wcDiffFailed'; group: ChangeGroup; path: string; error: string }
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
  | { type: 'wcCommitted' }
  /** コマンド（Commit・Commit and Push）から、コミット欄の中身でコミットする。押せなければわけを返す */
  | { type: 'wcCommitNow'; push: boolean }
  /** コミットを始めた・終えた（成り行きによらず。その間は Commit を押せない） */
  | { type: 'wcCommitting'; committing: boolean };

export type WorkingCopyWebviewMessage =
  /** ファイルを選んだ。side は、節の行を選んで見る側をその節にそろえたとき */
  | { type: 'wcSelect'; group: ChangeGroup; path: string; side?: DiffSide }
  /**
   * いくつかのファイルを選んだ（エンジンがあるとき。差分を並べる）。paths は選んだファイル（一覧の順）、group と
   * path は最後に押した行（並べるのをやめたら、その差分を出す）
   */
  | { type: 'wcSelectFiles'; group: ChangeGroup; path: string; paths: string[]; side?: DiffSide }
  /** 差分で見る側を選び直した（エンジンがあるとき。覚えない） */
  | { type: 'wcSide'; side: DiffSide }
  /** たたんだ差分の隙間の開き方を変えた（ファイルと側ごとに、拡張が動いている間だけ覚える） */
  | { type: 'wcGaps'; path: string; side: DiffSide; open: GapOpen[] }
  /** 見た目の好みを変えた（覚える） */
  | ({ type: 'wcLook' } & Partial<WorkingCopyLook>)
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
  /**
   * 選んだ行を stage・unstage する。combined は行が HEAD → 作業ツリーの差分（All）のもので、stage はそこで入れるか
   * （true）外すか（false）
   */
  | { type: 'wcStageLines'; group: ChangeGroup; path: string; version: string; lines: SelectedLine[]; stage?: boolean; combined?: boolean }
  /** 作業ツリーの側（unstaged）のファイルを丸ごと破棄する */
  | { type: 'wcDiscardFiles'; paths: string[] }
  /** 作業ツリーの側の、選んだ行だけを破棄する。combined は行が All の差分のもの */
  | { type: 'wcDiscardLines'; path: string; version: string; lines: SelectedLine[]; combined?: boolean }
  /** コミットする。push なら続けて push する（上流がなければ Publish） */
  | { type: 'wcCommit'; message: string; amend: boolean; push?: boolean }
  /** Commit の続きの一覧を出す */
  | { type: 'wcCommitMenu'; canCommit: boolean; message: string; amend: boolean }
  | { type: 'wcOpenFile'; path: string }
  /** 差分で行を比べるときに無視する空白を選び直した（覚えない。無視している間は行・hunk を stage・破棄しない） */
  | { type: 'wcWhitespace'; whitespace: DiffWhitespace }
  /** コマンドからのコミットを、押せないので断った（reason は Commit のツールチップと同じ） */
  | { type: 'wcCommitRefused'; reason: string }
  | { type: 'wcResolve'; path: string; action: 'merge' | 'current' | 'incoming' };
