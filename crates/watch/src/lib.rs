//! リポジトリの変化の検知（デスクトップ向け）。.git と作業ツリーを監視し、短い間の変化を
//! まとめて、何が変わったかの種類だけを知らせる。macOS は FSEvents、Linux は inotify、
//! Windows は ReadDirectoryChangesW（notify crate）。
//!
//! コアは OS 固有のコードを持たないので、監視はここに分けて FE の側から使う。

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

pub use amonhen_model::RepoPaths;
use notify::{RecursiveMode, Watcher as _};

/// 変化の種類（ビットの組み合わせ）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Changes(pub u32);

impl Changes {
    /// HEAD・ブランチ・タグ・stash。History とサイドバーを読み直す。
    pub const REFS: Self = Self(1 << 0);
    /// index（stage の状態）。
    pub const INDEX: Self = Self(1 << 1);
    /// 作業ツリーのファイル。
    pub const WORKTREE: Self = Self(1 << 2);
    /// 設定（リモートや上流の変更）。
    pub const CONFIG: Self = Self(1 << 3);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl std::ops::BitOr for Changes {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl std::ops::BitOrAssign for Changes {
    fn bitor_assign(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

/// `path` の変化がどの種類か。気にしなくてよい変化（オブジェクトの書き込み、ロックファイルなど）は空。
pub fn classify(paths: &RepoPaths, path: &Path) -> Changes {
    let in_git = |dir: &Path| path.strip_prefix(dir).ok();
    for dir in [&paths.git_dir, &paths.common_dir] {
        let Some(rel) = in_git(dir) else { continue };
        let first = rel
            .components()
            .next()
            .map(|c| c.as_os_str().to_string_lossy().into_owned());
        return match first.as_deref() {
            Some(
                "HEAD" | "refs" | "packed-refs" | "reftable" | "FETCH_HEAD" | "ORIG_HEAD"
                | "MERGE_HEAD",
            ) => Changes::REFS,
            // stash は refs/stash の reflog に積まれる。
            Some("logs") if rel.starts_with("logs/refs/stash") => Changes::REFS,
            Some("index") => Changes::INDEX,
            Some("config") => Changes::CONFIG,
            _ => Changes::default(),
        };
    }
    match &paths.workdir {
        Some(workdir) if path.starts_with(workdir) => Changes::WORKTREE,
        _ => Changes::default(),
    }
}

/// 中身を変えないイベント（開いた・読んだ・閉じた）。
fn is_read_only(kind: &notify::EventKind) -> bool {
    matches!(kind, notify::EventKind::Access(_))
}

/// 監視を止めるには落とす。
pub struct Watcher {
    _watcher: notify::RecommendedWatcher,
}

/// 変化を待つ時間。この間に続いた変化は 1 回にまとめる。
pub const SETTLE: Duration = Duration::from_millis(30);

impl Watcher {
    /// `on_change` は監視用のスレッドから呼ばれる。
    pub fn new(
        paths: RepoPaths,
        on_change: impl Fn(Changes) + Send + 'static,
    ) -> notify::Result<Self> {
        // FSEvents は実パス（macOS の /var は /private/var）で知らせてくるので、比べる側も揃える。
        let real = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_owned());
        let paths = RepoPaths {
            git_dir: real(&paths.git_dir),
            common_dir: real(&paths.common_dir),
            workdir: paths.workdir.as_deref().map(real),
        };
        let (tx, rx) = mpsc::channel::<Changes>();
        let classify_paths = paths.clone();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                let Ok(event) = event else { return };
                // Linux の inotify は読んだだけでも知らせる（git status が作業ツリーを読むたびに届き、
                // 読み直すとまた届く）。書き込みは Modify などで別に届くので、Access はすべて捨てる。
                if is_read_only(&event.kind) {
                    return;
                }
                let mut changes = Changes::default();
                for path in &event.paths {
                    changes |= classify(&classify_paths, path);
                }
                if !changes.is_empty() {
                    let _ = tx.send(changes);
                }
            })?;
        // 作業ツリーを再帰的に見れば、その中の .git も見える。外にある .git（worktree や
        // bare リポジトリ）だけ別に見る。
        let mut roots: Vec<&Path> = paths.workdir.iter().map(PathBuf::as_path).collect();
        for dir in [&paths.git_dir, &paths.common_dir] {
            if !roots.iter().any(|r| dir.starts_with(r)) {
                roots.push(dir);
            }
        }
        for root in roots {
            watcher.watch(root, RecursiveMode::Recursive)?;
        }

        thread::Builder::new()
            .name("amonhen-watch".into())
            .spawn(move || {
                while let Ok(first) = rx.recv() {
                    let mut changes = first;
                    loop {
                        match rx.recv_timeout(SETTLE) {
                            Ok(more) => changes |= more,
                            Err(RecvTimeoutError::Timeout) => break,
                            Err(RecvTimeoutError::Disconnected) => return,
                        }
                    }
                    on_change(changes);
                }
            })?;
        Ok(Self { _watcher: watcher })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    fn paths(root: &Path) -> RepoPaths {
        RepoPaths {
            git_dir: root.join(".git"),
            common_dir: root.join(".git"),
            workdir: Some(root.to_owned()),
        }
    }

    #[test]
    fn ignores_reads() {
        use notify::EventKind;
        use notify::event::{AccessKind, AccessMode, CreateKind, ModifyKind};
        assert!(is_read_only(&EventKind::Access(AccessKind::Open(
            AccessMode::Read
        ))));
        assert!(is_read_only(&EventKind::Access(AccessKind::Close(
            AccessMode::Write
        ))));
        assert!(!is_read_only(&EventKind::Modify(ModifyKind::Any)));
        assert!(!is_read_only(&EventKind::Create(CreateKind::File)));
    }

    #[test]
    fn classifies_paths() {
        let p = paths(Path::new("/r"));
        let c = |s: &str| classify(&p, Path::new(s));
        assert_eq!(c("/r/.git/HEAD"), Changes::REFS);
        assert_eq!(c("/r/.git/refs/heads/main"), Changes::REFS);
        assert_eq!(c("/r/.git/packed-refs"), Changes::REFS);
        assert_eq!(c("/r/.git/logs/refs/stash"), Changes::REFS);
        assert_eq!(c("/r/.git/logs/HEAD"), Changes::default());
        assert_eq!(c("/r/.git/index"), Changes::INDEX);
        assert_eq!(c("/r/.git/config"), Changes::CONFIG);
        assert_eq!(c("/r/.git/objects/ab/cdef"), Changes::default());
        assert_eq!(c("/r/src/main.rs"), Changes::WORKTREE);
        assert_eq!(c("/elsewhere/x"), Changes::default());
    }

    #[test]
    fn reports_a_burst_of_changes_once() {
        let tmp = tempfile::tempdir().unwrap();
        // 正規化していないパス（macOS の一時ディレクトリは /var → /private/var）を渡しても届くこと。
        let root = tmp.path().to_owned();
        std::fs::create_dir_all(root.join(".git/refs/heads")).unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let _w = Watcher::new(paths(&root), move |c| sink.lock().unwrap().push(c)).unwrap();
        // FSEvents は監視を始めてから届き始めるまで少しかかる。
        thread::sleep(Duration::from_millis(300));

        std::fs::write(root.join(".git/refs/heads/main"), "x").unwrap();
        std::fs::write(root.join("file.txt"), "y").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            let all = seen
                .lock()
                .unwrap()
                .iter()
                .fold(Changes::default(), |a, &b| a | b);
            if all.contains(Changes::REFS | Changes::WORKTREE) {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        panic!("変化が届かない: {:?}", seen.lock().unwrap());
    }
}
