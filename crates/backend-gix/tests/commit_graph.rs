#![allow(clippy::disallowed_methods)] // テストの git などはそのまま起動する
use std::path::Path;
use std::process::Command;

use amonhen_backend_gix::GixBackend;
use amonhen_model::GitBackend;

fn git(dir: &Path, args: &[&str]) -> String {
    amonhen_model::testing::isolate_from_user_settings();
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
    let out = String::from_utf8(out.stdout).unwrap().trim().to_owned();
    // テスト用のリポジトリは、CI の環境（利用者名がない・Windows の autocrlf）や、利用者の global の設定（コミットの
    // 署名。テストするコードの git は global を読むので、並べて走らせると gpg が失敗することがある）に左右されないようにする。
    if args.first() == Some(&"init") {
        for (key, value) in [
            ("user.name", "a"),
            ("user.email", "a@example.com"),
            ("core.autocrlf", "false"),
            ("commit.gpgsign", "false"),
            ("tag.gpgsign", "false"),
        ] {
            git(dir, &["config", key, value]);
        }
    }
    out
}

fn commit(dir: &Path, message: &str) {
    git(dir, &["commit", "--allow-empty", "-q", "-m", message]);
}

#[test]
fn reads_branches_and_merges_in_topological_order() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-q"]);
    git(dir, &["symbolic-ref", "HEAD", "refs/heads/main"]);
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
fn tips_are_head_branches_remotes_and_tags_only() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-q"]);
    git(dir, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    commit(dir, "root");
    let root = git(dir, &["rev-parse", "HEAD"]);
    git(dir, &["tag", "v1"]);
    commit(dir, "main");
    let main = git(dir, &["rev-parse", "HEAD"]);
    git(dir, &["update-ref", "refs/remotes/origin/main", &root]);
    // notes・stash・ブランチでない ref（git maintenance の prefetch など）のコミットは History に出さない。
    git(dir, &["notes", "add", "-m", "note"]);
    std::fs::write(dir.join("f.txt"), "f").unwrap();
    git(dir, &["add", "f.txt"]);
    git(dir, &["stash", "-q"]);
    let prefetched = git(dir, &["commit-tree", "-m", "prefetched", "HEAD^{tree}"]);
    git(
        dir,
        &["update-ref", "refs/prefetch/remotes/origin/x", &prefetched],
    );

    let backend = GixBackend::open(dir).unwrap();
    let tips: Vec<String> = backend
        .tips()
        .unwrap()
        .iter()
        .map(|t| t.to_string())
        .collect();
    // HEAD（main）・タグ（v1）とリモート（どちらも root）。
    assert_eq!(tips, [main, root]);
    let graph = backend.commit_graph(&backend.tips().unwrap()).unwrap();
    assert_eq!(graph.len(), 2);
}

#[test]
fn unborn_head_has_no_commit() {
    let tmp = tempfile::tempdir().unwrap();
    git(tmp.path(), &["init", "-q"]);
    git(tmp.path(), &["symbolic-ref", "HEAD", "refs/heads/main"]);
    let backend = GixBackend::open(tmp.path()).unwrap();
    assert_eq!(backend.head().unwrap(), None);
    assert!(backend.tips().unwrap().is_empty());
}

#[test]
fn reads_subject_and_author_without_copying() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-q"]);
    git(dir, &["symbolic-ref", "HEAD", "refs/heads/main"]);
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
    git(dir, &["init", "-q"]);
    git(dir, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    commit(dir, "first");
    let backend = GixBackend::open(dir).unwrap();
    assert!(!backend.has_commit_graph().unwrap());
    backend.write_commit_graph(false).unwrap();
    assert!(GixBackend::open(dir).unwrap().has_commit_graph().unwrap());
}

#[test]
fn adds_new_commits_to_the_commit_graph() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-q"]);
    git(dir, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    commit(dir, "first");
    let backend = GixBackend::open(dir).unwrap();
    backend.write_commit_graph(false).unwrap();
    commit(dir, "second");
    commit(dir, "third");
    let in_graph = |rev: &str| {
        let id = gix::ObjectId::from_hex(git(dir, &["rev-parse", rev]).as_bytes()).unwrap();
        let graph = gix::open(dir).unwrap().commit_graph().unwrap();
        graph.lookup(id).is_some()
    };
    assert!(in_graph("HEAD~2"));
    assert!(!in_graph("HEAD"));
    backend.write_commit_graph(true).unwrap();
    assert!(in_graph("HEAD~2"));
    assert!(in_graph("HEAD~1"));
    assert!(in_graph("HEAD"));
}

#[test]
fn lists_branches_remotes_tags_stashes_and_submodules() {
    use amonhen_model::RefKind;

    let tmp = tempfile::tempdir().unwrap();
    let sub = tmp.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    git(&sub, &["init", "-q"]);
    git(&sub, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    commit(&sub, "sub");

    let dir = tmp.path().join("repo");
    std::fs::create_dir(&dir).unwrap();
    let dir = dir.as_path();
    git(dir, &["init", "-q"]);
    git(dir, &["symbolic-ref", "HEAD", "refs/heads/main"]);
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
    // topic の上流は消えたリモートブランチ。
    git(dir, &["config", "branch.topic.remote", "origin"]);
    git(dir, &["config", "branch.topic.merge", "refs/heads/gone"]);

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
            (RefKind::Local, "topic", Some("origin/gone"))
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

    // main は origin/main の後に「add submodule」を 1 つコミットしている。
    assert_eq!(
        (
            refs.entries[0].ahead,
            refs.entries[0].behind,
            refs.entries[0].gone
        ),
        (1, 0, false)
    );
    assert!(refs.entries[1].gone);
    assert!(refs.entries[3].target.is_none(), "ツリーを指すタグ");
    assert!(refs.entries[6].target.is_some(), "サブモジュールのコミット");
}

#[test]
fn lists_changed_files_with_renames_and_reads_blobs() {
    use amonhen_model::ChangeKind;

    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-q"]);
    git(dir, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    let long = "line\n".repeat(20);
    std::fs::write(dir.join("keep.txt"), "a\n").unwrap();
    std::fs::write(dir.join("gone.txt"), "x\n").unwrap();
    std::fs::write(dir.join("old-name.txt"), &long).unwrap();
    git(dir, &["add", "."]);
    commit(dir, "first");

    let backend = GixBackend::open(dir).unwrap();
    let first = backend.head().unwrap().unwrap();
    let root = backend.commit_changes(&first).unwrap();
    assert_eq!(root.len(), 3);
    assert!(root.iter().all(|c| c.kind == ChangeKind::Added));

    std::fs::write(dir.join("keep.txt"), "a\nb\n").unwrap();
    std::fs::write(dir.join("new.txt"), "n\n").unwrap();
    std::fs::remove_file(dir.join("gone.txt")).unwrap();
    std::fs::rename(dir.join("old-name.txt"), dir.join("sub/new-name.txt")).unwrap_or_else(|_| {
        std::fs::create_dir(dir.join("sub")).unwrap();
        std::fs::rename(dir.join("old-name.txt"), dir.join("sub/new-name.txt")).unwrap();
    });
    git(dir, &["add", "-A"]);
    commit(dir, "second");

    let second = GixBackend::open(dir).unwrap().head().unwrap().unwrap();
    let changes = backend.commit_changes(&second).unwrap();
    let summary: Vec<(ChangeKind, &str, Option<&str>)> = changes
        .iter()
        .map(|c| (c.kind, c.path.as_str(), c.old_path.as_deref()))
        .collect();
    assert_eq!(
        summary,
        [
            (ChangeKind::Deleted, "gone.txt", None),
            (ChangeKind::Modified, "keep.txt", None),
            (ChangeKind::Added, "new.txt", None),
            (
                ChangeKind::Renamed,
                "sub/new-name.txt",
                Some("old-name.txt")
            ),
        ]
    );
    let keep = &changes[1];
    assert_eq!(
        &**backend.blob(keep.old_id.as_ref().unwrap()).unwrap(),
        b"a\n"
    );
    assert_eq!(
        &**backend.blob(keep.new_id.as_ref().unwrap()).unwrap(),
        b"a\nb\n"
    );
}

#[test]
fn shows_stash_contents_including_untracked_files_but_keeps_them_out_of_history() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-q"]);
    git(dir, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    std::fs::write(dir.join("a.txt"), "1\n").unwrap();
    git(dir, &["add", "."]);
    commit(dir, "first");
    std::fs::write(dir.join("a.txt"), "2\n").unwrap();
    std::fs::write(dir.join("new.txt"), "new\n").unwrap();
    git(
        dir,
        &["stash", "push", "-q", "--include-untracked", "-m", "wip"],
    );

    let backend = GixBackend::open(dir).unwrap();
    let tips = backend.tips().unwrap();
    assert_eq!(tips.len(), 1, "stash は History の起点にしない");
    let stash = backend
        .refs()
        .unwrap()
        .entries
        .iter()
        .find(|e| e.name == "stash@{0}")
        .unwrap()
        .target
        .unwrap();
    let paths: Vec<String> = backend
        .stash_changes(&stash)
        .unwrap()
        .into_iter()
        .map(|c| c.path)
        .collect();
    assert_eq!(paths, ["a.txt", "new.txt"]);
}

#[test]
fn stops_at_the_boundary_of_a_shallow_clone() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).unwrap();
    git(&src, &["init", "-q"]);
    git(&src, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    for (file, message) in [("a.txt", "root"), ("b.txt", "second"), ("c.txt", "third")] {
        std::fs::write(src.join(file), message).unwrap();
        git(&src, &["add", file]);
        git(&src, &["commit", "-q", "-m", message]);
    }
    let second = git(&src, &["rev-parse", "HEAD^"]);
    let third = git(&src, &["rev-parse", "HEAD"]);
    // --depth はパスの clone では効かないので file:// にする（Windows は file:///C:/…）。
    let path = src.to_str().unwrap().replace('\\', "/");
    let url = if path.starts_with('/') {
        format!("file://{path}")
    } else {
        format!("file:///{path}")
    };
    git(
        tmp.path(),
        &["clone", "-q", "--depth", "2", &url, "shallow"],
    );
    let dir = tmp.path().join("shallow");

    // 境界のコミット（second）の親（root）は手元にない。git と同じく親のないコミットとして扱う。
    let backend = GixBackend::open(&dir).unwrap();
    let mut commits = backend
        .read_commits(&backend.tips().unwrap(), &|_| false)
        .unwrap();
    commits.sort_by_key(|c| c.id.to_string() != third);
    let ids: Vec<String> = commits.iter().map(|c| c.id.to_string()).collect();
    assert_eq!(ids, [third.clone(), second.clone()]);
    assert_eq!(commits[0].parents.len(), 1);
    assert!(commits[1].parents.is_empty());
    assert_eq!(
        backend
            .commit_graph(&backend.tips().unwrap())
            .unwrap()
            .len(),
        2
    );

    // 境界のコミットの差分は、そのときのファイルをすべて足したもの。
    let mut paths: Vec<String> = backend
        .commit_changes(&amonhen_model::Oid::from_hex(&second).unwrap())
        .unwrap()
        .into_iter()
        .map(|c| c.path)
        .collect();
    paths.sort();
    assert_eq!(paths, ["a.txt", "b.txt"]);
    assert_eq!(
        backend.shallow().unwrap(),
        [amonhen_model::Oid::from_hex(&second).unwrap()]
    );

    // 根まで取ると、git は根のコミットを境界に残す。取得していない親はないので、境界に数えない。
    git(&dir, &["fetch", "-q", "--deepen", "1"]);
    assert!(dir.join(".git/shallow").exists());
    assert!(
        GixBackend::open(&dir)
            .unwrap()
            .shallow()
            .unwrap()
            .is_empty()
    );
}
