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
fn reports_progress_and_the_reason_when_the_daemon_goes_away() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["commit", "-q", "--allow-empty", "-m", "first"]);

    // 2 秒たったら daemon を止める（ネットワークが切れた代わり）。
    let conn = Connection::spawn(
        "sh",
        &[
            "-c".to_owned(),
            r#""$0" --stdio "$1" <&0 & sleep 2; kill $!; echo 'gone' >&2"#.to_owned(),
            env!("CARGO_BIN_EXE_amonhend").to_owned(),
            dir.display().to_string(),
        ],
    )
    .unwrap();
    let repo = Repository::new(RemoteBackend::connect(conn).unwrap());
    assert_eq!(repo.load_history().unwrap().graph.len(), 1);
    let state = repo.connection_state().unwrap();
    assert!(state.received > 0 && state.closed.is_none());

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let reason = loop {
        if let Some(r) = repo.connection_state().unwrap().closed {
            break r;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "切れたことに気づかない"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(reason.contains("gone"), "{reason}");
    assert!(repo.load_status().is_err());
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

/// SSH の代わりに、送られたコマンドを手元で（偽の HOME で）実行するスクリプト。
fn fake_ssh(dir: &Path, home: &Path) -> std::path::PathBuf {
    let script = dir.join("fake-ssh");
    std::fs::write(
        &script,
        format!(
            r#"#!/bin/sh
while [ $# -gt 0 ]; do
    case "$1" in
        -T) shift ;;
        -o|-p) shift 2 ;;
        *) break ;;
    esac
done
shift
HOME='{}' exec sh -c "$*"
"#,
            home.display()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

#[cfg(unix)]
#[test]
fn sends_the_daemon_once_and_reuses_it() {
    use amonhen_remote::ssh;
    let tmp = tempfile::tempdir().unwrap();
    let repo_dir = tmp.path().join("repo");
    std::fs::create_dir(&repo_dir).unwrap();
    git(&repo_dir, &["init", "-q", "-b", "main"]);
    git(&repo_dir, &["commit", "-q", "--allow-empty", "-m", "first"]);
    let home = tmp.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let ssh_program = fake_ssh(tmp.path(), &home);
    // 送り込む daemon の置き場所。手元の OS 向けに、テストで作った amonhend を置く。
    let bundled = tmp.path().join("daemons");
    std::fs::create_dir(&bundled).unwrap();
    std::fs::copy(
        env!("CARGO_BIN_EXE_amonhend"),
        bundled.join(format!("amonhend-{}", ssh::local_platform())),
    )
    .unwrap();
    let daemons = ssh::Daemons {
        dir: bundled,
        local: None,
    };
    let target = ssh::Target::parse(&format!("ssh://dev{}", repo_dir.display())).unwrap();

    let conn = ssh::connect(ssh_program.to_str().unwrap(), &target, &daemons).unwrap();
    let repo = Repository::new(RemoteBackend::connect(conn).unwrap());
    assert_eq!(repo.load_history().unwrap().graph.len(), 1);
    let cache = home.join(".cache/amonhen");
    let sent: Vec<_> = std::fs::read_dir(&cache)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(sent.len(), 1, "daemon を 1 つ送った: {sent:?}");
    let first = std::fs::metadata(&sent[0]).unwrap().modified().unwrap();
    drop(repo);

    // 2 回目は送らずに、置いてある daemon を使う。
    let conn = ssh::connect(ssh_program.to_str().unwrap(), &target, &daemons).unwrap();
    let repo = Repository::new(RemoteBackend::connect(conn).unwrap());
    assert_eq!(repo.load_history().unwrap().graph.len(), 1);
    assert_eq!(
        std::fs::metadata(&sent[0]).unwrap().modified().unwrap(),
        first
    );
    assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 1);
}
