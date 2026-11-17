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
pub(crate) const PROFILE: FrontendProfile = FrontendProfile {
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

    /// 署名のあるコミット（検証はまだ。History の行の印）。
    pub fn signed(&self, i: usize) -> bool {
        self.0.signatures.get(i).is_some_and(|&k| k != 0)
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

/// 署名の検証の結果（History の行の印の色。GTK4 版の `signature::Status` をまとめたもの）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signature {
    Verified,
    /// 鍵の信用度が決まっていない・署名者がコミッターと違う（黄色）。
    Caution,
    Expired,
    /// 失効した鍵・正しくない署名（赤）。
    Bad,
    /// 鍵がない・検証できない。
    Unknown,
}

impl Signature {
    /// `verifySignatures` の答えの 1 件から決める（GTK4 版の `signature::Status::new` と同じ判定）。
    pub fn of(check: &serde_json::Value) -> Self {
        let status = check["status"].as_str().unwrap_or_default();
        match status {
            "G" | "U" => {
                let signer = check["signer"].as_str().unwrap_or_default();
                let key = check["key"].as_str().unwrap_or_default();
                let ssh = !signer.contains('<') && key.starts_with("SHA256:");
                let level = if ssh {
                    3
                } else {
                    match check["trust"]
                        .as_str()
                        .unwrap_or_default()
                        .to_lowercase()
                        .as_str()
                    {
                        "ultimate" | "full" | "fully" => 3,
                        "marginal" => 2,
                        "never" => 0,
                        _ => 1,
                    }
                };
                if status == "U" || level < 2 || signer_mismatch(check) {
                    Self::Caution
                } else {
                    Self::Verified
                }
            }
            "X" | "Y" => Self::Expired,
            "R" | "B" => Self::Bad,
            _ => Self::Unknown,
        }
    }
}

/// 鍵のメールにコミッターのメールが（有効なものとして）ない（GTK4 版の `CommitterMatch::Mismatched`）。
fn signer_mismatch(check: &serde_json::Value) -> bool {
    let committer = check["committerEmail"]
        .as_str()
        .unwrap_or_default()
        .to_lowercase();
    let emails: Vec<(String, String)> = check["emails"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| {
            (
                e["email"].as_str().unwrap_or_default().to_lowercase(),
                e["validity"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    if emails.is_empty() || committer.is_empty() {
        return false;
    }
    !emails
        .iter()
        .any(|(e, v)| *e == committer && !matches!(v.as_str(), "r" | "e" | "n"))
}

/// 差分の `hunk` 行目（hunk の見出し）に続く、変わった行（次の hunk かファイルの見出しまで）。見出しでなければ空。
pub fn hunk_rows(rows: &[amonhen_layout::DiffRow], hunk: usize) -> Vec<usize> {
    use amonhen_layout::DiffRow;
    if rows.get(hunk).is_none_or(|r| r.kind != DiffRow::HUNK) {
        return Vec::new();
    }
    rows.iter()
        .enumerate()
        .skip(hunk + 1)
        .take_while(|(_, r)| !matches!(r.kind, DiffRow::HUNK | DiffRow::FILE))
        .filter(|(_, r)| matches!(r.kind, DiffRow::ADDED | DiffRow::REMOVED))
        .map(|(i, _)| i)
        .collect()
}

/// History の差分のファイル（見出しの右クリックのメニュー）。
#[derive(Clone, Debug, PartialEq)]
pub struct DiffFile {
    /// コミットの変更ファイルの順の番号。
    pub index: usize,
    pub path: String,
    pub collapsed: bool,
    /// 1 つのコミットの差分ならその ID（複数選んだときは None）。
    pub commit: Option<String>,
    /// そのコミットで消したファイル（Blame できない）。
    pub deleted: bool,
    /// そのコミットで足したファイル（その前の版はない）。
    pub added: bool,
}

/// History の行に付ける ref の名前（ブランチ・リモートブランチ・タグ）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefLabel {
    pub name: String,
    pub kind: RefKind,
    /// チェックアウト中のブランチ（worktree の札なら、書き込む先の worktree）。
    pub head: bool,
    /// worktree の HEAD の札（✎ ⌂ ⑂ と名前と変更の数。GTK4 版・macOS 版と同じ）。
    pub worktree: bool,
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
    /// 一致したコミットだけの History（元の History の世代、作った History、その行の ref の印、元の一致）。
    Filtered(u64, Arc<HistorySnapshot>, Labels, Vec<u32>),
    /// 署名の検証の結果（History の世代）。
    Signatures(u64, Vec<(amonhen_model::Oid, Signature)>),
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
    /// 一致したコミットだけを出している間の、元の History とその一致・ref の印（検索の欄の Show Only Matching
    /// Commits。GTK4 版の `search::apply_filter`）。探すのはいつも元の History。
    base: Option<(Arc<HistorySnapshot>, Vec<u32>, Labels)>,
    /// 一致したコミットだけを出したい（一致が届いたら作る）。
    matching_only: bool,
    /// 署名の検証の結果と、検証している・これから検証するコミット（見えている行のものだけ、裏で）。
    pub signatures: HashMap<amonhen_model::Oid, Signature>,
    verifying: HashSet<amonhen_model::Oid>,
    unverified: Vec<amonhen_model::Oid>,
    /// 今の History の行の、worktree の HEAD の札（ref のラベルの前に出す）と、その元（コミット、札の文字、
    /// 書き込む先か）。
    pub worktree_labels: Labels,
    worktree_heads: Vec<(amonhen_model::Oid, String, bool)>,
    /// 書き込む先・メインでない worktree が checkout しているブランチ（札を緑にする）。
    pub elsewhere: HashSet<String>,
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
    /// History を位相順に並べるか（裏のスレッドと共有）。
    topo: Arc<std::sync::atomic::AtomicBool>,
    /// マージ済みブランチを折りたたむか、取り込んだ側も見せる merge（裏のスレッドと共有）。
    fold: Arc<std::sync::atomic::AtomicBool>,
    expanded: Arc<std::sync::Mutex<Vec<amonhen_model::Oid>>>,
    /// History の範囲（裏のスレッドと共有）。
    scope: Arc<std::sync::Mutex<Scope>>,
    /// 範囲を変えた（作り直した History が届いたら、選んだ行を見せる）。
    pub scope_changed: bool,
}

/// History の範囲（GTK4 版の `historydisplay::Scope`）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Scope {
    /// 全ブランチ。
    #[default]
    All,
    /// 今のブランチ（HEAD から辿れるもの）。
    Current,
    /// サイドバーで選んだ ref（種類と名前）。それぞれから辿れるコミット。
    Refs(Vec<(RefKind, String)>),
}

impl Scope {
    /// 起点のコミット（ref がなくなっていれば、その分は除く）。
    pub fn tips(&self, refs: &Refs) -> Vec<amonhen_model::Oid> {
        match self {
            Scope::All => Vec::new(),
            Scope::Current => refs.head.into_iter().collect(),
            Scope::Refs(list) => list
                .iter()
                .filter_map(|(kind, name)| {
                    refs.entries
                        .iter()
                        .find(|e| e.kind == *kind && &e.name == name)
                        .and_then(|e| e.target)
                })
                .collect(),
        }
    }
}

/// 変化の知らせのビットのうち、History を作り直す頼み（`amonhen_watch::Changes` が使わない上のビット）。
const REBUILD: u32 = 1 << 31;
/// 並び順を変えて History を並べ直す頼み（並び順は `Data::topo`）。
const REORDER: u32 = 1 << 30;

impl Data {
    /// `path` を裏のスレッドで開き、前回の History をすぐ送ってから最新を読む。
    pub fn open(path: PathBuf, wake: Wake) -> Self {
        let (tx, rx) = channel();
        let (thread_tx, thread_wake) = (tx.clone(), wake.clone());
        // 変化の知らせ（`amonhen_watch::Changes` のビット）。見張りのほか、Refresh などで自分でも送る。
        let (changed_tx, changed_rx) = channel::<u32>();
        let watch_tx = changed_tx.clone();
        // History の並び順（View の Date Order・Topological Order。設定は UI のスレッドで読む）。
        let topo = Arc::new(std::sync::atomic::AtomicBool::new(
            crate::prefs::history_order() == amonhen_model::HistoryOrder::Topo,
        ));
        let thread_topo = topo.clone();
        // マージ済みブランチを折りたたむか（View の Fold Merged Branches）と、取り込んだ側も見せる merge。
        let fold = Arc::new(std::sync::atomic::AtomicBool::new(crate::prefs::bool(
            crate::prefs::FOLD_MERGED_BRANCHES,
        )));
        let expanded: Arc<std::sync::Mutex<Vec<amonhen_model::Oid>>> = Arc::default();
        let (thread_fold, thread_expanded) = (fold.clone(), expanded.clone());
        // History の範囲（全ブランチ・今のブランチ・選んだ ref）。
        let scope: Arc<std::sync::Mutex<Scope>> = Arc::default();
        let thread_scope = scope.clone();
        let order_of = move || {
            if thread_topo.load(std::sync::atomic::Ordering::Relaxed) {
                amonhen_model::HistoryOrder::Topo
            } else {
                amonhen_model::HistoryOrder::Date
            }
        };
        std::thread::spawn(move || {
            let send = |e| {
                let _ = thread_tx.send(e);
                thread_wake();
            };
            let repo = match open(&path) {
                Ok(repo) => Arc::new(repo),
                Err(e) => return send(Event::Failed(format!("リポジトリを開けない: {e}"))),
            };
            // 並び順は読む前にコアへ（既定の日付順なら何もしない。GTK4 版と同じ）。
            let order = order_of();
            if order != amonhen_model::HistoryOrder::Date
                && let Err(e) = repo.set_history_order(order)
            {
                eprintln!("History の並び順を変えられない: {e}");
            }
            // 出す History: 折りたたむなら、マージ済みブランチを折りたたんだもの（GTK4 版の `reshape`）。作れない・
            // 空になったときは全体のまま。
            let shape = |full: Arc<HistorySnapshot>| -> Arc<HistorySnapshot> {
                let fold = thread_fold.load(std::sync::atomic::Ordering::Relaxed);
                // 範囲を絞るなら、その起点（ref は今の指す先。ほかのプログラムで動いても付け替わる）。
                let scope = thread_scope.lock().unwrap().clone();
                let tips: Vec<amonhen_model::Oid> = match &scope {
                    Scope::All => Vec::new(),
                    Scope::Current | Scope::Refs(_) => match repo.load_refs() {
                        Ok(refs) => scope.tips(&refs.refs),
                        Err(e) => {
                            eprintln!("ref を読めない: {e}");
                            Vec::new()
                        }
                    },
                };
                if !fold && tips.is_empty() {
                    return full;
                }
                let expanded = thread_expanded.lock().unwrap().clone();
                let some = (!tips.is_empty()).then_some(&tips[..]);
                let shaped = if fold {
                    repo.history_folded(some, &expanded)
                } else {
                    repo.history_filtered(&tips)
                };
                match shaped {
                    Ok(folded) if !(folded.graph.is_empty() && !full.graph.is_empty()) => folded,
                    Ok(_) => full,
                    Err(e) => {
                        eprintln!("History を折りたためない: {e}");
                        full
                    }
                }
            };
            if let Some(cached) = repo.load_cached_history() {
                send(Event::History(repo.clone(), shape(cached)));
            }
            let history = match repo.load_history() {
                Ok(history) => shape(history),
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
                if bits & REORDER != 0 {
                    if let Err(e) = repo.set_history_order(order_of()) {
                        eprintln!("History を並べ直せない: {e}");
                    }
                    // 並べ直した History と、その行の ref の印を読み直す。
                    bits |= amonhen_watch::Changes::REFS.0;
                }
                let changes = amonhen_watch::Changes(bits);
                if bits & REBUILD != 0 {
                    // History の永続キャッシュを捨てて作り直す（View > Rebuild History）。
                    match repo.rebuild_history() {
                        Ok(h) => {
                            history = shape(h);
                            send(Event::History(repo.clone(), history.clone()));
                        }
                        Err(e) => eprintln!("History を作り直せない: {e}"),
                    }
                } else if changes.contains(amonhen_watch::Changes::REFS) {
                    match repo.load_history() {
                        Ok(h) => {
                            history = shape(h);
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
            signatures: HashMap::new(),
            verifying: HashSet::new(),
            unverified: Vec::new(),
            worktree_labels: HashMap::new(),
            worktree_heads: Vec::new(),
            elsewhere: HashSet::new(),
            matches: Vec::new(),
            base: None,
            matching_only: false,
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
            topo,
            fold,
            expanded,
            scope,
            scope_changed: false,
        }
    }

    /// History と ref を読み直す（View > Refresh）。
    pub fn reload(&self) {
        let all = amonhen_watch::Changes::REFS.0 | amonhen_watch::Changes::CONFIG.0;
        let _ = self.changed_tx.send(all);
    }

    /// 出す History を替える（行の番号が変わるので、行ごとのものは捨てる）。
    fn show(&mut self, history: Arc<HistorySnapshot>, labels: Labels, matches: Vec<u32>) {
        self.history = Some(history);
        self.history_changed = true;
        self.labels = labels;
        self.place_worktrees();
        // 署名の結果はコミットのものなので残すが、検証中のものは新しい世代で頼み直す。
        self.verifying.clear();
        self.matches = matches;
        self.multi.clear();
        self.commit = None;
        self.commit_wanted = None;
        self.chunks.clear();
        self.loading.clear();
    }

    /// 探す History（一致したコミットだけを出している間も、元の History）。
    fn searched(&self) -> Option<&Arc<HistorySnapshot>> {
        self.base
            .as_ref()
            .map(|(h, _, _)| h)
            .or(self.history.as_ref())
    }

    /// 一致したコミットだけを出しているか。
    pub fn is_filtered(&self) -> bool {
        self.base.is_some()
    }

    /// 一致したコミットだけを出すか（一致がなければ元の History のまま。GTK4 版と同じ）。
    pub fn set_matching_only(&mut self, on: bool) {
        self.matching_only = on;
        if !on {
            if let Some((history, matches, labels)) = self.base.take() {
                self.show(history, labels, matches);
            }
            return;
        }
        if self.base.is_none() {
            let matches = self.matches.clone();
            self.filter(matches);
        }
    }

    /// 一致したコミットだけの History を裏で作る（linux の 25 万件で 50 ms ほど。GTK4 版と同じく裏で）。
    fn filter(&mut self, matches: Vec<u32>) {
        let (Some(repo), Some(base)) = (self.repo.clone(), self.searched().cloned()) else {
            return;
        };
        if matches.is_empty() {
            return;
        }
        let refs = self.refs.clone();
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        std::thread::spawn(move || {
            let filtered = repo.history_matching(&base, &matches);
            let labels = refs.map_or_else(Labels::new, |r| labels(&filtered, &r.refs));
            let _ = tx.send(Event::Filtered(base.generation, filtered, labels, matches));
            wake();
        });
    }

    /// History を `query`（コアの問い合わせの書き方）で探す。答えは `search` に届く。
    pub fn search(&mut self, query: String) {
        let (Some(repo), Some(history)) = (self.repo.clone(), self.searched().cloned()) else {
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
    /// `prefer_staged`: 両側に変更があって 1 本の差分で表せないとき、stage 済みの側を読むか。
    pub fn want_working(&mut self, entry: Option<amonhen_model::StatusEntry>, prefer_staged: bool) {
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
                // 1 本の差分で表せなければ、選んだ側（片側にしか変更がなければその側。GTK4 版と同じ）。
                None => {
                    let staged = if entry.is_staged() && entry.is_unstaged() {
                        prefer_staged
                    } else {
                        !entry.is_unstaged()
                    };
                    repo.working_diff(&entry, staged, &PROFILE)
                }
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

    /// Working Copy の差分の hunk の見出し（`row`）の行をまとめて含める・外す（GTK4 版の hunk の印）。1 行でも
    /// 含めていなければすべて含め、すべて含めていればすべて外す。
    pub fn toggle_working_hunk(&self, row: usize) {
        let Some(working) = &self.working else { return };
        let Some(combined) = working.diff.combined.as_ref() else {
            return;
        };
        let rows = hunk_rows(&working.diff.view.rows, row);
        if rows.is_empty() {
            return;
        }
        let stage = rows
            .iter()
            .any(|&r| !combined.get(r).copied().unwrap_or(false));
        let (Some(repo), diff, changed) = (
            self.repo.clone(),
            working.diff.clone(),
            self.changed_tx.clone(),
        ) else {
            return;
        };
        std::thread::spawn(move || {
            if let Err(e) = repo.set_rows_staged(&diff, &rows, stage) {
                eprintln!("hunk を含められない: {e}");
            }
            let _ = changed.send(amonhen_watch::Changes::INDEX.0);
        });
    }

    /// Working Copy の差分で右クリックした `row` の行（変わった行ならその行、hunk の見出しならその hunk の行）と、
    /// そのうちに含めている行があるか（含めている行は捨てられない）。stage 済みの側の差分では None。
    pub fn working_rows_at(&self, row: usize) -> Option<(Vec<usize>, bool)> {
        use amonhen_layout::DiffRow;
        let working = self.working.as_ref()?;
        if working.diff.staged {
            return None;
        }
        let rows = match working.diff.view.rows.get(row)?.kind {
            DiffRow::ADDED | DiffRow::REMOVED => vec![row],
            DiffRow::HUNK => hunk_rows(&working.diff.view.rows, row),
            _ => return None,
        };
        let staged = working
            .diff
            .combined
            .as_ref()
            .is_some_and(|c| rows.iter().any(|&r| c.get(r).copied().unwrap_or(false)));
        (!rows.is_empty()).then_some((rows, staged))
    }

    /// Working Copy の今の差分（行を捨てるのに使う）。
    pub fn working_diff(&self) -> Option<(Arc<Repository>, Arc<amonhen_core::WorkingDiff>)> {
        Some((self.repo.clone()?, self.working.as_ref()?.diff.clone()))
    }

    /// Working Copy の status を読み直す（stage・コミットの後。見張りが拾う前に出す）。
    pub fn reload_status(&self) {
        let _ = self.changed_tx.send(amonhen_watch::Changes::INDEX.0);
    }

    /// History の並び順を変えて並べ直す（View の Date Order・Topological Order）。
    pub fn reorder(&self, order: amonhen_model::HistoryOrder) {
        self.topo.store(
            order == amonhen_model::HistoryOrder::Topo,
            std::sync::atomic::Ordering::Relaxed,
        );
        let _ = self.changed_tx.send(REORDER);
    }

    /// History の範囲。
    pub fn scope(&self) -> Scope {
        self.scope.lock().unwrap().clone()
    }

    /// History の範囲を変えて作り直す（GTK4 版の `set_scope`）。
    pub fn set_scope(&mut self, scope: Scope) {
        {
            let mut current = self.scope.lock().unwrap();
            if *current == scope {
                return;
            }
            *current = scope;
        }
        self.scope_changed = true;
        let _ = self.changed_tx.send(REORDER);
    }

    /// マージ済みブランチを折りたたむかを変えて、出し直す（View の Fold Merged Branches）。
    pub fn set_fold(&self, value: bool) {
        self.fold.store(value, std::sync::atomic::Ordering::Relaxed);
        let _ = self.changed_tx.send(REORDER);
    }

    /// 折りたたんだ History での、コミット `id` の merge の隠しているコミットの数（取り込んだ側を見せていれば 0。
    /// 右クリックの Show n Merged Commits・Fold Merged Commits。どちらでもなければ None）。
    pub fn merge_fold(&self, id: &str) -> Option<u32> {
        if !self.fold.load(std::sync::atomic::Ordering::Relaxed) {
            return None;
        }
        let oid = amonhen_model::Oid::from_hex(id)?;
        let history = self.history.as_ref()?;
        let row = history.row_of(&oid)?;
        let hidden = history.folded.get(row as usize).copied().unwrap_or(0);
        let open = self.expanded.lock().unwrap().contains(&oid);
        (hidden > 0 || open).then_some(hidden)
    }

    /// 折りたたんだ merge の取り込んだ側を見せる・たたむ。
    pub fn toggle_merge(&self, id: &str) {
        let Some(oid) = amonhen_model::Oid::from_hex(id) else {
            return;
        };
        {
            let mut expanded = self.expanded.lock().unwrap();
            match expanded.iter().position(|e| *e == oid) {
                Some(i) => {
                    expanded.remove(i);
                }
                None => expanded.push(oid),
            }
        }
        let _ = self.changed_tx.send(REORDER);
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
                    // 新しい History では、一致したコミットだけの History も作り直す（探し直すのは FE）。
                    self.base = None;
                    self.show(history, Labels::new(), Vec::new());
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
                Event::Signatures(generation, found) => {
                    if self
                        .history
                        .as_ref()
                        .is_some_and(|h| h.generation == generation)
                    {
                        for (id, signature) in found {
                            self.verifying.remove(&id);
                            self.signatures.insert(id, signature);
                        }
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
                    } else if let Some((h, _, base_labels)) = &mut self.base
                        && h.generation == generation
                    {
                        *base_labels = labels;
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
                    let current = self.searched().is_some_and(|h| h.generation == generation);
                    if current && self.searching.as_deref() == Some(query.as_str()) {
                        self.searching = None;
                        // 一致したコミットだけを出している間に探し直したら、元の History に戻してから作り直す。
                        if let Some((history, _, labels)) = self.base.take() {
                            self.show(history, labels, Vec::new());
                        }
                        self.search = Some(match rows {
                            Ok(rows) => {
                                self.matches = rows;
                                if self.matching_only {
                                    self.filter(self.matches.clone());
                                }
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
                Event::Filtered(generation, filtered, labels, matches) => {
                    let current = self
                        .history
                        .as_ref()
                        .is_some_and(|h| h.generation == generation);
                    if current && self.matching_only && self.base.is_none() {
                        let base = self.history.clone().expect("History");
                        let base_labels = std::mem::take(&mut self.labels);
                        self.base = Some((base, matches, base_labels));
                        // すべて一致なので塗らない（次・前は行を 1 つずつ。GTK4 版と同じ）。
                        let rows = (0..filtered.graph.len() as u32).collect();
                        self.show(filtered, labels, rows);
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

    /// 行のコミットの署名の印（署名がなければ None、まだ検証していなければ Some(None) で、裏で検証を頼む）。
    pub fn signature_mark(&mut self, id: amonhen_model::Oid) -> Option<Signature> {
        let known = self.signatures.get(&id).copied();
        if known.is_none() && !self.verifying.contains(&id) && !self.unverified.contains(&id) {
            self.unverified.push(id);
        }
        known
    }

    /// 頼まれた署名を裏でまとめて検証する（描いた後に呼ぶ。GTK4 版は 8 個ずつ）。
    pub fn verify_signatures(&mut self) {
        if self.unverified.is_empty() {
            return;
        }
        let (Some(repo), Some(history)) = (self.repo.clone(), self.history.as_ref()) else {
            return;
        };
        let generation = history.generation;
        let pending = std::mem::take(&mut self.unverified);
        self.verifying.extend(pending.iter().copied());
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        std::thread::spawn(move || {
            for batch in pending.chunks(8) {
                let oids: Vec<String> = batch.iter().map(ToString::to_string).collect();
                let command = serde_json::json!({ "op": "verifySignatures", "oids": oids });
                let reply: serde_json::Value =
                    serde_json::from_str(&repo.run_json(&command.to_string())).unwrap_or_default();
                let checks = reply["info"]["checks"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                let found = batch
                    .iter()
                    .zip(checks.iter())
                    .map(|(id, c)| (*id, Signature::of(c)))
                    .collect();
                let _ = tx.send(Event::Signatures(generation, found));
                wake();
            }
        });
    }

    /// History の差分の `diff_row` 行目が入っているファイル（右クリックのメニュー）。
    pub fn file_at(&self, diff_row: usize) -> Option<DiffFile> {
        let commit = self.commit.as_ref()?;
        let rows = commit.diff.rows.get(..=diff_row)?;
        let index = rows
            .iter()
            .filter(|r| r.kind == amonhen_layout::DiffRow::FILE)
            .count()
            .checked_sub(1)?;
        let path = commit.stacked.files.get(index)?.0.clone();
        // 足した・消したファイルか（GTK4 版の `deleted` の手元の決め方と同じく、前後の中身が空か）。
        let diff = &commit.stacked.files.get(index)?.1;
        let deleted = diff.new.is_empty() && !diff.old.is_empty();
        let added = diff.old.is_empty() && !diff.new.is_empty();
        Some(DiffFile {
            index,
            collapsed: commit.collapsed.get(index).copied().unwrap_or(false),
            commit: commit.info["oid"].as_str().map(str::to_owned),
            deleted,
            added,
            path,
        })
    }

    /// 差分の `diff_row` 行目（ファイルの見出し）のファイルを畳む・開く。見出しでなければ何もしない。
    pub fn toggle_file(&mut self, diff_row: usize) -> bool {
        let Some(commit) = self.commit.as_ref() else {
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
        self.toggle_file_index(file)
    }

    /// 差分の `file` 番目のファイルを畳む・開く。
    pub fn toggle_file_index(&mut self, file: usize) -> bool {
        let Some(generation) = self.history.as_ref().map(|h| h.generation) else {
            return false;
        };
        let Some(commit) = self.commit.as_mut() else {
            return false;
        };
        let Some(collapsed) = commit.collapsed.get_mut(file) else {
            return false;
        };
        *collapsed = !*collapsed;
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

    /// 各 worktree の HEAD の札と、ほかの worktree が checkout しているブランチ（GTK4 版の
    /// `HistoryView::set_worktrees`）。worktree が 1 つなら空で呼ぶ。
    pub fn set_worktrees(
        &mut self,
        heads: Vec<(amonhen_model::Oid, String, bool)>,
        elsewhere: HashSet<String>,
    ) {
        self.worktree_heads = heads;
        self.elsewhere = elsewhere;
        self.place_worktrees();
    }

    /// worktree の札を今の History の行に置く。
    fn place_worktrees(&mut self) {
        let mut labels = Labels::new();
        for (head, name, editing) in &self.worktree_heads {
            if let Some(row) = self.row_of(head) {
                labels.entry(row).or_default().push(RefLabel {
                    name: name.clone(),
                    kind: RefKind::Local,
                    head: *editing,
                    worktree: true,
                });
            }
        }
        self.worktree_labels = labels;
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
            worktree: false,
        });
    }
    for row in labels.values_mut() {
        row.sort_by_key(|l| (!l.head, l.kind as u8));
    }
    labels
}
