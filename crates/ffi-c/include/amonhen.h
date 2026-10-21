/* cbindgen が生成する。手で編集しないこと。 */

#ifndef AMONHEN_H
#define AMONHEN_H

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

/**
 * 変化の種類（`amonhen_repo_watch` のコールバックに渡すビット）。
 */
#define AMONHEN_CHANGE_REFS (1 << 0)

#define AMONHEN_CHANGE_INDEX (1 << 1)

#define AMONHEN_CHANGE_WORKTREE (1 << 2)

#define AMONHEN_CHANGE_CONFIG (1 << 3)

/**
 * ASCII 以外の文字列の符号化。
 */
enum AmonhenEncoding
#if defined(__cplusplus) || __STDC_VERSION__ >= 202311L
  : uint8_t
#endif // defined(__cplusplus) || __STDC_VERSION__ >= 202311L
 {
  AmonhenEncoding_Utf16 = 0,
  AmonhenEncoding_Utf8 = 1,
};
#ifndef __cplusplus
#if __STDC_VERSION__ >= 202311L
typedef enum AmonhenEncoding AmonhenEncoding;
#else
typedef uint8_t AmonhenEncoding;
#endif // __STDC_VERSION__ >= 202311L
#endif // __cplusplus

/**
 * コミットで変わったファイルの一覧。
 */
typedef struct AmonhenChanges AmonhenChanges;

/**
 * History の連続した行の、FE 向けの列。
 */
typedef struct AmonhenChunk AmonhenChunk;

/**
 * ファイル 1 つの差分。
 */
typedef struct AmonhenDiff AmonhenDiff;

/**
 * History のスナップショット。
 */
typedef struct AmonhenHistory AmonhenHistory;

typedef struct AmonhenLineage AmonhenLineage;

/**
 * ref の一覧。
 */
typedef struct AmonhenRefs AmonhenRefs;

/**
 * 開いたリポジトリ。
 */
typedef struct AmonhenRepo AmonhenRepo;

/**
 * 行番号の並び（検索の結果）。
 */
typedef struct AmonhenRows AmonhenRows;

/**
 * コミットの全ファイルの差分（畳むファイルを変えて表示を作り直す）。
 */
typedef struct AmonhenStacked AmonhenStacked;

/**
 * 作業ツリーと index の状態。
 */
typedef struct AmonhenStatus AmonhenStatus;

/**
 * 差分の構文の色付け。
 */
typedef struct AmonhenSyntax AmonhenSyntax;

/**
 * リポジトリの監視。
 */
typedef struct AmonhenWatch AmonhenWatch;

/**
 * Working Copy の 1 ファイルの差分。
 */
typedef struct AmonhenWorkingDiff AmonhenWorkingDiff;

/**
 * グラフの点（コミット 1 つ）。
 */
typedef struct {
  uint32_t row;
  uint16_t lane;
  uint16_t color;
  uint16_t flags;
  uint16_t _pad;
} AmonhenNode;
#define AmonhenNode_MERGE (1 << 0)
#define AmonhenNode_HEAD (1 << 1)
#define AmonhenNode_STASH (1 << 2)
/**
 * 未コミットの変更を表す仮の点。
 */
#define AmonhenNode_WORKING_COPY (1 << 3)

/**
 * グラフの線 1 本。長い縦線も行ごとに分けず、区間 1 つで表す。
 */
typedef struct {
  uint32_t row_start;
  uint32_t row_end;
  uint16_t lane_start;
  uint16_t lane_end;
  uint16_t color;
  uint16_t kind;
  /**
   * この線がつなぐ子と親の行（祖先の強調に使う）。縦線と終わりの曲線はレーンの持ち主の子のもの。
   */
  uint32_t child;
  uint32_t parent;
} AmonhenEdge;
/**
 * first-parent の線（幹線として太く描く）。
 */
#define AmonhenEdge_FIRST_PARENT (1 << 0)
/**
 * ほかの線が待っている親に合流する線の、始まりの曲線。そこから先の縦線と終わりの曲線は、
 * 合流先のレーンの持ち主が描く。
 */
#define AmonhenEdge_RIDER (1 << 1)

typedef struct {
  uint64_t generation;
  uint32_t row_count;
  uint16_t lane_count;
  /**
   * 行ごとの点。`row_count` 個。
   */
  const AmonhenNode *nodes;
  const AmonhenEdge *edges;
  uint32_t edge_count;
} AmonhenHistoryView;

/**
 * 祖先の強調の結果（`amonhen_lineage_view` で読む）。
 */
typedef struct {
  /**
   * 行ごとのビット（64 行ずつ）。立っていれば、選んだコミット（を含む）の祖先。
   */
  const uint64_t *rows;
  uint32_t row_word_count;
  /**
   * 線ごとの、光らせ始める行（`u32::MAX` なら光らせない）。添字は History の線と同じ。
   */
  const uint32_t *edge_from;
  uint32_t edge_count;
} AmonhenLineageView;

/**
 * レーンの幅の決め方（`amonhen_history_lane_positions`）。単位は FE の座標。
 */
typedef struct {
  /**
   * 点のあるレーンの幅。
   */
  float full;
  /**
   * 線だけのレーンの幅。
   */
  float thin;
  float min_full;
  float min_thin;
  /**
   * 使える幅。
   */
  float available;
} AmonhenLaneMetrics;

typedef struct {
  const uint32_t *rows;
  uint32_t count;
} AmonhenRowsView;

/**
 * 文字列置き場の中の 1 つの文字列。
 *
 * ASCII の文字列は全 FE で共通のバイト列を指し、`len` はバイト数。
 * ASCII 以外は FE の好む符号化（UTF-8 か UTF-16）の領域を指し、`len` はその符号単位の数。
 *
 * `buf` は文字列置き場の番号、`off` はその中のバイト位置。同じプロセス内では、ASCII の文字列は
 * git オブジェクトを展開したバッファをそのまま指す（集め直さない）。共有メモリ・ファイル・
 * ネットワークに出すときは 1 つの領域に集め、`buf` はすべて 0 にする。
 */
typedef struct {
  uint32_t buf;
  uint32_t off;
  /**
   * 最上位ビットが立っていれば ASCII 以外。残りの 31 ビットが長さ。
   */
  uint32_t len_and_kind;
} AmonhenStrRef;

typedef struct {
  uint32_t start_row;
  uint32_t row_count;
  /**
   * 選んだ列だけ `row_count` 個。選ばなかった列は NULL。
   */
  const AmonhenStrRef *subjects;
  const AmonhenStrRef *author_names;
  const int64_t *author_times;
  /**
   * 行ごとの署名の種類（0 はなし、1 GPG、2 SSH、3 X.509）。`row_count` 個。検証の結果ではない。
   */
  const uint8_t *signatures;
  /**
   * 文字列置き場の先頭アドレス。`StrRef::buf` で引き、`off` バイト進めた所から読む。
   */
  const uint8_t *const *bases;
  uint32_t base_count;
} AmonhenChunkView;

/**
 * ref の 1 項目。文字列は `AmonhenRefsView::strings` の中の UTF-8。
 */
typedef struct {
  /**
   * 0: ローカルブランチ、1: リモートブランチ、2: タグ、3: stash、4: サブモジュール。
   */
  uint8_t kind;
  uint8_t has_target;
  uint8_t _pad[2];
  uint32_t name_off;
  uint32_t name_len;
  /**
   * 上流ブランチ・stash のメッセージ。なければ `detail_len` が `u32::MAX`。
   */
  uint32_t detail_off;
  uint32_t detail_len;
  uint8_t target[20];
  /**
   * ローカルブランチの、上流より先・後ろにあるコミットの数。
   */
  uint32_t ahead;
  uint32_t behind;
  /**
   * 上流のリモートブランチがもうない。
   */
  uint8_t gone;
  uint8_t _pad2[3];
} AmonhenRefEntry;

typedef struct {
  uint64_t generation;
  const AmonhenRefEntry *entries;
  uint32_t count;
  const uint8_t *strings;
  /**
   * チェックアウト中のブランチ。detached HEAD なら `head_branch_len` が `u32::MAX`。
   */
  uint32_t head_branch_off;
  uint32_t head_branch_len;
  /**
   * HEAD のコミット。まだなければ `has_head` が 0。
   */
  uint8_t has_head;
  uint8_t head[20];
} AmonhenRefsView;

/**
 * 変わったファイル 1 つ。文字列は `AmonhenChangesView::strings` の中の UTF-8。
 */
typedef struct {
  /**
   * 0: 追加、1: 削除、2: 変更、3: 名前の変更、4: コピー。
   */
  uint8_t kind;
  uint8_t submodule;
  uint8_t _pad[2];
  uint32_t path_off;
  uint32_t path_len;
  /**
   * 名前を変えた・コピーしたときの元のパス。なければ `old_path_len` が `u32::MAX`。
   */
  uint32_t old_path_off;
  uint32_t old_path_len;
} AmonhenChangeEntry;

typedef struct {
  const AmonhenChangeEntry *entries;
  uint32_t count;
  const uint8_t *strings;
} AmonhenChangesView;

/**
 * 差分の 1 行。
 */
typedef struct {
  uint8_t kind;
  /**
   * `STAGED` など。
   */
  uint8_t flags;
  uint8_t _pad[2];
  /**
   * 変更前・変更後の行番号（1 始まり）。その側にない行は 0。
   */
  uint32_t old_no;
  uint32_t new_no;
  AmonhenStrRef text;
} AmonhenDiffRow;
#define AmonhenDiffRow_CONTEXT 0
#define AmonhenDiffRow_ADDED 1
#define AmonhenDiffRow_REMOVED 2
/**
 * `@@ -1,3 +1,4 @@`
 */
#define AmonhenDiffRow_HUNK 3
/**
 * コンフリクトの Current 側。
 */
#define AmonhenDiffRow_OURS 4
/**
 * コンフリクトの Incoming 側。
 */
#define AmonhenDiffRow_THEIRS 5
/**
 * コンフリクトの共通の祖先（diff3）。
 */
#define AmonhenDiffRow_BASE 6
/**
 * `<<<<<<<` などのマーカーの行。
 */
#define AmonhenDiffRow_MARKER 7
/**
 * コミットの全ファイルを並べた差分の、ファイルの見出し（文字列はパス、`old_no`・`new_no` は削除・追加の行数）。
 */
#define AmonhenDiffRow_FILE 8
/**
 * `flags`: Working Copy の HEAD と作業ツリーの差分で、この行の変更が index に入っている（次のコミットに含める）。
 */
#define AmonhenDiffRow_STAGED (1 << 0)
/**
 * `flags`: ファイルの見出しで、そのファイルの行を畳んでいる。
 */
#define AmonhenDiffRow_COLLAPSED (1 << 1)
/**
 * `flags`: ファイルの見出しで、バイナリのファイル（行の差分はない）。
 */
#define AmonhenDiffRow_BINARY (1 << 2)

/**
 * 差分の行の中で強調する範囲（単語単位の変更）。位置は FE の文字列の符号単位。
 */
typedef struct {
  uint32_t row;
  uint32_t start;
  uint32_t end;
} AmonhenHighlight;

typedef struct {
  const AmonhenDiffRow *rows;
  uint32_t row_count;
  /**
   * 文字列置き場の先頭アドレス。`StrRef::buf` で引く。
   */
  const uint8_t *const *bases;
  uint32_t base_count;
  uint8_t binary;
  uint32_t added;
  uint32_t removed;
  /**
   * 削除行と追加行の組の中で、変わった単語の範囲（行の順）。
   */
  const AmonhenHighlight *highlights;
  uint32_t highlight_count;
} AmonhenDiffView;

/**
 * 差分の行の中の、構文の色付けの範囲。位置は FE の文字列の符号単位。
 */
typedef struct {
  uint32_t row;
  uint32_t start;
  uint32_t end;
  uint8_t kind;
  uint8_t _pad[3];
} AmonhenSyntaxSpan;
#define AmonhenSyntaxSpan_KEYWORD 1
#define AmonhenSyntaxSpan_STRING 2
#define AmonhenSyntaxSpan_COMMENT 3
#define AmonhenSyntaxSpan_NUMBER 4
#define AmonhenSyntaxSpan_FUNCTION 5
#define AmonhenSyntaxSpan_TYPE 6
#define AmonhenSyntaxSpan_CONSTANT 7
#define AmonhenSyntaxSpan_PROPERTY 8
#define AmonhenSyntaxSpan_ATTRIBUTE 9
#define AmonhenSyntaxSpan_TAG 10
#define AmonhenSyntaxSpan_OPERATOR 11
/**
 * `self`・`this` などの組み込みの変数や、引数。
 */
#define AmonhenSyntaxSpan_VARIABLE 12
/**
 * 文字列の中のエスケープや正規表現など。
 */
#define AmonhenSyntaxSpan_ESCAPE 13
#define AmonhenSyntaxSpan_MODULE 14

typedef struct {
  /**
   * 行の順、行の中では位置の順。
   */
  const AmonhenSyntaxSpan *spans;
  uint32_t span_count;
} AmonhenSyntaxView;

/**
 * status の 1 項目。文字列は `AmonhenStatusView::strings` の中の UTF-8。
 */
typedef struct {
  /**
   * 0: 変更、1: 名前の変更、2: コンフリクト、3: untracked。
   */
  uint8_t kind;
  /**
   * git の XY。変更なしは `.`、untracked は `?`。
   */
  uint8_t index;
  uint8_t worktree;
  uint8_t submodule;
  uint32_t path_off;
  uint32_t path_len;
  /**
   * 名前を変えたときの元のパス。なければ `orig_path_len` が `u32::MAX`。
   */
  uint32_t orig_path_off;
  uint32_t orig_path_len;
} AmonhenStatusEntry;

typedef struct {
  uint64_t generation;
  const AmonhenStatusEntry *entries;
  uint32_t count;
  const uint8_t *strings;
  /**
   * チェックアウト中のブランチ。detached HEAD なら `branch_len` が `u32::MAX`。
   */
  uint32_t branch_off;
  uint32_t branch_len;
  /**
   * 上流ブランチ（`origin/main`）。なければ `upstream_len` が `u32::MAX`。
   */
  uint32_t upstream_off;
  uint32_t upstream_len;
  uint32_t ahead;
  uint32_t behind;
} AmonhenStatusView;

/**
 * Working Copy の差分のファイルの改行（`amonhen_working_diff_line_endings`）。改行は 0: なし、1: LF、2: CRLF、
 * 3: 混在。`present` が false なら情報がない（stage 済みの差分・サブモジュール・消したファイル）。
 */
typedef struct {
  bool present;
  /**
   * git（index）に入っている形。
   */
  uint8_t index;
  uint8_t worktree;
  /**
   * index の内容を checkout したときの形。
   */
  uint8_t checkout;
  /**
   * 見せる値打ちがある（改行を変換している、または `warning`）。
   */
  bool notable;
  /**
   * 改行が混ざっている、または作業ツリーが CRLF なのに checkout すると LF になる。
   */
  bool warning;
} AmonhenLineEndings;

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

/**
 * このスレッドで最後に失敗した関数の理由（UTF-8）。次に失敗するまで有効。
 */
const char *amonhen_last_error(void);

/**
 * 手元のリポジトリの status で、git の fsmonitor（`git fsmonitor--daemon`）を使うか（macOS と Windows）。
 * リポジトリごとに daemon が常駐し、`.git` にソケットを作る。
 */
void amonhen_set_fsmonitor(bool enabled);

/**
 * SSH 先のフォルダの下のリポジトリを探す（リポジトリの一覧の、SSH 先のスキャンするフォルダ）。`location` は
 * `ssh://[user@]host[:port]/path`。エンジンの用意は SSH 先のリポジトリを開くときと同じ（`daemon`・`daemons_dir`）。
 * `excludes` は入らないフォルダの名前を改行でつないだもの。見つけたリポジトリの場所（`ssh://host/path`）を改行で
 * つないで返す（`amonhen_string_free` で解放）。失敗したら NULL（`amonhen_last_error`）。
 *
 * # Safety
 * `location`・`daemon`・`daemons_dir`・`excludes` は NUL 終端の UTF-8 であること。
 */
char *amonhen_find_remote_repositories(const char *location,
                                       uint32_t depth,
                                       const char *excludes,
                                       const char *daemon,
                                       const char *daemons_dir);

/**
 * daemon 越しのリポジトリを開く。`location` は `daemon:/path`（手元の共有の daemon。いなければ `daemon` を起動）か
 * `ssh://[user@]host[:port]/path`（SSH 先に daemon を送り込んで起動）。SSH 先に送る daemon は
 * `daemons_dir` の `amonhend-<os>-<arch>`（SSH 先が手元と同じ OS と CPU なら `daemon`）。ssh のコマンドは
 * 環境変数 `AMONHEN_SSH` で差し替えられる。`cache_dir` を渡すと History を永続化する。
 *
 * # Safety
 * `location`・`daemon`・`daemons_dir` は NUL 終端の UTF-8、`cache_dir` は NULL か NUL 終端の UTF-8 であること。
 */
AmonhenRepo *amonhen_repo_open_location(const char *location,
                                        const char *daemon,
                                        const char *daemons_dir,
                                        const char *cache_dir);

/**
 * `path`（UTF-8）を含むリポジトリを開く。`cache_dir` を渡すと、History をそこに永続化する。
 *
 * # Safety
 * `path` は NUL 終端の文字列、`cache_dir` は NULL か NUL 終端の文字列であること。
 */
AmonhenRepo *amonhen_repo_open(const char *path,
                               const char *cache_dir);

/**
 * # Safety
 * `repo` は `amonhen_repo_open` が返したもので、まだ解放していないこと。
 */
void amonhen_repo_free(AmonhenRepo *repo);

/**
 * History を読み直して公開し、そのスナップショットを返す（`amonhen_history_release` で手放す）。
 *
 * # Safety
 * `repo` は有効なリポジトリであること。
 */
const AmonhenHistory *amonhen_history_load(const AmonhenRepo *repo_ptr);

/**
 * History の並び順を変える（`order` は 0 が日付順、1 がトポロジー順）。公開している History があれば、
 * git を読み直さずにその順に並べ直して公開し直す（大きなリポジトリでは秒単位。裏のスレッドから呼ぶこと）。
 * 手元の共有の daemon につないでいれば、daemon の分も変える。失敗なら false。
 *
 * # Safety
 * `repo` は有効なリポジトリであること。
 */
bool amonhen_history_set_order(const AmonhenRepo *repo_ptr,
                               uint32_t order);

/**
 * History を最初から作り直して公開し、そのスナップショットを返す（`amonhen_history_release` で手放す）。
 * 永続キャッシュを捨てる（手元の共有の daemon につないでいれば、daemon の分も）。裏のスレッドから呼ぶこと。
 *
 * # Safety
 * `repo` は有効なリポジトリであること。
 */
const AmonhenHistory *amonhen_history_rebuild(const AmonhenRepo *repo_ptr);

/**
 * 永続キャッシュにある前回の History を公開して返す。キャッシュがなければ NULL。
 * mmap するだけなので、起動直後にメインスレッドから呼んでよい。
 *
 * # Safety
 * `repo` は有効なリポジトリであること。
 */
const AmonhenHistory *amonhen_history_load_cached(const AmonhenRepo *repo_ptr);

/**
 * commit-graph がなければ書き出す（`git commit-graph write --reachable`）。書き出したら 1、
 * すでにあれば 0、失敗したら -1 を返す。利用者が設定で選んだときだけ、裏のスレッドから呼ぶこと。
 *
 * # Safety
 * `repo` は有効なリポジトリであること。
 */
int32_t amonhen_repo_write_commit_graph_if_missing(const AmonhenRepo *repo_ptr);

/**
 * 最新のスナップショット。まだ読んでいなければ NULL。
 *
 * # Safety
 * `repo` は有効なリポジトリであること。
 */
const AmonhenHistory *amonhen_history_acquire(const AmonhenRepo *repo_ptr);

/**
 * # Safety
 * `history` は `amonhen_history_load` / `amonhen_history_acquire` が返したもので、まだ手放していないこと。
 */
void amonhen_history_release(const AmonhenHistory *history);

/**
 * # Safety
 * `history` は有効なスナップショットであること。返したポインタは手放すまで有効。
 */
AmonhenHistoryView amonhen_history_view(const AmonhenHistory *history_ptr);

/**
 * `[start, end)` 行に重なる線の添字を `out` に最大 `capacity` 個書き、全体の個数を返す。
 *
 * # Safety
 * `history` は有効なスナップショット、`out` は `capacity` 個の `u32` を書ける領域であること。
 */
uint32_t amonhen_history_edges_in(const AmonhenHistory *history_ptr,
                                  uint32_t start,
                                  uint32_t end,
                                  uint32_t *out,
                                  uint32_t capacity);

/**
 * `row` 行目のコミットとその祖先、祖先をつなぐ線（`amonhen_lineage_free` で解放する）。History の大きさに
 * 比例した時間がかかる（linux で 10〜25 ms）ので、裏のスレッドで呼ぶ。
 *
 * # Safety
 * `history` は有効なスナップショットであること。
 */
AmonhenLineage *amonhen_history_lineage(const AmonhenHistory *history_ptr,
                                        uint32_t row);

/**
 * # Safety
 * `lineage` は有効であること。返したポインタは解放するまで有効。
 */
AmonhenLineageView amonhen_lineage_view(const AmonhenLineage *lineage);

/**
 * # Safety
 * `lineage` は `amonhen_history_lineage` が返したもので、まだ解放していないこと。
 */
void amonhen_lineage_free(AmonhenLineage *lineage);

/**
 * `start..end` 行を表示するときの、レーンごとの中心の横位置（魚眼）を `out` に書く。見えている範囲に点の
 * あるレーンを広く、線だけのレーンを細く、何もないレーンを幅 0 にする。
 * レーンの数を返す（`capacity` を超えた分は書かない）。`width` が NULL でなければ、レーン全体の幅を書く。
 *
 * # Safety
 * `history` は有効なスナップショット、`out` は `capacity` 個の `float` を書ける領域、`width` は NULL か
 * `float` を 1 つ書ける領域であること。
 */
uint32_t amonhen_history_lane_positions(const AmonhenHistory *history_ptr,
                                        uint32_t start,
                                        uint32_t end,
                                        AmonhenLaneMetrics metrics,
                                        float *out,
                                        uint32_t capacity,
                                        float *width);

/**
 * `query` に合うコミットの行（昇順、`amonhen_rows_free` で解放する）。構文は `amonhen-core` の
 * `query`（`author:` `subject:` `commit:` `path:` `contents:`、AND・OR・括弧）。すべてのコミットを読むので
 * 裏のスレッドで呼ぶ（linux の 148 万コミットで約 1.2 秒）。新しい検索を始めるか `amonhen_search_cancel` を
 * 呼ぶと、走っている検索は NULL を返す。
 *
 * # Safety
 * `repo` と `history` は有効、`query` は NUL 終端の UTF-8 であること。
 */
AmonhenRows *amonhen_history_search(const AmonhenRepo *repo_ptr,
                                    const AmonhenHistory *history_ptr,
                                    const char *query);

/**
 * 走っている検索をやめる。
 *
 * # Safety
 * `repo` は有効であること。
 */
void amonhen_search_cancel(const AmonhenRepo *repo_ptr);

/**
 * # Safety
 * `rows` は有効であること。返したポインタは解放するまで有効。
 */
AmonhenRowsView amonhen_rows_view(const AmonhenRows *rows);

/**
 * # Safety
 * `rows` は `amonhen_history_search` が返したもので、まだ解放していないこと。
 */
void amonhen_rows_free(AmonhenRows *rows);

/**
 * `start` 行から `count` 行分の列を作る（`amonhen_chunk_free` で解放する）。
 *
 * # Safety
 * `repo` と `history` は有効であること。
 */
AmonhenChunk *amonhen_history_chunk(const AmonhenRepo *repo_ptr,
                                    const AmonhenHistory *history_ptr,
                                    uint32_t start,
                                    uint32_t count,
                                    AmonhenEncoding encoding);

/**
 * # Safety
 * `chunk` は有効であること。返したポインタは解放するまで有効。
 */
AmonhenChunkView amonhen_chunk_view(const AmonhenChunk *chunk);

/**
 * # Safety
 * `chunk` は `amonhen_history_chunk` が返したもので、まだ解放していないこと。
 */
void amonhen_chunk_free(AmonhenChunk *chunk);

/**
 * `history` で `oid`（20 バイト）が何行目か。なければ -1。
 *
 * # Safety
 * `history` は有効なスナップショット、`oid` は 20 バイト読める領域であること。
 */
int64_t amonhen_history_row_of(const AmonhenHistory *history_ptr,
                               const uint8_t *oid);

/**
 * ref の一覧を読み直して返す（`amonhen_refs_release` で手放す）。前回と同じなら同じ世代。
 *
 * # Safety
 * `repo` は有効なリポジトリであること。
 */
const AmonhenRefs *amonhen_refs_load(const AmonhenRepo *repo_ptr);

/**
 * # Safety
 * `refs` は `amonhen_refs_load` が返したもので、まだ手放していないこと。
 */
void amonhen_refs_release(const AmonhenRefs *refs);

/**
 * # Safety
 * `refs` は有効であること。返したポインタは手放すまで有効。
 */
AmonhenRefsView amonhen_refs_view(const AmonhenRefs *refs);

/**
 * リポジトリの監視を始める。変化があると、短い間の変化をまとめてから `callback(context, 変化の種類)`
 * を監視用のスレッドから呼ぶ。`amonhen_watch_free` で止める。
 *
 * # Safety
 * `repo` は有効なリポジトリ。`callback` は `amonhen_watch_free` まで、どのスレッドから呼んでもよいこと。
 */
AmonhenWatch *amonhen_repo_watch(const AmonhenRepo *repo_ptr,
                                 void (*callback)(void*, uint32_t),
                                 void *context);

/**
 * # Safety
 * `watch` は `amonhen_repo_watch` が返したもので、まだ止めていないこと。
 */
void amonhen_watch_free(AmonhenWatch *watch);

/**
 * `commit`（20 バイト）で変わったファイル（`amonhen_changes_free` で解放する）。
 *
 * # Safety
 * `repo` は有効なリポジトリ、`commit` は 20 バイト読める領域であること。
 */
AmonhenChanges *amonhen_commit_changes(const AmonhenRepo *repo_ptr,
                                       const uint8_t *commit);

/**
 * stash（20 バイト）の中身。untracked のファイルを含めていればそれも（`amonhen_changes_free` で解放する）。
 *
 * # Safety
 * `repo` は有効なリポジトリ、`stash` は 20 バイト読める領域であること。
 */
AmonhenChanges *amonhen_stash_changes(const AmonhenRepo *repo_ptr,
                                      const uint8_t *stash);

/**
 * コミット `base`（20 バイト。NULL なら全ファイルの追加）から `tip`（20 バイト）への変更で変わったファイル。
 * push で送る範囲（upstream..HEAD）などに（`amonhen_changes_free` で解放する）。
 *
 * # Safety
 * `repo` は有効なリポジトリ、`base` は NULL か 20 バイト読める領域、`tip` は 20 バイト読める領域であること。
 */
AmonhenChanges *amonhen_range_changes(const AmonhenRepo *repo_ptr,
                                      const uint8_t *base,
                                      const uint8_t *tip);

/**
 * # Safety
 * `changes` は有効であること。返したポインタは解放するまで有効。
 */
AmonhenChangesView amonhen_changes_view(const AmonhenChanges *changes);

/**
 * # Safety
 * `changes` は `amonhen_commit_changes` が返したもので、まだ解放していないこと。
 */
void amonhen_changes_free(AmonhenChanges *changes);

/**
 * `changes` の `index` 番目のファイルの差分（`amonhen_diff_free` で解放する）。
 *
 * # Safety
 * `repo` と `changes` は有効であること。
 */
AmonhenDiff *amonhen_file_diff(const AmonhenRepo *repo_ptr,
                               const AmonhenChanges *changes,
                               uint32_t index,
                               AmonhenEncoding encoding);

/**
 * `changes` のすべてのファイルの差分（`amonhen_stacked_free` で解放する）。ファイルの数だけ blob を読むので、
 * 裏のスレッドで呼ぶ。
 *
 * # Safety
 * `repo` と `changes` は有効であること。
 */
AmonhenStacked *amonhen_stacked_diff(const AmonhenRepo *repo_ptr,
                                     const AmonhenChanges *changes);

/**
 * 全ファイルを見出しの行（`DiffRow::FILE`）でつないだ差分の表示（`amonhen_diff_free` で解放する。
 * `amonhen_diff_view`・`amonhen_diff_syntax` が使える）。`collapsed` の i 番目が true のファイルは見出しだけ。
 *
 * # Safety
 * `stacked` は有効、`collapsed` は `count` 個の `bool` を読める領域（`count` が 0 なら NULL でもよい）であること。
 */
AmonhenDiff *amonhen_stacked_view(const AmonhenStacked *stacked,
                                  const bool *collapsed,
                                  uint32_t count,
                                  AmonhenEncoding encoding);

/**
 * History で選んだコミット（`rows` は行、順不同）の全ファイルの差分（`amonhen_stacked_free` で解放する）。
 * `kind` に見せ方を書く: 0 は first-parent でつながる範囲（一番古いものの親から一番新しいものまで）、1 はつながって
 * いない 2 つ（古いほうから新しいほうまで）、2 はとびとびの 3 つ以上（コミットごとの差分を、見出し
 * 「パス — 短い ID」でパス順に）。`pair` は 2 つのときの取り方: 0 は古いほうから新しいほう、1 は逆、2 は共通の
 * 祖先から新しいほう（A...B）。裏のスレッドで呼ぶ。
 *
 * # Safety
 * `repo` と `history` は有効、`rows` は `count` 個の `u32`、`kind` は書き込める `u8` であること。
 */
AmonhenStacked *amonhen_selection_diff(const AmonhenRepo *repo_ptr,
                                       const AmonhenHistory *history_ptr,
                                       const uint32_t *rows,
                                       uint32_t count,
                                       uint8_t pair,
                                       uint8_t *kind);

/**
 * ファイルの数。
 *
 * # Safety
 * `stacked` は有効であること。
 */
uint32_t amonhen_stacked_count(const AmonhenStacked *stacked);

/**
 * # Safety
 * `stacked` は `amonhen_stacked_diff` が返したもので、まだ解放していないこと。
 */
void amonhen_stacked_free(AmonhenStacked *stacked);

/**
 * # Safety
 * `diff` は有効であること。返したポインタは解放するまで有効。
 */
AmonhenDiffView amonhen_diff_view(const AmonhenDiff *diff);

/**
 * # Safety
 * `diff` は `amonhen_file_diff` が返したもので、まだ解放していないこと。
 */
void amonhen_diff_free(AmonhenDiff *diff);

/**
 * 差分の構文の色付け（`amonhen_syntax_free` で解放する）。ファイル全体を解析するので、差分を表示してから
 * 裏のスレッドで呼ぶ。言語がわからなければ空。
 *
 * # Safety
 * `diff` は `amonhen_file_diff`・`amonhen_conflict_view` が返したもので、まだ解放していないこと。
 */
AmonhenSyntax *amonhen_diff_syntax(const AmonhenDiff *diff);

/**
 * # Safety
 * `diff` は `amonhen_working_diff` が返したもので、まだ解放していないこと。
 */
AmonhenSyntax *amonhen_working_diff_syntax(const AmonhenWorkingDiff *diff);

/**
 * テキスト全体の構文の色付け（`amonhen_syntax_free` で解放する）。範囲はテキストの先頭からの UTF-16 の
 * 位置で、`row` は 0。言語は `path` の拡張子で決め、わからなければ空。時間がかかるので裏のスレッドで呼ぶ。
 *
 * # Safety
 * `path` は NUL 終端の UTF-8、`text` は `len` バイト読める（`len` が 0 なら NULL でもよい）こと。
 */
AmonhenSyntax *amonhen_text_syntax(const char *path,
                                   const uint8_t *text,
                                   uintptr_t len);

/**
 * # Safety
 * `syntax` は有効であること。返したポインタは解放するまで有効。
 */
AmonhenSyntaxView amonhen_syntax_view(const AmonhenSyntax *syntax);

/**
 * # Safety
 * `syntax` は `amonhen_diff_syntax`・`amonhen_working_diff_syntax`・`amonhen_text_syntax` が返したもので、
 * まだ解放していないこと。
 */
void amonhen_syntax_free(AmonhenSyntax *syntax);

/**
 * `history` の `row` 行目のコミット ID を `out`（20 バイト）に書く。範囲外なら false。
 *
 * # Safety
 * `history` は有効なスナップショット、`out` は 20 バイト書ける領域であること。
 */
bool amonhen_history_commit_id(const AmonhenHistory *history_ptr,
                               uint32_t row,
                               uint8_t *out);

/**
 * 作業ツリーと index の状態を読み直す（`amonhen_status_release` で手放す）。前回と同じなら同じ世代。
 *
 * # Safety
 * `repo` は有効なリポジトリであること。
 */
const AmonhenStatus *amonhen_status_load(const AmonhenRepo *repo_ptr);

/**
 * # Safety
 * `status` は `amonhen_status_load` が返したもので、まだ手放していないこと。
 */
void amonhen_status_release(const AmonhenStatus *status);

/**
 * # Safety
 * `status` は有効であること。返したポインタは手放すまで有効。
 */
AmonhenStatusView amonhen_status_view(const AmonhenStatus *status);

/**
 * `status` の `index` 番目のファイルの差分。`staged` なら HEAD と index、そうでなければ index と作業ツリー
 * （`amonhen_working_diff_free` で解放する）。
 *
 * # Safety
 * `repo` と `status` は有効であること。
 */
AmonhenWorkingDiff *amonhen_working_diff(const AmonhenRepo *repo_ptr,
                                         const AmonhenStatus *status,
                                         uint32_t index,
                                         bool staged,
                                         AmonhenEncoding encoding);

/**
 * `status` の `index` 番目のファイルの、HEAD と作業ツリーの差分。変更の行ごとに、index に入っているかを
 * `DiffRow` の `flags`（`STAGED`）で示す（`amonhen_working_diff_free` で解放する）。index がその形で表せない
 * ファイルでは、`amonhen_last_error` を空にして NULL を返す（FE は `amonhen_working_diff` で 2 つに分けて見せる）。
 *
 * # Safety
 * `repo` と `status` は有効であること。
 */
AmonhenWorkingDiff *amonhen_working_diff_combined(const AmonhenRepo *repo_ptr,
                                                  const AmonhenStatus *status,
                                                  uint32_t index,
                                                  AmonhenEncoding encoding);

/**
 * # Safety
 * `diff` は `amonhen_working_diff` が返したもので、まだ解放していないこと。
 */
AmonhenLineEndings amonhen_working_diff_line_endings(const AmonhenWorkingDiff *diff);

/**
 * かかっている改行の変換（`text=auto eol=crlf`・`core.autocrlf=true` など。`amonhen_string_free` で解放する）。
 * 見せる値打ちがないとき・変換がないときは NULL。
 *
 * # Safety
 * `diff` は `amonhen_working_diff` が返したもので、まだ解放していないこと。
 */
char *amonhen_working_diff_line_ending_rule(const AmonhenWorkingDiff *diff);

/**
 * # Safety
 * `diff` は有効であること。返したポインタは解放するまで有効。
 */
AmonhenDiffView amonhen_working_diff_view(const AmonhenWorkingDiff *diff);

/**
 * # Safety
 * `diff` は `amonhen_working_diff` が返したもので、まだ解放していないこと。
 */
void amonhen_working_diff_free(AmonhenWorkingDiff *diff);

/**
 * `paths` の変更（追加・削除を含む）を stage する。
 *
 * # Safety
 * `repo` は有効なリポジトリ、`paths` は NUL 終端の文字列 `count` 個の配列であること。
 */
bool amonhen_stage(const AmonhenRepo *repo_ptr,
                   const char *const *paths_ptr,
                   uint32_t count);

/**
 * `paths` を unstage する（index を HEAD の状態に戻す）。
 *
 * # Safety
 * `repo` は有効なリポジトリ、`paths` は NUL 終端の文字列 `count` 個の配列であること。
 */
bool amonhen_unstage(const AmonhenRepo *repo_ptr,
                     const char *const *paths_ptr,
                     uint32_t count);

/**
 * `diff` の `rows` 行目の変更だけを stage する（stage 済みの差分なら unstage する）。
 *
 * # Safety
 * `repo` と `diff` は有効、`rows` は `count` 個の `u32` を読める領域であること。
 */
bool amonhen_stage_rows(const AmonhenRepo *repo_ptr,
                        const AmonhenWorkingDiff *diff,
                        const uint32_t *rows,
                        uint32_t count);

/**
 * `amonhen_working_diff_combined` の差分の `rows` 行目の変更を、`staged` なら index に入れ、そうでなければ外す。
 *
 * # Safety
 * `repo` と `diff` は有効、`rows` は `count` 個の `u32` を読める領域であること。
 */
bool amonhen_set_rows_staged(const AmonhenRepo *repo_ptr,
                             const AmonhenWorkingDiff *diff,
                             const uint32_t *rows,
                             uint32_t count,
                             bool staged);

/**
 * stage した変更をコミットする。`amend` なら直前のコミットを作り直す。
 *
 * # Safety
 * `repo` は有効なリポジトリ、`message` は NUL 終端の文字列であること。
 */
bool amonhen_commit(const AmonhenRepo *repo_ptr, const char *message, bool amend);

/**
 * JSON の命令（`{"op":"merge","rev":"topic"}` など）を実行し、JSON の結果
 * （`{"ok":true,"outcome":"done"}` か `{"ok":false,"error":"…"}`）を返す。結果は
 * `amonhen_string_free` で解放する。頻度の低い操作はすべてここを通す。
 *
 * # Safety
 * `repo` は有効なリポジトリ、`command` は NUL 終端の UTF-8 文字列であること。
 */
char *amonhen_command(const AmonhenRepo *repo_ptr,
                      const char *command);

/**
 * Activity の記録（git の起動とコマンドの実行、古い順、最大 1000 件）の JSON の配列。`amonhen_string_free` で
 * 解放する。
 */
char *amonhen_activity(void);

/**
 * 手元で走っている操作（Activity の記録の番号 `seq`）を取り消す。取り消せれば true。
 */
bool amonhen_activity_cancel(uint64_t seq);

/**
 * `url` を `dest`（まだないフォルダ）に clone する。失敗したら false（理由は `amonhen_last_error`）。時間が
 * かかるので裏のスレッドで呼ぶ。
 *
 * # Safety
 * `url` と `dest` は NUL 終端の UTF-8 文字列であること。
 */
bool amonhen_clone(const char *url,
                   const char *dest);

/**
 * `path` のリポジトリの軽い状態（リポジトリの一覧に出す）。行に分けた文字列で、順にブランチ（detached なら
 * 空）、上流があれば 1、ahead、behind、変更のあるファイルの数（untracked は数えない）。読めなければ NULL。
 * `amonhen_string_free` で解放する。git を 1 回起動するので、裏のスレッドで呼ぶ。
 *
 * # Safety
 * `path` は NUL 終端の UTF-8 文字列であること。
 */
char *amonhen_peek(const char *path);

/**
 * # Safety
 * `s` は `amonhen_command`・`amonhen_peek` が返したもので、まだ解放していないこと。
 */
void amonhen_string_free(char *s);

/**
 * `diff` の `rows` 行目の変更を作業ツリーから取り除く（Undo できる）。stage していない差分にだけ使える。
 *
 * # Safety
 * `repo` と `diff` は有効、`rows` は `count` 個の `u32` を読める領域であること。
 */
bool amonhen_discard_rows(const AmonhenRepo *repo_ptr,
                          const AmonhenWorkingDiff *diff,
                          const uint32_t *rows,
                          uint32_t count);

/**
 * 作業ツリーのルート（`amonhen_string_free` で解放する）。bare リポジトリなら NULL。
 *
 * # Safety
 * `repo` は有効なリポジトリであること。
 */
char *amonhen_repo_workdir(const AmonhenRepo *repo_ptr);

/**
 * daemon 越しのリポジトリのコミットの署名を、手元の鍵で検証するか（false なら SSH 先の git と鍵で検証する）。
 * 手元のリポジトリには効かない（いつも手元で検証する）。変えると、覚えていた検証の結果は捨てる。
 *
 * # Safety
 * `repo` は有効なリポジトリであること。
 */
void amonhen_repo_set_local_signature_check(const AmonhenRepo *repo_ptr,
                                            bool local);

/**
 * daemon とのつながりの様子。手元のリポジトリなら -1、つながっていれば 0、切れていれば 1 を返す。
 * `received`・`sent` が NULL でなければ、これまでに受け取った・送ったバイト数を書く。切れた理由は
 * `amonhen_repo_disconnect_reason` で読む。
 *
 * # Safety
 * `repo` は有効なリポジトリ、`received`・`sent` は NULL か書き込める `u64` を指すこと。
 */
int32_t amonhen_repo_connection(const AmonhenRepo *repo_ptr,
                                uint64_t *received,
                                uint64_t *sent);

/**
 * SSH 先で使っているエンジンと、その用意のしかた（`amonhen_string_free` で解放する）。SSH のつながりでなければ NULL。
 *
 * # Safety
 * `repo` は有効なリポジトリであること。
 */
char *amonhen_repo_remote_engine(const AmonhenRepo *repo_ptr);

/**
 * daemon との接続が切れた理由（`amonhen_string_free` で解放する）。切れていなければ NULL。
 *
 * # Safety
 * `repo` は有効なリポジトリであること。
 */
char *amonhen_repo_disconnect_reason(const AmonhenRepo *repo_ptr);

/**
 * コンフリクトのある作業ツリーのファイルを、差分と同じ形の行にする（`amonhen_diff_free` で解放する）。
 * 行の `kind` は Current・Incoming・Base・マーカーを表し、コンフリクトの行の `old_no` はその番号 + 1。
 *
 * # Safety
 * `repo` は有効なリポジトリ、`path` は NUL 終端の UTF-8 文字列であること。
 */
AmonhenDiff *amonhen_conflict_view(const AmonhenRepo *repo_ptr,
                                   const char *path,
                                   AmonhenEncoding encoding);

/**
 * `tips`（20 バイトの ID が `count` 個）から辿れるコミットだけの History（ブランチで絞り込む）。
 * `amonhen_history_release` で手放す。
 *
 * # Safety
 * `repo` は有効なリポジトリ、`tips` は `count * 20` バイト読める領域であること。
 */
const AmonhenHistory *amonhen_history_filtered(const AmonhenRepo *repo_ptr,
                                               const uint8_t *tips,
                                               uint32_t count);

/**
 * マージ済みブランチを折りたたんだ History。`all` なら全ブランチ、そうでなければ `tips`（20 バイトの ID が
 * `count` 個）から辿れるものだけ。`expanded`（20 バイトの ID が `expanded_count` 個）の merge は取り込んだ側も
 * 見せる。`amonhen_history_release` で手放す。
 *
 * # Safety
 * `repo` は有効なリポジトリ、`tips` は `count * 20` バイト、`expanded` は `expanded_count * 20` バイト読める
 * 領域であること（個数が 0 なら NULL でよい）。
 */
const AmonhenHistory *amonhen_history_folded(const AmonhenRepo *repo_ptr,
                                             bool all,
                                             const uint8_t *tips,
                                             uint32_t count,
                                             const uint8_t *expanded,
                                             uint32_t expanded_count);

/**
 * 折りたたんだ History で、`row` の merge が隠しているコミットの数（隠していなければ 0）。
 *
 * # Safety
 * `history` は有効な History であること。
 */
uint32_t amonhen_history_hidden(const AmonhenHistory *history_ptr,
                                uint32_t row);

/**
 * スクロールバーに描く密度。`out` に区間ごとの [頻度, 活動量]（0〜1）を `buckets * 2` 個書き、書いた区間の数を
 * 返す（行が区間より少なければ行の数）。
 *
 * # Safety
 * `history` は有効な History、`out` は `buckets * 2` 個の float を書ける領域であること。
 */
uint32_t amonhen_history_density(const AmonhenHistory *history_ptr,
                                 uint32_t buckets,
                                 float *out);

/**
 * `history` の `rows`（行が `count` 個）のコミットだけの History（検索で一致したものだけを出す）。線は最初の親を
 * たどって次に一致したコミットへつなぐ。`amonhen_history_release` で手放す。
 *
 * # Safety
 * `repo`・`history` は有効なもの、`rows` は `count` 個の u32 を読める領域であること（0 個なら NULL でよい）。
 */
const AmonhenHistory *amonhen_history_matching(const AmonhenRepo *repo_ptr,
                                               const AmonhenHistory *history_ptr,
                                               const uint32_t *rows,
                                               uint32_t count);

/**
 * 手元の共有の daemon の様子（JSON。`amonhen_string_free` で解放する）。動いていなければ NULL。
 */
char *amonhen_daemon_status(void);

/**
 * 手元の共有の daemon を止める。動いていなければ false。
 */
bool amonhen_daemon_stop(void);

/**
 * 利用者に見せる文（エラーなど）の言語を決める（`ja`・`en` など。訳のない言語は英語）。アプリの画面と同じ言語を
 * 起動したときに渡す。daemon・SSH 先にも、つなぐときにこの言語を伝える。
 *
 * # Safety
 * `language` は NUL 終端の UTF-8 であること。
 */
void amonhen_set_language(const char *language);

/**
 * `message`（`amonhen_last_error` や操作の記録の文）が、取り消したことを表すか（どの言語の文でも）。
 *
 * # Safety
 * `message` は NUL 終端の UTF-8 であること。
 */
bool amonhen_is_cancelled(const char *message);

/**
 * 手元の共有の daemon を起動する（`daemon` は `amonhend` のパス）。失敗したら false（理由は `amonhen_last_error`）。
 *
 * # Safety
 * `daemon` は NUL 終端の UTF-8 であること。
 */
bool amonhen_daemon_start(const char *daemon);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* AMONHEN_H */
