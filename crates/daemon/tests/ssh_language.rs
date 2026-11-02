//! アプリが共有の daemon を通さずに SSH 先を開いたとき（同じプロセスの `RemoteBackend`）、SSH 先のエラーの文は
//! アプリの言語（`i18n::set_default`）で来る。プロセスの既定の言語を変えるので、ほかの試験と別のファイルに置く。
#![allow(clippy::disallowed_methods)] // テストの git などはそのまま起動する
#![cfg(unix)]

use std::path::Path;
use std::process::Command;

use amonhen_model::GitBackend;
use amonhen_model::i18n::{self, Lang};
use amonhen_remote::{RemoteBackend, ssh};

fn git(dir: &Path, args: &[&str]) {
    amonhen_model::testing::isolate_from_user_settings();
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

/// 偽の ssh（接続先を読み飛ばし、残りを SSH 先のログインシェルの代わりの sh で動かす）。
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
exec env -i HOME='{0}' SHELL=/bin/sh PATH='{0}/bin:/usr/bin:/bin' /bin/sh -c "$*"
"#,
            home.display()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

#[test]
fn errors_from_an_ssh_host_opened_without_the_daemon_follow_the_app_language() {
    amonhen_model::testing::isolate_from_user_settings();
    let tmp = tempfile::tempdir().unwrap();
    let repo_dir = tmp.path().join("repo");
    std::fs::create_dir(&repo_dir).unwrap();
    git(&repo_dir, &["init", "-q"]);
    git(&repo_dir, &["commit", "-q", "--allow-empty", "-m", "first"]);
    let home = tmp.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let ssh_program = fake_ssh(tmp.path(), &home);
    let bundled = tmp.path().join("engines");
    std::fs::create_dir(&bundled).unwrap();
    std::fs::copy(
        env!("CARGO_BIN_EXE_amonhend"),
        bundled.join(format!("amonhen-{}", ssh::local_platform())),
    )
    .unwrap();
    let daemons = ssh::Daemons {
        dir: bundled,
        local: None,
        remote_engine: None,
        push: None,
    };
    let target = ssh::Target::parse(&format!("ssh://dev{}", repo_dir.display())).unwrap();
    // アプリは起動したときに `amonhen_set_language` で既定の言語を決め、そのまま SSH 先につなぐ。
    let open = |lang: Lang| {
        i18n::set_default(lang);
        let conn = ssh::connect(
            ssh_program.to_str().unwrap(),
            &target,
            &daemons,
            &Default::default(),
        )
        .unwrap();
        RemoteBackend::connect(conn).unwrap()
    };
    let resolve = |backend: &RemoteBackend| backend.operations().unwrap().resolve("nosuch");
    let japanese = resolve(&open(Lang::Ja)).unwrap_err().to_string();
    let english = resolve(&open(Lang::En)).unwrap_err().to_string();
    assert_eq!(japanese, "nosuch が見つからない");
    assert_eq!(english, "nosuch not found");
}
