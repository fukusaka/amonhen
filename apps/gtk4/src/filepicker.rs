//! ファイルやフォルダを選ぶシート（macOS 版の `FilePicker.swift`）。この機械のものも、SSH 先のものも同じ形で選ぶ。
//! 上にパスの欄（打ち込める。`~` はその機械のホーム）、下に根（`/`・`C:\`）から選んだところまでの列。中身はコアが
//! 読む（この機械は `amonhen_backend_cli::list_directory`、SSH 先はそのエンジンの `listDirectory`）。リポジトリは
//! 緑で出し、選べないもの（フォルダを選ぶときのファイル、プログラムを選ぶときの実行できないファイル）は薄く出す。
//! 列は ↑↓ で選び、→ で選んだフォルダに入り、← で上の列に戻り、Enter で決める。列は見えている行だけ部品を作る。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use amonhen_core::Repository;
use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gio, glib};
use serde_json::{Value, json};

/// 何を選ぶか。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 実行できるファイル。
    Executable,
    /// フォルダ（開いているフォルダを選ぶ）。
    Folder,
    /// ファイル。
    File,
    /// リポジトリ（`.git` のあるフォルダ。開かずに選ぶ）。
    Repository,
}

/// どこのものを選ぶか。
#[derive(Clone)]
pub enum Source {
    /// この機械。
    Local,
    /// SSH 先（エンジンにつなぐ。数秒かかる）。
    Host(String),
    /// つないであるリポジトリの機械（SSH 先の名前と、そのリポジトリ）。
    Connected(String, Arc<Repository>),
    /// SSH 先を選べる（最初のホスト。空でもよい）。
    AnyHost(String),
}

/// 選んだもの: SSH 先（この機械なら空）と、そこでの絶対パス、ホームの下なら `~/…` にしたもの。
#[derive(Clone, Debug)]
pub struct Chosen {
    pub host: String,
    pub path: String,
    pub shown: String,
}

impl Chosen {
    /// SSH 先のフォルダの場所（`ssh://host/~/src`・`ssh://host/C:/src`。Places と同じ書き方）。
    pub fn ssh_location(&self) -> String {
        let path = self.shown.replace('\\', "/");
        let path = if path.starts_with('/') {
            path
        } else {
            format!("/{path}")
        };
        format!("ssh://{}{path}", self.host)
    }
}

/// シートに出すもの。
pub struct Request {
    pub title: String,
    pub message: String,
    pub kind: Kind,
    /// 最初に出すところ（`~` 可。ファイルならそれを選んだところ。空ならホーム）。
    pub initial: String,
    /// すぐ行けるフォルダ（`/usr/local/bin` など、ふだん見えない所）。
    pub places: Vec<String>,
}

/// SSH 先でプログラムがふだんある所。
pub fn host_program_places() -> Vec<String> {
    ["/usr/local/bin", "/usr/bin", "~/.local/bin", "~/bin"]
        .map(str::to_owned)
        .to_vec()
}

/// この機械でプログラムがふだんある所（あるものだけ）。
pub fn local_program_places() -> Vec<String> {
    [
        "/usr/local/bin",
        "/usr/bin",
        "/opt/homebrew/bin",
        "~/.local/bin",
    ]
    .into_iter()
    .filter(|p| crate::bookmarks::expand(p).is_dir())
    .map(str::to_owned)
    .collect()
}

// パス（その機械の形のまま。Windows なら `C:\…`・`\\server\share\…`）

fn is_windows(path: &str) -> bool {
    let b = path.as_bytes();
    path.starts_with("\\\\") || (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':')
}

fn separator(path: &str) -> char {
    if is_windows(path) { '\\' } else { '/' }
}

/// 根（`/`・`C:\`・`\\server\share\`）。
fn root_of(path: &str) -> String {
    if let Some(unc) = path.strip_prefix("\\\\") {
        let parts: Vec<&str> = unc.split('\\').take(2).collect();
        return format!("\\\\{}\\", parts.join("\\"));
    }
    if is_windows(path) {
        return format!("{}\\", &path[..2]);
    }
    "/".to_owned()
}

fn join(dir: &str, name: &str) -> String {
    let sep = separator(dir);
    if dir.ends_with(sep) {
        format!("{dir}{name}")
    } else {
        format!("{dir}{sep}{name}")
    }
}

/// 根から `path` の上のフォルダまで（`path` は含まない）。
fn ancestors(path: &str) -> Vec<String> {
    let root = root_of(path);
    let sep = separator(path);
    let rest = path[root.len().min(path.len())..].trim_end_matches(sep);
    if rest.is_empty() {
        return Vec::new();
    }
    let parts: Vec<&str> = rest.split(sep).filter(|p| !p.is_empty()).collect();
    let mut out = vec![root.clone()];
    let mut current = root;
    for part in &parts[..parts.len() - 1] {
        current = join(&current, part);
        out.push(current.clone());
    }
    out
}

/// 上のフォルダと名前（根なら None）。`~/x` も分ける。
fn split(path: &str) -> Option<(String, String)> {
    let sep = if path.contains('\\') && !path.contains('/') {
        '\\'
    } else {
        '/'
    };
    let trimmed = path.trim_end_matches(sep);
    let i = trimmed.rfind(sep)?;
    let name = trimmed[i + 1..].to_owned();
    if name.is_empty() || name == "~" {
        return None;
    }
    let parent = if i == 0 {
        sep.to_string()
    } else if trimmed[..i].ends_with(':') {
        format!("{}{sep}", &trimmed[..i])
    } else {
        trimmed[..i].to_owned()
    };
    Some((parent, name))
}

/// ホームの下なら `~/…`。
fn shown(path: &str, home: Option<&str>) -> String {
    let Some(home) = home.filter(|h| !h.is_empty()) else {
        return path.to_owned();
    };
    let sep = separator(path);
    if path == home {
        "~".to_owned()
    } else if let Some(rest) = path.strip_prefix(&format!("{}{sep}", home.trim_end_matches(sep))) {
        format!("~{sep}{rest}")
    } else {
        path.to_owned()
    }
}

fn is_absolute(path: &str) -> bool {
    path.starts_with('/') || path.starts_with('~') || is_windows(path)
}

// 読む

/// フォルダの中の 1 つ。
#[derive(Clone, Debug)]
struct Entry {
    name: String,
    folder: bool,
    file: bool,
    link: bool,
    executable: bool,
    repository: bool,
}

/// フォルダの中身。
struct Dir {
    path: String,
    home: Option<String>,
    entries: Vec<Entry>,
}

/// 中身を読む所: この機械か、SSH 先のエンジン（つないだリポジトリ）。
#[derive(Clone)]
enum Reader {
    Local,
    Engine(Arc<Repository>),
}

fn parse(info: &Value) -> Dir {
    Dir {
        path: info["path"].as_str().unwrap_or_default().to_owned(),
        home: info["home"].as_str().map(str::to_owned),
        entries: info["entries"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|e| Entry {
                        name: e["name"].as_str().unwrap_or_default().to_owned(),
                        folder: e["kind"] == "folder",
                        file: e["kind"] == "file",
                        link: e["link"].as_bool().unwrap_or(false),
                        executable: e["executable"].as_bool().unwrap_or(false),
                        repository: e["repository"].as_bool().unwrap_or(false),
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// フォルダの中身を読む（裏の糸で呼ぶ）。
fn list(reader: &Reader, path: &str) -> Result<Dir, String> {
    match reader {
        Reader::Local => {
            let dir = amonhen_backend_cli::list_directory(path).map_err(|e| e.to_string())?;
            Ok(Dir {
                path: dir.path,
                home: dir.home,
                entries: dir
                    .entries
                    .into_iter()
                    .map(|e| Entry {
                        folder: e.kind == "folder",
                        file: e.kind == "file",
                        name: e.name,
                        link: e.link,
                        executable: e.executable,
                        repository: e.repository,
                    })
                    .collect(),
            })
        }
        Reader::Engine(repo) => {
            crate::reposettings::call(repo, &json!({ "op": "listDirectory", "path": path }))
                .map(|info| parse(&info))
        }
    }
}

// シート

/// 1 つの列: フォルダと、その中で選んでいるもの。
struct Column {
    path: String,
    entries: Option<Vec<Entry>>,
    error: Option<String>,
    selected: Option<String>,
}

impl Column {
    fn new(path: String) -> Self {
        Self {
            path,
            entries: None,
            error: None,
            selected: None,
        }
    }
}

/// 列の部品（見えている行だけ部品を作る）。
struct ColumnView {
    widget: gtk::Box,
    list: gtk::ListView,
    model: gtk::StringList,
    selection: gtk::SingleSelection,
    /// 出している中身（行の順）。
    shown: Rc<RefCell<Vec<Entry>>>,
    note: gtk::Label,
}

struct Picker {
    kind: Kind,
    window: gtk::Window,
    host_entry: Option<gtk::Entry>,
    field: gtk::Entry,
    home_button: gtk::Button,
    places_button: Option<gtk::MenuButton>,
    hidden: gtk::CheckButton,
    columns_box: gtk::Box,
    columns_scroll: gtk::ScrolledWindow,
    status: gtk::Label,
    /// 最初に列ができたら、列にキーボードを移す（macOS 版と同じく、始めは列で選ぶ。SSH 先を選べるときは欄のまま）。
    focus_columns: Cell<bool>,
    /// 列で打った名前と、最後に打った時刻（1 秒あけば打ち直し。macOS 版の `typeSelect`）。
    typed: RefCell<(String, Option<std::time::Instant>)>,
    /// つなげなかったときの「接続を確認」（もう一度つなぐ。macOS 版と同じ）。
    retry: gtk::Button,
    spinner: gtk::Spinner,
    choose: gtk::Button,
    views: RefCell<Vec<Rc<ColumnView>>>,
    columns: RefCell<Vec<Column>>,
    reader: RefCell<Option<Reader>>,
    host: RefCell<String>,
    home: RefCell<Option<String>>,
    connection_error: RefCell<Option<String>>,
    initial: String,
    /// 今の移動・つなぎ（古い答えは捨てる）。
    generation: Cell<u64>,
    attempt: Cell<u64>,
    /// 部品の選びを入れ直している間（選びの知らせを無視する）。
    filling: Cell<bool>,
    /// → で入ったが、まだ読んでいる列のパス（読めたら最初のものを選ぶ）。
    entering: RefCell<Option<String>>,
    tx: async_channel::Sender<Option<Chosen>>,
}

/// シートを出して、選んだものを返す（取り消したら None）。
pub async fn choose(
    parent: Option<&gtk::Window>,
    request: Request,
    source: Source,
) -> Option<Chosen> {
    let (tx, rx) = async_channel::bounded(1);
    let picker = Picker::new(parent, &request, &source, tx);
    match source {
        Source::Local => picker.use_reader(Reader::Local, ""),
        Source::Connected(host, repo) => picker.use_reader(Reader::Engine(repo), &host),
        Source::Host(host) | Source::AnyHost(host) => picker.connect(&host),
    }
    picker.window.present();
    // 列ができるまでは欄に（列ができたら列へ。`sync`）。
    picker.field.grab_focus();
    debug_snapshot(&picker);
    // シートが閉じるまで持つ。
    let result = rx.recv().await.ok().flatten();
    drop(picker);
    result
}

impl Picker {
    fn new(
        parent: Option<&gtk::Window>,
        request: &Request,
        source: &Source,
        tx: async_channel::Sender<Option<Chosen>>,
    ) -> Rc<Self> {
        let heading = gtk::Label::new(Some(&request.title));
        heading.add_css_class("title-3");
        heading.set_xalign(0.0);
        let message = gtk::Label::new(Some(&request.message));
        message.set_xalign(0.0);
        message.set_wrap(true);
        message.add_css_class("dim-label");
        message.set_visible(!request.message.is_empty());
        let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
        content.set_margin_top(16);
        content.set_margin_bottom(16);
        content.set_margin_start(18);
        content.set_margin_end(18);
        content.append(&heading);
        content.append(&message);

        // SSH 先を選べるなら、その欄（候補は使ったことのあるものと ~/.ssh/config・known_hosts）。
        let host_entry = matches!(source, Source::AnyHost(_)).then(|| {
            let entry = gtk::Entry::new();
            entry.set_hexpand(true);
            entry.set_placeholder_text(Some(&tr!("user@host:port (or a name in ~/.ssh/config)")));
            if let Source::AnyHost(h) = source {
                entry.set_text(h);
            }
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            row.append(&gtk::Label::new(Some(&tr!("Host:"))));
            row.append(&entry);
            let candidates = host_candidates();
            if !candidates.is_empty() {
                let menu = gio::Menu::new();
                for h in &candidates {
                    let item = gio::MenuItem::new(Some(h), None);
                    item.set_action_and_target_value(Some("picker.host"), Some(&h.to_variant()));
                    menu.append_item(&item);
                }
                let more = gtk::MenuButton::builder().menu_model(&menu).build();
                row.append(&more);
            }
            content.append(&row);
            entry
        });

        let field = gtk::Entry::new();
        field.set_hexpand(true);
        field.add_css_class("monospace");
        field.set_placeholder_text(Some(if request.kind == Kind::Executable {
            "/usr/local/bin/git"
        } else {
            "~/prj"
        }));
        let home_button = gtk::Button::with_label("~");
        home_button.set_tooltip_text(Some(&tr!("Home folder")));
        let spinner = gtk::Spinner::new();
        let path_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        path_row.append(&field);
        path_row.append(&home_button);
        let places_button = (!request.places.is_empty()).then(|| {
            let menu = gio::Menu::new();
            for p in &request.places {
                let item = gio::MenuItem::new(Some(p), None);
                item.set_action_and_target_value(Some("picker.go"), Some(&p.to_variant()));
                menu.append_item(&item);
            }
            let b = gtk::MenuButton::builder()
                .menu_model(&menu)
                .tooltip_text(tr!("Go to a folder"))
                .build();
            path_row.append(&b);
            b
        });
        path_row.append(&spinner);
        content.append(&path_row);

        let columns_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let columns_scroll = gtk::ScrolledWindow::builder()
            .child(&columns_box)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .hexpand(true)
            .build();
        columns_scroll.add_css_class("frame");
        content.append(&columns_scroll);
        let status = gtk::Label::new(None);
        status.set_xalign(0.0);
        status.set_wrap(true);
        status.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        status.set_selectable(true);
        status.add_css_class("caption");
        status.set_visible(false);
        content.append(&status);
        let retry = gtk::Button::with_label(&tr!("Check Connection"));
        retry.set_halign(gtk::Align::Start);
        retry.set_visible(false);
        content.append(&retry);

        let hidden = gtk::CheckButton::with_label(&tr!("Show hidden files"));
        hidden.set_hexpand(true);
        let cancel = gtk::Button::with_label(&tr!("Cancel"));
        let choose = gtk::Button::with_label(&tr!("Choose"));
        choose.add_css_class("suggested-action");
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        buttons.append(&hidden);
        buttons.append(&cancel);
        buttons.append(&choose);
        content.append(&buttons);

        let window = gtk::Window::builder()
            .modal(true)
            .resizable(true)
            .default_width(760)
            .default_height(if host_entry.is_some() { 500 } else { 460 })
            .title(&request.title)
            .child(&content)
            .build();
        window.set_size_request(520, 320);
        // 見出しの帯は出さない（シートの形）。
        window.set_titlebar(Some(&gtk::Box::new(gtk::Orientation::Horizontal, 0)));
        window.set_transient_for(parent);

        let initial = if request.initial.trim().is_empty() {
            "~".to_owned()
        } else {
            request.initial.trim().to_owned()
        };
        field.set_text(&initial);
        let picker = Rc::new(Self {
            kind: request.kind,
            window,
            host_entry,
            field,
            home_button,
            places_button,
            hidden,
            columns_box,
            columns_scroll,
            status,
            focus_columns: Cell::new(!matches!(source, Source::AnyHost(_))),
            typed: RefCell::default(),
            retry,
            spinner,
            choose,
            views: RefCell::default(),
            columns: RefCell::default(),
            reader: RefCell::default(),
            host: RefCell::default(),
            home: RefCell::default(),
            connection_error: RefCell::default(),
            initial,
            generation: Cell::new(0),
            attempt: Cell::new(0),
            filling: Cell::new(false),
            entering: RefCell::default(),
            tx,
        });
        picker.wire(&cancel);
        picker.update_buttons();
        picker
    }

    fn wire(self: &Rc<Self>, cancel: &gtk::Button) {
        let w = Rc::downgrade(self);
        self.retry.connect_clicked(move |_| {
            if let Some(p) = w.upgrade() {
                let host = p.host.borrow().clone();
                p.connect(&host);
            }
        });
        let group = gio::SimpleActionGroup::new();
        let go = gio::SimpleAction::new("go", Some(glib::VariantTy::STRING));
        let w = Rc::downgrade(self);
        go.connect_activate(move |_, v| {
            if let (Some(p), Some(path)) = (w.upgrade(), v.and_then(|v| v.get::<String>())) {
                p.go(&path, false);
            }
        });
        group.add_action(&go);
        let host = gio::SimpleAction::new("host", Some(glib::VariantTy::STRING));
        let w = Rc::downgrade(self);
        host.connect_activate(move |_, v| {
            if let (Some(p), Some(h)) = (w.upgrade(), v.and_then(|v| v.get::<String>())) {
                if let Some(e) = &p.host_entry {
                    e.set_text(&h);
                }
                p.connect(&h);
            }
        });
        group.add_action(&host);
        self.window.insert_action_group("picker", Some(&group));

        if let Some(entry) = &self.host_entry {
            let w = Rc::downgrade(self);
            entry.connect_activate(move |e| {
                if let Some(p) = w.upgrade() {
                    p.connect(&e.text());
                }
            });
        }
        let w = Rc::downgrade(self);
        self.field.connect_activate(move |f| {
            // 打ち込んだパスへ行く（つながらなかったときも。決めるのは Choose。macOS 版と同じ）。
            if let Some(p) = w.upgrade() {
                p.go(&f.text(), false);
            }
        });
        let w = Rc::downgrade(self);
        self.field.connect_changed(move |_| {
            if let Some(p) = w.upgrade() {
                p.update_buttons();
            }
        });
        let w = Rc::downgrade(self);
        self.home_button.connect_clicked(move |_| {
            if let Some(p) = w.upgrade() {
                p.go("~", false);
            }
        });
        let w = Rc::downgrade(self);
        self.hidden.connect_toggled(move |_| {
            if let Some(p) = w.upgrade() {
                for i in 0..p.views.borrow().len() {
                    p.fill(i);
                }
            }
        });
        let w = self.window.downgrade();
        cancel.connect_clicked(move |_| {
            if let Some(w) = w.upgrade() {
                w.close();
            }
        });
        let w = Rc::downgrade(self);
        self.choose.connect_clicked(move |_| {
            if let Some(p) = w.upgrade() {
                p.finish();
            }
        });
        let tx = self.tx.clone();
        self.window.connect_close_request(move |_| {
            let _ = tx.try_send(None);
            glib::Propagation::Proceed
        });
        // Esc で閉じる。Ctrl+Shift+. で隠しファイル。
        let keys = gtk::EventControllerKey::new();
        let w = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            let Some(p) = w.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if key == gtk::gdk::Key::Escape {
                p.window.close();
                return glib::Propagation::Stop;
            }
            let ctrl_shift =
                gtk::gdk::ModifierType::CONTROL_MASK | gtk::gdk::ModifierType::SHIFT_MASK;
            if modifiers.contains(ctrl_shift)
                && matches!(key, gtk::gdk::Key::period | gtk::gdk::Key::greater)
            {
                p.hidden.set_active(!p.hidden.is_active());
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        self.window.add_controller(keys);
    }

    // 読む所

    fn use_reader(self: &Rc<Self>, reader: Reader, host: &str) {
        self.host.replace(host.to_owned());
        self.reader.replace(Some(reader));
        let initial = self.initial.clone();
        self.go(&initial, true);
    }

    /// `host` のエンジンにつなぐ（リポジトリなしで動かす。共有の daemon を使う設定なら daemon 越し）。
    fn connect(self: &Rc<Self>, host: &str) {
        let host = host.trim().to_owned();
        let attempt = self.attempt.get() + 1;
        self.attempt.set(attempt);
        self.host.replace(host.clone());
        self.reader.replace(None);
        self.connection_error.replace(None);
        self.columns.borrow_mut().clear();
        self.sync(0);
        if host.is_empty() || host.contains('/') || host.contains(char::is_whitespace) {
            self.set_status(Some(tr!("Choose a host to see its folders.")), false);
            self.update_buttons();
            return;
        }
        self.set_status(Some(tr!("Connecting to {host}…", host = host)), false);
        self.spinner.set_spinning(true);
        self.update_buttons();
        let through = crate::settings::bool(crate::settings::OPEN_THROUGH_DAEMON);
        let location = format!("{}ssh://{host}", if through { "daemon:" } else { "" });
        let w = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || crate::open(std::path::Path::new(&location)))
                .await
                .unwrap_or_else(|_| Err("panic".into()));
            let Some(p) = w.upgrade() else { return };
            if p.attempt.get() != attempt {
                return;
            }
            p.spinner.set_spinning(false);
            match result {
                Ok(repo) => {
                    p.set_status(None, false);
                    p.reader.replace(Some(Reader::Engine(Arc::new(repo))));
                    // つないでいる間に欄を打ち直していれば、そこへ。
                    let typed = p.field.text().to_string();
                    p.go(&typed, true);
                }
                Err(e) => {
                    p.connection_error.replace(Some(e.clone()));
                    p.set_status(
                        Some(format!(
                            "{}\n{e}\n{}",
                            tr!("Couldn’t connect to {host}", host = p.host.borrow().clone()),
                            tr!("You can still type the path above and choose it.")
                        )),
                        true,
                    );
                    p.retry.set_visible(true);
                    p.update_buttons();
                }
            }
        });
    }

    fn set_status(&self, text: Option<String>, error: bool) {
        self.retry.set_visible(false);
        match text {
            Some(t) => {
                self.status.set_text(&t);
                self.status.set_visible(true);
                if error {
                    self.status.add_css_class("error");
                } else {
                    self.status.remove_css_class("error");
                }
            }
            None => self.status.set_visible(false),
        }
    }

    // 動く

    /// 打ち込んだパスへ行く: フォルダならその中、ファイルならそれのあるフォルダでそれを選ぶ。`or_home` なら、
    /// 読めないときはホームへ（最初に出すところがもうないとき）。
    fn go(self: &Rc<Self>, typed: &str, or_home: bool) {
        let typed = if typed.trim().is_empty() {
            "~".to_owned()
        } else {
            typed.trim().to_owned()
        };
        let Some(reader) = self.reader.borrow().clone() else {
            return;
        };
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        self.spinner.set_spinning(true);
        let kind = self.kind;
        let t = typed.clone();
        let w = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                // 上のフォルダで、それを選ぶ。
                let in_parent = |path: &str| -> Option<(Dir, Option<String>)> {
                    let (parent, name) = split(path)?;
                    let dir = list(&reader, &parent).ok()?;
                    dir.entries
                        .iter()
                        .any(|e| e.name == name)
                        .then_some((dir, Some(name)))
                };
                match list(&reader, &t) {
                    Ok(dir) => {
                        // リポジトリを選ぶなら、それは開かずに選ぶ。
                        if kind == Kind::Repository
                            && dir.entries.iter().any(|e| e.name == ".git")
                            && let Some(found) = in_parent(&dir.path)
                        {
                            return Ok(found);
                        }
                        Ok((dir, None))
                    }
                    Err(e) => in_parent(&t).ok_or(e),
                }
            })
            .await
            .unwrap_or_else(|_| Err("panic".into()));
            let Some(p) = w.upgrade() else { return };
            if p.generation.get() != generation {
                return;
            }
            p.spinner.set_spinning(false);
            match result {
                Ok((dir, name)) => {
                    p.set_status(None, false);
                    p.show(dir, name);
                }
                Err(_) if or_home && typed != "~" => p.go("~", false),
                Err(e) => p.set_status(Some(e), true),
            }
        });
    }

    /// `dir` を最後の列にし、その上のフォルダを根から並べる（中身はあとで読む）。
    fn show(self: &Rc<Self>, dir: Dir, select: Option<String>) {
        if dir.home.is_some() {
            self.home.replace(dir.home.clone());
        }
        let parts = ancestors(&dir.path);
        let mut columns = Vec::new();
        for (i, path) in parts.iter().enumerate() {
            let next = parts.get(i + 1).unwrap_or(&dir.path);
            let mut c = Column::new(path.clone());
            c.selected = split(next).map(|(_, n)| n);
            columns.push(c);
        }
        let mut last = Column::new(dir.path.clone());
        last.entries = Some(dir.entries);
        last.selected = select;
        columns.push(last);
        let pending: Vec<String> = columns
            .iter()
            .filter(|c| c.entries.is_none())
            .map(|c| c.path.clone())
            .collect();
        self.columns.replace(columns);
        self.sync(0);
        self.update_field();
        for path in pending {
            self.load(&path);
        }
        self.reveal();
    }

    /// 列の中身を読む。
    fn load(self: &Rc<Self>, path: &str) {
        let Some(reader) = self.reader.borrow().clone() else {
            return;
        };
        let p2 = path.to_owned();
        let w = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let path = p2.clone();
            let result = gio::spawn_blocking(move || list(&reader, &p2))
                .await
                .unwrap_or_else(|_| Err("panic".into()));
            let Some(p) = w.upgrade() else { return };
            let index = p.columns.borrow().iter().position(|c| c.path == path);
            let Some(index) = index else { return };
            {
                let mut columns = p.columns.borrow_mut();
                match result {
                    Ok(dir) => {
                        if dir.home.is_some() {
                            p.home.replace(dir.home);
                        }
                        columns[index].entries = Some(dir.entries);
                    }
                    Err(e) => {
                        columns[index].entries = Some(Vec::new());
                        columns[index].error = Some(e);
                    }
                }
            }
            p.fill(index);
            p.update_field();
            if p.entering.borrow().as_deref() == Some(path.as_str()) {
                p.entering.replace(None);
                p.select_first(index);
            }
        });
    }

    /// 選べるものか（フォルダを選ぶならフォルダ、プログラムを選ぶなら実行できるファイルなど）。
    fn selectable(&self, e: &Entry) -> bool {
        match self.kind {
            Kind::Folder => e.folder,
            Kind::Executable => e.file && e.executable,
            Kind::File => e.file,
            Kind::Repository => e.repository,
        }
    }

    /// クリックで中を次の列に出すか（フォルダ。リポジトリを選ぶときは、それは開かずに選ぶ）。
    fn opens(&self, e: &Entry) -> bool {
        e.folder && !(self.kind == Kind::Repository && e.repository)
    }

    /// 列 `index` の `name` を選ぶ。開くフォルダなら、その中を次の列に出す。
    fn select(self: &Rc<Self>, index: usize, name: Option<String>) {
        self.generation.set(self.generation.get() + 1);
        self.entering.replace(None);
        self.spinner.set_spinning(false);
        let entry = {
            let mut columns = self.columns.borrow_mut();
            if index >= columns.len() {
                return;
            }
            columns.truncate(index + 1);
            columns[index].selected = name.clone();
            name.and_then(|n| {
                columns[index]
                    .entries
                    .as_ref()
                    .and_then(|es| es.iter().find(|e| e.name == n).cloned())
            })
        };
        let mut next = None;
        if let Some(e) = entry.filter(|e| self.opens(e)) {
            let path = join(&self.columns.borrow()[index].path, &e.name);
            self.columns.borrow_mut().push(Column::new(path.clone()));
            next = Some(path);
        }
        self.sync(index + 1);
        self.update_field();
        if let Some(path) = next {
            self.load(&path);
        }
        self.reveal();
    }

    /// → : 選んでいるフォルダに入り、その最初のものを選ぶ（まだ読んでいれば、読めたら）。
    fn enter(self: &Rc<Self>, index: usize) {
        if index + 1 >= self.views.borrow().len() {
            return;
        }
        let unread = self
            .columns
            .borrow()
            .get(index + 1)
            .filter(|c| c.entries.is_none())
            .map(|c| c.path.clone());
        if unread.is_some() {
            self.entering.replace(unread);
        } else {
            self.select_first(index + 1);
        }
        if let Some(view) = self.views.borrow().get(index + 1) {
            view.list.grab_focus();
        }
    }

    /// 列 `index` で名前を打った: 続けて打てば続きで、その名前の行を選ぶ（なければ、並びでその後にあるもの、
    /// それもなければ最後。選べる（薄くない）ものだけ。macOS 版の `typeSelect`）。
    fn type_select(self: &Rc<Self>, index: usize, c: char) {
        let key = {
            let mut typed = self.typed.borrow_mut();
            let now = std::time::Instant::now();
            if typed
                .1
                .is_none_or(|at| now.duration_since(at) > std::time::Duration::from_secs(1))
            {
                typed.0.clear();
            }
            typed.0.push(c);
            typed.1 = Some(now);
            typed.0.to_lowercase()
        };
        let Some(view) = self.views.borrow().get(index).cloned() else {
            return;
        };
        let row = {
            let shown = view.shown.borrow();
            let usable: Vec<(usize, String)> = shown
                .iter()
                .enumerate()
                .filter(|(_, e)| self.selectable(e) || self.opens(e))
                .map(|(i, e)| (i, e.name.to_lowercase()))
                .collect();
            usable
                .iter()
                .find(|(_, n)| *n >= key)
                .or(usable.last())
                .map(|(i, _)| *i)
        };
        if let Some(row) = row
            && view.selection.selected() != row as u32
        {
            view.selection.set_selected(row as u32);
            view.list
                .scroll_to(row as u32, gtk::ListScrollFlags::NONE, None);
        }
    }

    /// 列 `index` の、選べる（薄くない）最初のものを選ぶ。
    fn select_first(self: &Rc<Self>, index: usize) {
        let Some(view) = self.views.borrow().get(index).cloned() else {
            return;
        };
        let first = view
            .shown
            .borrow()
            .iter()
            .position(|e| self.selectable(e) || self.opens(e));
        if let Some(row) = first {
            view.selection.set_selected(row as u32);
        }
    }

    /// ← : 上の列へ戻る（今の列の選びを外す）。
    fn back(self: &Rc<Self>, index: usize) {
        if index == 0 {
            return;
        }
        self.entering.replace(None);
        self.select(index, None);
        self.fill(index);
        if let Some(view) = self.views.borrow().get(index - 1) {
            view.list.grab_focus();
        }
    }

    // 部品

    /// 列 `from` から後ろの部品を作り直す（その前の列はそのまま）。
    fn sync(self: &Rc<Self>, from: usize) {
        let count = self.columns.borrow().len();
        let mut views = self.views.borrow_mut();
        while views.len() > from {
            let v = views.pop().expect("列");
            self.columns_box.remove(&v.widget);
        }
        for i in from..count {
            let v = self.column_view(i);
            self.columns_box.append(&v.widget);
            views.push(v);
        }
        drop(views);
        for i in from..count {
            self.fill(i);
        }
        self.update_buttons();
        if count > 0
            && self.focus_columns.replace(false)
            && let Some(view) = self.views.borrow().last()
        {
            view.list.grab_focus();
        }
    }

    fn column_view(self: &Rc<Self>, index: usize) -> Rc<ColumnView> {
        let shown: Rc<RefCell<Vec<Entry>>> = Rc::default();
        let model = gtk::StringList::new(&[]);
        let selection = gtk::SingleSelection::new(Some(model.clone()));
        selection.set_autoselect(false);
        selection.set_can_unselect(true);
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(|_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            let name = gtk::Label::new(None);
            name.set_xalign(0.0);
            name.set_hexpand(true);
            name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            let mark = gtk::Label::new(None);
            mark.add_css_class("dim-label");
            let line = gtk::Box::new(gtk::Orientation::Horizontal, 4);
            line.append(&name);
            line.append(&mark);
            item.set_child(Some(&line));
        });
        let (s, w) = (shown.clone(), Rc::downgrade(self));
        factory.connect_bind(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            let Some(p) = w.upgrade() else { return };
            let Some(line) = item.child() else { return };
            let (Some(name), Some(mark)) = (
                line.first_child().and_downcast::<gtk::Label>(),
                line.last_child().and_downcast::<gtk::Label>(),
            ) else {
                return;
            };
            let Some(e) = s.borrow().get(item.position() as usize).cloned() else {
                return;
            };
            let text = glib::markup_escape_text(&e.name);
            let usable = p.selectable(&e) || p.opens(&e);
            name.set_markup(&if e.repository {
                format!("<span foreground=\"#26a269\">{text}</span>")
            } else if !usable {
                format!("<span fgalpha=\"45%\">{text}</span>")
            } else {
                text.to_string()
            });
            let mut marks = String::new();
            if e.link {
                marks.push('↪');
            }
            if p.opens(&e) {
                marks.push('›');
            }
            mark.set_text(&marks);
            let tip = [
                e.repository.then(|| tr!("Git repository")),
                e.link.then(|| tr!("Symbolic link")),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("\n");
            line.set_tooltip_text((!tip.is_empty()).then_some(tip.as_str()));
        });
        let list = gtk::ListView::new(Some(selection.clone()), Some(factory));
        list.add_css_class("navigation-sidebar");
        let scroller = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();
        let note = gtk::Label::new(None);
        note.set_wrap(true);
        note.set_xalign(0.0);
        note.add_css_class("caption");
        note.add_css_class("error");
        note.set_margin_start(6);
        note.set_margin_end(6);
        note.set_visible(false);
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
        widget.set_size_request(210, -1);
        widget.append(&scroller);
        widget.append(&note);
        let outer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        outer.append(&widget);
        outer.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        let (s, w) = (shown.clone(), Rc::downgrade(self));
        selection.connect_selected_notify(move |sel| {
            let Some(p) = w.upgrade() else { return };
            if p.filling.get() {
                return;
            }
            let name = s
                .borrow()
                .get(sel.selected() as usize)
                .map(|e| e.name.clone());
            // 選べないものは選ばない（薄いもの）。
            let usable = s
                .borrow()
                .get(sel.selected() as usize)
                .is_some_and(|e| p.selectable(e) || p.opens(e));
            if name.is_some() && !usable {
                p.fill(index);
                return;
            }
            p.select(index, name);
        });
        // ダブルクリック・Enter: 選べるものなら決める（フォルダを選ぶときは、開いているフォルダ）、選べなければ
        // フォルダに入る（macOS 版の `activate`）。
        let w = Rc::downgrade(self);
        list.connect_activate(move |_, _| {
            if let Some(p) = w.upgrade() {
                if p.chosen().is_some() {
                    p.finish();
                } else {
                    p.enter(index);
                }
            }
        });
        let keys = gtk::EventControllerKey::new();
        let w = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            let Some(p) = w.upgrade() else {
                return glib::Propagation::Proceed;
            };
            match key {
                gtk::gdk::Key::Right => p.enter(index),
                gtk::gdk::Key::Left => p.back(index),
                // 名前を打つと、その名前の行を選ぶ（macOS 版と同じ）。
                _ => match key.to_unicode().filter(|c| !c.is_control() && *c != ' ') {
                    Some(c)
                        if !modifiers.intersects(
                            gtk::gdk::ModifierType::CONTROL_MASK
                                | gtk::gdk::ModifierType::ALT_MASK
                                | gtk::gdk::ModifierType::SUPER_MASK,
                        ) =>
                    {
                        p.type_select(index, c)
                    }
                    _ => return glib::Propagation::Proceed,
                },
            }
            glib::Propagation::Stop
        });
        list.add_controller(keys);
        Rc::new(ColumnView {
            widget: outer,
            list,
            model,
            selection,
            shown,
            note,
        })
    }

    /// 列 `index` の部品に、中身と選びを入れ直す。
    fn fill(&self, index: usize) {
        let Some(view) = self.views.borrow().get(index).cloned() else {
            return;
        };
        let columns = self.columns.borrow();
        let Some(column) = columns.get(index) else {
            return;
        };
        let hidden = self.hidden.is_active();
        let entries: Vec<Entry> = column
            .entries
            .clone()
            .unwrap_or_default()
            .into_iter()
            .filter(|e| {
                hidden || !e.name.starts_with('.') || column.selected.as_deref() == Some(&e.name)
            })
            .collect();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        let position = column
            .selected
            .as_ref()
            .and_then(|s| entries.iter().position(|e| &e.name == s));
        self.filling.set(true);
        view.shown.replace(entries.clone());
        view.model.splice(0, view.model.n_items(), &names);
        view.selection
            .set_selected(position.map_or(gtk::INVALID_LIST_POSITION, |p| p as u32));
        self.filling.set(false);
        if let Some(p) = position {
            view.list
                .scroll_to(p as u32, gtk::ListScrollFlags::NONE, None);
        }
        view.note
            .set_text(column.error.as_deref().unwrap_or_default());
        view.note.set_visible(column.error.is_some());
    }

    /// 最後の列を見えるところまで送る。
    fn reveal(&self) {
        let adj = self.columns_scroll.hadjustment();
        glib::idle_add_local_once(move || adj.set_value(adj.upper()));
    }

    // 決める

    /// 選んだもの（決めるボタンで返すパス）。選べなければ None。つながらなければ、打ち込んだパス。
    fn chosen(&self) -> Option<String> {
        if self.connection_error.borrow().is_some() {
            let typed = self.field.text().trim().to_owned();
            return match self.kind {
                Kind::Folder => (!typed.is_empty()).then_some(typed),
                _ => is_absolute(&typed).then_some(typed),
            };
        }
        let columns = self.columns.borrow();
        let last = columns.last()?;
        match self.kind {
            Kind::Folder => last.error.is_none().then(|| last.path.clone()),
            _ => {
                let name = last.selected.as_ref()?;
                let e = last.entries.as_ref()?.iter().find(|e| &e.name == name)?;
                self.selectable(e).then(|| join(&last.path, name))
            }
        }
    }

    fn finish(&self) {
        let Some(path) = self.chosen() else {
            self.window.error_bell();
            return;
        };
        let home = self.home.borrow().clone();
        let chosen = Chosen {
            host: self.host.borrow().clone(),
            shown: if path.starts_with('~') {
                path.clone()
            } else {
                shown(&path, home.as_deref())
            },
            path,
        };
        let _ = self.tx.try_send(Some(chosen));
        self.window.destroy();
    }

    /// 欄に、選んだもの（なければ最後のフォルダ）を出す。
    fn update_field(&self) {
        let text = {
            let columns = self.columns.borrow();
            let Some(last) = columns.last() else { return };
            let path = last
                .selected
                .as_ref()
                .map_or_else(|| last.path.clone(), |s| join(&last.path, s));
            shown(&path, self.home.borrow().as_deref())
        };
        self.field.set_text(&text);
        self.field.set_position(-1);
        self.update_buttons();
    }

    fn update_buttons(&self) {
        let usable = self.reader.borrow().is_some();
        self.choose.set_sensitive(self.chosen().is_some());
        self.home_button.set_sensitive(usable);
        if let Some(b) = &self.places_button {
            b.set_sensitive(usable);
        }
    }
}

/// 確認用: `AMONHEN_PICKER_SNAPSHOT=<png>` なら、出して 1.5 秒後にこのシートを撮り、`AMONHEN_PICKER_CHOOSE` が
/// あれば決めて（なければ取り消して）続ける。
fn debug_snapshot(picker: &Rc<Picker>) {
    let Some(png) = std::env::var_os("AMONHEN_PICKER_SNAPSHOT") else {
        return;
    };
    let w = Rc::downgrade(picker);
    glib::timeout_add_local_once(std::time::Duration::from_millis(1500), move || {
        let Some(p) = w.upgrade() else { return };
        match crate::snapshot(&p.window, std::path::Path::new(&png)) {
            Ok(()) => eprintln!("{}", std::path::Path::new(&png).display()),
            Err(e) => eprintln!("撮れない: {e}"),
        }
        if std::env::var_os("AMONHEN_PICKER_CHOOSE").is_some() {
            eprintln!("選んだもの: {:?}", p.chosen());
            p.finish();
        } else {
            p.window.close();
        }
    });
}

/// SSH 先の候補: 使ったことのあるもの（先に）と、~/.ssh/config・known_hosts のもの。
fn host_candidates() -> Vec<String> {
    let mut used: Vec<String> = crate::bookmarks::all_locations()
        .iter()
        .filter_map(|l| crate::bookmarks::ssh_host(l))
        .chain(
            crate::bookmarks::data()
                .scan_roots
                .iter()
                .filter_map(|r| crate::bookmarks::ssh_host(&r.path)),
        )
        .collect();
    used.sort();
    used.dedup();
    let rest: Vec<String> = crate::sshhosts::candidates()
        .into_iter()
        .filter(|h| !used.contains(h))
        .collect();
    used.extend(rest);
    used
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_paths_on_unix_and_windows() {
        assert_eq!(ancestors("/a/b/c"), vec!["/", "/a", "/a/b"]);
        assert!(ancestors("/").is_empty());
        assert_eq!(ancestors("C:\\src\\app"), vec!["C:\\", "C:\\src"]);
        assert_eq!(join("/", "usr"), "/usr");
        assert_eq!(join("C:\\src", "x"), "C:\\src\\x");
        assert_eq!(split("/usr/bin"), Some(("/usr".into(), "bin".into())));
        assert_eq!(split("/usr"), Some(("/".into(), "usr".into())));
        assert_eq!(split("~/prj"), Some(("~".into(), "prj".into())));
        assert_eq!(split("C:\\src"), Some(("C:\\".into(), "src".into())));
        assert_eq!(shown("/home/me/prj", Some("/home/me")), "~/prj");
        assert_eq!(shown("/home/me", Some("/home/me")), "~");
        assert_eq!(shown("/srv", Some("/home/me")), "/srv");
    }

    #[test]
    fn writes_the_ssh_location_like_places() {
        let c = |shown: &str| Chosen {
            host: "build".into(),
            path: String::new(),
            shown: shown.into(),
        };
        assert_eq!(c("~/src").ssh_location(), "ssh://build/~/src");
        assert_eq!(c("/srv/git").ssh_location(), "ssh://build/srv/git");
        assert_eq!(c("C:\\src").ssh_location(), "ssh://build/C:/src");
    }
}
