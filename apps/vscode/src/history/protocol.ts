/** メインビューの History と拡張ホストの間のメッセージ。Remote-SSH では SSH を通るので最小限にする */
import type { Signature } from '../git/commitObject.ts';
import type { FileDiff } from '../diff/fileDiff.ts';
import type { FileChange } from '../git/log.ts';
import type { SignatureKind } from '../git/signature.ts';
import type { GraphRow } from './graph.ts';

/** 履歴の範囲。現在のブランチ、すべて、または `ref:refs/heads/x` のような 1 つの ref */
export type Scope = 'head' | 'all' | `ref:${string}` | `refs:${string}`;

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
export type MessageEdit = 'reword' | 'squash' | 'combine';

/** 2 つ選んだときの差分の取り方（コアの `pair`） */
export type PairMode = 'forward' | 'reverse' | 'mergeBase';

/** いくつも選んだコミットの見せ方（`commitDiff` の `history`・`rows`）。差分はファイルごとに求める */
export interface SelectionView {
  /** 差分を求めるときの `oid` の代わり */
  key: string;
  count: number;
  /** 続いたコミット・離れた 2 つ・離れた 3 つ以上 */
  kind: 'range' | 'pair' | 'each';
  pair: PairMode;
  files: { path: string; added: number; removed: number; binary: boolean }[];
  /** 選んだコミット（上から。多ければ先頭だけ） */
  commits: { row: number; oid: string; subject: string; author: string }[];
}

/** History の検索の様子 */
export interface SearchState {
  query: string;
  /** 一致した行（昇順）。探している途中なら、そこまでに分かったもの */
  rows: number[];
  /** 探している途中か。`done` は探し終えた割合（0〜1。内容の条件がなければ 0 のまま） */
  searching: boolean;
  done: number;
  /** 一致だけの History を出している（行はすべて一致） */
  matchingOnly: boolean;
  /** 構文の誤りなど */
  error?: string;
}

export type HistoryHostMessage =
  /** `select` があれば、新しい History のその行を選ぶ（一致だけの History との切り替え） */
  | { type: 'reset'; title: string; scope: Scope; select?: number }
  /** `total` があれば daemon の History で、行は `range` で頼んだところだけ届く（`done` は常に true） */
  | { type: 'rows'; start: number; rows: RowData[]; done: boolean; total?: number }
  /** 検索できるか（daemon の History のときだけ） */
  /** その行を選んで見せる（Reflog の Show in History） */
  | { type: 'reveal'; row: number }
  | { type: 'searchable'; searchable: boolean }
  | { type: 'search'; state: SearchState }
  | { type: 'busy'; busy: boolean }
  | { type: 'details'; details: CommitDetails }
  /** いくつも選んだコミットの差分。`token` は頼んだときのもの。読めなければ `error` */
  | { type: 'selection'; token: number; view?: SelectionView; error?: string }
  /** コミットの中の 1 ファイルの差分（最初の親との差分） */
  | { type: 'commitDiff'; oid: string; path: string; diff: FileDiff }
  /** `last` はまとめる範囲のいちばん新しいコミット（combine のとき。`oid` がいちばん古い） */
  | { type: 'editMessage'; oid: string; last?: string; kind: MessageEdit; title: string; message: string }
  /** 行の署名の印（届いた行の分。署名のないコミットは含まない） */
  | { type: 'signatures'; marks: Record<string, SignatureView> }
  /** 詳細の残り（署名・子のコミット） */
  | { type: 'detailsExtras'; extras: DetailsExtras }
  /**
   * リポジトリの形（エンジンだけ）。`shallow` なら History の最後に取り足す帯を出し、`boundary` の行（shallow の
   * 境目のコミット）に印を付ける。`partial` は partial clone の promisor の remote と filter
   */
  | { type: 'form'; shallow: boolean; boundary: number[]; partial?: { remote: string; filter: string } };

/** 署名の見せ方（アプリの SignatureStatus。形と色は Webview が kind から決める） */
export interface SignatureView {
  kind: SignatureKind;
  /** 信用度の段階（verified のとき。3 は強い信用） */
  trust?: number;
  label: string;
  explanation: string;
  /** 署名した人・鍵（分かれば） */
  signer?: string;
  key?: string;
}

/** コミットの詳細のうち時間のかかる分（コアの `commitExtras`）。署名と子のコミット */
export interface DetailsExtras {
  oid: string;
  /** 署名がなければ undefined */
  signature?: SignatureView;
  /** そのコミットを親に持つコミット */
  children: string[];
}

export type HistoryWebviewMessage =
  | { type: 'more' }
  /** daemon の History の、start から count 行 */
  | { type: 'range'; start: number; count: number }
  /** 探す（空なら検索をやめる）。`selected` は今選んでいる行（一致だけの History との切り替えで選んだままにする） */
  | { type: 'search'; query: string; matchingOnly: boolean; selected: number }
  /** コミットの変更をまとめて開く */
  | { type: 'open'; oid: string }
  /** 選んだコミットの詳細を求める */
  | { type: 'select'; oid: string }
  /** いくつも選んだ行の差分を求める（エンジンだけ） */
  | { type: 'selectRows'; token: number; rows: number[]; pair: PairMode }
  /** コミットの中の 1 ファイルの差分を、エディタで開く */
  | { type: 'openFile'; oid: string; path: string }
  /** コミットの中の 1 ファイルの差分を求める（詳細に並べる） */
  | { type: 'commitDiff'; oid: string; path: string }
  | { type: 'scope'; scope: Scope }
  | { type: 'saveMessage'; oid: string; last?: string; kind: MessageEdit; message: string }
  /** 編集できる範囲の中で、新しい順の to 番目に動かす */
  | { type: 'move'; oid: string; to: number }
  /** HEAD・親・子のコミットに移る（`oid` は今選んでいるコミット） */
  | { type: 'goTo'; to: 'head' | 'parent' | 'child'; oid?: string }
  /** shallow clone の履歴を取り足す・すべて取る、partial clone の中身をすべて取る（帯と札のボタン） */
  | { type: 'fetchForm'; what: 'more' | 'all' | 'contents' };
