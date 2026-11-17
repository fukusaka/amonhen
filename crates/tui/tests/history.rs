//! History の画面を、試験用のバックエンドに描いて確かめる（端末は要らない）。

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use amonhen_front::{Front, InProcess};

/// コミットの時刻の基準（2026-01-01 00:00:00 UTC）。
const BASE: i64 = 1_767_225_600;
const DAY: i64 = 86_400;

fn git(dir: &Path, time: i64, args: &[&str]) {
    amonhen_model::testing::isolate_from_user_settings();
    // `git init -b` は git 2.28 から。サポートする最低の git でも通るよう、作ってから HEAD を向ける。
    let branch = match args {
        ["init", rest @ ..] => rest
            .iter()
            .position(|a| *a == "-b")
            .and_then(|i| rest.get(i + 1).copied()),
        _ => None,
    };
    let args: Vec<&str> = match branch {
        Some(branch) => args
            .iter()
            .copied()
            .filter(|a| *a != "-b" && *a != branch)
            .collect(),
        None => args.to_vec(),
    };
    let date = format!("{time} +0000");
    let out = amonhen_model::process::command("git")
        .args(["-c", "user.name=Ann", "-c", "user.email=ann@example.com"])
        .args(["-c", "commit.gpgsign=false"])
        .args(&args)
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_DATE", &date)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // 作った（か、clone した）リポジトリには、作者を決めておく。画面からのコミットは、ここの `-c` を通らない
    // （作者の設定のない機械では、コミットできない）。署名もしない。
    if let ["init" | "clone", rest @ ..] = args.as_slice() {
        let made = match rest.last() {
            Some(last) if !last.starts_with('-') && (args[0] == "clone" || rest.len() > 1) => {
                dir.join(last)
            }
            _ => dir.to_owned(),
        };
        if let Some(branch) = branch {
            let head = format!("refs/heads/{branch}");
            git(&made, time, &["symbolic-ref", "HEAD", &head]);
        }
        for (key, value) in [
            ("user.name", "Ann"),
            ("user.email", "ann@example.com"),
            ("commit.gpgsign", "false"),
            ("tag.gpgsign", "false"),
        ] {
            git(&made, time, &["config", key, value]);
        }
    }
}

fn commit(dir: &Path, time: i64, message: &str) {
    git(dir, time, &["commit", "-q", "--allow-empty", "-m", message]);
}

/// main に 2 つ、そこから分かれた topic に 1 つ、main でもう 1 つ進めてからマージ。
fn merged(dir: &Path) {
    git(dir, BASE, &["init", "-q", "-b", "main"]);
    commit(dir, BASE, "Start");
    commit(dir, BASE + DAY, "Add the graph");
    git(dir, BASE, &["switch", "-q", "-c", "topic"]);
    commit(dir, BASE + DAY * 2, "日本語の件名のコミット");
    git(dir, BASE, &["switch", "-q", "main"]);
    commit(dir, BASE + DAY * 3, "Bump the version");
    git(
        dir,
        BASE + DAY * 4,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "Merge branch 'topic'",
            "topic",
        ],
    );
    git(dir, BASE, &["tag", "v1"]);
}

fn open(dir: &Path) -> Arc<dyn Front> {
    Arc::new(InProcess::open(dir, None).unwrap())
}

/// グラフ・札・件名・作者・日付が 1 コミット 1 行で並ぶ
#[test]
fn history_shows_one_line_per_commit() {
    let dir = tempfile::tempdir().unwrap();
    merged(dir.path());
    let text = amonhen_tui::snapshot(open(dir.path()), 64, 8, BASE + DAY * 5).unwrap();
    println!("{text}");
    let name = dir.path().file_name().unwrap().to_string_lossy();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0].trim(), format!("{name}  main"));
    assert_eq!(
        &lines[1..6],
        [
            "◉─╮  [main] [v1] Merge branch 'topic'      Ann                1d",
            "● │  Bump the version                      Ann                2d",
            "│ ●  [topic] 日本語の件名のコミット        Ann                3d",
            "●─╯  Add the graph                         Ann                4d",
            "●    Start                                 Ann                5d",
        ]
    );
    assert!(lines[7].ends_with("1/5"), "{:?}", lines[7]);
}

/// 狭い端末では、作者と日付を出さずに件名を切り詰める
#[test]
fn a_narrow_terminal_drops_the_author_and_date() {
    let dir = tempfile::tempdir().unwrap();
    merged(dir.path());
    let text = amonhen_tui::snapshot(open(dir.path()), 24, 6, BASE + DAY * 5).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[1], "◉─╮  [main] [v1] Merge …");
    assert_eq!(lines[3], "│ ●  [topic] 日本語の件…");
}

/// 行の多い History をスクロールしても、手元の行だけで描ける間隔で測れる
#[test]
fn scrolling_is_measured_without_a_terminal() {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), BASE, &["init", "-q", "-b", "main"]);
    for i in 0..40 {
        commit(dir.path(), BASE + i, &format!("Commit {i}"));
    }
    let m = amonhen_tui::measure_scroll(open(dir.path()), 80, 12, 30, Duration::from_millis(1))
        .unwrap();
    assert_eq!((m.frames, m.rows), (30, 40));
    assert!(m.mean_bytes > 0 && m.max_bytes >= m.mean_bytes);
    assert_eq!(m.incomplete_frames, 0);
}
