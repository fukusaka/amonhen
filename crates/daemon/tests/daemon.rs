//! `amonhend` を子プロセスとして起動してつなぐ（SSH の代わりに手元で起動するだけで、通信は同じ）。

use std::path::Path;
use std::process::Command;
use std::sync::mpsc;
use std::time::Duration;

use amonhen_core::Repository;
use amonhen_remote::{Connection, Notification, RemoteBackend};

fn git(dir: &Path, args: &[&str]) {
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
fn talks_to_the_daemon_process_and_hears_about_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["commit", "-q", "--allow-empty", "-m", "first"]);

    let conn = Connection::spawn(
        env!("CARGO_BIN_EXE_amonhend"),
        &["--stdio".to_owned(), dir.display().to_string()],
    )
    .unwrap();
    let (tx, rx) = mpsc::channel();
    conn.on_notification(move |n| {
        let _ = tx.send(n);
    });
    let repo = Repository::new(RemoteBackend::connect(conn).unwrap());
    assert_eq!(repo.load_history().unwrap().graph.len(), 1);

    // daemon の側でリポジトリが変わると知らせが来る。
    git(dir, &["commit", "-q", "--allow-empty", "-m", "second"]);
    let Notification::Changed(bits) = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("変化の知らせが来ない");
    assert_ne!(bits, 0);
    assert_eq!(repo.load_history().unwrap().graph.len(), 2);
}

#[test]
fn refuses_a_missing_repository() {
    let conn = Connection::spawn(
        env!("CARGO_BIN_EXE_amonhend"),
        &["--stdio".to_owned(), "/nonexistent/repo".to_owned()],
    )
    .unwrap();
    assert!(RemoteBackend::connect(conn).is_err());
}
