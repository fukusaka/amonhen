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
use amonhen_core::diff::{DiffView, StackedDiff};
use amonhen_core::{HistorySnapshot, RefsSnapshot, Repository, StatusSnapshot};
use amonhen_layout::StrRef;
use amonhen_model::{RefKind, Refs};
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

    /// `s` を UTF-16 で `out` に足す。
    pub fn push_utf16(&self, s: StrRef, out: &mut Vec<u16>) {
        push_utf16(self.0.bytes(s), s.is_ascii(), out);
    }
}

/// コアが `PROFILE` で詰めた文字列を UTF-16 で `out` に足す。ASCII はここで広げ、それ以外はコアが詰めた UTF-16 を
/// そのまま写す。
pub fn push_utf16(bytes: &[u8], ascii: bool, out: &mut Vec<u16>) {
    if ascii {
        out.extend(bytes.iter().map(|&b| b as u16));
    } else if bytes.as_ptr().align_offset(2) == 0 {
        // SAFETY: ASCII 以外はコアの `Vec<u16>` の中を、偶数のバイト位置から指す。
        let units =
            unsafe { std::slice::from_raw_parts(bytes.as_ptr().cast::<u16>(), bytes.len() / 2) };
        out.extend_from_slice(units);
    } else {
        out.extend(
            bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&c| u16::from_ne_bytes(c)),
        );
    }
}

/// 差分の欄に描くもの（コミットの全ファイルの差分か、Working Copy の 1 ファイルの差分）。
#[derive(Clone, Copy)]
pub struct DiffContent<'a> {
    pub view: &'a DiffView,
    pub highlights: &'a HashMap<u32, Vec<(u32, u32)>>,
    pub syntax: &'a HashMap<u32, Vec<(u32, u32, u8)>>,
    /// 行ごとに次のコミットに含めるかの印を出す（Working Copy の 1 本の差分）。
    pub marks: bool,
}

/// 行ごとの、変わった単語の範囲。
fn highlights_of(view: &DiffView) -> HashMap<u32, Vec<(u32, u32)>> {
    let mut out: HashMap<u32, Vec<(u32, u32)>> = HashMap::new();
    for h in &view.highlights {
        out.entry(h.row).or_default().push((h.start, h.end));
    }
    out
}

/// 行ごとの構文の色（ファイル全体を解析するので、裏のスレッドで呼ぶ）。
fn syntax_of(view: &DiffView) -> HashMap<u32, Vec<(u32, u32, u8)>> {
    let mut out: HashMap<u32, Vec<(u32, u32, u8)>> = HashMap::new();
    for s in view.syntax() {
        out.entry(s.row).or_default().push((s.start, s.end, s.kind));
    }
    out
}

/// Working Copy で選んだファイルの差分（HEAD と作業ツリーの 1 本の差分か、片側の差分）。
pub struct WorkingShown {
    pub path: String,
    pub diff: Arc<amonhen_core::WorkingDiff>,
    pub highlights: HashMap<u32, Vec<(u32, u32)>>,
    pub syntax: HashMap<u32, Vec<(u32, u32, u8)>>,
}

impl WorkingShown {
    pub fn content(&self) -> DiffContent<'_> {
        DiffContent {
            view: &self.diff.view,
            highlights: &self.highlights,
            syntax: &self.syntax,
            marks: self.diff.combined.is_some(),
        }
    }
}

/// 選んだコミットの詳細（`commit_info` の答え）と、全ファイルを並べた差分。
pub struct CommitShown {
    pub row: u32,
    pub info: serde_json::Value,
    pub diff: DiffView,
    /// 差分の行ごとの、変わった単語の範囲（UTF-16 の位置）。
    pub highlights: HashMap<u32, Vec<(u32, u32)>>,
    /// 差分の行ごとの構文の色（範囲と種類）。ファイル全体を解析するので、差分より後に届く。
    pub syntax: HashMap<u32, Vec<(u32, u32, u8)>>,
    /// 時間のかかる詳細（`commit_extras` の答え。署名・子・含むブランチとタグなど）。後から届く。
    pub extras: Option<serde_json::Value>,
    /// 全ファイルの差分（畳むファイルを変えたら、ここから表示を作り直す）。
    pub stacked: Arc<StackedDiff>,
    /// ファイルごとに畳んでいるか（コミットの変更ファイルの順）。
    pub collapsed: Vec<bool>,
    /// 表示を作り直した回数（古い表示の構文の色を捨てるため）。
    version: u32,
}

impl CommitShown {
    fn new(row: u32, info: serde_json::Value, stacked: Arc<StackedDiff>) -> Self {
        let collapsed = vec![false; stacked.files.len()];
        let mut shown = Self {
            row,
            info,
            diff: stacked.view(&collapsed, &PROFILE),
            highlights: HashMap::new(),
            syntax: HashMap::new(),
            extras: None,
            stacked,
            collapsed,
            version: 0,
        };
        shown.index_highlights();
        shown
    }

    pub fn content(&self) -> DiffContent<'_> {
        DiffContent {
            view: &self.diff,
            highlights: &self.highlights,
            syntax: &self.syntax,
            marks: false,
        }
    }

    fn index_highlights(&mut self) {
        self.highlights.clear();
        for h in &self.diff.highlights {
            self.highlights
                .entry(h.row)
                .or_default()
                .push((h.start, h.end));
        }
    }
}

/// 構文の色を裏で求めて送る（`version` の表示のもの）。
fn send_syntax(
    tx: Sender<Event>,
    wake: Wake,
    generation: u64,
    row: u32,
    version: u32,
    stacked: Arc<StackedDiff>,
    collapsed: Vec<bool>,
) {
    std::thread::spawn(move || {
        // 同じ行の並びの表示をもう一度作って解析する（ファイルの中身は共有したまま）。
        let mut syntax: HashMap<u32, Vec<(u32, u32, u8)>> = HashMap::new();
        for s in stacked.view(&collapsed, &PROFILE).syntax() {
            syntax
                .entry(s.row)
                .or_default()
                .push((s.start, s.end, s.kind));
        }
        if !syntax.is_empty() {
            let _ = tx.send(Event::Syntax(generation, row, version, syntax));
            wake();
        }
    });
}

/// 届いた検索の答え。
pub enum SearchDone {
    Found(String),
    Failed(String, String),
}

/// History の行に付ける ref の名前（ブランチ・リモートブランチ・タグ）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefLabel {
    pub name: String,
    pub kind: RefKind,
    /// チェックアウト中のブランチ。
    pub head: bool,
}

/// 行ごとのラベル。行の番号は `generation` の History のもの。
pub type Labels = HashMap<u32, Vec<RefLabel>>;

enum Event {
    History(Arc<Repository>, Arc<HistorySnapshot>),
    Refs(u64, Arc<RefsSnapshot>, Labels),
    Commit(u64, Box<CommitShown>),
    Syntax(u64, u32, u32, HashMap<u32, Vec<(u32, u32, u8)>>),
    Extras(u64, u32, serde_json::Value),
    Status(Arc<StatusSnapshot>),
    Working(Box<WorkingShown>),
    /// History の検索の答え（History の世代、問い合わせ、一致した行）。
    Search(u64, String, Result<Vec<u32>, String>),
    Chunk(u64, u32, Chunk),
    Failed(String),
}

pub struct Data {
    pub history: Option<Arc<HistorySnapshot>>,
    pub refs: Option<Arc<RefsSnapshot>>,
    /// Working Copy の変更（index と作業ツリー）。
    pub status: Option<Arc<StatusSnapshot>>,
    /// Working Copy で選んだファイルの差分（届いたもの）と、読んでいるファイル。
    pub working: Option<Box<WorkingShown>>,
    working_wanted: Option<String>,
    /// 差分が届いた（見たら `false` に戻す）。
    pub working_changed: bool,
    /// 今の History の行の、ref のラベル。
    pub labels: Labels,
    /// History の検索で一致した行（昇順）。描くときに塗る。
    pub matches: Vec<u32>,
    /// History で複数選んでいる行（昇順。2 つ以上のときだけ）。描くときに塗る。
    pub multi: Vec<u32>,
    /// 複数選んだときの読み込みの番号（`commit_wanted` の鍵の上位ビットを立てて使う）。
    selection_seq: u32,
    /// 探している問い合わせ。
    searching: Option<String>,
    /// 届いた検索の答え（見たら取る）。
    pub search: Option<SearchDone>,
    /// 選んだコミットの詳細と差分（届いたもの）。
    pub commit: Option<Box<CommitShown>>,
    /// 読んでいるコミットの行（届いたものがこれと違えば捨てる）。
    commit_wanted: Option<u32>,
    /// 出している差分に構文の色が届いた（描き直す。見たら `false` に戻す）。
    pub syntax_changed: bool,
    /// 時間のかかる詳細が届いた（見たら `false` に戻す）。
    pub extras_changed: bool,
    /// History が替わった（行の番号が変わる。見たら `false` に戻す）。
    pub history_changed: bool,
    /// 差分より先に届いた、時間のかかる詳細。
    early_extras: Option<(u32, serde_json::Value)>,
    pub chunks: HashMap<u32, Rc<Chunk>>,
    repo: Option<Arc<Repository>>,
    loading: HashSet<u32>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    wake: Wake,
    changed_tx: Sender<u32>,
}

/// 変化の知らせのビットのうち、History を作り直す頼み（`amonhen_watch::Changes` が使わない上のビット）。
const REBUILD: u32 = 1 << 31;

impl Data {
    /// `path` を裏のスレッドで開き、前回の History をすぐ送ってから最新を読む。
    pub fn open(path: PathBuf, wake: Wake) -> Self {
        let (tx, rx) = channel();
        let (thread_tx, thread_wake) = (tx.clone(), wake.clone());
        // 変化の知らせ（`amonhen_watch::Changes` のビット）。見張りのほか、Refresh などで自分でも送る。
        let (changed_tx, changed_rx) = channel::<u32>();
        let watch_tx = changed_tx.clone();
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
            let history = match repo.load_history() {
                Ok(history) => history,
                Err(e) => return send(Event::Failed(format!("History を読めない: {e}"))),
            };
            send(Event::History(repo.clone(), history.clone()));
            let refs = |history: &HistorySnapshot| match repo.load_refs() {
                Ok(refs) => {
                    // 行の索引（`row_of`）もここで作っておく（150 万コミットで 80 ms ほど）。
                    let labels = labels(history, &refs.refs);
                    Some(Event::Refs(history.generation, refs, labels))
                }
                Err(e) => Some(Event::Failed(format!("ref を読めない: {e}"))),
            };
            if let Some(event) = refs(&history) {
                send(event);
            }
            let status = || match repo.load_status() {
                Ok(status) => Some(Event::Status(status)),
                Err(e) => {
                    eprintln!("status を読めない: {e}");
                    None
                }
            };
            if let Some(event) = status() {
                send(event);
            }

            // ref が変わったら History と ref を、設定が変わったら ref を読み直す（GTK4 版の `watch` と同じ）。
            // 知らせは監視のスレッドから来るので、このスレッドで受けて読む。読み直しの失敗は出さない（macOS 版と
            // 同じく、開いて最初の読み込みだけ）。
            let _watcher = match amonhen_watch::Watcher::new(repo.paths(), move |changes| {
                let _ = watch_tx.send(changes.0);
            }) {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("リポジトリを見張れない: {e}");
                    return;
                }
            };
            let mut history = history;
            while let Ok(mut bits) = changed_rx.recv() {
                // まとめて来た知らせは 1 回の読み直しにする。
                while let Ok(more) = changed_rx.try_recv() {
                    bits |= more;
                }
                let changes = amonhen_watch::Changes(bits);
                if bits & REBUILD != 0 {
                    // History の永続キャッシュを捨てて作り直す（View > Rebuild History）。
                    match repo.rebuild_history() {
                        Ok(h) => {
                            history = h;
                            send(Event::History(repo.clone(), history.clone()));
                        }
                        Err(e) => eprintln!("History を作り直せない: {e}"),
                    }
                } else if changes.contains(amonhen_watch::Changes::REFS) {
                    match repo.load_history() {
                        Ok(h) => {
                            history = h;
                            send(Event::History(repo.clone(), history.clone()));
                        }
                        Err(e) => eprintln!("History を読み直せない: {e}"),
                    }
                }
                if (changes.contains(amonhen_watch::Changes::REFS)
                    || changes.contains(amonhen_watch::Changes::CONFIG))
                    && let Some(event) = refs(&history)
                {
                    match event {
                        Event::Failed(e) => eprintln!("{e}"),
                        event => send(event),
                    }
                }
                // ref・index・作業ツリーが変わったら、Working Copy の status も。
                if (changes.contains(amonhen_watch::Changes::REFS)
                    || changes.contains(amonhen_watch::Changes::INDEX)
                    || changes.contains(amonhen_watch::Changes::WORKTREE))
                    && let Some(event) = status()
                {
                    send(event);
                }
            }
        });
        Self {
            history: None,
            refs: None,
            status: None,
            working: None,
            working_wanted: None,
            working_changed: false,
            labels: HashMap::new(),
            matches: Vec::new(),
            multi: Vec::new(),
            selection_seq: 0,
            searching: None,
            search: None,
            commit: None,
            commit_wanted: None,
            syntax_changed: false,
            extras_changed: false,
            history_changed: false,
            early_extras: None,
            chunks: HashMap::new(),
            repo: None,
            loading: HashSet::new(),
            tx,
            rx,
            wake,
            changed_tx,
        }
    }

    /// History と ref を読み直す（View > Refresh）。
    pub fn reload(&self) {
        let all = amonhen_watch::Changes::REFS.0 | amonhen_watch::Changes::CONFIG.0;
        let _ = self.changed_tx.send(all);
    }

    /// History を `query`（コアの問い合わせの書き方）で探す。答えは `search` に届く。
    pub fn search(&mut self, query: String) {
        let (Some(repo), Some(history)) = (self.repo.clone(), self.history.clone()) else {
            return;
        };
        self.searching = Some(query.clone());
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        std::thread::spawn(move || {
            let rows = repo
                .search_history(&history, &query)
                .map_err(|e| e.to_string());
            let _ = tx.send(Event::Search(history.generation, query, rows));
            wake();
        });
    }

    /// Working Copy のファイルの差分を裏で読む。1 本の差分で表せれば HEAD と作業ツリーの差分、表せなければ変更の
    /// ある側（両方なら作業ツリーの側）。`None` なら出さない。
    pub fn want_working(&mut self, entry: Option<amonhen_model::StatusEntry>) {
        let Some(entry) = entry else {
            self.working_wanted = None;
            self.working = None;
            self.working_changed = true;
            return;
        };
        let Some(repo) = self.repo.clone() else {
            return;
        };
        self.working_wanted = Some(entry.path.clone());
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        std::thread::spawn(move || {
            let diff = match repo.working_diff_combined(&entry, &PROFILE).ok().flatten() {
                Some(diff) => Ok(diff),
                None => repo.working_diff(&entry, !entry.is_unstaged(), &PROFILE),
            };
            let event = match diff {
                Ok(diff) => {
                    let (highlights, syntax) = (highlights_of(&diff.view), syntax_of(&diff.view));
                    Event::Working(Box::new(WorkingShown {
                        path: entry.path.clone(),
                        diff: Arc::new(diff),
                        highlights,
                        syntax,
                    }))
                }
                Err(e) => {
                    eprintln!("{} の差分を読めない: {e}", entry.path);
                    return;
                }
            };
            let _ = tx.send(event);
            wake();
        });
    }

    /// Working Copy の差分の `row` 行目（変更の行）を、次のコミットに含める・外す（今と逆にする）。終わったら status を
    /// 読み直し、差分も読み直される。
    pub fn toggle_working_row(&self, row: usize) {
        let Some(working) = &self.working else { return };
        let Some(staged) = working
            .diff
            .combined
            .as_ref()
            .and_then(|c| c.get(row).copied())
        else {
            return;
        };
        let (Some(repo), diff, changed) = (
            self.repo.clone(),
            working.diff.clone(),
            self.changed_tx.clone(),
        ) else {
            return;
        };
        std::thread::spawn(move || {
            if let Err(e) = repo.set_rows_staged(&diff, &[row], !staged) {
                eprintln!("行を含められない: {e}");
            }
            let _ = changed.send(amonhen_watch::Changes::INDEX.0);
        });
    }

    /// Working Copy の status を読み直す（stage・コミットの後。見張りが拾う前に出す）。
    pub fn reload_status(&self) {
        let _ = self.changed_tx.send(amonhen_watch::Changes::INDEX.0);
    }

    /// History の永続キャッシュを捨てて作り直し、ref も読み直す（View > Rebuild History）。
    pub fn rebuild(&self) {
        let _ = self
            .changed_tx
            .send(REBUILD | amonhen_watch::Changes::CONFIG.0);
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
                    self.history_changed = true;
                    self.labels.clear();
                    // 行の番号が変わるので、検索の一致も捨てる（探し直すのは FE）。
                    self.matches.clear();
                    self.multi.clear();
                    self.commit = None;
                    self.commit_wanted = None;
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
                Event::Refs(generation, refs, labels) => {
                    if self
                        .history
                        .as_ref()
                        .is_some_and(|h| h.generation == generation)
                    {
                        self.labels = labels;
                    }
                    self.refs = Some(refs);
                    changed = true;
                }
                Event::Commit(generation, shown) => {
                    if self
                        .history
                        .as_ref()
                        .is_some_and(|h| h.generation == generation)
                        && self.commit_wanted == Some(shown.row)
                    {
                        let mut shown = shown;
                        if let Some((row, extras)) = self.early_extras.take()
                            && row == shown.row
                        {
                            shown.extras = Some(extras);
                        }
                        self.commit = Some(shown);
                        changed = true;
                    }
                }
                Event::Syntax(generation, row, version, syntax) => {
                    if let Some(commit) = &mut self.commit
                        && commit.row == row
                        && commit.version == version
                        && self
                            .history
                            .as_ref()
                            .is_some_and(|h| h.generation == generation)
                    {
                        commit.syntax = syntax;
                        self.syntax_changed = true;
                        changed = true;
                    }
                }
                Event::Extras(generation, row, extras) => {
                    let current = self
                        .history
                        .as_ref()
                        .is_some_and(|h| h.generation == generation);
                    if current && self.commit_wanted == Some(row) {
                        match &mut self.commit {
                            Some(commit) if commit.row == row => commit.extras = Some(extras),
                            // 差分より先に届いた。差分が届いたときに足す。
                            _ => self.early_extras = Some((row, extras)),
                        }
                        self.extras_changed = true;
                        changed = true;
                    }
                }
                Event::Search(generation, query, rows) => {
                    let current = self
                        .history
                        .as_ref()
                        .is_some_and(|h| h.generation == generation);
                    if current && self.searching.as_deref() == Some(query.as_str()) {
                        self.searching = None;
                        self.search = Some(match rows {
                            Ok(rows) => {
                                self.matches = rows;
                                SearchDone::Found(query)
                            }
                            Err(e) => {
                                self.matches.clear();
                                SearchDone::Failed(query, e)
                            }
                        });
                        changed = true;
                    }
                }
                Event::Working(shown) => {
                    if self.working_wanted.as_deref() == Some(shown.path.as_str()) {
                        self.working = Some(shown);
                        self.working_changed = true;
                        changed = true;
                    }
                }
                Event::Status(status) => {
                    if !self
                        .status
                        .as_ref()
                        .is_some_and(|s| Arc::ptr_eq(s, &status))
                    {
                        self.status = Some(status);
                        changed = true;
                    }
                }
                Event::Failed(e) => return Err(e),
            }
        }
        Ok(changed)
    }

    /// 行 `row` のコミットの詳細と差分を裏で読む（同じ行を読んでいれば何もしない）。
    pub fn want_commit(&mut self, row: u32) {
        if self.commit_wanted == Some(row) {
            return;
        }
        let (Some(repo), Some(history)) = (self.repo.clone(), self.history.clone()) else {
            return;
        };
        let Some(&id) = history.graph.ids().get(row as usize) else {
            return;
        };
        self.commit_wanted = Some(row);
        self.early_extras = None;
        let generation = history.generation;
        // 時間のかかる詳細は別のスレッドで（差分を待たせない）。
        let (extras_repo, extras_tx, extras_wake) =
            (repo.clone(), self.tx.clone(), self.wake.clone());
        std::thread::spawn(move || {
            if let Ok(extras) = extras_repo.commit_extras(&id) {
                let _ = extras_tx.send(Event::Extras(generation, row, extras));
                extras_wake();
            }
        });
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        std::thread::spawn(move || {
            let load = || -> Result<CommitShown, String> {
                let info = repo.commit_info(&id).map_err(|e| e.to_string())?;
                let changes = repo.commit_changes(&id).map_err(|e| e.to_string())?;
                let stacked = repo.stacked_diff(&changes).map_err(|e| e.to_string())?;
                Ok(CommitShown::new(row, info, Arc::new(stacked)))
            };
            let shown = match load() {
                Ok(shown) => shown,
                Err(e) => {
                    let _ = tx.send(Event::Failed(format!("コミットを読めない: {e}")));
                    return wake();
                }
            };
            // 差分を先に出し、構文の色は後から（ファイル全体を解析するので時間がかかることがある）。
            let (stacked, collapsed) = (shown.stacked.clone(), shown.collapsed.clone());
            let _ = tx.send(Event::Commit(generation, Box::new(shown)));
            wake();
            send_syntax(tx, wake, generation, row, 0, stacked, collapsed);
        });
    }

    /// stash `id` の詳細と中身（`stash_changes`: 作業ツリーと index の変更、untracked のファイル）を裏で読む
    /// （Stashes の画面。History の行ではないので、複数選んだときと同じ鍵を使う）。
    pub fn want_stash(&mut self, id: amonhen_model::Oid) {
        self.want_oid(id, true);
    }

    /// History の行ではないコミット `id` の詳細と差分を裏で読む（Reflog。どのブランチにもないコミットもある）。
    pub fn want_commit_id(&mut self, id: amonhen_model::Oid) {
        self.want_oid(id, false);
    }

    /// `want_stash`・`want_commit_id` の中身（`stash` なら stash の中身、でなければコミットの変更）。
    fn want_oid(&mut self, id: amonhen_model::Oid, stash: bool) {
        let (Some(repo), Some(history)) = (self.repo.clone(), self.history.clone()) else {
            return;
        };
        self.selection_seq = self.selection_seq.wrapping_add(1) & 0x7fff_ffff;
        let key = 0x8000_0000 | self.selection_seq;
        self.commit_wanted = Some(key);
        self.early_extras = None;
        let generation = history.generation;
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        std::thread::spawn(move || {
            let load = || -> Result<CommitShown, String> {
                let info = repo.commit_info(&id).map_err(|e| e.to_string())?;
                let changes = if stash {
                    repo.stash_changes(&id)
                } else {
                    repo.commit_changes(&id)
                }
                .map_err(|e| e.to_string())?;
                let stacked = repo.stacked_diff(&changes).map_err(|e| e.to_string())?;
                Ok(CommitShown::new(key, info, Arc::new(stacked)))
            };
            let shown = match load() {
                Ok(shown) => shown,
                Err(e) => {
                    let _ = tx.send(Event::Failed(format!("stash を読めない: {e}")));
                    return wake();
                }
            };
            let (stacked, collapsed) = (shown.stacked.clone(), shown.collapsed.clone());
            let _ = tx.send(Event::Commit(generation, Box::new(shown)));
            wake();
            send_syntax(tx, wake, generation, key, 0, stacked, collapsed);
        });
    }

    /// コミット `id` での、ファイル `path` の差分と詳細を裏で読む（File History。名前を変えたコミットなら変える前の
    /// 名前でも探す）。History の行ではないので、複数選んだときと同じ鍵を使う。
    pub fn want_file_change(&mut self, id: amonhen_model::Oid, path: String) {
        let (Some(repo), Some(history)) = (self.repo.clone(), self.history.clone()) else {
            return;
        };
        self.selection_seq = self.selection_seq.wrapping_add(1) & 0x7fff_ffff;
        let key = 0x8000_0000 | self.selection_seq;
        self.commit_wanted = Some(key);
        self.early_extras = None;
        let generation = history.generation;
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        std::thread::spawn(move || {
            let load = || -> Result<CommitShown, String> {
                let info = repo.commit_info(&id).map_err(|e| e.to_string())?;
                let changes: Vec<_> = repo
                    .commit_changes(&id)
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .filter(|c| c.path == path || c.old_path.as_deref() == Some(path.as_str()))
                    .collect();
                let stacked = repo.stacked_diff(&changes).map_err(|e| e.to_string())?;
                Ok(CommitShown::new(key, info, Arc::new(stacked)))
            };
            let shown = match load() {
                Ok(shown) => shown,
                Err(e) => {
                    let _ = tx.send(Event::Failed(format!("差分を読めない: {e}")));
                    return wake();
                }
            };
            let (stacked, collapsed) = (shown.stacked.clone(), shown.collapsed.clone());
            let _ = tx.send(Event::Commit(generation, Box::new(shown)));
            wake();
            send_syntax(tx, wake, generation, key, 0, stacked, collapsed);
        });
    }

    /// 複数選んだ行（`rows`、昇順で 2 つ以上）の差分と、詳細の欄の見出しを裏で読む（GTK4 版と同じく、コアの
    /// `selection_diff` が範囲・2 つ・とびとびで差分の取り方を決める）。
    pub fn want_selection(&mut self, rows: Vec<u32>) {
        let (Some(repo), Some(history)) = (self.repo.clone(), self.history.clone()) else {
            return;
        };
        self.selection_seq = self.selection_seq.wrapping_add(1) & 0x7fff_ffff;
        let key = 0x8000_0000 | self.selection_seq;
        self.commit_wanted = Some(key);
        self.early_extras = None;
        let generation = history.generation;
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        std::thread::spawn(move || {
            let load = || -> Result<CommitShown, String> {
                let (selection, stacked) = repo
                    .selection_diff(&history, &rows, amonhen_core::PairMode::Forward)
                    .map_err(|e| e.to_string())?;
                let kind = match selection {
                    amonhen_core::Selection::Range { .. } => "range",
                    amonhen_core::Selection::Pair { .. } => "pair",
                    amonhen_core::Selection::Each(_) => "each",
                };
                // 各コミットの 1 行（短い ID・件名・作者。多ければ 20 まで）。
                let ids = history.graph.ids();
                let lines: Vec<serde_json::Value> = rows
                    .iter()
                    .take(20)
                    .filter_map(|&r| ids.get(r as usize))
                    .map(|id| {
                        let info = repo.commit_info(id).unwrap_or_default();
                        let message = info["message"].as_str().unwrap_or_default();
                        let subject = message.lines().next().unwrap_or_default();
                        let hex = id.to_string();
                        serde_json::json!([&hex[..8], subject, info["author"]])
                    })
                    .collect();
                let info = serde_json::json!({ "selection": {
                    "kind": kind, "count": rows.len(), "lines": lines,
                }});
                Ok(CommitShown::new(key, info, Arc::new(stacked)))
            };
            let shown = match load() {
                Ok(shown) => shown,
                Err(e) => {
                    let _ = tx.send(Event::Failed(format!("差分を読めない: {e}")));
                    return wake();
                }
            };
            let (stacked, collapsed) = (shown.stacked.clone(), shown.collapsed.clone());
            let _ = tx.send(Event::Commit(generation, Box::new(shown)));
            wake();
            send_syntax(tx, wake, generation, key, 0, stacked, collapsed);
        });
    }

    /// 差分の `diff_row` 行目（ファイルの見出し）のファイルを畳む・開く。見出しでなければ何もしない。
    pub fn toggle_file(&mut self, diff_row: usize) -> bool {
        let Some(generation) = self.history.as_ref().map(|h| h.generation) else {
            return false;
        };
        let Some(commit) = self.commit.as_mut() else {
            return false;
        };
        let rows = &commit.diff.rows;
        if rows
            .get(diff_row)
            .is_none_or(|r| r.kind != amonhen_layout::DiffRow::FILE)
        {
            return false;
        }
        let file = rows[..diff_row]
            .iter()
            .filter(|r| r.kind == amonhen_layout::DiffRow::FILE)
            .count();
        commit.collapsed[file] = !commit.collapsed[file];
        commit.diff = commit.stacked.view(&commit.collapsed, &PROFILE);
        commit.index_highlights();
        commit.syntax.clear();
        commit.version += 1;
        send_syntax(
            self.tx.clone(),
            self.wake.clone(),
            generation,
            commit.row,
            commit.version,
            commit.stacked.clone(),
            commit.collapsed.clone(),
        );
        true
    }

    pub fn repo(&self) -> Option<Arc<Repository>> {
        self.repo.clone()
    }

    /// コミットの行（今の History の）。
    pub fn row_of(&self, id: &amonhen_model::Oid) -> Option<u32> {
        self.history.as_ref()?.row_of(id)
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

/// ref を行ごとのラベルにする。チェックアウト中のブランチ、ローカルブランチ、リモートブランチ、タグの順。
fn labels(history: &HistorySnapshot, refs: &Refs) -> Labels {
    let mut labels = Labels::new();
    for entry in &refs.entries {
        if !matches!(entry.kind, RefKind::Local | RefKind::Remote | RefKind::Tag) {
            continue;
        }
        let Some(row) = entry.target.as_ref().and_then(|t| history.row_of(t)) else {
            continue;
        };
        let head = entry.kind == RefKind::Local && refs.head_branch.as_deref() == Some(&entry.name);
        labels.entry(row).or_default().push(RefLabel {
            name: entry.name.clone(),
            kind: entry.kind,
            head,
        });
    }
    for row in labels.values_mut() {
        row.sort_by_key(|l| (!l.head, l.kind as u8));
    }
    labels
}
