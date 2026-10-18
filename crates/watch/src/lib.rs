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

/// ほかの worktree の変化を、worktree ごとの番号で数える（変わったかどうかだけを覚え、何が変わったかは見ない）。
/// worktree の変更の数を、変わったものだけ数え直すために使う。
///
/// - `<common dir>/worktrees/<名前>/` の HEAD・index（と、メインの作業ツリーの `<common dir>` の HEAD・index）を
///   見張る。ファイルは少ないので軽く、stage・コミット・checkout はどの worktree でも分かる。
/// - 作業ツリーのファイルは見張らない。Linux の inotify はフォルダごとに 1 つ使い（カーネルのメモリを 1 つ約 1 KB）、
///   大きな作業ツリー（minix は 1 万 7 千フォルダ）を何十も見張ると上限（`max_user_watches`）に届く。ファイルの
///   変化は、使う側がときどき数え直して拾う。
pub struct WorktreeWatch {
    state: std::sync::Arc<std::sync::Mutex<WorktreeState>>,
    _watcher: notify::RecommendedWatcher,
}

#[derive(Default)]
struct WorktreeState {
    common_dir: PathBuf,
    /// worktree（作業ツリーのパス）ごとの番号。変わるたびに増やす。
    generations: std::collections::HashMap<PathBuf, u64>,
}

impl WorktreeState {
    /// `<common dir>/worktrees/<名前>/…` か `<common dir>/{HEAD,index}` なら、その worktree の作業ツリー。
    fn worktree_of_git_file(&self, path: &Path) -> Option<PathBuf> {
        let rel = path.strip_prefix(&self.common_dir).ok()?;
        let mut parts = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned());
        match parts.next()?.as_str() {
            "HEAD" | "index" => self.common_dir.parent().map(Path::to_owned),
            "worktrees" => {
                let name = parts.next()?;
                if !matches!(parts.next().as_deref(), Some("HEAD" | "index")) {
                    return None;
                }
                // `gitdir` に `<作業ツリー>/.git` が書いてある（相対パスなら、この名前のフォルダから）。
                let dir = self.common_dir.join("worktrees").join(name);
                let gitdir = std::fs::read_to_string(dir.join("gitdir")).ok()?;
                let gitdir = dir.join(gitdir.trim());
                let gitdir = gitdir.canonicalize().unwrap_or(gitdir);
                gitdir.parent().map(Path::to_owned)
            }
            _ => None,
        }
    }

    fn touch(&mut self, worktree: PathBuf) {
        *self.generations.entry(worktree).or_default() += 1;
    }
}

impl WorktreeWatch {
    /// `common_dir`（リポジトリの共有の .git）の worktree を見張る。
    pub fn new(common_dir: &Path) -> notify::Result<Self> {
        let common_dir = common_dir
            .canonicalize()
            .unwrap_or_else(|_| common_dir.to_owned());
        let state = std::sync::Arc::new(std::sync::Mutex::new(WorktreeState {
            common_dir: common_dir.clone(),
            ..Default::default()
        }));
        let events = state.clone();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                let Ok(event) = event else { return };
                if is_read_only(&event.kind) {
                    return;
                }
                let mut state = events.lock().unwrap();
                for path in &event.paths {
                    if let Some(worktree) = state.worktree_of_git_file(path) {
                        state.touch(worktree);
                    }
                }
            })?;
        watcher.watch(&common_dir, RecursiveMode::NonRecursive)?;
        let worktrees = common_dir.join("worktrees");
        if worktrees.is_dir() {
            watcher.watch(&worktrees, RecursiveMode::Recursive)?;
        }
        Ok(Self {
            state,
            _watcher: watcher,
        })
    }

    /// `path` の worktree の番号（変わるたびに増える）。
    pub fn generation(&self, path: &Path) -> u64 {
        self.state
            .lock()
            .unwrap()
            .generations
            .get(path)
            .copied()
            .unwrap_or(0)
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

    #[test]
    fn counts_changes_per_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        // git には普通のパスを渡し、番号は実パス（macOS の /private/var、Windows の \\?\C:\…）で引く。
        let root = tmp.path().to_owned();
        let key = |p: &Path| p.canonicalize().unwrap();
        let git = |dir: &Path, args: &[&str]| {
            let out = std::process::Command::new("git")
                .current_dir(dir)
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        let main = root.join("main");
        std::fs::create_dir(&main).unwrap();
        git(&main, &["init", "-q"]);
        std::fs::write(main.join("a"), "a").unwrap();
        git(&main, &["add", "."]);
        git(
            &main,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-qm",
                "a",
            ],
        );
        let (one, two) = (root.join("one"), root.join("two"));
        git(&main, &["worktree", "add", "-q", one.to_str().unwrap()]);
        git(&main, &["worktree", "add", "-q", two.to_str().unwrap()]);
        let watch = WorktreeWatch::new(&main.join(".git")).unwrap();
        let wait_for = |path: &Path, above: u64| {
            let started = Instant::now();
            while watch.generation(&key(path)) <= above {
                assert!(
                    started.elapsed() < Duration::from_secs(5),
                    "no change seen in {}",
                    path.display()
                );
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        // checkout は HEAD で分かる。
        let before = watch.generation(&key(&one));
        git(&one, &["switch", "-q", "-c", "topic"]);
        wait_for(&one, before);
        // stage は index で分かる。
        let before = watch.generation(&key(&two));
        std::fs::write(two.join("b"), "b").unwrap();
        git(&two, &["add", "b"]);
        wait_for(&two, before);
        // メインの作業ツリーの index。
        let before = watch.generation(&key(&main));
        git(&main, &["rm", "-q", "--cached", "a"]);
        wait_for(&main, before);
    }
}
