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

/// 時刻は、手元の時間帯によらないよう UNIX 時刻のまま出す。
fn local(time: i64) -> String {
    format!("@{time}")
}

fn clock() -> amonhen_tui::Clock {
    amonhen_tui::Clock {
        now: BASE + DAY * 5,
        local,
    }
}

/// ファイルを 1 つ足し、次のコミットで 1 行を書き換える。
fn changed(dir: &Path) {
    git(dir, BASE, &["init", "-q", "-b", "main"]);
    std::fs::write(
        dir.join("lane.rs"),
        "fn lane() -> u32 {\n    let lane = 0;\n    lane\n}\n",
    )
    .unwrap();
    git(dir, BASE, &["add", "."]);
    git(dir, BASE, &["commit", "-q", "-m", "Start"]);
    std::fs::write(
        dir.join("lane.rs"),
        "fn lane() -> u32 {\n    let lane = trunk();\n    lane\n}\n",
    )
    .unwrap();
    std::fs::write(dir.join("notes.txt"), "one\n").unwrap();
    git(dir, BASE, &["add", "."]);
    git(
        dir,
        BASE + DAY * 3,
        &[
            "commit",
            "-q",
            "-m",
            "Take the lane from the trunk\n\nThe first lane was always 0.",
        ],
    );
}

fn open(dir: &Path) -> Arc<dyn Front> {
    Arc::new(InProcess::open(dir, None).unwrap())
}

/// グラフ・札・件名・作者・日付が 1 コミット 1 行で並ぶ
#[test]
fn history_shows_one_line_per_commit() {
    let dir = tempfile::tempdir().unwrap();
    merged(dir.path());
    let text = amonhen_tui::snapshot(open(dir.path()), 64, 8, clock(), "").unwrap();
    println!("{text}");
    let name = dir.path().file_name().unwrap().to_string_lossy();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines[0].trim(),
        format!("{name}  main   1 Changes   2 History")
    );
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
    let text = amonhen_tui::snapshot(open(dir.path()), 24, 6, clock(), "").unwrap();
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
    let m = amonhen_tui::measure_scroll(
        open(dir.path()),
        80,
        12,
        30,
        Duration::from_millis(1),
        local,
    )
    .unwrap();
    assert_eq!((m.frames, m.rows), (30, 40));
    assert!(m.mean_bytes > 0 && m.max_bytes >= m.mean_bytes);
    assert_eq!(m.incomplete_frames, 0);
}

/// 幅があれば、History の右に選んでいるコミットの詳細と差分が並ぶ
#[test]
fn details_sit_beside_history_when_wide() {
    let dir = tempfile::tempdir().unwrap();
    changed(dir.path());
    let text = amonhen_tui::snapshot(open(dir.path()), 120, 20, clock(), "").unwrap();
    println!("{text}");
    let lines: Vec<&str> = text.lines().collect();
    // 左が History、区切りの右が詳細。
    let right = |line: &str| line.split_once('│').map(|(_, r)| r.to_owned());
    let details: Vec<String> = lines.iter().filter_map(|l| right(l)).collect();
    assert!(lines[1].starts_with("◉  [main] Take the lane from the trunk"));
    assert_eq!(details[0], "Take the lane from the trunk");
    assert_eq!(details[2], "The first lane was always 0.");
    assert_eq!(details[4], "Author   Ann <ann@example.com>");
    assert_eq!(details[5], format!("Date     @{} (2d ago)", BASE + DAY * 3));
    assert_eq!(details[9], "M lane.rs +1 -1");
    assert_eq!(details[10], "A notes.txt +1");
    assert!(details[12].starts_with("── lane.rs ─ +1 -1 ──"));
    assert_eq!(details[13], "@@ -1,4 +1,4 @@");
    assert_eq!(details[14], "1 1  fn lane() -> u32 {");
    assert_eq!(details[15], "2   -    let lane = 0;");
    assert_eq!(details[16], "  2 +    let lane = trunk();");
}

/// 狭ければ、Enter で History の所に詳細が開き、Esc で戻る
#[test]
fn details_open_over_history_when_narrow() {
    let dir = tempfile::tempdir().unwrap();
    changed(dir.path());
    let text = amonhen_tui::snapshot(open(dir.path()), 64, 20, clock(), "\n").unwrap();
    println!("{text}");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[1], "Take the lane from the trunk");
    assert_eq!(lines[15], "1 1  fn lane() -> u32 {");
    assert!(lines[19].contains("Esc back"));
    // 一番下まで動かすと、2 つ目のファイルが見える。
    let end = amonhen_tui::snapshot(open(dir.path()), 64, 20, clock(), "\nG").unwrap();
    println!("{end}");
    assert!(end.contains("── notes.txt ─ +1 ──"));
    assert!(end.contains("  1 +one"));
    // `]` は次のファイルの見出しを一番上に。
    let jumped = amonhen_tui::snapshot(open(dir.path()), 64, 8, clock(), "\n]").unwrap();
    println!("{jumped}");
    assert!(jumped.lines().nth(1).unwrap().starts_with("── lane.rs ─"));
    let back = amonhen_tui::snapshot(open(dir.path()), 64, 20, clock(), "\n\x1b").unwrap();
    assert!(back.lines().nth(1).unwrap().starts_with("◉  [main]"));
}

/// 書き換えたファイル・stage 済みの新しいファイル・追跡していないファイルがある作業ツリー。
fn dirty(dir: &Path) {
    changed(dir);
    std::fs::write(
        dir.join("lane.rs"),
        "fn lane() -> u32 {\n    let lane = branch();\n    lane\n}\n",
    )
    .unwrap();
    std::fs::write(dir.join("staged.txt"), "two\n").unwrap();
    git(dir, BASE, &["add", "staged.txt"]);
    std::fs::write(dir.join("untracked.txt"), "three\n").unwrap();
}

/// `1` で Working Copy: 変わったファイルの一覧と、選んだファイルの差分
#[test]
fn working_copy_lists_files_beside_the_diff() {
    let dir = tempfile::tempdir().unwrap();
    dirty(dir.path());
    let text = amonhen_tui::snapshot(open(dir.path()), 120, 12, clock(), "1").unwrap();
    println!("{text}");
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].ends_with("main   1 Changes (3)   2 History"));
    let (list, diff): (Vec<&str>, Vec<&str>) = lines[1..8]
        .iter()
        .map(|l| l.split_once('│').unwrap())
        .map(|(l, d)| (l.trim_end(), d))
        .unzip();
    // stage の印（まだ・済み）、状態の 1 文字、パス。
    assert_eq!(
        list[..3],
        ["[ ] M lane.rs", "[x] A staged.txt", "[ ] ? untracked.txt"]
    );
    assert_eq!(diff[0], "lane.rs +1 -1");
    assert_eq!(diff[1], "@@ -1,4 +1,4 @@");
    assert_eq!(diff[3], "2     -    let lane = trunk();");
    assert_eq!(diff[4], "  2   +    let lane = branch();");
}

/// Space で、選んでいるファイルを stage する。もう一度で unstage
#[test]
fn space_stages_and_unstages_the_file() {
    let dir = tempfile::tempdir().unwrap();
    dirty(dir.path());
    let staged = amonhen_tui::snapshot(open(dir.path()), 120, 12, clock(), "1 ").unwrap();
    println!("{staged}");
    // 一覧の印が替わり、差分の行に stage 済みの印がつく。
    assert!(staged.contains("[x] M lane.rs"));
    assert!(staged.contains("│2   ● -    let lane = trunk();"));
    let index = |dir: &Path| {
        let out = amonhen_model::process::command("git")
            .args(["diff", "--cached", "--name-only"])
            .current_dir(dir)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    assert_eq!(index(dir.path()), "lane.rs\nstaged.txt\n");
    let unstaged = amonhen_tui::snapshot(open(dir.path()), 120, 12, clock(), "1 ").unwrap();
    assert!(unstaged.contains("[ ] M lane.rs"));
    assert_eq!(index(dir.path()), "staged.txt\n");
}

/// 狭ければ、Enter で一覧の所に差分が開く
#[test]
fn working_copy_diff_opens_over_the_list_when_narrow() {
    let dir = tempfile::tempdir().unwrap();
    dirty(dir.path());
    let text = amonhen_tui::snapshot(open(dir.path()), 60, 12, clock(), "1jj\n").unwrap();
    println!("{text}");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[1], "untracked.txt +1");
    assert_eq!(lines[3], "  1   +three");
    assert!(lines[11].contains("Esc back"));
}
