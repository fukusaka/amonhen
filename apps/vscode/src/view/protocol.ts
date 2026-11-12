/** メインビュー（エディタ領域の 1 つの Webview）と拡張ホストの間のメッセージと、ナビのドラッグ＆ドロップの型 */
import type { BlameHostMessage, BlameWebviewMessage } from '../blame/protocol.ts';
import type { FileHistoryHostMessage, FileHistoryWebviewMessage } from '../fileHistory/protocol.ts';
import type { HistoryHostMessage, HistoryWebviewMessage } from '../history/protocol.ts';
import type { RangeDiffHostMessage, RangeDiffWebviewMessage } from '../rangeDiff/protocol.ts';
import type { Drawing } from '../history/drawing.ts';
import type { ReflogHostMessage, ReflogWebviewMessage } from '../reflog/protocol.ts';
import type { SettingsHostMessage, SettingsWebviewMessage } from '../settings/protocol.ts';
import type { WorkingCopyHostMessage, WorkingCopyWebviewMessage } from '../workingCopy/protocol.ts';

/** メインビューに出す画面 */
export type ViewName = 'workingCopy' | 'history' | 'reflog' | 'rangeDiff' | 'fileHistory' | 'blame' | 'settings';

/** ドラッグしたもの。tag とリモートブランチは local でない */
export interface DragSource {
  ref: string;
  local: boolean;
  upstream?: string;
}

/** ドロップ先 */
export type DropTarget =
  | { kind: 'branch'; ref: string }
  | { kind: 'remote'; remote: string }
  | { kind: 'remoteBranch'; remote: string; ref: string };

export type HostMessage =
  | HistoryHostMessage
  | WorkingCopyHostMessage
  | ReflogHostMessage
  | RangeDiffHostMessage
  | FileHistoryHostMessage
  | BlameHostMessage
  | SettingsHostMessage
  /** 指定した画面に切り替える */
  | { type: 'show'; view: ViewName }
  /** 確認のシートを重ねる（答えは sheetAnswer） */
  | ({ type: 'sheet' } & Sheet);

export type WebviewMessage =
  | HistoryWebviewMessage
  | WorkingCopyWebviewMessage
  | ReflogWebviewMessage
  | RangeDiffWebviewMessage
  | FileHistoryWebviewMessage
  | BlameWebviewMessage
  | SettingsWebviewMessage
  /** Webview の準備ができた */
  | { type: 'ready' }
  /** 画面を切り替えた */
  | { type: 'view'; view: ViewName }
  /** 確認のシートの答え。index は押したボタン（buttons の順）、やめたらなし。choice は選んでいた取り込み方など */
  | { type: 'sheetAnswer'; id: number; index?: number; choice?: number }
  /** 確認のシートの一覧の行を押した（item は lists の行を通して数えた番号。シートは閉じない） */
  | { type: 'sheetOpen'; id: number; item: number };

/** シートの答え: 押したボタン（buttons の順）と、選んでいたもの（choices の順。なければ 0） */
export interface SheetAnswer {
  index: number;
  choice: number;
}

/** シートで選ぶもの（ラジオ）。選んだものに絵があれば、シートの絵の代わりになる。文はシートの文の後ろに足す */
export interface SheetChoice {
  label: string;
  drawing?: Drawing;
  lines?: string[];
  /** 選べるが、既定のボタンを押せない（取り込めない取り込み方） */
  blocked?: boolean;
}

/** コミットの一覧。open のある行は押すとその差分を開く */
export interface SheetList {
  title: string;
  items: { label: string; description?: string; open?: boolean }[];
  /** 並べなかった数 */
  more?: string;
  /** 消える・外れるもの */
  tone?: 'red';
}

/**
 * メインビューに重ねる確認のシート（アプリのシートと同じく、前と後の絵を添える）。buttons の先頭が既定のボタン
 * （Enter）。Cancel は Webview が足す
 */
export interface Sheet {
  id: number;
  title: string;
  lines: string[];
  /** 注意（⚠︎ を付けて、目立つ色で） */
  warnings?: string[];
  /** 前と後の絵 */
  drawing?: Drawing;
  /** 選ぶもの（ラジオ）と、はじめに選んでおくもの */
  choices?: SheetChoice[];
  choice?: number;
  lists?: SheetList[];
  buttons: string[];
  /** 既定のボタンが取り消せない操作（Force Push） */
  destructive?: boolean;
}
