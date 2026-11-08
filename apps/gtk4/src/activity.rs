//! Activity の窓（macOS 版の `Activity.swift` と同じタブ）: Operations（実行した git とコアの命令）、History
//! （Undo できる操作と「ここまで戻す」）、Performance（画面の速さの記録）、Events（ファイルの変化の知らせ・
//! 読み直し）、Daemon（共有の daemon の様子と、起動・停止）。窓はアプリに 1 つで、開いているすべての窓・タブの
//! リポジトリを扱う（History のタブで選ぶ）。開いている間は 0.5 秒ごとに読み直す。

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::Arc;

use amonhen_core::Repository;
use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gio, glib};
use serde_json::{Value, json};

use crate::actions::Actions;

const KEEP: usize = 500;

/// 実行している操作の数の知らせ先（false を返したら外す）。
type RunningListener = Box<dyn Fn(u32) -> bool>;

thread_local! {
    /// 画面の速さの記録（項目ごとの直近の値、ms）。
    static PERFORMANCE: RefCell<HashMap<String, VecDeque<f64>>> = RefCell::default();
    /// 内部の出来事（時刻、リポジトリ、内容）。
    static EVENTS: RefCell<VecDeque<(i64, String, String)>> = RefCell::default();
    /// 開いている Activity の窓（アプリに 1 つ。macOS 版と同じ）。
    static OPEN: RefCell<Option<Activity>> = const { RefCell::new(None) };
    /// Activity に出す、開いているリポジトリのタブ（[`register`]）。
    static REGISTERED: RefCell<Vec<Registered>> = RefCell::default();
    /// 実行している操作の数（見出しの Activity に出す。macOS 版の `RunningOperations`）。
    static RUNNING: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// 数が変わったら知らせる先（false を返したものは外す）。
    static ON_RUNNING: RefCell<Vec<RunningListener>> = RefCell::default();
}

/// 操作を始めた（終わったら [`end`]）。
pub fn begin() {
    set_running(RUNNING.with(|r| r.get()) + 1);
}

/// 操作が終わった。
pub fn end() {
    set_running(RUNNING.with(|r| r.get()).saturating_sub(1));
}

fn set_running(count: u32) {
    RUNNING.with(|r| r.set(count));
    // 知らせる間に足されてもよいように、いったん取り出す。
    let mut listeners = ON_RUNNING.with(|l| l.take());
    listeners.retain(|f| f(count));
    ON_RUNNING.with(|l| {
        let mut l = l.borrow_mut();
        listeners.append(&mut l);
        *l = listeners;
    });
}

/// 実行している操作の数が変わったら `f(数)` を呼ぶ（今の数でもすぐ呼ぶ）。`f` が false を返したら外す。
pub fn connect_running(f: impl Fn(u32) -> bool + 'static) {
    if f(RUNNING.with(|r| r.get())) {
        ON_RUNNING.with(|l| l.borrow_mut().push(Box::new(f)));
    }
}

/// 見出しの Activity のボタン: 操作を実行している間は回り、押すと Activity の窓（macOS 版のツールバーと同じ）。
pub fn toolbar_button() -> gtk::Button {
    let spinner = gtk::Spinner::new();
    spinner.set_visible(false);
    let icon = crate::icon::Icon::default();
    icon.set(crate::icon::Kind::Activity, false);
    let inner = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    inner.append(&spinner);
    inner.append(&icon);
    let button = gtk::Button::new();
    button.set_child(Some(&inner));
    button.set_action_name(Some("win.activity"));
    let b = button.downgrade();
    connect_running(move |count| {
        let Some(button) = b.upgrade() else {
            return false;
        };
        spinner.set_visible(count > 0);
        spinner.set_spinning(count > 0);
        button.set_tooltip_text(Some(&if count > 0 {
            tr!(
                "{count} operations running (click for Activity)",
                count = count
            )
        } else {
            tr!("Git and commands run, the operation log, and UI speed (Ctrl+Alt+A)")
        }));
        true
    });
    button
}

/// 画面の速さを 1 つ記録する（メインスレッドから）。
pub fn record(metric: &str, ms: f64) {
    PERFORMANCE.with(|p| {
        let mut p = p.borrow_mut();
        let values = p.entry(metric.to_owned()).or_default();
        values.push_back(ms);
        if values.len() > KEEP {
            values.pop_front();
        }
    });
}

/// 内部の出来事を 1 つ記録する（メインスレッドから）。
pub fn event(repo: &str, text: impl Into<String>) {
    let now = glib::DateTime::now_local()
        .map_or(0, |d| d.to_unix() * 1000 + d.microsecond() as i64 / 1000);
    EVENTS.with(|e| {
        let mut e = e.borrow_mut();
        e.push_back((now, repo.to_owned(), text.into()));
        if e.len() > KEEP {
            e.pop_front();
        }
    });
}

fn time(ms: i64) -> String {
    glib::DateTime::from_unix_local(ms / 1000)
        .and_then(|d| d.format("%H:%M:%S"))
        .map(|s| format!("{s}.{:03}", ms % 1000))
        .unwrap_or_default()
}

fn ms(value: f64) -> String {
    if value < 10.0 {
        format!("{value:.2}")
    } else {
        format!("{value:.0}")
    }
}

/// Activity の窓の大きさを覚える設定の鍵（「幅x高さ」）。
const WINDOW_SIZE: &str = "activityWindowSize";

/// 時間（ミリ秒）を、秒・分・時間で（macOS 版の Daemon のタブと同じ）。
fn duration(ms: u64) -> String {
    match ms / 1000 {
        s if s < 60 => tr!("{s} s", s = s),
        s if s < 3600 && s % 60 == 0 => tr!("{m} min", m = s / 60),
        s if s < 3600 => tr!("{m} min {s} s", m = s / 60, s = s % 60),
        s => tr!("{h} h {m} min", h = s / 3600, m = s % 3600 / 60),
    }
}

/// 並べた `len` 個のうち、割合 `p` の位置（macOS 版と同じく `len × p` を切り捨て、最後を超えない）。
fn quantile_index(len: usize, p: f64) -> usize {
    ((len as f64 * p) as usize).min(len.saturating_sub(1))
}

fn label(text: &str, css: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    for c in css {
        l.add_css_class(c);
    }
    l
}

/// 成功・失敗・実行中の色（テーマの色を使う）。
fn install_css() {
    thread_local! {
        static DONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    if DONE.with(|d| d.replace(true)) {
        return;
    }
    let provider = gtk::CssProvider::new();
    provider.load_from_string(
        ".activity-success { color: @success_color; } \
         .activity-error { color: @error_color; } \
         .activity-warning { color: @warning_color; } \
         button.activity-small { min-height: 20px; padding: 1px 8px; font-size: smaller; }",
    );
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// 選べるか（選べても最初のフォーカスは取らない）。
fn selectable(label: &gtk::Label, on: bool) {
    label.set_selectable(on);
    label.set_focusable(false);
}

/// 前に出したときと同じ中身か（同じなら作り直さない。スクロールや説明がそのまま残る）。
fn unchanged(last: &std::cell::Cell<u64>, key: impl std::hash::Hash) -> bool {
    use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};
    let hash = BuildHasherDefault::<DefaultHasher>::default().hash_one(key);
    last.replace(hash) == hash
}

/// 時刻の列の幅（`23:44:31.539`）。
const TIME: i32 = 100;
/// リポジトリの列の幅（`name — host · daemon` も入るように、macOS 版の列より広く）。
const REPOSITORY: i32 = 180;
/// リポジトリの列を、名前に合わせて広げるときの上限（中身の列を残す）。
const REPOSITORY_MAX: i32 = 320;

/// リポジトリの列の幅: いちばん長い名前が入る幅（[`REPOSITORY`] から [`REPOSITORY_MAX`] まで）。文字の大きさは
/// 機械ごとに違うので、決まった幅では名前が切れる。
fn repository_width<S: AsRef<str>>(widget: &impl IsA<gtk::Widget>, names: &[S]) -> i32 {
    names
        .iter()
        .map(|n| widget.create_pango_layout(Some(n.as_ref())).pixel_size().0 + 2)
        .fold(REPOSITORY, i32::max)
        .min(REPOSITORY_MAX)
}

/// 中身を作り直す一覧（行は少ないので ListBox で足りる）。macOS 版で Table の一覧は `zebra` を付けて行を交互に塗る。
fn list() -> (gtk::ScrolledWindow, gtk::ListBox) {
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("view");
    let scrolled = gtk::ScrolledWindow::builder()
        .child(&list)
        .vexpand(true)
        .build();
    (scrolled, list)
}

/// 行（列の文字を左から並べる。幅はピクセル、0 は残りすべて）。幅は macOS 版の表の列に合わせ、文字の大きい分だけ
/// 広くする（文字数で決めると、日本語や長いリポジトリ名が切れる）。
fn row(cells: &[(&str, i32, &[&str])]) -> gtk::Box {
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    line.set_margin_top(3);
    line.set_margin_bottom(3);
    line.set_margin_start(6);
    line.set_margin_end(6);
    for (text, width, css) in cells {
        let l = label(text, css);
        l.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        if *width > 0 {
            // 中身の長さで幅が変わらないよう、自然な幅を縮めてからピクセルで決める（列が揃う）。
            l.set_max_width_chars(1);
            l.set_size_request(*width, -1);
        } else {
            l.set_hexpand(true);
        }
        line.append(&l);
    }
    line
}

/// 表の見出しの行（macOS 版の Table の列の見出し）。一覧の上に置き、流れない。行と同じ余白になるよう、一覧と
/// 同じ ListBox の行に入れる。返す行の部品は、列の幅を合わせ直すのに使う。
fn column_header(cells: &[(&str, i32)]) -> (gtk::ListBox, gtk::Box) {
    let cells: Vec<(&str, i32, &[&str])> = cells
        .iter()
        .map(|(t, w)| (*t, *w, &["dim-label", "caption-heading"][..]))
        .collect();
    let line = row(&cells);
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.append(&line);
    if let Some(r) = list.row_at_index(0) {
        r.set_activatable(false);
    }
    (list, line)
}

/// Operations の記録の印（どの daemon 越しのリポジトリのものか、番号。一覧を作り直しても選び直せる）。
fn record_key(r: &Value) -> String {
    format!(
        "{}:{}",
        r["remoteKey"]
            .as_str()
            .or(r["remote"].as_str())
            .unwrap_or_default(),
        r["seq"].as_u64().unwrap_or(0)
    )
}

/// Operations の詳細: リポジトリ・内容・エラー（macOS 版と同じ）。
fn record_detail(r: &Value) -> String {
    let mut text = format!(
        "{}\n{}",
        r["repo"].as_str().unwrap_or_default(),
        r["label"].as_str().unwrap_or_default()
    );
    if let Some(e) = r["error"].as_str() {
        text.push_str(&format!("\n\n{e}"));
    }
    text
}

/// `line` の `index` 番目の列を、頭を切って縮める（パス。終わりが見える）。
fn ellipsize_start(line: &gtk::Box, index: usize) {
    let mut cell = line.first_child();
    for _ in 0..index {
        cell = cell.and_then(|c| c.next_sibling());
    }
    if let Some(l) = cell.and_downcast::<gtk::Label>() {
        l.set_ellipsize(gtk::pango::EllipsizeMode::Start);
    }
}

/// `line` の `index` 番目の列の幅を変える（リポジトリの列は、名前の長さで幅が変わる）。
fn set_column_width(line: &gtk::Box, index: usize, width: i32) {
    let mut cell = line.first_child();
    for _ in 0..index {
        cell = cell.and_then(|c| c.next_sibling());
    }
    if let Some(cell) = cell {
        cell.set_size_request(width, -1);
    }
}

/// 選んでいるものを 1 つだけ押した形で出す、つながったボタン（macOS 版の segmented の Picker）。
struct Segmented {
    root: gtk::Box,
    buttons: Vec<gtk::ToggleButton>,
}

impl Segmented {
    fn new(labels: &[&str]) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.add_css_class("linked");
        let buttons: Vec<gtk::ToggleButton> = labels
            .iter()
            .map(|l| gtk::ToggleButton::with_label(l))
            .collect();
        for b in &buttons {
            if let Some(first) = buttons.first()
                && b != first
            {
                b.set_group(Some(first));
            }
            root.append(b);
        }
        if let Some(first) = buttons.first() {
            first.set_active(true);
        }
        Self { root, buttons }
    }

    fn selected(&self) -> usize {
        self.buttons.iter().position(|b| b.is_active()).unwrap_or(0)
    }

    /// 選び直したとき（押されて選ばれたものだけで呼ぶ）。
    fn connect_changed(&self, f: impl Fn() + 'static) {
        let f = Rc::new(f);
        for b in &self.buttons {
            let f = f.clone();
            b.connect_toggled(move |b| {
                if b.is_active() {
                    f();
                }
            });
        }
    }
}

/// タブが今見ているリポジトリ（まだつないでいなければ None）。
pub type CurrentRepo = Rc<dyn Fn() -> Option<Arc<Repository>>>;

/// 選び直したとき（あとで入れる）。
type Hook = Rc<RefCell<Option<Rc<dyn Fn()>>>>;
/// Daemon のタブの「このアプリのつながり」の 1 行（名前・場所・切れた理由・送受信量・SSH 先のエンジン）。
type ConnectionRow = (String, String, Option<String>, (u64, u64), String);

/// Activity に出す、開いているタブ。
struct Registered {
    tab: std::rc::Weak<crate::tabs::Tab>,
    current: CurrentRepo,
    actions: std::rc::Weak<Actions>,
}

/// リポジトリのタブを Activity の一覧に入れる（タブを閉じたら外れる）。`actions` は History の Undo の先。
pub fn register(tab: &Rc<crate::tabs::Tab>, current: CurrentRepo, actions: &Rc<Actions>) {
    REGISTERED.with(|r| {
        r.borrow_mut().push(Registered {
            tab: Rc::downgrade(tab),
            current,
            actions: Rc::downgrade(actions),
        })
    });
}

/// Activity が読む、開いているリポジトリ（macOS 版の `ActivityRepository`）。
#[derive(Clone)]
struct Entry {
    /// タブ（窓）の名前。
    name: String,
    /// 開いた場所（手元のパス、`daemon:/path`、`ssh://host/path`）。
    location: String,
    repo: Arc<Repository>,
    /// History の Undo の先（書き込む先の worktree。Undo の記録もここから読む）。
    actions: Rc<Actions>,
}

impl Entry {
    /// daemon 越しのリポジトリか（SSH 先・手元の共有の daemon。daemon の記録も読む）。
    fn is_remote(&self) -> bool {
        self.repo.connection_state().is_some()
    }

    /// daemon の記録を、どのリポジトリのものか見分ける鍵（開いているリポジトリごとに違う。窓の名前は同じことが
    /// あるので、名前では分けない。macOS 版と同じ）。
    fn key(&self) -> String {
        format!("{:p}", Arc::as_ptr(&self.repo))
    }

    fn same(&self, other: &Entry) -> bool {
        Arc::ptr_eq(&self.repo, &other.repo) && Rc::ptr_eq(&self.actions, &other.actions)
    }
}

/// 開いているすべての窓・タブのリポジトリ（開いた順。macOS 版と同じ）。
fn repositories() -> Vec<Entry> {
    REGISTERED.with(|r| {
        let mut r = r.borrow_mut();
        r.retain(|e| e.tab.upgrade().is_some_and(|t| !t.closed()) && e.actions.strong_count() > 0);
        r.iter()
            .filter_map(|e| {
                let tab = e.tab.upgrade()?;
                Some(Entry {
                    name: tab.name.clone(),
                    location: tab.location.clone(),
                    repo: (e.current)()?,
                    actions: e.actions.upgrade()?,
                })
            })
            .collect()
    })
}

/// SSH 先・daemon のどこを通っているか（Daemon のタブの表）。
fn location_kind(location: &str) -> String {
    if location.starts_with("ssh://") {
        tr!("SSH host (direct)")
    } else if location.starts_with("daemon:ssh://") {
        tr!("Shared daemon → SSH host")
    } else {
        tr!("Local shared daemon")
    }
}

#[derive(Clone)]
pub struct Activity {
    window: gtk::ApplicationWindow,
    stack: gtk::Stack,
    /// すべてのタブを読み直す。
    refresh: Rc<dyn Fn()>,
    /// 0.5 秒ごとの読み直しが動いている。
    ticking: Rc<std::cell::Cell<bool>>,
}

impl Activity {
    /// Activity を開く（開いていれば前に出す）。初めて開くときは、History のタブで開いているリポジトリの最初の
    /// ものを選ぶ。閉じても隠すだけで、開き直したときはタブ・種類や Failures Only・Filter・選んでいた行・
    /// History のリポジトリを保つ（macOS 版は窓のコントローラーを使い回す）。
    pub fn show(parent: &gtk::ApplicationWindow, engine: std::path::PathBuf) {
        let activity = match OPEN.with(|o| o.borrow().clone()) {
            Some(a) => a,
            None => {
                let a = Self::new(parent, engine);
                OPEN.with(|o| o.replace(Some(a.clone())));
                a
            }
        };
        // 出している間はアプリの窓にする（閉じたら外す。Settings と同じ）。
        if activity.window.application().is_none()
            && let Some(app) = parent.application()
        {
            activity.window.set_application(Some(&app));
        }
        activity.window.present();
        activity.start();
    }

    /// 読み直し、開いている間は 0.5 秒ごとに読み直す（macOS 版と同じ。daemon の記録と様子は 2 秒ごと。History の
    /// タブは、見せたときと操作の後に）。
    fn start(&self) {
        (self.refresh)();
        if self.ticking.replace(true) {
            return;
        }
        let (w, refresh, ticking) = (
            self.window.downgrade(),
            self.refresh.clone(),
            self.ticking.clone(),
        );
        glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
            if w.upgrade().is_none_or(|w| !w.is_visible()) {
                ticking.set(false);
                return glib::ControlFlow::Break;
            }
            refresh();
            glib::ControlFlow::Continue
        });
    }

    /// 開いている Activity のタブ（`operations` など）を選ぶ。
    pub fn select(tab: &str) {
        if let Some(a) = Self::find() {
            a.stack.set_visible_child_name(tab);
        }
    }

    /// 開いている Activity の窓（`--activity` の `--snapshot` で撮る）。
    pub fn window() -> Option<gtk::Window> {
        Self::find().map(|a| a.window.clone().upcast())
    }

    fn find() -> Option<Self> {
        OPEN.with(|o| o.borrow().clone())
            .filter(|a| a.window.is_visible())
    }

    fn new(parent: &gtk::ApplicationWindow, engine: std::path::PathBuf) -> Self {
        install_css();
        let stack = gtk::Stack::new();
        let switcher = gtk::StackSwitcher::builder().stack(&stack).build();
        let header = gtk::HeaderBar::new();
        header.set_title_widget(Some(&switcher));
        // アプリの窓にする: Ctrl+Q が効き、リポジトリの窓を閉じても開いている間はアプリが続く（macOS 版と同じ）。
        let mut builder = gtk::ApplicationWindow::builder()
            .title("Activity")
            .default_width(900)
            .default_height(560)
            .hide_on_close(true)
            .child(&stack);
        if let Some(app) = parent.application() {
            builder = builder.application(&app);
        }
        let window = builder.build();
        // macOS 版と同じく、最小 760×460。
        window.set_size_request(760, 460);
        window.set_titlebar(Some(&header));
        // Ctrl+W（`win.close`）で閉じる（macOS 版の ⌘W）。
        let close = gio::SimpleAction::new("close", None);
        let weak = window.downgrade();
        close.connect_activate(move |_, _| {
            if let Some(w) = weak.upgrade() {
                w.close();
            }
        });
        window.add_action(&close);
        // どの窓のものでもない（アプリに 1 つ。macOS 版と同じ）。最初は開いた窓の画面に出す。
        window.set_display(&WidgetExt::display(parent));
        // 大きさは覚えておく（macOS 版は位置も。GTK では窓の位置はウィンドウマネージャーが決める）。
        if let Some((w, h)) = crate::settings::string(WINDOW_SIZE)
            .as_deref()
            .and_then(|s| s.split_once('x'))
            .and_then(|(w, h)| Some((w.parse::<i32>().ok()?, h.parse::<i32>().ok()?)))
            .filter(|&(w, h)| w > 200 && h > 150)
        {
            window.set_default_size(w, h);
        }
        window.connect_close_request(|window| {
            let (w, h) = window.default_size();
            if w > 0 && h > 0 {
                crate::settings::set_string(WINDOW_SIZE, &format!("{w}x{h}"));
            }
            // 隠すだけなので、アプリの窓から外す（ほかに窓がなければアプリを終える。Settings と同じ）。
            window.set_application(None::<&gtk::Application>);
            glib::Propagation::Proceed
        });

        let mut refreshers: Vec<Box<dyn Fn()>> = Vec::new();
        // 開いているすべてのリポジトリ（読み直すたびに集め直す）と、History のタブで選んでいるもの（macOS 版の
        // Repository の選択。なければ最初のもの）。
        let entries: Rc<RefCell<Vec<Entry>>> = Rc::default();
        let selected: Rc<RefCell<Option<Entry>>> = Rc::default();
        let repo_choice = gtk::DropDown::from_strings(&[]);
        repo_choice.set_width_request(320);
        repo_choice.set_halign(gtk::Align::Start);
        // 選び直したとき（History の一覧を読み直す。下で入れる）。
        let on_selected: Hook = Rc::default();
        let choosing = Rc::new(std::cell::Cell::new(false));
        let refresh_repos: Rc<dyn Fn()> = {
            let (entries, selected, repo_choice, on_selected, choosing) = (
                entries.clone(),
                selected.clone(),
                repo_choice.clone(),
                on_selected.clone(),
                choosing.clone(),
            );
            Rc::new(move || {
                let now = repositories();
                let same_list = {
                    let old = entries.borrow();
                    old.len() == now.len()
                        && old
                            .iter()
                            .zip(&now)
                            .all(|(a, b)| a.same(b) && a.name == b.name)
                };
                if same_list {
                    return;
                }
                let names: Vec<&str> = now.iter().map(|e| e.name.as_str()).collect();
                let index = selected
                    .borrow()
                    .as_ref()
                    .and_then(|s| now.iter().position(|e| e.same(s)))
                    .or((!now.is_empty()).then_some(0));
                choosing.set(true);
                repo_choice.set_model(Some(&gtk::StringList::new(&names)));
                repo_choice.set_selected(index.map_or(gtk::INVALID_LIST_POSITION, |i| i as u32));
                choosing.set(false);
                let chosen = index.map(|i| now[i].clone());
                let changed = match (&*selected.borrow(), &chosen) {
                    (Some(a), Some(b)) => !a.same(b),
                    (None, None) => false,
                    _ => true,
                };
                entries.replace(now);
                selected.replace(chosen);
                if changed && let Some(f) = on_selected.borrow().clone() {
                    f();
                }
            })
        };
        {
            let (entries, selected, on_selected, choosing) = (
                entries.clone(),
                selected.clone(),
                on_selected.clone(),
                choosing.clone(),
            );
            repo_choice.connect_selected_notify(move |d| {
                if choosing.get() {
                    return;
                }
                let chosen = entries.borrow().get(d.selected() as usize).cloned();
                selected.replace(chosen);
                if let Some(f) = on_selected.borrow().clone() {
                    f();
                }
            });
        }
        refresh_repos();
        let r = refresh_repos.clone();
        refreshers.push(Box::new(move || r()));
        let margin = |w: &gtk::Box| {
            w.set_margin_top(10);
            w.set_margin_bottom(10);
            w.set_margin_start(12);
            w.set_margin_end(12);
        };

        // Operations: git の起動とコアの命令。
        let ops = gtk::Box::new(gtk::Orientation::Vertical, 8);
        margin(&ops);
        let kinds = Rc::new(Segmented::new(&[
            tr!("All").as_str(),
            "git",
            tr!("Commands").as_str(),
        ]));
        let failures = gtk::CheckButton::with_label(&tr!("Failures Only"));
        // 素の文字の欄（macOS 版の TextField。打つたびに絞り込む）。
        let filter = gtk::Entry::builder()
            .placeholder_text(tr!("Filter"))
            .hexpand(true)
            .build();
        let export = gtk::Button::with_label(&tr!("Export JSON…"));
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        bar.append(&kinds.root);
        bar.append(&failures);
        bar.append(&filter);
        bar.append(&export);
        let (scrolled, op_list) = list();
        op_list.add_css_class("zebra");
        // 選んだ記録の、リポジトリ・内容・エラーを選んでコピーできる字で（macOS 版と同じ）。
        op_list.set_selection_mode(gtk::SelectionMode::Single);
        let op_detail = label("", &["caption", "monospace"]);
        op_detail.set_selectable(true);
        op_detail.set_wrap(true);
        op_detail.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        op_detail.set_valign(gtk::Align::Start);
        let op_detail_scrolled = gtk::ScrolledWindow::builder()
            .child(&op_detail)
            .min_content_height(90)
            .max_content_height(90)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .has_frame(true)
            .visible(false)
            .build();
        // 一覧を作り直しても選び直せるよう、選んだ記録の印（daemon のものか、番号）を覚えておく。
        let op_selected: Rc<RefCell<Option<String>>> = Rc::default();
        let op_rebuilding = Rc::new(std::cell::Cell::new(false));
        {
            let (detail, scrolled, selected, rebuilding) = (
                op_detail.clone(),
                op_detail_scrolled.clone(),
                op_selected.clone(),
                op_rebuilding.clone(),
            );
            op_list.connect_row_selected(move |_, row| {
                if rebuilding.get() {
                    return;
                }
                let text = row.and_then(|r| r.child()).and_then(|c| unsafe {
                    c.data::<String>("detail").map(|d| d.as_ref().clone())
                });
                selected.replace(
                    row.and_then(|r| r.child())
                        .map(|c| c.widget_name().to_string()),
                );
                scrolled.set_visible(text.is_some());
                detail.set_text(text.as_deref().unwrap_or_default());
            });
        }
        let (op_header, op_header_line) = column_header(&[
            (tr!("Time").as_str(), TIME),
            ("", 16),
            (tr!("Repository").as_str(), REPOSITORY),
            (tr!("Content").as_str(), 0),
            ("ms", 64),
        ]);
        ops.append(&bar);
        ops.append(&op_header);
        ops.append(&scrolled);
        ops.append(&op_detail_scrolled);
        ops.append(&label(
            &tr!("git runs and core commands, newest first, up to 1,000 (for repositories through the daemon, also the git it starts). ● means running; fetch, pull, push and clone show their progress and can be canceled."),
            &["dim-label", "caption"],
        ));
        stack.add_titled(&ops, Some("operations"), &tr!("Operations"));
        // daemon 越しのリポジトリの記録（裏で 2 秒ごとに読む。macOS 版と同じく、名前はタブの名前）。
        let remote_records: Rc<RefCell<Vec<Value>>> = Rc::default();
        let refresh_ops = {
            let (op_selected, op_rebuilding, op_detail, op_detail_scrolled, op_header_line) = (
                op_selected.clone(),
                op_rebuilding.clone(),
                op_detail.clone(),
                op_detail_scrolled.clone(),
                op_header_line.clone(),
            );
            let (op_list, kinds, failures, filter, entries, remote_records) = (
                op_list.clone(),
                kinds.clone(),
                failures.clone(),
                filter.clone(),
                entries.clone(),
                remote_records.clone(),
            );
            let loading = Rc::new(std::cell::Cell::new(false));
            let tick = std::cell::Cell::new(0u32);
            let last = std::cell::Cell::new(0);
            move || {
                tick.set(tick.get() + 1);
                let remotes: Vec<Entry> = entries
                    .borrow()
                    .iter()
                    .filter(|e| e.is_remote())
                    .cloned()
                    .collect();
                if remotes.is_empty() {
                    remote_records.borrow_mut().clear();
                } else if !loading.get() && tick.get() % 4 == 1 {
                    loading.set(true);
                    let (remote_records, loading) = (remote_records.clone(), loading.clone());
                    glib::spawn_future_local(async move {
                        let mut loaded = Vec::new();
                        for entry in remotes {
                            let asked = entry.repo.clone();
                            let reply: Value = gio::spawn_blocking(move || {
                                serde_json::from_str(&asked.run_json(r#"{"op":"remoteActivity"}"#))
                                    .unwrap_or_default()
                            })
                            .await
                            .unwrap_or_default();
                            for mut r in reply["info"]["records"]
                                .as_array()
                                .cloned()
                                .unwrap_or_default()
                            {
                                r["remote"] = json!(entry.name);
                                r["remoteKey"] = json!(entry.key());
                                loaded.push(r);
                            }
                        }
                        loading.set(false);
                        remote_records.replace(loaded);
                    });
                }
                let mut records: Vec<Value> = amonhen_model::activity::entries()
                    .into_iter()
                    .map(|e| {
                        json!({
                            "seq": e.seq, "kind": e.kind, "repo": e.repo, "label": e.label,
                            "started": e.started, "duration": e.duration, "ok": e.ok,
                            "error": e.error, "progress": e.progress, "cancellable": e.cancellable,
                        })
                    })
                    .collect();
                records.extend(remote_records.borrow().iter().cloned());
                records.sort_by_key(|r| std::cmp::Reverse(r["started"].as_u64().unwrap_or(0)));
                let kind = match kinds.selected() {
                    1 => Some("git"),
                    2 => Some("command"),
                    _ => None,
                };
                let text = filter.text().to_lowercase();
                let key = (
                    Value::Array(records.clone()).to_string(),
                    kind,
                    failures.is_active(),
                    text.clone(),
                );
                if unchanged(&last, key) {
                    return;
                }
                op_rebuilding.set(true);
                op_list.remove_all();
                let wanted = op_selected.borrow().clone();
                // すべての記録から絞り込み、合うものを新しい順に 1,000 件まで（macOS 版と同じ）。
                let shown = records.iter().filter(|r| {
                    let label_text = r["label"].as_str().unwrap_or_default();
                    let repo_path = r["repo"].as_str().unwrap_or_default();
                    !(kind.is_some_and(|k| r["kind"].as_str() != Some(k))
                        || (failures.is_active() && r["ok"].as_bool() != Some(false))
                        || (!text.is_empty()
                            && !label_text.to_lowercase().contains(&text)
                            && !repo_path.to_lowercase().contains(&text)))
                });
                let shown: Vec<&Value> = shown.take(1000).collect();
                let names: Vec<String> = shown
                    .iter()
                    .map(|r| match r["remote"].as_str() {
                        Some(name) => name.to_owned(),
                        None => {
                            let repo_path = r["repo"].as_str().unwrap_or_default();
                            repo_path.rsplit('/').next().unwrap_or_default().to_owned()
                        }
                    })
                    .collect();
                let name_width = repository_width(&op_list, &names);
                set_column_width(&op_header_line, 2, name_width);
                for (r, name) in shown.into_iter().zip(names) {
                    let ok = r["ok"].as_bool();
                    let label_text = r["label"].as_str().unwrap_or_default();
                    let mark = match ok {
                        None => ("●", "activity-warning"),
                        Some(true) => ("✓", "activity-success"),
                        Some(false) => ("✗", "activity-error"),
                    };
                    let remote = r["remote"].as_str();
                    let duration = r["duration"].as_f64().map_or("…".to_owned(), ms);
                    let line = row(&[
                        (
                            &time(r["started"].as_i64().unwrap_or(0)),
                            TIME,
                            &["numeric", "dim-label"],
                        ),
                        (mark.0, 16, &[mark.1]),
                        (&name, name_width, &[]),
                        (label_text, 0, &[]),
                        (&duration, 64, &["numeric"]),
                    ]);
                    let tip = record_detail(r);
                    let key = record_key(r);
                    // 中身の列: 内容（説明は内容だけ）、走っていれば進み具合（小さく薄く）、右寄せの「取り消す」
                    // （macOS 版と同じ）。
                    let content_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                    content_box.set_hexpand(true);
                    if let Some(content) = line
                        .first_child()
                        .and_then(|c| c.next_sibling())
                        .and_then(|c| c.next_sibling())
                        .and_then(|c| c.next_sibling())
                        && let Some(before) = content.prev_sibling()
                    {
                        line.remove(&content);
                        content.set_hexpand(false);
                        content.set_tooltip_text(Some(label_text));
                        content_box.append(&content);
                        line.insert_child_after(&content_box, Some(&before));
                    }
                    if ok.is_none()
                        && let Some(p) = r["progress"].as_str()
                    {
                        let progress = label(p, &["caption", "dim-label"]);
                        progress.set_ellipsize(gtk::pango::EllipsizeMode::End);
                        content_box.append(&progress);
                    }
                    line.set_widget_name(&key);
                    // SAFETY: 鍵 "detail" には、いつも String を入れて String で読む。
                    unsafe { line.set_data("detail", tip.clone()) };
                    let reselect = wanted.as_deref() == Some(key.as_str());
                    // 走っていて取り消せる操作には「取り消す」。
                    if ok.is_none() && r["cancellable"].as_bool() == Some(true) {
                        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                        spacer.set_hexpand(true);
                        content_box.append(&spacer);
                        let cancel = gtk::Button::with_label(&tr!("Cancel Operation"));
                        cancel.add_css_class("activity-small");
                        let seq = r["seq"].as_u64().unwrap_or(0);
                        let remote = remote.and(r["remoteKey"].as_str()).map(str::to_owned);
                        let entries = entries.clone();
                        cancel.connect_clicked(move |_| {
                            if let Some(key) = &remote {
                                // daemon への往復なので、裏の糸で（macOS 版と同じ）。
                                let repo = entries
                                    .borrow()
                                    .iter()
                                    .find(|e| e.key() == *key)
                                    .map(|e| e.repo.clone());
                                if let Some(repo) = repo {
                                    let command = json!({ "op": "cancelActivity", "seq": seq, "remote": true });
                                    gio::spawn_blocking(move || {
                                        let _ = repo.run_json(&command.to_string());
                                    });
                                }
                            } else {
                                amonhen_model::activity::cancel(seq);
                            }
                        });
                        content_box.append(&cancel);
                    }
                    op_list.append(&line);
                    if reselect
                        && let Some(row) = op_list.last_child().and_downcast::<gtk::ListBoxRow>()
                    {
                        op_list.select_row(Some(&row));
                        op_detail.set_text(&tip);
                    }
                }
                op_rebuilding.set(false);
                // 選んでいた記録が絞り込みで隠れても、詳細は出したまま（macOS 版と同じく、すべての記録から探す）。
                // 記録そのものがなくなったら消す。
                if op_list.selected_row().is_none() {
                    let wanted = op_selected.borrow().clone();
                    match wanted.and_then(|w| records.iter().find(|r| record_key(r) == w)) {
                        Some(r) => op_detail.set_text(&record_detail(r)),
                        None => {
                            op_selected.replace(None);
                            op_detail.set_text("");
                            op_detail_scrolled.set_visible(false);
                        }
                    }
                }
            }
        };
        // 絞り込みを変えたら、すぐに出し直す。
        let refresh_ops = Rc::new(refresh_ops);
        let r = refresh_ops.clone();
        kinds.connect_changed(move || r());
        let r = refresh_ops.clone();
        failures.connect_toggled(move |_| r());
        let r = refresh_ops.clone();
        filter.connect_changed(move |_| r());
        refreshers.push(Box::new(move || refresh_ops()));
        let w = window.downgrade();
        export.connect_clicked(move |_| {
            let Some(window) = w.upgrade() else { return };
            // 題は付けない（macOS 版の NSSavePanel と同じ）。
            let dialog = gtk::FileDialog::builder()
                .initial_name("amonhen-activity.json")
                .modal(true)
                .build();
            glib::spawn_future_local(async move {
                let Ok(file) = dialog.save_future(Some(&window)).await else {
                    return;
                };
                let records: Vec<Value> = amonhen_model::activity::entries()
                    .into_iter()
                    .map(|e| {
                        json!({
                            "seq": e.seq, "kind": e.kind, "repo": e.repo, "label": e.label,
                            "started": e.started, "duration": e.duration, "ok": e.ok, "error": e.error,
                        })
                    })
                    .collect();
                if let Some(path) = file.path() {
                    let text = serde_json::to_string_pretty(&records).unwrap_or_default();
                    let _ = std::fs::write(path, text);
                }
            });
        });

        // History: Undo できる操作（新しい順）と「ここまで戻す」。
        let history = gtk::Box::new(gtk::Orientation::Vertical, 8);
        margin(&history);
        let (scrolled, undo_list) = list();
        // どのリポジトリの操作か（開いているすべての窓・タブから選ぶ。macOS 版と同じ）。
        let choice_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        choice_row.append(&label(&tr!("Repository"), &[]));
        choice_row.append(&repo_choice);
        history.append(&choice_row);
        history.append(&scrolled);
        history.append(&label(
            &tr!("Operations you can Undo (newest first). It stops where another tool has moved a ref."),
            &["dim-label", "caption"],
        ));
        stack.add_titled(&history, Some("history"), "History");
        let refresh_undo: Rc<dyn Fn()> = {
            let (undo_list, selected, w) =
                (undo_list.clone(), selected.clone(), window.downgrade());
            Rc::new(move || {
                // Undo する先（書き込む先の worktree）の記録を出す（一覧と Undo の先を取り違えない）。
                let Some(entry) = selected.borrow().clone() else {
                    undo_list.remove_all();
                    return;
                };
                let actions = entry.actions.clone();
                let repo = actions.repo().unwrap_or_else(|| entry.repo.clone());
                let (undo_list, selected, w) = (undo_list.clone(), selected.clone(), w.clone());
                glib::spawn_future_local(async move {
                    let r = repo.clone();
                    let reply: Value = gio::spawn_blocking(move || {
                        serde_json::from_str(&r.run_json(r#"{"op":"undoLog"}"#)).unwrap_or_default()
                    })
                    .await
                    .unwrap_or_default();
                    // 読んでいる間に選び直したら捨てる（選んだほうで読み直す）。
                    if !selected
                        .borrow()
                        .as_ref()
                        .is_some_and(|e| Rc::ptr_eq(&e.actions, &actions))
                    {
                        return;
                    }
                    undo_list.remove_all();
                    let entries = reply["info"]["entries"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                    for (i, e) in entries.iter().enumerate() {
                        let line = row(&[
                            (
                                &time(e["time"].as_f64().unwrap_or(0.0) as i64),
                                TIME,
                                &["numeric", "dim-label"],
                            ),
                            (e["label"].as_str().unwrap_or_default(), 0, &[]),
                        ]);
                        let button = gtk::Button::with_label(&if i == 0 {
                            "Undo".to_owned()
                        } else {
                            tr!("Undo to Here")
                        });
                        // 枠のあるボタン（macOS 版と同じ）。
                        button.set_valign(gtk::Align::Center);
                        button.set_tooltip_text(Some(&if i == 0 {
                            tr!("Undo this operation.")
                        } else {
                            tr!(
                                "Undo the {count} latest operations, down to and including this one.",
                                count = i + 1
                            )
                        }));
                        // 読んだ worktree の記録を戻す（読み直すまでの間に書き込む先を替えても）。
                        let (a, w, repo) = (actions.clone(), w.clone(), repo.clone());
                        let title = if i == 0 {
                            tr!(
                                "Undo “{label}”?",
                                label = e["label"].as_str().unwrap_or_default()
                            )
                        } else {
                            tr!("Undo the last {count} operations?", count = i + 1)
                        };
                        // 戻す前に確かめる（macOS 版と同じ）。
                        button.connect_clicked(move |_| {
                            let (a, w, title, repo) =
                                (a.clone(), w.clone(), title.clone(), repo.clone());
                            glib::spawn_future_local(async move {
                                let sheet = crate::sheets::Sheet::new(
                                    w.upgrade().map(|w| w.upcast::<gtk::Window>()).as_ref(),
                                    &title,
                                    &tr!("Returns HEAD and the branches to where they were."),
                                );
                                sheet.buttons("Undo", false, None);
                                if sheet.run().await == crate::sheets::Response::Ok {
                                    a.perform_in(
                                        repo,
                                        vec![json!({ "op": "undo" }); i + 1],
                                        |_| {},
                                    );
                                }
                            });
                        });
                        line.append(&button);
                        undo_list.append(&line);
                    }
                });
            })
        };
        refresh_undo();

        // Performance: 画面の速さの記録。
        let perf = gtk::Box::new(gtk::Orientation::Vertical, 8);
        margin(&perf);
        let (scrolled, perf_list) = list();
        perf_list.add_css_class("zebra");
        // 列の見出しは一覧の上に置き、流れない（macOS 版の表と同じ）。
        let (perf_header, _) = column_header(&[
            (tr!("Item").as_str(), 0),
            (tr!("Count").as_str(), 60),
            ("p50 ms", 70),
            ("p95 ms", 70),
            (tr!("Max ms").as_str(), 70),
            (tr!("Last ms").as_str(), 70),
        ]);
        perf.append(&perf_header);
        perf.append(&scrolled);
        perf.append(&label(
            &tr!("The last 500 of each since the app started. Orange when p95 exceeds twice a frame (16.7 ms)."),
            &["dim-label", "caption"],
        ));
        stack.add_titled(&perf, Some("performance"), &tr!("Performance"));
        let refresh_perf = {
            let perf_list = perf_list.clone();
            let last = std::cell::Cell::new(0);
            move || {
                let metrics: Vec<(String, Vec<f64>)> = PERFORMANCE.with(|p| {
                    p.borrow()
                        .iter()
                        .map(|(k, v)| (k.clone(), v.iter().copied().collect()))
                        .collect()
                });
                if unchanged(&last, format!("{metrics:?}")) {
                    return;
                }
                let mut metrics = metrics;
                perf_list.remove_all();
                metrics.sort_by(|a, b| a.0.cmp(&b.0));
                for (name, values) in metrics {
                    let last = *values.last().unwrap_or(&0.0);
                    let mut sorted = values.clone();
                    sorted.sort_by(f64::total_cmp);
                    let at = |p: f64| sorted[quantile_index(sorted.len(), p)];
                    let p95 = at(0.95);
                    let slow: &[&str] = if p95 > 16.7 {
                        &["activity-warning", "numeric"]
                    } else {
                        &["numeric"]
                    };
                    perf_list.append(&row(&[
                        (&name, 0, &[]),
                        (&values.len().to_string(), 60, &["numeric"]),
                        (&ms(at(0.5)), 70, &["numeric"]),
                        (&ms(p95), 70, slow),
                        (&ms(at(1.0)), 70, &["numeric"]),
                        (&ms(last), 70, &["numeric"]),
                    ]));
                }
            }
        };
        refreshers.push(Box::new(refresh_perf));

        // Events: 内部の出来事。
        let events = gtk::Box::new(gtk::Orientation::Vertical, 8);
        margin(&events);
        let (scrolled, event_list) = list();
        event_list.add_css_class("zebra");
        let (event_header, event_header_line) = column_header(&[
            (tr!("Time").as_str(), TIME),
            (tr!("Repository").as_str(), REPOSITORY),
            (tr!("Content").as_str(), 0),
        ]);
        events.append(&event_header);
        events.append(&scrolled);
        events.append(&label(
            &tr!("File change notices and the reloads they cause (newest first, up to 500)."),
            &["dim-label", "caption"],
        ));
        stack.add_titled(&events, Some("events"), &tr!("Events"));
        let refresh_events = {
            let event_list = event_list.clone();
            let header_line = event_header_line.clone();
            let last = std::cell::Cell::new(0);
            move || {
                let items: Vec<(i64, String, String)> =
                    EVENTS.with(|e| e.borrow().iter().rev().cloned().collect());
                if unchanged(&last, &items) {
                    return;
                }
                event_list.remove_all();
                let repos: Vec<&str> = items.iter().map(|(_, repo, _)| repo.as_str()).collect();
                let name_width = repository_width(&event_list, &repos);
                set_column_width(&header_line, 1, name_width);
                for (t, repo, text) in &items {
                    event_list.append(&row(&[
                        (&time(*t), TIME, &["numeric", "dim-label"]),
                        (repo, name_width, &[]),
                        (text, 0, &[]),
                    ]));
                }
            }
        };
        refreshers.push(Box::new(refresh_events));

        // Daemon: 共有の daemon の様子と、起動・停止。
        let daemon = gtk::Box::new(gtk::Orientation::Vertical, 8);
        margin(&daemon);
        // 見出しの前の丸: 動いていれば塗った緑、そうでなければ（調べている間も）白抜きの薄い色（macOS 版の
        // circle.fill・circle の Label）。
        let state_mark = label("○", &["heading", "dim-label"]);
        let state = label(&tr!("Checking…"), &["heading", "dim-label"]);
        let state_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        state_row.append(&state_mark);
        state_row.append(&state);
        // 動いていなければ（調べている間も）、ひとりでに起動することの説明。動いていれば pid などの行。
        let not_running = tr!(
            "It starts automatically when you open a repository through the daemon (right-click in the repository list, or Settings > Performance)."
        );
        let detail = label(&not_running, &["caption", "dim-label"]);
        detail.set_wrap(true);
        // 選べるのは動いているときのソケットの行だけ（macOS 版と同じ）。選べるラベルは最初のフォーカスを取って全体を
        // 選んだ形で出るので、フォーカスは取らない（マウスで選んでコピーはできる）。
        let socket = label("", &["caption", "dim-label"]);
        selectable(&socket, true);
        socket.set_visible(false);
        let toggle = gtk::Button::with_label(&tr!("Start"));
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let texts = gtk::Box::new(gtk::Orientation::Vertical, 2);
        texts.set_hexpand(true);
        texts.append(&state_row);
        texts.append(&detail);
        texts.append(&socket);
        top.append(&texts);
        top.append(&toggle);
        let (scrolled, repo_list) = list();
        repo_list.add_css_class("zebra");
        daemon.append(&top);
        daemon.append(&label(&tr!("Open Repositories"), &["heading"]));
        // 列の見出しは一覧の上に置き、流れない。daemon が動いていなくても出す（macOS 版の表と同じ）。
        let (repo_header, _) = column_header(&[
            (tr!("Repository").as_str(), 0),
            ("SSH", 150),
            (tr!("Connections").as_str(), 150),
            ("History", 70),
            (tr!("Watching").as_str(), 70),
            (tr!("Search Index").as_str(), 170),
        ]);
        daemon.append(&repo_header);
        daemon.append(&scrolled);
        // このアプリから daemon 越しに開いているリポジトリ（macOS 版と同じ列）。
        let (scrolled, conn_list) = list();
        conn_list.add_css_class("zebra");
        scrolled.set_min_content_height(80);
        daemon.append(&label(
            &tr!("This App’s Connections to the Daemon"),
            &["heading"],
        ));
        let (conn_header, _) = column_header(&[
            (tr!("Repository").as_str(), 0),
            (tr!("Location").as_str(), 190),
            (tr!("Status").as_str(), 110),
            (tr!("Sent · Received").as_str(), 160),
            (tr!("Engine on the SSH Host").as_str(), 240),
        ]);
        daemon.append(&conn_header);
        daemon.append(&scrolled);
        {
            let (conn_list, entries) = (conn_list.clone(), entries.clone());
            let last = std::cell::Cell::new(0);
            refreshers.push(Box::new(move || {
                let rows: Vec<ConnectionRow> = entries
                    .borrow()
                    .iter()
                    .filter_map(|e| {
                        let state = e.repo.connection_state()?;
                        Some((
                            e.name.clone(),
                            location_kind(&e.location),
                            state.closed,
                            (state.sent, state.received),
                            state.engine.unwrap_or_default(),
                        ))
                    })
                    .collect();
                if unchanged(&last, &rows) {
                    return;
                }
                conn_list.remove_all();
                for (name, kind, closed, traffic, engine) in rows {
                    let (status, status_css): (String, &[&str]) = match &closed {
                        Some(_) => (tr!("Disconnected"), &["activity-error"]),
                        None => (tr!("Connected"), &["activity-success"]),
                    };
                    let traffic = format!(
                        "{} · {}",
                        glib::format_size(traffic.0),
                        glib::format_size(traffic.1)
                    );
                    let engine_text = if engine.is_empty() { "—" } else { &engine };
                    let line = row(&[
                        (&name, 0, &[]),
                        (&kind, 190, &["dim-label"]),
                        (&status, 110, status_css),
                        (&traffic, 160, &["numeric"]),
                        (engine_text, 240, &["dim-label"]),
                    ]);
                    // エンジンのパスは頭を切る（macOS 版と同じ）。
                    ellipsize_start(&line, 4);
                    // 切れた理由と、SSH 先のエンジンの全文は説明に。
                    let mut cell = line.first_child();
                    let mut i = 0;
                    while let Some(c) = cell {
                        match i {
                            2 => c.set_tooltip_text(closed.as_deref()),
                            4 if !engine.is_empty() => c.set_tooltip_text(Some(&engine)),
                            _ => {}
                        }
                        cell = c.next_sibling();
                        i += 1;
                    }
                    conn_list.append(&line);
                }
            }));
        }
        // だれもつながらなくなって終わるまでの時間（macOS 版と同じ）。
        let idle_exit = label("", &["dim-label", "caption"]);
        idle_exit.set_wrap(true);
        daemon.append(&idle_exit);
        stack.add_titled(&daemon, Some("daemon"), "Daemon");
        let running = Rc::new(std::cell::Cell::new(false));
        // 0.5 秒ごとの呼び出しの数（4 回に 1 回読む）。起動・停止の後は 0 に戻して、すぐに読ませる。
        let daemon_tick = Rc::new(std::cell::Cell::new(0u32));
        let refresh_daemon = {
            let tick = daemon_tick.clone();
            let (state, detail, toggle, repo_list, running, idle_exit) = (
                state.clone(),
                detail.clone(),
                toggle.clone(),
                repo_list.clone(),
                running.clone(),
                idle_exit.clone(),
            );
            let (state_mark, socket, not_running) =
                (state_mark.clone(), socket.clone(), not_running.clone());
            // 2 秒ごとに、前の問い合わせが返ってから（macOS 版と同じ。遅いソケットで積み重ねない）。
            let loading = Rc::new(std::cell::Cell::new(false));
            let last = Rc::new(std::cell::Cell::new(0));
            move || {
                tick.set(tick.get() + 1);
                if loading.get() || tick.get() % 4 != 1 {
                    return;
                }
                loading.set(true);
                let (state, detail, toggle, repo_list, running, loading, last, idle_exit) = (
                    state.clone(),
                    detail.clone(),
                    toggle.clone(),
                    repo_list.clone(),
                    running.clone(),
                    loading.clone(),
                    last.clone(),
                    idle_exit.clone(),
                );
                let (state_mark, socket, not_running) =
                    (state_mark.clone(), socket.clone(), not_running.clone());
                glib::spawn_future_local(async move {
                    let status = gio::spawn_blocking(|| {
                        amonhen_remote::local::control("status")
                            .ok()
                            .flatten()
                            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                    })
                    .await
                    .ok()
                    .flatten();
                    loading.set(false);
                    running.set(status.is_some());
                    // 一覧は、中身が変わったときだけ作り直す（ちらつかせず、スクロールの位置も保つ）。
                    idle_exit.set_text(&tr!(
                        "The daemon exits after {duration} with no connections. From a terminal, run amonhen daemon status to see it.",
                        duration = duration(
                            status
                                .as_ref()
                                .and_then(|d| d["idleExitMs"].as_u64())
                                .unwrap_or(600_000)
                        )
                    ));
                    let repos = status.as_ref().map(|d| d["repos"].to_string());
                    let same_repos = unchanged(&last, &repos);
                    if !same_repos {
                        repo_list.remove_all();
                    }
                    let shown = status.is_some();
                    state_mark.set_text(if shown { "●" } else { "○" });
                    for l in [&state_mark, &state] {
                        if shown {
                            l.remove_css_class("dim-label");
                            l.add_css_class("activity-success");
                        } else {
                            l.remove_css_class("activity-success");
                            l.add_css_class("dim-label");
                        }
                    }
                    if shown {
                        detail.remove_css_class("dim-label");
                        detail.add_css_class("numeric");
                    } else {
                        detail.remove_css_class("numeric");
                        detail.add_css_class("dim-label");
                    }
                    socket.set_visible(shown);
                    let Some(d) = status else {
                        state.set_text(&tr!("The shared daemon isn’t running"));
                        detail.set_text(&not_running);
                        toggle.set_label(&tr!("Start"));
                        toggle.set_tooltip_text(None);
                        return;
                    };
                    state.set_text(&tr!("The shared daemon is running"));
                    detail.set_text(&tr!(
                        "pid {pid} · version {version} (build {build}) · up {uptime} · connections {connections}",
                        pid = d["pid"],
                        version = d["version"].as_str().unwrap_or_default(),
                        build = d["build"].as_str().unwrap_or_default(),
                        uptime = duration(d["uptimeMs"].as_u64().unwrap_or(0)),
                        connections = d["connections"],
                    ));
                    socket.set_text(d["socket"].as_str().unwrap_or_default());
                    toggle.set_label(&tr!("Stop"));
                    toggle.set_tooltip_text(Some(&tr!(
                        "Repositories opened through the daemon get disconnected (Reconnect starts it again)."
                    )));
                    if same_repos {
                        return;
                    }
                    for r in d["repos"].as_array().into_iter().flatten() {
                        let yes = |key: &str| {
                            if r[key].as_bool() == Some(true) {
                                tr!("Yes")
                            } else {
                                "—".to_owned()
                            }
                        };
                        let clients = r["clients"].as_u64().unwrap_or(0);
                        let clients = if clients > 0 {
                            clients.to_string()
                        } else {
                            tr!(
                                "None (for {duration})",
                                duration = duration(r["idleMs"].as_u64().unwrap_or(0))
                            )
                        };
                        // SSH 先: 送った・受け取った量（切れていれば「切れた」と理由）。
                        let bytes = |key: &str| glib::format_size(r[key].as_u64().unwrap_or(0));
                        let (ssh, ssh_css, ssh_tip): (String, &[&str], Option<String>) =
                            if r["ssh"].as_bool() != Some(true) {
                                ("—".to_owned(), &["dim-label"], None)
                            } else if let Some(reason) = r["closed"].as_str() {
                                (
                                    tr!("Disconnected"),
                                    &["activity-error"],
                                    Some(reason.to_owned()),
                                )
                            } else {
                                let mut tip = tr!("Sent to and received from the SSH host");
                                if let Some(engine) = r["engine"].as_str() {
                                    tip.push_str(&format!(
                                        "\n{}",
                                        tr!("Engine on the SSH host: {engine}", engine = engine)
                                    ));
                                }
                                (
                                    format!("{} · {}", bytes("sent"), bytes("received")),
                                    &["numeric"],
                                    Some(tip),
                                )
                            };
                        let index = match r["indexRows"].as_u64() {
                            Some(rows) => tr!(
                                "{rows} rows · {size}",
                                rows = rows,
                                size = bytes("indexBytes")
                            ),
                            None => "—".to_owned(),
                        };
                        let path = r["path"].as_str().unwrap_or_default();
                        let line = row(&[
                            (path, 0, &[]),
                            (&ssh, 150, ssh_css),
                            (&clients, 150, &["numeric"]),
                            (&yes("history"), 70, &[]),
                            (&yes("watching"), 70, &[]),
                            (&index, 170, &["numeric"]),
                        ]);
                        line.set_tooltip_text(Some(path));
                        // パスは頭を切る（終わりの名前が見える。macOS 版と同じ）。
                        ellipsize_start(&line, 0);
                        if let (Some(tip), Some(cell)) =
                            (ssh_tip, line.first_child().and_then(|c| c.next_sibling()))
                        {
                            cell.set_tooltip_text(Some(&tip));
                        }
                        repo_list.append(&line);
                    }
                });
            }
        };
        let refresh_daemon = Rc::new(refresh_daemon);
        let r = refresh_daemon.clone();
        refreshers.push(Box::new(move || r()));
        let w = window.downgrade();
        toggle.connect_clicked(move |_| {
            let (running, engine, w) = (running.get(), engine.clone(), w.clone());
            let (refresh_daemon, tick) = (refresh_daemon.clone(), daemon_tick.clone());
            glib::spawn_future_local(async move {
                let result = gio::spawn_blocking(move || {
                    if running {
                        let _ = amonhen_remote::local::control("stop");
                        Ok(())
                    } else {
                        amonhen_remote::local::start(&engine.to_string_lossy())
                            .map_err(|e| e.to_string())
                    }
                })
                .await;
                // 2 秒待たずに、すぐに様子を読み直す（macOS 版の loadDaemon(force: true)）。
                tick.set(0);
                refresh_daemon();
                // 起動できなければ、エラーの文そのものを見出しにして知らせる（macOS 版の NSAlert(error:)）。
                if let Ok(Err(e)) = result {
                    gtk::AlertDialog::builder()
                        .message(e)
                        .modal(true)
                        .build()
                        .show(w.upgrade().as_ref());
                }
            });
        });

        // すべてのタブを読み直す（出すたびと、出している間は 0.5 秒ごと。`start`）。
        let refresh_all: Rc<dyn Fn()> = Rc::new(move || {
            for f in refreshers.iter() {
                f();
            }
        });
        let r = refresh_undo.clone();
        stack.connect_visible_child_name_notify(move |s| {
            if s.visible_child_name().as_deref() == Some("history") {
                r();
            }
        });
        // 操作の後と、同じタブで書き込む先の worktree を替えたら読み直す（付け替えた先の操作でも。同じものには
        // 1 度だけつなぐ）。
        let connected: Rc<RefCell<Vec<std::rc::Weak<Actions>>>> = Rc::default();
        let connect_after = {
            let r = refresh_undo.clone();
            move |actions: &Rc<Actions>| {
                let mut c = connected.borrow_mut();
                c.retain(|w| w.strong_count() > 0);
                if c.iter().any(|w| w.as_ptr() == Rc::as_ptr(actions)) {
                    return;
                }
                c.push(Rc::downgrade(actions));
                let r1 = r.clone();
                actions.connect_after(move || r1());
                let r = r.clone();
                actions.connect_repo_changed(move || r());
            }
        };
        // 選び直したら、その操作の記録を読み、その後の操作でも読み直す。
        on_selected.replace(Some({
            let (selected, r) = (selected.clone(), refresh_undo);
            Rc::new(move || {
                let actions = selected.borrow().as_ref().map(|e| e.actions.clone());
                if let Some(actions) = actions {
                    connect_after(&actions);
                }
                r();
            })
        }));
        if let Some(f) = on_selected.borrow().clone() {
            f();
        }
        Self {
            window,
            stack,
            refresh: refresh_all,
            ticking: Rc::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// p50・p95 の位置は macOS 版と同じ（`len × p` を切り捨て）。
    #[test]
    fn quantiles_match_macos() {
        assert_eq!(quantile_index(1, 0.95), 0);
        assert_eq!(quantile_index(2, 0.5), 1);
        assert_eq!(quantile_index(10, 0.5), 5);
        assert_eq!(quantile_index(10, 0.95), 9);
        assert_eq!(quantile_index(100, 0.95), 95);
        assert_eq!(quantile_index(10, 1.0), 9);
    }

    /// 時間は macOS 版と同じく、秒・分・時間で。
    #[test]
    fn durations_use_seconds_minutes_and_hours() {
        amonhen_model::i18n::with(amonhen_model::i18n::Lang::En, || {
            assert_eq!(duration(5_000), "5 s");
            assert_eq!(duration(600_000), "10 min");
            assert_eq!(duration(125_000), "2 min 5 s");
            assert_eq!(duration(7_380_000), "2 h 3 min");
        });
    }
}
