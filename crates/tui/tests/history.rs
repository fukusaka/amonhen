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
    let help = amonhen_tui::snapshot(open(dir.path()), 80, 30, clock(), "?").unwrap();
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
    let asked = amonhen_tui::snapshot(open(&work), 100, 8, clock(), "P").unwrap();
    println!("{asked}");
    // 一番上の行に、上流より進んでいる数。
    assert!(asked.lines().next().unwrap().contains("main ↑1"));
    assert!(
        asked.contains(
            "Push main to origin/main (1 commit)?   y push   t another branch…   n cancel"
        )
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

/// `f` で fetch、`p` で pull: リモートに増えたコミットを取り込む
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

    let fetched = amonhen_tui::snapshot(open(&work), 100, 8, clock(), "f").unwrap();
    println!("{fetched}");
    assert!(fetched.contains("Fetched"));
    assert!(fetched.lines().next().unwrap().contains("main ↓1"));
    assert!(fetched.contains("[origin/main] From elsewhere"));
    let pulled = amonhen_tui::snapshot(open(&work), 100, 8, clock(), "p").unwrap();
    println!("{pulled}");
    assert!(pulled.contains("Pulled"));
    assert_eq!(
        git_out(&work, &["log", "-1", "--format=%s"]),
        "From elsewhere\n"
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
    let asked = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "0jjjm").unwrap();
    assert!(asked.contains("Merge topic into main?   y merge   n cancel"));
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
    let asked = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "A").unwrap();
    assert!(asked.contains("Abort the merge?   y abort   n cancel"));
    let aborted = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "Ay").unwrap();
    assert!(aborted.contains("Aborted"));
    assert!(!dir.path().join(".git/MERGE_HEAD").exists());
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
    assert!(asked.contains("Rebase topic onto main?   y rebase   n cancel"));
    amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "0jjry").unwrap();
    assert_eq!(
        git_out(dir.path(), &["merge-base", "topic", "main"]),
        git_out(dir.path(), &["rev-parse", "main"])
    );
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
    let asked = amonhen_tui::snapshot(open(dir.path()), 120, 10, clock(), "s\n").unwrap();
    assert!(asked.contains(
        "Stash the working changes?   y tracked files   u with untracked files   k keep what is staged   n cancel"
    ));
    let keys = "sbefore lunch\ny";
    let stashed = amonhen_tui::snapshot(open(dir.path()), 100, 10, clock(), keys).unwrap();
    println!("{stashed}");
    assert!(stashed.contains("Stashed"));
    assert_eq!(read(dir.path()), "one\ntwo\nthree\nfour\n");
    assert!(git_out(dir.path(), &["stash", "list"]).contains(": before lunch\n"));
    let popped = amonhen_tui::snapshot(open(dir.path()), 100, 10, clock(), "S").unwrap();
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

/// サイドバーの Stashes: メッセージで並び、`a` で戻す、`x` で確かめてから消す
#[test]
fn stashes_are_listed_and_applied_from_the_sidebar() {
    let dir = tempfile::tempdir().unwrap();
    two_lines(dir.path());
    git(dir.path(), BASE, &["stash", "push", "-q", "-m", "Capitals"]);
    let read = |dir: &Path| std::fs::read_to_string(dir.join("a.txt")).unwrap();
    let text = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "").unwrap();
    println!("{text}");
    let side: Vec<String> = text.lines().skip(1).filter_map(|l| column(l, 0)).collect();
    let at = side.iter().position(|l| l == "Stashes").unwrap();
    assert!(side[at + 1].contains("Capitals"), "{side:?}");
    // 一番下の行（stash）で `a`: 変更が戻り、stash は残る。
    let applied = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "0Ga").unwrap();
    assert!(applied.contains("Applied the stash"));
    assert_eq!(read(dir.path()), "ONE\ntwo\nTHREE\nfour\n");
    assert_eq!(git_out(dir.path(), &["stash", "list"]).lines().count(), 1);
    let asked = amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "0Gx").unwrap();
    assert!(asked.contains("Drop stash@{0}? Undo brings it back.   y drop   n cancel"));
    amonhen_tui::snapshot(open(dir.path()), 160, 12, clock(), "0Gxy").unwrap();
    assert_eq!(git_out(dir.path(), &["stash", "list"]), "");
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
    assert!(last.contains("s soft (keep the index and files)   m mixed (keep the files)"));
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
    let done = amonhen_tui::snapshot(open(dir.path()), 200, 8, clock(), &keys).unwrap();
    println!("{done}");
    assert!(done.contains("Exported to"));
    let bytes = std::fs::read(&zip).unwrap();
    assert_eq!(&bytes[..2], b"PK");
    // 中身は、ファイルの名前のフォルダの下に。
    assert!(bytes.windows(11).any(|w| w == b"snap/lane.r"));
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
    let done = amonhen_tui::snapshot(open(dir.path()), 200, 8, clock(), &keys).unwrap();
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
    assert!(
        asked.contains("Apply the patch to the current branch as a commit?   y apply   n cancel")
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
    let asked = amonhen_tui::snapshot(open(dir.path()), 200, 8, clock(), &keys).unwrap();
    println!("{asked}");
    assert!(asked.contains("Apply the diff to the working tree?"));
    amonhen_tui::snapshot(open(dir.path()), 200, 8, clock(), &format!("{keys}y")).unwrap();
    assert_eq!(git_out(dir.path(), &["diff", "--name-only"]), "lane.rs\n");
    assert_eq!(
        git_out(dir.path(), &["diff", "--cached", "--name-only"]),
        ""
    );
    // もう当たらない（同じ diff をもう一度）。
    let again = amonhen_tui::snapshot(open(dir.path()), 200, 8, clock(), &keys).unwrap();
    assert!(again.contains("The patch doesn’t apply"));
}

/// コマンドの一覧の Save as Patch: History で選んでいるコミットを、git の名前のパッチにしてフォルダに書く
#[test]
fn save_the_selected_commit_as_a_patch() {
    let dir = tempfile::tempdir().unwrap();
    changed(dir.path());
    let out = tempfile::tempdir().unwrap();
    let folder = out.path().join("patches");
    let keys = format!(":save as patch\n\x15{}\n", folder.display());
    let done = amonhen_tui::snapshot(open(dir.path()), 200, 8, clock(), &keys).unwrap();
    println!("{done}");
    assert!(done.contains("Saved "));
    let patch = folder.join("0001-Take-the-lane-from-the-trunk.patch");
    let text = std::fs::read_to_string(&patch).unwrap();
    assert!(text.contains("Subject: [PATCH] Take the lane from the trunk"));
    assert!(text.contains("+    let lane = trunk();"));
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

/// サイドバーの worktree の行で `x`: 確かめてから消す。変更が残っていれば git が断り、開いている worktree は
/// 消せない。
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
    // 変更が残っていると、git が断る（理由が出て、フォルダは残る）。
    std::fs::write(other.join("new.txt"), "new\n").unwrap();
    let refused = amonhen_tui::snapshot(open(&main), 200, 16, clock(), "0Gxy").unwrap();
    println!("{refused}");
    assert!(!refused.contains("Removed the worktree"));
    assert!(other.exists());
    std::fs::remove_file(other.join("new.txt")).unwrap();
    let removed = amonhen_tui::snapshot(open(&main), 200, 16, clock(), "0Gxy").unwrap();
    println!("{removed}");
    assert!(removed.contains("Removed the worktree"));
    assert!(!other.exists());
    // worktree が 1 つになったので、サイドバーからも消える。
    assert!(!removed.contains("Worktrees"));
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
    let pull = amonhen_tui::snapshot(open(&work), 120, 8, clock(), "p").unwrap();
    println!("{pull}");
    assert!(pull.contains("have diverged. How?   y as git is set   m merge   r rebase   n cancel"));
    let asked = amonhen_tui::snapshot(open(&work), 140, 8, clock(), "P").unwrap();
    println!("{asked}");
    assert!(
        asked.contains("y push   f force push (with lease), replacing 1 on the remote   t another branch…   n cancel")
    );
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
    let stopped = amonhen_tui::snapshot(open(dir.path()), 120, 10, clock(), "jyy").unwrap();
    assert!(stopped.contains("Stopped at conflicts"));
    amonhen_tui::snapshot(open(dir.path()), 120, 10, clock(), "Ay").unwrap();
    // 一番上（On main）を打ち消す。
    let asked = amonhen_tui::snapshot(open(dir.path()), 120, 10, clock(), "v").unwrap();
    assert!(asked.contains("Revert "));
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

/// `s` の `u` は、追跡していないファイルも stash に入れる
#[test]
fn stash_can_include_untracked_files() {
    let dir = tempfile::tempdir().unwrap();
    two_lines(dir.path());
    std::fs::write(dir.path().join("new.txt"), "new\n").unwrap();
    amonhen_tui::snapshot(open(dir.path()), 100, 10, clock(), "s\ny").unwrap();
    assert!(dir.path().join("new.txt").exists());
    amonhen_tui::snapshot(open(dir.path()), 100, 10, clock(), "S").unwrap();
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
    let asked = amonhen_tui::snapshot(front(), 120, 14, clock(), "1 cAdd a file\x10").unwrap();
    println!("{asked}");
    assert_eq!(
        git_out(&work, &["log", "-1", "--format=%s"]),
        "Add a file\n"
    );
    assert!(
        asked.contains(
            "Push main to origin/main (1 commit)?   y push   t another branch…   n cancel"
        )
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
    let asked = amonhen_tui::snapshot(open(dir.path()), 200, 12, clock(), "1LjV").unwrap();
    println!("{asked}");
    assert!(asked.contains("Restore lane.rs to its version at "));
    assert!(asked.contains("y restore   n cancel"));
    assert_eq!(read(), edited);
    let restored = amonhen_tui::snapshot(open(dir.path()), 200, 12, clock(), "1LjVy").unwrap();
    println!("{restored}");
    assert!(restored.contains("Restored the version"));
    assert_eq!(read(), git_out(dir.path(), &["show", "HEAD~1:lane.rs"]));
    // Undo で、戻す前の中身に。
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
