/** メインビュー（エディタ領域の 1 つの Webview）と拡張ホストの間のメッセージと、ナビのドラッグ＆ドロップの型 */
import type { BlameHostMessage, BlameWebviewMessage } from '../blame/protocol.ts';
import type { FileHistoryHostMessage, FileHistoryWebviewMessage } from '../fileHistory/protocol.ts';
import type { HistoryHostMessage, HistoryWebviewMessage } from '../history/protocol.ts';
import type { RangeDiffHostMessage, RangeDiffWebviewMessage } from '../rangeDiff/protocol.ts';
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
  | { type: 'show'; view: ViewName };

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
  | { type: 'view'; view: ViewName };
