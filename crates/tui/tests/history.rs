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
        format!("{name}  main   1 Changes   2 History   3 Reflog")
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
    // ホイールと、`j` `k` での選択の移動（端で折り返すまで）、詳細のスクロール。
    use amonhen_tui::Step;
    for (step, frames) in [(Step::Wheel, 30), (Step::Move, 100), (Step::Diff, 30)] {
        let m = amonhen_tui::measure_scroll(
            open(dir.path()),
            80,
            12,
            frames,
            Duration::from_millis(1),
            step,
            "",
            local,
        )
        .unwrap();
        assert_eq!((m.frames, m.rows), (frames, 40));
        // 詳細は 1 画面に収まるので、スクロールしても何も変わらない。
        assert!((m.mean_bytes > 0 || step == Step::Diff) && m.max_bytes >= m.mean_bytes);
        assert_eq!(m.incomplete_frames, 0);
    }
}

/// Working Copy の差分で選んでいる行を動かすのも測れる（最後の画面は、すべてを描いたものと同じ）
#[test]
fn moving_through_the_working_copy_diff_is_measured() {
    let dir = tempfile::tempdir().unwrap();
    two_lines(dir.path());
    let m = amonhen_tui::measure_scroll(
        open(dir.path()),
        80,
        12,
        20,
        Duration::from_millis(1),
        amonhen_tui::Step::Changes,
        "",
        local,
    )
    .unwrap();
    assert!(m.details_lines > 2 && m.mean_bytes > 0);
    // 横に動かすのも（行は短いので、消えていくだけ）。
    for step in [amonhen_tui::Step::ChangesPan, amonhen_tui::Step::DiffPan] {
        let m = amonhen_tui::measure_scroll(
            open(dir.path()),
            80,
            12,
            50,
            Duration::from_millis(1),
            step,
            "",
            local,
        )
        .unwrap();
        assert!(m.mean_bytes > 0, "{m:?}");
    }
}

/// 区切りの線で分けた `index` 番目の列（右の空白は除く）。
fn column(line: &str, index: usize) -> Option<String> {
    line.split('│').nth(index).map(|c| c.trim_end().to_owned())
}

/// 幅があれば、History の右に選んでいるコミットの詳細と差分が並ぶ
#[test]
fn details_sit_beside_history_when_wide() {
    let dir = tempfile::tempdir().unwrap();
    changed(dir.path());
    let text = amonhen_tui::snapshot(open(dir.path()), 160, 20, clock(), "").unwrap();
    println!("{text}");
    let lines: Vec<&str> = text.lines().collect();
    // 左からサイドバー、History、詳細。
    let details: Vec<String> = lines.iter().filter_map(|l| column(l, 2)).collect();
    assert!(
        column(lines[1], 1)
            .unwrap()
            .starts_with("◉  [main] Take the lane from the trunk")
    );
    assert_eq!(details[0], "Take the lane from the trunk");
    assert_eq!(details[2], "The first lane was always 0.");
    assert_eq!(details[4], "Author   Ann <ann@example.com>");
    assert_eq!(details[5], format!("Date     @{} (2d ago)", BASE + DAY * 3));
    assert_eq!(details[10], "M lane.rs +1 -1");
    assert_eq!(details[11], "A notes.txt +1");
    assert!(details[13].starts_with("── lane.rs ─ +1 -1 ──"));
    assert_eq!(details[14], "@@ -1,4 +1,4 @@");
    assert_eq!(details[15], "1 1  fn lane() -> u32 {");
    assert_eq!(details[16], "2   -    let lane = 0;");
    assert_eq!(details[17], "  2 +    let lane = trunk();");
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
    assert_eq!(lines[16], "1 1  fn lane() -> u32 {");
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
    let text = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "1").unwrap();
    println!("{text}");
    let lines: Vec<&str> = text.lines().collect();
    // サイドバーの Changes の行に、変わったファイルの数。
    assert!(column(lines[2], 0).unwrap().starts_with("  Changes"));
    assert!(column(lines[2], 0).unwrap().ends_with(" 3"));
    let list: Vec<String> = lines[1..8].iter().filter_map(|l| column(l, 1)).collect();
    let diff: Vec<String> = lines[1..8].iter().filter_map(|l| column(l, 2)).collect();
    // stage の印（まだ・済み）、状態の 1 文字、パス。
    assert_eq!(
        list[..3],
        ["[ ] M lane.rs", "[x] A staged.txt", "[ ] ? untracked.txt"]
    );
    assert_eq!(diff[0], " lane.rs +1 -1");
    assert_eq!(diff[1], " @@ -1,4 +1,4 @@");
    assert_eq!(diff[3], " 2     -    let lane = trunk();");
    assert_eq!(diff[4], "   2   +    let lane = branch();");
}

/// Space で、選んでいるファイルを stage する。もう一度で unstage
/// `+` は Stage All: 変更のあるファイルをすべて stage する（コマンドの一覧からも）
#[test]
fn plus_stages_all() {
    let dir = tempfile::tempdir().unwrap();
    dirty(dir.path());
    let staged = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "1+").unwrap();
    println!("{staged}");
    assert!(staged.contains("[x] M lane.rs"));
    assert!(staged.contains("[x] A untracked.txt"), "{staged}");
    // 作業ツリーに、stage していない変更は残らない。
    assert_eq!(git_out(dir.path(), &["diff", "--name-only"]), "");
    assert!(!git_out(dir.path(), &["status", "--porcelain"]).contains("??"));
    // もう stage するものはない。
    let none = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "1+").unwrap();
    assert!(none.contains("Nothing to stage"));
}

/// コマンドの一覧の Stage All
#[test]
fn stage_all_from_the_command_list() {
    let dir = tempfile::tempdir().unwrap();
    dirty(dir.path());
    amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "1:stage all\n").unwrap();
    assert_eq!(
        git_out(dir.path(), &["diff", "--cached", "--name-only"]),
        "lane.rs\nstaged.txt\nuntracked.txt\n"
    );
}

#[test]
fn space_stages_and_unstages_the_file() {
    let dir = tempfile::tempdir().unwrap();
    dirty(dir.path());
    let staged = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "1 ").unwrap();
    println!("{staged}");
    // 一覧の印が替わり、差分の行に stage 済みの印がつく。
    assert!(staged.contains("[x] M lane.rs"));
    assert!(staged.contains("│ 2   ● -    let lane = trunk();"));
    let index = |dir: &Path| {
        let out = amonhen_model::process::command("git")
            .args(["diff", "--cached", "--name-only"])
            .current_dir(dir)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    assert_eq!(index(dir.path()), "lane.rs\nstaged.txt\n");
    let unstaged = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "1 ").unwrap();
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
    // 左の 1 列は、選んでいる行の印。
    assert_eq!(lines[1], "▌untracked.txt +1");
    assert_eq!(lines[3], "   1   +three");
    assert!(lines[11].contains("Space stage/unstage"));
}

fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = amonhen_model::process::command("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// 2 か所を書き換えたファイル（hunk は 1 つ）。
fn two_lines(dir: &Path) {
    git(dir, BASE, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("a.txt"), "one\ntwo\nthree\nfour\n").unwrap();
    git(dir, BASE, &["add", "."]);
    git(dir, BASE, &["commit", "-q", "-m", "Start"]);
    std::fs::write(dir.join("a.txt"), "ONE\ntwo\nTHREE\nfour\n").unwrap();
}

/// 重ねて出せないファイル（バイナリ）で、stage 済みの変更と作業ツリーの変更があれば、差分で `s` で出す側を替える
/// （アプリの Unstaged / Staged の切り替え）。重ねて出せるファイルでは、そう言う
#[test]
fn switch_between_the_staged_and_unstaged_sides() {
    let dir = tempfile::tempdir().unwrap();
    two_lines(dir.path());
    std::fs::write(dir.path().join("b.bin"), b"\0one").unwrap();
    git(dir.path(), BASE, &["add", "b.bin"]);
    git(dir.path(), BASE, &["commit", "-q", "-m", "Binary"]);
    std::fs::write(dir.path().join("b.bin"), b"\0two").unwrap();
    git(dir.path(), BASE, &["add", "b.bin"]);
    std::fs::write(dir.path().join("b.bin"), b"\0three").unwrap();
    // a.txt は作業ツリーの変更だけ、b.bin は両方。一覧の 2 つ目が b.bin。
    let staged = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "1j\ns").unwrap();
    println!("{staged}");
    assert!(staged.contains("Showing the staged changes"), "{staged}");
    let back = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "1j\nss").unwrap();
    assert!(back.contains("Showing the unstaged changes"), "{back}");
    // 重ねて出せるファイル（テキスト）では、替えるものがない。
    let both = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "1\ns").unwrap();
    assert!(
        both.contains("This diff already shows both the staged and the unstaged changes"),
        "{both}"
    );
}

/// 差分の行で Space: その行だけを stage する。hunk の見出しなら、その hunk すべて
#[test]
fn space_in_the_diff_stages_a_line_or_a_hunk() {
    let dir = tempfile::tempdir().unwrap();
    two_lines(dir.path());
    // Enter で差分へ。パス、hunk の見出し、「-one」「+ONE」…の順。
    let text = amonhen_tui::snapshot(open(dir.path()), 80, 12, clock(), "1\njj j ").unwrap();
    println!("{text}");
    assert_eq!(
        git_out(dir.path(), &["diff", "--cached", "-U0"])
            .lines()
            .filter(|l| l.starts_with(['+', '-']) && !l.starts_with("+++") && !l.starts_with("---"))
            .collect::<Vec<_>>(),
        ["-one", "+ONE"]
    );
    assert!(
        text.contains("[-] M a.txt") || text.contains("● -one"),
        "{text}"
    );
    // hunk の見出しで Space: 残りも stage される。
    amonhen_tui::snapshot(open(dir.path()), 80, 12, clock(), "1\nj ").unwrap();
    assert_eq!(git_out(dir.path(), &["diff", "--name-only"]), "");
    // もう一度で、hunk すべてを unstage。
    amonhen_tui::snapshot(open(dir.path()), 80, 12, clock(), "1\nj ").unwrap();
    assert_eq!(
        git_out(dir.path(), &["diff", "--cached", "--name-only"]),
        ""
    );
}

/// `d` は確かめてから捨てる。`y` で行い、ほかのキーでやめる
#[test]
fn discard_asks_first() {
    let dir = tempfile::tempdir().unwrap();
    two_lines(dir.path());
    let read = |dir: &Path| std::fs::read_to_string(dir.join("a.txt")).unwrap();
    let asked = amonhen_tui::snapshot(open(dir.path()), 100, 12, clock(), "1\njjd").unwrap();
    println!("{asked}");
    assert!(asked.contains("Discard this line? Undo brings it back."));
    assert_eq!(read(dir.path()), "ONE\ntwo\nTHREE\nfour\n");
    // やめる。
    amonhen_tui::snapshot(open(dir.path()), 100, 12, clock(), "1\njjdn").unwrap();
    assert_eq!(read(dir.path()), "ONE\ntwo\nTHREE\nfour\n");
    // 「-one」の行だけを捨てる: 消した行が戻り、足した行は残る。
    amonhen_tui::snapshot(open(dir.path()), 100, 12, clock(), "1\njjdy").unwrap();
    assert_eq!(read(dir.path()), "one\nONE\ntwo\nTHREE\nfour\n");
    // 一覧で `d`: ファイルの変更をすべて捨てる。
    let asked = amonhen_tui::snapshot(open(dir.path()), 100, 12, clock(), "1d").unwrap();
    assert!(asked.contains("Discard the changes in a.txt? Undo brings them back."));
    let gone = amonhen_tui::snapshot(open(dir.path()), 100, 12, clock(), "1dy").unwrap();
    println!("{gone}");
    assert_eq!(read(dir.path()), "one\ntwo\nthree\nfour\n");
    assert!(gone.contains("No changes"));
}

/// `c` でメッセージを書き、`Ctrl-s` で stage 済みの変更をコミットする
#[test]
fn commit_from_the_message_field() {
    let dir = tempfile::tempdir().unwrap();
    two_lines(dir.path());
    let front = || {
        amonhen_model::testing::isolate_from_user_settings();
        open(dir.path())
    };
    // 何も stage していなければ、理由を出してコミットしない。
    let nothing = amonhen_tui::snapshot(front(), 100, 14, clock(), "1cFix\x13").unwrap();
    println!("{nothing}");
    assert!(nothing.contains("Nothing is staged"));
    assert_eq!(git_out(dir.path(), &["rev-list", "--count", "HEAD"]), "1\n");
    // 書いている途中の画面: 欄の見出しに件名の文字数。
    let typing =
        amonhen_tui::snapshot(front(), 100, 14, clock(), "1 cUse capitals\n\nBody").unwrap();
    println!("{typing}");
    assert!(typing.contains("─ Message ─"));
    assert!(typing.contains(" 12 ─"));
    assert!(typing.contains(" Use capitals"));
    assert!(typing.contains("Ctrl-s commit"));
    // stage 済み（上の Space）なので、コミットできる。
    let done =
        amonhen_tui::snapshot(front(), 100, 14, clock(), "1cUse capitals\n\nBody\x13").unwrap();
    println!("{done}");
    assert!(done.contains("Committed"));
    assert!(done.contains("No changes"));
    assert_eq!(
        git_out(dir.path(), &["log", "-1", "--format=%B"]).trim_end(),
        "Use capitals\n\nBody"
    );
}

/// hooks が何行も書いたら、一番下の行には 1 行目と「`!` で全部」、`!` で全部を枠に出す（アプリのシートの代わり）
#[cfg(unix)]
#[test]
fn the_hooks_output_shows_in_full_with_bang() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    two_lines(dir.path());
    let hook = dir.path().join(".git/hooks/pre-commit");
    let write_hook = |body: &str| {
        std::fs::write(&hook, format!("#!/bin/sh\n{body}")).unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    write_hook("echo 'lint: first' >&2\necho 'lint: second' >&2\nexit 0\n");
    let front = || {
        amonhen_model::testing::isolate_from_user_settings();
        open(dir.path())
    };
    let none = amonhen_tui::snapshot(front(), 120, 14, clock(), "!").unwrap();
    assert!(none.contains("No output from the hooks yet"), "{none}");
    let done = amonhen_tui::snapshot(front(), 120, 14, clock(), "1 cFix\x13").unwrap();
    println!("{done}");
    assert!(
        done.contains("Committed: lint: first (! shows all)"),
        "{done}"
    );
    // 断られたとき: 1 行目と、全部を出せること。
    std::fs::write(dir.path().join("a.txt"), "again\n").unwrap();
    write_hook("echo 'lint: bad style' >&2\necho 'lint: see the guide' >&2\nexit 1\n");
    let refused = amonhen_tui::snapshot(front(), 120, 14, clock(), "1 cFix\x13").unwrap();
    println!("{refused}");
    assert!(refused.contains("(! shows all)"), "{refused}");
    // 上で stage したまま（断られたので）なので、そのままもう一度コミットする。
    let all = amonhen_tui::snapshot(front(), 120, 14, clock(), "1cFix\x13\x1b!").unwrap();
    println!("{all}");
    assert!(all.contains(" Output of the hooks "), "{all}");
    assert!(
        all.contains("lint: bad style") && all.contains("lint: see the guide"),
        "{all}"
    );
    assert_eq!(git_out(dir.path(), &["rev-list", "--count", "HEAD"]), "2\n");
}

/// `a` で amend: 欄に直前のメッセージが入り、コミットを作り直す
#[test]
fn amend_rewrites_the_last_commit() {
    let dir = tempfile::tempdir().unwrap();
    two_lines(dir.path());
    let front = || {
        amonhen_model::testing::isolate_from_user_settings();
        open(dir.path())
    };
    let shown = amonhen_tui::snapshot(front(), 100, 14, clock(), "1a").unwrap();
    println!("{shown}");
    assert!(shown.contains("─ Message (amend) ─"));
    assert!(shown.contains(" Start"));
    // メッセージに書き足して、stage した変更ごと作り直す。
    let done = amonhen_tui::snapshot(front(), 100, 14, clock(), "1 ac again\x13").unwrap();
    println!("{done}");
    assert_eq!(git_out(dir.path(), &["rev-list", "--count", "HEAD"]), "1\n");
    assert_eq!(
        git_out(dir.path(), &["log", "-1", "--format=%s"]),
        "Start again\n"
    );
    assert_eq!(git_out(dir.path(), &["status", "--porcelain"]), "");
}

/// `F`: 今のブランチの最近のコミットの件名を選び、欄を fixup! にしてコミットする（fixup! の件名は候補に出さない）。
/// Tab で squash! にすると、書いていた本文は残る
#[test]
fn fixup_and_squash_commit_for_a_recent_commit() {
    let dir = tempfile::tempdir().unwrap();
    two_lines(dir.path());
    commit(dir.path(), BASE + DAY, "Add the graph");
    commit(dir.path(), BASE + DAY * 2, "fixup! Start");
    let front = || {
        amonhen_model::testing::isolate_from_user_settings();
        open(dir.path())
    };
    let listed = amonhen_tui::snapshot(front(), 100, 16, clock(), "1 F").unwrap();
    println!("{listed}");
    assert!(
        listed.contains(" Fixup Commit For (Tab: squash!) "),
        "{listed}"
    );
    assert!(listed.contains("Add the graph"), "{listed}");
    assert!(!listed.contains("fixup! Start"), "{listed}");
    let narrowed = amonhen_tui::snapshot(front(), 100, 16, clock(), "1 Fsta").unwrap();
    assert!(!narrowed.contains("Add the graph"), "{narrowed}");

    let done = amonhen_tui::snapshot(front(), 100, 16, clock(), "1 Fgraph\n\x13").unwrap();
    println!("{done}");
    assert_eq!(
        git_out(dir.path(), &["log", "-1", "--format=%B"]).trim_end(),
        "fixup! Add the graph"
    );

    std::fs::write(dir.path().join("a.txt"), "one\n").unwrap();
    let keys = "1 cDraft\n\nKept body\x1bF\tgraph\n\x13";
    let done = amonhen_tui::snapshot(front(), 100, 16, clock(), keys).unwrap();
    println!("{done}");
    assert_eq!(
        git_out(dir.path(), &["log", "-1", "--format=%B"]).trim_end(),
        "squash! Add the graph\n\nKept body"
    );
}

/// untracked のファイルで `i`: そのパスか拡張子のパターンを .gitignore に足す（もうあれば足さない）
#[test]
fn i_adds_an_untracked_file_to_gitignore() {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), BASE, &["init", "-q", "-b", "main"]);
    commit(dir.path(), BASE, "Start");
    std::fs::write(dir.path().join("notes.log"), "x\n").unwrap();
    let front = || {
        amonhen_model::testing::isolate_from_user_settings();
        open(dir.path())
    };
    let asked = amonhen_tui::snapshot(front(), 100, 14, clock(), "1i").unwrap();
    println!("{asked}");
    assert!(asked.contains("Add to .gitignore:"), "{asked}");
    assert!(asked.contains("*.log"), "{asked}");
    let done = amonhen_tui::snapshot(front(), 100, 14, clock(), "1ie").unwrap();
    println!("{done}");
    assert!(done.contains("Added to .gitignore"), "{done}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join(".gitignore")).unwrap(),
        "*.log\n"
    );
    // .gitignore だけが残る（notes.log は無視される）。`p` でそのパス。
    std::fs::write(dir.path().join("todo.txt"), "x\n").unwrap();
    let keys = "1jip";
    let listed = amonhen_tui::snapshot(front(), 100, 14, clock(), "1").unwrap();
    println!("{listed}");
    let done = amonhen_tui::snapshot(front(), 100, 14, clock(), keys).unwrap();
    println!("{done}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join(".gitignore")).unwrap(),
        "*.log\ntodo.txt\n"
    );
}

/// Working Copy の `v`: パス → 木（フォルダの行で中をまとめて stage、Enter で畳む）→ ファイル名。`/` でパスを絞る
#[test]
fn the_working_copy_lists_a_tree_or_names_and_filters() {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), BASE, &["init", "-q", "-b", "main"]);
    commit(dir.path(), BASE, "Start");
    for path in ["src/a.rs", "src/b.rs", "docs/guide.md", "top.txt"] {
        let file = dir.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, "x\n").unwrap();
    }
    let front = || {
        amonhen_model::testing::isolate_from_user_settings();
        open(dir.path())
    };
    let list = |keys: &str| -> Vec<String> {
        let text = amonhen_tui::snapshot(front(), 100, 16, clock(), keys).unwrap();
        println!("{text}");
        (text.lines().skip(1).take(7))
            .filter_map(|l| column(l, 1))
            .map(|l| l.trim_end().to_owned())
            .collect()
    };
    assert_eq!(
        list("1v")[..6],
        [
            "[ ]   ▾ docs/",
            "[ ] ?   guide.md",
            "[ ]   ▾ src/",
            "[ ] ?   a.rs",
            "[ ] ?   b.rs",
            "[ ] ? top.txt"
        ]
    );
    // 選んでいたファイル（guide.md）は木でも選んだまま。1 つ下の src/ で Space: 中のすべてを stage。Enter で畳む。
    let staged = list("1vj \n");
    assert_eq!(staged[2..4], ["[x]   ▸ src/", "[ ] ? top.txt"]);
    assert_eq!(
        git_out(dir.path(), &["diff", "--cached", "--name-only"]),
        "src/a.rs\nsrc/b.rs\n"
    );
    // ファイル名を先に、フォルダは後ろに。
    let names = list("1vv");
    assert!(
        names
            .iter()
            .any(|l| l.starts_with("[x] A a.rs") && l.ends_with("src")),
        "{names:?}"
    );
    // パスで絞る。
    let filtered = list("1/src\n");
    assert_eq!(filtered[..3], ["[x] A src/a.rs", "[x] A src/b.rs", ""]);
}

/// 書きかけのメッセージは `<git dir>/amonhen/commit-draft` に残り、次に開いたときに欄に戻る。コミットすると消える。
/// 下書きがなければ `commit.template`（`#` の行を除く）を入れ、書き足すまでは下書きにしない
#[test]
fn the_message_draft_is_kept_and_the_template_fills_an_empty_field() {
    let dir = tempfile::tempdir().unwrap();
    two_lines(dir.path());
    let draft = dir.path().join(".git/amonhen/commit-draft");
    let front = || {
        amonhen_model::testing::isolate_from_user_settings();
        open(dir.path())
    };
    // テンプレートで始まる。触らなければ下書きにしない。
    std::fs::write(dir.path().join("template.txt"), "Area: \n# what changed\n").unwrap();
    let template = dir.path().join("template.txt");
    git(
        dir.path(),
        BASE,
        &["config", "commit.template", template.to_str().unwrap()],
    );
    let shown = amonhen_tui::snapshot(front(), 100, 14, clock(), "1").unwrap();
    println!("{shown}");
    assert!(shown.contains(" Area:"), "{shown}");
    assert!(!shown.contains("what changed"), "{shown}");
    assert!(!draft.exists());
    // 欄を離れると、書きかけを覚える。
    amonhen_tui::snapshot(front(), 100, 14, clock(), "1cparser\x1b").unwrap();
    assert_eq!(std::fs::read_to_string(&draft).unwrap(), "Area: parser");
    let shown = amonhen_tui::snapshot(front(), 100, 14, clock(), "1").unwrap();
    assert!(shown.contains(" Area: parser"), "{shown}");
    // コミットすると消える。
    amonhen_tui::snapshot(front(), 100, 14, clock(), "1 \x13").unwrap();
    assert_eq!(
        git_out(dir.path(), &["log", "-1", "--format=%s"]),
        "Area: parser\n"
    );
    assert!(!draft.exists());
}

/// サイドバー: Workspace の行と、Branches・Tags。ref を開くと、History でそのコミットを選ぶ
#[test]
fn sidebar_lists_refs_and_jumps_to_them() {
    let dir = tempfile::tempdir().unwrap();
    merged(dir.path());
    let text = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "").unwrap();
    println!("{text}");
    let side: Vec<String> = text.lines().skip(1).filter_map(|l| column(l, 0)).collect();
    assert_eq!(
        side[..9],
        [
            "Workspace",
            "  Changes",
            "  History",
            "  Reflog",
            "Branches",
            "● main",
            "  topic",
            "Tags",
            "  v1"
        ]
    );
    // `0` でサイドバーへ。History の 2 つ下が topic。Enter で、そのコミットを選ぶ。
    let jumped = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "0jjj\n").unwrap();
    println!("{jumped}");
    assert!(jumped.contains("3/5"));
    let details: Vec<String> = jumped.lines().filter_map(|l| column(l, 2)).collect();
    assert_eq!(details[0], "日本語の件名のコミット");
    // Changes の行を開くと、Working Copy。
    let changes = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "0k\n").unwrap();
    assert!(changes.contains("No changes"));
}

/// 狭ければ、サイドバーは `0` で呼んだときだけ出る
#[test]
fn sidebar_opens_on_demand_when_narrow() {
    let dir = tempfile::tempdir().unwrap();
    merged(dir.path());
    let text = amonhen_tui::snapshot(open(dir.path()), 64, 10, clock(), "0").unwrap();
    println!("{text}");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[1], "Workspace");
    assert_eq!(lines[6], "● main");
    let back = amonhen_tui::snapshot(open(dir.path()), 64, 10, clock(), "0jjj\n").unwrap();
    println!("{back}");
    assert!(
        back.lines()
            .nth(3)
            .unwrap()
            .contains("[topic] 日本語の件名のコミット")
    );
}

/// `/` で History を検索し、合った行を選ぶ。`n` `N` で次と前、Esc でやめる
#[test]
fn slash_searches_history() {
    let dir = tempfile::tempdir().unwrap();
    merged(dir.path());
    // 打っている間は、一番下の行に語が出る。
    let typing = amonhen_tui::snapshot(open(dir.path()), 80, 9, clock(), "/vers").unwrap();
    println!("{typing}");
    assert!(typing.lines().last().unwrap().starts_with(" /vers"));
    // Enter で検索: 合った行（Bump the version）を選ぶ。
    let found = amonhen_tui::snapshot(open(dir.path()), 80, 9, clock(), "/version\n").unwrap();
    println!("{found}");
    let last = found.lines().last().unwrap();
    assert!(last.starts_with(" /version: 1/1"), "{last}");
    assert!(last.ends_with("2/5"));
    // 作者で検索すると、すべての行が合う。`n` で次へ、`N` で前へ（端では回る）。
    let all = amonhen_tui::snapshot(open(dir.path()), 80, 9, clock(), "/author:Ann\nnn").unwrap();
    assert!(all.lines().last().unwrap().starts_with(" /author:Ann: 3/5"));
    let wrapped =
        amonhen_tui::snapshot(open(dir.path()), 80, 9, clock(), "/author:Ann\nN").unwrap();
    assert!(
        wrapped
            .lines()
            .last()
            .unwrap()
            .starts_with(" /author:Ann: 5/5")
    );
    // 合う行がなければ 0。Esc で検索をやめる。
    let none =
        amonhen_tui::snapshot(open(dir.path()), 80, 9, clock(), "/nothing-like-this\n").unwrap();
    assert!(
        none.lines()
            .last()
            .unwrap()
            .starts_with(" /nothing-like-this: 0")
    );
    let cleared =
        amonhen_tui::snapshot(open(dir.path()), 80, 9, clock(), "/version\n\x1b").unwrap();
    assert!(cleared.lines().last().unwrap().contains("/ search"));
}

/// 選んでいるコミットの祖先でない行は薄くなる
#[test]
fn rows_outside_the_selected_commits_ancestry_are_dimmed() {
    use ratatui_core::style::Modifier;
    let dir = tempfile::tempdir().unwrap();
    merged(dir.path());
    let dim = |keys: &str| -> Vec<bool> {
        let frame = amonhen_tui::frame(open(dir.path()), 64, 8, clock(), keys).unwrap();
        // 件名のあたりのセルで見る。
        (1..6)
            .map(|y| frame[(8, y)].modifier.contains(Modifier::DIM))
            .collect()
    };
    // マージを選んでいる間は、すべてが祖先。
    assert_eq!(dim(""), [false; 5]);
    // 「Bump the version」の祖先は、main の側の 2 つ。マージと topic のコミットは薄くなる。
    assert_eq!(dim("j"), [true, false, true, false, false]);
    // topic のコミットの祖先は、分かれる前の 2 つ。
    assert_eq!(dim("jj"), [true, true, false, false, false]);
}

/// グラフは、祖先へ続く線と祖先の点だけが光る（行ごとではなく、線ごと）。
#[test]
fn graph_lines_are_lit_along_the_ancestry() {
    use ratatui_core::style::Modifier;
    let dir = tempfile::tempdir().unwrap();
    merged(dir.path());
    // グラフの 3 セルを、行ごとに: 薄ければ `.`、光っていれば文字のまま。
    let graph = |keys: &str| -> Vec<String> {
        let frame = amonhen_tui::frame(open(dir.path()), 64, 8, clock(), keys).unwrap();
        (1..6)
            .map(|y| {
                (0..3)
                    .map(|x| {
                        let cell = &frame[(x, y)];
                        match cell.modifier.contains(Modifier::DIM) && cell.symbol() != " " {
                            true => ".",
                            false => cell.symbol(),
                        }
                    })
                    .collect()
            })
            .collect()
    };
    // マージを選んでいる間は、すべてが光る。
    assert_eq!(graph(""), ["◉─╮", "● │", "│ ●", "●─╯", "●  "]);
    // 「Bump the version」: main の側の線と点だけ。topic へ行く線と、topic から戻る線は薄い。
    assert_eq!(graph("j"), ["...", "● .", "│ .", "●..", "●  "]);
    // topic のコミット: topic から分かれた所へ戻る線と、その先。main の側の線は、合流する行（Add the graph）から下だけ。
    assert_eq!(graph("jj"), ["...", ". .", ". ●", "●─╯", "●  "]);
}

/// サイドバーで `/`: 打った語を名前に含む ref だけを出す。Esc でやめる
#[test]
fn sidebar_filter_narrows_refs() {
    let dir = tempfile::tempdir().unwrap();
    merged(dir.path());
    let side = |keys: &str| -> Vec<String> {
        amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), keys)
            .unwrap()
            .lines()
            .skip(1)
            .filter_map(|l| column(l, 0))
            .filter(|l| !l.is_empty())
            .collect()
    };
    let filtered = side("0/TOP");
    assert_eq!(
        filtered[..6],
        [
            "Workspace",
            "  Changes",
            "  History",
            "  Reflog",
            "Branches",
            "  topic"
        ]
    );
    assert!(filtered[6].starts_with(" Filter: TOP"), "{filtered:?}");
    // Enter で確定しても絞り込んだまま。Esc で戻る。
    assert_eq!(side("0/TOP\n")[5], "  topic");
    assert_eq!(side("0/TOP\n\x1b")[5], "● main");
}

/// `?` で今の枠のキーの一覧。次のキーで閉じる
#[test]
fn question_mark_lists_the_keys() {
    let dir = tempfile::tempdir().unwrap();
    merged(dir.path());
    // History のキーは、収まるだけの高さで（収まらなければ `j` `k` で送る。それは別の試験）。
    let help = amonhen_tui::snapshot(open(dir.path()), 80, 40, clock(), "?").unwrap();
    println!("{help}");
    assert!(help.contains("┌─ Keys ─"));
    assert!(help.contains("│ n N          Next and previous match"));
    assert!(help.contains("│ q            Quit"));
    let closed = amonhen_tui::snapshot(open(dir.path()), 80, 30, clock(), "?x").unwrap();
    assert!(!closed.contains("Keys"));
    // Working Copy の差分では、その枠のキー。
    let diff = amonhen_tui::snapshot(open(dir.path()), 80, 30, clock(), "1\n?").unwrap();
    assert!(diff.contains("Previous and next hunk"));
}

/// `:` はコマンドの一覧: 名前はアプリのメニューと同じ、打った語で絞って、Enter でそのキーを押したのと同じに動かす。
#[test]
fn colon_lists_commands_and_runs_the_selected_one() {
    let dir = tempfile::tempdir().unwrap();
    merged(dir.path());
    let list = amonhen_tui::snapshot(open(dir.path()), 80, 30, clock(), ":").unwrap();
    println!("{list}");
    assert!(list.contains("┌─ Commands ─"));
    assert!(list.contains("Fetch…"));
    assert!(list.contains("Find in History…"));
    // 途中の操作がないので Continue は出ない。サイドバーにいないので Merge も出ない。
    assert!(!list.contains("Continue"));
    assert!(!list.contains("Merge…"));
    let side = amonhen_tui::snapshot(open(dir.path()), 160, 30, clock(), "0:").unwrap();
    assert!(side.contains("Merge…"));
    // 打った語で絞る。合うものがなければ、そう書く。
    let narrowed = amonhen_tui::snapshot(open(dir.path()), 80, 30, clock(), ":hist").unwrap();
    println!("{narrowed}");
    assert!(narrowed.contains(":hist"));
    assert!(narrowed.contains("Find in History…"));
    assert!(!narrowed.contains("Fetch…"));
    let none = amonhen_tui::snapshot(open(dir.path()), 80, 30, clock(), ":zzz").unwrap();
    assert!(none.contains("No command matches"));
    // Esc で閉じる。
    let closed = amonhen_tui::snapshot(open(dir.path()), 80, 30, clock(), ":\x1b").unwrap();
    assert!(!closed.contains("Commands"));
    // Enter で動かす: Working Copy から「Find in History…」は、History に移って検索の語を聞く。
    let find = amonhen_tui::snapshot(open(dir.path()), 80, 30, clock(), "1:find\ngraph").unwrap();
    println!("{find}");
    assert!(!find.contains("Commands"));
    assert!(find.contains("/graph"));
    // Ctrl-n で次の行を選ぶ: 「new」に合うのは New Branch… と New Tag…。
    let tag = amonhen_tui::snapshot(open(dir.path()), 80, 30, clock(), ":new\x0e\nv1").unwrap();
    println!("{tag}");
    assert!(tag.contains("New tag at"));
    assert!(tag.contains(": v1"));
}

/// 署名したコミットの詳細には、検証の結果（アプリと同じ言葉）と署名した人を出す。SSH の署名は git 2.34 から
#[test]
fn details_show_the_signature_check() {
    let version = git_out(Path::new("."), &["--version"]);
    let numbers: Vec<u32> = version
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|n| n.parse().ok())
        .collect();
    if numbers.len() < 2 || (numbers[0], numbers[1]) < (2, 34) {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let keys = tempfile::tempdir().unwrap();
    let key = keys.path().join("key");
    let made = amonhen_model::process::command("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-C", "ann", "-f"])
        .arg(&key)
        .output();
    let Ok(made) = made else { return };
    assert!(made.status.success());
    let public = std::fs::read_to_string(keys.path().join("key.pub")).unwrap();
    let allowed = keys.path().join("allowed");
    std::fs::write(&allowed, format!("ann@example.com {public}")).unwrap();
    git(dir.path(), BASE, &["init", "-q", "-b", "main"]);
    for (k, v) in [
        ("gpg.format", "ssh"),
        ("user.signingkey", key.to_str().unwrap()),
    ] {
        git(dir.path(), BASE, &["config", k, v]);
    }
    git(
        dir.path(),
        BASE,
        &["commit", "-q", "-S", "--allow-empty", "-m", "Signed"],
    );
    // 許可した署名者の一覧がなければ、検証できない。
    let unknown = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "").unwrap();
    println!("{unknown}");
    assert!(unknown.contains("Signature"), "{unknown}");
    assert!(!unknown.contains("Verified"));
    git(
        dir.path(),
        BASE,
        &[
            "config",
            "gpg.ssh.allowedSignersFile",
            allowed.to_str().unwrap(),
        ],
    );
    let verified = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "").unwrap();
    println!("{verified}");
    assert!(
        verified.contains("Signature  Verified, ann@example.com"),
        "{verified}"
    );
    // History の行にも、署名の印（アプリの行の印）。署名のないコミットには付けない。
    git(
        dir.path(),
        BASE,
        &[
            "commit",
            "-q",
            "--no-gpg-sign",
            "--allow-empty",
            "-m",
            "Plain",
        ],
    );
    let rows = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "").unwrap();
    println!("{rows}");
    let history: Vec<String> = rows.lines().skip(1).filter_map(|l| column(l, 1)).collect();
    assert!(
        history.iter().any(|l| l.contains("✓ Signed")),
        "{history:?}"
    );
    assert!(
        history
            .iter()
            .any(|l| l.contains("Plain") && !l.contains('✓')),
        "{history:?}"
    );
    // 署名したタグには、サイドバーで印（正しい署名は ✓）。署名のないタグには付けない。
    git(dir.path(), BASE, &["tag", "-s", "-m", "Release", "v1"]);
    git(dir.path(), BASE, &["tag", "plain"]);
    let tags = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "").unwrap();
    println!("{tags}");
    let side: Vec<String> = tags.lines().skip(1).filter_map(|l| column(l, 0)).collect();
    assert!(
        side.iter()
            .any(|l| l.trim_start().starts_with("v1") && l.ends_with('✓')),
        "{side:?}"
    );
    assert!(side.iter().any(|l| l.trim() == "plain"), "{side:?}");
}

/// History の `@` `]` `[`: HEAD・親（1 つ目の親）・すぐ上の子へ移る。
#[test]
fn go_to_head_parent_and_child() {
    let dir = tempfile::tempdir().unwrap();
    merged(dir.path());
    let at = |keys: &str| -> String {
        let out = amonhen_tui::snapshot(open(dir.path()), 100, 10, clock(), keys).unwrap();
        out.lines()
            .last()
            .unwrap()
            .split_whitespace()
            .last()
            .unwrap()
            .to_owned()
    };
    // 0: マージ（HEAD）、1: Bump the version、2: topic のコミット、3: Add the graph、4: Start。
    // マージの 1 つ目の親は main の側。
    assert_eq!(at("]"), "2/5");
    // topic のコミットの親は Add the graph。
    assert_eq!(at("jj]"), "4/5");
    // Add the graph の子のうち、すぐ上は topic のコミット。
    assert_eq!(at("jjj["), "3/5");
    // いちばん上には子がない。
    assert_eq!(at("["), "1/5");
    assert_eq!(at("G@"), "1/5");
    // コマンドの一覧からも。
    assert_eq!(at("G:go to head\n"), "1/5");
}

/// History の `o`: 日付順とトポロジー順（ブランチのコミットをそのマージの下にまとめる）を替える
#[test]
fn o_switches_history_between_date_and_topological_order() {
    let dir = tempfile::tempdir().unwrap();
    merged(dir.path());
    let subjects = |text: &str| -> Vec<String> {
        text.lines()
            .skip(1)
            .take(5)
            .map(|l| {
                [
                    "Merge branch",
                    "Bump the version",
                    "日本語の件名",
                    "Add the graph",
                    "Start",
                ]
                .into_iter()
                .find(|s| l.contains(s))
                .unwrap_or("")
                .to_owned()
            })
            .collect()
    };
    let date = amonhen_tui::snapshot(open(dir.path()), 100, 10, clock(), "").unwrap();
    assert_eq!(
        subjects(&date),
        [
            "Merge branch",
            "Bump the version",
            "日本語の件名",
            "Add the graph",
            "Start"
        ]
    );
    let topo = amonhen_tui::snapshot(open(dir.path()), 100, 10, clock(), "o").unwrap();
    println!("{topo}");
    assert!(topo.contains("History in topological order"));
    // topic のコミットが、マージのすぐ下に来る。
    assert_eq!(
        subjects(&topo),
        [
            "Merge branch",
            "日本語の件名",
            "Bump the version",
            "Add the graph",
            "Start"
        ]
    );
    let back = amonhen_tui::snapshot(open(dir.path()), 100, 10, clock(), "oo").unwrap();
    assert!(back.contains("History in date order"));
    assert_eq!(subjects(&back), subjects(&date));
}

/// History の `z`: マージ済みのブランチを折りたたむ（マージの行に隠した数）。`Z` で選んでいるマージを開く・閉じる
#[test]
fn z_folds_merged_branches() {
    let dir = tempfile::tempdir().unwrap();
    merged(dir.path());
    let count = |text: &str| {
        text.lines()
            .last()
            .unwrap()
            .split('/')
            .next_back()
            .unwrap()
            .trim()
            .to_owned()
    };
    let all = amonhen_tui::snapshot(open(dir.path()), 100, 10, clock(), "").unwrap();
    assert_eq!(count(&all), "5");
    let folded = amonhen_tui::snapshot(open(dir.path()), 100, 10, clock(), "z").unwrap();
    println!("{folded}");
    assert!(folded.contains("Merged branches folded"));
    assert!(!folded.contains("日本語の件名"));
    let merge = folded.lines().find(|l| l.contains("Merge branch")).unwrap();
    assert!(merge.contains("+1 Merge branch"), "{merge}");
    assert_eq!(count(&folded), "4");
    // マージの行で `Z`: 取り込んだ側を出す。もう一度で閉じる。
    let opened = amonhen_tui::snapshot(open(dir.path()), 100, 10, clock(), "zZ").unwrap();
    println!("{opened}");
    assert!(opened.contains("日本語の件名"));
    let closed = amonhen_tui::snapshot(open(dir.path()), 100, 10, clock(), "zZZ").unwrap();
    assert!(!closed.contains("日本語の件名"));
    // 折りたたむものがない行では、そう知らせる。
    let none = amonhen_tui::snapshot(open(dir.path()), 100, 10, clock(), "zjZ").unwrap();
    assert!(none.contains("This commit has nothing folded"));
    // `z` をもう一度で、すべて出す。
    let back = amonhen_tui::snapshot(open(dir.path()), 100, 10, clock(), "zz").unwrap();
    assert!(back.contains("日本語の件名"));
}

/// Space は、History とサイドバーで Enter と同じ（詳細へ入る・選んでいる行を開く）
#[test]
fn space_opens_like_enter() {
    let dir = tempfile::tempdir().unwrap();
    merged(dir.path());
    let shot = |keys: &str| amonhen_tui::snapshot(open(dir.path()), 80, 30, clock(), keys).unwrap();
    // 狭いので、詳細は History の所に開く。
    assert_ne!(shot("j "), shot("j"));
    assert_eq!(shot("j "), shot("j\n"));
    // サイドバーの Changes の行を開く。
    assert_ne!(shot("0gj "), shot("0gj"));
    assert_eq!(shot("0gj "), shot("0gj\n"));
}

/// `l` で右の枠へ入り、`h` で戻る。横に動かしてあれば、`h` はまず左へ戻す
#[test]
fn h_goes_back_from_the_right_pane() {
    let dir = tempfile::tempdir().unwrap();
    merged(dir.path());
    let shot = |keys: &str| amonhen_tui::snapshot(open(dir.path()), 80, 30, clock(), keys).unwrap();
    // 狭いので、詳細は History の所に開く。
    assert_ne!(shot("jl"), shot("j"));
    assert_eq!(shot("jlh"), shot("j"));
    // 右へ動かしてからの `h` は、詳細のまま左へ戻す。次の `h` で History へ。
    assert_eq!(shot("jllh"), shot("jl"));
    assert_eq!(shot("jllhh"), shot("j"));
    // Working Copy の差分でも同じ。
    let dir = tempfile::tempdir().unwrap();
    two_lines(dir.path());
    let shot = |keys: &str| amonhen_tui::snapshot(open(dir.path()), 80, 12, clock(), keys).unwrap();
    assert_ne!(shot("1l"), shot("1"));
    assert_eq!(shot("1lh"), shot("1"));
    assert_eq!(shot("1llh"), shot("1l"));
}

/// git がコンフリクトの見込み（`merge-tree --write-tree`、2.38 から）を出せるか。
/// 使っている git が `major.minor` 以降か。
fn git_at_least(major: u32, minor: u32) -> bool {
    let version = git_out(Path::new("."), &["--version"]);
    let mut parts = version
        .trim()
        .trim_start_matches("git version ")
        .split('.')
        .map(|p| p.parse::<u32>().unwrap_or(0));
    (parts.next().unwrap_or(0), parts.next().unwrap_or(0)) >= (major, minor)
}

fn predicts_conflicts(dir: &Path) -> bool {
    let version = git_out(dir, &["--version"]);
    let numbers: Vec<u32> = version
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|n| n.parse().ok())
        .collect();
    (numbers[0], numbers[1]) >= (2, 38)
}

/// 上流のあるリポジトリ: 空のリモートに main を push してから、手元で 1 つ進める。
fn with_remote(dir: &Path) -> std::path::PathBuf {
    let remote = dir.join("origin.git");
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    git(
        dir,
        BASE,
        &["init", "-q", "--bare", "-b", "main", "origin.git"],
    );
    git(&work, BASE, &["init", "-q", "-b", "main"]);
    commit(&work, BASE, "Start");
    git(
        &work,
        BASE,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&work, BASE, &["push", "-q", "-u", "origin", "main"]);
    commit(&work, BASE + DAY, "Ahead of the remote");
    work
}

/// `P` は確かめてから、今のブランチを上流へ push する
#[test]
fn push_asks_then_pushes_to_the_upstream() {
    let dir = tempfile::tempdir().unwrap();
    let work = with_remote(dir.path());
    let remote_head = || {
        git_out(
            &dir.path().join("origin.git"),
            &["log", "-1", "--format=%s", "main"],
        )
    };
    let asked = amonhen_tui::snapshot(open(&work), 160, 8, clock(), "P").unwrap();
    println!("{asked}");
    // 一番上の行に、上流より進んでいる数。
    assert!(asked.lines().next().unwrap().contains("main ↑1"));
    // 送るコミットの件名も（アプリの Push のシートの一覧の代わり）。
    assert!(
        asked.contains(
            "Push main to origin/main (1 commit)? (“Ahead of the remote”)   y push   t another branch…   n cancel"
        ),
        "{asked}"
    );
    assert_eq!(remote_head(), "Start\n");
    // やめる。
    amonhen_tui::snapshot(open(&work), 100, 8, clock(), "Pn").unwrap();
    assert_eq!(remote_head(), "Start\n");
    let pushed = amonhen_tui::snapshot(open(&work), 100, 8, clock(), "Py").unwrap();
    println!("{pushed}");
    assert!(pushed.contains("Pushed"));
    assert_eq!(remote_head(), "Ahead of the remote\n");
    assert!(!pushed.lines().next().unwrap().contains("↑"));
}

/// サイドバーのリモートのブランチで `U`: 今のブランチの上流にする（確かめてから）
#[test]
fn set_upstream_from_a_remote_branch() {
    let dir = tempfile::tempdir().unwrap();
    let work = with_remote(dir.path());
    git(&work, BASE, &["push", "-q", "origin", "HEAD:review"]);
    git(&work, BASE, &["fetch", "-q"]);
    let upstream = || git_out(&work, &["rev-parse", "--abbrev-ref", "main@{upstream}"]);
    assert_eq!(upstream(), "origin/main\n");
    // サイドバーの一番下が origin/review。
    let asked = amonhen_tui::snapshot(open(&work), 120, 16, clock(), "0GU").unwrap();
    println!("{asked}");
    assert!(asked.contains("Set the upstream of main to origin/review?   y set   n cancel"));
    let set = amonhen_tui::snapshot(open(&work), 120, 16, clock(), "0GUy").unwrap();
    assert!(set.contains("Set the upstream"));
    assert_eq!(upstream(), "origin/review\n");
    // リモートのブランチでない行では、理由を出す。
    let wrong = amonhen_tui::snapshot(open(&work), 120, 16, clock(), "0U").unwrap();
    assert!(wrong.contains("Select a remote branch to be the upstream"));
}

/// push を確かめているときの `t`: 送り先を打って、ほかのブランチへ送る（上流は変えない）。
#[test]
fn push_to_another_branch() {
    let dir = tempfile::tempdir().unwrap();
    let work = with_remote(dir.path());
    let origin = dir.path().join("origin.git");
    // 初めは上流が入っている。
    let typing = amonhen_tui::snapshot(open(&work), 120, 8, clock(), "Pt").unwrap();
    println!("{typing}");
    assert!(typing.contains("Push main to (remote/branch): origin/main"));
    // 打ち直して、origin/review へ。
    let keys = "Pt\x7f\x7f\x7f\x7freview\n";
    let pushed = amonhen_tui::snapshot(open(&work), 120, 8, clock(), keys).unwrap();
    println!("{pushed}");
    assert!(pushed.contains("Pushed"));
    assert_eq!(
        git_out(&origin, &["log", "-1", "--format=%s", "review"]),
        "Ahead of the remote\n"
    );
    // 上流はそのまま（まだ 1 つ進んでいる）。
    assert_eq!(
        git_out(&origin, &["log", "-1", "--format=%s", "main"]),
        "Start\n"
    );
    assert!(pushed.lines().next().unwrap().contains("main ↑1"));
    // リモートの名前がなければ、書き方を出す。
    let keys = "Pt\x7f\x7f\x7f\x7f\x7f\x7f\x7f\x7f\x7f\x7f\x7freview\n";
    let wrong = amonhen_tui::snapshot(open(&work), 120, 8, clock(), keys).unwrap();
    println!("{wrong}");
    assert!(wrong.contains("Write the remote and the branch, like origin/main"));
}

/// `f` で fetch（どこから取るかを確かめる）、`p` で pull: リモートに増えたコミットを取り込む
#[test]
fn fetch_and_pull_bring_in_remote_commits() {
    let dir = tempfile::tempdir().unwrap();
    let work = with_remote(dir.path());
    git(&work, BASE, &["push", "-q"]);
    // 別の clone から、リモートを 1 つ進める。
    let remote = dir.path().join("origin.git");
    git(
        dir.path(),
        BASE,
        &["clone", "-q", remote.to_str().unwrap(), "other"],
    );
    let other = dir.path().join("other");
    commit(&other, BASE + DAY * 2, "From elsewhere");
    git(&other, BASE, &["push", "-q"]);

    let asked = amonhen_tui::snapshot(open(&work), 100, 8, clock(), "f").unwrap();
    assert!(
        asked.contains("Fetch from origin?   y fetch   n cancel"),
        "{asked}"
    );
    let fetched = amonhen_tui::snapshot(open(&work), 100, 8, clock(), "fy").unwrap();
    println!("{fetched}");
    assert!(fetched.contains("Fetched"));
    assert!(fetched.lines().next().unwrap().contains("main ↓1"));
    assert!(fetched.contains("[origin/main] From elsewhere"));
    // 取り込むコミットの数と件名を見せて確かめる（アプリの Pull のシート）。
    let asked = amonhen_tui::snapshot(open(&work), 160, 8, clock(), "p").unwrap();
    println!("{asked}");
    assert!(
        asked.contains("Pull 1 commit from origin/main? (“From elsewhere”)   y pull   n cancel"),
        "{asked}"
    );
    let pulled = amonhen_tui::snapshot(open(&work), 100, 8, clock(), "py").unwrap();
    println!("{pulled}");
    assert!(pulled.contains("Pulled"));
    assert_eq!(
        git_out(&work, &["log", "-1", "--format=%s"]),
        "From elsewhere\n"
    );

    // リモートがいくつもあれば、すべてか 1 つを選ぶ。片づける設定があれば、そう書く。
    git(
        &work,
        BASE,
        &["remote", "add", "mirror", remote.to_str().unwrap()],
    );
    git(&work, BASE, &["config", "remote.mirror.prune", "true"]);
    let asked = amonhen_tui::snapshot(open(&work), 200, 8, clock(), "f").unwrap();
    println!("{asked}");
    assert!(
        asked.contains("Fetch from where? Remote branches deleted on mirror are removed here too (remote.mirror.prune).   a all remotes (mirror, origin)   1 mirror   2 origin   n cancel"),
        "{asked}"
    );
    amonhen_tui::snapshot(open(&work), 200, 8, clock(), "f1").unwrap();
    assert!(git_out(&work, &["branch", "-r"]).contains("mirror/main"));
}

/// サイドバーのリモートのブランチの行で、そのリモートを `e` で直し（名前と URL）、`X` で片づけ、`x` で消す
#[test]
fn edit_prune_and_remove_a_remote_from_the_sidebar() {
    let dir = tempfile::tempdir().unwrap();
    let work = with_remote(dir.path());
    let remote = dir.path().join("origin.git");
    let url = remote.to_str().unwrap().to_owned();
    let remotes = || git_out(&work, &["remote", "-v"]);
    // 一番下の行が origin/main。`e` は今の名前、次に今の URL を入れて聞く。
    let asked = amonhen_tui::snapshot(open(&work), 200, 10, clock(), "0Ge").unwrap();
    println!("{asked}");
    assert!(
        asked.contains("Rename the remote origin to: origin"),
        "{asked}"
    );
    let url_asked = amonhen_tui::snapshot(
        open(&work),
        200,
        10,
        clock(),
        "0Ge\x7f\x7f\x7f\x7f\x7f\x7fup\n",
    )
    .unwrap();
    assert!(
        url_asked.contains(&format!("URL of up: {url}")),
        "{url_asked}"
    );
    // 名前だけ変える（URL はそのまま）。
    amonhen_tui::snapshot(
        open(&work),
        200,
        10,
        clock(),
        "0Ge\x7f\x7f\x7f\x7f\x7f\x7fup\n\n",
    )
    .unwrap();
    assert!(
        remotes().starts_with(&format!("up\t{url} (fetch)")),
        "{}",
        remotes()
    );
    // URL だけ変える。
    let other = dir.path().join("other.git");
    git(
        dir.path(),
        BASE,
        &["clone", "-q", "--bare", &url, "other.git"],
    );
    let keys = format!("0Ge\n\x15{}\n", other.to_str().unwrap());
    amonhen_tui::snapshot(open(&work), 200, 10, clock(), &keys).unwrap();
    assert!(
        remotes().starts_with(&format!("up\t{} (fetch)", other.to_str().unwrap())),
        "{}",
        remotes()
    );
    // 何も変えなければ、そう知らせる。
    let same = amonhen_tui::snapshot(open(&work), 200, 10, clock(), "0Ge\n\n").unwrap();
    assert!(same.contains("Nothing changed"), "{same}");

    // 向こうで消えたブランチの追跡を片づける。
    git(&work, BASE, &["fetch", "-q", "up"]);
    git(&work, BASE, &["update-ref", "refs/remotes/up/gone", "HEAD"]);
    let asked = amonhen_tui::snapshot(open(&work), 200, 10, clock(), "0GX").unwrap();
    println!("{asked}");
    assert!(asked.contains("Prune up? "), "{asked}");
    assert!(asked.contains("y prune   n cancel"), "{asked}");
    amonhen_tui::snapshot(open(&work), 200, 10, clock(), "0GXy").unwrap();
    assert!(!git_out(&work, &["branch", "-r"]).contains("up/gone"));

    // 消す。
    let asked = amonhen_tui::snapshot(open(&work), 200, 10, clock(), "0Gx").unwrap();
    assert!(asked.contains("Remove up? "), "{asked}");
    amonhen_tui::snapshot(open(&work), 200, 10, clock(), "0Gxy").unwrap();
    assert_eq!(remotes(), "");
}

/// サイドバーのタグの行で、`t` でリモートへ push（同じ名前の別のタグがあれば置き換え）、`X` でリモートから消す
#[test]
fn push_replace_and_delete_a_tag_on_a_remote() {
    let dir = tempfile::tempdir().unwrap();
    let work = with_remote(dir.path());
    let remote = dir.path().join("origin.git");
    git(&work, BASE, &["tag", "v1"]);
    let remote_tag = || git_out(&remote, &["tag", "--format=%(objectname)", "-l", "v1"]);
    // まだ fetch でリモートを読んでいないので、そう書いて出す。一番下の行が v1。
    let asked = amonhen_tui::snapshot(open(&work), 220, 10, clock(), "0Gt").unwrap();
    println!("{asked}");
    assert!(
        asked.contains("Push the tag v1 to which remote? Everyone who fetches from it gets the tag. Undo can’t take it back.   1 origin (not checked yet)   n cancel"),
        "{asked}"
    );
    amonhen_tui::snapshot(open(&work), 220, 10, clock(), "0Gt1").unwrap();
    assert_eq!(remote_tag(), git_out(&work, &["rev-parse", "HEAD"]));
    // fetch で読んだ後は、もうあるリモートには出さない。
    amonhen_tui::snapshot(open(&work), 220, 10, clock(), "fy").unwrap();
    let every = amonhen_tui::snapshot(open(&work), 220, 10, clock(), "0Gt").unwrap();
    assert!(every.contains("Every remote has v1"), "{every}");
    // 手元の v1 を別のコミットにすると、置き換えになる。
    git(&work, BASE, &["tag", "-f", "v1", "HEAD~1"]);
    let replace = amonhen_tui::snapshot(open(&work), 220, 10, clock(), "0Gt").unwrap();
    assert!(replace.contains("1 replace on origin"), "{replace}");
    amonhen_tui::snapshot(open(&work), 220, 10, clock(), "0Gt1").unwrap();
    assert_eq!(remote_tag(), git_out(&work, &["rev-parse", "HEAD~1"]));
    // リモートから消す（手元のものは残る）。
    let asked = amonhen_tui::snapshot(open(&work), 220, 10, clock(), "0GX").unwrap();
    assert!(
        asked.contains("Delete the tag v1 from which remote?"),
        "{asked}"
    );
    amonhen_tui::snapshot(open(&work), 220, 10, clock(), "0GX1").unwrap();
    assert_eq!(remote_tag(), "");
    assert_eq!(git_out(&work, &["tag"]), "v1\n");
}

/// サイドバーでチェックアウトしていないブランチを選んで、`P` で publish・push、`p` で上流まで fast-forward する
#[test]
fn sync_a_branch_that_is_not_checked_out() {
    let dir = tempfile::tempdir().unwrap();
    let work = with_remote(dir.path());
    let remote = dir.path().join("origin.git");
    let there = || git_out(&remote, &["rev-parse", "side"]);
    let here = || git_out(&work, &["rev-parse", "side"]);
    // 上流のない side は publish する（サイドバーの 2 つ目のブランチ）。
    git(&work, BASE, &["branch", "side", "main~1"]);
    let asked = amonhen_tui::snapshot(open(&work), 200, 10, clock(), "0jjjP").unwrap();
    println!("{asked}");
    assert!(
        asked.contains("Publish side to origin?   y publish   n cancel"),
        "{asked}"
    );
    amonhen_tui::snapshot(open(&work), 200, 10, clock(), "0jjjPy").unwrap();
    assert_eq!(there(), here());
    assert_eq!(
        git_out(&work, &["rev-parse", "--abbrev-ref", "side@{u}"]),
        "origin/side\n"
    );
    // 進んでいれば push する（チェックアウトしているのは main のまま）。
    git(&work, BASE, &["branch", "-f", "side", "main"]);
    let asked = amonhen_tui::snapshot(open(&work), 200, 10, clock(), "0jjjP").unwrap();
    assert!(
        asked.contains("Push side to origin/side (1 commit)?"),
        "{asked}"
    );
    amonhen_tui::snapshot(open(&work), 200, 10, clock(), "0jjjPy").unwrap();
    assert_eq!(there(), git_out(&work, &["rev-parse", "main"]));
    // 遅れていれば、上流まで fast-forward する（前と後の絵つき、Undo で戻る）。
    git(&work, BASE, &["branch", "-f", "side", "main~1"]);
    let asked = amonhen_tui::snapshot(open(&work), 200, 14, clock(), "0jjjp").unwrap();
    println!("{asked}");
    assert!(
        asked.contains("Fast-forward side to origin/side (+1 commits)? Undo brings it back.   y fast-forward   n cancel"),
        "{asked}"
    );
    assert!(asked.contains(" Fast-forward side "), "{asked}");
    amonhen_tui::snapshot(open(&work), 200, 10, clock(), "0jjjpy").unwrap();
    assert_eq!(here(), git_out(&work, &["rev-parse", "main"]));
    assert_eq!(git_out(&work, &["branch", "--show-current"]), "main\n");
    // 同じなら、そう知らせる。
    let same = amonhen_tui::snapshot(open(&work), 200, 10, clock(), "0jjjp").unwrap();
    assert!(
        same.contains("side has everything in origin/side"),
        "{same}"
    );
}

/// `Ctrl-r` で読み直し（アプリの Refresh）、命令の一覧の Rebuild History で History のキャッシュを作り直す
#[test]
fn refresh_and_rebuild_history() {
    let dir = tempfile::tempdir().unwrap();
    merged(dir.path());
    let listed = amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), ":rebuild").unwrap();
    println!("{listed}");
    assert!(listed.contains("Rebuild History"), "{listed}");
    let refresh = amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), ":refresh").unwrap();
    assert!(refresh.contains("Ctrl-r"), "{refresh}");
    let rebuilt =
        amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), ":rebuild history\n").unwrap();
    println!("{rebuilt}");
    assert!(rebuilt.contains("Rebuilt History"), "{rebuilt}");
    assert!(rebuilt.contains("Merge branch 'topic'"), "{rebuilt}");
    // 読み直しても、History はそのまま。
    let again = amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), "\x12").unwrap();
    assert!(again.contains("Merge branch 'topic'"), "{again}");
}

/// 詳細の中を `/` で探し、`n` `N` で次と前の一致へ（アプリの詳細の中の検索）
#[test]
fn find_inside_the_details() {
    let dir = tempfile::tempdir().unwrap();
    four_lines(dir.path());
    // 一番上（Four）の詳細で探す: 件名と、差分の足した行。
    let found = amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), "\n/four\n").unwrap();
    println!("{found}");
    assert!(
        found.contains("“four”: 1 of 2 (n N for the next and previous)"),
        "{found}"
    );
    let next = amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), "\n/four\nn").unwrap();
    assert!(next.contains("“four”: 2 of 2"), "{next}");
    let around = amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), "\n/four\nnn").unwrap();
    assert!(around.contains("“four”: 1 of 2"), "{around}");
    let back = amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), "\n/four\nN").unwrap();
    assert!(back.contains("“four”: 2 of 2"), "{back}");
    let none = amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), "\n/nowhere\n").unwrap();
    assert!(none.contains("“nowhere” isn’t in the details"), "{none}");
}

/// コミットの詳細で、今見ているファイルのパスを `Y` でコピーし、`e` で作業ツリーのそのファイルを開く
#[test]
fn copy_the_path_or_open_the_file_in_view() {
    let dir = tempfile::tempdir().unwrap();
    four_lines(dir.path());
    // 一番上のコミット（a.txt を変える）の詳細へ。
    let copied = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "\nY").unwrap();
    println!("{copied}");
    assert!(
        copied.contains("Copied a.txt (to the terminal’s clipboard)"),
        "{copied}"
    );
    // 作業ツリーにないファイルは開けない（理由を出す）。
    std::fs::remove_file(dir.path().join("a.txt")).unwrap();
    let missing = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "\ne").unwrap();
    assert!(
        missing.contains("a.txt isn’t a file in the working tree"),
        "{missing}"
    );
}

/// サイドバーの `c` でブランチをチェックアウトし、`b` で新しいブランチを作る。`u` で戻す
#[test]
fn checkout_new_branch_and_undo() {
    let dir = tempfile::tempdir().unwrap();
    merged(dir.path());
    let branch = || git_out(dir.path(), &["branch", "--show-current"]);
    // History の 2 つ下が topic。
    let out = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "0jjjc").unwrap();
    println!("{out}");
    assert_eq!(branch(), "topic\n");
    assert!(out.contains("● topic"));
    // main の行（1 つ上）で `b`: 名前を聞き、main から作って切り替える。
    let typing = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "0jjbne").unwrap();
    assert!(typing.contains("New branch at main: ne"));
    amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "0jjbnext\n").unwrap();
    assert_eq!(branch(), "next\n");
    assert_eq!(
        git_out(dir.path(), &["rev-parse", "next"]),
        git_out(dir.path(), &["rev-parse", "main"])
    );
    // `u` は、戻す操作の名前を出して確かめる。
    let asked = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "u").unwrap();
    println!("{asked}");
    assert!(asked.contains("Undo “"));
    assert!(asked.contains("y undo   n cancel"));
    amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "uy").unwrap();
    assert_eq!(branch(), "topic\n");
}

/// main と topic が同じ行を別々に書き換えたリポジトリ（main にいる）。
fn diverged(dir: &Path) {
    git(dir, BASE, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("a.txt"), "one\n").unwrap();
    git(dir, BASE, &["add", "."]);
    git(dir, BASE, &["commit", "-q", "-m", "Start"]);
    git(dir, BASE, &["switch", "-q", "-c", "topic"]);
    std::fs::write(dir.join("a.txt"), "topic\n").unwrap();
    git(dir, BASE + DAY, &["commit", "-q", "-am", "On topic"]);
    git(dir, BASE, &["switch", "-q", "main"]);
    std::fs::write(dir.join("a.txt"), "main\n").unwrap();
    git(dir, BASE + DAY * 2, &["commit", "-q", "-am", "On main"]);
}

/// サイドバーの `m` で merge。コンフリクトで止まったら、一番下の行に途中の状態と操作が出る。`A` でやめる
#[test]
fn merge_stops_at_conflicts_and_can_be_aborted() {
    let dir = tempfile::tempdir().unwrap();
    diverged(dir.path());
    // History の 2 つ下が topic。
    // 確かめる文に、取り込む数・分かれてからの数・merge コミットの件名・コンフリクトの見込み。
    let asked = amonhen_tui::snapshot(open(dir.path()), 200, 12, clock(), "0jjjm").unwrap();
    println!("{asked}");
    // 件名の「into main」の有無は git の版と設定による。コンフリクトの見込みは git 2.38 から（merge-tree
    // --write-tree）で、古い git では何も足さない。
    let line = asked.lines().last().unwrap();
    assert!(
        line.contains("Merge topic into main as “Merge branch 'topic'"),
        "{line}"
    );
    assert!(
        line.contains("(+1 in, 1 on main since they split)."),
        "{line}"
    );
    // 分かれているので、merge コミットか、まとめて 1 つのコミットか。
    assert!(
        line.contains("   y merge   s squash into one commit   n cancel"),
        "{line}"
    );
    if predicts_conflicts(dir.path()) {
        assert!(line.contains("Conflicts are likely in a.txt."), "{line}");
    }
    // アプリのシートと同じく、前と後の絵: 後のいちばん上に新しい merge コミット、その下に topic から来るもの。
    assert!(asked.contains(" Merge topic into main "), "{asked}");
    assert!(asked.contains("[main]  new merge"), "{asked}");
    assert!(asked.contains("On topic  from topic"), "{asked}");
    // rebase なら、前は main のコミット、後はその写しが topic のコミットの上に。
    let rebase = amonhen_tui::snapshot(open(dir.path()), 200, 12, clock(), "0jjjr").unwrap();
    println!("{rebase}");
    assert!(rebase.contains(" Rebase main onto topic "), "{rebase}");
    assert!(
        rebase.contains("On main  [main]  leaves the branch"),
        "{rebase}"
    );
    assert!(rebase.contains("On main  [main]  new copy"), "{rebase}");
    assert!(rebase.contains("On topic  on topic"), "{rebase}");
    let stopped = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "0jjjmy").unwrap();
    println!("{stopped}");
    assert!(stopped.contains("Stopped at conflicts"));
    // Working Copy に移り、コンフリクトのファイルが並ぶ。
    assert!(stopped.contains("U a.txt"));
    assert!(dir.path().join(".git/MERGE_HEAD").exists());
    // 次に開くと、途中の状態が出ている。
    let state = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "").unwrap();
    println!("{state}");
    assert!(state.contains("merge · 1 conflict   C continue   K skip   A abort"));
    // サイドバーの Workspace に、コンフリクトの数の行（アプリの Conflicts の行と同じ）。
    let side: Vec<String> = state.lines().skip(1).filter_map(|l| column(l, 0)).collect();
    assert!(
        side.iter()
            .any(|l| l.trim_start().starts_with("Conflicts") && l.ends_with(" 1")),
        "{side:?}"
    );
    let asked = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "A").unwrap();
    assert!(asked.contains("Abort the merge?   y abort   n cancel"));
    let aborted = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "Ay").unwrap();
    assert!(aborted.contains("Aborted"));
    assert!(!dir.path().join(".git/MERGE_HEAD").exists());
    // コンフリクトがなくなれば、行もなくなる。
    let side: Vec<String> = aborted
        .lines()
        .skip(1)
        .filter_map(|l| column(l, 0))
        .collect();
    assert!(!side.iter().any(|l| l.contains("Conflicts")), "{side:?}");
}

/// `Ctrl-b` でブランチを切り替える（アプリの Switch Branch…）: 打った語で絞り、最近のものを先に。合うものがなければ
/// その名前で作る
#[test]
fn switch_branch_with_ctrl_b() {
    let dir = tempfile::tempdir().unwrap();
    merged(dir.path());
    git(dir.path(), BASE, &["branch", "aaa"]);
    // topic に切り替えて戻った（最近のもの）。aaa は一度も切り替えていない。
    git(dir.path(), BASE, &["switch", "-q", "topic"]);
    git(dir.path(), BASE, &["switch", "-q", "main"]);
    let current = || git_out(dir.path(), &["branch", "--show-current"]);
    let list = amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), "\x02").unwrap();
    println!("{list}");
    assert!(list.contains(" Switch Branch "), "{list}");
    // 枠の中の行で比べる。
    let row = |name: &str| {
        list.lines()
            .position(|l| l.contains(&format!("│ {name} ")))
            .unwrap()
    };
    assert!(
        row("topic") < row("aaa"),
        "the recent branch comes first: {list}"
    );
    assert!(list.contains("topic  recent"), "{list}");
    // 絞って切り替える。
    amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), "\x02aa\n").unwrap();
    assert_eq!(current(), "aaa\n");
    // 合うものがなければ、その名前で作って切り替える。
    let new = amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), "\x02fresh").unwrap();
    assert!(new.contains("fresh") && new.contains("new branch"), "{new}");
    amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), "\x02fresh\n").unwrap();
    assert_eq!(current(), "fresh\n");
}

/// 失敗したときの答えが何行もあれば、一番下の行には 1 行目と「`!` で全部」、`!` で全部を枠に出す（アプリの Details）
#[test]
fn a_failure_shows_in_full_with_bang() {
    let dir = tempfile::tempdir().unwrap();
    diverged(dir.path());
    // 作業ツリーで a.txt を変えておくと、topic へのチェックアウトを git が何行かの理由で断る。
    std::fs::write(dir.path().join("a.txt"), "local\n").unwrap();
    let failed = amonhen_tui::snapshot(open(dir.path()), 200, 14, clock(), "0jjjc").unwrap();
    println!("{failed}");
    assert!(failed.contains("(! shows all)"), "{failed}");
    let all = amonhen_tui::snapshot(open(dir.path()), 200, 14, clock(), "0jjjc!").unwrap();
    println!("{all}");
    assert!(all.contains(" Details of the failure "), "{all}");
    assert!(all.contains("a.txt"), "{all}");
    assert_eq!(git_out(dir.path(), &["branch", "--show-current"]), "main\n");
}

/// サイドバーの `v` で ref に印をつけると、History はそれらから辿れるものだけになる（アプリの、いくつも選んだ ref の
/// History）。Esc で印を外す
#[test]
fn history_of_the_marked_refs() {
    let dir = tempfile::tempdir().unwrap();
    diverged(dir.path());
    // サイドバーの 3 つ下が topic、2 つ下が main。
    let topic = amonhen_tui::snapshot(open(dir.path()), 200, 12, clock(), "0jjjv").unwrap();
    println!("{topic}");
    assert!(
        topic.contains("History of topic (Esc clears the marks)"),
        "{topic}"
    );
    assert!(
        topic.contains("On topic") && topic.contains("Start"),
        "{topic}"
    );
    assert!(!topic.contains("On main"), "{topic}");
    let both = amonhen_tui::snapshot(open(dir.path()), 200, 12, clock(), "0jjjvkv").unwrap();
    assert!(both.contains("History of topic, main"), "{both}");
    assert!(
        both.contains("On topic") && both.contains("On main"),
        "{both}"
    );
    let all = amonhen_tui::snapshot(open(dir.path()), 200, 12, clock(), "0jjjv\x1b").unwrap();
    assert!(all.contains("History of all branches"), "{all}");
    assert!(all.contains("On main"), "{all}");
}

/// 印をつけた ref がいくつもあれば、サイドバーの `m` で 1 つの merge コミットで取り込み（octopus）、`D` でまとめて消す
#[test]
fn merge_and_delete_the_marked_refs() {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), BASE, &["init", "-q", "-b", "main"]);
    commit(dir.path(), BASE, "Start");
    for name in ["a1", "a2"] {
        git(dir.path(), BASE, &["switch", "-q", "-c", name, "main"]);
        std::fs::write(dir.path().join(format!("{name}.txt")), name).unwrap();
        git(dir.path(), BASE, &["add", "."]);
        git(
            dir.path(),
            BASE + DAY,
            &["commit", "-q", "-m", &format!("On {name}")],
        );
    }
    git(dir.path(), BASE, &["switch", "-q", "main"]);
    // main も進めておく（そうでなければ、git は最初の 1 つへ fast-forward してから取り込む）。
    commit(dir.path(), BASE + DAY * 2, "On main");
    // サイドバーのブランチは a1・a2・main の順（2 つ下が a1）。
    let asked = amonhen_tui::snapshot(open(dir.path()), 220, 12, clock(), "0jjvjvm").unwrap();
    println!("{asked}");
    assert!(
        asked.contains("Merge a1 (1), a2 (1) into main as one merge commit (octopus)? Undo brings it back.   y merge   n cancel"),
        "{asked}"
    );
    amonhen_tui::snapshot(open(dir.path()), 220, 12, clock(), "0jjvjvmy").unwrap();
    let parents = git_out(dir.path(), &["log", "-1", "--format=%P"]);
    assert_eq!(parents.split_whitespace().count(), 3, "{parents}");
    // 取り込んだので、まとめて消せる。
    let asked = amonhen_tui::snapshot(open(dir.path()), 220, 12, clock(), "0jjvjvD").unwrap();
    assert!(asked.contains("Delete 2 branches (a1, a2)?"), "{asked}");
    amonhen_tui::snapshot(open(dir.path()), 220, 12, clock(), "0jjvjvDy").unwrap();
    assert_eq!(
        git_out(dir.path(), &["branch", "--format=%(refname:short)"]),
        "main\n"
    );
}

/// サイドバーの `r` で、今のブランチを選んだ ref の上に rebase する
#[test]
fn rebase_onto_the_picked_ref() {
    let dir = tempfile::tempdir().unwrap();
    merged(dir.path());
    // topic に切り替えて（`c`）、main（1 つ上）の上に rebase する。
    amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "0jjjc").unwrap();
    let asked = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "0jjr").unwrap();
    println!("{asked}");
    // 見通しでコンフリクトを調べられなかったときは、何も足さない。
    assert!(asked.contains("Rebase topic onto main?   y rebase   n cancel"));
    // topic は main にもう入っていて、載せ直すコミットがない（前と後の絵は出ない）。
    assert!(!asked.contains(" Rebase topic onto main "), "{asked}");
    // main を topic に merge するのは fast-forward。取り込む数を言う。
    let ff = amonhen_tui::snapshot(open(dir.path()), 200, 12, clock(), "0jjm").unwrap();
    println!("{ff}");
    assert!(
        ff.contains("Fast-forward topic to main (+2)?   y merge   f fast-forward only   c always a merge commit   s squash into one commit   n cancel"),
        "{ff}"
    );
    assert!(ff.contains(" Fast-forward topic "), "{ff}");
    amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "0jjry").unwrap();
    assert_eq!(
        git_out(dir.path(), &["merge-base", "topic", "main"]),
        git_out(dir.path(), &["rev-parse", "main"])
    );
}

/// merge の取り込み方: fast-forward で済むときも、`c` で merge コミットを作る。`s` でまとめて 1 つのコミットにする
#[test]
fn merge_modes() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    // main に 2 つ足し、topic はその前にいる（main を merge すると fast-forward で済む）。
    git(dir, BASE, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("a.txt"), "a\n").unwrap();
    git(dir, BASE, &["add", "."]);
    git(dir, BASE, &["commit", "-q", "-m", "Start"]);
    git(dir, BASE, &["branch", "topic"]);
    for (i, name) in ["b", "c"].iter().enumerate() {
        std::fs::write(dir.join(format!("{name}.txt")), "x\n").unwrap();
        git(dir, BASE + DAY * (i as i64 + 1), &["add", "."]);
        git(
            dir,
            BASE + DAY * (i as i64 + 1),
            &["commit", "-q", "-m", &format!("Add {name}")],
        );
    }
    git(dir, BASE, &["switch", "-q", "topic"]);
    let parents = || {
        git_out(dir, &["log", "-1", "--format=%p"])
            .split_whitespace()
            .count()
    };
    let head = git_out(dir, &["rev-parse", "HEAD"]);
    // サイドバーで 2 つ下が main。
    let asked = amonhen_tui::snapshot(open(dir), 200, 12, clock(), "0jjm").unwrap();
    println!("{asked}");
    assert!(
        asked.contains("Fast-forward topic to main (+2)?"),
        "{asked}"
    );
    amonhen_tui::snapshot(open(dir), 200, 12, clock(), "0jjmc").unwrap();
    assert_eq!(parents(), 2, "a merge commit");
    // 戻して、まとめて 1 つのコミットに（merge コミットではなく、main の変更がすべて入る）。
    git(dir, BASE, &["reset", "-q", "--hard", head.trim()]);
    amonhen_tui::snapshot(open(dir), 200, 12, clock(), "0jjms").unwrap();
    assert_eq!(parents(), 1, "an ordinary commit");
    assert_ne!(git_out(dir, &["rev-parse", "HEAD"]), head);
    assert!(dir.join("c.txt").exists());
}

/// `s` で作業中の変更を stash し、`S` で戻す
#[test]
fn stash_and_pop() {
    let dir = tempfile::tempdir().unwrap();
    two_lines(dir.path());
    let read = |dir: &Path| std::fs::read_to_string(dir.join("a.txt")).unwrap();
    // 先にメッセージを聞く（Enter だけなら付けない）。Esc でやめる。
    let message = amonhen_tui::snapshot(open(dir.path()), 120, 10, clock(), "swip").unwrap();
    assert!(message.contains("Stash message (Enter for none): wip"));
    let left = amonhen_tui::snapshot(open(dir.path()), 120, 10, clock(), "swip\x1b").unwrap();
    assert!(!left.contains("Stash"));
    assert_eq!(git_out(dir.path(), &["stash", "list"]), "");
    let asked = amonhen_tui::snapshot(open(dir.path()), 200, 10, clock(), "1s\n").unwrap();
    assert!(
        asked.contains(
            "Stash the working changes?   y tracked files   u with untracked files   k keep what is staged   s only what is staged   f only a.txt   n cancel"
        ),
        "{asked}"
    );
    let keys = "sbefore lunch\ny";
    let stashed = amonhen_tui::snapshot(open(dir.path()), 100, 10, clock(), keys).unwrap();
    println!("{stashed}");
    assert!(stashed.contains("Stashed"));
    assert_eq!(read(dir.path()), "one\ntwo\nthree\nfour\n");
    assert!(git_out(dir.path(), &["stash", "list"]).contains(": before lunch\n"));
    // アプリと同じく、どの stash かを見せて確かめる。
    let asked = amonhen_tui::snapshot(open(dir.path()), 200, 10, clock(), "S").unwrap();
    assert!(
        asked.contains("Pop stash@{0} “On main: before lunch”? It goes back to the working tree and leaves the list. Undo brings it back.   y pop   n cancel"),
        "{asked}"
    );
    amonhen_tui::snapshot(open(dir.path()), 100, 10, clock(), "Sn").unwrap();
    assert_eq!(read(dir.path()), "one\ntwo\nthree\nfour\n");
    let popped = amonhen_tui::snapshot(open(dir.path()), 100, 10, clock(), "Sy").unwrap();
    assert!(popped.contains("Popped the stash"));
    assert_eq!(read(dir.path()), "ONE\ntwo\nTHREE\nfour\n");
    // stash がなければ、理由を出す。
    let none = amonhen_tui::snapshot(open(dir.path()), 100, 10, clock(), "S").unwrap();
    assert!(none.contains("No stash to pop"));
}

/// コンフリクトのあるファイル: マーカーつきの中身が出て、差分の中で `o` `t` `b` でそのコンフリクトを解決する
#[test]
fn conflicts_are_resolved_by_side() {
    let dir = tempfile::tempdir().unwrap();
    diverged(dir.path());
    let read = |dir: &Path| std::fs::read_to_string(dir.join("a.txt")).unwrap();
    amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "0jjjmy").unwrap();
    assert!(read(dir.path()).contains("<<<<<<<"));
    // Working Copy を開くと、マーカーつきの中身。
    let shown = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "1").unwrap();
    println!("{shown}");
    let diff: Vec<String> = shown.lines().skip(1).filter_map(|l| column(l, 2)).collect();
    assert!(diff.iter().any(|l| l.contains("<<<<<<<")), "{diff:?}");
    assert!(diff.iter().any(|l| l.ends_with("main")));
    assert!(diff.iter().any(|l| l.ends_with("topic")));
    // 差分へ移り、コンフリクトの中（マーカーの次の行）で `t`: Incoming の側にする。
    let resolved = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "1\tjjt").unwrap();
    println!("{resolved}");
    assert!(resolved.contains("Resolved"));
    assert_eq!(read(dir.path()), "topic\n");
}

/// コンフリクトのファイルで `m`: Magic Merge。規則で解けるものを解き（空白だけの違い）、解けないものは残す
#[test]
fn magic_merge_on_a_conflicted_file() {
    let dir = tempfile::tempdir().unwrap();
    let read = |dir: &Path| std::fs::read_to_string(dir.join("a.txt")).unwrap();
    git(dir.path(), BASE, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.path().join("a.txt"), "x = 1\n").unwrap();
    git(dir.path(), BASE, &["add", "."]);
    git(dir.path(), BASE, &["commit", "-q", "-m", "Start"]);
    git(dir.path(), BASE, &["switch", "-q", "-c", "topic"]);
    std::fs::write(dir.path().join("a.txt"), "x = 2\n").unwrap();
    git(dir.path(), BASE + DAY, &["commit", "-q", "-am", "On topic"]);
    git(dir.path(), BASE, &["switch", "-q", "main"]);
    std::fs::write(dir.path().join("a.txt"), "x =  2\n").unwrap();
    git(
        dir.path(),
        BASE + DAY * 2,
        &["commit", "-q", "-am", "On main"],
    );
    amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "0jjjmy").unwrap();
    assert!(read(dir.path()).contains("<<<<<<<"));
    let done = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "1m").unwrap();
    println!("{done}");
    assert!(done.contains("Magic Merge resolved 1; none left"));
    assert_eq!(read(dir.path()), "x =  2\n");
    // 両側が別に変えたものは解けない。
    let other = tempfile::tempdir().unwrap();
    diverged(other.path());
    amonhen_tui::snapshot(open(other.path()), 160, 12, clock(), "0jjjmy").unwrap();
    let none = amonhen_tui::snapshot(open(other.path()), 160, 12, clock(), "1m").unwrap();
    assert!(none.contains("Magic Merge found nothing it can resolve on its own"));
    assert!(read(other.path()).contains("<<<<<<<"));
}

/// `a` は、コンフリクトを共通の祖先の側にする。その側が入っていないコンフリクトでは、理由を出す。
#[test]
fn a_conflict_can_take_the_base_side() {
    let dir = tempfile::tempdir().unwrap();
    diverged(dir.path());
    let read = |dir: &Path| std::fs::read_to_string(dir.join("a.txt")).unwrap();
    amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "0jjjmy").unwrap();
    assert!(read(dir.path()).contains("<<<<<<<"));
    assert!(!read(dir.path()).contains("|||||||"));
    let refused = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "1\tjja").unwrap();
    println!("{refused}");
    assert!(refused.contains("This conflict has no base side"));
    assert!(read(dir.path()).contains("<<<<<<<"));
    // やり直して、diff3 の形で止める。
    git(dir.path(), BASE, &["merge", "--abort"]);
    git(
        dir.path(),
        BASE,
        &["config", "merge.conflictStyle", "diff3"],
    );
    amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "0jjjmy").unwrap();
    assert!(read(dir.path()).contains("|||||||"));
    let resolved = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "1\tjja").unwrap();
    println!("{resolved}");
    assert!(resolved.contains("Resolved"));
    assert_eq!(read(dir.path()), "one\n");
}

/// 一覧の `o` は、ファイルごと Current の側にして解決済みにする（先に確かめる）
#[test]
fn take_one_side_of_a_conflicted_file() {
    let dir = tempfile::tempdir().unwrap();
    diverged(dir.path());
    amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "0jjjmy").unwrap();
    let asked = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "1o").unwrap();
    assert!(asked.contains("Take the current side of a.txt?   y take   n cancel"));
    amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "1oy").unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "main\n"
    );
    // 解決済みになる（コンフリクトのファイルが残っていない）。
    assert_eq!(
        git_out(dir.path(), &["diff", "--name-only", "--diff-filter=U"]),
        ""
    );
}

/// サイドバーの Stashes: メッセージで並び、`a` で戻す、`x` で消す（どれも先に確かめる）
#[test]
fn stashes_are_listed_and_applied_from_the_sidebar() {
    let dir = tempfile::tempdir().unwrap();
    two_lines(dir.path());
    git(dir.path(), BASE, &["stash", "push", "-q", "-m", "Capitals"]);
    let read = |dir: &Path| std::fs::read_to_string(dir.join("a.txt")).unwrap();
    let text = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "").unwrap();
    println!("{text}");
    let side: Vec<String> = text.lines().skip(1).filter_map(|l| column(l, 0)).collect();
    // 見出しの右に stash の数。
    let at = side
        .iter()
        .position(|l| l.starts_with("Stashes") && l.ends_with(" 1"))
        .unwrap_or_else(|| panic!("{side:?}"));
    assert!(side[at + 1].contains("Capitals"), "{side:?}");
    // Enter で詳細に出し、一番下の行に様子（作ったブランチ・HEAD との関係・土台）を書く。
    let base = git_out(dir.path(), &["rev-parse", "--short=8", "HEAD"]);
    let about = amonhen_tui::snapshot(open(dir.path()), 200, 12, clock(), "0G\n").unwrap();
    println!("{about}");
    assert!(
        about.contains(&format!(
            "stash@{{0}}: made on main · at HEAD · base {}",
            base.trim()
        )),
        "{about}"
    );
    // 一番下の行（stash）で `a`: 変更が戻り、stash は残る。
    let asked = amonhen_tui::snapshot(open(dir.path()), 200, 12, clock(), "0Ga").unwrap();
    assert!(
        asked.contains("Apply stash@{0} “On main: Capitals”? It goes back to the working tree and stays in the list."),
        "{asked}"
    );
    let applied = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "0Gay").unwrap();
    assert!(applied.contains("Applied the stash"));
    assert_eq!(read(dir.path()), "ONE\ntwo\nTHREE\nfour\n");
    assert_eq!(git_out(dir.path(), &["stash", "list"]).lines().count(), 1);
    let asked = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "0Gx").unwrap();
    assert!(asked.contains("Drop stash@{0}? Undo brings it back.   y drop   n cancel"));
    amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "0Gxy").unwrap();
    assert_eq!(git_out(dir.path(), &["stash", "list"]), "");
}

/// 1 行ずつ足す 4 つのコミット（One から Four）。
fn four_lines(dir: &Path) {
    git(dir, BASE, &["init", "-q", "-b", "main"]);
    let mut text = String::new();
    for (i, line) in ["One", "Two", "Three", "Four"].iter().enumerate() {
        text += &format!("{line}\n");
        std::fs::write(dir.join("a.txt"), &text).unwrap();
        git(dir, BASE + DAY * i as i64, &["add", "."]);
        git(dir, BASE + DAY * i as i64, &["commit", "-q", "-m", line]);
    }
}

/// History の `E`: 選んだコミットから HEAD までを並べ、行ごとにすることを選んで、確かめてからまとめて当てる
#[test]
fn edit_mode_applies_several_edits_at_once() {
    let dir = tempfile::tempdir().unwrap();
    four_lines(dir.path());
    let log = || git_out(dir.path(), &["log", "--format=%s"]);
    // Two を選んで `E`: Four・Three・Two の 3 行。
    let opened = amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), "jjE").unwrap();
    println!("{opened}");
    assert!(opened.contains(" Edit Commits "), "{opened}");
    assert!(opened.contains("No changes yet."));
    assert_eq!(opened.matches("Keep").count(), 3, "{opened}");
    // Three を Two にまとめ（メッセージは Two のまま）、Four を取り消す。
    let keys = "jjEjfkd";
    let chosen = amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), keys).unwrap();
    println!("{chosen}");
    assert!(
        chosen.contains("Combine with Older, Keep Its Message"),
        "{chosen}"
    );
    assert!(
        chosen.contains("Changes: 1 combined and 1 dropped."),
        "{chosen}"
    );
    // Enter で見通しを読んでから確かめる。やめれば何も変わらない。
    let asked =
        amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), &format!("{keys}\n")).unwrap();
    let line = asked.lines().last().unwrap();
    assert!(
        line.contains("Apply these changes? Changes: 1 combined and 1 dropped."),
        "{line}"
    );
    // 確かめる間は、前と後の絵を重ねる（前の 3 つが、後では Two 1 つに。土台は One）。
    println!("{asked}");
    assert!(asked.contains(" Apply These Changes? "), "{asked}");
    let row = |subject: &str| {
        asked
            .lines()
            .find(|l| l.contains(subject))
            .unwrap_or("")
            .to_owned()
    };
    // 編集モードの行にも件名が出るので、絵の行（短い ID の後）で探す。
    assert!(row(" Four  [main]").contains("drop"), "{asked}");
    assert!(row(" Three  fixup").contains('●'), "{asked}");
    assert!(row("+1 combined").contains("Two"), "{asked}");
    assert_eq!(asked.matches("One  unchanged").count(), 2, "{asked}");
    assert!(line.contains("Undo brings it all back."), "{line}");
    amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), &format!("{keys}\nn")).unwrap();
    assert_eq!(log(), "Four\nThree\nTwo\nOne\n");
    let done =
        amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), &format!("{keys}\ny")).unwrap();
    assert!(!done.contains(" Edit Commits "), "{done}");
    assert_eq!(log(), "Two\nOne\n");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "One\nTwo\nThree\n"
    );
    // Undo 1 回で全部戻る。
    amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), "uy").unwrap();
    assert_eq!(log(), "Four\nThree\nTwo\nOne\n");
}

/// 編集モードの並べ替え（`J` `K`）と、Esc でやめる（変更があれば 2 度）
#[test]
fn edit_mode_reorders_and_cancels() {
    let dir = tempfile::tempdir().unwrap();
    four_lines(dir.path());
    let log = || git_out(dir.path(), &["log", "--format=%s"]);
    // Three を選んで `E`、Four を 1 つ下へ。
    let moved = amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), "jEJ").unwrap();
    assert!(moved.contains("Changes: reordered."), "{moved}");
    let once = amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), "jEJ\u{1b}").unwrap();
    assert!(
        once.contains("Press Esc again to discard these changes"),
        "{once}"
    );
    assert!(once.contains(" Edit Commits "));
    let twice =
        amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), "jEJ\u{1b}\u{1b}").unwrap();
    assert!(!twice.contains(" Edit Commits "), "{twice}");
    assert_eq!(log(), "Four\nThree\nTwo\nOne\n");
    // どれも同じファイルの続く行を足すので、入れ替えるとコンフリクトしそうだと先に言う。
    let asked = amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), "jEJ\n").unwrap();
    let line = asked.lines().last().unwrap();
    assert!(
        line.contains("Apply these changes? Changes: reordered."),
        "{line}"
    );
    if predicts_conflicts(dir.path()) {
        assert!(line.contains("Conflicts are likely in a.txt."), "{line}");
    }
}

/// サイドバーの stash で `b`: 名前を聞いて、stash を作ったところにブランチを作り、そこに戻す（git stash branch）
#[test]
fn a_branch_is_made_from_a_stash() {
    let dir = tempfile::tempdir().unwrap();
    two_lines(dir.path());
    git(dir.path(), BASE, &["stash", "push", "-q", "-m", "Capitals"]);
    let asked = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "0Gbwip").unwrap();
    let line = asked.lines().last().unwrap();
    assert!(
        line.contains("New branch where stash@{0} was made, with it applied: wip"),
        "{line}"
    );
    let done = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "0Gbwip\n").unwrap();
    println!("{done}");
    assert!(
        done.contains("Applied the stash on the new branch"),
        "{done}"
    );
    assert_eq!(git_out(dir.path(), &["branch", "--show-current"]), "wip\n");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "ONE\ntwo\nTHREE\nfour\n"
    );
    // 戻せた stash は消える。
    assert_eq!(git_out(dir.path(), &["stash", "list"]), "");
}

/// `O`: 変更のないファイルも、パスを打って（Tab で補って）File History を開く。File の画面の `B` `L` で Blame と
/// File History を切り替える
#[test]
fn open_a_file_by_path() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    git(dir, BASE, &["init", "-q", "-b", "main"]);
    std::fs::create_dir(dir.join("src")).unwrap();
    for (i, file) in ["src/lib.rs", "src/main.rs", "README"].iter().enumerate() {
        std::fs::write(dir.join(file), "x\n").unwrap();
        git(dir, BASE + DAY * i as i64, &["add", "."]);
        git(
            dir,
            BASE + DAY * i as i64,
            &["commit", "-q", "-m", &format!("Add {file}")],
        );
    }
    let last = |text: &str| text.lines().last().unwrap().to_owned();
    // 1 つに決まるところまで補う（フォルダには `/`）。
    let folder = amonhen_tui::snapshot(open(dir), 160, 12, clock(), "Osr\t").unwrap();
    assert!(
        last(&folder).contains("File History of (Tab completes): src/"),
        "{folder}"
    );
    // いくつもあれば、共通のところまで埋めて候補を出す。
    let several = amonhen_tui::snapshot(open(dir), 160, 12, clock(), "Osrc/\t").unwrap();
    println!("{several}");
    assert!(several.contains("lib.rs  main.rs"), "{several}");
    let file = amonhen_tui::snapshot(open(dir), 160, 12, clock(), "Osr\tl\t").unwrap();
    assert!(last(&file).contains("src/lib.rs"), "{file}");
    // Enter で File History（変更のないファイルでも、そのファイルを変えたコミット）。
    let history = amonhen_tui::snapshot(open(dir), 160, 12, clock(), "Osr\tl\t\n").unwrap();
    println!("{history}");
    assert!(history.contains("Add src/lib.rs"), "{history}");
    assert!(!history.contains("Add README"), "{history}");
    // `B` で同じファイルの Blame、`L` で戻る。
    let blame = amonhen_tui::snapshot(open(dir), 160, 12, clock(), "Osr\tl\t\nB").unwrap();
    println!("{blame}");
    assert!(blame.contains("Blame"), "{blame}");
    let back = amonhen_tui::snapshot(open(dir), 160, 12, clock(), "Osr\tl\t\nBL").unwrap();
    assert!(back.contains("Add src/lib.rs"), "{back}");
    assert!(!back.contains("Blame"), "{back}");
}

/// `3` で Reflog: HEAD が動いた記録が新しい順に並び、選んだ行のコミットの詳細が出る
#[test]
fn reflog_lists_where_head_was() {
    let dir = tempfile::tempdir().unwrap();
    merged(dir.path());
    let text = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "3").unwrap();
    println!("{text}");
    let list: Vec<String> = text.lines().skip(1).filter_map(|l| column(l, 1)).collect();
    assert!(list[0].starts_with("HEAD@{0}   merge topic"), "{list:?}");
    assert!(list[1].starts_with("HEAD@{1}   commit: Bump the version"));
    // 1 つ下を選ぶと、そのコミットの詳細。
    let next = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "3j").unwrap();
    let details: Vec<String> = next.lines().filter_map(|l| column(l, 2)).collect();
    assert_eq!(details[0], "Bump the version");
    assert!(next.lines().last().unwrap().ends_with("2/7"), "{next}");
}

/// Reflog の `c` でそこをチェックアウト（先に確かめる）、`H` で History で選ぶ、`Y` で ID をコピー
#[test]
fn reflog_entries_check_out_show_in_history_and_copy() {
    let dir = tempfile::tempdir().unwrap();
    merged(dir.path());
    let bump = git_out(dir.path(), &["rev-parse", "HEAD@{1}"]);
    let bump = bump.trim();
    // 2 行目（commit: Bump the version）を、History で選ぶ。
    let shown = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "3jH").unwrap();
    println!("{shown}");
    let details: Vec<String> = shown.lines().filter_map(|l| column(l, 2)).collect();
    assert_eq!(details[0], "Bump the version", "{shown}");
    assert!(shown.lines().last().unwrap().ends_with("2/5"), "{shown}");
    assert!(!shown.contains("HEAD@{1}"), "History is shown: {shown}");
    // ID をコピーする。
    let copied = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "3jY").unwrap();
    assert!(
        copied.contains(&format!(
            "Copied {} (to the terminal’s clipboard)",
            &bump[..8]
        )),
        "{copied}"
    );
    // そこをチェックアウトする（detached HEAD）。
    let asked = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "3jc").unwrap();
    assert!(
        asked.contains(&format!(
            "Check out {} (detached HEAD)?   y check out   n cancel",
            &bump[..8]
        )),
        "{asked}"
    );
    amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "3jcy").unwrap();
    assert_eq!(git_out(dir.path(), &["rev-parse", "HEAD"]).trim(), bump);
    assert_eq!(git_out(dir.path(), &["branch", "--show-current"]), "");
}

/// サイドバーのブランチで `L`: そのブランチの reflog。`3` で HEAD のものに戻る。
#[test]
fn reflog_of_the_selected_branch() {
    let dir = tempfile::tempdir().unwrap();
    merged(dir.path());
    // サイドバーの 3 つ下が topic。
    let text = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "0jjjL").unwrap();
    println!("{text}");
    let list: Vec<String> = text.lines().skip(1).filter_map(|l| column(l, 1)).collect();
    assert!(list[0].starts_with("topic@{0}"), "{list:?}");
    assert!(
        list[0].contains("commit: 日本語の件名のコミット"),
        "{list:?}"
    );
    assert!(text.lines().last().unwrap().contains("Reflog of topic"));
    // HEAD の reflog より短い（topic が動いたのは、作ったときとコミットの 2 回）。
    assert!(text.lines().last().unwrap().ends_with("1/2"), "{text}");
    let head = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "0jjjL3").unwrap();
    let list: Vec<String> = head.lines().skip(1).filter_map(|l| column(l, 1)).collect();
    assert!(list[0].starts_with("HEAD@{0}"), "{list:?}");
    assert!(!head.contains("Reflog of"));
    // ブランチでない行では、理由を出す。
    let none = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "0L").unwrap();
    assert!(none.contains("Select a branch to see its reflog"));
}

/// `R` は、今のブランチを選んでいるコミットへ動かす。どう動かすかを選ばせる
#[test]
fn reset_offers_the_modes() {
    let dir = tempfile::tempdir().unwrap();
    merged(dir.path());
    let head = || git_out(dir.path(), &["log", "-1", "--format=%s"]);
    // 1 つ下（Bump the version）を選んで `R`。
    let asked = amonhen_tui::snapshot(open(dir.path()), 160, 10, clock(), "jR").unwrap();
    println!("{asked}");
    let last = asked.lines().last().unwrap();
    assert!(last.contains("Reset main to "), "{last}");
    // 外れるコミット（merge と topic の 1 つ）の数を先に見せる。
    assert!(
        last.contains(" (−2 +0 commits)? Undo brings it back."),
        "{last}"
    );
    assert!(last.contains("s soft (keep the index and files)   m mixed (keep the files)"));
    // アプリのシートと同じく、前と後の絵を重ねる（前は外れる 2 つ、後は何も加わらず、main は土台に付く）。
    assert!(asked.contains(" Reset main "), "{asked}");
    assert!(asked.contains("Before"), "{asked}");
    assert_eq!(asked.matches("leaves the branch").count(), 2, "{asked}");
    assert!(
        asked.contains("Bump the version  [main]  unchanged"),
        "{asked}"
    );
    // ほかのキーでやめる。
    amonhen_tui::snapshot(open(dir.path()), 160, 10, clock(), "jRn").unwrap();
    assert_eq!(head(), "Merge branch 'topic'\n");
    let done = amonhen_tui::snapshot(open(dir.path()), 160, 10, clock(), "jRh").unwrap();
    assert!(done.contains("Reset"));
    assert_eq!(head(), "Bump the version\n");
    // Undo で戻る。
    amonhen_tui::snapshot(open(dir.path()), 160, 10, clock(), "uy").unwrap();
    assert_eq!(head(), "Merge branch 'topic'\n");
}

/// コマンドの一覧の Activity: 操作ログを新しい順に出し、選んだところまで確かめてから戻す
#[test]
fn operation_log_undoes_back_to_the_selected_one() {
    let dir = tempfile::tempdir().unwrap();
    merged(dir.path());
    let head = || git_out(dir.path(), &["log", "-1", "--format=%s"]);
    let none = amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), ":activity\n").unwrap();
    assert!(none.contains("Nothing to undo"), "{none}");
    // 2 回 reset する（Bump the version へ、次に Add the graph へ。merge はタグ v1 があるので残る）。
    amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), "jRs").unwrap();
    assert_eq!(head(), "Bump the version\n");
    amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), "jjjRs").unwrap();
    let moved = head();
    assert_eq!(moved, "Add the graph\n");
    let log = amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), ":activity\n").unwrap();
    println!("{log}");
    assert!(log.contains(" Operation Log "), "{log}");
    assert_eq!(log.matches("Reset").count(), 2, "{log}");
    assert!(log.contains("Enter undoes back to the selected one"));
    // 1 つ目だけなら、その名前で確かめる。
    let one = amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), ":activity\n\n").unwrap();
    assert!(one.lines().last().unwrap().contains("Undo “Reset"), "{one}");
    // 2 つ目まで選ぶと、いくつ戻すかで確かめる。やめれば何も変わらない。
    let two = amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), ":activity\nj\n").unwrap();
    assert!(
        two.lines()
            .last()
            .unwrap()
            .contains("Undo the last 2 operations?   y undo   n cancel"),
        "{two}"
    );
    amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), ":activity\nj\nn").unwrap();
    assert_eq!(head(), moved);
    amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), ":activity\nj\ny").unwrap();
    assert_eq!(head(), "Merge branch 'topic'\n");
}

/// 上流が持っているコミットを外す reset では、push に Force Push が要ると先に言う
#[test]
fn reset_warns_when_the_upstream_has_the_commits() {
    let dir = tempfile::tempdir().unwrap();
    let work = with_remote(dir.path());
    git(&work, BASE, &["push", "-q"]);
    // 1 つ下（Start）を選んで `R`。
    let asked = amonhen_tui::snapshot(open(&work), 260, 10, clock(), "jR").unwrap();
    let last = asked.lines().last().unwrap();
    assert!(last.contains(" (−1 +0 commits)?"), "{last}");
    assert!(
        last.contains(
            "origin/main has 1 of the commits it leaves, so pushing will need Force Push."
        ),
        "{last}"
    );
    // 上流にないコミットだけを外すなら、言わない。
    commit(&work, BASE + 2 * DAY, "Local only");
    let local = amonhen_tui::snapshot(open(&work), 260, 10, clock(), "jR").unwrap();
    let last = local.lines().last().unwrap();
    assert!(last.contains(" (−1 +0 commits)?"), "{last}");
    assert!(!last.contains("Force Push"), "{last}");
}

/// `M` でマウスの取り込みを切り替える
#[test]
fn m_toggles_mouse_capture() {
    let dir = tempfile::tempdir().unwrap();
    merged(dir.path());
    let off = amonhen_tui::snapshot(open(dir.path()), 80, 8, clock(), "M").unwrap();
    assert!(off.contains("Mouse off: the terminal selects text"));
    let on = amonhen_tui::snapshot(open(dir.path()), 80, 8, clock(), "MM").unwrap();
    assert!(on.contains("Mouse on"));
}

/// 差分で `v` を押して動かすと、行の範囲を選ぶ。Space はその中の変更すべてを stage する
#[test]
fn v_selects_a_range_of_lines() {
    let dir = tempfile::tempdir().unwrap();
    two_lines(dir.path());
    // 「-one」で `v`、「+THREE」まで動かして Space（間の「two」は変更ではない）。
    let text = amonhen_tui::snapshot(open(dir.path()), 80, 12, clock(), "1\njjvjjjj").unwrap();
    println!("{text}");
    // 範囲の行すべてに印。
    assert_eq!(text.lines().filter(|l| l.starts_with('▌')).count(), 5);
    amonhen_tui::snapshot(open(dir.path()), 80, 12, clock(), "1\njjvjjjj ").unwrap();
    assert_eq!(git_out(dir.path(), &["diff", "--name-only"]), "");
    assert_eq!(
        git_out(dir.path(), &["diff", "--cached", "--name-only"]),
        "a.txt\n"
    );
}

/// サイドバーで stash を開くと、その差分が詳細の枠に出る
#[test]
fn opening_a_stash_shows_its_diff() {
    let dir = tempfile::tempdir().unwrap();
    two_lines(dir.path());
    git(dir.path(), BASE, &["stash", "push", "-q", "-m", "Capitals"]);
    let text = amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), "0G\n").unwrap();
    println!("{text}");
    let details: Vec<String> = text.lines().filter_map(|l| column(l, 2)).collect();
    assert!(details[0].contains("Capitals"), "{details:?}");
    assert!(details.iter().any(|l| l == "M a.txt +2 -2"), "{details:?}");
    assert!(details.iter().any(|l| l.ends_with("+ONE")));
    // History を動かすと、選んでいるコミットの詳細に戻る。
    let back = amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), "0G\n\x1bj").unwrap();
    let details: Vec<String> = back.lines().filter_map(|l| column(l, 2)).collect();
    assert_eq!(details[0], "Start");
}

/// worktree が 2 つ以上あれば、サイドバーに並ぶ
#[test]
fn worktrees_are_listed_in_the_sidebar() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main");
    std::fs::create_dir(&main).unwrap();
    merged(&main);
    let other = dir.path().join("other");
    git(
        &main,
        BASE,
        &["worktree", "add", "-q", other.to_str().unwrap(), "topic"],
    );
    let text = amonhen_tui::snapshot(open(&main), 160, 16, clock(), "").unwrap();
    println!("{text}");
    let side: Vec<String> = text.lines().skip(1).filter_map(|l| column(l, 0)).collect();
    let at = side.iter().position(|l| l == "Worktrees").unwrap();
    assert_eq!(side[at + 1], "● main (main)");
    assert_eq!(side[at + 2], "  other (topic)");
}

/// コマンドの一覧の New Orphan Branch: 名前を聞き、空からか、今のファイルを stage したままかを選ぶ
#[test]
fn new_orphan_branch_from_the_command_list() {
    let dir = tempfile::tempdir().unwrap();
    two_lines(dir.path());
    let branch = || git_out(dir.path(), &["branch", "--show-current"]);
    let asked =
        amonhen_tui::snapshot(open(dir.path()), 140, 10, clock(), ":orphan\npages\n").unwrap();
    println!("{asked}");
    assert!(asked.contains(
        "Start pages with no history?   e empty   k keeping the current files staged   n cancel"
    ));
    // 作業中の変更があると、空からは git が断る。今のファイルを残すほうは作れる。
    let refused =
        amonhen_tui::snapshot(open(dir.path()), 140, 10, clock(), ":orphan\npages\ne").unwrap();
    println!("{refused}");
    assert!(!refused.contains("Created the branch"));
    assert_eq!(branch(), "main\n");
    let kept =
        amonhen_tui::snapshot(open(dir.path()), 140, 10, clock(), ":orphan\npages\nk").unwrap();
    assert!(kept.contains("Created the branch"));
    assert_eq!(branch(), "pages\n");
    // 履歴はなく、ファイルはすべて新しく足すものとして index にある（作業中の変更は、そのまま残る）。
    assert_eq!(
        git_out(dir.path(), &["status", "--porcelain"]),
        "AM a.txt\n"
    );
}

/// コマンドの一覧の Clean Up Branches: 既定のブランチに取り込み済みのブランチを、確かめてからまとめて消す
#[test]
fn clean_up_merged_branches() {
    let dir = tempfile::tempdir().unwrap();
    merged(dir.path());
    let branches = || git_out(dir.path(), &["branch", "--format=%(refname:short)"]);
    assert_eq!(branches(), "main\ntopic\n");
    let asked = amonhen_tui::snapshot(open(dir.path()), 140, 10, clock(), ":clean up\n").unwrap();
    println!("{asked}");
    assert!(asked.contains(
        "Clean up topic? Undo brings them back.   y delete those merged into main   n cancel"
    ));
    let done = amonhen_tui::snapshot(open(dir.path()), 140, 10, clock(), ":clean up\ny").unwrap();
    assert!(done.contains("Deleted the branches"));
    assert_eq!(branches(), "main\n");
    // 片付けるものがなければ、そう知らせる。
    let none = amonhen_tui::snapshot(open(dir.path()), 140, 10, clock(), ":clean up\n").unwrap();
    assert!(none.contains("No branches to clean up"));
    // Undo で戻る。
    amonhen_tui::snapshot(open(dir.path()), 140, 10, clock(), "uy").unwrap();
    assert_eq!(branches(), "main\ntopic\n");
}

/// 上流が消えただけのブランチは最初は選ばず、`a` で含める
#[test]
fn clean_up_offers_branches_whose_upstream_is_gone() {
    let dir = tempfile::tempdir().unwrap();
    let work = with_remote(dir.path());
    git(&work, BASE, &["switch", "-q", "-c", "feature"]);
    commit(&work, BASE + DAY, "Feature work");
    git(&work, BASE, &["push", "-q", "-u", "origin", "feature"]);
    git(&work, BASE, &["switch", "-q", "main"]);
    git(
        &dir.path().join("origin.git"),
        BASE,
        &["branch", "-q", "-D", "feature"],
    );
    git(&work, BASE, &["fetch", "-q", "--prune"]);
    let asked = amonhen_tui::snapshot(open(&work), 160, 10, clock(), ":clean up\n").unwrap();
    println!("{asked}");
    assert!(asked.contains("Clean up feature? Undo brings them back."));
    assert!(asked.contains("a delete those whose upstream is gone (1)"));
    assert!(!asked.contains("y delete"));
    amonhen_tui::snapshot(open(&work), 160, 10, clock(), ":clean up\na").unwrap();
    assert_eq!(
        git_out(&work, &["branch", "--format=%(refname:short)"]),
        "main\n"
    );
}

/// コマンドの一覧の Add Remote: 名前、次に URL を聞いて足す。同じ名前のリモートがあれば断る
#[test]
fn add_remote_from_the_command_list() {
    let dir = tempfile::tempdir().unwrap();
    let work = with_remote(dir.path());
    let url = dir.path().join("origin.git");
    let url = url.to_str().unwrap();
    let named =
        amonhen_tui::snapshot(open(&work), 160, 8, clock(), ":add remote\nmirror\n").unwrap();
    println!("{named}");
    assert!(named.contains("URL of mirror: "));
    let keys = format!(":add remote\nmirror\n{url}\n");
    let added = amonhen_tui::snapshot(open(&work), 160, 8, clock(), &keys).unwrap();
    assert!(added.contains("Added the remote"));
    assert_eq!(git_out(&work, &["remote"]), "mirror\norigin\n");
    assert_eq!(
        git_out(&work, &["remote", "get-url", "mirror"]),
        format!("{url}\n")
    );
    let taken =
        amonhen_tui::snapshot(open(&work), 160, 8, clock(), ":add remote\norigin\n").unwrap();
    assert!(taken.contains("There is a remote named origin"));
    // リモートがまだなければ、初めは origin。
    let empty = tempfile::tempdir().unwrap();
    merged(empty.path());
    let first =
        amonhen_tui::snapshot(open(empty.path()), 160, 8, clock(), ":add remote\n").unwrap();
    assert!(first.contains("New remote's name: origin"));
}

/// コマンドの一覧の Update Submodules: すべての submodule を取り出す。なければ、そう知らせる
#[test]
fn update_submodules_from_the_command_list() {
    let dir = tempfile::tempdir().unwrap();
    let (sub, main, clone) = (
        dir.path().join("sub"),
        dir.path().join("main"),
        dir.path().join("clone"),
    );
    for d in [&sub, &main] {
        std::fs::create_dir(d).unwrap();
        git(d, BASE, &["init", "-q", "-b", "main"]);
        commit(d, BASE, "Start");
    }
    let file = ["-c", "protocol.file.allow=always"];
    let add = [
        &file[..],
        &["submodule", "add", "-q", sub.to_str().unwrap(), "lib"],
    ]
    .concat();
    git(&main, BASE, &add);
    git(&main, BASE, &["commit", "-q", "-m", "Add lib"]);
    git(
        dir.path(),
        BASE,
        &["clone", "-q", main.to_str().unwrap(), "clone"],
    );
    git(&clone, BASE, &["config", "protocol.file.allow", "always"]);
    assert!(!clone.join("lib/.git").exists());
    let asked = amonhen_tui::snapshot(open(&clone), 160, 8, clock(), ":update sub\n").unwrap();
    println!("{asked}");
    assert!(asked.contains("Update lib?   y to the recorded commits   r recursively   t to their branches' latest   n cancel"));
    let done = amonhen_tui::snapshot(open(&clone), 160, 8, clock(), ":update sub\ny").unwrap();
    println!("{done}");
    assert!(done.contains("Updated the submodules"));
    assert!(clone.join("lib/.git").exists());
    let none = amonhen_tui::snapshot(open(&sub), 160, 8, clock(), ":update sub\n").unwrap();
    assert!(none.contains("There are no submodules"));
}

/// コマンドの一覧の Delete Ignored Files: 数と名前を出して確かめ、ignore されたものを消す
#[test]
fn delete_ignored_files_from_the_command_list() {
    let dir = tempfile::tempdir().unwrap();
    merged(dir.path());
    let none = amonhen_tui::snapshot(open(dir.path()), 160, 8, clock(), ":ignored\n").unwrap();
    assert!(none.contains("No ignored files"));
    std::fs::write(dir.path().join(".gitignore"), "*.log\nbuild/\n").unwrap();
    std::fs::write(dir.path().join("a.log"), "x").unwrap();
    std::fs::create_dir(dir.path().join("build")).unwrap();
    std::fs::write(dir.path().join("build/out"), "x").unwrap();
    std::fs::write(dir.path().join("keep.txt"), "x").unwrap();
    let asked = amonhen_tui::snapshot(open(dir.path()), 160, 8, clock(), ":ignored\n").unwrap();
    println!("{asked}");
    assert!(
        asked.contains("Delete 2 ignored items (a.log and build/)? Undo can’t bring them back.")
    );
    assert!(dir.path().join("a.log").exists());
    amonhen_tui::snapshot(open(dir.path()), 160, 8, clock(), ":ignored\ny").unwrap();
    assert!(!dir.path().join("a.log").exists());
    assert!(!dir.path().join("build").exists());
    assert!(dir.path().join("keep.txt").exists());
}

/// コマンドの一覧の Export Archive: History で選んでいるコミットのファイルを、拡張子の形で書き出す
#[test]
fn export_archive_of_the_selected_commit() {
    let dir = tempfile::tempdir().unwrap();
    changed(dir.path());
    let asked = amonhen_tui::snapshot(open(dir.path()), 200, 8, clock(), ":export\n").unwrap();
    println!("{asked}");
    assert!(asked.contains("Export "));
    assert!(asked.contains(".zip"));
    let out = tempfile::tempdir().unwrap();
    let zip = out.path().join("snap.zip");
    let keys = format!(":export\n\x15{}\n", zip.display());
    // フォルダに入れるか、先頭に置くか、1 つのフォルダだけにするかを選ぶ。
    let choose = amonhen_tui::snapshot(open(dir.path()), 240, 8, clock(), &keys).unwrap();
    println!("{choose}");
    assert!(
        choose.contains(
            "y in a folder snap/   t at the top, without a folder   o only one folder…   n cancel"
        ),
        "{choose}"
    );
    let done =
        amonhen_tui::snapshot(open(dir.path()), 200, 8, clock(), &format!("{keys}y")).unwrap();
    println!("{done}");
    assert!(done.contains("Exported to"));
    let bytes = std::fs::read(&zip).unwrap();
    assert_eq!(&bytes[..2], b"PK");
    // 中身は、ファイルの名前のフォルダの下に。
    assert!(bytes.windows(11).any(|w| w == b"snap/lane.r"));
    // 先頭に置く・1 つのフォルダだけ（tar で中身の名前を見る）。
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    std::fs::write(dir.path().join("sub/x.txt"), "x\n").unwrap();
    git(dir.path(), BASE, &["add", "."]);
    git(dir.path(), BASE + DAY * 5, &["commit", "-q", "-m", "Sub"]);
    let names = |file: &Path| {
        let listed = amonhen_model::process::command("tar")
            .arg("-tf")
            .arg(file)
            .output()
            .unwrap();
        String::from_utf8(listed.stdout).unwrap()
    };
    let top = out.path().join("top.tar");
    let keys = format!(":export\n\x15{}\nt", top.display());
    amonhen_tui::snapshot(open(dir.path()), 200, 8, clock(), &keys).unwrap();
    let listed = names(&top);
    assert!(listed.lines().any(|l| l == "lane.rs"), "{listed}");
    let one = out.path().join("one.tar");
    let keys = format!(":export\n\x15{}\nosub\ny", one.display());
    amonhen_tui::snapshot(open(dir.path()), 200, 8, clock(), &keys).unwrap();
    let listed = names(&one);
    assert!(listed.lines().any(|l| l == "one/sub/x.txt"), "{listed}");
    assert!(!listed.contains("lane.rs"), "{listed}");
    // 分からない拡張子は断る。
    let keys = format!(":export\n\x15{}\n", out.path().join("snap.rar").display());
    let wrong = amonhen_tui::snapshot(open(dir.path()), 200, 8, clock(), &keys).unwrap();
    assert!(wrong.contains("End the file name with .zip, .tar.gz or .tar"));
}

/// コマンドの一覧の Save Repository as Bundle: HEAD とすべてのブランチ・タグを 1 つのファイルに（clone できる）
#[test]
fn save_the_repository_as_a_bundle() {
    let dir = tempfile::tempdir().unwrap();
    merged(dir.path());
    let asked = amonhen_tui::snapshot(open(dir.path()), 200, 8, clock(), ":bundle\n").unwrap();
    assert!(asked.contains("Save the repository as a bundle at: "));
    assert!(asked.contains(".bundle"));
    let out = tempfile::tempdir().unwrap();
    let file = out.path().join("all.bundle");
    let keys = format!(":bundle\n\x15{}\n", file.display());
    // 何を入れるかを選ぶ。
    let choose = amonhen_tui::snapshot(open(dir.path()), 240, 8, clock(), &keys).unwrap();
    println!("{choose}");
    assert!(
        choose.contains(
            "y HEAD and every branch and tag   b only main   a only main after a base…   n cancel"
        ),
        "{choose}"
    );
    let done =
        amonhen_tui::snapshot(open(dir.path()), 200, 8, clock(), &format!("{keys}y")).unwrap();
    println!("{done}");
    assert!(done.contains("Saved the bundle to"));
    // git が読めて、ブランチとタグが入っている。
    let heads = git_out(
        out.path(),
        &["bundle", "list-heads", file.to_str().unwrap()],
    );
    for name in ["refs/heads/main", "refs/heads/topic", "refs/tags/v1"] {
        assert!(heads.contains(name), "{heads}");
    }
    // 今のブランチだけ。
    let main_only = out.path().join("main.bundle");
    let keys = format!(":bundle\n\x15{}\nb", main_only.display());
    amonhen_tui::snapshot(open(dir.path()), 200, 8, clock(), &keys).unwrap();
    let heads = git_out(
        out.path(),
        &["bundle", "list-heads", main_only.to_str().unwrap()],
    );
    assert!(
        heads.contains("refs/heads/main") && !heads.contains("topic"),
        "{heads}"
    );
    // 今のブランチの、基点より後だけ（受け取る側が持っているはずのコミットは入れない）。
    let after = out.path().join("after.bundle");
    let keys = format!(":bundle\n\x15{}\na\x15main~1\n", after.display());
    amonhen_tui::snapshot(open(dir.path()), 200, 8, clock(), &keys).unwrap();
    let base = git_out(dir.path(), &["rev-parse", "main~1"]);
    let header = std::fs::read(&after).unwrap();
    let header = String::from_utf8_lossy(&header[..200.min(header.len())]).into_owned();
    assert!(header.contains(&format!("-{}", base.trim())), "{header}");
}

/// コマンドの一覧の Fetch from Bundle: ファイルを読み、名前を聞いて、そのブランチを refs/remotes/<名前>/ に取り込む
#[test]
fn fetch_from_a_bundle() {
    let dir = tempfile::tempdir().unwrap();
    let (from, to) = (dir.path().join("from"), dir.path().join("to"));
    std::fs::create_dir(&from).unwrap();
    merged(&from);
    let file = dir.path().join("work.bundle");
    git(
        &from,
        BASE,
        // `-q` は git 2.35 から（サポートする最低の git にはない）。
        &["bundle", "create", file.to_str().unwrap(), "--all"],
    );
    std::fs::create_dir(&to).unwrap();
    git(&to, BASE, &["init", "-q", "-b", "main"]);
    commit(&to, BASE, "Elsewhere");
    let keys = format!(":fetch from bundle\n\x15{}\n", file.display());
    let named = amonhen_tui::snapshot(open(&to), 200, 8, clock(), &keys).unwrap();
    println!("{named}");
    assert!(named.contains("Fetch its branches as remote: work"));
    let done = amonhen_tui::snapshot(open(&to), 200, 8, clock(), &format!("{keys}\n")).unwrap();
    println!("{done}");
    assert!(done.contains("Fetched"));
    let refs = git_out(
        &to,
        &["for-each-ref", "--format=%(refname)", "refs/remotes/work"],
    );
    assert!(refs.contains("refs/remotes/work/main"), "{refs}");
    assert!(refs.contains("refs/remotes/work/topic"), "{refs}");
    // bundle でないファイルは、エンジンが断る（理由が出る）。
    let keys = format!(
        ":fetch from bundle\n\x15{}\n",
        from.join("lane.rs").display()
    );
    let wrong = amonhen_tui::snapshot(open(&to), 200, 8, clock(), &keys).unwrap();
    assert!(!wrong.contains("Fetch its branches"));
}

/// コマンドの一覧の Apply Patch: メールの形はコミットとして（git am）、ただの diff は作業ツリーに当てる
#[test]
fn apply_patch_from_the_command_list() {
    let dir = tempfile::tempdir().unwrap();
    changed(dir.path());
    let out = tempfile::tempdir().unwrap();
    // いちばん新しいコミットをパッチにして、そのコミットを外す。
    let mail = out.path().join("change.patch");
    let patch = git_out(dir.path(), &["format-patch", "-1", "--stdout"]);
    std::fs::write(&mail, patch).unwrap();
    let subject = git_out(dir.path(), &["log", "-1", "--format=%s"]);
    git(dir.path(), BASE, &["reset", "-q", "--hard", "HEAD~1"]);
    let keys = format!(":apply patch\n\x15{}\n", mail.display());
    let asked = amonhen_tui::snapshot(open(dir.path()), 200, 8, clock(), &keys).unwrap();
    println!("{asked}");
    // 件名を見せてから確かめる。
    assert!(
        asked.contains(&format!(
            "Apply the patch to the current branch as a commit? (“{}”)   y apply   n cancel",
            subject.trim()
        )),
        "{asked}"
    );
    amonhen_tui::snapshot(open(dir.path()), 200, 8, clock(), &format!("{keys}y")).unwrap();
    assert_eq!(git_out(dir.path(), &["log", "-1", "--format=%s"]), subject);
    // ただの diff は作業ツリーだけに。
    // ただの diff は、git で作った作業ツリーの変更を、戻してから当てる。
    let diff = out.path().join("change.diff");
    let lane = dir.path().join("lane.rs");
    let before = std::fs::read_to_string(&lane).unwrap();
    std::fs::write(&lane, before.replace("u32", "u64")).unwrap();
    std::fs::write(&diff, git_out(dir.path(), &["diff"])).unwrap();
    std::fs::write(&lane, &before).unwrap();
    let keys = format!(":apply patch\n\x15{}\n", diff.display());
    let asked = amonhen_tui::snapshot(open(dir.path()), 220, 8, clock(), &keys).unwrap();
    println!("{asked}");
    // ファイルと行の数を見せ、作業ツリーだけか index にもかを選ぶ。
    assert!(
        asked.contains("Apply the diff? Undo brings the files back. (lane.rs +1 −1)   y to the working tree   i to the index too (staged)   n cancel"),
        "{asked}"
    );
    amonhen_tui::snapshot(open(dir.path()), 200, 8, clock(), &format!("{keys}y")).unwrap();
    assert_eq!(git_out(dir.path(), &["diff", "--name-only"]), "lane.rs\n");
    assert_eq!(
        git_out(dir.path(), &["diff", "--cached", "--name-only"]),
        ""
    );
    // もう当たらない（同じ diff をもう一度）。3-way で当てるかを聞く。
    let again = amonhen_tui::snapshot(open(dir.path()), 260, 8, clock(), &keys).unwrap();
    println!("{again}");
    assert!(again.contains("The patch doesn’t apply"), "{again}");
    assert!(again.contains("y apply 3-way   n cancel"), "{again}");
    // index にも当てる。
    git(dir.path(), BASE, &["checkout", "--", "lane.rs"]);
    amonhen_tui::snapshot(open(dir.path()), 220, 8, clock(), &format!("{keys}i")).unwrap();
    assert_eq!(
        git_out(dir.path(), &["diff", "--cached", "--name-only"]),
        "lane.rs\n"
    );
}

/// 当たらずに止まった git am のパッチを、`R` で 3-way merge で当て直す（アプリの Retry with 3-Way）
#[test]
fn retry_a_stopped_am_with_a_three_way_merge() {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), BASE, &["init", "-q", "-b", "main"]);
    let file = dir.path().join("n.txt");
    std::fs::write(&file, "1\n2\n3\n4\n5\n6\n7\n").unwrap();
    git(dir.path(), BASE, &["add", "."]);
    git(dir.path(), BASE, &["commit", "-q", "-m", "Start"]);
    std::fs::write(&file, "1\n2\n3\nfour\n5\n6\n7\n").unwrap();
    git(dir.path(), BASE + DAY, &["commit", "-q", "-am", "Four"]);
    let out = tempfile::tempdir().unwrap();
    let mail = out.path().join("four.patch");
    std::fs::write(
        &mail,
        git_out(dir.path(), &["format-patch", "-1", "--stdout"]),
    )
    .unwrap();
    git(dir.path(), BASE, &["reset", "-q", "--hard", "HEAD~1"]);
    // 近くの行を変えておくと、そのままでは当たらない（3-way なら当たる）。
    std::fs::write(&file, "1\ntwo\n3\n4\n5\n6\n7\n").unwrap();
    git(dir.path(), BASE + DAY * 2, &["commit", "-q", "-am", "Two"]);
    let stopped = amonhen_model::process::command("git")
        .current_dir(dir.path())
        .args(["-c", "am.threeWay=false", "am", "-q"])
        .arg(&mail)
        .output()
        .unwrap();
    assert!(!stopped.status.success());
    let band = amonhen_tui::snapshot(open(dir.path()), 200, 10, clock(), "").unwrap();
    println!("{band}");
    assert!(band.contains("R retry with 3-way"), "{band}");
    let done = amonhen_tui::snapshot(open(dir.path()), 200, 10, clock(), "R").unwrap();
    println!("{done}");
    assert!(done.contains("Done"), "{done}");
    assert_eq!(git_out(dir.path(), &["log", "-1", "--format=%s"]), "Four\n");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "1\ntwo\n3\nfour\n5\n6\n7\n"
    );
}

/// コマンドの一覧の Save as Patch: History で選んでいるコミットを、git の名前のパッチにしてフォルダに書く
#[test]
fn save_the_selected_commit_as_a_patch() {
    let dir = tempfile::tempdir().unwrap();
    changed(dir.path());
    let out = tempfile::tempdir().unwrap();
    let folder = out.path().join("patches");
    let keys = format!(":save as patch\n\x15{}\n", folder.display());
    // 1 つなら送り状は出さない。版の番号は選べる。
    let asked = amonhen_tui::snapshot(open(dir.path()), 240, 8, clock(), &keys).unwrap();
    println!("{asked}");
    assert!(
        asked.contains("   y save   v as a new version (-v)…   n cancel"),
        "{asked}"
    );
    let done =
        amonhen_tui::snapshot(open(dir.path()), 200, 8, clock(), &format!("{keys}y")).unwrap();
    println!("{done}");
    assert!(done.contains("Saved "));
    let patch = folder.join("0001-Take-the-lane-from-the-trunk.patch");
    let text = std::fs::read_to_string(&patch).unwrap();
    assert!(text.contains("Subject: [PATCH] Take the lane from the trunk"));
    assert!(text.contains("+    let lane = trunk();"));
    // 版の番号を付ける（-v 3）。
    let keys = format!(":save as patch\n\x15{}\nv\x153\ny", folder.display());
    amonhen_tui::snapshot(open(dir.path()), 200, 8, clock(), &keys).unwrap();
    let text =
        std::fs::read_to_string(folder.join("v3-0001-Take-the-lane-from-the-trunk.patch")).unwrap();
    assert!(
        text.contains("Subject: [PATCH v3] Take the lane from the trunk"),
        "{text}"
    );
    // .mbox で終わる名前なら、1 つのファイルに。
    let mbox = out.path().join("all.mbox");
    let keys = format!(":save as patch\n\x15{}\ny", mbox.display());
    amonhen_tui::snapshot(open(dir.path()), 200, 8, clock(), &keys).unwrap();
    assert!(
        std::fs::read_to_string(&mbox)
            .unwrap()
            .contains("Subject: [PATCH] Take the lane")
    );
}

/// コマンドの一覧の Compare with Previous Version: 今のブランチを、最後に書き換える前の版と比べる（range-diff）
#[test]
fn compare_with_the_previous_version() {
    let dir = tempfile::tempdir().unwrap();
    changed(dir.path());
    let none = amonhen_tui::snapshot(open(dir.path()), 160, 10, clock(), ":compare\n").unwrap();
    assert!(none.contains("The branch hasn’t been rewritten"));
    // いちばん新しいコミットの中身を変えて amend する。
    std::fs::write(dir.path().join("notes.txt"), "two\n").unwrap();
    git(
        dir.path(),
        BASE + DAY * 4,
        &["commit", "-q", "-a", "--amend", "--no-edit"],
    );
    let shown = amonhen_tui::snapshot(open(dir.path()), 160, 10, clock(), ":compare\n").unwrap();
    println!("{shown}");
    let last = shown.lines().last().unwrap();
    assert!(last.contains("main compared with main@{1}"), "{last}");
    let row = shown
        .lines()
        .find(|l| l.contains("Take the lane from the trunk"))
        .unwrap();
    assert!(row.contains("1: ") && row.contains(" ! 1: "), "{row}");
    // 選んだ組のコミット（今の版）の詳細が、横に出る。
    let new = git_out(dir.path(), &["rev-parse", "--short=8", "HEAD"]);
    assert!(row.contains(new.trim()), "{row}");
    // 変わった組の下に、差分の差分（前の版の行は `-`、今の版の行は `+` を前に付けて）。
    let tall = amonhen_tui::snapshot(open(dir.path()), 160, 30, clock(), ":compare\n").unwrap();
    println!("{tall}");
    let list: Vec<String> = tall.lines().skip(1).filter_map(|l| column(l, 1)).collect();
    assert!(list.iter().any(|l| l.trim_start() == "-+one"), "{list:?}");
    assert!(list.iter().any(|l| l.trim_start() == "++two"), "{list:?}");
    // 差分の差分の行でも、組のコミットの詳細が出る。
    let below = amonhen_tui::snapshot(open(dir.path()), 160, 30, clock(), ":compare\nG").unwrap();
    assert!(below.contains("Take the lane from the trunk"));
}

/// partial clone は一番上の行に印。sparse checkout は Working Copy の一覧の上に知らせ（git 2.25 から）
#[test]
fn partial_clone_and_sparse_checkout_marks() {
    // `git sparse-checkout` は 2.25 から。2.23 は file:// の partial clone の取り出しにも失敗する。
    let version = git_out(Path::new("."), &["--version"]);
    let numbers: Vec<u32> = version
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|n| n.parse().ok())
        .collect();
    if numbers.len() < 2 || (numbers[0], numbers[1]) < (2, 25) {
        return;
    }
    let source = tempfile::tempdir().unwrap();
    let source = source.path();
    git(source, BASE, &["init", "-q", "-b", "main"]);
    for path in ["app/a.txt", "docs/b.txt", "top.txt"] {
        let path = source.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "one\n").unwrap();
    }
    git(source, BASE, &["add", "."]);
    git(source, BASE, &["commit", "-q", "-m", "Start"]);
    git(source, BASE, &["config", "uploadpack.allowFilter", "true"]);
    let place = tempfile::tempdir().unwrap();
    let url = format!("file://{}", source.display());
    git(
        place.path(),
        BASE,
        &["clone", "-q", "--filter=blob:none", &url, "partial"],
    );
    let dir = place.path().join("partial");
    let top = amonhen_tui::snapshot(open(&dir), 140, 12, clock(), "").unwrap();
    println!("{top}");
    assert!(top.lines().next().unwrap().contains("[partial]"), "{top}");
    assert!(!top.contains("Sparse checkout"), "{top}");
    git(&dir, BASE, &["sparse-checkout", "set", "app"]);
    let sparse = amonhen_tui::snapshot(open(&dir), 140, 12, clock(), "1").unwrap();
    println!("{sparse}");
    assert!(
        sparse.contains("Sparse checkout: only 1 folder is checked out."),
        "{sparse}"
    );
}

/// shallow clone: 最後の行の下に帯、境界のコミットに点線の印。`+` で取り足す（数を選ぶ、`a` ですべて）
#[test]
fn shallow_clone_band_and_fetch_more() {
    let source = tempfile::tempdir().unwrap();
    let source = source.path();
    git(source, BASE, &["init", "-q", "-b", "main"]);
    for n in 1..=5 {
        git(
            source,
            BASE + DAY * n,
            &[
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                &format!("Commit {n}"),
            ],
        );
    }
    let place = tempfile::tempdir().unwrap();
    let url = format!("file://{}", source.display());
    git(
        place.path(),
        BASE,
        &["clone", "-q", "--depth", "2", &url, "shallow"],
    );
    let dir = place.path().join("shallow");
    let shown = amonhen_tui::snapshot(open(&dir), 140, 12, clock(), "2").unwrap();
    println!("{shown}");
    assert!(
        shown.contains("Older history isn’t fetched (shallow clone)"),
        "{shown}"
    );
    assert!(shown.lines().any(|l| l.contains("┄ Commit 4")), "{shown}");
    // 一番上の行にも、shallow clone の印。
    assert!(
        shown.lines().next().unwrap().contains("[shallow]"),
        "{shown}"
    );
    let asked = amonhen_tui::snapshot(open(&dir), 140, 12, clock(), "2+").unwrap();
    let last = asked.lines().last().unwrap();
    assert!(
        last.contains("Fetch older history from the remote?"),
        "{last}"
    );
    // 日付まで: `d` で日付を打つ（それより新しいコミットをすべて）。
    let typing = amonhen_tui::snapshot(open(&dir), 140, 12, clock(), "2+d").unwrap();
    assert!(typing.contains("Fetch the commits newer than"), "{typing}");
    let since = amonhen_tui::snapshot(open(&dir), 140, 12, clock(), "2+d2000-01-01\n").unwrap();
    println!("{since}");
    assert!(since.contains("Commit 1"), "{since}");
    git(&dir, BASE, &["fetch", "-q", "--depth", "2"]);
    let all = amonhen_tui::snapshot(open(&dir), 140, 12, clock(), "2+a").unwrap();
    println!("{all}");
    assert!(all.contains("Commit 1"), "{all}");
    assert!(!all.contains("shallow clone"), "{all}");
}

/// 詳細の `+`: 今見ているファイルの前後を 20 行広げる。`F` でファイル全体を出し、もう一度で戻す
#[test]
fn widen_the_context_and_show_the_whole_file() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    git(dir, BASE, &["init", "-q", "-b", "main"]);
    let mut lines: Vec<String> = (1..=60).map(|n| format!("line {n}")).collect();
    std::fs::write(dir.join("a.txt"), lines.join("\n") + "\n").unwrap();
    git(dir, BASE, &["add", "."]);
    git(dir, BASE, &["commit", "-q", "-m", "Start"]);
    lines[29] = "changed".into();
    std::fs::write(dir.join("a.txt"), lines.join("\n") + "\n").unwrap();
    git(dir, BASE, &["commit", "-q", "-a", "-m", "Change"]);
    let shown = |keys: &str| {
        let text = amonhen_tui::snapshot(open(dir), 120, 90, clock(), keys).unwrap();
        let has = |n: u32| {
            text.contains(&format!(" line {n}\n")) || text.contains(&format!(" line {n} "))
        };
        (text.clone(), has(5), has(10), has(27))
    };
    let (text, five, ten, near) = shown("2\n");
    assert!(near && !ten && !five, "{text}");
    let (text, five, ten, _) = shown("2\n+");
    println!("{text}");
    assert!(ten && !five, "{text}");
    let (text, five, _, _) = shown("2\nF");
    assert!(five && text.contains("Showing the whole file"), "{text}");
    let (text, five, ten, _) = shown("2\nFF");
    assert!(!five && !ten, "{text}");
}

/// 詳細の `w`: 差分で無視する空白を、行末 → 量 → すべて → なしの順に替える
#[test]
fn details_ignore_whitespace() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    git(dir, BASE, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("a.txt"), "one\n  two\nthree\n").unwrap();
    git(dir, BASE, &["add", "."]);
    git(dir, BASE, &["commit", "-q", "-m", "Start"]);
    std::fs::write(dir.join("a.txt"), "one\n    two\nthree\nfour\n").unwrap();
    git(dir, BASE, &["commit", "-q", "-a", "-m", "Indent"]);
    let exact = amonhen_tui::snapshot(open(dir), 120, 30, clock(), "2\n").unwrap();
    assert!(exact.contains("+    two"), "{exact}");
    let eol = amonhen_tui::snapshot(open(dir), 120, 30, clock(), "2\nw").unwrap();
    println!("{eol}");
    assert!(eol.contains("Ignoring whitespace at line ends"), "{eol}");
    assert!(eol.contains("+    two"), "{eol}");
    let amount = amonhen_tui::snapshot(open(dir), 120, 30, clock(), "2\nww").unwrap();
    println!("{amount}");
    assert!(!amount.contains("+    two"), "{amount}");
    assert!(amount.contains("+four"), "{amount}");
    let back = amonhen_tui::snapshot(open(dir), 120, 30, clock(), "2\nwwww").unwrap();
    assert!(back.contains("Whitespace is compared"), "{back}");
    assert!(back.contains("+    two"), "{back}");
}

/// 詳細の `t`: 変わったファイルを木で出す（フォルダの下に名前だけ）。`z` でフォルダをたたみ、もう一度で開く
#[test]
fn changed_files_as_a_tree() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    git(dir, BASE, &["init", "-q", "-b", "main"]);
    for path in ["src/ui/view.rs", "src/main.rs", "README.md"] {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "one\n").unwrap();
    }
    git(dir, BASE, &["add", "."]);
    git(dir, BASE, &["commit", "-q", "-m", "Start"]);
    let list = amonhen_tui::snapshot(open(dir), 120, 30, clock(), "2\n").unwrap();
    assert!(list.contains("A src/ui/view.rs"), "{list}");
    let tree = amonhen_tui::snapshot(open(dir), 120, 30, clock(), "2\nt").unwrap();
    println!("{tree}");
    let rows: Vec<String> = tree
        .lines()
        .filter_map(|l| column(l, 1))
        .map(|l| l.trim_end().to_owned())
        .collect();
    let at = rows
        .iter()
        .position(|l| l.starts_with("A README.md"))
        .unwrap();
    let names: Vec<&str> = rows[at..at + 6]
        .iter()
        .map(|l| l.split(" +").next().unwrap().trim_end())
        .collect();
    assert_eq!(
        names,
        [
            "A README.md",
            "▾ src/",
            "  A main.rs",
            "  ▾ ui/",
            "    A view.rs",
            ""
        ],
        "{rows:?}"
    );
    let folded = amonhen_tui::snapshot(open(dir), 120, 30, clock(), "2\ntz").unwrap();
    assert!(folded.contains("▸ src/ (2)"), "{folded}");
    assert!(!folded.contains("A view.rs"), "{folded}");
    let opened = amonhen_tui::snapshot(open(dir), 120, 30, clock(), "2\ntzz").unwrap();
    assert!(opened.contains("A view.rs"), "{opened}");
}

/// History で 2 つ選んで `=`: それぞれを先端とする 2 つの版として比べる。Reflog の `=` は、その行と今の HEAD を
#[test]
fn compare_two_commits_as_two_versions() {
    let dir = tempfile::tempdir().unwrap();
    changed(dir.path());
    // 書き換える前の版をブランチに残してから、いちばん新しいコミットの中身を変えて amend する。
    git(dir.path(), BASE, &["branch", "before"]);
    std::fs::write(dir.path().join("notes.txt"), "two\n").unwrap();
    git(
        dir.path(),
        BASE + DAY * 4,
        &["commit", "-q", "-a", "--amend", "--no-edit"],
    );
    let new = git_out(dir.path(), &["rev-parse", "--short=8", "HEAD"]);
    let old = git_out(dir.path(), &["rev-parse", "--short=8", "before"]);
    let shown = amonhen_tui::snapshot(open(dir.path()), 160, 30, clock(), "2mjm=").unwrap();
    println!("{shown}");
    let last = shown.lines().last().unwrap();
    assert!(
        last.contains(&format!(
            "{} and {} compared as two versions",
            old.trim(),
            new.trim()
        )),
        "{last}"
    );
    let row = shown
        .lines()
        .find(|l| l.contains("Take the lane from the trunk"))
        .unwrap();
    assert!(row.contains(" ! "), "{row}");
    // Reflog: 1 つ前（amend の前）の行と、今の HEAD を比べる。
    let reflog = amonhen_tui::snapshot(open(dir.path()), 160, 30, clock(), "3j=").unwrap();
    println!("{reflog}");
    let last = reflog.lines().last().unwrap();
    assert!(
        last.contains("HEAD@{1} compared with the current HEAD"),
        "{last}"
    );
    assert!(
        reflog
            .lines()
            .any(|l| l.contains("Take the lane from the trunk") && l.contains(" ! "))
    );
}

/// range-diff で `c`: 変わらなかった組（`=`）を出さない（アプリの Changes Only）。もう一度で戻る
#[test]
fn range_diff_shows_changes_only() {
    let dir = tempfile::tempdir().unwrap();
    changed(dir.path());
    std::fs::write(dir.path().join("extra.txt"), "extra\n").unwrap();
    git(dir.path(), BASE, &["add", "."]);
    git(
        dir.path(),
        BASE + DAY * 4,
        &["commit", "-q", "-m", "Add extra"],
    );
    let before = git_out(dir.path(), &["rev-parse", "HEAD"]);
    // 1 つ前のコミットだけを変えて、その上に同じものを載せ直す。
    git(dir.path(), BASE, &["checkout", "-q", "--detach", "HEAD~1"]);
    std::fs::write(dir.path().join("notes.txt"), "two\n").unwrap();
    git(
        dir.path(),
        BASE + DAY * 5,
        &["commit", "-q", "-a", "--amend", "--no-edit"],
    );
    git(dir.path(), BASE + DAY * 5, &["cherry-pick", before.trim()]);
    git(dir.path(), BASE, &["checkout", "-q", "-B", "main"]);
    let same = |text: &str| {
        text.lines()
            .skip(1)
            .filter_map(|l| column(l, 1))
            .any(|l| l.contains(" = "))
    };
    let all = amonhen_tui::snapshot(open(dir.path()), 160, 30, clock(), ":compare\n").unwrap();
    println!("{all}");
    assert!(same(&all), "{all}");
    let changes = amonhen_tui::snapshot(open(dir.path()), 160, 30, clock(), ":compare\nc").unwrap();
    println!("{changes}");
    assert!(!same(&changes), "{changes}");
    assert!(changes.contains("(changes only)"), "{changes}");
    assert!(
        changes.contains("Take the lane from the trunk"),
        "{changes}"
    );
    let back = amonhen_tui::snapshot(open(dir.path()), 160, 30, clock(), ":compare\ncc").unwrap();
    assert!(same(&back), "{back}");
}

/// コマンドの一覧の Prune Worktrees: フォルダのなくなった worktree の記録を片付ける（キーはない）
#[test]
fn prune_worktrees_from_the_command_list() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main");
    std::fs::create_dir(&main).unwrap();
    merged(&main);
    let other = dir.path().join("other");
    git(
        &main,
        BASE,
        &["worktree", "add", "-q", other.to_str().unwrap(), "topic"],
    );
    std::fs::remove_dir_all(&other).unwrap();
    let count = || git_out(&main, &["worktree", "list"]).lines().count();
    assert_eq!(count(), 2);
    let list = amonhen_tui::snapshot(open(&main), 120, 30, clock(), ":prune").unwrap();
    println!("{list}");
    // キーのない操作は、キーの欄が空。
    let row = list
        .lines()
        .find(|l| l.contains("Prune Worktrees"))
        .unwrap();
    assert!(row.trim_end().ends_with("│"), "{row}");
    let pruned = amonhen_tui::snapshot(open(&main), 120, 30, clock(), ":prune\n").unwrap();
    assert!(pruned.contains("Pruned the worktrees"));
    assert_eq!(count(), 1);
}

/// サイドバーのブランチで `w`: 場所を聞いて（初めは作業ツリーの隣）、そのブランチの worktree を作る。
#[test]
fn a_worktree_is_added_for_the_selected_branch() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main");
    std::fs::create_dir(&main).unwrap();
    merged(&main);
    // サイドバーの 3 つ下が topic、その上が今の main。
    let here = amonhen_tui::snapshot(open(&main), 200, 16, clock(), "0jjw").unwrap();
    assert!(here.contains("This branch is checked out here"));
    let asked = amonhen_tui::snapshot(open(&main), 200, 16, clock(), "0jjjw").unwrap();
    println!("{asked}");
    assert!(asked.contains("New worktree for topic at: "));
    assert!(asked.lines().last().unwrap().contains("main-topic"));
    // 場所を打ち直して作る（初めの場所の末尾を消して、別の名前に）。
    let keys = "0jjjw\x7f\x7f\x7f\x7f\x7fside\n";
    let added = amonhen_tui::snapshot(open(&main), 200, 16, clock(), keys).unwrap();
    println!("{added}");
    assert!(added.contains("Added the worktree"));
    let other = dir.path().join("main-side");
    assert_eq!(git_out(&other, &["branch", "--show-current"]), "topic\n");
    let side: Vec<String> = added.lines().skip(1).filter_map(|l| column(l, 0)).collect();
    assert!(side.iter().any(|l| l == "  main-side (topic)"), "{side:?}");
    // 同じブランチをもう 1 つの worktree には出せない（git が断る）。
    let refused = amonhen_tui::snapshot(open(&main), 200, 16, clock(), "0jjjw\n").unwrap();
    println!("{refused}");
    assert!(!refused.contains("Added the worktree"));
    assert!(!dir.path().join("main-topic").exists());
}

/// サイドバーの worktree の行で `x`: 確かめてから消す。変更が残っていれば、その数を書いて変更ごと消す
/// （`--force`）。開いている worktree は消せない。
#[test]
fn a_worktree_is_removed_from_the_sidebar() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main");
    std::fs::create_dir(&main).unwrap();
    merged(&main);
    let other = dir.path().join("other");
    git(
        &main,
        BASE,
        &["worktree", "add", "-q", other.to_str().unwrap(), "topic"],
    );
    // 一番下の行が other。その上が、開いている main。
    let open_one = amonhen_tui::snapshot(open(&main), 160, 16, clock(), "0Gkx").unwrap();
    println!("{open_one}");
    assert!(open_one.contains("This worktree is the one that is open"));
    let asked = amonhen_tui::snapshot(open(&main), 200, 16, clock(), "0Gx").unwrap();
    println!("{asked}");
    assert!(asked.contains("Remove the worktree at"));
    assert!(asked.contains("y remove   n cancel"));
    assert!(other.exists());
    assert!(asked.contains("The branch stays."), "{asked}");
    // 変更が残っていれば、その数を書く。消すと変更ごと消える。
    std::fs::write(other.join("new.txt"), "new\n").unwrap();
    let warned = amonhen_tui::snapshot(open(&main), 240, 16, clock(), "0Gx").unwrap();
    println!("{warned}");
    assert!(
        warned.contains("Its uncommitted changes in 1 files are lost with the folder"),
        "{warned}"
    );
    assert!(other.exists());
    let removed = amonhen_tui::snapshot(open(&main), 200, 16, clock(), "0Gxy").unwrap();
    println!("{removed}");
    assert!(removed.contains("Removed the worktree"));
    assert!(!other.exists());
    // worktree が 1 つになったので、サイドバーからも消える。
    assert!(!removed.contains("Worktrees"));

    // History のコミットから、新しいブランチの worktree を作る（名前、次に場所を聞く）。
    let asked = amonhen_tui::snapshot(open(&main), 200, 16, clock(), "jw").unwrap();
    assert!(asked.contains("New worktree from "), "{asked}");
    assert!(asked.contains("with a new branch: "), "{asked}");
    let place = amonhen_tui::snapshot(open(&main), 200, 16, clock(), "jwfix\n").unwrap();
    let beside = format!("{}-fix", main.to_str().unwrap());
    assert!(
        place.contains(&format!("New worktree for fix at: {beside}")),
        "{place}"
    );
    amonhen_tui::snapshot(open(&main), 200, 16, clock(), "jwfix\n\n").unwrap();
    assert_eq!(
        git_out(Path::new(&beside), &["branch", "--show-current"]),
        "fix\n"
    );
    assert_eq!(
        git_out(Path::new(&beside), &["rev-parse", "HEAD"]),
        git_out(&main, &["rev-parse", "HEAD~1"])
    );
}

/// サイドバーの worktree の行で、`L` でロックし・外し、`n` でフォルダを移す（見つからなければ今の場所を聞いて
/// つなぎ直す）
#[test]
fn lock_move_and_locate_a_worktree() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main");
    std::fs::create_dir(&main).unwrap();
    merged(&main);
    let other = dir.path().join("other");
    git(
        &main,
        BASE,
        &["worktree", "add", "-q", other.to_str().unwrap(), "topic"],
    );
    let list = || git_out(&main, &["worktree", "list", "--porcelain"]);
    // ロックの理由は git の記録のファイルに（porcelain に理由が出るのは git 2.31 から）。
    let lock = main.join(".git/worktrees/other/locked");
    // 一番下の行が other。
    let asked = amonhen_tui::snapshot(open(&main), 200, 16, clock(), "0GL").unwrap();
    println!("{asked}");
    assert!(
        asked.contains(
            "git then won’t prune, remove or move it.   y lock   r with a reason…   n cancel"
        ),
        "{asked}"
    );
    amonhen_tui::snapshot(open(&main), 200, 16, clock(), "0GLrtravelling\n").unwrap();
    assert_eq!(std::fs::read_to_string(&lock).unwrap().trim(), "travelling");
    let locked = amonhen_tui::snapshot(open(&main), 200, 16, clock(), "0Gn").unwrap();
    assert!(locked.contains("other (topic) · locked"), "{locked}");
    assert!(locked.contains("The worktree is locked"), "{locked}");
    amonhen_tui::snapshot(open(&main), 200, 16, clock(), "0GLy").unwrap();
    assert!(!lock.exists());
    // 移す。
    let moved = dir.path().join("moved");
    let keys = format!("0Gn\x15{}\n", moved.display());
    amonhen_tui::snapshot(open(&main), 200, 16, clock(), &keys).unwrap();
    assert!(moved.join(".git").exists() && !other.exists());
    // 手で移すと、見つからない印が付く。`n` で今の場所を教えて、つなぎ直す（git worktree repair は 2.29 から）。
    if !git_at_least(2, 29) {
        return;
    }
    let again = dir.path().join("again");
    std::fs::rename(&moved, &again).unwrap();
    let missing = amonhen_tui::snapshot(open(&main), 200, 16, clock(), "").unwrap();
    assert!(missing.contains("moved (topic) · missing"), "{missing}");
    let keys = format!("0Gn\x15{}\n", again.display());
    let done = amonhen_tui::snapshot(open(&main), 200, 16, clock(), &keys).unwrap();
    println!("{done}");
    assert!(!done.contains("· missing"), "{done}");
    assert!(
        list().contains(again.file_name().unwrap().to_str().unwrap()),
        "{}",
        list()
    );
}

/// 上流のないブランチの `P` は、リモートに同じ名前で publish して上流にする
#[test]
fn push_publishes_a_branch_without_an_upstream() {
    let dir = tempfile::tempdir().unwrap();
    let work = with_remote(dir.path());
    git(&work, BASE, &["switch", "-q", "-c", "topic"]);
    let asked = amonhen_tui::snapshot(open(&work), 100, 8, clock(), "P").unwrap();
    println!("{asked}");
    assert!(asked.contains("Publish topic to origin?   y publish   n cancel"));
    let done = amonhen_tui::snapshot(open(&work), 100, 8, clock(), "Py").unwrap();
    assert!(done.contains("Pushed"));
    assert_eq!(
        git_out(&work, &["rev-parse", "--abbrev-ref", "topic@{upstream}"]),
        "origin/topic\n"
    );
}

/// 分かれた上流を pull する前に、コンフリクトしそうなファイルを言う
#[test]
fn pull_names_the_files_likely_to_conflict() {
    let dir = tempfile::tempdir().unwrap();
    diverged(dir.path());
    // topic（同じ行を別に書き換えた）を main の上流にする。
    git(
        dir.path(),
        BASE,
        &["branch", "-q", "--set-upstream-to=topic"],
    );
    let pull = amonhen_tui::snapshot(open(dir.path()), 200, 8, clock(), "p").unwrap();
    let line = pull.lines().last().unwrap();
    println!("{line}");
    assert!(
        line.contains(" How?   y as git is set   m merge   r rebase   n cancel"),
        "{line}"
    );
    if predicts_conflicts(dir.path()) {
        assert!(
            line.contains("have diverged. Conflicts are likely in a.txt. How?"),
            "{line}"
        );
    }
}

/// 上流と分かれていれば、`P` は置き換える push も選ばせ、`p` は取り込み方を選ばせる
#[test]
fn diverged_branches_offer_force_push_and_pull_modes() {
    let dir = tempfile::tempdir().unwrap();
    let work = with_remote(dir.path());
    git(&work, BASE, &["push", "-q"]);
    // push 済みのコミットを作り直す: 手元と上流が 1 つずつ違う。
    git(
        &work,
        BASE + DAY,
        &[
            "commit",
            "-q",
            "--amend",
            "--allow-empty",
            "-m",
            "Rewritten",
        ],
    );
    let remote_head = || {
        git_out(
            &dir.path().join("origin.git"),
            &["log", "-1", "--format=%s", "main"],
        )
    };
    let pull = amonhen_tui::snapshot(open(&work), 160, 8, clock(), "p").unwrap();
    println!("{pull}");
    assert!(pull.contains(" How?   y as git is set   m merge   r rebase   n cancel"));
    // 先にコンフリクトの見込みを読む（空のコミットどうしなので、ない）。
    if predicts_conflicts(&work) {
        assert!(
            pull.contains("have diverged. No conflicts are expected. How?"),
            "{pull}"
        );
    } else {
        assert!(pull.contains("have diverged. How?"), "{pull}");
    }
    let asked = amonhen_tui::snapshot(open(&work), 200, 8, clock(), "P").unwrap();
    println!("{asked}");
    assert!(
        asked.contains("y push   f force push (with lease), replacing 1 on the remote   t another branch…   n cancel"),
        "{asked}"
    );
    // 置き換える push の前と後の絵: 上流の 1 つが外れ、手元のものが origin/main に。
    assert!(
        asked.contains(" If you force push to origin/main "),
        "{asked}"
    );
    assert!(
        asked.contains("Ahead of the remote  [origin/main]  leaves the branch"),
        "{asked}"
    );
    assert!(asked.contains("Rewritten  [origin/main]"), "{asked}");
    // ふつうの push は断られる（理由が出る）。
    let refused = amonhen_tui::snapshot(open(&work), 140, 8, clock(), "Py").unwrap();
    println!("{refused}");
    assert_eq!(remote_head(), "Ahead of the remote\n");
    assert!(!refused.contains("Pushed"));
    let forced = amonhen_tui::snapshot(open(&work), 140, 8, clock(), "Pf").unwrap();
    assert!(forced.contains("Pushed"));
    assert_eq!(remote_head(), "Rewritten\n");
}

/// サイドバーの `n` でブランチの名前を変え、`D` で確かめてから消す。`T` でタグを作る
#[test]
fn rename_and_delete_branches_and_tags() {
    let dir = tempfile::tempdir().unwrap();
    merged(dir.path());
    let branches = || git_out(dir.path(), &["branch", "--format=%(refname:short)"]);
    // topic の行で `n`: 今の名前が入っている。消して打ち直す。
    let typing = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "0jjjn").unwrap();
    assert!(typing.contains("Rename topic to: topic"));
    let keys = format!("0jjjn{}feature\n", "\x7f".repeat(5));
    amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), &keys).unwrap();
    assert_eq!(branches(), "feature\nmain\n");
    // 今のブランチ（main）は消せない。
    let refused = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "0jjjD").unwrap();
    println!("{refused}");
    assert!(refused.contains("The current branch can’t be deleted"));
    // feature（main の 1 つ上）を消す。
    let asked = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "0jjD").unwrap();
    assert!(asked.contains("Delete feature? Undo brings it back.   y delete   n cancel"));
    amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "0jjDy").unwrap();
    assert_eq!(branches(), "main\n");
    // History で選んでいるコミットにタグ。
    let typing = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "jTv").unwrap();
    assert!(typing.contains("New tag at "));
    amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "jTv2\n").unwrap();
    assert_eq!(
        git_out(dir.path(), &["log", "-1", "--format=%s", "v2"]),
        "Bump the version\n"
    );
    // タグの行（一番下）で `D`。
    amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "0GDy").unwrap();
    assert_eq!(git_out(dir.path(), &["tag"]), "v1\n");
}

/// ほかのどこにもないコミットのあるブランチは、外れるコミットの絵と数を出し、Delete Anyway（`-D`）で消す
#[test]
fn delete_anyway_a_branch_with_commits_on_no_other_branch() {
    let dir = tempfile::tempdir().unwrap();
    diverged(dir.path());
    let branches = || git_out(dir.path(), &["branch", "--format=%(refname:short)"]);
    // サイドバーの 2 つ下が topic（On topic は main にない）。
    let asked = amonhen_tui::snapshot(open(dir.path()), 200, 14, clock(), "0jjjD").unwrap();
    println!("{asked}");
    assert!(
        asked.contains("Delete topic? Commits on no other branch (1) go with it. Undo brings it back.   y delete anyway   n cancel"),
        "{asked}"
    );
    assert!(asked.contains(" After deleting topic "), "{asked}");
    assert!(asked.contains("On topic"), "{asked}");
    // 後の側に、消えるブランチの名前は付かない。
    assert!(!asked.contains("Start  [topic]"), "{asked}");
    amonhen_tui::snapshot(open(dir.path()), 200, 14, clock(), "0jjjDn").unwrap();
    assert_eq!(branches(), "main\ntopic\n");
    amonhen_tui::snapshot(open(dir.path()), 200, 14, clock(), "0jjjDy").unwrap();
    assert_eq!(branches(), "main\n");
    // Undo で戻る。
    amonhen_tui::snapshot(open(dir.path()), 200, 14, clock(), "uy").unwrap();
    assert_eq!(branches(), "main\ntopic\n");

    // 上流に取り込まれていれば（今のブランチになくても）、ふつうに消す。
    git(
        dir.path(),
        BASE,
        &["update-ref", "refs/remotes/origin/topic", "topic"],
    );
    git(
        dir.path(),
        BASE,
        &["config", "branch.topic.remote", "origin"],
    );
    git(
        dir.path(),
        BASE,
        &["config", "branch.topic.merge", "refs/heads/topic"],
    );
    git(
        dir.path(),
        BASE,
        &["config", "remote.origin.url", "/nowhere"],
    );
    git(
        dir.path(),
        BASE,
        &[
            "config",
            "remote.origin.fetch",
            "+refs/heads/*:refs/remotes/origin/*",
        ],
    );
    let asked = amonhen_tui::snapshot(open(dir.path()), 200, 14, clock(), "0jjjD").unwrap();
    println!("{asked}");
    assert!(
        asked.contains("Delete topic? Undo brings it back.   y delete   n cancel"),
        "{asked}"
    );
}

/// History の `y` で cherry-pick、`v` で revert、`c` でそのコミットをチェックアウトする（どれも先に確かめる）
#[test]
fn cherry_pick_revert_and_detach_from_history() {
    let dir = tempfile::tempdir().unwrap();
    diverged(dir.path());
    let subject = || git_out(dir.path(), &["log", "-1", "--format=%s"]);
    // 2 行目（On topic）を main に持ってくる: 同じ行を書き換えているので、コンフリクトで止まる。
    let asked = amonhen_tui::snapshot(open(dir.path()), 120, 10, clock(), "jy").unwrap();
    println!("{asked}");
    assert!(asked.contains("“On topic”?   y cherry-pick   n cancel"));
    // アプリのシートと同じく、前と後の絵: 後は今の先端（On main）の上に新しいコミット。
    assert!(asked.contains(" Cherry-pick "), "{asked}");
    assert!(asked.contains("On topic  [main]  new"), "{asked}");
    let stopped = amonhen_tui::snapshot(open(dir.path()), 120, 10, clock(), "jyy").unwrap();
    assert!(stopped.contains("Stopped at conflicts"));
    amonhen_tui::snapshot(open(dir.path()), 120, 10, clock(), "Ay").unwrap();
    // 一番上（On main）を打ち消す。
    let asked = amonhen_tui::snapshot(open(dir.path()), 120, 10, clock(), "v").unwrap();
    println!("{asked}");
    assert!(asked.contains("Revert "));
    assert!(asked.contains("Revert \"On main\"  [main]  new"), "{asked}");
    amonhen_tui::snapshot(open(dir.path()), 120, 10, clock(), "vy").unwrap();
    assert_eq!(subject(), "Revert \"On main\"\n");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "one\n"
    );
    // 一番下（Start）をチェックアウト: ブランチから離れる。
    let asked = amonhen_tui::snapshot(open(dir.path()), 120, 10, clock(), "Gc").unwrap();
    assert!(asked.contains("(detached HEAD)?   y check out   n cancel"));
    amonhen_tui::snapshot(open(dir.path()), 120, 10, clock(), "Gcy").unwrap();
    assert_eq!(subject(), "Start\n");
    assert_eq!(git_out(dir.path(), &["branch", "--show-current"]), "");
}

/// キーの一覧が収まらなければ、`j` `k` で動かせる
#[test]
fn the_key_list_scrolls_when_it_does_not_fit() {
    let dir = tempfile::tempdir().unwrap();
    merged(dir.path());
    let top = amonhen_tui::snapshot(open(dir.path()), 80, 12, clock(), "?").unwrap();
    println!("{top}");
    assert!(top.contains("j/k more"));
    assert!(top.contains("│ j k"));
    assert!(!top.contains("│ q "));
    let bottom = amonhen_tui::snapshot(
        open(dir.path()),
        80,
        12,
        clock(),
        &format!("?{}", "j".repeat(40)),
    )
    .unwrap();
    println!("{bottom}");
    assert!(bottom.contains("│ q "));
    assert!(!bottom.contains("│ j k"));
}

/// 詳細には、そのコミットを指すブランチとタグも出る
#[test]
fn details_list_the_refs_at_the_commit() {
    let dir = tempfile::tempdir().unwrap();
    merged(dir.path());
    let text = amonhen_tui::snapshot(open(dir.path()), 160, 14, clock(), "").unwrap();
    println!("{text}");
    let details: Vec<String> = text.lines().filter_map(|l| column(l, 2)).collect();
    let refs = details.iter().find(|l| l.starts_with("Refs")).unwrap();
    assert!(refs.contains("main") && refs.contains("v1"), "{refs}");
    // 署名のないコミットには、署名の行がない。
    assert!(!details.iter().any(|l| l.starts_with("Signed")));
}

/// `s` の `f` は選んでいるファイルだけ、`s` は stage したものだけを stash する（アプリの Stash のシートと同じ）
#[test]
fn stash_only_the_selected_file_or_what_is_staged() {
    let dir = tempfile::tempdir().unwrap();
    two_lines(dir.path());
    std::fs::write(dir.path().join("b.txt"), "b\n").unwrap();
    git(dir.path(), BASE, &["add", "b.txt"]);
    git(dir.path(), BASE, &["commit", "-q", "-m", "Add b"]);
    std::fs::write(dir.path().join("b.txt"), "B\n").unwrap();
    let status = || git_out(dir.path(), &["status", "--porcelain"]);
    // Working Copy の 2 つ目（b.txt）を選んで、それだけを stash する。
    amonhen_tui::snapshot(open(dir.path()), 200, 10, clock(), "1js\nf").unwrap();
    assert_eq!(status(), " M a.txt\n");
    assert!(git_out(dir.path(), &["stash", "show", "--name-only"]).contains("b.txt"));
    git(dir.path(), BASE, &["stash", "pop", "-q"]);
    // stage したものだけ（git 2.35 から。前の git では理由が出る）。
    git(dir.path(), BASE, &["add", "a.txt"]);
    let done = amonhen_tui::snapshot(open(dir.path()), 200, 10, clock(), "1s\ns").unwrap();
    if done.contains("Stashed") {
        assert_eq!(status(), " M b.txt\n");
    } else {
        assert!(done.contains("2.35"), "{done}");
    }
}

/// `s` の `u` は、追跡していないファイルも stash に入れる
#[test]
fn stash_can_include_untracked_files() {
    let dir = tempfile::tempdir().unwrap();
    two_lines(dir.path());
    std::fs::write(dir.path().join("new.txt"), "new\n").unwrap();
    amonhen_tui::snapshot(open(dir.path()), 100, 10, clock(), "s\ny").unwrap();
    assert!(dir.path().join("new.txt").exists());
    amonhen_tui::snapshot(open(dir.path()), 100, 10, clock(), "Sy").unwrap();
    amonhen_tui::snapshot(open(dir.path()), 100, 10, clock(), "s\nu").unwrap();
    assert!(!dir.path().join("new.txt").exists());
    assert_eq!(git_out(dir.path(), &["status", "--porcelain"]), "");
}

/// `Ctrl-p` は、コミットしてから push を確かめる
#[test]
fn commit_and_push() {
    let dir = tempfile::tempdir().unwrap();
    let work = with_remote(dir.path());
    git(&work, BASE, &["push", "-q"]);
    std::fs::write(work.join("a.txt"), "one\n").unwrap();
    let front = || {
        amonhen_model::testing::isolate_from_user_settings();
        open(&work)
    };
    // 新しいファイルを stage し、メッセージを書いて Ctrl-p。
    let asked = amonhen_tui::snapshot(front(), 160, 14, clock(), "1 cAdd a file\x10").unwrap();
    println!("{asked}");
    assert_eq!(
        git_out(&work, &["log", "-1", "--format=%s"]),
        "Add a file\n"
    );
    assert!(
        asked.contains(
            "Push main to origin/main (1 commit)? (“Add a file”)   y push   t another branch…   n cancel"
        ),
        "{asked}"
    );
    let remote = dir.path().join("origin.git");
    assert_eq!(
        git_out(&remote, &["log", "-1", "--format=%s", "main"]),
        "Ahead of the remote\n"
    );
    // メッセージがなければ、コミットも push もしない。
    let nothing = amonhen_tui::snapshot(front(), 120, 14, clock(), "1\x10").unwrap();
    assert!(nothing.contains("Write a commit message first"));
}

/// Working Copy の `B` で、選んでいるファイルの Blame。`L` で File History
#[test]
fn blame_and_file_history_of_a_file() {
    let dir = tempfile::tempdir().unwrap();
    changed(dir.path());
    // lane.rs をもう一度書き換える（Working Copy に出るように）。
    std::fs::write(
        dir.path().join("lane.rs"),
        "fn lane() -> u32 {\n    let lane = trunk();\n    lane + 1\n}\n",
    )
    .unwrap();
    let blame = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "1B").unwrap();
    println!("{blame}");
    let list: Vec<String> = blame.lines().skip(1).filter_map(|l| column(l, 1)).collect();
    // 1 行目は最初のコミット、2 行目は書き換えたコミット、3 行目はまだコミットしていない。
    assert!(list[0].ends_with("fn lane() -> u32 {"), "{list:?}");
    assert!(list[0].contains("Ann"));
    assert!(list[1].ends_with("    let lane = trunk();"));
    assert!(list[2].starts_with("not committed"));
    assert!(blame.lines().last().unwrap().contains("Blame: lane.rs"));
    // 2 行目を選ぶと、そのコミットの詳細。
    let picked = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "1Bj").unwrap();
    let details: Vec<String> = picked.lines().filter_map(|l| column(l, 2)).collect();
    assert_eq!(details[0], "Take the lane from the trunk");
    // File History: 変えたコミットが新しい順に。
    let log = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "1L").unwrap();
    println!("{log}");
    let list: Vec<String> = log.lines().skip(1).filter_map(|l| column(l, 1)).collect();
    assert!(list[0].starts_with("Take the lane from the trunk"));
    assert!(list[1].starts_with("Start"));
    // Esc で Working Copy に戻る。
    let back = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "1L\x1b").unwrap();
    assert!(back.contains("[ ] M lane.rs"));
}

/// File History で `V`: 作業ツリーのファイルを、選んだ版に戻す（確かめてから。Undo で元に戻る）
#[test]
fn file_history_restores_a_version() {
    let dir = tempfile::tempdir().unwrap();
    changed(dir.path());
    let read = || std::fs::read_to_string(dir.path().join("lane.rs")).unwrap();
    let now = read();
    // 何もしないコミットを足して、lane.rs が Working Copy に出るように書き換える。
    std::fs::write(dir.path().join("lane.rs"), format!("{now}// later\n")).unwrap();
    let edited = read();
    // 2 行目が最初のコミット（Start）。
    let asked = amonhen_tui::snapshot(open(dir.path()), 260, 12, clock(), "1LjV").unwrap();
    println!("{asked}");
    assert!(asked.contains("Restore lane.rs from "), "{asked}");
    assert!(
        asked.contains("y this version   s this version, staged too   b as it was before   B as it was before, staged too   n cancel"),
        "{asked}"
    );
    assert_eq!(read(), edited);
    let restored = amonhen_tui::snapshot(open(dir.path()), 200, 12, clock(), "1LjVy").unwrap();
    println!("{restored}");
    assert!(restored.contains("Restored the version"));
    assert_eq!(read(), git_out(dir.path(), &["show", "HEAD~1:lane.rs"]));
    // Undo で、戻す前の中身に。
    amonhen_tui::snapshot(open(dir.path()), 200, 12, clock(), "uy").unwrap();
    assert_eq!(read(), edited);
    // 1 行目（最後のコミット）の前の版に、stage もして戻す。
    amonhen_tui::snapshot(open(dir.path()), 200, 12, clock(), "1LVB").unwrap();
    let before = git_out(dir.path(), &["show", "HEAD~1:lane.rs"]);
    assert_eq!(read(), before);
    assert_eq!(git_out(dir.path(), &["show", ":lane.rs"]), before);
    amonhen_tui::snapshot(open(dir.path()), 200, 12, clock(), "uy").unwrap();
    assert_eq!(read(), edited);
    // Blame では、File History を開くように知らせる。
    let blame = amonhen_tui::snapshot(open(dir.path()), 200, 12, clock(), "1BV").unwrap();
    assert!(blame.contains("Open File History (L) to restore a version"));
    // 名前を変える前の版は、戻さない（理由を出す）。
    git(
        dir.path(),
        BASE + DAY * 3,
        &["checkout", "-q", "--", "lane.rs"],
    );
    git(dir.path(), BASE + DAY * 3, &["mv", "lane.rs", "road.rs"]);
    git(
        dir.path(),
        BASE + DAY * 3,
        &["commit", "-q", "-m", "Rename"],
    );
    std::fs::write(dir.path().join("road.rs"), format!("{now}// later\n")).unwrap();
    let renamed = amonhen_tui::snapshot(open(dir.path()), 200, 12, clock(), "1LjV").unwrap();
    println!("{renamed}");
    assert!(renamed.contains("The file was lane.rs at that commit"));
}

/// コミットの詳細で `B`: 見ているファイルの、そのコミットの時点の Blame。Esc で元の画面に戻る
#[test]
fn blame_from_a_commits_file() {
    let dir = tempfile::tempdir().unwrap();
    changed(dir.path());
    // 詳細へ移り、2 つ目のファイル（notes.txt）の見出しまで進めて `B`。
    let blame = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "\t]]B").unwrap();
    println!("{blame}");
    assert!(blame.lines().last().unwrap().contains("Blame at "));
    assert!(blame.lines().last().unwrap().contains(": notes.txt"));
    let list: Vec<String> = blame.lines().skip(1).filter_map(|l| column(l, 1)).collect();
    assert!(list[0].ends_with("one"), "{list:?}");
    // 最初のファイル（lane.rs）の File History（見出しが一番上に見える所から）。
    let log = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "\t]L").unwrap();
    assert!(
        log.lines()
            .last()
            .unwrap()
            .contains("File History: lane.rs")
    );
    // Esc で History に戻る。
    let back = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "\t]L\x1b").unwrap();
    assert!(back.contains("◉  [main] Take the lane from the trunk"));
}

/// History の `F` は選んでいるコミットを親にまとめ、`X` は消す（どちらも先に確かめ、Undo で戻る）
#[test]
fn fixup_and_drop_rewrite_history() {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), BASE, &["init", "-q", "-b", "main"]);
    for (i, name) in ["one", "two", "three"].iter().enumerate() {
        std::fs::write(dir.path().join(format!("{name}.txt")), name).unwrap();
        git(dir.path(), BASE, &["add", "."]);
        git(
            dir.path(),
            BASE + DAY * i as i64,
            &["commit", "-q", "-m", name],
        );
    }
    let subjects = || git_out(dir.path(), &["log", "--format=%s"]);
    let front = || {
        amonhen_model::testing::isolate_from_user_settings();
        open(dir.path())
    };
    // 一番上（three）を親（two）にまとめる。
    let asked = amonhen_tui::snapshot(front(), 140, 10, clock(), "F").unwrap();
    println!("{asked}");
    assert!(asked.contains("“three” into its parent? Undo brings it back.   y fix up   n cancel"));
    amonhen_tui::snapshot(front(), 140, 10, clock(), "Fy").unwrap();
    assert_eq!(subjects(), "two\none\n");
    assert!(dir.path().join("three.txt").exists());
    // Undo で戻る。
    amonhen_tui::snapshot(front(), 140, 10, clock(), "uy").unwrap();
    assert_eq!(subjects(), "three\ntwo\none\n");
    // 2 行目（two）を消す。
    let asked = amonhen_tui::snapshot(front(), 140, 10, clock(), "jX").unwrap();
    assert!(asked.contains("“two”? Undo brings it back.   y drop   n cancel"));
    amonhen_tui::snapshot(front(), 140, 10, clock(), "jXy").unwrap();
    assert_eq!(subjects(), "three\none\n");
    assert!(!dir.path().join("two.txt").exists());
}

/// History の `|` は選んでいるコミットで止め、変更を作業ツリーに戻す（Working Copy で分けてコミットし、Continue）
#[test]
fn split_stops_at_the_commit_with_its_changes_in_the_working_tree() {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), BASE, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.path().join("one.txt"), "one").unwrap();
    git(dir.path(), BASE, &["add", "."]);
    git(dir.path(), BASE, &["commit", "-q", "-m", "one"]);
    std::fs::write(dir.path().join("x.txt"), "x").unwrap();
    std::fs::write(dir.path().join("y.txt"), "y").unwrap();
    git(dir.path(), BASE, &["add", "."]);
    git(dir.path(), BASE + DAY, &["commit", "-q", "-m", "x and y"]);
    std::fs::write(dir.path().join("z.txt"), "z").unwrap();
    git(dir.path(), BASE, &["add", "."]);
    git(dir.path(), BASE + 2 * DAY, &["commit", "-q", "-m", "z"]);
    let subjects = || git_out(dir.path(), &["log", "--format=%s"]);
    let front = || {
        amonhen_model::testing::isolate_from_user_settings();
        open(dir.path())
    };
    let asked = amonhen_tui::snapshot(front(), 200, 10, clock(), "j|").unwrap();
    println!("{asked}");
    assert!(asked.contains("Split "), "{asked}");
    assert!(asked.contains("y split   n cancel"), "{asked}");
    // 取り消せば何も起きない。
    amonhen_tui::snapshot(front(), 200, 10, clock(), "j|n").unwrap();
    assert_eq!(subjects(), "z\nx and y\none\n");

    let stopped = amonhen_tui::snapshot(front(), 200, 20, clock(), "j|y").unwrap();
    println!("{stopped}");
    assert!(
        stopped.contains("commit its changes in parts, then continue"),
        "{stopped}"
    );
    assert!(stopped.contains("x.txt") && stopped.contains("y.txt"));
    assert_eq!(subjects(), "one\n");
    assert_eq!(
        git_out(dir.path(), &["status", "--porcelain"]),
        "?? x.txt\n?? y.txt\n"
    );
    // 分けてコミットし、続ける。
    git(dir.path(), BASE, &["add", "x.txt"]);
    git(dir.path(), BASE, &["commit", "-q", "-m", "x"]);
    git(dir.path(), BASE, &["add", "y.txt"]);
    git(dir.path(), BASE, &["commit", "-q", "-m", "y"]);
    amonhen_tui::snapshot(front(), 200, 10, clock(), "C").unwrap();
    assert_eq!(subjects(), "z\ny\nx\none\n");
    // 1 回の Undo で分ける前に戻る。
    amonhen_tui::snapshot(front(), 200, 10, clock(), "uy").unwrap();
    assert_eq!(subjects(), "z\nx and y\none\n");

    // merge でないコミットだけ。一番古いコミットはコアが断る。
    let first = amonhen_tui::snapshot(front(), 200, 10, clock(), "G|y").unwrap();
    assert!(first.contains("Can’t split the first commit"), "{first}");
}

/// History をホイールで速く回し続けても（行が届く前に次の目盛り）、描けるところだけ描いて落ちない
#[test]
fn fast_wheel_scrolling_never_outruns_the_rows() {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), BASE, &["init", "-q", "-b", "main"]);
    let mut stream = String::new();
    for i in 0..3000 {
        let message = format!("Change {i}\n");
        stream += "commit refs/heads/main\n";
        stream += &format!("committer Ann <ann@example.com> {} +0000\n", BASE + i * 60);
        stream += &format!("data {}\n{message}", message.len());
        stream += &format!("M 100644 inline f{}.txt\ndata 2\n{}\n\n", i % 10, i % 10);
    }
    let mut child = amonhen_model::process::command("git")
        .args(["fast-import", "--quiet"])
        .current_dir(dir.path())
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(&mut child.stdin.take().unwrap(), stream.as_bytes()).unwrap();
    assert!(child.wait().unwrap().success());
    git(dir.path(), BASE, &["checkout", "-q", "-f", "main"]);
    let mut scroll =
        amonhen_tui::Scroll::new(open(dir.path()), 200, 60, amonhen_tui::Step::Wheel).unwrap();
    for _ in 0..2000 {
        scroll.step().unwrap();
    }
}

/// History の複数選択: Shift-↓ で続けて、`m` で 1 つずつ印をつけ、詳細にその差分を出す（アプリと同じ取り方）
#[test]
fn several_selected_commits_show_their_diff_together() {
    let dir = tempfile::tempdir().unwrap();
    four_lines(dir.path());
    // Four と Three: 続いているので、まとめた差分（Two の後に足した 2 行）。
    let range = amonhen_tui::snapshot(open(dir.path()), 160, 24, clock(), "⇣").unwrap();
    println!("{range}");
    assert!(range.contains("2 commits · 1 author"), "{range}");
    assert!(
        range.contains("The changes of the 2 commits together"),
        "{range}"
    );
    assert!(range.contains("+2 −0"), "{range}");
    assert!(
        range.contains("+Three") && range.contains("+Four"),
        "{range}"
    );
    assert!(!range.contains("+Two"), "{range}");
    // Four と Two（Three を飛ばす）: 2 つの間の差分。`x` で向きを入れ替える。
    let pair = amonhen_tui::snapshot(open(dir.path()), 160, 24, clock(), "mjjm").unwrap();
    assert!(pair.contains("x swaps the direction"), "{pair}");
    assert!(pair.contains("+Three") && pair.contains("+Four"), "{pair}");
    println!("{pair}");
    let swapped = amonhen_tui::snapshot(open(dir.path()), 160, 24, clock(), "mjjmx").unwrap();
    assert!(
        swapped.contains("-Three") && swapped.contains("-Four"),
        "{swapped}"
    );
    // 3 つを飛び飛びに: コミットごとの差分。
    let each = amonhen_tui::snapshot(open(dir.path()), 160, 24, clock(), "mjjmjm").unwrap();
    println!("{each}");
    assert!(each.contains("3 commits"), "{each}");
    assert!(each.contains("Each commit's changes"), "{each}");
    // History を読み直しても（タグを作った）、同じコミットに印が残る。
    let reloaded = amonhen_tui::snapshot(open(dir.path()), 160, 24, clock(), "⇣Tv1\n").unwrap();
    assert!(reloaded.contains("[v1]"), "{reloaded}");
    assert!(reloaded.contains("2 commits"), "{reloaded}");
    // Esc で外すと、選んでいるコミットだけに戻る。
    let cleared = amonhen_tui::snapshot(open(dir.path()), 160, 24, clock(), "⇣\u{1b}").unwrap();
    assert!(!cleared.contains("2 commits"), "{cleared}");
    assert!(cleared.contains("Parents"), "{cleared}");
}

/// main から分かれた topic に 2 つ（別のファイル）、その後に main で 1 つ。
fn topic_of_two(dir: &Path) {
    git(dir, BASE, &["init", "-q", "-b", "main"]);
    commit(dir, BASE, "Start");
    git(dir, BASE, &["switch", "-q", "-c", "topic"]);
    for (i, name) in ["a", "b"].iter().enumerate() {
        let time = BASE + DAY * (i as i64 + 1);
        std::fs::write(dir.join(format!("{name}.txt")), "x\n").unwrap();
        git(dir, time, &["add", "."]);
        git(dir, time, &["commit", "-q", "-m", &format!("Add {name}")]);
    }
    git(dir, BASE, &["switch", "-q", "main"]);
    commit(dir, BASE + DAY * 3, "Main work");
}

/// 印をつけたコミットの操作: 古い順に cherry-pick、新しい順に revert（どちらも 1 つの命令で、前と後の絵つき）
#[test]
fn cherry_pick_and_revert_the_selected_commits() {
    let dir = tempfile::tempdir().unwrap();
    topic_of_two(dir.path());
    let log = || git_out(dir.path(), &["log", "--format=%s", "main"]);
    // Add b と Add a に印をつけて `y`。
    let asked = amonhen_tui::snapshot(open(dir.path()), 160, 14, clock(), "j⇣y").unwrap();
    println!("{asked}");
    assert!(
        asked.contains("Cherry-pick 2 commits onto main, oldest first?"),
        "{asked}"
    );
    assert!(asked.contains(" Cherry-pick 2 commits "), "{asked}");
    amonhen_tui::snapshot(open(dir.path()), 160, 14, clock(), "j⇣yy").unwrap();
    assert_eq!(log(), "Add b\nAdd a\nMain work\nStart\n");
    // 上の 2 つ（今 cherry-pick したもの）を打ち消す: 新しい方から。
    let asked = amonhen_tui::snapshot(open(dir.path()), 160, 14, clock(), "⇣v").unwrap();
    assert!(
        asked.contains("Revert 2 commits on main, newest first?"),
        "{asked}"
    );
    amonhen_tui::snapshot(open(dir.path()), 160, 14, clock(), "⇣vy").unwrap();
    assert_eq!(
        log(),
        "Revert \"Add a\"\nRevert \"Add b\"\nAdd b\nAdd a\nMain work\nStart\n"
    );
    // 1 つのコミットにしかできない操作は断る。
    let refused = amonhen_tui::snapshot(open(dir.path()), 160, 14, clock(), "⇣c").unwrap();
    assert!(
        refused.contains("This works on one commit: Esc clears the selection first"),
        "{refused}"
    );
}

/// 印をつけた続くコミットを `E` で 1 つにまとめる（編集モードを、まとめる行にして開く）
#[test]
fn e_combines_the_selected_commits_in_edit_mode() {
    let dir = tempfile::tempdir().unwrap();
    four_lines(dir.path());
    let log = || git_out(dir.path(), &["log", "--format=%s"]);
    let opened = amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), "j⇣E").unwrap();
    println!("{opened}");
    assert!(opened.contains(" Edit Commits "), "{opened}");
    assert!(opened.contains("Changes: 1 combined."), "{opened}");
    amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), "j⇣E\ny").unwrap();
    assert_eq!(log().lines().count(), 3, "{}", log());
    assert!(log().ends_with("One\n"), "{}", log());
    assert!(
        git_out(dir.path(), &["log", "-1", "--skip=1", "--format=%B"]).contains("Three"),
        "the combined message keeps both"
    );
    // 続いていなければ断る。
    let dir = tempfile::tempdir().unwrap();
    four_lines(dir.path());
    let refused = amonhen_tui::snapshot(open(dir.path()), 160, 16, clock(), "mjjmE").unwrap();
    assert!(
        refused.contains("Only commits next to each other on the current branch can be combined"),
        "{refused}"
    );
    assert!(!refused.contains(" Edit Commits "), "{refused}");
}

/// `Y` は ID をコピーし、Save as Patch は印をつけたものを古い順にパッチにする
#[test]
fn copy_ids_and_save_the_selected_commits_as_patches() {
    let dir = tempfile::tempdir().unwrap();
    four_lines(dir.path());
    let copied = amonhen_tui::snapshot(open(dir.path()), 160, 10, clock(), "⇣Y").unwrap();
    assert!(
        copied.contains("Copied 2 commit IDs (to the terminal’s clipboard)"),
        "{copied}"
    );
    let one = amonhen_tui::snapshot(open(dir.path()), 160, 10, clock(), "Y").unwrap();
    assert!(one.contains("(to the terminal’s clipboard)"), "{one}");
    let out = tempfile::tempdir().unwrap();
    let folder = out.path().join("patches");
    let keys = format!("⇣:save as patch\n\x15{}\nc", folder.display());
    let done = amonhen_tui::snapshot(open(dir.path()), 200, 8, clock(), &keys).unwrap();
    assert!(done.contains("Saved 3 patches in "), "{done}");
    let mut names: Vec<String> = std::fs::read_dir(&folder)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    // 送り状を付けた。
    assert_eq!(
        names,
        [
            "0000-cover-letter.patch",
            "0001-Three.patch",
            "0002-Four.patch"
        ]
    );
}
