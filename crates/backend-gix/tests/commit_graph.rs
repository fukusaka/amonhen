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

#[test]
fn writes_a_commit_graph_on_request() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-q", "-b", "main"]);
    commit(dir, "first");
    let backend = GixBackend::open(dir).unwrap();
    assert!(!backend.has_commit_graph().unwrap());
    backend.write_commit_graph().unwrap();
    assert!(GixBackend::open(dir).unwrap().has_commit_graph().unwrap());
}

#[test]
fn lists_branches_remotes_tags_stashes_and_submodules() {
    use amonhen_model::RefKind;

    let tmp = tempfile::tempdir().unwrap();
    let sub = tmp.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    git(&sub, &["init", "-q", "-b", "main"]);
    commit(&sub, "sub");

    let dir = tmp.path().join("repo");
    std::fs::create_dir(&dir).unwrap();
    let dir = dir.as_path();
    git(dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("a.txt"), "a").unwrap();
    git(dir, &["add", "a.txt"]);
    commit(dir, "first");
    git(dir, &["branch", "topic"]);
    git(dir, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    git(
        dir,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    git(
        dir,
        &["config", "remote.origin.url", "https://example.com/r.git"],
    );
    git(
        dir,
        &[
            "config",
            "remote.origin.fetch",
            "+refs/heads/*:refs/remotes/origin/*",
        ],
    );
    git(dir, &["config", "branch.main.remote", "origin"]);
    git(dir, &["config", "branch.main.merge", "refs/heads/main"]);
    git(dir, &["tag", "v1"]);
    git(dir, &["tag", "-a", "-m", "tree", "tree-tag", "HEAD^{tree}"]);
    git(
        dir,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            sub.to_str().unwrap(),
            "libs/sub",
        ],
    );
    commit(dir, "add submodule");
    std::fs::write(dir.join("a.txt"), "changed").unwrap();
    git(dir, &["stash", "push", "-q", "-m", "wip"]);

    let refs = GixBackend::open(dir).unwrap().refs().unwrap();
    assert_eq!(refs.head_branch.as_deref(), Some("main"));
    let names: Vec<(RefKind, &str, Option<&str>)> = refs
        .entries
        .iter()
        .map(|e| (e.kind, e.name.as_str(), e.detail.as_deref()))
        .collect();
    assert_eq!(
        names[..2],
        [
            (RefKind::Local, "main", Some("origin/main")),
            (RefKind::Local, "topic", None)
        ]
    );
    assert_eq!(names[2], (RefKind::Remote, "origin/main", None));
    assert_eq!(
        names[3..5],
        [(RefKind::Tag, "tree-tag", None), (RefKind::Tag, "v1", None)]
    );
    assert_eq!(names[5].0, RefKind::Stash);
    assert_eq!(names[5].1, "stash@{0}");
    assert!(names[5].2.unwrap().contains("wip"));
    assert_eq!(names[6], (RefKind::Submodule, "libs/sub", None));
    assert_eq!(names.len(), 7);

    assert!(refs.entries[3].target.is_none(), "ツリーを指すタグ");
    assert!(refs.entries[6].target.is_some(), "サブモジュールのコミット");
}
