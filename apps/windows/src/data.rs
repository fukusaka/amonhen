//! リポジトリを開いて History を読み、件名などのチャンクを裏のスレッドで読む。
//!
//! 結果は `mpsc` でメインスレッドに送り、`wake` で起こす（Win32 なら `PostMessage`。設計の `Platform` の
//! 「メインスレッドを起こす」に当たる）。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use amonhen_backend_gix::GixBackend;
use amonhen_core::{HistorySnapshot, Repository};
use amonhen_layout::StrRef;
use amonhen_projection::{ColumnSet, FrontendProfile, HistoryChunk, StringEncoding};

pub const CHUNK_ROWS: u32 = 256;
/// 持っておくチャンクの数（見えている所に近いものから）。
const KEPT_CHUNKS: usize = 24;

/// Windows の FE の形。文字列は ASCII ならそのまま、それ以外は UTF-16（DirectWrite に `PCWSTR` で渡す）。
const PROFILE: FrontendProfile = FrontendProfile {
    string: StringEncoding::Utf16,
    columns: ColumnSet::ALL,
    page_aligned: false,
};

pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// 裏のスレッドから受け取るチャンク。文字列置き場は自分のバッファを持っているので、どのスレッドに渡してもよい。
pub struct Chunk(HistoryChunk);

// SAFETY: `HistoryChunk` のポインタは、自分が持つバッファの先頭を指すだけ。
unsafe impl Send for Chunk {}

impl Chunk {
    pub fn index(&self, row: u32) -> usize {
        (row - self.0.start_row) as usize
    }

    pub fn subject(&self, i: usize) -> StrRef {
        self.0.subjects[i]
    }

    pub fn author(&self, i: usize) -> StrRef {
        self.0.author_names[i]
    }

    pub fn time(&self, i: usize) -> i64 {
        self.0.author_times[i]
    }

    /// `s` を UTF-16 で `out` に足す。ASCII はここで広げ、それ以外はコアが詰めた UTF-16 をそのまま写す。
    pub fn push_utf16(&self, s: StrRef, out: &mut Vec<u16>) {
        let bytes = self.0.bytes(s);
        if s.is_ascii() {
            out.extend(bytes.iter().map(|&b| b as u16));
        } else {
            // SAFETY: ASCII 以外は 0 番の `Vec<u16>` の中を、偶数のバイト位置から指す。
            let units = unsafe {
                std::slice::from_raw_parts(bytes.as_ptr().cast::<u16>(), bytes.len() / 2)
            };
            out.extend_from_slice(units);
        }
    }
}

enum Event {
    History(Arc<Repository>, Arc<HistorySnapshot>),
    Chunk(u64, u32, Chunk),
    Failed(String),
}

pub struct Data {
    pub history: Option<Arc<HistorySnapshot>>,
    pub chunks: HashMap<u32, Rc<Chunk>>,
    repo: Option<Arc<Repository>>,
    loading: HashSet<u32>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    wake: Wake,
}

impl Data {
    /// `path` を裏のスレッドで開き、前回の History をすぐ送ってから最新を読む。
    pub fn open(path: PathBuf, wake: Wake) -> Self {
        let (tx, rx) = channel();
        let (thread_tx, thread_wake) = (tx.clone(), wake.clone());
        std::thread::spawn(move || {
            let send = |e| {
                let _ = thread_tx.send(e);
                thread_wake();
            };
            let repo = match open(&path) {
                Ok(repo) => Arc::new(repo),
                Err(e) => return send(Event::Failed(format!("リポジトリを開けない: {e}"))),
            };
            if let Some(cached) = repo.load_cached_history() {
                send(Event::History(repo.clone(), cached));
            }
            match repo.load_history() {
                Ok(history) => send(Event::History(repo, history)),
                Err(e) => send(Event::Failed(format!("History を読めない: {e}"))),
            }
        });
        Self {
            history: None,
            chunks: HashMap::new(),
            repo: None,
            loading: HashSet::new(),
            tx,
            rx,
            wake,
        }
    }

    /// 届いたものを取り込む。History が替わったか、チャンクが届いたら `true`。
    pub fn poll(&mut self) -> Result<bool, String> {
        let mut changed = false;
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::History(repo, history) => {
                    if self
                        .history
                        .as_ref()
                        .is_some_and(|h| Arc::ptr_eq(h, &history))
                    {
                        continue;
                    }
                    self.repo = Some(repo);
                    self.history = Some(history);
                    self.chunks.clear();
                    self.loading.clear();
                    changed = true;
                }
                Event::Chunk(generation, index, chunk) => {
                    if self
                        .history
                        .as_ref()
                        .is_some_and(|h| h.generation == generation)
                    {
                        self.loading.remove(&index);
                        self.chunks.insert(index, Rc::new(chunk));
                        changed = true;
                    }
                }
                Event::Failed(e) => return Err(e),
            }
        }
        Ok(changed)
    }

    pub fn row_count(&self) -> u32 {
        self.history
            .as_ref()
            .map_or(0, |h| h.layout.nodes().len() as u32)
    }

    /// `rows` を含むチャンクを読み始め、遠いチャンクを捨てる。
    pub fn want(&mut self, rows: std::ops::Range<u32>) {
        let (Some(repo), Some(history)) = (self.repo.clone(), self.history.clone()) else {
            return;
        };
        let first = rows.start / CHUNK_ROWS;
        let last = rows.end.saturating_sub(1) / CHUNK_ROWS;
        for index in first..=last {
            if self.chunks.contains_key(&index) || !self.loading.insert(index) {
                continue;
            }
            let (tx, wake) = (self.tx.clone(), self.wake.clone());
            let (repo, history) = (repo.clone(), history.clone());
            std::thread::spawn(move || {
                match repo.history_chunk(&history, index * CHUNK_ROWS, CHUNK_ROWS, &PROFILE) {
                    Ok(chunk) => {
                        let _ = tx.send(Event::Chunk(history.generation, index, Chunk(chunk)));
                    }
                    Err(e) => {
                        let _ = tx.send(Event::Failed(format!("件名を読めない: {e}")));
                    }
                }
                wake();
            });
        }
        if self.chunks.len() > KEPT_CHUNKS {
            let center = (first + last) / 2;
            let mut indices: Vec<u32> = self.chunks.keys().copied().collect();
            indices.sort_by_key(|&i| i.abs_diff(center));
            for i in &indices[KEPT_CHUNKS..] {
                self.chunks.remove(i);
            }
        }
    }

    /// 見えている行のチャンクがすべて揃っているか。
    pub fn has_rows(&self, rows: std::ops::Range<u32>) -> bool {
        if rows.is_empty() {
            return self.history.is_some();
        }
        let first = rows.start / CHUNK_ROWS;
        let last = (rows.end - 1) / CHUNK_ROWS;
        (first..=last).all(|i| self.chunks.contains_key(&i))
    }
}

fn open(path: &Path) -> Result<Repository, String> {
    let backend = GixBackend::open(path).map_err(|e| e.to_string())?;
    let key = std::fs::canonicalize(path)
        .map_or_else(|_| path.display().to_string(), |p| p.display().to_string());
    Ok(match amonhen_remote::local::cache_dir() {
        Some(dir) => Repository::with_cache(backend, amonhen_core::cache::path_for(&dir, &key)),
        None => Repository::new(backend),
    })
}
