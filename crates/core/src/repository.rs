use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use amonhen_model::{CommitGraph, GitBackend, Result};
use amonhen_projection::{FrontendProfile, HistoryChunk};
use arc_swap::ArcSwapOption;

use crate::graph::{self, GraphLayout};

/// History の不変のスナップショット。公開したら書き換えない。
pub struct HistorySnapshot {
    pub generation: u64,
    pub graph: CommitGraph,
    pub layout: GraphLayout,
}

/// 1 つのリポジトリの状態。読み取りはロックなしで、最新のスナップショットを取るだけ。
pub struct Repository {
    backend: Box<dyn GitBackend>,
    history: ArcSwapOption<HistorySnapshot>,
    generation: AtomicU64,
}

impl Repository {
    pub fn new(backend: impl GitBackend + 'static) -> Self {
        Self {
            backend: Box::new(backend),
            history: ArcSwapOption::empty(),
            generation: AtomicU64::new(0),
        }
    }

    /// History を読み直して公開する。前回のレイアウトがあれば、変わった所だけ計算し直す。
    pub fn load_history(&self) -> Result<Arc<HistorySnapshot>> {
        let tips = self.backend.tips()?;
        let head = self.backend.head()?;
        let graph = self.backend.commit_graph(&tips)?;
        let head_row = head
            .and_then(|h| graph.ids().iter().position(|id| *id == h))
            .map(|r| r as u32);
        let layout = match self.history.load_full() {
            Some(old) => graph::relayout(&old.layout, &old.graph, &graph, head_row),
            None => graph::layout(&graph, head_row),
        };
        let snapshot = Arc::new(HistorySnapshot {
            generation: self.generation.fetch_add(1, Ordering::Relaxed) + 1,
            graph,
            layout,
        });
        self.history.store(Some(snapshot.clone()));
        Ok(snapshot)
    }

    pub fn history(&self) -> Option<Arc<HistorySnapshot>> {
        self.history.load_full()
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
