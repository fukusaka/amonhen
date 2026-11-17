//! Working Copy の画面: 上にコミットの入力と Commit、その下に変更のあるファイルの一覧（チェックで次のコミットに
//! 含めるか）。GTK4 版の `workcopy.rs`（macOS 版の `WorkingCopyViewController`）と同じ並び。
//!
//! チェックは index の状態そのもので、FE だけの選択状態は持たない（ターミナルで `git add` しても揃う）。

use amonhen_core::StatusSnapshot;
use amonhen_model::{StatusEntry, StatusKind, tr};
use windows_reactor::*;

/// 画面の中身を作るのに要るもの。
/// 件名と本文をコミットのメッセージにする（本文があれば空行で分ける）。
pub fn message(summary: &str, description: &str) -> String {
    let (summary, description) = (summary.trim(), description.trim());
    if description.is_empty() {
        summary.to_owned()
    } else {
        format!("{summary}\n\n{description}")
    }
}

/// パスの絞り込みに合うファイル（大文字小文字を区別しない。空なら全部）。一覧の行の番号はこの並びの番号。
pub fn visible<'a>(entries: &'a [StatusEntry], filter: &str) -> Vec<&'a StatusEntry> {
    let filter = filter.trim().to_lowercase();
    entries
        .iter()
        .filter(|e| {
            filter.is_empty()
                || e.path.to_lowercase().contains(&filter)
                || e.orig_path
                    .as_ref()
                    .is_some_and(|p| p.to_lowercase().contains(&filter))
        })
        .collect()
}

/// 一覧の見せ方（GTK4 版・macOS 版と同じ 3 つ。設定に覚える名前は `list`・`tree`・`flat`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// パスの一覧。
    List,
    /// ディレクトリの木。
    Tree,
    /// ファイル名を先に、親のディレクトリを薄く。
    Flat,
}

impl Layout {
    pub fn name(self) -> &'static str {
        match self {
            Layout::List => "list",
            Layout::Tree => "tree",
            Layout::Flat => "flat",
        }
    }

    pub fn from_name(name: &str) -> Self {
        match name {
            "tree" => Layout::Tree,
            "flat" => Layout::Flat,
            _ => Layout::List,
        }
    }
}

/// 一覧の 1 行: ディレクトリ（木のときだけ）か、ファイル。
pub enum Row<'a> {
    Dir {
        name: String,
        path: String,
        depth: usize,
        /// 畳んでいるか（中の行を出さない）。
        collapsed: bool,
        /// 中の（コンフリクト以外の）ファイルのパスと、すべて含めているか・一部でも含めているか。
        files: Vec<String>,
        all: bool,
        some: bool,
    },
    File {
        entry: &'a StatusEntry,
        depth: usize,
    },
}

/// 一覧の行（`entries` は絞り込んだもの）。木では、ディレクトリの行と、その中のファイル（コンフリクトを先に）。
/// 子が 1 つのディレクトリだけなら名前をつなぐ（GTK4 版の `tree`）。`collapsed` のディレクトリの中は出さない。
pub fn rows<'a>(
    entries: &[&'a StatusEntry],
    layout: Layout,
    collapsed: &std::collections::HashSet<String>,
) -> Vec<Row<'a>> {
    if layout != Layout::Tree {
        return entries
            .iter()
            .map(|e| Row::File { entry: e, depth: 0 })
            .collect();
    }
    #[derive(Default)]
    struct Dir<'a> {
        dirs: std::collections::BTreeMap<String, Dir<'a>>,
        files: Vec<&'a StatusEntry>,
    }
    let mut root = Dir::default();
    for e in entries {
        let mut dir = &mut root;
        let parts: Vec<&str> = e.path.split('/').collect();
        for part in &parts[..parts.len() - 1] {
            dir = dir.dirs.entry((*part).to_owned()).or_default();
        }
        dir.files.push(e);
    }
    /// `dir` の行を `out` に足し、中の（コンフリクト以外の）ファイルを返す。
    fn walk<'a>(
        dir: &Dir<'a>,
        prefix: &str,
        depth: usize,
        out: &mut Vec<Row<'a>>,
    ) -> Vec<&'a StatusEntry> {
        let mut inside_all = Vec::new();
        for (name, child) in &dir.dirs {
            let (mut name, mut child) = (name.clone(), child);
            while child.files.is_empty() && child.dirs.len() == 1 {
                let (n, c) = child.dirs.iter().next().expect("one");
                name = format!("{name}/{n}");
                child = c;
            }
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let at = out.len();
            out.push(Row::Dir {
                name,
                path: path.clone(),
                depth,
                collapsed: false,
                files: Vec::new(),
                all: false,
                some: false,
            });
            let inside = walk(child, &path, depth + 1, out);
            if let Row::Dir {
                files, all, some, ..
            } = &mut out[at]
            {
                *files = inside.iter().map(|e| e.path.clone()).collect();
                *all = !inside.is_empty() && inside.iter().all(|e| included(e));
                *some = inside.iter().any(|e| e.is_staged());
            }
            inside_all.extend(inside);
        }
        let mut files = dir.files.clone();
        files.sort_by_key(|e| (e.kind != StatusKind::Unmerged, &e.path));
        for e in files {
            if e.kind != StatusKind::Unmerged {
                inside_all.push(e);
            }
            out.push(Row::File { entry: e, depth });
        }
        inside_all
    }
    let mut out = Vec::new();
    walk(&root, "", 0, &mut out);
    // 畳んだディレクトリの中の行を除く（チェックは中のファイルすべてから数えてある）。
    let mut folded_at: Option<usize> = None;
    out.retain_mut(|row| {
        let depth = match row {
            Row::Dir { depth, .. } | Row::File { depth, .. } => *depth,
        };
        if folded_at.is_some_and(|d| depth > d) {
            return false;
        }
        folded_at = None;
        if let Row::Dir {
            path,
            collapsed: folded,
            ..
        } = row
            && collapsed.contains(path.as_str())
        {
            *folded = true;
            folded_at = Some(depth);
        }
        true
    });
    out
}

/// `.gitignore` の中身に `pattern` の行を足したもの（もうあれば None。GTK4 版の `with_ignore_pattern`）。改行は
/// ファイルに合わせる（Windows では CRLF のことが多い）。
pub fn with_ignore_pattern(gitignore: &str, pattern: &str) -> Option<String> {
    if gitignore.lines().any(|l| l.trim() == pattern) {
        return None;
    }
    let newline = if gitignore.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut text = gitignore.to_owned();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push_str(newline);
    }
    text.push_str(pattern);
    text.push_str(newline);
    Some(text)
}

/// untracked のファイルを .gitignore に足すときのパターン（そのパスと、拡張子があれば `*.ext`）。
fn ignore_patterns(path: &str) -> Vec<String> {
    let mut patterns = vec![path.to_owned()];
    let name = path.rsplit('/').next().unwrap_or(path);
    // `.env` のように名前だけのものに拡張子はない（GTK4 版・macOS 版と同じ）。
    if let Some((stem, ext)) = name.rsplit_once('.')
        && !stem.is_empty()
        && !ext.is_empty()
    {
        patterns.push(format!("*.{ext}"));
    }
    patterns
}

/// Conventional Commits の型（GTK4 版・macOS 版と同じ並び）。
const TYPES: [&str; 11] = [
    "feat", "fix", "docs", "style", "refactor", "perf", "test", "build", "ci", "chore", "revert",
];

/// 件名の先頭の `type(scope)!: ` を外したもの（`^[a-z]+(\([^)]*\))?!?:\s*`。GTK4 版・macOS 版と同じ）。
pub fn without_commit_type(subject: &str) -> &str {
    let rest = subject.trim_start_matches(|c: char| c.is_ascii_lowercase());
    if rest.len() == subject.len() {
        return subject;
    }
    let rest = match rest.strip_prefix('(') {
        Some(scope) => match scope.split_once(')') {
            Some((_, after)) => after,
            None => return subject,
        },
        None => rest,
    };
    let rest = rest.strip_prefix('!').unwrap_or(rest);
    rest.strip_prefix(':')
        .map_or(subject, |after| after.trim_start())
}

/// 本文の末尾にトレーラーを足したもの（もうあれば None。前の行がトレーラーでなければ空行で分ける）。
pub fn with_trailer(body: &str, line: &str) -> Option<String> {
    if body.contains(line) {
        return None;
    }
    let body = body.trim();
    let last = body.lines().last().unwrap_or_default();
    let last_is_trailer = last.split_once(": ").is_some_and(|(k, _)| {
        !k.is_empty() && k.chars().all(|c| c.is_ascii_alphabetic() || c == '-')
    });
    Some(if body.is_empty() {
        line.to_owned()
    } else if last_is_trailer {
        format!("{body}\n{line}")
    } else {
        format!("{body}\n\n{line}")
    })
}

/// 最近のメッセージ（`recent:<番号>`。同じメッセージは 1 つに）。
fn recent_menu(recent: &[(String, String, String)]) -> Vec<MenuItem> {
    let mut seen: Vec<&str> = Vec::new();
    let mut items = Vec::new();
    for (i, (message, _, _)) in recent.iter().enumerate() {
        if seen.contains(&message.as_str()) {
            continue;
        }
        seen.push(message);
        let subject: String = message
            .lines()
            .next()
            .unwrap_or_default()
            .chars()
            .take(80)
            .collect();
        items.push(MenuItem::item(format!("recent:{i}"), subject));
    }
    if items.is_empty() {
        items.push(MenuItem::disabled("recent-none", tr!("Not loaded yet")));
    }
    items
}

/// 「+」のメニュー（GTK4 版の `insert_menu`）: Conventional Commits の型と Co-authored-by は下のメニュー、
/// Signed-off-by はそのまま。
fn insert_menu(recent: &[(String, String, String)]) -> Vec<MenuItem> {
    let types = TYPES
        .iter()
        .map(|t| MenuItem::item(format!("type:{t}"), format!("{t}:")));
    let mut authors = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for (_, author, email) in recent {
        if email.is_empty() || seen.contains(&email.to_lowercase()) {
            continue;
        }
        seen.push(email.to_lowercase());
        authors.push(MenuItem::item(
            format!("trailer:Co-authored-by: {author} <{email}>"),
            format!("{author} <{email}>"),
        ));
    }
    if authors.is_empty() {
        authors.push(MenuItem::disabled(
            "authors-none",
            tr!("Recent authors aren’t loaded yet"),
        ));
    }
    vec![
        MenuItem::submenu("types", tr!("Conventional Commits Type"), types),
        MenuItem::submenu("authors", "Co-authored-by", authors),
        MenuItem::item("signoff", tr!("Signed-off-by (Me)")),
    ]
}

/// Commit ▾ のメニュー（GTK4 版の Commit の横の ▾ と `install_fixups`）: Commit and Push・Amend Last Commit と、
/// 最近のコミットの件名への Fixup Commit For・Squash Commit For。
fn commit_menu(recent: &[(String, String, String)], can_commit: bool) -> Vec<MenuItem> {
    let mut items = vec![
        if can_commit {
            MenuItem::item("commit-push", "Commit and Push")
        } else {
            MenuItem::disabled("commit-push", "Commit and Push")
        },
        MenuItem::item("amend", "Amend Last Commit"),
    ];
    let mut subjects: Vec<&str> = Vec::new();
    for (message, _, _) in recent {
        let subject = message.lines().next().unwrap_or_default();
        // fixup! を重ねない（rebase --autosquash は元のコミットの件名で探す）。
        if subject.is_empty()
            || subject.starts_with("fixup! ")
            || subject.starts_with("squash! ")
            || subjects.contains(&subject)
        {
            continue;
        }
        subjects.push(subject);
        if subjects.len() == 15 {
            break;
        }
    }
    if !subjects.is_empty() {
        items.push(MenuItem::separator("-fixup"));
        for (key, title) in [
            ("fixup", "Fixup Commit For"),
            ("squash", "Squash Commit For"),
        ] {
            items.push(MenuItem::submenu(
                key,
                title,
                subjects
                    .iter()
                    .map(|s| MenuItem::item(format!("{key}:{s}"), *s)),
            ));
        }
    }
    items
}

pub struct Input<'a> {
    pub status: Option<&'a StatusSnapshot>,
    /// パスの絞り込み。
    pub filter: &'a str,
    /// sparse checkout なら、その説明（一覧の上に出す。GTK4 版の Working Copy の見出しの注記）。
    pub sparse: Option<&'a str>,
    /// ファイルごとの行数（追加, 削除。バイナリは None）。読めていなければ空。
    pub stats: &'a std::collections::HashMap<String, (Option<u64>, Option<u64>)>,
    /// 木で畳んでいるディレクトリ。
    pub collapsed: &'a std::collections::HashSet<String>,
    /// 一覧の見せ方と、切り替えのボタンを押した回数（押すたびにボタンを作り直し、押したままのボタンを
    /// もう一度押しても外れないようにする）。
    pub layout: Layout,
    pub layout_clicks: u64,
    /// 一覧を包む Grid（Space・Delete のキーを付ける）。
    pub list_host: &'a ElementRef<Grid>,
    /// 最近のコミットの（メッセージ, 作者, メール）。新しい順。
    pub recent: &'a [(String, String, String)],
    /// 件名と本文。
    pub summary: &'a str,
    pub description: &'a str,
    /// コミットしている最中（Commit を押せない）。
    pub committing: bool,
    /// 直前のコミットをやり直す（Amend）。
    pub amend: bool,
    /// 途中の操作（1 行目の見出し、2 行目の説明、Skip できるか）。あれば上に帯を出す。
    pub sequence: Option<(&'a str, &'a str, bool)>,
    /// 止まった git am を 3-way マージでやり直せるか（帯に Retry with 3-Way Merge を出す。GTK4 版と同じ条件）。
    pub retry_three_way: bool,
}

/// 画面が受ける操作。
pub struct Callbacks<'a> {
    pub summary: Callback<String>,
    /// 件名の欄（Alt+↑・Alt+↓ で最近のメッセージを呼び出すキーを付ける）。
    pub summary_box: &'a ElementRef<TextBox>,
    pub description: Callback<String>,
    pub commit: Callback<()>,
    /// パスのファイルを含める（true）・外す（false）。
    pub stage: Box<dyn Fn(String) -> Callback<bool> + 'a>,
    pub selected: Callback<Option<usize>>,
    pub filter: Callback<String>,
    /// 一覧の見せ方を替える。
    pub layout: Box<dyn Fn(Layout) -> Callback<bool> + 'a>,
    /// ディレクトリの中のファイルをまとめて含める（true）・外す（false）。
    pub stage_all: Box<dyn Fn(Vec<String>, bool) -> Callback<bool> + 'a>,
    /// 最近のメッセージ・「+」のメニューで選んだ項目のキー。
    pub message_menu: Callback<String>,
    pub amend: Callback<bool>,
    /// パスのファイルの右クリックのメニュー（File History・Discard File。押した項目のキー）。
    pub file_menu: Box<dyn Fn(String) -> Callback<String> + 'a>,
    /// 途中の操作の帯のボタン（`continue`・`skip`・`abort`）。
    pub sequence: Box<dyn Fn(&'static str) -> Callback<()> + 'a>,
}

/// 一覧の 1 行の印（M・A・D・R・?・U）。作業ツリー側に変更があればそちら、なければ index 側。
fn mark(entry: &StatusEntry) -> char {
    match entry.kind {
        StatusKind::Untracked => '?',
        StatusKind::Unmerged => 'U',
        StatusKind::Renamed => 'R',
        StatusKind::Changed => {
            let side = if entry.worktree != b'.' {
                entry.worktree
            } else {
                entry.index
            };
            char::from(side)
        }
    }
}

/// 次のコミットに含めているか（stage 済みで、作業ツリーに残りがない）。
pub fn included(entry: &StatusEntry) -> bool {
    entry.is_staged() && !entry.is_unstaged()
}

pub fn view(input: Input, callbacks: Callbacks) -> View {
    let all: &[StatusEntry] = input.status.map_or(&[], |s| &s.status.entries);
    let staged = all.iter().filter(|e| e.is_staged()).count();
    let entries = visible(all, input.filter);
    // 件名と本文（macOS 版と同じく、件名は 1 行、本文は空なら薄く「Description (optional)」）。
    // ボタンに付けたメニューは、押すと開く（Button.Flyout）。
    let menu_button = |title: &str, tip: String, items: Vec<MenuItem>, column: i32| -> View {
        Button::new()
            .margin(Thickness::new(4.0, 0.0, 0.0, 0.0))
            .grid_column(column)
            .content(TextBlock::new().text(title))
            .tooltip(tip)
            .menu(Menu::new(items, callbacks.message_menu.clone()))
    };
    let summary = Grid::new()
        .columns([GridLength::Star(1.0), GridLength::Auto, GridLength::Auto])
        .children((
            TextBox::new()
                .element_ref(callbacks.summary_box)
                .text(input.summary)
                .placeholder_text("Summary")
                .grid_column(0)
                .on_text_changed(callbacks.summary),
            menu_button(
                "🕘",
                tr!("Recent commit messages"),
                recent_menu(input.recent),
                1,
            ),
            menu_button(
                "+",
                tr!("Insert a Conventional Commits type, Co-authored-by or Signed-off-by"),
                insert_menu(input.recent),
                2,
            ),
        ));
    let message = StackPanel::new().spacing(6.0).grid_row(1).children((
        summary,
        TextBox::new()
            .text(input.description)
            .placeholder_text(tr!("Description (optional)"))
            .accepts_return(true)
            .text_wrapping(TextWrapping::Wrap)
            .height(80.0)
            .on_text_changed(callbacks.description),
    ));
    // Amend なら含めるファイルがなくても押せる（メッセージだけ直す。macOS 版と同じ）。
    // 見出しのチェック: すべて（コンフリクト以外）を含める・外す。一部だけなら薄く（GTK4 版の一覧の見出し）。
    let checkable: Vec<&StatusEntry> = all
        .iter()
        .filter(|e| e.kind != StatusKind::Unmerged)
        .collect();
    let all_in = !checkable.is_empty() && checkable.iter().all(|e| included(e));
    let some_in = checkable.iter().any(|e| e.is_staged());
    let header_check = CheckBox::new()
        .is_checked(some_in)
        .is_enabled(!checkable.is_empty())
        .opacity(if some_in && !all_in { 0.5 } else { 1.0 })
        .on_is_checked_changed((callbacks.stage_all)(
            checkable.iter().map(|e| e.path.clone()).collect(),
            !all_in,
        ));
    let has_message = !input.summary.trim().is_empty();
    let can_commit = !input.committing && (staged > 0 || input.amend) && has_message;
    // 押せないときは、ツールチップでわけを言う（docs/ux.md の決まり。GTK4 版・macOS 版と同じ）。
    let mut reasons = Vec::new();
    if input.committing {
        reasons.push(tr!("Committing…"));
    } else {
        if staged == 0 && !input.amend {
            reasons.push(tr!("No files are checked"));
        }
        if !has_message {
            reasons.push(tr!("Write a commit message"));
        }
    }
    let commit_tip = if reasons.is_empty() {
        tr!("Commit the checked files (Ctrl+Return)")
    } else {
        reasons.join("\n")
    };
    // 含めるファイルの数も出す（GTK4 版・macOS 版と同じ）。
    let label = match (input.amend, staged) {
        (true, _) => "Amend Commit".to_owned(),
        (false, 0) => "Commit".to_owned(),
        (false, 1) => "Commit 1 File".to_owned(),
        (false, n) => format!("Commit {n} Files"),
    };
    let commit = Grid::new()
        .columns([
            GridLength::Star(1.0),
            GridLength::Auto,
            GridLength::Auto,
            GridLength::Auto,
        ])
        .margin(Thickness::new(0.0, 8.0, 0.0, 8.0))
        .grid_row(2)
        .children((
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .grid_column(0)
                .children((
                    header_check,
                    TextBlock::new()
                        .text(format!("{staged} / {}", all.len()))
                        .vertical_alignment(VerticalAlignment::Center)
                        .tooltip(tr!("Files in the next commit / changed files")),
                )),
            CheckBox::new()
                .is_checked(input.amend)
                .on_is_checked_changed(callbacks.amend)
                .grid_column(1)
                .content(TextBlock::new().text("Amend")),
            Button::new()
                .is_enabled(can_commit)
                .on_click(callbacks.commit)
                .grid_column(2)
                .content(TextBlock::new().text(label))
                .tooltip(commit_tip),
            Button::new()
                .margin(Thickness::new(2.0, 0.0, 0.0, 0.0))
                .grid_column(3)
                .content(TextBlock::new().text("▾"))
                .tooltip(tr!("Commit and Push (Ctrl+Shift+Return), Amend"))
                .menu(Menu::new(
                    commit_menu(input.recent, can_commit),
                    callbacks.message_menu.clone(),
                )),
        ));
    // 絞り込みと、一覧の見せ方の切り替え（GTK4 版の一覧の上の 3 つのボタン）。
    let layout_button = |layout: Layout, title: &str, tip: String, column: i32| -> View {
        ToggleButton::new()
            .is_checked(input.layout == layout)
            .on_is_checked_changed((callbacks.layout)(layout))
            .margin(Thickness::new(4.0, 0.0, 0.0, 0.0))
            .grid_column(column)
            .content(TextBlock::new().text(title))
            .tooltip(tip)
    };
    let sparse_note: View = match input.sparse {
        Some(note) => TextBlock::new()
            .text(note.to_owned())
            .font_size(12.0)
            .opacity(0.65)
            .text_wrapping(TextWrapping::Wrap)
            .margin(Thickness::new(0.0, 0.0, 0.0, 4.0))
            .into(),
        None => View::empty(),
    };
    let filter_row = Grid::new()
        .columns([
            GridLength::Star(1.0),
            GridLength::Auto,
            GridLength::Auto,
            GridLength::Auto,
        ])
        .margin(Thickness::new(0.0, 0.0, 0.0, 6.0))
        .children((
            TextBox::new()
                .text(input.filter)
                .placeholder_text(tr!("Filter"))
                .grid_column(0)
                .on_text_changed(callbacks.filter),
            View::keyed_fragment([(
                input.layout_clicks,
                View::fragment((
                    layout_button(Layout::List, "☰", tr!("Show files as a list of paths"), 1),
                    layout_button(Layout::Tree, "🗀", tr!("Show files as a directory tree"), 2),
                    layout_button(
                        Layout::Flat,
                        "🗎",
                        tr!("Show the file name first and the parent directory dimmed"),
                        3,
                    ),
                )),
            )]),
        ));
    // sparse checkout の注記は、絞り込みの欄の上。
    let filter = StackPanel::new()
        .grid_row(3)
        .children((sparse_note, filter_row));
    let indent = |depth: usize| Thickness::new(18.0 * depth as f64, 0.0, 0.0, 0.0);
    let rows = rows(&entries, input.layout, input.collapsed)
        .into_iter()
        .map(|row| {
            let (entry, depth) = match row {
                Row::Dir {
                    name,
                    path,
                    depth,
                    collapsed,
                    files,
                    all,
                    some,
                } => {
                    // 一部だけ含めているディレクトリは、薄いチェック（reactor の CheckBox に 3 つ目の状態がない）。
                    let check = CheckBox::new()
                        .is_checked(some)
                        .is_enabled(!files.is_empty())
                        .on_is_checked_changed((callbacks.stage_all)(files, !all))
                        .opacity(if some && !all { 0.5 } else { 1.0 })
                        .grid_column(0);
                    let row = Grid::new()
                        .columns([GridLength::Auto, GridLength::Star(1.0)])
                        .margin(indent(depth))
                        .children((
                            check,
                            TextBlock::new()
                                // 行を選ぶと畳む・開く（GTK4 版の ▸・▾）。
                                .text(format!("{}  🗀  {name}", if collapsed { "▸" } else { "▾" }))
                                .opacity(0.8)
                                .vertical_alignment(VerticalAlignment::Center)
                                .text_trimming(TextTrimming::CharacterEllipsis)
                                .grid_column(1),
                        ));
                    let key = format!("dir:{path}");
                    return (key.clone(), ListViewItem::new().tag(key).content(row));
                }
                Row::File { entry, depth } => (entry, depth),
            };
            let (parent, file) = entry.path.rsplit_once('/').unwrap_or(("", &entry.path));
            let shown = match input.layout {
                Layout::List => entry.path.as_str(),
                _ => file,
            };
            let label = match &entry.orig_path {
                Some(from) if input.layout == Layout::List => {
                    format!("{}  {from} → {}", mark(entry), entry.path)
                }
                _ => format!("{}  {shown}", mark(entry)),
            };
            // 行数（+ 追加 − 削除。0 の側は出さない。行数のないバイナリは薄く「bin」。GTK4 版の `fill_counts`）。
            let counts =
                input
                    .stats
                    .get(&entry.path)
                    .map_or_else(View::empty, |&(added, removed)| {
                        let count =
                            |n: Option<u64>, mark: &str, color: Color, column: i32| -> View {
                                match n.filter(|&n| n > 0) {
                                    Some(n) => TextBlock::new()
                                        .text(format!("{mark}{n}"))
                                        .font_size(11.0)
                                        .foreground(Brush::Solid(color))
                                        .vertical_alignment(VerticalAlignment::Center)
                                        .margin(Thickness::new(6.0, 0.0, 0.0, 0.0))
                                        .grid_column(column)
                                        .into(),
                                    None => View::empty(),
                                }
                            };
                        if added.is_none() && removed.is_none() {
                            return TextBlock::new()
                                .text("bin")
                                .font_size(11.0)
                                .opacity(0.5)
                                .vertical_alignment(VerticalAlignment::Center)
                                .margin(Thickness::new(6.0, 0.0, 0.0, 0.0))
                                .grid_column(2)
                                .into();
                        }
                        View::fragment((
                            count(added, "+", Color::rgb(0x1a, 0x7f, 0x37), 2),
                            count(removed, "−", Color::rgb(0xcf, 0x22, 0x2e), 3),
                        ))
                    });
            // File Name: 親のディレクトリを、名前の後ろに薄く。
            let name: View = if input.layout == Layout::Flat && !parent.is_empty() {
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(8.0)
                    .vertical_alignment(VerticalAlignment::Center)
                    .grid_column(1)
                    .children((
                        TextBlock::new().text(label),
                        TextBlock::new()
                            .text(parent.to_owned())
                            .opacity(0.5)
                            .text_trimming(TextTrimming::CharacterEllipsis),
                    ))
            } else {
                TextBlock::new()
                    .text(label)
                    .vertical_alignment(VerticalAlignment::Center)
                    .text_trimming(TextTrimming::CharacterEllipsis)
                    .grid_column(1)
                    .into()
            };
            let row = Grid::new()
                .columns([
                    GridLength::Auto,
                    GridLength::Star(1.0),
                    GridLength::Auto,
                    GridLength::Auto,
                ])
                .margin(indent(depth))
                .children((
                    CheckBox::new()
                        .is_checked(included(entry))
                        .on_is_checked_changed((callbacks.stage)(entry.path.clone()))
                        .grid_column(0),
                    name,
                    counts,
                ));
            // 右クリック: Blame・File History（git がまだ知らない untracked のファイルにはない）と、変更を捨てる（untracked
            // なら消す。確かめてから。コンフリクトのファイルには出さない）。GTK4 版の `connect_file_menu` と同じ。
            let mut items = Vec::new();
            // コンフリクトのファイル: 片側のファイルを取る・解決済みにする（GTK4 版の差分の上のボタンと同じ）。
            if entry.kind == StatusKind::Unmerged {
                items.push(MenuItem::item("merge-editor", tr!("Merge Editor")));
                items.push(MenuItem::item("take-ours", tr!("Take Current File")));
                items.push(MenuItem::item("take-theirs", tr!("Take Incoming File")));
                items.push(MenuItem::item("resolved", tr!("Mark Resolved")));
                items.push(MenuItem::separator("-conflict"));
            }
            // untracked のファイルにはまだ履歴がない。代わりに .gitignore に足せる（GTK4 版と同じ）。
            if entry.kind == StatusKind::Untracked {
                for pattern in ignore_patterns(&entry.path) {
                    items.push(MenuItem::item(
                        format!("ignore:{pattern}"),
                        tr!("Add “{pattern}” to .gitignore", pattern = &pattern),
                    ));
                }
                items.push(MenuItem::separator("-ignore"));
            }
            if entry.kind != StatusKind::Untracked {
                // 作業ツリーで消したファイルには Blame はない。
                if entry.worktree != b'D' {
                    items.push(MenuItem::item("blame", "Blame"));
                }
                items.push(MenuItem::item("history", "File History"));
            }
            if entry.kind != StatusKind::Unmerged && entry.is_unstaged() {
                if !items.is_empty() {
                    items.push(MenuItem::separator("-discard"));
                }

                items.push(MenuItem::item("discard", "Discard File"));
            }
            // このファイルの変更だけを stash する（GTK4 版の Working Copy の右クリックと同じ）。
            if entry.kind != StatusKind::Unmerged {
                items.push(MenuItem::item("stash", "Stash…"));
            }
            let row: View = if items.is_empty() {
                row
            } else {
                row.menu(Menu::new(items, (callbacks.file_menu)(entry.path.clone())))
            };
            (
                entry.path.clone(),
                ListViewItem::new().tag(entry.path.clone()).content(row),
            )
        });
    let list = ListView::new()
        .on_selection_changed(callbacks.selected)
        .collection_slot(ListViewSlot::Items, rows);
    // reactor の ListView は、キーつきの項目を消すと、その場所が空いたまま残る。status が替わるたびに一覧ごと
    // 作り直して避ける（替わるのは stage・コミット・外での変更のとき）。
    // 絞り込みを変えたときも作り直す。
    let generation = input.status.map_or(0, |s| s.generation);
    let list = Grid::new()
        .element_ref(input.list_host)
        .grid_row(4)
        .children((View::keyed_fragment([(
            format!(
                "{generation}:{}:{}:{}",
                input.filter,
                input.layout.name(),
                input.collapsed.len()
            ),
            list,
        )]),));
    // 途中の操作の帯: 何の途中かと、Continue・Skip（1 つずつ当てていく操作だけ）・Abort（GTK4 版のメニューの
    // 使えるかと同じ）。
    let band: View = match input.sequence {
        Some((title, detail, skip)) => {
            let button = |key: &'static str, title: &str, enabled: bool| {
                Button::new()
                    .is_enabled(enabled)
                    .on_click((callbacks.sequence)(key))
                    .content(TextBlock::new().text(title))
            };
            let text = StackPanel::new()
                .spacing(2.0)
                .vertical_alignment(VerticalAlignment::Center)
                .grid_column(0)
                .children((
                    TextBlock::new().text(title),
                    TextBlock::new()
                        .text(detail)
                        .font_size(12.0)
                        .opacity(0.75)
                        .text_wrapping(TextWrapping::Wrap),
                ));
            let buttons = |column: i32| {
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(6.0)
                    .horizontal_alignment(HorizontalAlignment::Right)
                    .grid_column(column)
                    .children((
                        if input.retry_three_way {
                            button("am-three-way", &tr!("Retry with 3-Way Merge"), true)
                        } else {
                            View::empty()
                        },
                        // やり直せるうちは Continue を押せない（解くものが残っている。GTK4 版と同じ）。
                        button("continue", "Continue", !input.retry_three_way),
                        button("skip", "Skip", skip),
                        button("abort", "Abort", true),
                    ))
            };
            // ボタンが 4 つになると見出しが隠れるので、そのときはボタンを下の段に置く。
            let content: View = if input.retry_three_way {
                StackPanel::new().spacing(6.0).children((text, buttons(0)))
            } else {
                Grid::new()
                    .columns([GridLength::Star(1.0), GridLength::Auto])
                    .children((text, buttons(1)))
            };
            Border::new()
                .background(Brush::Solid(Color::rgb(0xff, 0xf4, 0xce)))
                .padding(Thickness::new(10.0, 6.0, 10.0, 6.0))
                .margin(Thickness::new(0.0, 0.0, 0.0, 8.0))
                .grid_row(0)
                .content(content)
        }
        None => View::empty(),
    };
    Grid::new()
        .rows([
            GridLength::Auto,
            GridLength::Auto,
            GridLength::Auto,
            GridLength::Auto,
            GridLength::Star(1.0),
        ])
        .margin(Thickness::new(14.0, 10.0, 14.0, 0.0))
        .children((band, message, commit, filter, list))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignore_pattern_keeps_line_endings() {
        assert_eq!(
            with_ignore_pattern("a\r\nb", "*.log").as_deref(),
            Some("a\r\nb\r\n*.log\r\n")
        );
        assert_eq!(
            with_ignore_pattern("a\n", "*.log").as_deref(),
            Some("a\n*.log\n")
        );
        assert_eq!(with_ignore_pattern("", "x").as_deref(), Some("x\n"));
        assert_eq!(with_ignore_pattern("*.log\r\n", "*.log"), None);
    }

    #[test]
    fn commit_types_and_trailers() {
        assert_eq!(without_commit_type("feat(ui)!: add"), "add");
        assert_eq!(without_commit_type("Add: x"), "Add: x");
        assert_eq!(with_trailer("", "A: b").as_deref(), Some("A: b"));
        assert_eq!(
            with_trailer("Body", "A: b").as_deref(),
            Some("Body\n\nA: b")
        );
        assert_eq!(
            with_trailer("x\n\nA: b", "C: d").as_deref(),
            Some("x\n\nA: b\nC: d")
        );
        assert_eq!(with_trailer("A: b", "A: b"), None);
    }

    #[test]
    fn ignore_patterns_add_the_extension() {
        assert_eq!(ignore_patterns("src/a.log"), ["src/a.log", "*.log"]);
        assert_eq!(ignore_patterns(".env"), [".env"]);
    }
}
