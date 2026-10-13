/** History の Webview と拡張ホストの間のメッセージ。Remote-SSH では SSH を通るので最小限にする */
import type { Signature } from '../git/commitObject.ts';
import type { FileChange } from '../git/log.ts';
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
  /** 編集できる範囲での位置（新しい順、HEAD が 0）。並べ替えの移動先に使う */
  chain?: number;
  /** 親も編集できる範囲にあり、squash / fixup できる */
  canSquash: boolean;
}

/** 右側に出すコミットの詳細 */
export interface CommitDetails {
  oid: string;
  parents: string[];
  author: Signature;
  committer: Signature;
  /** subject と本文 */
  message: string;
  refs: RefLabel[];
  /** 最初の親との差分 */
  files: FileChange[];
}

/** メッセージを編集してから行う操作 */
export type MessageEdit = 'reword' | 'squash';

export type HostMessage =
  | { type: 'reset'; title: string; scope: Scope }
  | { type: 'rows'; start: number; rows: RowData[]; done: boolean }
  | { type: 'busy'; busy: boolean }
  | { type: 'details'; details: CommitDetails }
  | { type: 'editMessage'; oid: string; kind: MessageEdit; title: string; message: string };

export type WebviewMessage =
  | { type: 'ready' }
  | { type: 'more' }
  /** コミットの変更をまとめて開く */
  | { type: 'open'; oid: string }
  /** 選んだコミットの詳細を求める */
  | { type: 'select'; oid: string }
  /** コミットの中の 1 ファイルの差分を開く */
  | { type: 'openFile'; oid: string; path: string }
  | { type: 'scope'; scope: Scope }
  | { type: 'saveMessage'; oid: string; kind: MessageEdit; message: string }
  /** 編集できる範囲の中で、新しい順の to 番目に動かす */
  | { type: 'move'; oid: string; to: number };
