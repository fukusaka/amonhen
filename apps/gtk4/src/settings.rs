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
    /// 聞き手と、それを持つもの（なくなったら外す。None はずっと）。
    static WATCHERS: RefCell<Vec<(Option<glib::WeakRef<glib::Object>>, Watcher)>> =
        RefCell::default();
    static WINDOW: RefCell<glib::WeakRef<gtk::Window>> = RefCell::default();
    /// 設定の窓のタブの中身（`select` が切り替える）。
    static STACK: RefCell<glib::WeakRef<gtk::Stack>> = RefCell::default();
    /// Git のタブで、読み終えたら開くページ（`--settings git/programs`）。
    static GIT_PAGE: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Git のタブの中身（読み終えて作ったら。窓が開いている間に `select("git/<page>")` されたらそのページへ）。
    static GIT_SETTINGS: RefCell<std::rc::Weak<crate::reposettings::RepoSettings>> =
        RefCell::default();
}

/// 鍵の名前（macOS 版の UserDefaults と同じ）。
pub const MONOSPACED_COMMIT_MESSAGE: &str = "monospacedCommitMessage";
pub const WRITE_COMMIT_GRAPH: &str = "writeCommitGraph";
pub const OPEN_THROUGH_DAEMON: &str = "openThroughDaemon";
/// 手元のリポジトリの status で git の fsmonitor を使うか（既定は使う。macOS と Windows でだけ効く）。
pub const USE_FSMONITOR: &str = "useFSMonitor";
pub const WORKING_COPY_LAYOUT: &str = "workingCopyLayout";
pub const AUTO_MAGIC_MERGE: &str = "autoMagicMerge";
/// File History に merge のコミットも出すか。
pub const FILE_HISTORY_MERGES: &str = "fileHistoryMerges";
/// 開いたリポジトリをブックマークに入れる（Repositories の一覧）。
pub const AUTO_BOOKMARK: &str = "autoBookmark";
/// SSH 先ごとに決めていないとき（と、SSH でない daemon 越し）に、署名を手元の鍵で検証するか（既定は手元。
/// macOS 版と同じ鍵。画面では変えない）。
pub const CHECK_REMOTE_SIGNATURES_LOCALLY: &str = "checkRemoteSignaturesLocally";
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
/// コミットの詳細に作者・コミッターのアバターを出すか（既定は出す。Gravatar にメールアドレスのハッシュを送る）。
pub const SHOW_AVATARS: &str = "showAvatars";
/// GitHub（gh で読む。Settings > Accounts）: History のコミットごとに CI の状態を出すか（既定は出す）。
pub const GITHUB_CI_STATUS: &str = "gitHubCIStatus";
/// コミットの作者の GitHub のアバターを使うか（既定は使う。使わなければ Gravatar だけ）。
pub const GITHUB_AVATARS: &str = "gitHubAvatars";
/// CI を読むブランチに、既定のブランチも含めるか（既定は含める。外すと今のブランチだけ）。
pub const GITHUB_CI_DEFAULT_BRANCH: &str = "gitHubCIDefaultBranch";
/// ブランチごとに読む直近のコミットの数（30・50・100。既定は 100）。
pub const GITHUB_CI_COMMITS: &str = "gitHubCICommits";
/// アバターの URL は、窓を開いて最初の 1 回だけ読むか（既定はそうする）。
pub const GITHUB_AVATARS_ONCE: &str = "gitHubAvatarsOnce";

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
    /// GitHub の CI の状態。
    pub const CI: u32 = 32;
    pub const ALL: u32 = REFS | AUTHOR | DATE | ID | SIGNATURE | CI;
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

/// GitHub で、ブランチごとに読む直近のコミットの数。
pub fn github_ci_commits() -> u32 {
    string(GITHUB_CI_COMMITS)
        .and_then(|s| s.parse().ok())
        .filter(|n| [30, 50, 100].contains(n))
        .unwrap_or(100)
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
    // 持ち主がなくなった聞き手は外す（設定の窓・Repository Settings を開くたびに足されるので）。
    let watchers: Vec<Watcher> = WATCHERS.with(|w| {
        let mut w = w.borrow_mut();
        w.retain(|(owner, _)| owner.as_ref().is_none_or(|o| o.upgrade().is_some()));
        w.iter().map(|(_, f)| f.clone()).collect()
    });
    for f in watchers {
        f(key);
    }
}

pub fn bool(key: &str) -> bool {
    FILE.with(|f| f.boolean(GROUP, key)).unwrap_or(matches!(
        key,
        MONOSPACED_COMMIT_MESSAGE
            | AUTO_MAGIC_MERGE
            | HIGHLIGHT_ANCESTORS
            | AUTO_BOOKMARK
            | SHOW_AVATARS
            | GITHUB_CI_STATUS
            | GITHUB_AVATARS
            | GITHUB_CI_DEFAULT_BRANCH
            | GITHUB_AVATARS_ONCE
            | USE_FSMONITOR
            | CHECK_REMOTE_SIGNATURES_LOCALLY
            | crate::commitfiles::STACKED
    ))
}

/// status で git の fsmonitor を使うかを設定から決め、設定を変えたら合わせる（起動したときに一度呼ぶ）。
pub fn apply_fsmonitor() {
    amonhen_backend_cli::set_fsmonitor(bool(USE_FSMONITOR));
    connect_changed(|key| {
        if key == USE_FSMONITOR {
            amonhen_backend_cli::set_fsmonitor(bool(USE_FSMONITOR));
        }
    });
}

/// 設定の窓の大きさの覚え場所。
const WINDOW_SIZE: &str = "settingsWindowSize";

/// git の `fsmonitor--daemon` を使える OS か（macOS と Windows だけ）。
const FSMONITOR_AVAILABLE: bool = cfg!(any(target_os = "macos", target_os = "windows"));

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

/// 鍵を消す（なければ何もしない）。
pub fn remove(key: &str) {
    let removed = FILE.with(|f| {
        let removed = f.remove_key(GROUP, key).is_ok();
        if removed {
            save(f);
        }
        removed
    });
    if removed {
        changed(key);
    }
}

/// 設定が変わったら `f(鍵)` を呼ぶ。
pub fn connect_changed(f: impl Fn(&str) + 'static) {
    WATCHERS.with(|w| w.borrow_mut().push((None, Rc::new(f))));
}

/// `connect_changed` の、`owner` がある間だけ呼ぶ形（なくなったら外す）。
pub fn connect_changed_while(owner: &impl IsA<glib::Object>, f: impl Fn(&str) + 'static) {
    let owner = owner.upcast_ref::<glib::Object>().downgrade();
    WATCHERS.with(|w| w.borrow_mut().push((Some(owner), Rc::new(f))));
}

/// 項目の下に添える説明（macOS 版の `Note`。項目の名前の左端にそろえる）。
fn note(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    l.set_wrap(true);
    l.set_max_width_chars(56);
    l.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    l.set_natural_wrap_mode(gtk::NaturalWrapMode::Word);
    l.add_css_class("dim-label");
    l.add_css_class("caption");
    l
}

/// 入り切りの行: 名前を左に、スイッチを右に（macOS 版の `.grouped` の Form の Toggle）。`get` で今の値を
/// 読み、`set` で書く。`key` の設定がほかの所（History の表示のメニューなど）で変わったら合わせる。
fn switch_row(
    title: &str,
    key: &'static str,
    get: impl Fn() -> bool + 'static,
    set: impl Fn(bool) + 'static,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let l = gtk::Label::new(Some(title));
    l.set_hexpand(true);
    l.set_xalign(0.0);
    l.set_wrap(true);
    l.set_natural_wrap_mode(gtk::NaturalWrapMode::Word);
    row.append(&l);
    let switch = gtk::Switch::new();
    switch.set_valign(gtk::Align::Center);
    switch.set_active(get());
    let get = Rc::new(get);
    let g = get.clone();
    switch.connect_active_notify(move |s| {
        if s.is_active() != g() {
            set(s.is_active());
        }
    });
    row.append(&switch);
    let weak = switch.downgrade();
    connect_changed_while(&switch, move |changed| {
        if changed == key
            && let Some(s) = weak.upgrade()
        {
            s.set_active(get());
        }
    });
    row
}

fn check(key: &'static str, title: &str) -> gtk::Box {
    switch_row(title, key, move || bool(key), move |on| set_bool(key, on))
}

/// History の行に出す項目の 1 つ（`bit`）を出すか。
fn item_check(bit: u32, title: &str) -> gtk::Box {
    switch_row(
        title,
        HISTORY_ITEMS,
        move || history_items() & bit != 0,
        move |on| {
            let items = history_items();
            let next = if on { items | bit } else { items & !bit };
            if next != items {
                set_history_items(next);
            }
        },
    )
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
    connect_changed_while(&choice, move |changed| {
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
    let n = note(&tr!(
        "Topological order keeps a branch’s commits together under the merge that brought them in."
    ));
    let b = gtk::Box::new(gtk::Orientation::Vertical, 4);
    b.append(&row);
    b.append(&n);
    b
}

/// 選ぶ行（名前と、`titles` から選ぶ欄）。`get`・`set` で `key` の設定を読み書きする。
fn choice_row(
    title: &str,
    titles: &[String],
    get: fn() -> usize,
    set: impl Fn(usize) + 'static,
    key: &'static str,
) -> gtk::Box {
    let refs: Vec<&str> = titles.iter().map(String::as_str).collect();
    let choice = gtk::DropDown::from_strings(&refs);
    choice.set_selected(get() as u32);
    choice.connect_selected_notify(move |d| {
        if d.selected() as usize != get() {
            set(d.selected() as usize);
        }
    });
    let weak = choice.downgrade();
    connect_changed_while(&choice, move |changed| {
        if changed == key
            && let Some(d) = weak.upgrade()
        {
            d.set_selected(get() as u32);
        }
    });
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let l = gtk::Label::new(Some(title));
    l.set_hexpand(true);
    l.set_xalign(0.0);
    row.append(&l);
    row.append(&choice);
    row
}

/// Settings > Accounts（macOS 版の `AccountsSettings`）: GitHub から History に読むもの。
fn accounts() -> gtk::Box {
    const COMMITS: [u32; 3] = [100, 50, 30];
    let ci = check(
        GITHUB_CI_STATUS,
        &tr!("Show each commit’s CI status in History"),
    );
    let avatars = check(GITHUB_AVATARS, &tr!("Use commit authors’ GitHub avatars"));
    let branches = choice_row(
        &tr!("Branches to read CI for"),
        &[
            tr!("Current and default branch"),
            tr!("Current branch only"),
        ],
        || usize::from(!bool(GITHUB_CI_DEFAULT_BRANCH)),
        |i| set_bool(GITHUB_CI_DEFAULT_BRANCH, i == 0),
        GITHUB_CI_DEFAULT_BRANCH,
    );
    let commits = choice_row(
        &tr!("Commits to read per branch"),
        &COMMITS.map(|n| tr!("Last {count}", count = n)),
        || {
            COMMITS
                .iter()
                .position(|&n| n == github_ci_commits())
                .unwrap_or(0)
        },
        |i| set_string(GITHUB_CI_COMMITS, &COMMITS[i].to_string()),
        GITHUB_CI_COMMITS,
    );
    let once = check(
        GITHUB_AVATARS_ONCE,
        &tr!("Read avatar URLs only once per window"),
    );
    // どちらも読まないなら、ブランチと数は選べない。アバターを使わないなら、1 回だけかも選べない。
    let enable = {
        let (b, c, o) = (branches.downgrade(), commits.downgrade(), once.downgrade());
        move || {
            let any = bool(GITHUB_CI_STATUS) || bool(GITHUB_AVATARS);
            if let (Some(b), Some(c), Some(o)) = (b.upgrade(), c.upgrade(), o.upgrade()) {
                b.set_sensitive(any);
                c.set_sensitive(any);
                o.set_sensitive(bool(GITHUB_AVATARS));
            }
        }
    };
    enable();
    connect_changed_while(&branches, move |key| {
        if key == GITHUB_CI_STATUS || key == GITHUB_AVATARS {
            enable();
        }
    });
    page(&[section(
        Some("GitHub"),
        &[
            ci.upcast(),
            avatars.upcast(),
            branches.upcast(),
            commits.upcast(),
            once.upcast(),
        ],
    )])
}

/// 見出しと、枠で囲んだ行の一覧（macOS 版の `.grouped` の Form の Section。説明も 1 行として中に入る）。
fn section(title: Option<&str>, items: &[gtk::Widget]) -> gtk::Box {
    crate::reposettings::section(title, items)
}

fn page(sections: &[gtk::Box]) -> gtk::Box {
    // 余白は Git のタブのページ（`reposettings::page`）と同じ。
    let b = gtk::Box::new(gtk::Orientation::Vertical, 18);
    b.set_margin_top(18);
    b.set_margin_bottom(18);
    b.set_margin_start(20);
    b.set_margin_end(20);
    for s in sections {
        b.append(s);
    }
    b
}

/// 設定の窓を開く（開いていれば前に出す）。窓はアプリに 1 つで、どの窓のものでもない（macOS 版の
/// `SettingsWindowController.shared`）。閉じても隠すだけで、中身（SSH 先のつながりなど）は残す。`near` は
/// 初めて出すときの画面を決めるだけ。
pub fn show(near: &impl IsA<gtk::Widget>) {
    if let Some(w) = WINDOW.with(|w| w.borrow().upgrade()) {
        // 出し直すたびに読み直す（端末やほかの窓で変えた git の設定・新しい SSH 先。macOS 版と同じ）。
        if let Some(settings) = GIT_SETTINGS.with(|s| s.borrow().upgrade()) {
            settings.load();
        }
        crate::sshhosts::reload_shown();
        join_application(&w, near);
        w.present();
        return;
    }
    crate::reposettings::install_css();
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
        &page(&[
            section(
                None,
                &[
                    // 説明は付けない（macOS 版と同じ）。
                    check(AUTO_BOOKMARK, &tr!("Bookmark repositories you open")).upcast(),
                    fetch_row.upcast(),
                ],
            ),
            section(None, &[crate::editor::settings_row().upcast()]),
        ]),
        Some("general"),
        &tr!("General"),
    );

    // Git: すべてのリポジトリの段（`--global`）の、Repository Settings と同じページ。git の設定は手元の空の
    // リポジトリ（settings-repo）から読み書きする（macOS 版の `MachineSettings` と同じ）。
    let git = gtk::Box::new(gtk::Orientation::Vertical, 0);
    git.set_vexpand(true);
    stack.add_titled(&git, Some("git"), "Git");
    // 読むまでは回る印（macOS 版の ProgressView）。
    let loading = gtk::Spinner::builder()
        .spinning(true)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .vexpand(true)
        .build();
    git.append(&loading);
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
        git.remove(&loading);
        match result {
            Ok((repo, location)) => {
                let settings = crate::reposettings::RepoSettings::new(
                    std::sync::Arc::new(repo),
                    &location,
                    crate::reposettings::Level::AllRepositories,
                );
                settings.widget.set_vexpand(true);
                GIT_SETTINGS.with(|s| s.replace(Rc::downgrade(&settings)));
                if let Some(page) = GIT_PAGE.with(|p| p.take()) {
                    settings.select(&page);
                    // 確認用の手順（`--settings git/<page>` と一緒に）。
                    if let Ok(script) = std::env::var("AMONHEN_SETTINGS_SCRIPT") {
                        settings.run_script(&script);
                    }
                }
                git.append(&settings.widget);
                // 窓を閉じるまで持っておく。
                unsafe { git.set_data("settings", settings) };
            }
            // 読めなければ赤で（macOS 版と同じ）。
            Err(e) => {
                let l = gtk::Label::new(Some(&e));
                l.set_wrap(true);
                l.add_css_class("error");
                l.set_margin_top(24);
                l.set_margin_start(24);
                l.set_margin_end(24);
                git.append(&l);
            }
        }
    });

    // SSH Hosts: 開いたときに作る（選んだ SSH 先につなぎに行くので、タブを出すまではつながない）。
    let hosts = gtk::Box::new(gtk::Orientation::Vertical, 0);
    stack.add_titled(&hosts, Some("ssh-hosts"), &tr!("SSH Hosts"));
    stack.connect_visible_child_name_notify(move |stack| {
        // タブを出し直したら読み直す（macOS 版と同じ）。
        match stack.visible_child_name().as_deref() {
            Some("git") => {
                if let Some(settings) = GIT_SETTINGS.with(|s| s.borrow().upgrade()) {
                    settings.load();
                }
            }
            Some("ssh-hosts") if hosts.first_child().is_some() => crate::sshhosts::reload_shown(),
            _ => {}
        }
        if stack.visible_child_name().as_deref() == Some("ssh-hosts")
            && hosts.first_child().is_none()
        {
            let tab = crate::sshhosts::SshHosts::new();
            tab.widget.set_vexpand(true);
            hosts.append(&tab.widget);
            // 窓を閉じるまで持っておく。
            unsafe { hosts.set_data("tab", tab) };
        }
    });

    // Accounts: 外部サービス。今は GitHub の表示だけ（macOS 版と同じ）。
    stack.add_titled(&accounts(), Some("accounts"), &tr!("Accounts"));

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
                    item_check(items::CI, &tr!("CI Status")).upcast(),
                    note(&tr!("What each commit’s row shows besides the subject.")).upcast(),
                ],
            ),
            section(
                Some(&tr!("Commit Details")),
                &[
                    check(
                        MONOSPACED_COMMIT_MESSAGE,
                        &tr!("Show the message in a monospaced font"),
                    )
                    .upcast(),
                    check(SHOW_AVATARS, &tr!("Show avatars")).upcast(),
                    note(&tr!(
                        "Fetched from Gravatar (sends a hash of the email address to gravatar.com)."
                    ))
                    .upcast(),
                    check(
                        crate::commitfiles::STACKED,
                        &tr!("Stack the diffs of all files"),
                    )
                    .upcast(),
                    check(
                        crate::commitfiles::FILE_TREE,
                        &tr!("Show changed files as a tree left of the diff"),
                    )
                    .upcast(),
                ],
            ),
            section(
                Some("File History"),
                &[check(FILE_HISTORY_MERGES, &tr!("Show merge commits")).upcast()],
            ),
        ]),
        Some("appearance"),
        &tr!("Appearance"),
    );

    // Diff & Merge
    stack.add_titled(
        &page(&[section(
            Some(&tr!("Merge Editor")),
            &[check(
                AUTO_MAGIC_MERGE,
                &tr!("Resolve unambiguous conflicts when the merge editor opens"),
            )
            .upcast()],
        )]),
        Some("diff-merge"),
        &tr!("Diff & Merge"),
    );

    // AI（`aisettings.rs`）。
    stack.add_titled(&crate::aisettings::page(), Some("ai"), "AI");

    // Performance
    // fsmonitor: git の fsmonitor--daemon のない OS（Linux など）では押せなくし、理由を出す。
    let fsmonitor = check(USE_FSMONITOR, &tr!("Speed up status with fsmonitor"));
    if !FSMONITOR_AVAILABLE {
        fsmonitor.set_sensitive(false);
        fsmonitor.set_tooltip_text(Some(&tr!(
            "git’s fsmonitor--daemon runs only on macOS and Windows."
        )));
    }
    stack.add_titled(
        &page(&[
            section(
                None,
                &[
                    check(WRITE_COMMIT_GRAPH, &tr!("Write commit-graph automatically")).upcast(),
                    note(&tr!(
                        "Writes to .git, except in repositories git maintenance looks after."
                    ))
                    .upcast(),
                    fsmonitor.upcast(),
                    note(&tr!("A git fsmonitor--daemon runs for each repository.")).upcast(),
                ],
            ),
            section(
                Some(&tr!("Shared daemon")),
                &[check(
                    OPEN_THROUGH_DAEMON,
                    &tr!("Open repositories through the shared daemon (SSH hosts too)"),
                )
                .upcast()],
            ),
        ]),
        Some("performance"),
        &tr!("Performance"),
    );

    // 上にアイコン付きのタブ（macOS 版のツールバーのタブ）。窓の大きさはどのタブでも同じで、中身は窓いっぱいに
    // 広がり、長ければ中でスクロールする（macOS 版と同じく、最小 960×400、初めは 960×720）。
    // 高さはページごと（短いページはスクロールしない）。
    stack.set_vhomogeneous(false);
    let scrolled = gtk::ScrolledWindow::builder()
        .child(&stack)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&tab_bar(&stack));
    content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    content.append(&scrolled);
    let header = gtk::HeaderBar::new();
    let window = gtk::Window::builder()
        .title(tr!("Settings"))
        .default_width(960)
        .default_height(720)
        .child(&content)
        .hide_on_close(true)
        .build();
    window.set_size_request(960, 400);
    window.set_titlebar(Some(&header));
    // 窓のタイトルは選んでいるタブの名前（macOS 標準の設定画面と同じ）。
    let retitle = {
        let w = window.downgrade();
        move |stack: &gtk::Stack| {
            let title = stack
                .visible_child()
                .map(|c| stack.page(&c))
                .and_then(|p| p.title());
            if let (Some(w), Some(title)) = (w.upgrade(), title) {
                w.set_title(Some(&title));
            }
        }
    };
    retitle(&stack);
    stack.connect_visible_child_notify(retitle);
    // Ctrl+W で閉じる（アプリの `win.close` の近道を、この窓では閉じることに。macOS 版の ⌘W と同じ）。
    let actions = gio::SimpleActionGroup::new();
    let close = gio::SimpleAction::new("close", None);
    let w = window.downgrade();
    close.connect_activate(move |_, _| {
        if let Some(w) = w.upgrade() {
            w.close();
        }
    });
    actions.add_action(&close);
    window.insert_action_group("win", Some(&actions));
    window.set_display(&WidgetExt::display(near));
    // 大きさは覚えておく（macOS 版は位置も。GTK では窓の位置はウィンドウマネージャーが決める）。
    if let Some((w, h)) = string(WINDOW_SIZE).as_deref().and_then(parse_size) {
        window.set_default_size(w, h);
    }
    window.connect_close_request(|window| {
        let (w, h) = window.default_size();
        if w > 0 && h > 0 {
            set_string(WINDOW_SIZE, &format!("{w}x{h}"));
        }
        // 隠すだけなので、アプリの窓から外す（ほかに窓がなければ、Activity と同じくアプリを終える）。
        window.set_application(None::<&gtk::Application>);
        glib::Propagation::Proceed
    });
    WINDOW.with(|w| w.replace(window.downgrade()));
    STACK.with(|s| s.replace(stack.downgrade()));
    join_application(&window, near);
    window.present();
}

/// 設定の窓の上のタブ（アイコンと名前。macOS 版の `tabStyle = .toolbar`）。押すとそのページを出す。
fn tab_bar(stack: &gtk::Stack) -> gtk::Box {
    use crate::icon::{Icon, Kind};
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    bar.set_halign(gtk::Align::Center);
    bar.set_margin_top(4);
    bar.set_margin_bottom(4);
    let icons = [
        ("general", Kind::Gear),
        ("git", Kind::Branch),
        ("ssh-hosts", Kind::Remote),
        ("accounts", Kind::Person),
        ("appearance", Kind::Brush),
        ("diff-merge", Kind::Merge),
        ("ai", Kind::Sparkles),
        ("performance", Kind::Speedometer),
    ];
    let mut buttons: Vec<(String, gtk::ToggleButton)> = Vec::new();
    let mut child = stack.first_child();
    while let Some(c) = child {
        child = c.next_sibling();
        let page = stack.page(&c);
        let Some(name) = page.name() else { continue };
        let kind = icons
            .iter()
            .find(|(n, _)| *n == name.as_str())
            .map_or(Kind::Gear, |(_, k)| *k);
        let icon = Icon::default();
        icon.set(kind, false);
        icon.set_pixel_size(24);
        let label = gtk::Label::new(page.title().as_deref());
        label.add_css_class("caption");
        let inner = gtk::Box::new(gtk::Orientation::Vertical, 2);
        inner.append(&icon);
        inner.append(&label);
        let button = gtk::ToggleButton::new();
        button.set_child(Some(&inner));
        button.add_css_class("flat");
        button.set_tooltip_text(page.title().as_deref());
        if let Some((_, first)) = buttons.first() {
            button.set_group(Some(first));
        }
        let (s, n) = (stack.downgrade(), name.to_string());
        button.connect_toggled(move |b| {
            if b.is_active()
                && let Some(s) = s.upgrade()
            {
                s.set_visible_child_name(&n);
            }
        });
        bar.append(&button);
        buttons.push((name.to_string(), button));
    }
    let sync = move |stack: &gtk::Stack| {
        let shown = stack.visible_child_name();
        for (name, button) in &buttons {
            if shown.as_deref() == Some(name.as_str()) && !button.is_active() {
                button.set_active(true);
            }
        }
    };
    sync(stack);
    stack.connect_visible_child_notify(sync);
    bar
}

/// 出している間はアプリの窓にする: 開いたリポジトリの窓を閉じても、Settings とアプリが続く（macOS 版と同じ）。
fn join_application(window: &gtk::Window, near: &impl IsA<gtk::Widget>) {
    let app = near
        .root()
        .and_downcast::<gtk::Window>()
        .and_then(|w| w.application())
        .or_else(|| gio::Application::default().and_downcast::<gtk::Application>());
    if let Some(app) = app
        && window.application().as_ref() != Some(&app)
    {
        window.set_application(Some(&app));
    }
}

/// 出ている設定の窓（撮るときに使う）。
pub fn window() -> Option<gtk::Window> {
    WINDOW
        .with(|w| w.borrow().upgrade())
        .filter(|w| w.is_visible())
}

/// 覚えておいた窓の大きさ（`幅x高さ`）。小さすぎるものは使わない。
fn parse_size(s: &str) -> Option<(i32, i32)> {
    let (w, h) = s.split_once('x')?;
    let (w, h) = (w.parse::<i32>().ok()?, h.parse::<i32>().ok()?);
    (w > 200 && h > 150).then_some((w, h))
}

/// 開いている設定の窓のタブ（`git` など）を選ぶ（確認用）。`git/programs` なら Git のタブのそのページ、
/// `ssh-hosts/<SSH 先>/<ページ>` なら SSH Hosts のタブでその SSH 先（なければ加える）のページ。
pub fn select(tab: &str) {
    let (tab, page) = tab.split_once('/').unwrap_or((tab, ""));
    if tab == "git" && !page.is_empty() {
        // もう作ってあればそのページへ。まだなら、作ったときに開く。
        match GIT_SETTINGS.with(|s| s.borrow().upgrade()) {
            Some(settings) => settings.select(page),
            None => {
                GIT_PAGE.with(|p| p.replace(Some(page.to_owned())));
            }
        }
    }
    if tab == "ssh-hosts" && !page.is_empty() {
        let (host, page) = page.split_once('/').unwrap_or((page, ""));
        crate::sshhosts::select_pending(host, page);
    }
    if let Some(stack) = STACK.with(|s| s.borrow().upgrade()) {
        stack.set_visible_child_name(tab);
    }
}

/// 選ぶ欄の一覧に区切りの線を入れる（macOS 版の Picker の `Divider()`）。`above` の位置の項目の上に線を引く。
/// 一覧の項目は自前で作るので、選んでいる項目の印（✓）もここで出す。
pub fn separate_items(dropdown: &gtk::DropDown, above: Rc<RefCell<Vec<u32>>>) {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let column = gtk::Box::new(gtk::Orientation::Vertical, 4);
        column.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        label.set_hexpand(true);
        line.append(&label);
        let check = gtk::Image::from_icon_name("object-select-symbolic");
        item.bind_property("selected", &check, "visible")
            .sync_create()
            .build();
        line.append(&check);
        column.append(&line);
        item.set_child(Some(&column));
    });
    factory.connect_bind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let Some(column) = item.child() else { return };
        let Some(separator) = column.first_child() else {
            return;
        };
        let label = separator
            .next_sibling()
            .and_then(|line| line.first_child())
            .and_downcast::<gtk::Label>();
        let text = item
            .item()
            .and_downcast::<gtk::StringObject>()
            .map(|s| s.string());
        if let (Some(label), Some(text)) = (label, text) {
            label.set_text(&text);
        }
        separator.set_visible(above.borrow().contains(&item.position()));
    });
    dropdown.set_list_factory(Some(&factory));
}
