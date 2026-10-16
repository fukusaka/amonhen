//! `amonhen` を実際に起動して確かめる。

use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git").current_dir(dir).args(args).status().unwrap().success();
    assert!(ok, "git {args:?}");
}

fn commit(dir: &Path, file: &str, message: &str) {
    std::fs::write(dir.join(file), message).unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", message]);
}

fn amonhen(home: &Path, args: &[&str]) -> (String, i32) {
    // キャッシュは試験の中に置く（手元のアプリのキャッシュに混ぜない）。
    let out = Command::new(env!("CARGO_BIN_EXE_amonhen")).env("HOME", home).args(args).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    (text, out.status.code().unwrap_or(-1))
}

/// main に、topic を取り込んだ merge がある。
fn repository() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["config", "user.name", "a"]);
    git(dir, &["config", "user.email", "a@example.com"]);
    commit(dir, "a.txt", "first");
    git(dir, &["switch", "-q", "-c", "topic"]);
    commit(dir, "t.txt", "topic work");
    git(dir, &["switch", "-q", "main"]);
    commit(dir, "m.txt", "main work");
    git(dir, &["merge", "-q", "--no-ff", "-m", "merge topic", "topic"]);
    tmp
}

#[test]
fn log_draws_the_branch_and_the_merge() {
    let repo = repository();
    let home = tempfile::tempdir().unwrap();
    let path = repo.path().to_str().unwrap();
    let (out, code) = amonhen(home.path(), &["log", "-C", path, "--color=never"]);
    assert_eq!(code, 0, "{out}");
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines[0].starts_with('◉') || lines[0].starts_with('◆'), "{out}");
    assert!(lines[0].contains("(HEAD → main)") && lines[0].contains("merge topic"), "{out}");
    assert!(out.contains("├─╮"), "枝分かれのつなぎ:\n{out}");
    assert!(out.contains("╯"), "合流のつなぎ:\n{out}");
    assert!(out.contains("topic work") && out.contains("first"), "{out}");

    // 折りたたむと、topic の側は merge の数になる。
    let (out, _) = amonhen(home.path(), &["log", "-C", path, "--fold", "--color=never"]);
    assert!(!out.contains("topic work"), "{out}");
    assert!(out.contains("1 件を折りたたみ"), "{out}");

    // 検索はグラフなしで、合うものだけ。
    let (out, _) = amonhen(home.path(), &["log", "-C", path, "--color=never", "subject:topic"]);
    assert_eq!(out.lines().count(), 2, "{out}");
}

#[test]
fn undo_and_merge_file() {
    let repo = repository();
    let home = tempfile::tempdir().unwrap();
    let path = repo.path().to_str().unwrap();
    let (out, code) = amonhen(home.path(), &["undo", "-C", path, "--peek"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("戻せる操作はありません"), "{out}");

    // 片側だけが変えたコンフリクトは、間違えようのない規則で解ける。両側が変えたものは残る。
    let file = repo.path().join("conflict.txt");
    std::fs::write(
        &file,
        "<<<<<<< ours\nsame\n||||||| base\nold\n=======\nsame\n>>>>>>> theirs\n\
         <<<<<<< ours\nx = 1\n||||||| base\nx = 0\n=======\nx = 2\n>>>>>>> theirs\n",
    )
    .unwrap();
    let file = file.to_str().unwrap();
    let (out, code) = amonhen(home.path(), &["merge-file", file]);
    assert_eq!(code, 1, "残りがあれば 1: {out}");
    assert!(out.contains("2 個のうち 1 個を解いた、残り 1 個"), "{out}");
    assert!(std::fs::read_to_string(file).unwrap().starts_with("same\n"));

    let (out, code) = amonhen(home.path(), &["log", "-C", "/"]);
    assert_eq!(code, 2, "{out}");
}
