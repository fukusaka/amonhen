/** メインビュー（エディタ領域の 1 つの Webview）と拡張ホストの間のメッセージ */
import type { HistoryHostMessage, HistoryWebviewMessage } from '../history/protocol.ts';
import type { WorkingCopyHostMessage, WorkingCopyWebviewMessage } from '../workingCopy/protocol.ts';

/** 左のナビで切り替える画面 */
export type ViewName = 'workingCopy' | 'history';

/** ナビに出すリポジトリの要約 */
export interface RepoSummary {
  name: string;
  /** detached HEAD のときは undefined */
  branch?: string;
  /** Working Copy の変更の数 */
  changes: number;
  /** 途中で止まっている merge や rebase */
  sequence?: string;
}

export type HostMessage =
  | HistoryHostMessage
  | WorkingCopyHostMessage
  | { type: 'repo'; repo: RepoSummary | undefined }
  /** 指定した画面に切り替える */
  | { type: 'show'; view: ViewName };

export type WebviewMessage =
  | HistoryWebviewMessage
  | WorkingCopyWebviewMessage
  /** Webview の準備ができた */
  | { type: 'ready' }
  /** 画面を切り替えた */
  | { type: 'view'; view: ViewName };
