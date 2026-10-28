//! mailmap を git と同じ所から読み、変わったら読み直す。
#![allow(clippy::disallowed_methods)] // テストの git などはそのまま起動する

use std::path::Path;
use std::process::Command;

use amonhen_backend_gix::GixBackend;
use amonhen_model::GitBackend;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}");
}

fn mapped(backend: &GixBackend, name: &str, email: &str) -> (String, String) {
    backend.mailmap().unwrap().map_str(name, email)
}

#[test]
fn reads_the_worktree_mailmap_and_rereads_it_after_a_change() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-q"]);
    let backend = GixBackend::open(dir).unwrap();
    assert!(!backend.mailmap().unwrap().is_active(), "mailmap がない");

    std::fs::write(dir.join(".mailmap"), "Taro Yamada <old@example.com>\n").unwrap();
    assert_eq!(
        mapped(&backend, "T. Yamada", "old@example.com").0,
        "Taro Yamada"
    );

    std::fs::write(
        dir.join(".mailmap"),
        "Taro Yamada <taro@example.com> <old@example.com>\n",
    )
    .unwrap();
    assert_eq!(
        mapped(&backend, "T. Yamada", "old@example.com"),
        ("Taro Yamada".into(), "taro@example.com".into())
    );

    // log.mailmap が false なら置き換えない（設定を変えても開き直さずに分かる）。
    git(dir, &["config", "log.mailmap", "false"]);
    let mailmap = backend.mailmap().unwrap();
    assert!(!mailmap.enabled && !mailmap.is_active());
    assert_eq!(
        mapped(&backend, "T. Yamada", "old@example.com").0,
        "T. Yamada"
    );
    git(dir, &["config", "--unset", "log.mailmap"]);

    // mailmap.file も読む（作業ツリーの .mailmap の後）。
    let other = tmp.path().join("other-mailmap");
    std::fs::write(&other, "Robert <bob@example.com>\n").unwrap();
    git(dir, &["config", "mailmap.file", other.to_str().unwrap()]);
    assert_eq!(mapped(&backend, "Bob", "bob@example.com").0, "Robert");
    assert_eq!(
        mapped(&backend, "T. Yamada", "old@example.com").0,
        "Taro Yamada"
    );
    std::fs::write(&other, "Bobby <bob@example.com>\n").unwrap();
    assert_eq!(mapped(&backend, "Bob", "bob@example.com").0, "Bobby");
}

#[test]
fn a_bare_repository_reads_the_mailmap_of_head() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("work");
    std::fs::create_dir(&work).unwrap();
    git(&work, &["init", "-q"]);
    std::fs::write(work.join(".mailmap"), "Taro Yamada <old@example.com>\n").unwrap();
    git(&work, &["add", ".mailmap"]);
    git(
        &work,
        &[
            "-c",
            "user.name=a",
            "-c",
            "user.email=a@x",
            "commit",
            "-qm",
            "mailmap",
        ],
    );
    git(tmp.path(), &["clone", "-q", "--bare", "work", "bare.git"]);
    let backend = GixBackend::open(tmp.path().join("bare.git")).unwrap();
    assert_eq!(mapped(&backend, "T", "old@example.com").0, "Taro Yamada");
}
