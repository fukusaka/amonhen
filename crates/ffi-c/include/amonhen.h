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

/**
 * ref の一覧。
 */
typedef struct AmonhenRefs AmonhenRefs;

/**
 * 開いたリポジトリ。
 */
typedef struct AmonhenRepo AmonhenRepo;

/**
 * リポジトリの監視。
 */
typedef struct AmonhenWatch AmonhenWatch;

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
} AmonhenEdge;
/**
 * first-parent の線（幹線として太く描く）。
 */
#define AmonhenEdge_FIRST_PARENT (1 << 0)

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
  uint8_t _pad[3];
  /**
   * 変更前・変更後の行番号（1 始まり）。その側にない行は 0。
   */
  uint32_t old_no;
  uint32_t new_no;
  AmonhenStrRef text;
} DiffRow;
#define DiffRow_CONTEXT 0
#define DiffRow_ADDED 1
#define DiffRow_REMOVED 2
/**
 * `@@ -1,3 +1,4 @@`
 */
#define DiffRow_HUNK 3

typedef struct {
  const DiffRow *rows;
  uint32_t row_count;
  /**
   * 文字列置き場の先頭アドレス。`StrRef::buf` で引く。
   */
  const uint8_t *const *bases;
  uint32_t base_count;
  uint8_t binary;
  uint32_t added;
  uint32_t removed;
} AmonhenDiffView;

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

/**
 * このスレッドで最後に失敗した関数の理由（UTF-8）。次に失敗するまで有効。
 */
const char *amonhen_last_error(void);

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
 * `history` の `row` 行目のコミット ID を `out`（20 バイト）に書く。範囲外なら false。
 *
 * # Safety
 * `history` は有効なスナップショット、`out` は 20 バイト書ける領域であること。
 */
bool amonhen_history_commit_id(const AmonhenHistory *history_ptr,
                               uint32_t row,
                               uint8_t *out);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* AMONHEN_H */
