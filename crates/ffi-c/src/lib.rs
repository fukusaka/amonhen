//! C ABI。History・差分・status の行データのように速さが要る経路はここを通す。
//!
//! - 返したポインタが指すメモリは、対応する `*_release` / `*_free` を呼ぶまで変わらない。
//! - スナップショットは不変なので、読むのにロックは要らない。どのスレッドから読んでもよい。
//! - 失敗した関数は NULL か false を返し、理由は `amonhen_last_error` で取れる。

use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char};
use std::sync::Arc;

use amonhen_backend_gix::GixBackend;
use amonhen_core::diff::DiffView;
use amonhen_core::{HistorySnapshot, RefsSnapshot, Repository, StatusSnapshot, WorkingDiff, cache};
use amonhen_layout::{DiffRow, Edge, Highlight, Node, StrRef, SyntaxSpan};
use amonhen_model::{FileChange, Oid};
use amonhen_projection::{FrontendProfile, HistoryChunk, StringEncoding};
use amonhen_watch::Watcher;
use std::path::Path;

/// 開いたリポジトリ。
pub struct AmonhenRepo {
    _private: [u8; 0],
}

/// History のスナップショット。
pub struct AmonhenHistory {
    _private: [u8; 0],
}

/// ref の一覧。
pub struct AmonhenRefs {
    _private: [u8; 0],
}

/// リポジトリの監視。
pub struct AmonhenWatch {
    _private: [u8; 0],
}

/// 手元ならファイルの監視、daemon 越しなら知らせを受けているかの印。
enum WatchHandle {
    Local(#[allow(dead_code)] Watcher),
    Remote(Arc<std::sync::atomic::AtomicBool>),
}

/// コミットで変わったファイルの一覧。
pub struct AmonhenChanges {
    _private: [u8; 0],
}

/// 作業ツリーと index の状態。
pub struct AmonhenStatus {
    _private: [u8; 0],
}

/// Working Copy の 1 ファイルの差分。
pub struct AmonhenWorkingDiff {
    _private: [u8; 0],
}

/// ファイル 1 つの差分。
pub struct AmonhenDiff {
    _private: [u8; 0],
}

/// コミットの全ファイルの差分（畳むファイルを変えて表示を作り直す）。
pub struct AmonhenStacked {
    _private: [u8; 0],
}

/// 差分の構文の色付け。
pub struct AmonhenSyntax {
    _private: [u8; 0],
}

/// 行番号の並び（検索の結果）。
pub struct AmonhenRows {
    _private: [u8; 0],
}

/// History の連続した行の、FE 向けの列。
pub struct AmonhenChunk {
    _private: [u8; 0],
}

/// ASCII 以外の文字列の符号化。
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AmonhenEncoding {
    Utf16 = 0,
    Utf8 = 1,
}

#[repr(C)]
pub struct AmonhenHistoryView {
    pub generation: u64,
    pub row_count: u32,
    pub lane_count: u16,
    /// 行ごとの点。`row_count` 個。
    pub nodes: *const Node,
    pub edges: *const Edge,
    pub edge_count: u32,
}

#[repr(C)]
pub struct AmonhenChunkView {
    pub start_row: u32,
    pub row_count: u32,
    /// 選んだ列だけ `row_count` 個。選ばなかった列は NULL。
    pub subjects: *const StrRef,
    pub author_names: *const StrRef,
    pub author_times: *const i64,
    /// 行ごとの署名の種類（0 はなし、1 GPG、2 SSH、3 X.509）。`row_count` 個。検証の結果ではない。
    pub signatures: *const u8,
    /// 文字列置き場の先頭アドレス。`StrRef::buf` で引き、`off` バイト進めた所から読む。
    pub bases: *const *const u8,
    pub base_count: u32,
}

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

fn set_error(e: impl std::fmt::Display) {
    let message = CString::new(e.to_string().replace('\0', " ")).unwrap_or_default();
    LAST_ERROR.with(|last| *last.borrow_mut() = message);
}

/// このスレッドで最後に失敗した関数の理由（UTF-8）。次に失敗するまで有効。
#[unsafe(no_mangle)]
pub extern "C" fn amonhen_last_error() -> *const c_char {
    LAST_ERROR.with(|last| last.borrow().as_ptr())
}

fn repo<'a>(repo: *const AmonhenRepo) -> &'a Repository {
    unsafe { &*repo.cast::<Repository>() }
}

fn history<'a>(history: *const AmonhenHistory) -> &'a HistorySnapshot {
    unsafe { &*history.cast::<HistorySnapshot>() }
}

fn into_history(snapshot: Arc<HistorySnapshot>) -> *const AmonhenHistory {
    Arc::into_raw(snapshot).cast()
}

fn profile(encoding: AmonhenEncoding) -> FrontendProfile {
    FrontendProfile {
        string: match encoding {
            AmonhenEncoding::Utf16 => StringEncoding::Utf16,
            AmonhenEncoding::Utf8 => StringEncoding::Utf8,
        },
        ..FrontendProfile::MACOS
    }
}

fn non_null<T>(slice: &[T]) -> *const T {
    if slice.is_empty() {
        std::ptr::null()
    } else {
        slice.as_ptr()
    }
}

fn str_arg<'a>(s: *const c_char) -> Result<&'a str, std::str::Utf8Error> {
    unsafe { CStr::from_ptr(s) }.to_str()
}

/// 手元のリポジトリの status で、git の fsmonitor（`git fsmonitor--daemon`）を使うか（macOS と Windows）。
/// リポジトリごとに daemon が常駐し、`.git` にソケットを作る。
#[unsafe(no_mangle)]
pub extern "C" fn amonhen_set_fsmonitor(enabled: bool) {
    amonhen_backend_cli::set_fsmonitor(enabled);
}

/// daemon 越しのリポジトリを開く。`location` は `daemon:/path`（手元の共有の daemon。いなければ `daemon` を起動）か
/// `ssh://[user@]host[:port]/path`（SSH 先に daemon を送り込んで起動）。SSH 先に送る daemon は
/// `daemons_dir` の `amonhend-<os>-<arch>`（SSH 先が手元と同じ OS と CPU なら `daemon`）。ssh のコマンドは
/// 環境変数 `AMONHEN_SSH` で差し替えられる。`cache_dir` を渡すと History を永続化する。
///
/// # Safety
/// `location`・`daemon`・`daemons_dir` は NUL 終端の UTF-8、`cache_dir` は NULL か NUL 終端の UTF-8 であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_repo_open_location(
    location: *const c_char,
    daemon: *const c_char,
    daemons_dir: *const c_char,
    cache_dir: *const c_char,
) -> *mut AmonhenRepo {
    let result = (|| -> amonhen_model::Result<Repository> {
        let location = str_arg(location)?;
        let daemon = str_arg(daemon)?;
        let conn = if location.starts_with("ssh://") {
            use amonhen_remote::ssh;
            let daemons = ssh::Daemons {
                dir: str_arg(daemons_dir)?.into(),
                local: Some((ssh::local_platform(), daemon.into())),
            };
            let program = std::env::var("AMONHEN_SSH").unwrap_or_else(|_| "ssh".into());
            ssh::connect(&program, &ssh::Target::parse(location)?, &daemons)?
        } else {
            // 手元の共有の daemon（いなければ `daemon` を起動する）につなぐ。
            let path = amonhen_remote::location::local_path(location)?;
            amonhen_remote::local::connect(path, daemon)?
        };
        let backend = amonhen_remote::RemoteBackend::connect(conn)?;
        // 永続キャッシュは場所の文字列ごとに手元に持つ（つなぎ直したら新しいコミットだけを受け取る）。
        Ok(if cache_dir.is_null() {
            Repository::new(backend)
        } else {
            Repository::with_cache(
                backend,
                cache::path_for(Path::new(str_arg(cache_dir)?), location),
            )
        })
    })();
    match result {
        Ok(repo) => Box::into_raw(Box::new(repo)).cast(),
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// `path`（UTF-8）を含むリポジトリを開く。`cache_dir` を渡すと、History をそこに永続化する。
///
/// # Safety
/// `path` は NUL 終端の文字列、`cache_dir` は NULL か NUL 終端の文字列であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_repo_open(
    path: *const c_char,
    cache_dir: *const c_char,
) -> *mut AmonhenRepo {
    let (path, cache_dir) = match (
        str_arg(path),
        (!cache_dir.is_null())
            .then(|| str_arg(cache_dir))
            .transpose(),
    ) {
        (Ok(p), Ok(c)) => (p, c),
        (Err(e), _) | (_, Err(e)) => {
            set_error(e);
            return std::ptr::null_mut();
        }
    };
    match GixBackend::open(path) {
        Ok(backend) => {
            let key = std::fs::canonicalize(path)
                .map_or_else(|_| path.to_owned(), |p| p.display().to_string());
            let repo = match cache_dir {
                Some(dir) => Repository::with_cache(backend, cache::path_for(Path::new(dir), &key)),
                None => Repository::new(backend),
            };
            Box::into_raw(Box::new(repo)).cast()
        }
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// # Safety
/// `repo` は `amonhen_repo_open` が返したもので、まだ解放していないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_repo_free(repo: *mut AmonhenRepo) {
    if !repo.is_null() {
        drop(unsafe { Box::from_raw(repo.cast::<Repository>()) });
    }
}

/// History を読み直して公開し、そのスナップショットを返す（`amonhen_history_release` で手放す）。
///
/// # Safety
/// `repo` は有効なリポジトリであること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_load(
    repo_ptr: *const AmonhenRepo,
) -> *const AmonhenHistory {
    match repo(repo_ptr).load_history() {
        Ok(snapshot) => into_history(snapshot),
        Err(e) => {
            set_error(e);
            std::ptr::null()
        }
    }
}

/// 永続キャッシュにある前回の History を公開して返す。キャッシュがなければ NULL。
/// mmap するだけなので、起動直後にメインスレッドから呼んでよい。
///
/// # Safety
/// `repo` は有効なリポジトリであること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_load_cached(
    repo_ptr: *const AmonhenRepo,
) -> *const AmonhenHistory {
    repo(repo_ptr)
        .load_cached_history()
        .map_or(std::ptr::null(), into_history)
}

/// commit-graph がなければ書き出す（`git commit-graph write --reachable`）。書き出したら 1、
/// すでにあれば 0、失敗したら -1 を返す。利用者が設定で選んだときだけ、裏のスレッドから呼ぶこと。
///
/// # Safety
/// `repo` は有効なリポジトリであること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_repo_write_commit_graph_if_missing(
    repo_ptr: *const AmonhenRepo,
) -> i32 {
    match repo(repo_ptr).write_commit_graph_if_missing() {
        Ok(written) => i32::from(written),
        Err(e) => {
            set_error(e);
            -1
        }
    }
}

/// 最新のスナップショット。まだ読んでいなければ NULL。
///
/// # Safety
/// `repo` は有効なリポジトリであること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_acquire(
    repo_ptr: *const AmonhenRepo,
) -> *const AmonhenHistory {
    repo(repo_ptr)
        .history()
        .map_or(std::ptr::null(), into_history)
}

/// # Safety
/// `history` は `amonhen_history_load` / `amonhen_history_acquire` が返したもので、まだ手放していないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_release(history: *const AmonhenHistory) {
    if !history.is_null() {
        drop(unsafe { Arc::from_raw(history.cast::<HistorySnapshot>()) });
    }
}

/// # Safety
/// `history` は有効なスナップショットであること。返したポインタは手放すまで有効。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_view(
    history_ptr: *const AmonhenHistory,
) -> AmonhenHistoryView {
    let h = history(history_ptr);
    AmonhenHistoryView {
        generation: h.generation,
        row_count: h.layout.nodes().len() as u32,
        lane_count: h.layout.lane_count(),
        nodes: non_null(h.layout.nodes()),
        edges: non_null(h.layout.edges()),
        edge_count: h.layout.edges().len() as u32,
    }
}

/// `[start, end)` 行に重なる線の添字を `out` に最大 `capacity` 個書き、全体の個数を返す。
///
/// # Safety
/// `history` は有効なスナップショット、`out` は `capacity` 個の `u32` を書ける領域であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_edges_in(
    history_ptr: *const AmonhenHistory,
    start: u32,
    end: u32,
    out: *mut u32,
    capacity: u32,
) -> u32 {
    let mut count = 0;
    for i in history(history_ptr).layout.edges_in(start..end) {
        if count < capacity {
            unsafe { out.add(count as usize).write(i) };
        }
        count += 1;
    }
    count
}

pub enum AmonhenLineage {}

/// 祖先の強調の結果（`amonhen_lineage_view` で読む）。
#[repr(C)]
pub struct AmonhenLineageView {
    /// 行ごとのビット（64 行ずつ）。立っていれば、選んだコミット（を含む）の祖先。
    pub rows: *const u64,
    pub row_word_count: u32,
    /// 線ごとの、光らせ始める行（`u32::MAX` なら光らせない）。添字は History の線と同じ。
    pub edge_from: *const u32,
    pub edge_count: u32,
}

/// `row` 行目のコミットとその祖先、祖先をつなぐ線（`amonhen_lineage_free` で解放する）。History の大きさに
/// 比例した時間がかかる（linux で 10〜25 ms）ので、裏のスレッドで呼ぶ。
///
/// # Safety
/// `history` は有効なスナップショットであること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_lineage(
    history_ptr: *const AmonhenHistory,
    row: u32,
) -> *mut AmonhenLineage {
    let h = history(history_ptr);
    let lineage = amonhen_core::lineage::lineage(&h.graph, &h.layout, row);
    Box::into_raw(Box::new(lineage)).cast()
}

/// # Safety
/// `lineage` は有効であること。返したポインタは解放するまで有効。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_lineage_view(
    lineage: *const AmonhenLineage,
) -> AmonhenLineageView {
    let l = unsafe { &*lineage.cast::<amonhen_core::lineage::Lineage>() };
    AmonhenLineageView {
        rows: non_null(&l.rows),
        row_word_count: l.rows.len() as u32,
        edge_from: non_null(&l.edge_from),
        edge_count: l.edge_from.len() as u32,
    }
}

/// # Safety
/// `lineage` は `amonhen_history_lineage` が返したもので、まだ解放していないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_lineage_free(lineage: *mut AmonhenLineage) {
    if !lineage.is_null() {
        drop(unsafe { Box::from_raw(lineage.cast::<amonhen_core::lineage::Lineage>()) });
    }
}

/// レーンの幅の決め方（`amonhen_history_lane_positions`）。単位は FE の座標。
#[repr(C)]
pub struct AmonhenLaneMetrics {
    /// 点のあるレーンの幅。
    pub full: f32,
    /// 線だけのレーンの幅。
    pub thin: f32,
    pub min_full: f32,
    pub min_thin: f32,
    /// 使える幅。
    pub available: f32,
}

/// `start..end` 行を表示するときの、レーンごとの中心の横位置（魚眼）を `out` に書く。見えている範囲に点の
/// あるレーンを広く、線だけのレーンを細く、何もないレーンを幅 0 にする。
/// レーンの数を返す（`capacity` を超えた分は書かない）。
///
/// # Safety
/// `history` は有効なスナップショット、`out` は `capacity` 個の `float` を書ける領域であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_lane_positions(
    history_ptr: *const AmonhenHistory,
    start: u32,
    end: u32,
    metrics: AmonhenLaneMetrics,
    out: *mut f32,
    capacity: u32,
) -> u32 {
    let m = amonhen_core::lanes::LaneMetrics {
        full: metrics.full,
        thin: metrics.thin,
        min_full: metrics.min_full,
        min_thin: metrics.min_thin,
        available: metrics.available,
    };
    let positions =
        amonhen_core::lanes::lane_positions(&history(history_ptr).layout, start..end, &m);
    for (i, x) in positions.iter().take(capacity as usize).enumerate() {
        unsafe { out.add(i).write(*x) };
    }
    positions.len() as u32
}

#[repr(C)]
pub struct AmonhenRowsView {
    pub rows: *const u32,
    pub count: u32,
}

/// `query` に合うコミットの行（昇順、`amonhen_rows_free` で解放する）。構文は `amonhen-core` の
/// `query`（`author:` `subject:` `commit:` `path:` `contents:`、AND・OR・括弧）。すべてのコミットを読むので
/// 裏のスレッドで呼ぶ（linux の 148 万コミットで約 1.2 秒）。新しい検索を始めるか `amonhen_search_cancel` を
/// 呼ぶと、走っている検索は NULL を返す。
///
/// # Safety
/// `repo` と `history` は有効、`query` は NUL 終端の UTF-8 であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_search(
    repo_ptr: *const AmonhenRepo,
    history_ptr: *const AmonhenHistory,
    query: *const c_char,
) -> *mut AmonhenRows {
    let result = str_arg(query)
        .map_err(Into::into)
        .and_then(|q| repo(repo_ptr).search_history(history(history_ptr), q));
    match result {
        Ok(rows) => Box::into_raw(Box::new(rows)).cast(),
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// 走っている検索をやめる。
///
/// # Safety
/// `repo` は有効であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_search_cancel(repo_ptr: *const AmonhenRepo) {
    repo(repo_ptr).cancel_search();
}

/// # Safety
/// `rows` は有効であること。返したポインタは解放するまで有効。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_rows_view(rows: *const AmonhenRows) -> AmonhenRowsView {
    let rows = unsafe { &*rows.cast::<Vec<u32>>() };
    AmonhenRowsView {
        rows: rows.as_ptr(),
        count: rows.len() as u32,
    }
}

/// # Safety
/// `rows` は `amonhen_history_search` が返したもので、まだ解放していないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_rows_free(rows: *mut AmonhenRows) {
    if !rows.is_null() {
        drop(unsafe { Box::from_raw(rows.cast::<Vec<u32>>()) });
    }
}

/// `start` 行から `count` 行分の列を作る（`amonhen_chunk_free` で解放する）。
///
/// # Safety
/// `repo` と `history` は有効であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_chunk(
    repo_ptr: *const AmonhenRepo,
    history_ptr: *const AmonhenHistory,
    start: u32,
    count: u32,
    encoding: AmonhenEncoding,
) -> *mut AmonhenChunk {
    match repo(repo_ptr).history_chunk(history(history_ptr), start, count, &profile(encoding)) {
        Ok(chunk) => Box::into_raw(Box::new(chunk)).cast(),
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// # Safety
/// `chunk` は有効であること。返したポインタは解放するまで有効。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_chunk_view(chunk: *const AmonhenChunk) -> AmonhenChunkView {
    let c = unsafe { &*chunk.cast::<HistoryChunk>() };
    AmonhenChunkView {
        start_row: c.start_row,
        row_count: c.len() as u32,
        subjects: non_null(&c.subjects),
        author_names: non_null(&c.author_names),
        author_times: non_null(&c.author_times),
        signatures: non_null(&c.signatures),
        bases: c.bases().as_ptr(),
        base_count: c.bases().len() as u32,
    }
}

/// # Safety
/// `chunk` は `amonhen_history_chunk` が返したもので、まだ解放していないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_chunk_free(chunk: *mut AmonhenChunk) {
    if !chunk.is_null() {
        drop(unsafe { Box::from_raw(chunk.cast::<HistoryChunk>()) });
    }
}

/// `history` で `oid`（20 バイト）が何行目か。なければ -1。
///
/// # Safety
/// `history` は有効なスナップショット、`oid` は 20 バイト読める領域であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_row_of(
    history_ptr: *const AmonhenHistory,
    oid: *const u8,
) -> i64 {
    let id = Oid(unsafe { *oid.cast::<[u8; 20]>() });
    history(history_ptr).row_of(&id).map_or(-1, i64::from)
}

/// ref の 1 項目。文字列は `AmonhenRefsView::strings` の中の UTF-8。
#[repr(C)]
pub struct AmonhenRefEntry {
    /// 0: ローカルブランチ、1: リモートブランチ、2: タグ、3: stash、4: サブモジュール。
    pub kind: u8,
    pub has_target: u8,
    pub _pad: [u8; 2],
    pub name_off: u32,
    pub name_len: u32,
    /// 上流ブランチ・stash のメッセージ。なければ `detail_len` が `u32::MAX`。
    pub detail_off: u32,
    pub detail_len: u32,
    pub target: [u8; 20],
    /// ローカルブランチの、上流より先・後ろにあるコミットの数。
    pub ahead: u32,
    pub behind: u32,
    /// 上流のリモートブランチがもうない。
    pub gone: u8,
    pub _pad2: [u8; 3],
}

#[repr(C)]
pub struct AmonhenRefsView {
    pub generation: u64,
    pub entries: *const AmonhenRefEntry,
    pub count: u32,
    pub strings: *const u8,
    /// チェックアウト中のブランチ。detached HEAD なら `head_branch_len` が `u32::MAX`。
    pub head_branch_off: u32,
    pub head_branch_len: u32,
    /// HEAD のコミット。まだなければ `has_head` が 0。
    pub has_head: u8,
    pub head: [u8; 20],
}

/// 一覧は数百〜数千件なので、文字列は 1 つの領域に詰めて渡す。
struct RefsHandle {
    snapshot: Arc<RefsSnapshot>,
    entries: Vec<AmonhenRefEntry>,
    strings: Vec<u8>,
    head_branch: (u32, u32),
}

impl RefsHandle {
    fn new(snapshot: Arc<RefsSnapshot>) -> Self {
        let mut strings = Vec::new();
        let mut put = |s: Option<&str>| match s {
            Some(s) => {
                let off = strings.len() as u32;
                strings.extend_from_slice(s.as_bytes());
                (off, s.len() as u32)
            }
            None => (0, u32::MAX),
        };
        let head_branch = put(snapshot.refs.head_branch.as_deref());
        let entries = snapshot
            .refs
            .entries
            .iter()
            .map(|e| {
                let (name_off, name_len) = put(Some(&e.name));
                let (detail_off, detail_len) = put(e.detail.as_deref());
                AmonhenRefEntry {
                    kind: e.kind as u8,
                    has_target: u8::from(e.target.is_some()),
                    _pad: [0; 2],
                    name_off,
                    name_len,
                    detail_off,
                    detail_len,
                    target: e.target.map_or([0; 20], |o| o.0),
                    ahead: e.ahead,
                    behind: e.behind,
                    gone: u8::from(e.gone),
                    _pad2: [0; 3],
                }
            })
            .collect();
        Self {
            snapshot,
            entries,
            strings,
            head_branch,
        }
    }
}

/// ref の一覧を読み直して返す（`amonhen_refs_release` で手放す）。前回と同じなら同じ世代。
///
/// # Safety
/// `repo` は有効なリポジトリであること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_refs_load(repo_ptr: *const AmonhenRepo) -> *const AmonhenRefs {
    match repo(repo_ptr).load_refs() {
        Ok(snapshot) => Arc::into_raw(Arc::new(RefsHandle::new(snapshot))).cast(),
        Err(e) => {
            set_error(e);
            std::ptr::null()
        }
    }
}

/// # Safety
/// `refs` は `amonhen_refs_load` が返したもので、まだ手放していないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_refs_release(refs: *const AmonhenRefs) {
    if !refs.is_null() {
        drop(unsafe { Arc::from_raw(refs.cast::<RefsHandle>()) });
    }
}

/// # Safety
/// `refs` は有効であること。返したポインタは手放すまで有効。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_refs_view(refs: *const AmonhenRefs) -> AmonhenRefsView {
    let r = unsafe { &*refs.cast::<RefsHandle>() };
    AmonhenRefsView {
        generation: r.snapshot.generation,
        entries: r.entries.as_ptr(),
        count: r.entries.len() as u32,
        strings: r.strings.as_ptr(),
        head_branch_off: r.head_branch.0,
        head_branch_len: r.head_branch.1,
        has_head: u8::from(r.snapshot.refs.head.is_some()),
        head: r.snapshot.refs.head.map_or([0; 20], |o| o.0),
    }
}

/// 変化の種類（`amonhen_repo_watch` のコールバックに渡すビット）。
pub const AMONHEN_CHANGE_REFS: u32 = 1 << 0;
pub const AMONHEN_CHANGE_INDEX: u32 = 1 << 1;
pub const AMONHEN_CHANGE_WORKTREE: u32 = 1 << 2;
pub const AMONHEN_CHANGE_CONFIG: u32 = 1 << 3;

struct Context(*mut std::ffi::c_void);

// FE が渡す文脈。コールバックで FE に返すだけで、Rust 側では触らない。
unsafe impl Send for Context {}
unsafe impl Sync for Context {}

/// リポジトリの監視を始める。変化があると、短い間の変化をまとめてから `callback(context, 変化の種類)`
/// を監視用のスレッドから呼ぶ。`amonhen_watch_free` で止める。
///
/// # Safety
/// `repo` は有効なリポジトリ。`callback` は `amonhen_watch_free` まで、どのスレッドから呼んでもよいこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_repo_watch(
    repo_ptr: *const AmonhenRepo,
    callback: extern "C" fn(*mut std::ffi::c_void, u32),
    context: *mut std::ffi::c_void,
) -> *mut AmonhenWatch {
    let context = Arc::new(Context(context));
    // daemon 越しなら daemon の知らせを使う。止めたら（`amonhen_watch_free`）呼ばない。
    let active = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let (remote_context, remote_active) = (context.clone(), active.clone());
    if repo(repo_ptr).subscribe_changes(Box::new(move |bits| {
        if remote_active.load(std::sync::atomic::Ordering::Relaxed) {
            callback(remote_context.0, bits);
        }
    })) {
        return Box::into_raw(Box::new(WatchHandle::Remote(active))).cast();
    }
    let watcher = Watcher::new(repo(repo_ptr).paths(), move |changes| {
        callback(context.0, changes.0);
    });
    match watcher {
        Ok(w) => Box::into_raw(Box::new(WatchHandle::Local(w))).cast(),
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// # Safety
/// `watch` は `amonhen_repo_watch` が返したもので、まだ止めていないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_watch_free(watch: *mut AmonhenWatch) {
    if !watch.is_null() {
        let handle = unsafe { Box::from_raw(watch.cast::<WatchHandle>()) };
        if let WatchHandle::Remote(active) = &*handle {
            active.store(false, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

/// 変わったファイル 1 つ。文字列は `AmonhenChangesView::strings` の中の UTF-8。
#[repr(C)]
pub struct AmonhenChangeEntry {
    /// 0: 追加、1: 削除、2: 変更、3: 名前の変更、4: コピー。
    pub kind: u8,
    pub submodule: u8,
    pub _pad: [u8; 2],
    pub path_off: u32,
    pub path_len: u32,
    /// 名前を変えた・コピーしたときの元のパス。なければ `old_path_len` が `u32::MAX`。
    pub old_path_off: u32,
    pub old_path_len: u32,
}

#[repr(C)]
pub struct AmonhenChangesView {
    pub entries: *const AmonhenChangeEntry,
    pub count: u32,
    pub strings: *const u8,
}

struct ChangesHandle {
    changes: Vec<FileChange>,
    entries: Vec<AmonhenChangeEntry>,
    strings: Vec<u8>,
}

/// `commit`（20 バイト）で変わったファイル（`amonhen_changes_free` で解放する）。
///
/// # Safety
/// `repo` は有効なリポジトリ、`commit` は 20 バイト読める領域であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_commit_changes(
    repo_ptr: *const AmonhenRepo,
    commit: *const u8,
) -> *mut AmonhenChanges {
    let id = Oid(unsafe { *commit.cast::<[u8; 20]>() });
    changes_handle(repo(repo_ptr).commit_changes(&id))
}

/// stash（20 バイト）の中身。untracked のファイルを含めていればそれも（`amonhen_changes_free` で解放する）。
///
/// # Safety
/// `repo` は有効なリポジトリ、`stash` は 20 バイト読める領域であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_stash_changes(
    repo_ptr: *const AmonhenRepo,
    stash: *const u8,
) -> *mut AmonhenChanges {
    let id = Oid(unsafe { *stash.cast::<[u8; 20]>() });
    changes_handle(repo(repo_ptr).stash_changes(&id))
}

fn changes_handle(result: amonhen_model::Result<Vec<FileChange>>) -> *mut AmonhenChanges {
    let changes = match result {
        Ok(c) => c,
        Err(e) => {
            set_error(e);
            return std::ptr::null_mut();
        }
    };
    let mut strings = Vec::new();
    let mut put = |s: Option<&str>| match s {
        Some(s) => {
            let off = strings.len() as u32;
            strings.extend_from_slice(s.as_bytes());
            (off, s.len() as u32)
        }
        None => (0, u32::MAX),
    };
    let entries = changes
        .iter()
        .map(|c| {
            let (path_off, path_len) = put(Some(&c.path));
            let (old_path_off, old_path_len) = put(c.old_path.as_deref());
            AmonhenChangeEntry {
                kind: c.kind as u8,
                submodule: u8::from(c.submodule),
                _pad: [0; 2],
                path_off,
                path_len,
                old_path_off,
                old_path_len,
            }
        })
        .collect();
    Box::into_raw(Box::new(ChangesHandle {
        changes,
        entries,
        strings,
    }))
    .cast()
}

/// # Safety
/// `changes` は有効であること。返したポインタは解放するまで有効。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_changes_view(
    changes: *const AmonhenChanges,
) -> AmonhenChangesView {
    let c = unsafe { &*changes.cast::<ChangesHandle>() };
    AmonhenChangesView {
        entries: c.entries.as_ptr(),
        count: c.entries.len() as u32,
        strings: c.strings.as_ptr(),
    }
}

/// # Safety
/// `changes` は `amonhen_commit_changes` が返したもので、まだ解放していないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_changes_free(changes: *mut AmonhenChanges) {
    if !changes.is_null() {
        drop(unsafe { Box::from_raw(changes.cast::<ChangesHandle>()) });
    }
}

#[repr(C)]
pub struct AmonhenDiffView {
    pub rows: *const DiffRow,
    pub row_count: u32,
    /// 文字列置き場の先頭アドレス。`StrRef::buf` で引く。
    pub bases: *const *const u8,
    pub base_count: u32,
    pub binary: u8,
    pub added: u32,
    pub removed: u32,
    /// 削除行と追加行の組の中で、変わった単語の範囲（行の順）。
    pub highlights: *const Highlight,
    pub highlight_count: u32,
}

/// `changes` の `index` 番目のファイルの差分（`amonhen_diff_free` で解放する）。
///
/// # Safety
/// `repo` と `changes` は有効であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_file_diff(
    repo_ptr: *const AmonhenRepo,
    changes: *const AmonhenChanges,
    index: u32,
    encoding: AmonhenEncoding,
) -> *mut AmonhenDiff {
    let c = unsafe { &*changes.cast::<ChangesHandle>() };
    let Some(change) = c.changes.get(index as usize) else {
        set_error("ファイルの番号が範囲外");
        return std::ptr::null_mut();
    };
    match repo(repo_ptr).file_diff(change, &profile(encoding)) {
        Ok(view) => Box::into_raw(Box::new(view)).cast(),
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// `changes` のすべてのファイルの差分（`amonhen_stacked_free` で解放する）。ファイルの数だけ blob を読むので、
/// 裏のスレッドで呼ぶ。
///
/// # Safety
/// `repo` と `changes` は有効であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_stacked_diff(
    repo_ptr: *const AmonhenRepo,
    changes: *const AmonhenChanges,
) -> *mut AmonhenStacked {
    let c = unsafe { &*changes.cast::<ChangesHandle>() };
    match repo(repo_ptr).stacked_diff(&c.changes) {
        Ok(s) => Box::into_raw(Box::new(s)).cast(),
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// 全ファイルを見出しの行（`DiffRow::FILE`）でつないだ差分の表示（`amonhen_diff_free` で解放する。
/// `amonhen_diff_view`・`amonhen_diff_syntax` が使える）。`collapsed` の i 番目が true のファイルは見出しだけ。
///
/// # Safety
/// `stacked` は有効、`collapsed` は `count` 個の `bool` を読める領域（`count` が 0 なら NULL でもよい）であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_stacked_view(
    stacked: *const AmonhenStacked,
    collapsed: *const bool,
    count: u32,
    encoding: AmonhenEncoding,
) -> *mut AmonhenDiff {
    let s = unsafe { &*stacked.cast::<amonhen_core::diff::StackedDiff>() };
    let collapsed: Vec<bool> = (0..count as usize)
        .map(|i| unsafe { *collapsed.add(i) })
        .collect();
    Box::into_raw(Box::new(s.view(&collapsed, &profile(encoding)))).cast()
}

/// History で選んだコミット（`rows` は行、順不同）の全ファイルの差分（`amonhen_stacked_free` で解放する）。
/// `kind` に見せ方を書く: 0 は first-parent でつながる範囲（一番古いものの親から一番新しいものまで）、1 はつながって
/// いない 2 つ（古いほうから新しいほうまで）、2 はとびとびの 3 つ以上（コミットごとの差分を、見出し
/// 「パス — 短い ID」でパス順に）。`pair` は 2 つのときの取り方: 0 は古いほうから新しいほう、1 は逆、2 は共通の
/// 祖先から新しいほう（A...B）。裏のスレッドで呼ぶ。
///
/// # Safety
/// `repo` と `history` は有効、`rows` は `count` 個の `u32`、`kind` は書き込める `u8` であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_selection_diff(
    repo_ptr: *const AmonhenRepo,
    history_ptr: *const AmonhenHistory,
    rows: *const u32,
    count: u32,
    pair: u8,
    kind: *mut u8,
) -> *mut AmonhenStacked {
    let rows: Vec<u32> = (0..count as usize)
        .map(|i| unsafe { *rows.add(i) })
        .collect();
    let pair = match pair {
        1 => amonhen_core::PairMode::Reverse,
        2 => amonhen_core::PairMode::MergeBase,
        _ => amonhen_core::PairMode::Forward,
    };
    match repo(repo_ptr).selection_diff(history(history_ptr), &rows, pair) {
        Ok((selection, stacked)) => {
            let k = match selection {
                amonhen_core::Selection::Range { .. } => 0,
                amonhen_core::Selection::Pair { .. } => 1,
                amonhen_core::Selection::Each(_) => 2,
            };
            unsafe { *kind = k };
            Box::into_raw(Box::new(stacked)).cast()
        }
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// ファイルの数。
///
/// # Safety
/// `stacked` は有効であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_stacked_count(stacked: *const AmonhenStacked) -> u32 {
    unsafe { &*stacked.cast::<amonhen_core::diff::StackedDiff>() }
        .files
        .len() as u32
}

/// # Safety
/// `stacked` は `amonhen_stacked_diff` が返したもので、まだ解放していないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_stacked_free(stacked: *mut AmonhenStacked) {
    if !stacked.is_null() {
        drop(unsafe { Box::from_raw(stacked.cast::<amonhen_core::diff::StackedDiff>()) });
    }
}

/// # Safety
/// `diff` は有効であること。返したポインタは解放するまで有効。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_diff_view(diff: *const AmonhenDiff) -> AmonhenDiffView {
    diff_view(unsafe { &*diff.cast::<DiffView>() })
}

fn diff_view(d: &DiffView) -> AmonhenDiffView {
    AmonhenDiffView {
        rows: d.rows.as_ptr(),
        row_count: d.rows.len() as u32,
        bases: d.strings.bases().as_ptr(),
        base_count: d.strings.bases().len() as u32,
        binary: u8::from(d.binary),
        added: d.added,
        removed: d.removed,
        highlights: d.highlights.as_ptr(),
        highlight_count: d.highlights.len() as u32,
    }
}

/// # Safety
/// `diff` は `amonhen_file_diff` が返したもので、まだ解放していないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_diff_free(diff: *mut AmonhenDiff) {
    if !diff.is_null() {
        drop(unsafe { Box::from_raw(diff.cast::<DiffView>()) });
    }
}

#[repr(C)]
pub struct AmonhenSyntaxView {
    /// 行の順、行の中では位置の順。
    pub spans: *const SyntaxSpan,
    pub span_count: u32,
}

/// 差分の構文の色付け（`amonhen_syntax_free` で解放する）。ファイル全体を解析するので、差分を表示してから
/// 裏のスレッドで呼ぶ。言語がわからなければ空。
///
/// # Safety
/// `diff` は `amonhen_file_diff`・`amonhen_conflict_view` が返したもので、まだ解放していないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_diff_syntax(diff: *const AmonhenDiff) -> *mut AmonhenSyntax {
    let spans = unsafe { &*diff.cast::<DiffView>() }.syntax();
    Box::into_raw(Box::new(spans)).cast()
}

/// # Safety
/// `diff` は `amonhen_working_diff` が返したもので、まだ解放していないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_working_diff_syntax(
    diff: *const AmonhenWorkingDiff,
) -> *mut AmonhenSyntax {
    let spans = unsafe { &*diff.cast::<WorkingDiff>() }.view.syntax();
    Box::into_raw(Box::new(spans)).cast()
}

/// テキスト全体の構文の色付け（`amonhen_syntax_free` で解放する）。範囲はテキストの先頭からの UTF-16 の
/// 位置で、`row` は 0。言語は `path` の拡張子で決め、わからなければ空。時間がかかるので裏のスレッドで呼ぶ。
///
/// # Safety
/// `path` は NUL 終端の UTF-8、`text` は `len` バイト読める（`len` が 0 なら NULL でもよい）こと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_text_syntax(
    path: *const c_char,
    text: *const u8,
    len: usize,
) -> *mut AmonhenSyntax {
    let path = str_arg(path).unwrap_or_default();
    let text = if len == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(text, len) }
    };
    Box::into_raw(Box::new(amonhen_core::diff::text_syntax(path, text))).cast()
}

/// # Safety
/// `syntax` は有効であること。返したポインタは解放するまで有効。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_syntax_view(syntax: *const AmonhenSyntax) -> AmonhenSyntaxView {
    let spans = unsafe { &*syntax.cast::<Vec<SyntaxSpan>>() };
    AmonhenSyntaxView {
        spans: spans.as_ptr(),
        span_count: spans.len() as u32,
    }
}

/// # Safety
/// `syntax` は `amonhen_diff_syntax`・`amonhen_working_diff_syntax`・`amonhen_text_syntax` が返したもので、
/// まだ解放していないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_syntax_free(syntax: *mut AmonhenSyntax) {
    if !syntax.is_null() {
        drop(unsafe { Box::from_raw(syntax.cast::<Vec<SyntaxSpan>>()) });
    }
}

/// `history` の `row` 行目のコミット ID を `out`（20 バイト）に書く。範囲外なら false。
///
/// # Safety
/// `history` は有効なスナップショット、`out` は 20 バイト書ける領域であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_commit_id(
    history_ptr: *const AmonhenHistory,
    row: u32,
    out: *mut u8,
) -> bool {
    match history(history_ptr).graph.ids().get(row as usize) {
        Some(id) => {
            unsafe { out.cast::<[u8; 20]>().write(id.0) };
            true
        }
        None => false,
    }
}

/// status の 1 項目。文字列は `AmonhenStatusView::strings` の中の UTF-8。
#[repr(C)]
pub struct AmonhenStatusEntry {
    /// 0: 変更、1: 名前の変更、2: コンフリクト、3: untracked。
    pub kind: u8,
    /// git の XY。変更なしは `.`、untracked は `?`。
    pub index: u8,
    pub worktree: u8,
    pub submodule: u8,
    pub path_off: u32,
    pub path_len: u32,
    /// 名前を変えたときの元のパス。なければ `orig_path_len` が `u32::MAX`。
    pub orig_path_off: u32,
    pub orig_path_len: u32,
}

#[repr(C)]
pub struct AmonhenStatusView {
    pub generation: u64,
    pub entries: *const AmonhenStatusEntry,
    pub count: u32,
    pub strings: *const u8,
    /// チェックアウト中のブランチ。detached HEAD なら `branch_len` が `u32::MAX`。
    pub branch_off: u32,
    pub branch_len: u32,
    /// 上流ブランチ（`origin/main`）。なければ `upstream_len` が `u32::MAX`。
    pub upstream_off: u32,
    pub upstream_len: u32,
    pub ahead: u32,
    pub behind: u32,
}

struct StatusHandle {
    snapshot: Arc<StatusSnapshot>,
    entries: Vec<AmonhenStatusEntry>,
    strings: Vec<u8>,
    branch: (u32, u32),
    upstream: (u32, u32),
}

/// 作業ツリーと index の状態を読み直す（`amonhen_status_release` で手放す）。前回と同じなら同じ世代。
///
/// # Safety
/// `repo` は有効なリポジトリであること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_status_load(repo_ptr: *const AmonhenRepo) -> *const AmonhenStatus {
    let snapshot = match repo(repo_ptr).load_status() {
        Ok(s) => s,
        Err(e) => {
            set_error(e);
            return std::ptr::null();
        }
    };
    let mut strings = Vec::new();
    let mut put = |s: Option<&str>| match s {
        Some(s) => {
            let off = strings.len() as u32;
            strings.extend_from_slice(s.as_bytes());
            (off, s.len() as u32)
        }
        None => (0, u32::MAX),
    };
    let branch = put(snapshot.status.branch.as_deref());
    let upstream = put(snapshot.status.upstream.as_deref());
    let entries = snapshot
        .status
        .entries
        .iter()
        .map(|e| {
            let (path_off, path_len) = put(Some(&e.path));
            let (orig_path_off, orig_path_len) = put(e.orig_path.as_deref());
            AmonhenStatusEntry {
                kind: e.kind as u8,
                index: e.index,
                worktree: e.worktree,
                submodule: u8::from(e.submodule),
                path_off,
                path_len,
                orig_path_off,
                orig_path_len,
            }
        })
        .collect();
    Arc::into_raw(Arc::new(StatusHandle {
        snapshot,
        entries,
        strings,
        branch,
        upstream,
    }))
    .cast()
}

/// # Safety
/// `status` は `amonhen_status_load` が返したもので、まだ手放していないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_status_release(status: *const AmonhenStatus) {
    if !status.is_null() {
        drop(unsafe { Arc::from_raw(status.cast::<StatusHandle>()) });
    }
}

/// # Safety
/// `status` は有効であること。返したポインタは手放すまで有効。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_status_view(status: *const AmonhenStatus) -> AmonhenStatusView {
    let s = unsafe { &*status.cast::<StatusHandle>() };
    AmonhenStatusView {
        generation: s.snapshot.generation,
        entries: s.entries.as_ptr(),
        count: s.entries.len() as u32,
        strings: s.strings.as_ptr(),
        branch_off: s.branch.0,
        branch_len: s.branch.1,
        upstream_off: s.upstream.0,
        upstream_len: s.upstream.1,
        ahead: s.snapshot.status.ahead,
        behind: s.snapshot.status.behind,
    }
}

/// `status` の `index` 番目のファイルの差分。`staged` なら HEAD と index、そうでなければ index と作業ツリー
/// （`amonhen_working_diff_free` で解放する）。
///
/// # Safety
/// `repo` と `status` は有効であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_working_diff(
    repo_ptr: *const AmonhenRepo,
    status: *const AmonhenStatus,
    index: u32,
    staged: bool,
    encoding: AmonhenEncoding,
) -> *mut AmonhenWorkingDiff {
    let s = unsafe { &*status.cast::<StatusHandle>() };
    let Some(entry) = s.snapshot.status.entries.get(index as usize) else {
        set_error("ファイルの番号が範囲外");
        return std::ptr::null_mut();
    };
    match repo(repo_ptr).working_diff(entry, staged, &profile(encoding)) {
        Ok(d) => Box::into_raw(Box::new(d)).cast(),
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// `status` の `index` 番目のファイルの、HEAD と作業ツリーの差分。変更の行ごとに、index に入っているかを
/// `DiffRow` の `flags`（`STAGED`）で示す（`amonhen_working_diff_free` で解放する）。index がその形で表せない
/// ファイルでは、`amonhen_last_error` を空にして NULL を返す（FE は `amonhen_working_diff` で 2 つに分けて見せる）。
///
/// # Safety
/// `repo` と `status` は有効であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_working_diff_combined(
    repo_ptr: *const AmonhenRepo,
    status: *const AmonhenStatus,
    index: u32,
    encoding: AmonhenEncoding,
) -> *mut AmonhenWorkingDiff {
    let s = unsafe { &*status.cast::<StatusHandle>() };
    let Some(entry) = s.snapshot.status.entries.get(index as usize) else {
        set_error("ファイルの番号が範囲外");
        return std::ptr::null_mut();
    };
    match repo(repo_ptr).working_diff_combined(entry, &profile(encoding)) {
        Ok(Some(d)) => Box::into_raw(Box::new(d)).cast(),
        Ok(None) => {
            set_error("");
            std::ptr::null_mut()
        }
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// # Safety
/// `diff` は有効であること。返したポインタは解放するまで有効。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_working_diff_view(
    diff: *const AmonhenWorkingDiff,
) -> AmonhenDiffView {
    let d = unsafe { &*diff.cast::<WorkingDiff>() };
    diff_view(&d.view)
}

/// # Safety
/// `diff` は `amonhen_working_diff` が返したもので、まだ解放していないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_working_diff_free(diff: *mut AmonhenWorkingDiff) {
    if !diff.is_null() {
        drop(unsafe { Box::from_raw(diff.cast::<WorkingDiff>()) });
    }
}

fn paths<'a>(paths: *const *const c_char, count: u32) -> Result<Vec<&'a str>, std::str::Utf8Error> {
    (0..count as usize)
        .map(|i| str_arg(unsafe { *paths.add(i) }))
        .collect()
}

fn done(result: amonhen_model::Result<()>) -> bool {
    match result {
        Ok(()) => true,
        Err(e) => {
            set_error(e);
            false
        }
    }
}

/// `paths` の変更（追加・削除を含む）を stage する。
///
/// # Safety
/// `repo` は有効なリポジトリ、`paths` は NUL 終端の文字列 `count` 個の配列であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_stage(
    repo_ptr: *const AmonhenRepo,
    paths_ptr: *const *const c_char,
    count: u32,
) -> bool {
    match paths(paths_ptr, count) {
        Ok(p) => done(repo(repo_ptr).stage(&p)),
        Err(e) => done(Err(e.into())),
    }
}

/// `paths` を unstage する（index を HEAD の状態に戻す）。
///
/// # Safety
/// `repo` は有効なリポジトリ、`paths` は NUL 終端の文字列 `count` 個の配列であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_unstage(
    repo_ptr: *const AmonhenRepo,
    paths_ptr: *const *const c_char,
    count: u32,
) -> bool {
    match paths(paths_ptr, count) {
        Ok(p) => done(repo(repo_ptr).unstage(&p)),
        Err(e) => done(Err(e.into())),
    }
}

/// `diff` の `rows` 行目の変更だけを stage する（stage 済みの差分なら unstage する）。
///
/// # Safety
/// `repo` と `diff` は有効、`rows` は `count` 個の `u32` を読める領域であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_stage_rows(
    repo_ptr: *const AmonhenRepo,
    diff: *const AmonhenWorkingDiff,
    rows: *const u32,
    count: u32,
) -> bool {
    let d = unsafe { &*diff.cast::<WorkingDiff>() };
    let rows: Vec<usize> = (0..count as usize)
        .map(|i| unsafe { *rows.add(i) } as usize)
        .collect();
    done(repo(repo_ptr).stage_rows(d, &rows))
}

/// `amonhen_working_diff_combined` の差分の `rows` 行目の変更を、`staged` なら index に入れ、そうでなければ外す。
///
/// # Safety
/// `repo` と `diff` は有効、`rows` は `count` 個の `u32` を読める領域であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_set_rows_staged(
    repo_ptr: *const AmonhenRepo,
    diff: *const AmonhenWorkingDiff,
    rows: *const u32,
    count: u32,
    staged: bool,
) -> bool {
    let d = unsafe { &*diff.cast::<WorkingDiff>() };
    let rows: Vec<usize> = (0..count as usize)
        .map(|i| unsafe { *rows.add(i) } as usize)
        .collect();
    done(repo(repo_ptr).set_rows_staged(d, &rows, staged))
}

/// stage した変更をコミットする。`amend` なら直前のコミットを作り直す。
///
/// # Safety
/// `repo` は有効なリポジトリ、`message` は NUL 終端の文字列であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_commit(
    repo_ptr: *const AmonhenRepo,
    message: *const c_char,
    amend: bool,
) -> bool {
    match str_arg(message) {
        Ok(m) => done(repo(repo_ptr).commit(m, amend)),
        Err(e) => done(Err(e.into())),
    }
}

/// JSON の命令（`{"op":"merge","rev":"topic"}` など）を実行し、JSON の結果
/// （`{"ok":true,"outcome":"done"}` か `{"ok":false,"error":"…"}`）を返す。結果は
/// `amonhen_string_free` で解放する。頻度の低い操作はすべてここを通す。
///
/// # Safety
/// `repo` は有効なリポジトリ、`command` は NUL 終端の UTF-8 文字列であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_command(
    repo_ptr: *const AmonhenRepo,
    command: *const c_char,
) -> *mut c_char {
    let response = match str_arg(command) {
        Ok(json) => repo(repo_ptr).run_json(json),
        Err(e) => format!(r#"{{"ok":false,"error":"{e}"}}"#),
    };
    CString::new(response).unwrap_or_default().into_raw()
}

/// Activity の記録（git の起動とコマンドの実行、古い順、最大 1000 件）の JSON の配列。`amonhen_string_free` で
/// 解放する。
#[unsafe(no_mangle)]
pub extern "C" fn amonhen_activity() -> *mut c_char {
    CString::new(amonhen_core::activity_json())
        .unwrap_or_default()
        .into_raw()
}

/// 手元で走っている操作（Activity の記録の番号 `seq`）を取り消す。取り消せれば true。
#[unsafe(no_mangle)]
pub extern "C" fn amonhen_activity_cancel(seq: u64) -> bool {
    amonhen_core::cancel_activity(seq)
}

/// `url` を `dest`（まだないフォルダ）に clone する。失敗したら false（理由は `amonhen_last_error`）。時間が
/// かかるので裏のスレッドで呼ぶ。
///
/// # Safety
/// `url` と `dest` は NUL 終端の UTF-8 文字列であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_clone(url: *const c_char, dest: *const c_char) -> bool {
    match (str_arg(url), str_arg(dest)) {
        (Ok(url), Ok(dest)) => done(amonhen_backend_cli::clone(url, std::path::Path::new(dest))),
        (Err(e), _) | (_, Err(e)) => done(Err(e.into())),
    }
}

/// `path` のリポジトリの軽い状態（リポジトリの一覧に出す）。行に分けた文字列で、順にブランチ（detached なら
/// 空）、上流があれば 1、ahead、behind、変更のあるファイルの数（untracked は数えない）。読めなければ NULL。
/// `amonhen_string_free` で解放する。git を 1 回起動するので、裏のスレッドで呼ぶ。
///
/// # Safety
/// `path` は NUL 終端の UTF-8 文字列であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_peek(path: *const c_char) -> *mut c_char {
    let Ok(path) = str_arg(path) else {
        return std::ptr::null_mut();
    };
    match amonhen_backend_cli::Git::new(path).peek() {
        Ok(p) => {
            let text = format!(
                "{}\n{}\n{}\n{}\n{}",
                p.branch.unwrap_or_default(),
                u8::from(p.upstream),
                p.ahead,
                p.behind,
                p.changed
            );
            CString::new(text).unwrap_or_default().into_raw()
        }
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// # Safety
/// `s` は `amonhen_command`・`amonhen_peek` が返したもので、まだ解放していないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_string_free(s: *mut c_char) {
    if !s.is_null() {
        drop(unsafe { CString::from_raw(s) });
    }
}

/// `diff` の `rows` 行目の変更を作業ツリーから取り除く（Undo できる）。stage していない差分にだけ使える。
///
/// # Safety
/// `repo` と `diff` は有効、`rows` は `count` 個の `u32` を読める領域であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_discard_rows(
    repo_ptr: *const AmonhenRepo,
    diff: *const AmonhenWorkingDiff,
    rows: *const u32,
    count: u32,
) -> bool {
    let d = unsafe { &*diff.cast::<WorkingDiff>() };
    let rows: Vec<usize> = (0..count as usize)
        .map(|i| unsafe { *rows.add(i) } as usize)
        .collect();
    done(repo(repo_ptr).discard_rows(d, &rows))
}

/// 作業ツリーのルート（`amonhen_string_free` で解放する）。bare リポジトリなら NULL。
///
/// # Safety
/// `repo` は有効なリポジトリであること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_repo_workdir(repo_ptr: *const AmonhenRepo) -> *mut c_char {
    repo(repo_ptr)
        .paths()
        .workdir
        .and_then(|p| CString::new(p.display().to_string()).ok())
        .map_or(std::ptr::null_mut(), CString::into_raw)
}

/// daemon 越しのリポジトリのコミットの署名を、手元の鍵で検証するか（false なら SSH 先の git と鍵で検証する）。
/// 手元のリポジトリには効かない（いつも手元で検証する）。変えると、覚えていた検証の結果は捨てる。
///
/// # Safety
/// `repo` は有効なリポジトリであること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_repo_set_local_signature_check(
    repo_ptr: *const AmonhenRepo,
    local: bool,
) {
    static VERIFIER: std::sync::OnceLock<Arc<amonhen_backend_cli::LocalVerifier>> =
        std::sync::OnceLock::new();
    let repo = repo(repo_ptr);
    if repo.connection_state().is_none() {
        return;
    }
    let verifier = local.then(|| {
        let v = VERIFIER.get_or_init(|| {
            // コミットのオブジェクトだけを書く作業用のリポジトリ（同じ ID は 1 回だけ書かれる）。
            Arc::new(amonhen_backend_cli::LocalVerifier::new(
                std::env::temp_dir().join("amonhen-verify.git"),
            ))
        });
        v.clone() as Arc<dyn amonhen_model::SignatureVerifier>
    });
    repo.set_signature_verifier(verifier);
}

/// daemon とのつながりの様子。手元のリポジトリなら -1、つながっていれば 0、切れていれば 1 を返す。
/// `received` が NULL でなければ、これまでに受け取ったバイト数を書く。切れた理由は
/// `amonhen_repo_disconnect_reason` で読む。
///
/// # Safety
/// `repo` は有効なリポジトリ、`received` は NULL か書き込める `u64` を指すこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_repo_connection(
    repo_ptr: *const AmonhenRepo,
    received: *mut u64,
) -> i32 {
    let Some(state) = repo(repo_ptr).connection_state() else {
        return -1;
    };
    if !received.is_null() {
        unsafe { *received = state.received };
    }
    i32::from(state.closed.is_some())
}

/// daemon との接続が切れた理由（`amonhen_string_free` で解放する）。切れていなければ NULL。
///
/// # Safety
/// `repo` は有効なリポジトリであること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_repo_disconnect_reason(
    repo_ptr: *const AmonhenRepo,
) -> *mut c_char {
    repo(repo_ptr)
        .connection_state()
        .and_then(|s| s.closed)
        .and_then(|r| CString::new(r.replace('\0', " ")).ok())
        .map_or(std::ptr::null_mut(), CString::into_raw)
}

/// コンフリクトのある作業ツリーのファイルを、差分と同じ形の行にする（`amonhen_diff_free` で解放する）。
/// 行の `kind` は Current・Incoming・Base・マーカーを表し、コンフリクトの行の `old_no` はその番号 + 1。
///
/// # Safety
/// `repo` は有効なリポジトリ、`path` は NUL 終端の UTF-8 文字列であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_conflict_view(
    repo_ptr: *const AmonhenRepo,
    path: *const c_char,
    encoding: AmonhenEncoding,
) -> *mut AmonhenDiff {
    let result = str_arg(path)
        .map_err(Into::into)
        .and_then(|p| repo(repo_ptr).conflict_view(p, &profile(encoding)));
    match result {
        Ok(view) => Box::into_raw(Box::new(view)).cast(),
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// `tips`（20 バイトの ID が `count` 個）から辿れるコミットだけの History（ブランチで絞り込む）。
/// `amonhen_history_release` で手放す。
///
/// # Safety
/// `repo` は有効なリポジトリ、`tips` は `count * 20` バイト読める領域であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_filtered(
    repo_ptr: *const AmonhenRepo,
    tips: *const u8,
    count: u32,
) -> *const AmonhenHistory {
    let tips: Vec<Oid> = (0..count as usize)
        .map(|i| Oid(unsafe { *tips.add(i * 20).cast::<[u8; 20]>() }))
        .collect();
    match repo(repo_ptr).history_filtered(&tips) {
        Ok(s) => into_history(s),
        Err(e) => {
            set_error(e);
            std::ptr::null()
        }
    }
}

/// マージ済みブランチを折りたたんだ History。`all` なら全ブランチ、そうでなければ `tips`（20 バイトの ID が
/// `count` 個）から辿れるものだけ。`expanded`（20 バイトの ID が `expanded_count` 個）の merge は取り込んだ側も
/// 見せる。`amonhen_history_release` で手放す。
///
/// # Safety
/// `repo` は有効なリポジトリ、`tips` は `count * 20` バイト、`expanded` は `expanded_count * 20` バイト読める
/// 領域であること（個数が 0 なら NULL でよい）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_folded(
    repo_ptr: *const AmonhenRepo,
    all: bool,
    tips: *const u8,
    count: u32,
    expanded: *const u8,
    expanded_count: u32,
) -> *const AmonhenHistory {
    let read = |ptr: *const u8, n: u32| -> Vec<Oid> {
        (0..n as usize).map(|i| Oid(unsafe { *ptr.add(i * 20).cast::<[u8; 20]>() })).collect()
    };
    let tips = (!all).then(|| read(tips, count));
    let expanded = read(expanded, expanded_count);
    match repo(repo_ptr).history_folded(tips.as_deref(), &expanded) {
        Ok(s) => into_history(s),
        Err(e) => {
            set_error(e);
            std::ptr::null()
        }
    }
}

/// 折りたたんだ History で、`row` の merge が隠しているコミットの数（隠していなければ 0）。
///
/// # Safety
/// `history` は有効な History であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_hidden(history_ptr: *const AmonhenHistory, row: u32) -> u32 {
    history(history_ptr).folded.get(row as usize).copied().unwrap_or(0)
}

/// スクロールバーに描く密度。`out` に区間ごとの [頻度, 活動量]（0〜1）を `buckets * 2` 個書き、書いた区間の数を
/// 返す（行が区間より少なければ行の数）。
///
/// # Safety
/// `history` は有効な History、`out` は `buckets * 2` 個の float を書ける領域であること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amonhen_history_density(
    history_ptr: *const AmonhenHistory,
    buckets: u32,
    out: *mut f32,
) -> u32 {
    let density = history(history_ptr).density(buckets as usize);
    for (i, [f, a]) in density.iter().enumerate() {
        unsafe {
            *out.add(i * 2) = *f;
            *out.add(i * 2 + 1) = *a;
        }
    }
    density.len() as u32
}
