use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use amonhen_model::{
    CommitGraph, FileChange, GitBackend, Oid, OidMap, Refs, RepoPaths, Result, topo,
};
use amonhen_projection::{FrontendProfile, HistoryChunk};
use arc_swap::ArcSwapOption;

use crate::cache::{self, CachedHistory};
use crate::diff::{self, DiffView};
use crate::graph::{self, GraphLayout};

/// History の不変のスナップショット。公開したら書き換えない。
pub struct HistorySnapshot {
    pub generation: u64,
    /// 起点と HEAD。どちらも変わっていなければ History も変わらない（コミットは不変）。
    pub tips: Vec<Oid>,
    pub head: Option<Oid>,
    pub graph: CommitGraph,
    pub layout: GraphLayout,
    /// コミット ID から行への索引。次に読み直すとき、既知のコミットで走査を止めるのに使う。
    rows: OnceLock<OidMap<u32>>,
}

/// サイドバーの ref の一覧の不変のスナップショット。
pub struct RefsSnapshot {
    pub generation: u64,
    pub refs: Refs,
}

impl HistorySnapshot {
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
    generation: AtomicU64,
    cache_path: Option<PathBuf>,
}

impl Repository {
    pub fn new(backend: impl GitBackend + 'static) -> Self {
        Self {
            backend: Box::new(backend),
            history: ArcSwapOption::empty(),
            refs: ArcSwapOption::empty(),
            generation: AtomicU64::new(0),
            cache_path: None,
        }
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
                    .read_commits(&tips, &|id| rows.contains_key(id))?;
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

    pub fn history(&self) -> Option<Arc<HistorySnapshot>> {
        self.history.load_full()
    }

    pub fn paths(&self) -> RepoPaths {
        self.backend.paths()
    }

    /// ref の一覧を読み直して公開する。前回と同じなら前回のものを返す。
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

    /// 変わったファイル 1 つの、前後 3 行の文脈つきの差分。
    pub fn file_diff(&self, change: &FileChange, profile: &FrontendProfile) -> Result<DiffView> {
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
        let file = diff::file_diff(side(&change.old_id)?, side(&change.new_id)?, 3);
        Ok(DiffView::new(&file, profile))
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
        let texts = self.backend.commit_texts(&ids[start..end])?;
        Ok(HistoryChunk::new(start as u32, &texts, profile))
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
