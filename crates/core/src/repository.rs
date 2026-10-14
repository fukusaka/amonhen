use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

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
    pub staged: bool,
    pub file: FileDiff,
    pub view: DiffView,
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
    status: ArcSwapOption<StatusSnapshot>,
    generation: AtomicU64,
    cache_path: Option<PathBuf>,
    undo: OnceLock<UndoLog>,
}

impl Repository {
    pub fn new(backend: impl GitBackend + 'static) -> Self {
        Self {
            backend: Box::new(backend),
            history: ArcSwapOption::empty(),
            refs: ArcSwapOption::empty(),
            status: ArcSwapOption::empty(),
            generation: AtomicU64::new(0),
            cache_path: None,
            undo: OnceLock::new(),
        }
    }

    pub(crate) fn backend(&self) -> &dyn GitBackend {
        &*self.backend
    }

    pub(crate) fn undo_log(&self) -> Result<&UndoLog> {
        Ok(self.undo.get_or_init(|| {
            let paths = self.backend.paths();
            UndoLog::new(&paths.git_dir, paths.workdir.as_deref())
        }))
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
        let view = DiffView::new(&file, profile);
        Ok(WorkingDiff {
            entry: entry.clone(),
            staged,
            file,
            view,
        })
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
        Ok(crate::conflict::view(
            self.backend.worktree_file(path)?,
            profile,
        ))
    }

    /// `diff` の `rows` 行目の変更を作業ツリーから取り除く（Undo できる）。stage していない差分にだけ使える。
    pub fn discard_rows(&self, diff: &WorkingDiff, rows: &[usize]) -> Result<()> {
        if diff.staged {
            return Err("stage 済みの変更は破棄できない（先に unstage する）".into());
        }
        let content = diff::apply_selection(&diff.file, |r| !rows.contains(&r));
        // 作業ツリーの内容を、選ばなかった変更だけを index に当てたものにする = 選んだ行を戻す。
        let workdir = self
            .backend
            .paths()
            .workdir
            .ok_or("作業ツリーのないリポジトリ")?;
        let path = diff.entry.path.clone();
        let ops = self.backend.operations()?;
        self.undo_log()?
            .record_files(ops, &format!("Discard lines in {path}"), &[&path], || {
                Ok(std::fs::write(workdir.join(&path), &content)?)
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
