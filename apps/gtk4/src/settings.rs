//! 利用者の設定（macOS 版の `Settings.swift` のうち、GTK4 版に機能のあるもの）。`~/.config/amonhen/gtk.ini`
//! （GLib の KeyFile）に置く。GSettings はスキーマを入れる必要があり、展開しただけの実行ファイルでは使えないため。
//! 設定の窓は macOS 版と同じタブのうち、中身のあるもの（General・Appearance・Diff & Merge・Performance）。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gio, glib};

const GROUP: &str = "settings";

/// 設定が変わったときに呼ぶ（変わった鍵）。
type Watcher = Rc<dyn Fn(&str)>;

thread_local! {
    static FILE: glib::KeyFile = load();
    static WATCHERS: RefCell<Vec<Watcher>> = RefCell::default();
    static WINDOW: RefCell<glib::WeakRef<gtk::Window>> = RefCell::default();
}

/// 鍵の名前（macOS 版の UserDefaults と同じ）。
pub const MONOSPACED_COMMIT_MESSAGE: &str = "monospacedCommitMessage";
pub const WRITE_COMMIT_GRAPH: &str = "writeCommitGraph";
pub const OPEN_THROUGH_DAEMON: &str = "openThroughDaemon";
pub const WORKING_COPY_LAYOUT: &str = "workingCopyLayout";
pub const AUTO_MAGIC_MERGE: &str = "autoMagicMerge";
/// File History に merge のコミットも出すか。
pub const FILE_HISTORY_MERGES: &str = "fileHistoryMerges";
/// 開いたリポジトリをブックマークに入れる（Repositories の一覧）。
pub const AUTO_BOOKMARK: &str = "autoBookmark";
/// 開いている間、何分ごとに fetch するか（0 はしない）。
pub const AUTO_FETCH_MINUTES: &str = "autoFetchMinutes";
/// History の並び順（`date`・`topo`。既定は日付順）。
pub const HISTORY_ORDER: &str = "historyOrder";
/// History の行に出す項目（[`items`] のビット。既定はすべて）。
pub const HISTORY_ITEMS: &str = "historyItems";
/// History の行を 1 行にするか（既定は件名と作者・日時の 2 行）。
pub const COMPACT_HISTORY_ROWS: &str = "compactHistoryRows";
/// History でマージ済みブランチを折りたたむか（既定は折りたたまない）。
pub const FOLD_MERGED_BRANCHES: &str = "foldMergedBranches";
/// 選んだコミットの祖先を強調するか（既定はする）。
pub const HIGHLIGHT_ANCESTORS: &str = "highlightAncestors";

/// History の行に出す項目のビット（macOS 版の `HistoryItems` と同じ値）。件名はいつも出す。
pub mod items {
    /// ブランチ・タグのラベル。
    pub const REFS: u32 = 1;
    pub const AUTHOR: u32 = 2;
    pub const DATE: u32 = 4;
    /// 短いコミット ID。
    pub const ID: u32 = 8;
    /// 署名の検証結果の印。
    pub const SIGNATURE: u32 = 16;
    /// GitHub の CI の状態（GTK4 版にはまだない。macOS 版と同じ値を残すため）。
    pub const CI: u32 = 32;
    pub const ALL: u32 = REFS | AUTHOR | DATE | ID | SIGNATURE | CI;
    /// 作者・日時の行に出すもの。
    pub const META: u32 = AUTHOR | DATE | ID;
}

/// History の行に出す項目（[`items`] のビット）。
pub fn history_items() -> u32 {
    string(HISTORY_ITEMS)
        .and_then(|s| s.parse().ok())
        .unwrap_or(items::ALL)
}

pub fn set_history_items(value: u32) {
    set_string(HISTORY_ITEMS, &value.to_string());
}

/// History の並び順。
pub fn history_order() -> amonhen_model::HistoryOrder {
    string(HISTORY_ORDER)
        .and_then(|s| amonhen_model::HistoryOrder::from_name(&s))
        .unwrap_or_default()
}

/// 自動の fetch の間隔（分。0 はしない）。
pub fn auto_fetch_minutes() -> u32 {
    string(AUTO_FETCH_MINUTES)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

fn path() -> PathBuf {
    glib::user_config_dir().join("amonhen").join("gtk.ini")
}

fn load() -> glib::KeyFile {
    let file = glib::KeyFile::new();
    let _ = file.load_from_file(path(), glib::KeyFileFlags::NONE);
    file
}

fn save(file: &glib::KeyFile) {
    let path = path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = file.save_to_file(&path) {
        eprintln!("設定を書けない: {e}");
    }
}

fn changed(key: &str) {
    let watchers: Vec<_> = WATCHERS.with(|w| w.borrow().clone());
    for f in watchers {
        f(key);
    }
}

pub fn bool(key: &str) -> bool {
    FILE.with(|f| f.boolean(GROUP, key)).unwrap_or(matches!(
        key,
        MONOSPACED_COMMIT_MESSAGE | AUTO_MAGIC_MERGE | HIGHLIGHT_ANCESTORS | AUTO_BOOKMARK
    ))
}

pub fn set_bool(key: &str, value: bool) {
    FILE.with(|f| {
        f.set_boolean(GROUP, key, value);
        save(f);
    });
    changed(key);
}

pub fn string(key: &str) -> Option<String> {
    FILE.with(|f| f.string(GROUP, key)).ok().map(Into::into)
}

pub fn set_string(key: &str, value: &str) {
    FILE.with(|f| {
        f.set_string(GROUP, key, value);
        save(f);
    });
    changed(key);
}

/// 設定が変わったら `f(鍵)` を呼ぶ。
pub fn connect_changed(f: impl Fn(&str) + 'static) {
    WATCHERS.with(|w| w.borrow_mut().push(Rc::new(f)));
}

/// 項目の下に添える説明。
fn note(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    l.set_wrap(true);
    l.set_max_width_chars(56);
    l.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    l.set_natural_wrap_mode(gtk::NaturalWrapMode::Word);
    l.add_css_class("dim-label");
    l.add_css_class("caption");
    l.set_margin_start(28);
    l
}

fn check(key: &'static str, title: &str) -> gtk::CheckButton {
    let c = gtk::CheckButton::with_label(title);
    c.set_active(bool(key));
    c.connect_toggled(move |c| {
        if c.is_active() != bool(key) {
            set_bool(key, c.is_active());
        }
    });
    // History の表示のメニューなど、ほかの所で変えたら合わせる。
    let weak = c.downgrade();
    connect_changed(move |changed| {
        if changed == key
            && let Some(c) = weak.upgrade()
        {
            c.set_active(bool(key));
        }
    });
    c
}

/// History の行に出す項目の 1 つ（`bit`）を出すか。
fn item_check(bit: u32, title: &str) -> gtk::CheckButton {
    let c = gtk::CheckButton::with_label(title);
    c.set_active(history_items() & bit != 0);
    c.connect_toggled(move |c| {
        let items = history_items();
        let next = if c.is_active() {
            items | bit
        } else {
            items & !bit
        };
        if next != items {
            set_history_items(next);
        }
    });
    let weak = c.downgrade();
    connect_changed(move |changed| {
        if changed == HISTORY_ITEMS
            && let Some(c) = weak.upgrade()
        {
            c.set_active(history_items() & bit != 0);
        }
    });
    c
}

/// History の並び順を選ぶ行と、その説明（Settings > Appearance）。
fn order_row() -> gtk::Box {
    use amonhen_model::HistoryOrder;
    const ORDERS: [HistoryOrder; 2] = [HistoryOrder::Date, HistoryOrder::Topo];
    let titles = [tr!("Date Order"), tr!("Topological Order")];
    let refs: Vec<&str> = titles.iter().map(String::as_str).collect();
    let choice = gtk::DropDown::from_strings(&refs);
    let index = |o: HistoryOrder| ORDERS.iter().position(|&x| x == o).unwrap_or(0) as u32;
    choice.set_selected(index(history_order()));
    choice.connect_selected_notify(move |d| {
        let order = ORDERS[d.selected() as usize];
        if order != history_order() {
            set_string(HISTORY_ORDER, order.name());
        }
    });
    let weak = choice.downgrade();
    connect_changed(move |changed| {
        if changed == HISTORY_ORDER
            && let Some(d) = weak.upgrade()
        {
            d.set_selected(index(history_order()));
        }
    });
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let l = gtk::Label::new(Some(&tr!("Order")));
    l.set_hexpand(true);
    l.set_xalign(0.0);
    row.append(&l);
    row.append(&choice);
    // 説明は見出しの下にそろえる（ほかの説明はチェックボックスの文字にそろえる）。
    let n = note(&tr!(
        "Topological order keeps a branch’s commits together under the merge that brought them in."
    ));
    n.set_margin_start(0);
    let b = gtk::Box::new(gtk::Orientation::Vertical, 4);
    b.append(&row);
    b.append(&n);
    b
}

fn section(title: Option<&str>, items: &[gtk::Widget]) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 6);
    if let Some(title) = title {
        let l = gtk::Label::new(Some(title));
        l.set_xalign(0.0);
        l.add_css_class("heading");
        b.append(&l);
    }
    for w in items {
        b.append(w);
    }
    b
}

fn page(sections: &[gtk::Box]) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 18);
    b.set_margin_top(18);
    b.set_margin_bottom(18);
    b.set_margin_start(24);
    b.set_margin_end(24);
    for s in sections {
        b.append(s);
    }
    b
}

/// 設定の窓を開く（開いていれば前に出す）。
pub fn show(parent: &gtk::ApplicationWindow) {
    if let Some(w) = WINDOW.with(|w| w.borrow().upgrade()) {
        w.present();
        return;
    }
    let stack = gtk::Stack::new();

    // General
    let choices: [u32; 5] = [0, 5, 15, 30, 60];
    let titles: Vec<String> = choices
        .iter()
        .map(|&m| {
            if m == 0 {
                tr!("Never")
            } else {
                tr!("Every {count} minutes", count = m)
            }
        })
        .collect();
    let refs: Vec<&str> = titles.iter().map(String::as_str).collect();
    let fetch = gtk::DropDown::from_strings(&refs);
    fetch.set_selected(
        choices
            .iter()
            .position(|&m| m == auto_fetch_minutes())
            .unwrap_or(0) as u32,
    );
    fetch.connect_selected_notify(move |d| {
        set_string(
            AUTO_FETCH_MINUTES,
            &choices[d.selected() as usize].to_string(),
        )
    });
    let fetch_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let l = gtk::Label::new(Some(&tr!("Fetch automatically")));
    l.set_hexpand(true);
    l.set_xalign(0.0);
    fetch_row.append(&l);
    fetch_row.append(&fetch);
    stack.add_titled(
        &page(&[section(
            None,
            &[
                check(AUTO_BOOKMARK, &tr!("Bookmark repositories you open")).upcast(),
                note(&tr!(
                    "Repositories already found in folders to scan aren’t added."
                ))
                .upcast(),
                fetch_row.upcast(),
                note(&tr!(
                    "Fetches all remotes of each open repository without asking."
                ))
                .upcast(),
            ],
        )]),
        Some("general"),
        &tr!("General"),
    );

    // Git: すべてのリポジトリの段（`--global`）の、Repository Settings と同じページ。git の設定は手元の空の
    // リポジトリ（settings-repo）から読み書きする（macOS 版の `MachineSettings` と同じ）。
    let git = gtk::Box::new(gtk::Orientation::Vertical, 0);
    git.set_size_request(860, 560);
    stack.add_titled(&git, Some("git"), "Git");
    let g = git.downgrade();
    glib::spawn_future_local(async move {
        let result =
            gio::spawn_blocking(|| -> Result<(amonhen_core::Repository, String), String> {
                let dir = glib::user_data_dir().join("amonhen").join("settings-repo");
                if !dir.join(".git").exists() {
                    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                    let ok = amonhen_model::process::command("git")
                        .args(["init", "-q"])
                        .current_dir(&dir)
                        .status()
                        .is_ok_and(|s| s.success());
                    if !ok {
                        return Err(tr!("Couldn’t prepare the settings repository"));
                    }
                }
                let repo = crate::open(&dir)?;
                Ok((repo, dir.to_string_lossy().into_owned()))
            })
            .await
            .unwrap_or_else(|_| Err(String::new()));
        let Some(git) = g.upgrade() else { return };
        match result {
            Ok((repo, location)) => {
                let settings = crate::reposettings::RepoSettings::new(
                    std::sync::Arc::new(repo),
                    &location,
                    crate::reposettings::Level::AllRepositories,
                );
                settings.widget.set_vexpand(true);
                git.append(&settings.widget);
                // 窓を閉じるまで持っておく。
                unsafe { git.set_data("settings", settings) };
            }
            Err(e) => git.append(&note(&e)),
        }
    });

    // Appearance
    stack.add_titled(
        &page(&[
            section(
                Some("History"),
                &[
                    order_row().upcast(),
                    check(COMPACT_HISTORY_ROWS, &tr!("Show each commit on one line")).upcast(),
                    check(
                        HIGHLIGHT_ANCESTORS,
                        &tr!("Highlight the selected commit’s ancestors"),
                    )
                    .upcast(),
                ],
            ),
            section(
                Some(&tr!("Items")),
                &[
                    item_check(items::REFS, &tr!("Branches and Tags")).upcast(),
                    item_check(items::AUTHOR, &tr!("Author")).upcast(),
                    item_check(items::DATE, &tr!("Date")).upcast(),
                    item_check(items::ID, &tr!("Commit ID")).upcast(),
                    item_check(items::SIGNATURE, &tr!("Signature")).upcast(),
                    note(&tr!("What each commit’s row shows besides the subject.")).upcast(),
                ],
            ),
            section(
                Some(&tr!("Commit Details")),
                &[check(
                    MONOSPACED_COMMIT_MESSAGE,
                    &tr!("Show the message in a monospaced font"),
                )
                .upcast()],
            ),
        ]),
        Some("appearance"),
        &tr!("Appearance"),
    );

    // Diff & Merge
    stack.add_titled(
        &page(&[section(
            Some(&tr!("Merge Editor")),
            &[
                check(AUTO_MAGIC_MERGE, &tr!("Resolve unambiguous conflicts when the merge editor opens")).upcast(),
                note(&tr!("Conflicts where both sides are the same, or only one side changed. Ctrl+Z undoes it. Use ✨ Magic Merge for the rest."))
                    .upcast(),
            ],
        )]),
        Some("diff-merge"),
        &tr!("Diff & Merge"),
    );

    // Performance
    stack.add_titled(
        &page(&[
            section(
                None,
                &[
                    check(WRITE_COMMIT_GRAPH, &tr!("Write commit-graph automatically")).upcast(),
                    note(&tr!("When you open a repository without a commit-graph, runs git commit-graph write --reachable --changed-paths in the background (writes to .git). Loading history and searching with path: get faster."))
                        .upcast(),
                ],
            ),
            section(
                Some(&tr!("Shared daemon")),
                &[
                    check(OPEN_THROUGH_DAEMON, &tr!("Open repositories through the shared daemon (SSH hosts too)")).upcast(),
                    note(&tr!("Shares History, the search index, the operation log and SSH connections among the app, the CLI (amonhen -C daemon:… or -C ssh://…) and the VSCode extension. Applies to repositories opened from now on. See Daemon in Activity for its status."))
                        .upcast(),
                ],
            ),
        ]),
        Some("performance"),
        &tr!("Performance"),
    );

    let switcher = gtk::StackSwitcher::builder().stack(&stack).build();
    let header = gtk::HeaderBar::new();
    header.set_title_widget(Some(&switcher));
    let window = gtk::Window::builder()
        .title(tr!("Settings"))
        .default_width(660)
        .child(&stack)
        .transient_for(parent)
        .destroy_with_parent(true)
        .build();
    window.set_titlebar(Some(&header));
    WINDOW.with(|w| w.replace(window.downgrade()));
    window.present();
}

/// 開いている設定の窓のタブ（`git` など）を選ぶ（確認用）。
pub fn select(tab: &str) {
    if let Some(stack) = WINDOW
        .with(|w| w.borrow().upgrade())
        .and_then(|w| w.child())
        .and_downcast::<gtk::Stack>()
    {
        stack.set_visible_child_name(tab);
    }
}
