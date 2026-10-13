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

/** ナビに出すブランチ・リモートブランチ・タグ */
export interface NavRef {
  /** `refs/heads/main` など */
  ref: string;
  /** 表示名（`main`、`origin/main`、`v1.0`） */
  name: string;
  current?: boolean;
  ahead?: number;
  behind?: number;
  /** upstream がリモートから消えている */
  gone?: boolean;
  upstream?: string;
}

export interface NavRefs {
  branches: NavRef[];
  remotes: { name: string; branches: NavRef[] }[];
  /** 展開されるまで送らない */
  tags?: NavRef[];
  stashes?: { name: string; subject: string }[];
  submodules?: { path: string; state?: string }[];
}

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

/** ナビのツールバーから実行するコマンド（`amonhen.<name>`） */
export type ToolbarCommand = 'fetch' | 'pull' | 'push' | 'undo';

export type HostMessage =
  | HistoryHostMessage
  | WorkingCopyHostMessage
  | { type: 'repo'; repo: RepoSummary | undefined }
  /** 指定した画面に切り替える */
  | { type: 'show'; view: ViewName }
  | { type: 'refs'; refs: NavRefs };

export type WebviewMessage =
  | HistoryWebviewMessage
  | WorkingCopyWebviewMessage
  /** Webview の準備ができた */
  | { type: 'ready' }
  /** 画面を切り替えた */
  | { type: 'view'; view: ViewName }
  /** Tags・Stashes・Submodules を展開した */
  | { type: 'navExpand'; section: 'tags' | 'stashes' | 'submodules' }
  /** ブランチなどを選んだ。History でその履歴を見る */
  | { type: 'showRef'; ref: string }
  /** ブランチをダブルクリックした。ローカルなら切り替え、リモートなら追跡するブランチを作る */
  | { type: 'checkout'; ref: string }
  | { type: 'dropRef'; source: DragSource; target: DropTarget }
  | { type: 'command'; command: ToolbarCommand };
