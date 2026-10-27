/** メインビューの History と拡張ホストの間のメッセージ。Remote-SSH では SSH を通るので最小限にする */
import type { Signature } from '../git/commitObject.ts';
import type { FileDiff } from '../diff/fileDiff.ts';
import type { FileChange } from '../git/log.ts';
import type { GraphRow } from './graph.ts';

/** 履歴の範囲。現在のブランチ、すべて、または `ref:refs/heads/x` のような 1 つの ref */
export type Scope = 'head' | 'all' | `ref:${string}`;

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
  /** 作者とコミッター。mailmap を使うなら置き換えたもの */
  author: Signature;
  committer: Signature;
  /** mailmap で置き換えて記録と違えば、コミットに記録されたもの */
  recordedAuthor?: Person;
  recordedCommitter?: Person;
  /** subject と本文 */
  message: string;
  refs: RefLabel[];
  /** 最初の親との差分 */
  files: FileChange[];
}

export interface Person {
  name: string;
  email: string;
}

/** メッセージを編集してから行う操作 */
export type MessageEdit = 'reword' | 'squash';

export type HistoryHostMessage =
  | { type: 'reset'; title: string; scope: Scope }
  | { type: 'rows'; start: number; rows: RowData[]; done: boolean }
  | { type: 'busy'; busy: boolean }
  | { type: 'details'; details: CommitDetails }
  /** コミットの中の 1 ファイルの差分（最初の親との差分） */
  | { type: 'commitDiff'; oid: string; path: string; diff: FileDiff }
  | { type: 'editMessage'; oid: string; kind: MessageEdit; title: string; message: string };

export type HistoryWebviewMessage =
  | { type: 'more' }
  /** コミットの変更をまとめて開く */
  | { type: 'open'; oid: string }
  /** 選んだコミットの詳細を求める */
  | { type: 'select'; oid: string }
  /** コミットの中の 1 ファイルの差分を、エディタで開く */
  | { type: 'openFile'; oid: string; path: string }
  /** コミットの中の 1 ファイルの差分を求める（詳細に並べる） */
  | { type: 'commitDiff'; oid: string; path: string }
  | { type: 'scope'; scope: Scope }
  | { type: 'saveMessage'; oid: string; kind: MessageEdit; message: string }
  /** 編集できる範囲の中で、新しい順の to 番目に動かす */
  | { type: 'move'; oid: string; to: number };
