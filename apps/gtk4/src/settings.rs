//! 利用者の設定（macOS 版の `Settings.swift` のうち、GTK4 版に機能のあるもの）。`~/.config/amonhen/gtk.ini`
//! （GLib の KeyFile）に置く。GSettings はスキーマを入れる必要があり、展開しただけの実行ファイルでは使えないため。
//! 設定の窓は macOS 版と同じタブ（Git・Appearance・Performance・Advanced）。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;

const GROUP: &str = "settings";

/// 設定が変わったときに呼ぶ（変わった鍵）。
type Watcher = Rc<dyn Fn(&str)>;

thread_local! {
    static FILE: glib::KeyFile = load();
    static WATCHERS: RefCell<Vec<Watcher>> = RefCell::default();
    static WINDOW: RefCell<glib::WeakRef<gtk::Window>> = RefCell::default();
}

/// 鍵の名前（macOS 版の UserDefaults と同じ）。
pub const PULL_MODE: &str = "pullMode";
pub const MONOSPACED_COMMIT_MESSAGE: &str = "monospacedCommitMessage";
pub const WRITE_COMMIT_GRAPH: &str = "writeCommitGraph";
pub const OPEN_THROUGH_DAEMON: &str = "openThroughDaemon";
pub const WORKING_COPY_LAYOUT: &str = "workingCopyLayout";

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
    FILE.with(|f| f.boolean(GROUP, key))
        .unwrap_or(key == MONOSPACED_COMMIT_MESSAGE)
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

/// Pull の既定（コアの `pull` の `mode`）。
pub fn pull_mode() -> &'static str {
    match string(PULL_MODE).as_deref() {
        Some("rebase") => "rebase",
        Some("fastForwardOnly") => "fastForwardOnly",
        _ => "merge",
    }
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
    c.connect_toggled(move |c| set_bool(key, c.is_active()));
    c
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

    // Git
    let modes = [
        ("merge", "Merge"),
        ("rebase", "Rebase"),
        ("fastForwardOnly", "Fast-Forward Only"),
    ];
    let pull = gtk::DropDown::from_strings(&modes.map(|m| m.1));
    pull.set_selected(modes.iter().position(|m| m.0 == pull_mode()).unwrap_or(0) as u32);
    pull.connect_selected_notify(move |d| {
        if let Some(m) = modes.get(d.selected() as usize) {
            set_string(PULL_MODE, m.0);
        }
    });
    let pull_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let l = gtk::Label::new(Some("Pull の既定"));
    l.set_hexpand(true);
    l.set_xalign(0.0);
    pull_row.append(&l);
    pull_row.append(&pull);
    stack.add_titled(
        &page(&[section(
            None,
            &[
                pull_row.upcast(),
                note("ツールバーの Pull と Ctrl+Alt+L で使う。Pull ボタンの ▾ や Repository > Pull With でその都度も選べる。")
                    .upcast(),
            ],
        )]),
        Some("git"),
        "Git",
    );

    // Appearance
    stack.add_titled(
        &page(&[section(
            Some("コミットの詳細"),
            &[check(MONOSPACED_COMMIT_MESSAGE, "本文を等幅の字体で表示する").upcast()],
        )]),
        Some("appearance"),
        "Appearance",
    );

    // Performance
    stack.add_titled(
        &page(&[section(
            None,
            &[
                check(WRITE_COMMIT_GRAPH, "commit-graph を自動で書き出す").upcast(),
                note("commit-graph がないリポジトリを開いたとき、裏で git commit-graph write --reachable --changed-paths を実行する（.git に書き込む）。履歴の読み込みと path: での検索が速くなる。")
                    .upcast(),
            ],
        )]),
        Some("performance"),
        "Performance",
    );

    // Advanced
    stack.add_titled(
        &page(&[section(
            Some("共有の daemon"),
            &[
                check(OPEN_THROUGH_DAEMON, "リポジトリを共有の daemon 越しに開く（SSH 先も）").upcast(),
                note("History・検索の索引・操作の記録と SSH のつながりを、アプリ・CLI（amonhen -C daemon:… や -C ssh://…）・VSCode 拡張と共有する。これから開くリポジトリから効く。daemon の様子は Activity の Daemon。")
                    .upcast(),
            ],
        )]),
        Some("advanced"),
        "Advanced",
    );

    let switcher = gtk::StackSwitcher::builder().stack(&stack).build();
    let header = gtk::HeaderBar::new();
    header.set_title_widget(Some(&switcher));
    let window = gtk::Window::builder()
        .title("Settings")
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
