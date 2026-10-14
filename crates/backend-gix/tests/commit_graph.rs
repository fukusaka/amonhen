use std::path::Path;
use std::process::Command;

use amonhen_backend_gix::GixBackend;
use amonhen_model::GitBackend;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "a")
        .env("GIT_AUTHOR_EMAIL", "a@example.com")
        .env("GIT_COMMITTER_NAME", "a")
        .env("GIT_COMMITTER_EMAIL", "a@example.com")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn commit(dir: &Path, message: &str) {
    git(dir, &["commit", "--allow-empty", "-q", "-m", message]);
}

#[test]
fn reads_branches_and_merges_in_topological_order() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-q", "-b", "main"]);
    commit(dir, "root");
    git(dir, &["switch", "-q", "-c", "topic"]);
    commit(dir, "topic 1");
    commit(dir, "topic 2");
    git(dir, &["switch", "-q", "main"]);
    commit(dir, "main 1");
    git(
        dir,
        &["merge", "-q", "--no-ff", "-m", "merge topic", "topic"],
    );
    git(dir, &["switch", "-q", "-c", "side", "HEAD~1"]);
    commit(dir, "side 1");
    git(
        dir,
        &["tag", "-a", "-m", "tree tag", "tree-tag", "HEAD^{tree}"],
    );

    let backend = GixBackend::open(dir).unwrap();
    let tips = backend.tips().unwrap();
    // HEAD（side）・main・topic。ツリーを指すタグは除く。
    assert_eq!(tips.len(), 3);

    let side = git(dir, &["rev-parse", "HEAD"]);
    assert_eq!(backend.head().unwrap().unwrap().to_string(), side);

    let graph = backend.commit_graph(&tips).unwrap();
    assert_eq!(graph.len(), 6);
    for row in 0..graph.len() {
        for &parent in graph.parents(row) {
            assert!(parent as usize > row, "親は子より後ろの行に来る");
        }
    }

    let merge = git(dir, &["rev-parse", "main"]);
    let merge_row = graph
        .ids()
        .iter()
        .position(|id| id.to_string() == merge)
        .unwrap();
    assert_eq!(graph.parents(merge_row).len(), 2);
}

#[test]
fn unborn_head_has_no_commit() {
    let tmp = tempfile::tempdir().unwrap();
    git(tmp.path(), &["init", "-q", "-b", "main"]);
    let backend = GixBackend::open(tmp.path()).unwrap();
    assert_eq!(backend.head().unwrap(), None);
    assert!(backend.tips().unwrap().is_empty());
}

#[test]
fn reads_subject_and_author_without_copying() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-q", "-b", "main"]);
    git(
        dir,
        &[
            "commit",
            "--allow-empty",
            "-q",
            "-m",
            "  日本語の件名  \n\n本文",
        ],
    );
    commit(dir, "second");

    let backend = GixBackend::open(dir).unwrap();
    let tips = backend.tips().unwrap();
    let graph = backend.commit_graph(&tips).unwrap();
    let texts = backend.commit_texts(graph.ids()).unwrap();
    assert_eq!(texts[0].subject(), b"second");
    assert_eq!(texts[1].subject(), "日本語の件名".as_bytes());
    assert_eq!(texts[1].author_name(), b"a");
    assert!(texts[1].author_time > 0);
}
