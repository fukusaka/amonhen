//! 手元のリポジトリを、同じプロセスの daemon（パイプでつないだ `Server`）越しに使い、直接使った場合と
//! 同じ結果になることを確かめる。
#![allow(clippy::disallowed_methods)] // テストの git などはそのまま起動する

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use amonhen_backend_gix::GixBackend;
use amonhen_core::Repository;
use amonhen_model::GitBackend;
use amonhen_projection::FrontendProfile;
use amonhen_remote::{Connection, RemoteBackend, Server};

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

fn init(dir: &Path) {
    git(dir, &["init", "-q"]);
    git(dir, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    for (k, v) in [
        ("user.name", "a"),
        ("user.email", "a@example.com"),
        ("core.autocrlf", "false"),
    ] {
        git(dir, &["config", k, v]);
    }
}

fn commit(dir: &Path, name: &str, content: &str, message: &str) {
    std::fs::write(dir.join(name), content).unwrap();
    git(dir, &["add", name]);
    git(dir, &["commit", "-q", "-m", message]);
}

/// パイプでつないだ daemon（`Server`）越しのリポジトリ。
fn remote(dir: &Path) -> Repository {
    let (client_read, server_write) = std::io::pipe().unwrap();
    let (server_read, client_write) = std::io::pipe().unwrap();
    let backend: Arc<dyn GitBackend> = Arc::new(GixBackend::open(dir).unwrap());
    std::thread::spawn(move || {
        let server = Server::new(backend, server_write);
        let _ = server.run(server_read);
    });
    let conn = Connection::new(client_read, client_write);
    Repository::new(RemoteBackend::connect(conn).unwrap())
}

#[test]
fn a_remote_repository_matches_the_local_one() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    init(dir);
    commit(dir, "a.txt", "1\n", "first");
    git(dir, &["switch", "-q", "-c", "topic"]);
    commit(dir, "b.txt", "b\n", "topic work");
    git(dir, &["switch", "-q", "main"]);
    commit(dir, "a.txt", "1\n2\n", "second");

    let local = Repository::new(GixBackend::open(dir).unwrap());
    let remote = remote(dir);

    // 初めての読み込み（daemon が並べた History を受け取る）。
    let (l, r) = (
        local.load_history().unwrap(),
        remote.load_history().unwrap(),
    );
    assert_eq!(l.graph, r.graph);
    assert_eq!(l.layout.nodes(), r.layout.nodes());
    assert_eq!(r.graph.len(), 3);

    // 新しいコミットだけを読み足す（daemon は rev-list で前回からの差分を返す）。
    commit(dir, "c.txt", "c\n", "third");
    git(dir, &["merge", "-q", "--no-edit", "topic"]);
    let (l, r) = (
        local.load_history().unwrap(),
        remote.load_history().unwrap(),
    );
    assert_eq!(r.graph.len(), 5);
    assert_eq!(l.graph, r.graph);
    assert_eq!(l.layout.edges(), r.layout.edges());

    assert_eq!(
        local.load_refs().unwrap().refs,
        remote.load_refs().unwrap().refs
    );
    std::fs::write(dir.join("a.txt"), "changed\n").unwrap();
    assert_eq!(
        local.load_status().unwrap().status,
        remote.load_status().unwrap().status
    );

    // コミットの詳細と差分（blob を受け取って手元で差分を取る）。
    let head = r.graph.ids()[1];
    let changes = remote.commit_changes(&head).unwrap();
    assert_eq!(changes, local.commit_changes(&head).unwrap());
    let diff = remote
        .file_diff(&changes[0], &FrontendProfile::MACOS)
        .unwrap();
    assert_eq!(
        diff.rows.len(),
        local
            .file_diff(&changes[0], &FrontendProfile::MACOS)
            .unwrap()
            .rows
            .len()
    );
    // 署名・notes・行数は daemon の git に、子・含むブランチは手元のグラフで求める。
    remote.load_refs().unwrap();
    local.load_refs().unwrap();
    let extras = format!(r#"{{"op":"commitExtras","oid":"{head}"}}"#);
    assert_eq!(remote.run_json(&extras), local.run_json(&extras));
    assert!(remote.run_json(&extras).contains("\"stats\":[{"));
    let verify = format!(r#"{{"op":"verifySignatures","oids":["{head}"]}}"#);
    assert_eq!(remote.run_json(&verify), local.run_json(&verify));
    let chunk = remote
        .history_chunk(&r, 0, 5, &FrontendProfile::MACOS)
        .unwrap();
    assert_eq!(chunk.start_row, 0);

    // 検索: 件名は手元で走査し、パスは daemon の git に問い合わせる。
    assert_eq!(
        remote.search_history(&r, "topic").unwrap(),
        local.search_history(&l, "topic").unwrap()
    );
    assert_eq!(
        remote.search_history(&r, "path:b.txt").unwrap(),
        local.search_history(&l, "path:b.txt").unwrap()
    );
}

#[test]
fn commands_and_undo_run_against_the_remote_repository() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    init(dir);
    commit(dir, "a.txt", "1\n", "first");
    let remote = remote(dir);
    remote.load_history().unwrap();

    let run =
        |json: &str| -> serde_json::Value { serde_json::from_str(&remote.run_json(json)).unwrap() };
    assert_eq!(
        run(r#"{"op":"createBranch","name":"feature","start":"main","checkout":false}"#)["ok"],
        true
    );
    assert!(
        dir.join(".git/refs/heads/feature").exists(),
        "daemon の側のリポジトリに作られる"
    );

    // ファイルの変更の破棄と Undo（作業ツリーのファイルを daemon 越しに読み書きする）。
    std::fs::write(dir.join("a.txt"), "edited\n").unwrap();
    assert_eq!(run(r#"{"op":"discard","paths":["a.txt"]}"#)["ok"], true);
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "1\n");
    assert_eq!(run(r#"{"op":"undo"}"#)["ok"], true);
    assert_eq!(
        std::fs::read_to_string(dir.join("a.txt")).unwrap(),
        "edited\n"
    );
    assert!(
        dir.join(".git/amonhen/undo.json").exists(),
        "Undo の記録も daemon の側に残る"
    );
}
