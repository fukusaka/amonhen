/** メインビューの History と拡張ホストの間のメッセージ。Remote-SSH では SSH を通るので最小限にする */
import type { EditRow, PlanStep } from './editPlan.ts';
import type { Signature } from '../git/commitObject.ts';
import type { FileDiff } from '../diff/fileDiff.ts';
import type { FileChange } from '../git/log.ts';
import type { SignatureKind } from '../git/signature.ts';
import type { GraphRow } from './graph.ts';

/** 履歴の範囲。現在のブランチ、すべて、または `ref:refs/heads/x` のような 1 つの ref */
export type Scope = 'head' | 'all' | `ref:${string}` | `refs:${string}`;

/** History の並び順: 日付順か、ブランチのコミットを merge の下にまとめるか */
export type HistoryOrder = 'date' | 'topo';
/** History の行に出す項目 */
export type HistoryItem = 'refs' | 'author' | 'date' | 'oid' | 'signature';

export interface RefLabel {
  name: string;
  kind: 'head' | 'branch' | 'remote' | 'tag' | 'worktree';
  /** checkout しているブランチ */
  current?: boolean;
}

export interface RowData {
  oid: string;
  /** merge（親が 2 つ以上） */
  merge?: boolean;
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
  /** 畳んだ merge が隠している、取り込んだコミットの数 */
  hidden?: number;
  /** 開いている merge（畳める） */
  expanded?: boolean;
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
  /** 取っていない親（shallow clone の境目） */
  missingParents?: string[];
  /** 最初の親との差分 */
  files: FileChange[];
  /** partial clone で中身が手元になく、差分を取っていないファイル（パス。「取ってくる」の帯で取るまで） */
  missingFiles?: string[];
}

export interface Person {
  name: string;
  email: string;
}

/** メッセージを編集してから行う操作 */
export type MessageEdit = 'reword' | 'squash' | 'combine';

/** 2 つ選んだときの差分の取り方（コアの `pair`） */
export type PairMode = 'forward' | 'reverse' | 'mergeBase';

/** 編集モードの「積み重ねたブランチも動かす」: git の設定（configured）と、選べるか（git 2.38 から） */
export interface EditStacked {
  configured: boolean;
  available: boolean;
}

/** 詳細の欄の、変わったファイルの見せ方。`tree` はフォルダごとにまとめた木、そうでなければパスの並び */
export interface DetailsLayout {
  tree: boolean;
}

/** 差分を比べるときに無視する空白（`git diff --ignore-space-at-eol`・`-b`・`-w`）。exact は無視しない */
export type DiffWhitespace = 'exact' | 'at-eol' | 'change' | 'all';

/**
 * 詳細の差分で選べること（エンジンだけ。なければ出さない）: 無視する空白（今の選び方）、ファイルごとに前後の行を
 * 広げる・ファイル全体を出す
 */
export interface DiffChoices {
  whitespace: DiffWhitespace;
}

/** 画像の差分の片側（data URL と、元の大きさ） */
export interface ImageBlob {
  url: string;
  bytes: number;
}

/** コミットの木の 1 つ（「すべてのファイル」） */
export interface TreeEntry {
  name: string;
  kind: 'folder' | 'file' | 'executable' | 'link' | 'submodule';
}

/**
 * 「すべてのファイル」のファイルの中身の 1 行。`mark` はこのコミットで足した行（added）か変えた行（changed、`old` は
 * 前の行）。`removedBefore` はこの行の前で消した行の数
 */
export interface ViewLine {
  text: string;
  syntax?: [number, number, number][];
  mark?: 'added' | 'changed';
  old?: string;
  removedBefore?: number;
}

/**
 * ある描画部品での SVG の見え方（コアの `svgmobile::Preview`）。`svg` は描ける SVG（部品が読めなければなし。わけは
 * `error`）、`title` は絵の見出し（近似なら「（approximation）」つき）、`caption` は絵の作り方の説明、`messages` は
 * 取り込みの警告と同じに描けなかったこと、`exact` は本物の取り込みを移したものか
 */
export interface MobilePreview {
  svg?: string;
  title: string;
  caption: string;
  messages: string[];
  error?: string;
  exact: boolean;
}

/** SVG のモバイル向けの描画部品の確認（崩れる機能と、部品ごとの見え方。android・ios・flutter・reactNative） */
export interface SvgCheckView {
  issues: { targets: string[]; feature: string; message: string; count: number }[];
  previews: Record<string, MobilePreview>;
}

/** 「すべてのファイル」で開いたファイル */
export type FileView =
  | {
      kind: 'text';
      lines: ViewLine[];
      /** 最後の行の後で消した行の数 */
      removedAtEnd?: number;
      /** Markdown なら、整形して見せるブロック（コアの `markdown::Block`） */
      markdown?: unknown[];
      /** 数式の大きさ（本文の字に対する倍率。設定 `amonhen.math.formulaSize`） */
      mathScale?: number;
      /** SVG なら、絵とモバイルの確認 */
      svg?: { image: ImageBlob; check?: SvgCheckView };
      /** Android の VectorDrawable なら、描いた絵と、絵の下に添える説明（引けなかった色など） */
      drawable?: { image: ImageBlob; notes: string[] };
    }
  | { kind: 'image'; image: ImageBlob }
  | { kind: 'binary'; bytes: number }
  /** partial clone で中身が手元にない（取りに行かずに「取ってくる」の帯で聞く） */
  | { kind: 'missing' }
  | { kind: 'error'; message: string };

/**
 * Markdown の図・数式と VectorDrawable の絵に当てる、Webview の画面の色（`#rrggbb`。コアの `PictureColors`）。
 * `kinds` は構文の種類（`keyword` など）→ 色
 */
export interface PictureColors {
  dark: boolean;
  text: string;
  background: string;
  kinds: Record<string, string>;
}

/** 中身に添えること（このコミットで消したファイルは、前の版を出す） */
export type FileViewNote = { deleted?: boolean };

/** いくつも選んだコミットの見せ方（`commitDiff` の `history`・`rows`）。差分はファイルごとに求める */
export interface SelectionView {
  /** 差分を求めるときの `oid` の代わり */
  key: string;
  count: number;
  /** 続いたコミット・離れた 2 つ・離れた 3 つ以上 */
  kind: 'range' | 'pair' | 'each';
  pair: PairMode;
  /** `missing` は partial clone で中身が手元になく、差分を取っていないファイル */
  files: { path: string; added: number; removed: number; binary: boolean; missing?: boolean }[];
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
  /** コミットのメッセージを等幅で出すか（表示のメニューの Monospaced Commit Message。すべての詳細の欄） */
  | { type: 'messageFont'; monospace: boolean }
  /** 密度の帯: 区間ごとの頻度と動き（0〜1）を交互に並べた f32 の base64 */
  | { type: 'density'; values: string }
  /** コマンド（Find in History…）から、検索の欄にフォーカスを移す */
  | { type: 'focusSearch' }
  /** `select` があれば、新しい History のその行を選ぶ（一致だけの History との切り替え） */
  | { type: 'reset'; title: string; scope: Scope; select?: number }
  /** `total` があれば daemon の History で、行は `range` で頼んだところだけ届く（`done` は常に true） */
  | { type: 'rows'; start: number; rows: RowData[]; done: boolean; total?: number }
  /** 検索できるか（daemon の History のときだけ） */
  /** その行を選んで見せる（Reflog の Show in History） */
  | { type: 'reveal'; row: number }
  | { type: 'searchable'; searchable: boolean }
  /** 行に出さない項目 */
  | { type: 'historyDisplay'; hidden: HistoryItem[]; oneLine: boolean }
  /** 詳細の欄の、変わったファイルの見せ方 */
  | { type: 'detailsLayout'; layout: DetailsLayout }
  /**
   * 編集モードを開く（行は新しい順。最初の変更を入れてある。`original` は入れる前の並び）。`editMessageAt` の行は、
   * すぐメッセージの編集欄を開く
   */
  | { type: 'editMode'; rows: EditRow[]; original: string[]; editMessageAt?: number }
  /** 編集モードの範囲を広げた: いちばん古い行より古いコミット（新しい順） */
  | { type: 'editMore'; rows: EditRow[] }
  /** 編集モードの見通し（`key` は頼んだときのもの）。`conflicts` はコンフリクトしそうなコミット */
  | { type: 'editPreview'; key: number; lines: string[]; conflicts: string[]; stacked?: EditStacked }
  /** Apply を終えた。`ok` なら編集モードを閉じ、そうでなければ開いたまま（もう一度押せる） */
  | { type: 'editApplied'; ok: boolean }
  /** 祖先の行（ビットの列を base64 で。`row` の下位から）。目立たせないなら null */
  | { type: 'lineage'; rows: string | null; edges?: string }
  | { type: 'search'; state: SearchState }
  | { type: 'busy'; busy: boolean }
  | { type: 'details'; details: CommitDetails; choices?: DiffChoices }
  /** いくつも選んだコミットの差分。`token` は頼んだときのもの。読めなければ `error` */
  | { type: 'selection'; token: number; view?: SelectionView; error?: string; choices?: DiffChoices }
  /** コミットの中の画像の前と後（足した・消したものは片方。読めなければ `error`） */
  | { type: 'commitImage'; oid: string; path: string; before?: ImageBlob; after?: ImageBlob; error?: string }
  /** コミットの木のフォルダ `path` の中身（「すべてのファイル」） */
  | { type: 'treeEntries'; oid: string; path: string; entries: TreeEntry[]; error?: string }
  /** コミットのファイル `path` の中身（「すべてのファイル」） */
  | { type: 'treeFile'; oid: string; path: string; view: FileView & FileViewNote }
  /** ファイルの右クリックの Show Whole File（その詳細を出している欄が、ファイル全体を頼む） */
  | { type: 'wholeFile'; oid: string; path: string }
  /** ファイルの見出しのメニュー: 開く・畳む（toggle）、ファイル全体をやめて変えたところだけに戻す（changesOnly） */
  | { type: 'fileAction'; oid: string; path: string; action: 'toggle' | 'changesOnly' }
  /** Open File at This Commit（見出し・差分の行の右クリック）。`line` は後の側の行番号 */
  | { type: 'openAtCommit'; oid: string; path: string; line?: number }
  /** コミットの中の 1 ファイルの差分（最初の親との差分） */
  | { type: 'commitDiff'; oid: string; path: string; diff: FileDiff }
  /**
   * partial clone で手元になかった中身を取り終えた（`fetchContents` の答え）。`path` があれば「すべてのファイル」の
   * そのファイル。取れなければ `error`（差分なら、取れたときは詳細を送り直してある）
   */
  | { type: 'contentsFetched'; oid: string; path?: string; error?: string }
  /** `last` はまとめる範囲のいちばん新しいコミット（combine のとき。`oid` がいちばん古い） */
  | { type: 'editMessage'; oid: string; last?: string; kind: MessageEdit; title: string; message: string }
  /** 行の署名の印（届いた行の分。署名のないコミットは含まない）。`reset` なら前の印を捨ててから（検証の設定が変わった） */
  | { type: 'signatures'; marks: Record<string, SignatureView>; reset?: boolean }
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
  /** 署名の形式（GPG・SSH・X.509）と鍵の指紋（分かれば。アプリの署名の吹き出しと同じ） */
  format?: string;
  fingerprint?: string;
}

/** コミットを指すタグ・merge したタグ（注釈付きならタガー・日時・件名と署名） */
export interface TagView {
  name: string;
  tagger?: string;
  /** UNIX 時刻（秒） */
  time?: number;
  subject?: string;
  signature?: SignatureView;
}

/** コミット情報の見せ方（アプリの表示項目: 簡潔・標準・詳細） */
export type DetailsStyle = 'brief' | 'standard' | 'full';

/** コミットの 1 行の要約（merge で取り込んだもの、メッセージが指すもの） */
export interface CommitSummaryView {
  oid: string;
  subject: string;
  author?: string;
}

/** 詳細（Full）だけに出す項目（コアの `commitExtras`） */
export interface FullExtras {
  /** HEAD からの道（`HEAD~2^2` など）。HEAD が含まなければ undefined */
  headRev?: string;
  branches: string[];
  /** 含むタグ（古いものから 50 まで）と、その数 */
  tags: string[];
  tagCount: number;
  previousTag?: string;
  /** 含むリモートのブランチ。リモートがあるのにどれも含まなければ空（Not pushed）。リモートがなければ undefined */
  pushed?: string[];
  /** merge で取り込んだコミット（20 まで）と、その数 */
  merged: CommitSummaryView[];
  mergedCount: number;
  /** メッセージが指すコミット（Cherry-picked from・Reverts・Fixes・Mentions） */
  related: { kind: string; commit: CommitSummaryView }[];
}

/** コミットの詳細のうち時間のかかる分（コアの `commitExtras`）。署名と子のコミット */
export interface DetailsExtras {
  oid: string;
  /** 署名がなければ undefined */
  signature?: SignatureView;
  /** そのコミットを親に持つコミット */
  children: string[];
  /** 詳細（Full）の項目 */
  full?: FullExtras;
  /** このコミットを指すタグ（エンジンの `tagObjects`） */
  tags?: TagView[];
  /** この merge が取り込んだタグ（コミットの mergetag） */
  mergedTags?: TagView[];
  /** merge で取り込んだコミットの数（merge でなければ 0） */
  mergedCount?: number;
  /** git notes（refs/notes/commits）。なければ undefined */
  notes?: string;
}

export type HistoryWebviewMessage =
  | { type: 'more' }
  /** 密度の帯（コミットの頻度とブランチの動き）を buckets 区間で頼む（daemon の History だけ） */
  | { type: 'density'; buckets: number }
  /** merge を 1 つ開く・畳む（← →。右クリックの Show・Fold Merged Commits と同じ） */
  | { type: 'toggleFold'; oid: string }
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
  /** コミットの木のフォルダ `path`（根は空）の中身を求める（「すべてのファイル」。エンジンだけ） */
  | { type: 'treeEntries'; oid: string; path: string }
  /**
   * コミットのファイル `path` の中身を求める（「すべてのファイル」。エンジンだけ）。`colors` は図・数式・VectorDrawable
   * の絵に当てる色、`timeZone` は Webview の時間帯の IANA の名前（設定の時間帯が「OS の時間帯」のとき、gantt の日付を
   * 数える）
   */
  | { type: 'treeFile'; oid: string; path: string; colors?: PictureColors; timeZone?: string }
  /** そのコミットのファイルをエディタで開く（「すべてのファイル」のバイナリ） */
  | { type: 'openTreeFile'; oid: string; path: string; line?: number }
  /** コミットの中の画像の前と後を求める（画像の差分） */
  | { type: 'commitImage'; oid: string; path: string }
  /** そのファイルの差分を、前後 `context` 行（all ならファイル全体）で求め直す（エンジンだけ） */
  | { type: 'fileContext'; oid: string; path: string; context: number | 'all' }
  /** 無視する空白を選び直す（出している詳細を読み直す。覚えない） */
  | { type: 'whitespace'; whitespace: DiffWhitespace }
  | { type: 'detailsLayout'; layout: DetailsLayout }
  /** 編集モードの見通しを求める。`updateRefs` は、積み重ねたブランチを今回だけどうするか（触っていなければなし） */
  | { type: 'editPreview'; key: number; steps: PlanStep[]; updateRefs?: boolean }
  /** 編集モードの範囲の境目を下へ引いた: oldest より古い count 個のコミットを取り込む */
  | { type: 'editExtend'; oldest: string; count: number }
  /** 編集モードの変更を当てる */
  | { type: 'editApply'; steps: PlanStep[]; updateRefs?: boolean }
  | { type: 'scope'; scope: Scope }
  /** 並び順と項目の一覧を出す */
  | { type: 'displayMenu' }
  /** 畳んだ merge を開く・畳む */
  | { type: 'toggleFold'; oid: string }
  | { type: 'saveMessage'; oid: string; last?: string; kind: MessageEdit; message: string }
  /** 編集できる範囲の中で、新しい順の to 番目に動かす */
  | { type: 'move'; oid: string; to: number }
  /** History で行に重ねて落とした: oid を onto にまとめる（編集モードを、その変更を入れて開く） */
  | { type: 'squashOnto'; oid: string; onto: string }
  /** HEAD・親・子のコミットに移る（`oid` は今選んでいるコミット） */
  | { type: 'goTo'; to: 'head' | 'parent' | 'child'; oid?: string }
  /** shallow clone の履歴を取り足す・すべて取る、partial clone の中身をすべて取る（帯と札のボタン） */
  | { type: 'fetchForm'; what: 'more' | 'all' | 'contents' }
  /**
   * partial clone で手元にない中身を取ってくる（「取ってくる」の帯のボタン。押したときだけ）。`oid` はコミット（か
   * 選択の key）で、その差分のファイルの分。`path` があれば「すべてのファイル」のそのファイルの分
   */
  | { type: 'fetchContents'; oid: string; path?: string };
