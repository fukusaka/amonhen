//! C ABI。History・差分・status の行データのように速さが要る経路はここを通す。
//!
//! - 返したポインタが指すメモリは、対応する `*_release` / `*_free` を呼ぶまで変わらない。
//! - スナップショットは不変なので、読むのにロックは要らない。どのスレッドから読んでもよい。
//! - 失敗した関数は NULL か false を返し、理由は `amonhen_last_error` で取れる。

use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char};
use std::sync::Arc;

use amonhen_backend_gix::GixBackend;
use amonhen_core::{HistorySnapshot, RefsSnapshot, Repository, cache};
use amonhen_layout::{Edge, Node, StrRef};
use amonhen_model::Oid;
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
    let profile = FrontendProfile {
        string: match encoding {
            AmonhenEncoding::Utf16 => StringEncoding::Utf16,
            AmonhenEncoding::Utf8 => StringEncoding::Utf8,
        },
        ..FrontendProfile::MACOS
    };
    match repo(repo_ptr).history_chunk(history(history_ptr), start, count, &profile) {
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
    let context = Context(context);
    let watcher = Watcher::new(repo(repo_ptr).paths(), move |changes| {
        let context = &context;
        callback(context.0, changes.0);
    });
    match watcher {
        Ok(w) => Box::into_raw(Box::new(w)).cast(),
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
        drop(unsafe { Box::from_raw(watch.cast::<Watcher>()) });
    }
}
