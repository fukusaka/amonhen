/** History の Webview と拡張ホストの間のメッセージ。Remote-SSH では SSH を通るので最小限にする */
import type { GraphRow } from './graph.ts';

export type Scope = 'head' | 'all';

export interface RefLabel {
  name: string;
  kind: 'head' | 'branch' | 'remote' | 'tag';
  /** checkout しているブランチ */
  current?: boolean;
}

export interface RowData {
  oid: string;
  subject: string;
  author: string;
  /** unix 秒 */
  date: number;
  refs: RefLabel[];
  graph: GraphRow;
  /** 非対話の rebase で編集できるか（HEAD から first-parent で辿れ、間に merge が無い） */
  editable: boolean;
}

export type HostMessage =
  | { type: 'reset'; title: string; scope: Scope }
  | { type: 'rows'; start: number; rows: RowData[]; done: boolean }
  | { type: 'busy'; busy: boolean }
  | { type: 'editMessage'; oid: string; title: string; message: string };

export type WebviewMessage =
  | { type: 'ready' }
  | { type: 'more' }
  | { type: 'open'; oid: string }
  | { type: 'scope'; scope: Scope }
  | { type: 'saveMessage'; oid: string; message: string }
  | { type: 'move'; oid: string; to: number };
