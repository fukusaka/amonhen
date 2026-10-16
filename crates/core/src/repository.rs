use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use amonhen_layout::DiffRow;
use amonhen_model::{
    CommitGraph, FileChange, GitBackend, Oid, OidMap, Refs, RepoPaths, Result, Status, StatusEntry,
    topo,
};
use amonhen_projection::{FrontendProfile, HistoryChunk};
use arc_swap::ArcSwapOption;

use crate::cache::{self, CachedHistory};
use crate::diff::{self, DiffView, FileDiff};
use crate::graph::{self, GraphLayout};
use crate::undo::UndoLog;

/// History の不変のスナップショット。公開したら書き換えない。
pub struct HistorySnapshot {
    pub generation: u64,
    /// 起点と HEAD。どちらも変わっていなければ History も変わらない（コミットは不変）。
    pub tips: Vec<Oid>,
    pub head: Option<Oid>,
    pub graph: CommitGraph,
    pub layout: GraphLayout,
    /// マージ済みブランチを折りたたんだ History なら、行ごとの隠したコミットの数（そうでなければ空）。
    pub folded: Vec<u32>,
    /// コミット ID から行への索引。次に読み直すとき、既知のコミットで走査を止めるのに使う。
    rows: OnceLock<OidMap<u32>>,
}

/// 作業ツリーと index の状態の不変のスナップショット。
pub struct StatusSnapshot {
    pub generation: u64,
    pub status: Status,
}

/// Working Copy の 1 ファイルの差分。行を選んで stage・unstage するために、元の差分も持つ。
pub struct WorkingDiff {
    pub entry: StatusEntry,
    /// stage 済みの変更（HEAD と index の差分）か、stage していない変更（index と作業ツリー）か。
    /// `combined` なら false。
    pub staged: bool,
    /// HEAD と作業ツリーの差分で、行ごとに index に入っているか（`working_diff_combined`）。
    pub combined: Option<Vec<bool>>,
    pub file: FileDiff,
    pub view: DiffView,
}

/// 2 つ選んだときの差分の取り方。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PairMode {
    /// 古いほうから新しいほうまで（A..B）。
    #[default]
    Forward,
    /// 新しいほうから古いほうまで（向きを入れ替える）。
    Reverse,
    /// 共通の祖先から新しいほうまで（A...B。新しいほうの側で変わったことだけ）。
    MergeBase,
}

/// `a` と `b`（History の行）の共通の祖先のうち、一番新しいもの（行の小さいもの）。行はトポロジカル順で、親は
/// 子より後ろにあるので、それぞれの祖先を前から順に印を付ければよい。
pub fn merge_base(graph: &CommitGraph, a: u32, b: u32) -> Option<u32> {
    let n = graph.len();
    let ancestors = |start: u32| {
        let mut mark = vec![false; n];
        mark[start as usize] = true;
        for r in start as usize..n {
            if mark[r] {
                for &p in graph.parents(r) {
                    mark[p as usize] = true;
                }
            }
        }
        mark
    };
    let (ma, mb) = (ancestors(a), ancestors(b));
    (a.min(b) as usize..n)
        .find(|&r| ma[r] && mb[r])
        .map(|r| r as u32)
}

/// History で選んだ複数のコミットの差分の見せ方（`Repository::selection_diff`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    /// first-parent でつながる `count` 個のコミット。`base`（一番古いものの最初の親。なければ None）から `tip` まで。
    Range {
        base: Option<Oid>,
        tip: Oid,
        count: usize,
    },
    /// つながっていない 2 つ。`from`（古いほう）から `to` まで。
    Pair { from: Oid, to: Oid },
    /// とびとびの 3 つ以上（古い順）。
    Each(Vec<Oid>),
}

impl Selection {
    /// `rows`（History の行。0 が一番新しい）の選び方。
    pub fn of(graph: &CommitGraph, rows: &[u32]) -> Result<Self> {
        let mut rows = rows.to_vec();
        rows.sort_unstable();
        rows.dedup();
        let (Some(&newest), Some(&oldest)) = (rows.first(), rows.last()) else {
            return Err("コミットを選んでいない".into());
        };
        if rows.iter().any(|&r| r as usize >= graph.len()) {
            return Err("行が範囲外".into());
        }
        let id = |r: u32| graph.ids()[r as usize];
        let first_parent = |r: u32| graph.parents(r as usize).first().copied();
        if rows.windows(2).all(|w| first_parent(w[0]) == Some(w[1])) {
            return Ok(Selection::Range {
                base: first_parent(oldest).map(id),
                tip: id(newest),
                count: rows.len(),
            });
        }
        if rows.len() == 2 {
            return Ok(Selection::Pair {
                from: id(oldest),
                to: id(newest),
            });
        }
        Ok(Selection::Each(rows.iter().rev().map(|&r| id(r)).collect()))
    }
}

/// サイドバーの ref の一覧の不変のスナップショット。
pub struct RefsSnapshot {
    pub generation: u64,
    pub refs: Refs,
}

impl HistorySnapshot {
    /// スクロールバーに描く密度。行を `buckets` 個の区間に分け、区間ごとに [コミットの頻度, ブランチの活動量]
    /// をそれぞれ 0〜1 で返す。頻度は時間あたりのコミット数（対数で縮める）、活動量は区間の中で点のあるレーンの数。
    /// どちらも、いちばん少ない区間を 0、多い区間を 1 にする。
    pub fn density(&self, buckets: usize) -> Vec<[f32; 2]> {
        let n = self.graph.len();
        if n == 0 || buckets == 0 {
            return Vec::new();
        }
        let buckets = buckets.min(n);
        let times = self.graph.commit_times();
        let nodes = self.layout.nodes();
        let mut lanes = Vec::new();
        let mut raw: Vec<(f64, f64)> = (0..buckets)
            .map(|b| {
                let (start, end) = (b * n / buckets, (b + 1) * n / buckets);
                let (lo, hi) = times[start..end].iter().fold((i64::MAX, i64::MIN), |(lo, hi), &t| (lo.min(t), hi.max(t)));
                // 1 時間より短い区間は 1 時間とみなす（同じ時刻のコミットで無限大にしない）。
                let hours = ((hi - lo).max(3600) as f64) / 3600.0;
                let frequency = ((end - start) as f64 / hours).ln_1p();
                lanes.clear();
                lanes.extend(nodes.get(start..end).unwrap_or(&[]).iter().map(|node| node.lane));
                lanes.sort_unstable();
                lanes.dedup();
                (frequency, lanes.len() as f64)
            })
            .collect();
        // いちばん少ない区間を 0、多い区間を 1 に広げる（差を見やすくする）。
        let range = |values: &mut dyn Iterator<Item = f64>| {
            values.fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)))
        };
        let (lo_f, hi_f) = range(&mut raw.iter().map(|r| r.0));
        let (lo_a, hi_a) = range(&mut raw.iter().map(|r| r.1));
        let scale = |v: f64, lo: f64, hi: f64| if hi > lo { ((v - lo) / (hi - lo)) as f32 } else { 0.5 };
        raw.drain(..).map(|(f, a)| [scale(f, lo_f, hi_f), scale(a, lo_a, hi_a)]).collect()
    }

    /// コミットの行。初回は索引を作る（150 万コミットで 80 ms ほど）。
    pub fn row_of(&self, id: &Oid) -> Option<u32> {
        self.rows().get(id).copied()
    }

    fn rows(&self) -> &OidMap<u32> {
        self.rows.get_or_init(|| {
            self.graph
                .ids()
                .iter()
                .enumerate()
                .map(|(r, id)| (*id, r as u32))
                .collect()
        })
    }
}

/// 1 つのリポジトリの状態。読み取りはロックなしで、最新のスナップショットを取るだけ。
pub struct Repository {
    backend: Box<dyn GitBackend>,
    history: ArcSwapOption<HistorySnapshot>,
    refs: ArcSwapOption<RefsSnapshot>,
    status: ArcSwapOption<StatusSnapshot>,
    generation: AtomicU64,
    /// 検索のたびに進める。進んだら、走っている検索はやめる。
    search_generation: AtomicU64,
    cache_path: Option<PathBuf>,
    undo: OnceLock<UndoLog>,
    /// 署名の検証の結果（`%G?`）。コミットは変わらないので、一度検証したら覚えておく。検証する側を変えたら
    /// 捨てる（番号を進め、変える前に始めた検証の結果は覚えない）。
    signatures: std::sync::Mutex<(
        u64,
        std::collections::HashMap<Oid, amonhen_model::SignatureCheck>,
    )>,
    /// 手元で検証するもの。あれば、daemon 越しのリポジトリでもコミットのオブジェクトを受け取って手元で検証する。
    verifier: std::sync::Mutex<Option<Arc<dyn amonhen_model::SignatureVerifier>>>,
    /// History の検索で探す、件名と作者名の索引（初めて探すときに作る）。
    texts: amonhen_model::search::TextIndexCache,
}

impl Repository {
    pub fn new(backend: impl GitBackend + 'static) -> Self {
        Self {
            backend: Box::new(backend),
            history: ArcSwapOption::empty(),
            refs: ArcSwapOption::empty(),
            status: ArcSwapOption::empty(),
            generation: AtomicU64::new(0),
            search_generation: AtomicU64::new(0),
            cache_path: None,
            undo: OnceLock::new(),
            signatures: Default::default(),
            verifier: Default::default(),
            texts: Default::default(),
        }
    }

    /// 署名を手元で検証するもの（`None` ならリポジトリのある側の git で検証する）。変えると、覚えていた
    /// 検証の結果を捨てる（検証する側によって鍵や信用度が違うので、結果を引き継がない）。
    pub fn set_signature_verifier(
        &self,
        verifier: Option<Arc<dyn amonhen_model::SignatureVerifier>>,
    ) {
        let mut signatures = self.signatures.lock().unwrap();
        *self.verifier.lock().unwrap() = verifier;
        signatures.0 += 1;
        signatures.1.clear();
    }

    /// 今の検証する側の番号と、手元で検証するもの。
    fn signature_checker(&self) -> (u64, Option<Arc<dyn amonhen_model::SignatureVerifier>>) {
        let signatures = self.signatures.lock().unwrap();
        (signatures.0, self.verifier.lock().unwrap().clone())
    }

    /// 今の検証する側の番号（検証を始める前に読み、`remember_signatures` に渡す）。
    pub(crate) fn signature_generation(&self) -> u64 {
        self.signatures.lock().unwrap().0
    }

    /// 1 つのコミットの検証の詳細を手元で求める。手元で検証するものがなければ `None`。
    pub(crate) fn local_signature_details(
        &self,
        commit: &Oid,
    ) -> Option<Result<(u64, Option<amonhen_model::SignatureCheck>)>> {
        let (generation, verifier) = self.signature_checker();
        let verifier = verifier?;
        Some((|| {
            let text = self
                .backend
                .commit_texts(std::slice::from_ref(commit))?
                .remove(0);
            Ok((generation, verifier.details(commit, &text.raw)?))
        })())
    }

    /// `ids` の署名の検証の結果（`%G?` の 1 文字）を同じ順に返す。覚えていないものだけを検証する。
    pub fn verify_signatures(&self, ids: &[Oid]) -> Result<Vec<amonhen_model::SignatureCheck>> {
        let unknown: Vec<Oid> = {
            let known = self.signatures.lock().unwrap();
            let mut seen = std::collections::HashSet::new();
            ids.iter()
                .filter(|id| !known.1.contains_key(id) && seen.insert(**id))
                .copied()
                .collect()
        };
        let mut fresh = std::collections::HashMap::new();
        if !unknown.is_empty() {
            let (generation, verifier) = self.signature_checker();
            let found = match verifier {
                // コミットのオブジェクトを受け取り（daemon 越しなら数 KB ずつ）、手元の鍵で検証する。
                Some(verifier) => {
                    let texts = self.backend.commit_texts(&unknown)?;
                    let commits: Vec<(Oid, &[u8])> = unknown
                        .iter()
                        .zip(&texts)
                        .map(|(id, t)| (*id, &t.raw[..]))
                        .collect();
                    verifier.verify(&commits)?
                }
                None => self.backend.verify_signatures(&unknown)?,
            };
            fresh.extend(unknown.iter().copied().zip(found));
            self.remember_signatures(generation, fresh.clone());
        }
        let known = self.signatures.lock().unwrap();
        Ok(ids
            .iter()
            .map(|id| {
                fresh
                    .get(id)
                    .or_else(|| known.1.get(id))
                    .cloned()
                    .unwrap_or(amonhen_model::SignatureCheck {
                        status: 'E',
                        ..Default::default()
                    })
            })
            .collect())
    }

    /// 検証の結果を覚える（コミットの詳細で検証したときにも呼ぶ）。`generation` は検証を始めたときの番号で、
    /// その後に検証する側が変わっていたら覚えない。
    pub(crate) fn remember_signatures(
        &self,
        generation: u64,
        results: impl IntoIterator<Item = (Oid, amonhen_model::SignatureCheck)>,
    ) {
        let mut signatures = self.signatures.lock().unwrap();
        if signatures.0 == generation {
            signatures.1.extend(results);
        }
    }

    /// リポジトリの変化を知らせてもらう（daemon 越しなら true。手元なら false で、呼び出し側が監視する）。
    pub fn subscribe_changes(&self, f: Box<dyn Fn(u32) + Send + Sync>) -> bool {
        self.backend.subscribe_changes(f)
    }

    /// daemon とのつながりの様子。手元なら `None`。
    pub fn connection_state(&self) -> Option<amonhen_model::ConnectionState> {
        self.backend.connection_state()
    }

    pub(crate) fn backend(&self) -> &dyn GitBackend {
        &*self.backend
    }

    pub(crate) fn undo_log(&self) -> Result<&UndoLog> {
        Ok(self.undo.get_or_init(|| UndoLog::new(self.backend.files())))
    }

    /// History を `cache_path` に永続化する。
    pub fn with_cache(backend: impl GitBackend + 'static, cache_path: PathBuf) -> Self {
        Self {
            cache_path: Some(cache_path),
            ..Self::new(backend)
        }
    }

    fn publish(
        &self,
        tips: Vec<Oid>,
        head: Option<Oid>,
        graph: CommitGraph,
        layout: GraphLayout,
    ) -> Arc<HistorySnapshot> {
        let snapshot = Arc::new(HistorySnapshot {
            generation: self.generation.fetch_add(1, Ordering::Relaxed) + 1,
            tips,
            head,
            graph,
            layout,
            folded: Vec::new(),
            rows: OnceLock::new(),
        });
        self.history.store(Some(snapshot.clone()));
        snapshot
    }

    /// 永続キャッシュから前回の History を公開する。まだ何も公開していないときだけ。
    /// mmap するだけなので速い。この後 `load_history` で最新にする。
    pub fn load_cached_history(&self) -> Option<Arc<HistorySnapshot>> {
        if let Some(current) = self.history.load_full() {
            return Some(current);
        }
        let cached = cache::read(self.cache_path.as_deref()?)?;
        Some(self.publish(cached.tips, cached.head, cached.graph, cached.layout))
    }

    /// History を読み直して公開する。起点と HEAD が前回と同じなら何もしない。
    /// 変わっていれば、新しい起点から前回の History に当たるまでだけ辿り、レイアウトも
    /// 変わった所だけ計算し直す。
    pub fn load_history(&self) -> Result<Arc<HistorySnapshot>> {
        // 手元の共有の daemon が用意してくれるなら、レイアウトは計算せず、そのファイルを mmap する。
        if let Some(path) = self.backend.prepared_history()? {
            let cached = cache::read(&path).ok_or("daemon が用意した History を読めない")?;
            if let Some(current) = self.history.load_full()
                && current.tips == cached.tips
                && current.head == cached.head
            {
                return Ok(current);
            }
            return Ok(self.publish(cached.tips, cached.head, cached.graph, cached.layout));
        }
        let tips = self.backend.tips()?;
        let head = self.backend.head()?;
        let current = self.history.load_full();
        if let Some(current) = &current
            && current.tips == tips
            && current.head == head
        {
            // 行の索引を裏のスレッドで作っておく（サイドバーから History の行に飛ぶときに使う）。
            current.rows();
            return Ok(current.clone());
        }
        let graph = match &current {
            Some(old) => {
                let rows = old.rows();
                let new = self
                    .backend
                    .read_new_commits(&tips, &old.tips, &|id| rows.contains_key(id))?;
                topo::extend(new, &old.graph, rows, &tips)
            }
            None => self.backend.commit_graph(&tips)?,
        };
        let head_row = head
            .and_then(|h| graph.ids().iter().position(|id| *id == h))
            .map(|r| r as u32);
        let layout = match current {
            Some(old) => graph::relayout(&old.layout, &old.graph, &graph, head_row),
            None => graph::layout(&graph, head_row),
        };
        if let Some(path) = &self.cache_path {
            let cached = CachedHistory {
                tips,
                head,
                graph,
                layout,
            };
            // キャッシュを書けなくても History は使える。次の起動が遅くなるだけ。
            let _ = cache::write(path, &cached);
            return Ok(self.publish(cached.tips, cached.head, cached.graph, cached.layout));
        }
        Ok(self.publish(tips, head, graph, layout))
    }

    /// `tips` から辿れるコミットだけの History（ブランチで絞り込む）。最新の History から作り、公開はしない。
    pub fn history_filtered(&self, tips: &[Oid]) -> Result<Arc<HistorySnapshot>> {
        self.history_shaped(Some(tips), None)
    }

    /// マージ済みブランチを折りたたんだ History。`tips` があればそこから辿れるものだけ。`expanded` の merge は
    /// 取り込んだ側も見せる。最新の History から作り、公開はしない。
    pub fn history_folded(&self, tips: Option<&[Oid]>, expanded: &[Oid]) -> Result<Arc<HistorySnapshot>> {
        self.history_shaped(tips, Some(expanded))
    }

    fn history_shaped(&self, tips: Option<&[Oid]>, fold: Option<&[Oid]>) -> Result<Arc<HistorySnapshot>> {
        let full = match self.history.load_full() {
            Some(h) => h,
            None => self.load_history()?,
        };
        let filtered = tips.map(|tips| {
            let roots: Vec<u32> = tips.iter().filter_map(|t| full.row_of(t)).collect();
            full.graph.reachable_from(&roots)
        });
        let mut merges = Vec::new();
        let (graph, folded) = match (fold, filtered) {
            (Some(expanded), filtered) => {
                let base = filtered.as_ref().unwrap_or(&full.graph);
                // 起点（絞り込んだ ref と HEAD）は、ほかのブランチの途中にあっても幹として残す。
                // ID から行は 1 回なめて引く（ref が数千あっても、行ごとに探さない）。
                // 同じ ID が何度出ても（HEAD が起点にも含まれるなど）、番号は 1 つにする。
                let mut wanted: std::collections::HashMap<Oid, usize> = std::collections::HashMap::new();
                for id in tips.unwrap_or(&[]).iter().chain(full.head.iter()).chain(expanded.iter()) {
                    let next = wanted.len();
                    wanted.entry(*id).or_insert(next);
                }
                let mut found = vec![u32::MAX; wanted.len()];
                for (r, id) in base.ids().iter().enumerate() {
                    if let Some(&i) = wanted.get(id) {
                        found[i] = r as u32;
                    }
                }
                let rows_of = |ids: &[Oid]| -> Vec<u32> {
                    ids.iter().filter_map(|id| wanted.get(id).map(|&i| found[i])).filter(|&r| r != u32::MAX).collect()
                };
                // 全ブランチのときは、ほかのコミットの親になっていない先端（と HEAD）だけを幹にする。取り込み済みの
                // ブランチの先端は merge の親なので、折りたたまれる。
                let mut roots = tips.map(rows_of).unwrap_or_default();
                roots.extend(full.head.iter().filter_map(|h| wanted.get(h).map(|&i| found[i])).filter(|&r| r != u32::MAX));
                let expanded = rows_of(expanded);
                let f = crate::fold::fold_merged(base, &roots, &expanded);
                merges = f.merges;
                (f.graph, f.hidden)
            }
            (None, Some(filtered)) => (filtered, Vec::new()),
            (None, None) => return Ok(full),
        };
        let head_row = full
            .head
            .and_then(|h| graph.ids().iter().position(|id| *id == h))
            .map(|r| r as u32);
        let mut layout = graph::layout(&graph, head_row);
        layout.mark_merges(&merges);
        Ok(Arc::new(HistorySnapshot {
            generation: self.generation.fetch_add(1, Ordering::Relaxed) + 1,
            tips: tips.map_or_else(|| full.tips.clone(), <[Oid]>::to_vec),
            head: full.head,
            graph,
            layout,
            folded,
            rows: OnceLock::new(),
        }))
    }

    /// History の永続キャッシュのファイル（あれば）。
    pub fn cache_path(&self) -> Option<&std::path::Path> {
        self.cache_path.as_deref()
    }

    pub fn history(&self) -> Option<Arc<HistorySnapshot>> {
        self.history.load_full()
    }

    pub fn paths(&self) -> RepoPaths {
        self.backend.paths()
    }

    /// ref の一覧を読み直して公開する。前回と同じなら前回のものを返す。
    /// 最後に読んだ ref の一覧。まだ読んでいなければ読む（変化の検知で `load_refs` が読み直すので、
    /// コミットの詳細のような頻繁な問い合わせではこちらを使う）。
    pub fn refs(&self) -> Result<Arc<RefsSnapshot>> {
        match self.refs.load_full() {
            Some(refs) => Ok(refs),
            None => self.load_refs(),
        }
    }

    pub fn load_refs(&self) -> Result<Arc<RefsSnapshot>> {
        let refs = self.backend.refs()?;
        if let Some(current) = self.refs.load_full()
            && current.refs == refs
        {
            return Ok(current);
        }
        let snapshot = Arc::new(RefsSnapshot {
            generation: self.generation.fetch_add(1, Ordering::Relaxed) + 1,
            refs,
        });
        self.refs.store(Some(snapshot.clone()));
        Ok(snapshot)
    }

    /// `commit` で変わったファイル。
    pub fn commit_changes(&self, commit: &Oid) -> Result<Vec<FileChange>> {
        self.backend.commit_changes(commit)
    }

    /// stash の中身（作業中の変更と、含めていれば untracked のファイル）。
    pub fn stash_changes(&self, stash: &Oid) -> Result<Vec<FileChange>> {
        self.backend.stash_changes(stash)
    }

    /// 変わったファイル 1 つの、前後 3 行の文脈つきの差分。
    pub fn file_diff(&self, change: &FileChange, profile: &FrontendProfile) -> Result<DiffView> {
        let diff = match self.change_diffs(std::slice::from_ref(change))? {
            Some(mut diffs) if diffs.len() == 1 => diffs.remove(0),
            _ => self.change_diff(change)?,
        };
        Ok(DiffView::new(&diff, profile).with_path(&change.path))
    }

    /// `changes` のすべてのファイルの差分（History で全ファイルを縦に並べて見せる）。ファイルの数だけ blob を読んで
    /// 差分を取るので、裏のスレッドで呼ぶ。
    pub fn stacked_diff(&self, changes: &[FileChange]) -> Result<diff::StackedDiff> {
        if let Some(diffs) = self.change_diffs(changes)? {
            let files = changes.iter().map(|c| c.path.clone()).zip(diffs).collect();
            return Ok(diff::StackedDiff { files });
        }
        let one = |c: &FileChange| Ok((c.path.clone(), self.change_diff(c)?));
        // ファイルごとの差分は独立しているので、多ければ分けて並列に取る（コミットを選んでから表示までの時間）。
        let threads = std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .min(8);
        if changes.len() < 4 || threads < 2 {
            let files = changes.iter().map(one).collect::<Result<_>>()?;
            return Ok(diff::StackedDiff { files });
        }
        let chunk = changes.len().div_ceil(threads);
        let parts: Vec<Result<Vec<_>>> = std::thread::scope(|scope| {
            let handles: Vec<_> = changes
                .chunks(chunk)
                .map(|part| scope.spawn(move || part.iter().map(one).collect::<Result<Vec<_>>>()))
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().unwrap_or_else(|_| Err("差分を取れない".into())))
                .collect()
        });
        let mut files = Vec::with_capacity(changes.len());
        for part in parts {
            files.extend(part?);
        }
        Ok(diff::StackedDiff { files })
    }

    /// History で選んだコミット（行、順不同）の差分の見せ方と、その全ファイルの差分。
    /// - first-parent でつながる範囲: 一番古いものの親から一番新しいものまで（まとめて 1 つにしたのと同じ）
    /// - 2 つ（つながっていない）: 古いほうから新しいほうまで
    /// - 3 つ以上でとびとび: コミットごとの差分を、ファイルのパス順・コミットの古い順に並べる（見出しは
    ///   「パス — 短い ID」）
    ///
    /// 裏のスレッドで呼ぶ。
    pub fn selection_diff(
        &self,
        snapshot: &HistorySnapshot,
        rows: &[u32],
        pair: PairMode,
    ) -> Result<(Selection, diff::StackedDiff)> {
        let selection = Selection::of(&snapshot.graph, rows)?;
        let stacked = match &selection {
            Selection::Pair { from, to } if pair == PairMode::Reverse => {
                self.stacked_diff(&self.backend.changes_between(Some(to), from)?)?
            }
            Selection::Pair { to, .. } if pair == PairMode::MergeBase => {
                let mut sorted = rows.to_vec();
                sorted.sort_unstable();
                let base = merge_base(&snapshot.graph, sorted[0], sorted[1])
                    .map(|r| snapshot.graph.ids()[r as usize]);
                self.stacked_diff(&self.backend.changes_between(base.as_ref(), to)?)?
            }
            Selection::Range { base, tip, .. } => {
                self.stacked_diff(&self.backend.changes_between(base.as_ref(), tip)?)?
            }
            Selection::Pair { from, to } => {
                self.stacked_diff(&self.backend.changes_between(Some(from), to)?)?
            }
            Selection::Each(commits) => {
                let mut files = Vec::new();
                for (order, id) in commits.iter().enumerate() {
                    let short = id.to_string()[..8].to_owned();
                    for (path, diff) in self.stacked_diff(&self.backend.commit_changes(id)?)?.files
                    {
                        files.push((path.clone(), order, format!("{path} — {short}"), diff));
                    }
                }
                files.sort_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)));
                diff::StackedDiff {
                    files: files.into_iter().map(|(_, _, name, d)| (name, d)).collect(),
                }
            }
        };
        Ok((selection, stacked))
    }

    /// 手元の共有の daemon のコアに任せる（つないでいなければ `None`）。答えと、大きなバイト列（`parts`）を返す。
    pub(crate) fn delegate(
        &self,
        request: &crate::delegate::Request,
    ) -> Result<Option<(crate::delegate::Reply, Vec<amonhen_layout::Blob>)>> {
        let Some(answer) = self.backend.delegate(&crate::delegate::encode(request)?)? else {
            return Ok(None);
        };
        Ok(Some((crate::delegate::decode(&answer.reply)?, answer.parts)))
    }

    /// 変更ごとの差分。手元の共有の daemon につないでいれば、daemon で計算して 1 回でもらう（中身は共有メモリ）。
    fn change_diffs(&self, changes: &[FileChange]) -> Result<Option<Vec<FileDiff>>> {
        match self.delegate(&crate::delegate::Request::ChangeDiffs(changes.to_vec()))? {
            Some((crate::delegate::Reply::ChangeDiffs(metas), parts)) => {
                if parts.len() != metas.len() * 2 {
                    return Err("daemon の差分の中身の数が合わない".into());
                }
                let mut parts = parts.into_iter();
                Ok(Some(
                    metas
                        .into_iter()
                        .map(|m| {
                            let (old, new) = (parts.next().unwrap(), parts.next().unwrap());
                            m.into_diff(old, new)
                        })
                        .collect(),
                ))
            }
            Some(_) => Err("daemon の答えの形が違う".into()),
            None => Ok(None),
        }
    }

    /// daemon の側: つないだ側のコアから任された処理を実行する。
    pub fn serve_delegated(&self, request: &[u8]) -> Result<amonhen_model::Delegated> {
        use crate::delegate::{DiffMeta, Reply, Request};
        let (reply, parts) = match crate::delegate::decode::<Request>(request)? {
            Request::Command(json) => (Reply::Command(self.run_json(&json)), Vec::new()),
            Request::ChangeDiffs(changes) => {
                let diffs = self.stacked_diff(&changes)?;
                let metas = diffs.files.iter().map(|(_, d)| DiffMeta::from(d)).collect();
                let parts = diffs.files.into_iter().flat_map(|(_, d)| [d.old, d.new]).collect();
                (Reply::ChangeDiffs(metas), parts)
            }
        };
        Ok(amonhen_model::Delegated {
            reply: crate::delegate::encode(&reply)?,
            parts,
        })
    }

    fn change_diff(&self, change: &FileChange) -> Result<FileDiff> {
        let side = |id: &Option<Oid>| -> Result<Arc<Vec<u8>>> {
            match id {
                None => Ok(Arc::default()),
                // サブモジュールは git と同じく、指すコミットを 1 行の文字列にして比べる。
                Some(id) if change.submodule => {
                    Ok(Arc::new(format!("Subproject commit {id}\n").into_bytes()))
                }
                Some(id) => self.backend.blob(id),
            }
        };
        Ok(diff::file_diff(
            side(&change.old_id)?,
            side(&change.new_id)?,
            3,
        ))
    }

    /// 作業ツリーと index の状態を読み直す。前回と同じなら前回のものを返す。
    pub fn load_status(&self) -> Result<Arc<StatusSnapshot>> {
        let status = self.backend.status()?;
        if let Some(current) = self.status.load_full()
            && current.status == status
        {
            return Ok(current);
        }
        let snapshot = Arc::new(StatusSnapshot {
            generation: self.generation.fetch_add(1, Ordering::Relaxed) + 1,
            status,
        });
        self.status.store(Some(snapshot.clone()));
        Ok(snapshot)
    }

    /// Working Copy の 1 ファイルの差分。`staged` なら HEAD と index、そうでなければ index と作業ツリー。
    pub fn working_diff(
        &self,
        entry: &StatusEntry,
        staged: bool,
        profile: &FrontendProfile,
    ) -> Result<WorkingDiff> {
        let blob = |id: &Option<Oid>| -> Result<Arc<Vec<u8>>> {
            match id {
                Some(id) if entry.submodule => {
                    Ok(Arc::new(format!("Subproject commit {id}\n").into_bytes()))
                }
                Some(id) => self.backend.blob(id),
                None => Ok(Arc::default()),
            }
        };
        let (old, new) = if staged {
            (blob(&entry.head_id)?, blob(&entry.index_id)?)
        } else {
            let worktree = match entry.worktree {
                b'D' => Arc::default(),
                _ if entry.submodule => Arc::default(),
                _ => self.backend.worktree_file(&entry.path)?,
            };
            (blob(&entry.index_id)?, worktree)
        };
        let file = diff::file_diff(old, new, 3);
        let view = DiffView::new(&file, profile).with_path(&entry.path);
        Ok(WorkingDiff {
            entry: entry.clone(),
            staged,
            combined: None,
            file,
            view,
        })
    }

    /// Working Copy の 1 ファイルの、HEAD と作業ツリーの差分。変更の行ごとに、index に入っているか
    /// （次のコミットに含めるか）を `DiffRow::STAGED` で示す。index がその形で表せないとき・サブモジュール・
    /// バイナリ・コンフリクトでは None（FE は stage 済みと作業ツリーの差分を分けて見せる）。
    pub fn working_diff_combined(
        &self,
        entry: &StatusEntry,
        profile: &FrontendProfile,
    ) -> Result<Option<WorkingDiff>> {
        if entry.submodule || entry.kind == amonhen_model::StatusKind::Unmerged {
            return Ok(None);
        }
        let blob = |id: &Option<Oid>| -> Result<Arc<Vec<u8>>> {
            match id {
                Some(id) => self.backend.blob(id),
                None => Ok(Arc::default()),
            }
        };
        let head = blob(&entry.head_id)?;
        let index = blob(&entry.index_id)?;
        let worktree = match entry.worktree {
            b'D' => Arc::default(),
            _ => self.backend.worktree_file(&entry.path)?,
        };
        let file = diff::file_diff(head, worktree, 3);
        let Some(staged) = diff::staged_lines(&file, &index) else {
            return Ok(None);
        };
        let mut view = DiffView::new(&file, profile).with_path(&entry.path);
        for (row, &on) in view.rows.iter_mut().zip(&staged) {
            if on {
                row.flags |= DiffRow::STAGED;
            }
        }
        Ok(Some(WorkingDiff {
            entry: entry.clone(),
            staged: false,
            combined: Some(staged),
            file,
            view,
        }))
    }

    /// `working_diff_combined` の差分の `rows` 行目の変更を、index に入れる（`stage`）・外す。ほかの行はそのまま。
    /// すべて入れるなら `git add`、すべて外すなら `git reset` と同じにする（新しいファイル・消したファイルも
    /// 正しく扱うため）。
    pub fn set_rows_staged(&self, diff: &WorkingDiff, rows: &[usize], stage: bool) -> Result<()> {
        let Some(current) = &diff.combined else {
            return Err("HEAD と作業ツリーの差分ではない".into());
        };
        let state = |r: usize| if rows.contains(&r) { stage } else { current[r] };
        let changes: Vec<usize> = (0..diff.file.lines.len())
            .filter(|&r| matches!(diff.file.lines[r].kind, DiffRow::ADDED | DiffRow::REMOVED))
            .collect();
        let path = diff.entry.path.as_str();
        if changes.iter().all(|&r| state(r)) {
            return self.backend.stage(&[path]);
        }
        if !changes.iter().any(|&r| state(r)) {
            return self.backend.unstage(&[path]);
        }
        let content = diff::apply_selection(&diff.file, state);
        let mode = match diff.entry.index_mode {
            0 => 0o100644,
            mode => mode,
        };
        self.backend.set_index_content(path, mode, &content)
    }

    pub fn stage(&self, paths: &[&str]) -> Result<()> {
        self.backend.stage(paths)
    }

    pub fn unstage(&self, paths: &[&str]) -> Result<()> {
        self.backend.unstage(paths)
    }

    /// `diff` の `rows` 行目の変更だけを stage する（stage 済みの差分なら unstage する）。
    pub fn stage_rows(&self, diff: &WorkingDiff, rows: &[usize]) -> Result<()> {
        let selected = |r: usize| rows.contains(&r);
        let content = if diff.staged {
            // index から選んだ変更を取り除く = HEAD に選ばなかった変更だけを反映する。
            diff::apply_selection(&diff.file, |r| !selected(r))
        } else {
            diff::apply_selection(&diff.file, selected)
        };
        let mode = match diff.entry.index_mode {
            0 => 0o100644,
            mode => mode,
        };
        self.backend
            .set_index_content(&diff.entry.path, mode, &content)
    }

    /// コミットする（Undo できる。index と作業ツリーはそのままでブランチだけ戻る）。
    pub fn commit(&self, message: &str, amend: bool) -> Result<()> {
        let label = if amend { "Amend commit" } else { "Commit" };
        let ops = self.backend.operations()?;
        self.undo_log()?.record(
            ops,
            label,
            &[],
            true,
            || self.backend.commit(message, amend),
            |_| false,
        )
    }

    /// コンフリクトのある作業ツリーのファイルを、FE 向けの行にする。
    pub fn conflict_view(&self, path: &str, profile: &FrontendProfile) -> Result<DiffView> {
        Ok(crate::conflict::view(self.backend.worktree_file(path)?, profile).with_path(path))
    }

    /// `diff` の `rows` 行目の変更を作業ツリーから取り除く（Undo できる）。stage していない差分にだけ使える。
    pub fn discard_rows(&self, diff: &WorkingDiff, rows: &[usize]) -> Result<()> {
        let staged_row = diff
            .combined
            .as_ref()
            .is_some_and(|c| rows.iter().any(|&r| c[r]));
        if diff.staged || staged_row {
            return Err("stage 済みの変更は破棄できない（先に unstage する）".into());
        }
        let content = diff::apply_selection(&diff.file, |r| !rows.contains(&r));
        // 作業ツリーの内容を、選ばなかった変更だけを index に当てたものにする = 選んだ行を戻す。
        let path = diff.entry.path.clone();
        let ops = self.backend.operations()?;
        let files = self.backend.files();
        self.undo_log()?
            .record_files(ops, &format!("Discard lines in {path}"), &[&path], || {
                files.write(amonhen_model::Root::Workdir, &path, &content)
            })
    }

    /// commit-graph がなければ書き出し、書き出したら true を返す。リポジトリの `.git` に書き込むので、
    /// 利用者が設定で選んだときだけ呼ぶ。大きなリポジトリでは数十秒かかるので裏のスレッドで呼ぶこと。
    pub fn write_commit_graph_if_missing(&self) -> Result<bool> {
        if self.backend.has_commit_graph()? {
            return Ok(false);
        }
        self.backend.write_commit_graph()?;
        Ok(true)
    }

    /// `snapshot` の `start` 行から `count` 行分の、FE 向けの列。
    pub fn history_chunk(
        &self,
        snapshot: &HistorySnapshot,
        start: u32,
        count: u32,
        profile: &FrontendProfile,
    ) -> Result<HistoryChunk> {
        let ids = snapshot.graph.ids();
        let start = (start as usize).min(ids.len());
        let end = start.saturating_add(count as usize).min(ids.len());
        let texts = self.backend.commit_summaries(&ids[start..end])?;
        Ok(HistoryChunk::new(start as u32, &texts, profile))
    }

    /// コミットの件名・作者・日時（CLI の log など、FE 向けの形にしないで使うとき）。
    pub fn commit_texts(&self, ids: &[Oid]) -> Result<Vec<amonhen_model::CommitText>> {
        self.backend.commit_summaries(ids)
    }

    /// 走っている検索をやめる。
    pub fn cancel_search(&self) {
        self.search_generation.fetch_add(1, Ordering::Relaxed);
    }

    /// `query`（`crate::query` の構文）に合うコミットの行（昇順）。件名・作者・ID はコミットを読みながら
    /// 行を分けて並列に、パス・内容は git に問い合わせて探し、行の集合を組み合わせる。新しい検索を始めるか
    /// `cancel_search` すると、走っている検索は `Err` で終わる。
    pub fn search_history(&self, snapshot: &HistorySnapshot, query: &str) -> Result<Vec<u32>> {
        let generation = self.search_generation.fetch_add(1, Ordering::Relaxed) + 1;
        let cancelled = || self.search_generation.load(Ordering::Relaxed) != generation;
        let Some(expr) = crate::query::parse(query)? else {
            return Ok(Vec::new());
        };
        let rows = snapshot.graph.len();
        let terms = expr.terms();
        // 葉の条件ごとの、合う行の集合（ビット列）。
        let mut sets: Vec<Option<RowSet>> = vec![None; terms.len()];
        let textual: Vec<usize> = (0..terms.len())
            .filter(|&i| terms[i].field.is_textual())
            .collect();
        let ops = self.backend.operations()?;
        // パス・内容の問い合わせ（git）は、件名・作者を読む走査と同時に進める。
        let (scanned, queried) = std::thread::scope(|scope| {
            let queries: Vec<_> = terms
                .iter()
                .enumerate()
                .filter(|(_, t)| !t.field.is_textual())
                .map(|(i, term)| {
                    let (path, contents) = match term.field {
                        crate::query::Field::Path => (Some(term.value.as_str()), None),
                        _ => (None, Some(term.value.as_str())),
                    };
                    let cancelled = &cancelled;
                    let handle = scope
                        .spawn(move || ops.log_matching(&snapshot.tips, path, contents, cancelled));
                    (i, handle)
                })
                .collect();
            let scanned = if textual.is_empty() {
                Ok(Vec::new())
            } else {
                let textual_terms: Vec<_> = textual.iter().map(|&i| terms[i]).collect();
                self.scan_texts(snapshot, &textual_terms, &cancelled)
            };
            let queried: Vec<_> = queries
                .into_iter()
                .map(|(i, handle)| {
                    let ids = handle
                        .join()
                        .unwrap_or_else(|_| Err("検索が異常終了しました".into()));
                    (i, ids)
                })
                .collect();
            (scanned, queried)
        });
        for (i, set) in textual.iter().zip(scanned?) {
            sets[*i] = Some(set);
        }
        for (i, ids) in queried {
            let mut set = RowSet::new(rows);
            for id in &ids? {
                if let Some(row) = snapshot.row_of(id) {
                    set.insert(row as usize);
                }
            }
            sets[i] = Some(set);
        }
        if cancelled() {
            return Err("検索を取り消しました".into());
        }
        let mut next = 0;
        let result = combine(&expr, &mut sets, &mut next, rows);
        Ok(result.rows())
    }

    /// 件名・作者・ID の条件を、コミットを読みながら行を分けて並列に調べる。
    fn scan_texts(
        &self,
        snapshot: &HistorySnapshot,
        terms: &[&crate::query::Term],
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Vec<RowSet>> {
        use crate::query::Field;
        use amonhen_model::search::{TextField, TextTerm};
        let terms: Vec<TextTerm> = terms
            .iter()
            .map(|t| {
                let field = match t.field {
                    Field::Author => TextField::Author,
                    Field::Subject => TextField::Subject,
                    Field::Commit => TextField::Commit,
                    _ => TextField::Any,
                };
                TextTerm::new(field, &t.value)
            })
            .collect();
        let rows = snapshot.graph.len();
        // daemon 越しなら daemon の側で探してもらい、合ったコミットの ID を行に直す。
        if let Some(found) = self
            .backend
            .search_texts(&snapshot.tips, &terms, cancelled)?
        {
            return Ok(found
                .iter()
                .map(|ids| {
                    let mut set = RowSet::new(rows);
                    for id in ids {
                        if let Some(row) = snapshot.row_of(id) {
                            set.insert(row as usize);
                        }
                    }
                    set
                })
                .collect());
        }
        let words = self.texts.search(
            snapshot.graph.ids(),
            &terms,
            &|batch| self.backend.commit_texts(batch),
            cancelled,
        )?;
        Ok(words.into_iter().map(|words| RowSet { words }).collect())
    }
}

/// 行の集合（行番号のビット列）。
#[derive(Clone, Debug)]
struct RowSet {
    words: Vec<u64>,
}

impl RowSet {
    fn new(rows: usize) -> Self {
        Self {
            words: vec![0; rows.div_ceil(64)],
        }
    }

    fn insert(&mut self, row: usize) {
        self.words[row / 64] |= 1 << (row % 64);
    }

    fn rows(&self) -> Vec<u32> {
        let mut out = Vec::new();
        for (i, &w) in self.words.iter().enumerate() {
            let mut w = w;
            while w != 0 {
                out.push((i * 64 + w.trailing_zeros() as usize) as u32);
                w &= w - 1;
            }
        }
        out
    }
}

/// 葉の集合（`terms()` の順）を、式に従って組み合わせる。
fn combine(
    expr: &crate::query::Expr,
    sets: &mut [Option<RowSet>],
    next: &mut usize,
    rows: usize,
) -> RowSet {
    use crate::query::Expr;
    match expr {
        Expr::Term(_) => {
            let set = sets[*next].take().unwrap_or_else(|| RowSet::new(rows));
            *next += 1;
            set
        }
        Expr::And(all) | Expr::Or(all) => {
            let is_and = matches!(expr, Expr::And(_));
            let mut acc: Option<RowSet> = None;
            for e in all {
                let set = combine(e, sets, next, rows);
                acc = Some(match acc {
                    None => set,
                    Some(mut a) => {
                        for (x, y) in a.words.iter_mut().zip(&set.words) {
                            if is_and { *x &= y } else { *x |= y }
                        }
                        a
                    }
                });
            }
            acc.unwrap_or_else(|| RowSet::new(rows))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use amonhen_backend_gix::GixBackend;
    use std::process::Command;

    fn git(dir: &std::path::Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "a")
            .env("GIT_AUTHOR_EMAIL", "a@example.com")
            .env("GIT_COMMITTER_NAME", "a")
            .env("GIT_COMMITTER_EMAIL", "a@example.com")
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
        // テスト用のリポジトリは、CI の環境（利用者名がない・Windows の autocrlf）に左右されないようにする。
        if args.first() == Some(&"init") {
            for (key, value) in [
                ("user.name", "a"),
                ("user.email", "a@example.com"),
                ("core.autocrlf", "false"),
            ] {
                git(dir, &["config", key, value]);
            }
        }
    }

    #[test]
    fn shows_the_cached_history_first_and_skips_work_when_nothing_changed() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("repo");
        std::fs::create_dir(&dir).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        git(&dir, &["commit", "--allow-empty", "-q", "-m", "first"]);
        let cache = tmp.path().join("cache/history.bin");

        let repo = Repository::with_cache(GixBackend::open(&dir).unwrap(), cache.clone());
        assert!(repo.load_cached_history().is_none());
        let loaded = repo.load_history().unwrap();
        assert!(cache.exists());

        // 次の起動。
        let repo = Repository::with_cache(GixBackend::open(&dir).unwrap(), cache.clone());
        let cached = repo.load_cached_history().unwrap();
        assert_eq!(cached.graph, loaded.graph);
        let same = repo.load_history().unwrap();
        assert!(
            Arc::ptr_eq(&same, &cached),
            "変わっていなければ作り直さない"
        );

        git(&dir, &["commit", "--allow-empty", "-q", "-m", "second"]);
        let updated = repo.load_history().unwrap();
        assert_eq!((updated.generation, updated.graph.len()), (2, 2));
        let repo = Repository::with_cache(GixBackend::open(&dir).unwrap(), cache);
        assert_eq!(repo.load_cached_history().unwrap().graph.len(), 2);
    }

    #[test]
    fn searches_subjects_authors_and_commit_ids() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        git(dir, &["init", "-q", "-b", "main"]);
        for message in ["Add Parser", "fix typo", "parse more", "release"] {
            git(dir, &["commit", "--allow-empty", "-q", "-m", message]);
        }
        let repo =
            Repository::with_cache(GixBackend::open(dir).unwrap(), tmp.path().join("cache.bin"));
        let history = repo.load_history().unwrap();
        // 新しい順: release, parse more, fix typo, Add Parser。
        assert_eq!(repo.search_history(&history, "PARSE").unwrap(), [1, 3]);
        assert_eq!(
            repo.search_history(&history, "  ").unwrap(),
            Vec::<u32>::new()
        );
        // 作者名（テストでは "a"）。
        assert_eq!(repo.search_history(&history, "a").unwrap().len(), 4);
        let id = history.graph.ids()[2].to_string();
        assert_eq!(repo.search_history(&history, &id[..7]).unwrap(), [2]);
        assert_eq!(
            repo.search_history(&history, &format!("commit:{}", &id[..7]))
                .unwrap(),
            [2]
        );
        // 構文: AND・OR・括弧・接頭辞。
        assert_eq!(repo.search_history(&history, "parse more").unwrap(), [1]);
        assert_eq!(
            repo.search_history(&history, "typo or release").unwrap(),
            [0, 2]
        );
        assert_eq!(
            repo.search_history(&history, "subject:parse (add or more)")
                .unwrap(),
            [1, 3]
        );
        assert_eq!(
            repo.search_history(&history, "author:nobody").unwrap(),
            Vec::<u32>::new()
        );
        assert!(repo.search_history(&history, "(unclosed").is_err());
    }

    #[test]
    fn searches_paths_and_contents_through_git() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        git(dir, &["init", "-q", "-b", "main"]);
        std::fs::create_dir(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/a.rs"), "fn a() {}\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "add a"]);
        std::fs::write(dir.join("README"), "spin_lock(x)\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "readme"]);
        std::fs::write(dir.join("src/a.rs"), "fn a() { spin_lock(y); }\n").unwrap();
        git(dir, &["commit", "-q", "-am", "lock in a"]);
        let repo =
            Repository::with_cache(GixBackend::open(dir).unwrap(), tmp.path().join("cache.bin"));
        let history = repo.load_history().unwrap();
        // 新しい順: lock in a, readme, add a。
        assert_eq!(repo.search_history(&history, "path:src").unwrap(), [0, 2]);
        // 正規表現の記号もそのままの文字として探す。
        assert_eq!(
            repo.search_history(&history, r#"contents:"spin_lock(""#)
                .unwrap(),
            [0, 1]
        );
        assert_eq!(
            repo.search_history(&history, r#"path:src contents:"spin_lock(""#)
                .unwrap(),
            [0]
        );
        assert_eq!(
            repo.search_history(&history, "path:README or subject:add")
                .unwrap(),
            [1, 2]
        );
    }

    #[test]
    fn reads_only_new_commits_and_drops_deleted_branches() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        git(dir, &["init", "-q", "-b", "main"]);
        git(dir, &["commit", "--allow-empty", "-q", "-m", "root"]);
        git(dir, &["switch", "-q", "-c", "topic"]);
        git(dir, &["commit", "--allow-empty", "-q", "-m", "topic"]);
        git(dir, &["switch", "-q", "main"]);
        let repo = Repository::new(GixBackend::open(dir).unwrap());
        assert_eq!(repo.load_history().unwrap().graph.len(), 2);

        git(dir, &["commit", "--allow-empty", "-q", "-m", "main 1"]);
        git(
            dir,
            &["merge", "-q", "--no-ff", "-m", "merge topic", "topic"],
        );
        git(dir, &["branch", "-q", "-D", "topic"]);
        git(dir, &["switch", "-q", "-c", "gone"]);
        git(dir, &["commit", "--allow-empty", "-q", "-m", "gone"]);
        git(dir, &["switch", "-q", "main"]);
        git(dir, &["branch", "-q", "-D", "gone"]);
        let incremental = repo.load_history().unwrap();
        let full = GixBackend::open(dir)
            .unwrap()
            .commit_graph(&incremental.tips)
            .unwrap();

        // 同じコミットと親子関係（並び順は違ってよい）。
        let edges = |g: &CommitGraph| {
            let mut e: Vec<_> = (0..g.len())
                .map(|r| {
                    (
                        g.ids()[r],
                        g.parents(r)
                            .iter()
                            .map(|&p| g.ids()[p as usize])
                            .collect::<Vec<_>>(),
                    )
                })
                .collect();
            e.sort();
            e
        };
        assert_eq!(incremental.graph.len(), 4);
        assert_eq!(edges(&incremental.graph), edges(&full));
        for r in 0..incremental.graph.len() {
            assert!(incremental.graph.parents(r).iter().all(|&p| p as usize > r));
        }
    }

    #[test]
    fn stages_and_unstages_selected_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        git(dir, &["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("a.txt"), "1\n2\n3\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "first"]);
        std::fs::write(dir.join("a.txt"), "one\n2\nthree\n").unwrap();

        let repo = Repository::new(GixBackend::open(dir).unwrap());
        let status = repo.load_status().unwrap();
        let entry = &status.status.entries[0];
        let unstaged = repo
            .working_diff(entry, false, &FrontendProfile::MACOS)
            .unwrap();
        // @@, -1, +one, 2, -3, +three
        assert_eq!(unstaged.view.rows.len(), 6);
        repo.stage_rows(&unstaged, &[1, 2]).unwrap();
        let show = |spec: &str| {
            let out = Command::new("git")
                .args(["show", spec])
                .current_dir(dir)
                .output()
                .unwrap();
            String::from_utf8(out.stdout).unwrap()
        };
        assert_eq!(show(":a.txt"), "one\n2\n3\n");

        let status = repo.load_status().unwrap();
        let entry = &status.status.entries[0];
        let staged = repo
            .working_diff(entry, true, &FrontendProfile::MACOS)
            .unwrap();
        repo.stage_rows(&staged, &[1, 2]).unwrap();
        assert_eq!(show(":a.txt"), "1\n2\n3\n");

        repo.stage(&["a.txt"]).unwrap();
        repo.commit("change a", false).unwrap();
        assert!(repo.load_status().unwrap().status.entries.is_empty());
    }

    #[test]
    fn marks_and_toggles_staged_lines_in_one_diff() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        git(dir, &["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("a.txt"), "1\n2\n3\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "first"]);
        // 1 行目の変更だけを stage し、3 行目は作業ツリーにだけある。
        std::fs::write(dir.join("a.txt"), "one\n2\n3\n").unwrap();
        git(dir, &["add", "."]);
        std::fs::write(dir.join("a.txt"), "one\n2\nthree\n").unwrap();
        let show = |spec: &str| {
            let out = Command::new("git")
                .args(["show", spec])
                .current_dir(dir)
                .output()
                .unwrap();
            String::from_utf8(out.stdout).unwrap()
        };
        let repo = Repository::new(GixBackend::open(dir).unwrap());
        let diff = |repo: &Repository| {
            let status = repo.load_status().unwrap();
            repo.working_diff_combined(&status.status.entries[0], &FrontendProfile::MACOS)
                .unwrap()
                .unwrap()
        };
        let staged = |d: &WorkingDiff| {
            d.view
                .rows
                .iter()
                .map(|r| r.flags & DiffRow::STAGED != 0)
                .collect::<Vec<_>>()
        };

        // @@, -1, +one, 2, -3, +three
        let d = diff(&repo);
        assert_eq!(staged(&d), [false, true, true, false, false, false]);
        // 3 行目の変更も入れる。
        repo.set_rows_staged(&d, &[4, 5], true).unwrap();
        assert_eq!(show(":a.txt"), "one\n2\nthree\n");
        // 1 行目の変更を外す。
        let d = diff(&repo);
        assert_eq!(staged(&d), [false, true, true, false, true, true]);
        repo.set_rows_staged(&d, &[1, 2], false).unwrap();
        assert_eq!(show(":a.txt"), "1\n2\nthree\n");
        // stage 済みの行は破棄できない。stage していない行は破棄できる。
        let d = diff(&repo);
        assert!(repo.discard_rows(&d, &[4, 5]).is_err());
        repo.discard_rows(&d, &[1, 2]).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "1\n2\nthree\n"
        );
        // すべて外すと index は HEAD に戻る。差分は @@, 1, 2, -3, +three になっている。
        let d = diff(&repo);
        assert_eq!(staged(&d), [false, false, false, true, true]);
        repo.set_rows_staged(&d, &[3, 4], false).unwrap();
        assert_eq!(show(":a.txt"), "1\n2\n3\n");
    }

    #[test]
    fn falls_back_when_the_index_is_not_part_of_the_worktree_change() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        git(dir, &["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("a.txt"), "1\n2\n3\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "first"]);
        // index にだけある変更を、作業ツリーで書き戻す。
        std::fs::write(dir.join("a.txt"), "one\n2\n3\n").unwrap();
        git(dir, &["add", "."]);
        std::fs::write(dir.join("a.txt"), "1\n2\n3\nfour\n").unwrap();
        let repo = Repository::new(GixBackend::open(dir).unwrap());
        let status = repo.load_status().unwrap();
        let entry = &status.status.entries[0];
        assert!(
            repo.working_diff_combined(entry, &FrontendProfile::MACOS)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn stages_part_of_a_new_file_and_unstages_it_entirely() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        git(dir, &["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("a.txt"), "1\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "first"]);
        std::fs::write(dir.join("new.txt"), "x\ny\n").unwrap();
        let repo = Repository::new(GixBackend::open(dir).unwrap());
        let diff = |repo: &Repository| {
            let status = repo.load_status().unwrap();
            let entry = status
                .status
                .entries
                .iter()
                .find(|e| e.path == "new.txt")
                .unwrap();
            repo.working_diff_combined(entry, &FrontendProfile::MACOS)
                .unwrap()
                .unwrap()
        };
        // @@, +x, +y
        let d = diff(&repo);
        repo.set_rows_staged(&d, &[1], true).unwrap();
        let out = Command::new("git")
            .args(["show", ":new.txt"])
            .current_dir(dir)
            .output()
            .unwrap();
        assert_eq!(String::from_utf8(out.stdout).unwrap(), "x\n");
        let d = diff(&repo);
        repo.set_rows_staged(&d, &[1], false).unwrap();
        // すべて外すと index から消え、untracked に戻る。
        let status = repo.load_status().unwrap();
        let entry = status
            .status
            .entries
            .iter()
            .find(|e| e.path == "new.txt")
            .unwrap();
        assert!(entry.index_id.is_none());
    }

    #[test]
    fn diffs_a_range_a_pair_or_each_selected_commit() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        git(dir, &["init", "-q", "-b", "main"]);
        // 0: c4（a を 4 に）、1: c3（b を足す）、2: c2（a を 2 に）、3: c1（a を作る）
        for (file, text, message) in [
            ("a.txt", "1\n", "c1"),
            ("a.txt", "2\n", "c2"),
            ("b.txt", "b\n", "c3"),
            ("a.txt", "4\n", "c4"),
        ] {
            std::fs::write(dir.join(file), text).unwrap();
            git(dir, &["add", "."]);
            git(dir, &["commit", "-q", "-m", message]);
        }
        let repo = Repository::new(GixBackend::open(dir).unwrap());
        let history = repo.load_history().unwrap();
        let paths =
            |d: &diff::StackedDiff| d.files.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>();

        // 続く c2..c4 は、c1 から c4 までの差分。
        let (selection, stacked) = repo
            .selection_diff(&history, &[2, 0, 1], PairMode::Forward)
            .unwrap();
        assert!(matches!(selection, Selection::Range { count: 3, .. }));
        assert_eq!(paths(&stacked), ["a.txt", "b.txt"]);
        let (_, a) = &stacked.files[0];
        assert_eq!((a.added, a.removed), (1, 1));

        // とびとびの 2 つ（c4 と c2）は、c2 から c4 まで。
        let (selection, stacked) = repo
            .selection_diff(&history, &[0, 2], PairMode::Forward)
            .unwrap();
        assert!(matches!(selection, Selection::Pair { .. }));
        assert_eq!(paths(&stacked), ["a.txt", "b.txt"]);

        // とびとびの 3 つ（c4・c2・c1）は、コミットごとの差分をパス順・古い順に。
        let (selection, stacked) = repo
            .selection_diff(&history, &[0, 2, 3], PairMode::Forward)
            .unwrap();
        assert!(matches!(selection, Selection::Each(ref ids) if ids.len() == 3));
        let names = paths(&stacked);
        assert_eq!(names.len(), 3);
        assert!(names.iter().all(|n| n.starts_with("a.txt — ")));
    }

    #[test]
    fn diffs_a_pair_reversed_or_from_the_merge_base() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        git(dir, &["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("a.txt"), "1\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "base"]);
        git(dir, &["checkout", "-q", "-b", "topic"]);
        std::fs::write(dir.join("t.txt"), "t\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "topic"]);
        git(dir, &["checkout", "-q", "main"]);
        std::fs::write(dir.join("m.txt"), "m\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "main"]);
        let repo = Repository::new(GixBackend::open(dir).unwrap());
        let history = repo.load_history().unwrap();
        let row = |subject: &str| {
            let out = Command::new("git")
                .args(["rev-parse", subject])
                .current_dir(dir)
                .output()
                .unwrap();
            let oid = Oid::from_hex(String::from_utf8(out.stdout).unwrap().trim()).unwrap();
            history.row_of(&oid).unwrap()
        };
        let (main, topic) = (row("main"), row("topic"));
        let paths = |mode| {
            let (_, s) = repo.selection_diff(&history, &[main, topic], mode).unwrap();
            s.files
                .iter()
                .map(|(p, d)| (p.clone(), d.added, d.removed))
                .collect::<Vec<_>>()
        };
        // 古いほうから新しいほう: 片方は足し、片方は消す。
        assert_eq!(paths(PairMode::Forward).len(), 2);
        // 向きを入れ替えると、足すと消すが入れ替わる。
        let (f, r) = (paths(PairMode::Forward), paths(PairMode::Reverse));
        assert_eq!(
            f.iter().map(|x| (x.1, x.2)).collect::<Vec<_>>(),
            r.iter().map(|x| (x.2, x.1)).collect::<Vec<_>>()
        );
        // 共通の祖先からは、新しいほうの側の変更だけ。
        let newer = if main < topic { "m.txt" } else { "t.txt" };
        assert_eq!(paths(PairMode::MergeBase), [(newer.to_owned(), 1, 0)]);
        assert_eq!(merge_base(&history.graph, main, topic), Some(row("main~1")));
    }

    #[test]
    fn publishes_a_new_generation_on_every_load() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        git(dir, &["init", "-q", "-b", "main"]);
        git(dir, &["commit", "--allow-empty", "-q", "-m", "first"]);

        let repo = Repository::new(GixBackend::open(dir).unwrap());
        assert!(repo.history().is_none());
        let first = repo.load_history().unwrap();
        assert_eq!((first.generation, first.graph.len()), (1, 1));

        git(dir, &["commit", "--allow-empty", "-q", "-m", "second"]);
        let second = repo.load_history().unwrap();
        assert_eq!((second.generation, second.graph.len()), (2, 2));
        // 古いスナップショットは手元にある限り書き換わらない。
        assert_eq!(first.graph.len(), 1);
        assert!(Arc::ptr_eq(&repo.history().unwrap(), &second));

        let chunk = repo
            .history_chunk(&second, 0, 10, &FrontendProfile::MACOS)
            .unwrap();
        assert_eq!(chunk.len(), 2);
        assert_eq!(chunk.bytes(chunk.subjects[0]), b"second");
    }
}
