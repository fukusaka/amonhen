//! partial clone（`--filter=blob:none`）で、手元にない blob を git に取ってこさせる。
#![allow(clippy::disallowed_methods)] // テストの git などはそのまま起動する

use std::path::Path;
use std::process::Command;

use amonhen_backend_gix::GixBackend;
use amonhen_model::{GitBackend, Oid};

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
    if args.first() == Some(&"init") {
        for (key, value) in [
            ("user.name", "a"),
            ("user.email", "a@example.com"),
            ("core.autocrlf", "false"),
            ("commit.gpgsign", "false"),
        ] {
            git(dir, &["config", key, value]);
        }
    }
    out
}

fn packs(dir: &Path) -> usize {
    std::fs::read_dir(dir.join(".git/objects/pack"))
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .is_ok_and(|e| e.path().extension().is_some_and(|x| x == "pack"))
        })
        .count()
}

/// 古いコミットにだけ中身があるファイルを 3 つ持つリポジトリを作り、`--filter=blob:none` で clone する。clone
/// した場所と、作ったコミット（古い順に 4 つ。最後はファイルを消したもの）を返す。
fn partial_clone(tmp: &Path) -> (std::path::PathBuf, Vec<String>) {
    let src = tmp.join("src");
    std::fs::create_dir(&src).unwrap();
    git(&src, &["init", "-q"]);
    git(&src, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    // 取り出したときに中身が要らないよう、古いコミットにだけあるファイルを作る。
    for (n, text) in ["one", "two", "three"].iter().enumerate() {
        for file in ["a.txt", "b.txt", "c.txt"] {
            std::fs::write(src.join(file), format!("{file} {text}\n")).unwrap();
        }
        git(&src, &["add", "."]);
        git(&src, &["commit", "-q", "-m", &format!("commit {n}")]);
    }
    git(&src, &["rm", "-q", "a.txt", "b.txt", "c.txt"]);
    git(&src, &["commit", "-q", "-m", "remove"]);
    git(&src, &["config", "uploadpack.allowFilter", "true"]);
    git(&src, &["config", "uploadpack.allowAnySHA1InWant", "true"]);
    let commits = (0..4)
        .rev()
        .map(|n| git(&src, &["rev-parse", &format!("HEAD~{n}")]))
        .collect();

    let path = src.to_str().unwrap().replace('\\', "/");
    let url = if path.starts_with('/') {
        format!("file://{path}")
    } else {
        format!("file:///{path}")
    };
    git(tmp, &["clone", "-q", "--filter=blob:none", &url, "partial"]);
    (tmp.join("partial"), commits)
}

#[test]
fn reads_blobs_missing_from_a_partial_clone() {
    let tmp = tempfile::tempdir().unwrap();
    let (dir, commits) = partial_clone(tmp.path());
    let [first, second, third] = [&commits[0], &commits[1], &commits[2]];
    let backend = GixBackend::open(&dir).unwrap();
    let oid = |hex: &str| Oid::from_hex(hex).unwrap();

    // 差分に要る blob を、1 回の fetch でまとめて取る。
    let changes = backend
        .changes_between(Some(&oid(first)), &oid(second))
        .unwrap();
    assert_eq!(changes.len(), 3);
    let ids: Vec<Oid> = changes
        .iter()
        .flat_map(|c| [c.old_id, c.new_id])
        .flatten()
        .collect();
    let before = packs(&dir);
    backend.prefetch_blobs(&ids).unwrap();
    assert_eq!(packs(&dir), before + 1);
    backend.prefetch_blobs(&ids).unwrap();
    assert_eq!(packs(&dir), before + 1, "揃っていれば取りに行かない");
    for c in &changes {
        let new = backend.blob(&c.new_id.unwrap()).unwrap();
        assert!(String::from_utf8_lossy(&new).ends_with(" two\n"));
    }
    assert!(!dir.join(".git/FETCH_HEAD").exists() || git_older_than_2_29());

    // 先に取っていなくても、1 つずつなら git に読ませる。
    let changes = backend
        .changes_between(Some(&oid(second)), &oid(third))
        .unwrap();
    let new = backend.blob(&changes[0].new_id.unwrap()).unwrap();
    assert_eq!(&**new, b"a.txt three\n");
}

#[test]
fn fetches_missing_contents_at_once_before_searching_them() {
    let tmp = tempfile::tempdir().unwrap();
    let (dir, commits) = partial_clone(tmp.path());
    let backend = GixBackend::open(&dir).unwrap();
    let tip = Oid::from_hex(&commits[3]).unwrap();
    let before = packs(&dir);
    // `log -G` は足りない blob を 1 つずつ取りに行く（6 個なら 6 回）。先にまとめて 1 回で取る。
    let found = backend
        .operations()
        .unwrap()
        .log_matching(&[tip], None, Some("b.txt two"), &|| false)
        .unwrap();
    let found: Vec<String> = found.iter().map(Oid::to_string).collect();
    // 足したコミットと、消したコミット（`-G` は消えた行も数える）。
    assert_eq!(found, [commits[2].clone(), commits[1].clone()]);
    assert_eq!(packs(&dir), before + 1);
}

#[test]
fn lists_submodules_of_a_partial_clone_not_checked_out() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).unwrap();
    git(&src, &["init", "-q"]);
    git(&src, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    std::fs::write(
        src.join(".gitmodules"),
        "[submodule \"lib\"]\n\tpath = lib\n\turl = ../lib\n",
    )
    .unwrap();
    git(&src, &["add", ".gitmodules"]);
    let commit = "1111111111111111111111111111111111111111";
    git(
        &src,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{commit},lib"),
        ],
    );
    git(&src, &["commit", "-q", "-m", "add a submodule"]);
    git(&src, &["config", "uploadpack.allowFilter", "true"]);
    git(&src, &["config", "uploadpack.allowAnySHA1InWant", "true"]);
    let path = src.to_str().unwrap().replace('\\', "/");
    let url = if path.starts_with('/') {
        format!("file://{path}")
    } else {
        format!("file:///{path}")
    };
    // 取り出さないので、.gitmodules の中身は手元にない。
    git(
        tmp.path(),
        &[
            "clone",
            "-q",
            "--no-checkout",
            "--filter=blob:none",
            &url,
            "partial",
        ],
    );
    let backend = GixBackend::open(tmp.path().join("partial")).unwrap();
    let refs = backend.refs().unwrap();
    let subs: Vec<_> = refs
        .entries
        .iter()
        .filter(|e| e.kind == amonhen_model::RefKind::Submodule)
        .map(|e| e.name.as_str())
        .collect();
    assert_eq!(subs, ["lib"]);
}

fn git_older_than_2_29() -> bool {
    amonhen_backend_cli::git_version().is_some_and(|(a, b, _)| (a, b) < (2, 29))
}
