/** メインビュー（エディタ領域の 1 つの Webview）と拡張ホストの間のメッセージと、ナビのドラッグ＆ドロップの型 */
import type { DetailsStyle } from '../history/protocol.ts';
import type { BlameHostMessage, BlameWebviewMessage } from '../blame/protocol.ts';
import type { DaemonHostMessage, DaemonWebviewMessage } from '../daemonStatus/protocol.ts';
import type { FileHistoryHostMessage, FileHistoryWebviewMessage } from '../fileHistory/protocol.ts';
import type { HistoryHostMessage, HistoryWebviewMessage } from '../history/protocol.ts';
import type { RangeDiffHostMessage, RangeDiffWebviewMessage } from '../rangeDiff/protocol.ts';
import type { Drawing } from '../history/drawing.ts';
import type { ReflogHostMessage, ReflogWebviewMessage } from '../reflog/protocol.ts';
import type { SettingsHostMessage, SettingsWebviewMessage } from '../settings/protocol.ts';
import type { WorkingCopyHostMessage, WorkingCopyWebviewMessage } from '../workingCopy/protocol.ts';

/** メインビューに出す画面 */
export type ViewName = 'workingCopy' | 'history' | 'reflog' | 'rangeDiff' | 'fileHistory' | 'blame' | 'settings' | 'daemon';

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
  | DaemonHostMessage
  /** 指定した画面に切り替える */
  | { type: 'show'; view: ViewName }
  /** 長い行を折り返すか（設定 `amonhen.wrapLongLines`。すべての差分とファイルの中身） */
  | { type: 'wrapLines'; wrap: boolean }
  /** コミット情報の見せ方（簡潔・標準・詳細。すべての詳細の欄） */
  | { type: 'detailsStyle'; style: DetailsStyle }
  /** 表示の設定で選んだ: 高さを中身に合わせ直す（fit）、コミット情報だけを表示する・戻す（expand） */
  | { type: 'detailsView'; action: 'fit' | 'expand' }
  /** 確認のシートを重ねる（答えは sheetAnswer） */
  | ({ type: 'sheet' } & Sheet)
  /** シートの Check Connection の結果（つながったときの要約か、エラー） */
  | { type: 'sheetChecked'; id: number; text: string; ok: boolean };

export type WebviewMessage =
  | HistoryWebviewMessage
  | WorkingCopyWebviewMessage
  | ReflogWebviewMessage
  | RangeDiffWebviewMessage
  | FileHistoryWebviewMessage
  | BlameWebviewMessage
  | SettingsWebviewMessage
  | DaemonWebviewMessage
  /** Webview の準備ができた */
  | { type: 'ready' }
  /** 画面を切り替えた */
  | { type: 'view'; view: ViewName }
  /** 差分の帯の ↩︎: 長い行の折り返しを切り替える */
  | { type: 'toggleWrapLines' }
  /** 詳細の帯の表示の設定（kept: 仕切りを動かした高さを保っている、expanded: コミット情報だけを表示している） */
  | { type: 'detailsViewOptions'; kept: boolean; expanded: boolean }
  /** 確認のシートの答え。index は押したボタン（buttons の順）、やめたらなし。choice は選んでいた取り込み方など */
  | { type: 'sheetAnswer'; id: number; index?: number; choice?: number; values?: string[] }
  /** シートの Check Connection を押した（url は欄の値） */
  | { type: 'sheetCheck'; id: number; url: string }
  /** 確認のシートの一覧の行を押した（item は lists の行を通して数えた番号。シートは閉じない） */
  | { type: 'sheetOpen'; id: number; item: number };

/** シートの答え: 押したボタン（buttons の順）と、選んでいたもの（choices の順。なければ 0） */
export interface SheetAnswer {
  index: number;
  choice: number;
  /** 入力欄の値（fields の順） */
  values?: string[];
}

/** シートの入力欄（Add Remote の Name・Fetch URL・Push URL） */
export interface SheetField {
  label: string;
  value: string;
  /** 空のときに薄く出す文 */
  placeholder?: string;
  /** 空なら既定のボタンを押せない */
  required?: boolean;
  /** この値（前後の空白を除いて）なら既定のボタンを押せず、そう言う（もうある名前） */
  taken?: string[];
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
  /** 入力欄（あれば答えの values に入る） */
  fields?: SheetField[];
  /** この番号の欄の URL の先が何か（GitHub・パス・SSH 先…）を Points to の行に出す */
  pointsTo?: number;
  /** この番号の欄の URL へつないでみるボタン（Check Connection。結果は sheetChecked で受ける） */
  check?: number;
}
