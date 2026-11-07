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
    /// リポジトリの木（2 つめの時点、なければ HEAD）のフォルダ（まだ取り出していないものも。sparse checkout で
    /// 取り出すフォルダや、アーカイブに書き出すフォルダ。パスは `/` を根にした形。ホームはなく、根は選べない）。
    Tree(Arc<Repository>, Option<String>),
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
        ssh_location(&self.host, &self.shown)
            .unwrap_or_else(|| format!("ssh://{}/{}", self.host, self.shown))
    }
}

/// SSH の場所を組み立てる（macOS 版の `RepositoryLocation.ssh`）。`host` は `[user@]host[:port]`、`path` は
/// 向こうの絶対パス（Windows の `C:\…` は `/C:/…` にする）か `~/` から始まるパス（どちらでもなければホームからの
/// 相対とみなす）。終わりの `/` は落とす。書き方がおかしければ None。
pub fn ssh_location(host: &str, path: &str) -> Option<String> {
    let host = host.trim();
    let path = path.trim();
    if host.is_empty() || path.is_empty() || host.contains(|c: char| c == '/' || c.is_whitespace())
    {
        return None;
    }
    let mut path = if path == "~" || path.starts_with("~/") {
        format!("/{path}")
    } else if is_windows(path) {
        format!("/{}", path.replace('\\', "/"))
    } else if !path.starts_with('/') {
        format!("/~/{path}")
    } else {
        path.to_owned()
    };
    // ドライブの根（`/C:/`）の `/` は残す（`C:` だけはそのドライブの今のフォルダ）。
    while path.len() > 1 && path.ends_with('/') && !(path.len() == 4 && is_windows(&path[1..])) {
        path.pop();
    }
    Some(format!("ssh://{host}{path}"))
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
    /// Ctrl+クリックでいくつも選べる（最後の列の、選べるファイル・リポジトリ。[`choose_several`] で受ける）。
    pub multiple: bool,
    /// 開いているフォルダの中にフォルダを作れる（New Folder…・Ctrl+Shift+N。この機械だけ）。
    pub new_folder: bool,
}

impl Default for Request {
    fn default() -> Self {
        Self {
            title: String::new(),
            message: String::new(),
            kind: Kind::Folder,
            initial: String::new(),
            places: Vec::new(),
            multiple: false,
            new_folder: false,
        }
    }
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

/// ↑↓ で行く先（候補の中の位置）。`current` は今の位置、候補がなければ None。端では止まる。
fn step_target(current: Option<usize>, step: i32, count: usize) -> Option<usize> {
    if count == 0 {
        return None;
    }
    Some(match current {
        Some(c) => (c as i64 + step as i64).clamp(0, count as i64 - 1) as usize,
        None if step > 0 => 0,
        None => count - 1,
    })
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

/// 根で始まるパスか（`/`、`C:\`・`C:/`・`C:`、`\\server\share`。`~/x` や相対パスは違う。macOS 版の
/// `HostPath.isAbsolute`）。
fn is_absolute(path: &str) -> bool {
    if path.starts_with('/') {
        return true;
    }
    if let Some(unc) = path.strip_prefix("\\\\") {
        return unc.split(['\\', '/']).filter(|p| !p.is_empty()).count() >= 2;
    }
    let b = path.as_bytes();
    b.len() >= 2
        && b[0].is_ascii_alphabetic()
        && b[1] == b':'
        && matches!(b.get(2), None | Some(b'\\' | b'/'))
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

/// 列の行のアイコン（種類, 強調の色か, 緑か）。macOS 版の `icon(_:)` と同じ分け方。
fn entry_icon(e: &Entry) -> (crate::icon::Kind, bool, bool) {
    use crate::icon::Kind;
    if e.repository {
        (Kind::Branch, false, true)
    } else if e.folder {
        (Kind::FolderFill, true, false)
    } else if e.file {
        (
            if e.executable {
                Kind::Terminal
            } else {
                Kind::Doc
            },
            false,
            false,
        )
    } else {
        (Kind::QuestionSquare, false, false)
    }
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
    /// リポジトリの木（時点、なければ HEAD。フォルダだけ）。
    Tree(Arc<Repository>, Option<String>),
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
        Reader::Tree(repo, rev) => {
            let rev = rev.as_deref().unwrap_or("HEAD");
            crate::reposettings::call(
                repo,
                &json!({ "op": "treeFolders", "path": path, "rev": rev }),
            )
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
    /// Ctrl+クリックで足したもの（`selected` のほかに。いくつも選べるとき）。
    marked: Vec<String>,
}

impl Column {
    fn new(path: String) -> Self {
        Self {
            path,
            entries: None,
            error: None,
            selected: None,
            marked: Vec::new(),
        }
    }

    fn is_selected(&self, name: &str) -> bool {
        self.selected.as_deref() == Some(name) || self.marked.iter().any(|m| m == name)
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
    multiple: bool,
    /// New Folder…（作れないときはなし）。
    new_folder: Option<gtk::Button>,
    window: gtk::Window,
    host_entry: Option<gtk::Entry>,
    field: gtk::Entry,
    home_button: gtk::Button,
    places_button: Option<gtk::MenuButton>,
    hidden: gtk::CheckButton,
    columns_box: gtk::Box,
    columns_scroll: gtk::ScrolledWindow,
    /// 列を足した後、幅が決まったら最後の列まで送る（足した直後はまだ幅が変わっていない）。
    reveal_pending: Cell<bool>,
    status: gtk::Label,
    /// 列ができたら、列にキーボードを移す（macOS 版と同じく、始めは列で選ぶ。SSH 先を選べるときは、つないで列が
    /// 出るたび。パスの欄に打ち込んでいれば欄のまま）。
    focus_columns: Cell<bool>,
    /// パスの欄に打ち込んだ（macOS 版の `typing`。選んだものを欄に出すのは数えない）。
    typing: Cell<bool>,
    /// 選んだものを欄に出している間（打ち込んだことにしない）。
    setting_field: Cell<bool>,
    /// 列で打った名前と、最後に打った時刻（1 秒あけば打ち直し。macOS 版の `typeSelect`）。
    typed: RefCell<(String, Option<std::time::Instant>)>,
    /// つなげなかったときの「接続を確認」（もう一度つなぐ。macOS 版と同じ）。
    retry: gtk::Button,
    /// 列の所（列・SSH 先を選ぶ案内・つなげなかったとき）と、つなげなかったときの見出しと訳。
    area: gtk::Stack,
    failed: (gtk::Label, gtk::Label),
    spinner: gtk::Spinner,
    /// つないでいる間に、列の所に出す印と言葉。
    connecting: (gtk::Box, gtk::Spinner, gtk::Label),
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
    /// SSH 先の欄で最後につないだ名前（同じ名前で何度もつながない。macOS 版の `last`）。
    committed_host: RefCell<Option<String>>,
    /// 部品の選びを入れ直している間（選びの知らせを無視する）。
    filling: Cell<bool>,
    /// → で入ったが、まだ読んでいる列のパス（読めたら最初のものを選ぶ）。
    entering: RefCell<Option<String>>,
    tx: async_channel::Sender<Option<Vec<Chosen>>>,
}

/// シートを出して、選んだものを返す（取り消したら None）。
pub async fn choose(
    parent: Option<&gtk::Window>,
    request: Request,
    source: Source,
) -> Option<Chosen> {
    choose_several(parent, request, source)
        .await
        .and_then(|c| c.into_iter().next())
}

/// [`choose`] の、いくつも選べる形（`request.multiple` なら Ctrl+クリックで。並びは列の順）。
pub async fn choose_several(
    parent: Option<&gtk::Window>,
    request: Request,
    source: Source,
) -> Option<Vec<Chosen>> {
    install_css();
    let (tx, rx) = async_channel::bounded(1);
    let picker = Picker::new(parent, &request, &source, tx);
    match source {
        Source::Local => picker.use_reader(Reader::Local, ""),
        Source::Tree(repo, rev) => picker.use_reader(Reader::Tree(repo, rev), ""),
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

/// Ctrl+クリックで足した行の見た目（選んだ行に近い色。一度だけ入れる）。
fn install_css() {
    thread_local! {
        static DONE: Cell<bool> = const { Cell::new(false) };
    }
    if DONE.with(|d| d.replace(true)) {
        return;
    }
    let provider = gtk::CssProvider::new();
    provider.load_from_string(
        ".picker-marked { background: alpha(@theme_selected_bg_color, 0.45); border-radius: 4px; }",
    );
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

impl Picker {
    fn new(
        parent: Option<&gtk::Window>,
        request: &Request,
        source: &Source,
        tx: async_channel::Sender<Option<Vec<Chosen>>>,
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
                complete_from(&entry, candidates);
            }
            content.append(&row);
            entry
        });

        let field = gtk::Entry::new();
        field.set_hexpand(true);
        field.add_css_class("monospace");
        let tree = matches!(source, Source::Tree(..));
        field.set_placeholder_text(Some(if tree {
            "/src"
        } else if request.kind == Kind::Executable {
            "/usr/local/bin/git"
        } else {
            "~/prj"
        }));
        // ホームは家のアイコン（macOS 版と同じ）。
        let home_button = gtk::Button::new();
        let house = crate::icon::Icon::default();
        house.set(crate::icon::Kind::House, false);
        home_button.set_child(Some(&house));
        home_button.set_tooltip_text(Some(&tr!("Home folder")));
        // リポジトリの木にはホームがない。
        home_button.set_visible(!tree);
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
        // つないでいる間は、列の所の真ん中に回る印と「〜につないでいます…」（macOS 版の ProgressView）。
        let connecting_spinner = gtk::Spinner::new();
        let connecting_label = gtk::Label::new(None);
        connecting_label.add_css_class("dim-label");
        let connecting = gtk::Box::new(gtk::Orientation::Vertical, 8);
        connecting.set_halign(gtk::Align::Center);
        connecting.set_valign(gtk::Align::Center);
        connecting.append(&connecting_spinner);
        connecting.append(&connecting_label);
        connecting.set_visible(false);
        // 列の所: 列、SSH 先を選ぶまでの案内（真ん中に）、つなげなかったとき（左上に、訳と「接続を確認」）を
        // 入れ替える（macOS 版と同じく、列の代わりに出す）。
        let hint = gtk::Label::new(Some(&tr!("Choose a host to see its folders.")));
        hint.add_css_class("dim-label");
        hint.set_halign(gtk::Align::Center);
        hint.set_valign(gtk::Align::Center);
        let failed_title = gtk::Label::new(None);
        failed_title.set_xalign(0.0);
        failed_title.set_wrap(true);
        let failed_detail = gtk::Label::new(None);
        failed_detail.set_xalign(0.0);
        failed_detail.set_wrap(true);
        failed_detail.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        failed_detail.set_selectable(true);
        failed_detail.add_css_class("caption");
        failed_detail.add_css_class("error");
        let failed_note = gtk::Label::new(Some(&tr!(
            "You can still type the path above and choose it."
        )));
        failed_note.set_xalign(0.0);
        failed_note.set_wrap(true);
        failed_note.add_css_class("caption");
        failed_note.add_css_class("dim-label");
        let retry = gtk::Button::with_label(&tr!("Check Connection"));
        retry.set_halign(gtk::Align::Start);
        let failed = gtk::Box::new(gtk::Orientation::Vertical, 8);
        failed.set_halign(gtk::Align::Fill);
        failed.set_valign(gtk::Align::Start);
        failed.set_margin_top(8);
        failed.set_margin_start(8);
        failed.set_margin_end(8);
        failed.append(&failed_title);
        failed.append(&failed_detail);
        failed.append(&failed_note);
        failed.append(&retry);
        let area = gtk::Stack::new();
        area.set_vexpand(true);
        area.add_named(&columns_scroll, Some("columns"));
        area.add_named(&hint, Some("hint"));
        area.add_named(&failed, Some("failed"));
        let columns_area = gtk::Overlay::new();
        columns_area.set_child(Some(&area));
        columns_area.add_overlay(&connecting);
        content.append(&columns_area);
        let status = gtk::Label::new(None);
        status.set_xalign(0.0);
        status.set_wrap(true);
        status.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        status.set_selectable(true);
        status.add_css_class("caption");
        status.set_visible(false);
        content.append(&status);

        let hidden = gtk::CheckButton::with_label(&tr!("Show hidden files"));
        let cancel = gtk::Button::with_label(&tr!("Cancel"));
        let choose = gtk::Button::with_label(&tr!("Choose"));
        choose.add_css_class("suggested-action");
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        buttons.append(&hidden);
        // 隠しファイルの切り替えの隣に（macOS 版と同じ）。
        let new_folder = request.new_folder.then(|| {
            let b = gtk::Button::with_label(&tr!("New Folder…"));
            buttons.append(&b);
            b
        });
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        buttons.append(&spacer);
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
            multiple: request.multiple,
            new_folder,
            window,
            host_entry,
            field,
            home_button,
            places_button,
            hidden,
            columns_box,
            columns_scroll,
            reveal_pending: Cell::new(false),
            status,
            focus_columns: Cell::new(true),
            typing: Cell::new(false),
            setting_field: Cell::new(false),
            typed: RefCell::default(),
            retry,
            area,
            failed: (failed_title, failed_detail),
            spinner,
            connecting: (connecting, connecting_spinner, connecting_label),
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
            committed_host: RefCell::default(),
            filling: Cell::new(false),
            entering: RefCell::default(),
            tx,
        });
        picker.wire(&cancel);
        picker.update_buttons();
        picker
    }

    fn wire(self: &Rc<Self>, cancel: &gtk::Button) {
        // 列の幅が決まって送れる幅が変わったら、頼まれていれば最後の列まで送る。幅が変わるのは割り付けの
        // 途中なので、送るのはそれが終わってから（途中で値を変えても、列の位置は次に割り付けるまで動かない）。
        let w = Rc::downgrade(self);
        self.columns_scroll
            .hadjustment()
            .connect_upper_notify(move |adj| {
                if !w.upgrade().is_some_and(|p| p.reveal_pending.get()) {
                    return;
                }
                let adj = adj.clone();
                glib::idle_add_local_once(move || adj.set_value(adj.upper()));
            });
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
                p.commit_host(&h);
            }
        });
        group.add_action(&host);
        self.window.insert_action_group("picker", Some(&group));

        if let Some(entry) = &self.host_entry {
            let w = Rc::downgrade(self);
            entry.connect_activate(move |e| {
                if let Some(p) = w.upgrade() {
                    p.commit_host(&e.text());
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
                if !p.setting_field.get() {
                    p.typing.set(true);
                }
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
        if let Some(b) = &self.new_folder {
            let w = Rc::downgrade(self);
            b.connect_clicked(move |_| {
                if let Some(p) = w.upgrade() {
                    p.ask_new_folder();
                }
            });
        }
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
            // Ctrl+Shift+N でフォルダを作る（macOS 版の ⌘⇧N）。
            if modifiers.contains(ctrl_shift)
                && matches!(key, gtk::gdk::Key::n | gtk::gdk::Key::N)
                && p.new_folder.as_ref().is_some_and(|b| b.is_sensitive())
            {
                p.ask_new_folder();
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
    /// SSH 先の欄で決めた名前につなぐ。前に決めた名前と同じなら何もしない（macOS 版と同じ）。
    fn commit_host(self: &Rc<Self>, host: &str) {
        let host = host.trim().to_owned();
        if self.committed_host.borrow().as_deref() == Some(host.as_str()) {
            return;
        }
        self.committed_host.replace(Some(host.clone()));
        self.connect(&host);
    }

    fn connect(self: &Rc<Self>, host: &str) {
        let host = host.trim().to_owned();
        let attempt = self.attempt.get() + 1;
        self.attempt.set(attempt);
        self.host.replace(host.clone());
        self.reader.replace(None);
        self.connection_error.replace(None);
        self.columns.borrow_mut().clear();
        // つないで列が出たら、列に移す（macOS 版は列の部品が出るたび）。
        self.focus_columns.set(true);
        self.sync(0);
        if host.is_empty() || host.contains('/') || host.contains(char::is_whitespace) {
            self.show_connecting(None);
            self.set_status(None, false);
            self.area.set_visible_child_name("hint");
            self.update_buttons();
            return;
        }
        self.set_status(None, false);
        self.area.set_visible_child_name("columns");
        self.show_connecting(Some(&host));
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
            p.show_connecting(None);
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
                    let (title, detail) = &p.failed;
                    title.set_text(&tr!(
                        "Couldn’t connect to {host}",
                        host = p.host.borrow().clone()
                    ));
                    detail.set_text(&e);
                    p.area.set_visible_child_name("failed");
                    p.update_buttons();
                }
            }
        });
    }

    /// つないでいる間の印を `host` で出す（None なら消す）。
    fn show_connecting(&self, host: Option<&str>) {
        let (area, spinner, label) = &self.connecting;
        if let Some(host) = host {
            label.set_text(&tr!("Connecting to {host}…", host = host));
        }
        area.set_visible(host.is_some());
        spinner.set_spinning(host.is_some());
    }

    fn set_status(&self, text: Option<String>, error: bool) {
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
            columns[index].marked.clear();
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

    /// ↑↓: 列 `index` の中で、選べるもの・開くものの前後を選ぶ。何も選んでいなければ端のもの（macOS 版の `move`）。
    fn step(self: &Rc<Self>, index: usize, step: i32) {
        let Some(view) = self.views.borrow().get(index).cloned() else {
            return;
        };
        let rows: Vec<usize> = view
            .shown
            .borrow()
            .iter()
            .enumerate()
            .filter(|(_, e)| self.selectable(e) || self.opens(e))
            .map(|(i, _)| i)
            .collect();
        let selected = self
            .columns
            .borrow()
            .get(index)
            .and_then(|c| c.selected.clone());
        let current = selected.and_then(|name| {
            let shown = view.shown.borrow();
            rows.iter().position(|&r| shown[r].name == name)
        });
        let Some(next) = step_target(current, step, rows.len()) else {
            return;
        };
        let row = rows[next] as u32;
        if current != Some(next) {
            view.selection.set_selected(row);
        }
        view.list.scroll_to(row, gtk::ListScrollFlags::FOCUS, None);
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
            && !self.typing.get()
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
        let w = Rc::downgrade(self);
        factory.connect_setup(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            let name = gtk::Label::new(None);
            name.set_xalign(0.0);
            name.set_hexpand(true);
            name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            name.set_hexpand(false);
            // 種類のアイコン・名前・リンクの印・（開けるなら）右端の ›（macOS 版と同じ並び）。
            let kind = crate::icon::Icon::default();
            let link = gtk::Label::new(Some("↱"));
            link.add_css_class("dim-label");
            link.add_css_class("caption");
            link.set_tooltip_text(Some(&tr!("Symbolic link")));
            let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            spacer.set_hexpand(true);
            let mark = gtk::Label::new(None);
            mark.add_css_class("dim-label");
            let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            line.append(&kind);
            line.append(&name);
            line.append(&link);
            line.append(&spacer);
            line.append(&mark);
            item.set_child(Some(&line));
            // いくつも選べるなら、Ctrl+クリックで選んだものに足す・外す（macOS 版の ⌘ クリック）。行の選びより先に受ける。
            if w.upgrade().is_some_and(|p| p.multiple) {
                let click = gtk::GestureClick::new();
                let (w, item) = (w.clone(), item.downgrade());
                click.connect_pressed(move |gesture, _, _, _| {
                    let ctrl = gesture
                        .current_event_state()
                        .contains(gtk::gdk::ModifierType::CONTROL_MASK);
                    let (Some(p), Some(item)) = (w.upgrade(), item.upgrade()) else {
                        return;
                    };
                    if ctrl && p.toggle_mark(index, item.position() as usize) {
                        gesture.set_state(gtk::EventSequenceState::Claimed);
                    }
                });
                line.add_controller(click);
            }
        });
        let (s, w) = (shown.clone(), Rc::downgrade(self));
        factory.connect_bind(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            let Some(p) = w.upgrade() else { return };
            let Some(line) = item.child() else { return };
            let kind = line.first_child().and_downcast::<crate::icon::Icon>();
            let name = kind
                .as_ref()
                .and_then(|k| k.next_sibling())
                .and_downcast::<gtk::Label>();
            let link = name.as_ref().and_then(|n| n.next_sibling());
            let (Some(kind), Some(name), Some(link), Some(mark)) = (
                kind,
                name,
                link,
                line.last_child().and_downcast::<gtk::Label>(),
            ) else {
                return;
            };
            let Some(e) = s.borrow().get(item.position() as usize).cloned() else {
                return;
            };
            let text = glib::markup_escape_text(&e.name);
            let usable = p.selectable(&e) || p.opens(&e);
            // Ctrl+クリックで足したもの（選んだ行と同じように見せる）。
            let marked = p
                .columns
                .borrow()
                .get(index)
                .is_some_and(|c| c.marked.contains(&e.name));
            line.set_widget_name(&e.name);
            if marked {
                line.add_css_class("picker-marked");
            } else {
                line.remove_css_class("picker-marked");
            }
            name.set_markup(&if !usable {
                format!("<span fgalpha=\"45%\">{text}</span>")
            } else {
                text.to_string()
            });
            // リポジトリは緑、フォルダは強調の色、ほかは薄く（macOS 版と同じ）。
            let (icon, accent, green) = entry_icon(&e);
            kind.set(icon, accent);
            if green {
                kind.add_css_class("success");
            } else {
                kind.remove_css_class("success");
            }
            kind.set_tooltip_text(e.repository.then(|| tr!("Git repository")).as_deref());
            kind.set_opacity(if usable { 1.0 } else { 0.6 });
            link.set_visible(e.link);
            mark.set_text(if p.opens(&e) { "›" } else { "" });
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
        // ダブルクリック・Enter: その行が選べるものなら、それを選んで決める（フォルダを選ぶときは、そのフォルダ）。
        // 選べないが開くフォルダなら、Enter では中に入り、ダブルクリックでは何もしない（1 回目の押しで開いている。
        // 中の最初のものを選ぶと、そのまま決められてしまう）。薄いもの（選べず、開きもしない）では何もしない
        // （macOS 版と同じ。開いているフォルダで決めてしまわない）。
        let double = Rc::new(Cell::new(false));
        let clicks = gtk::GestureClick::new();
        clicks.set_propagation_phase(gtk::PropagationPhase::Capture);
        let d = double.clone();
        clicks.connect_pressed(move |_, n, _, _| {
            if n >= 2 {
                d.set(true);
                let d = d.clone();
                glib::idle_add_local_once(move || d.set(false));
            }
        });
        list.add_controller(clicks);
        let (s, w) = (shown.clone(), Rc::downgrade(self));
        list.connect_activate(move |_, position| {
            let Some(p) = w.upgrade() else { return };
            let Some(entry) = s.borrow().get(position as usize).cloned() else {
                return;
            };
            if p.selectable(&entry) {
                let selected = p
                    .columns
                    .borrow()
                    .get(index)
                    .and_then(|c| c.selected.clone());
                if selected.as_deref() != Some(entry.name.as_str()) {
                    p.select(index, Some(entry.name.clone()));
                }
                p.finish();
            } else if p.opens(&entry) && !double.get() {
                p.enter(index);
            }
        });
        let keys = gtk::EventControllerKey::new();
        let w = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            let Some(p) = w.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let plain = !modifiers.intersects(
                gtk::gdk::ModifierType::CONTROL_MASK
                    | gtk::gdk::ModifierType::ALT_MASK
                    | gtk::gdk::ModifierType::SUPER_MASK,
            );
            match key {
                gtk::gdk::Key::Right => p.enter(index),
                gtk::gdk::Key::Left => p.back(index),
                // 選べるものなら選ぶ。選べなければ、選んでいるフォルダに入る（行がなくても開いているフォルダを選べる。
                // macOS 版と同じ）。
                gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter if plain => {
                    if p.chosen().is_some() {
                        p.finish();
                    } else {
                        p.enter(index);
                    }
                }
                // ↑↓ は選べるもの・開くものだけを渡る（ListView のままだと薄い行に止まる。macOS 版と同じ）。
                gtk::gdk::Key::Up | gtk::gdk::Key::KP_Up if plain => p.step(index, -1),
                gtk::gdk::Key::Down | gtk::gdk::Key::KP_Down if plain => p.step(index, 1),
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
        // 行より先に受ける（↑↓ を ListView に渡さない）。
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
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
            .filter(|e| hidden || !e.name.starts_with('.') || column.is_selected(&e.name))
            .collect();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        let position = column
            .selected
            .as_ref()
            .and_then(|s| entries.iter().position(|e| &e.name == s));
        // 並びが同じ（Ctrl+クリックで足した・外しただけ）なら、作り直さずに選びと印だけ直す。作り直すと
        // 一覧が先頭まで戻り、続けて Ctrl+クリックする行を見失う。
        let same = view.model.n_items() as usize == names.len()
            && names
                .iter()
                .enumerate()
                .all(|(i, n)| view.model.string(i as u32).as_deref() == Some(*n));
        self.filling.set(true);
        view.shown.replace(entries.clone());
        if !same {
            view.model.splice(0, view.model.n_items(), &names);
        }
        view.selection
            .set_selected(position.map_or(gtk::INVALID_LIST_POSITION, |p| p as u32));
        self.filling.set(false);
        if same {
            self.refresh_marks(index, &view);
        } else if let Some(p) = position {
            // 割り付ける前の一覧では送っても効かない（列を作ってすぐに中身が届く）ので、割り付けてから。
            view.list.add_tick_callback(move |list, _| {
                if list.height() == 0 {
                    return glib::ControlFlow::Continue;
                }
                list.scroll_to(p as u32, gtk::ListScrollFlags::NONE, None);
                glib::ControlFlow::Break
            });
        }
        view.note
            .set_text(column.error.as_deref().unwrap_or_default());
        view.note.set_visible(column.error.is_some());
    }

    /// 出ている行の、Ctrl+クリックで足した印を付け直す（行の名前は bind で部品の名前に入れてある）。
    fn refresh_marks(&self, index: usize, view: &ColumnView) {
        let columns = self.columns.borrow();
        let Some(column) = columns.get(index) else {
            return;
        };
        let mut row = view.list.first_child();
        while let Some(r) = row {
            if let Some(line) = r.first_child() {
                if column.marked.iter().any(|m| *m == line.widget_name()) {
                    line.add_css_class("picker-marked");
                } else {
                    line.remove_css_class("picker-marked");
                }
            }
            row = r.next_sibling();
        }
    }

    /// 最後の列を見えるところまで送る。
    fn reveal(self: &Rc<Self>) {
        let adj = self.columns_scroll.hadjustment();
        adj.set_value(adj.upper());
        // 幅は列の中身を読むたびに何度か変わるので、少しの間は変わるたびに送る。
        self.reveal_pending.set(true);
        let generation = self.generation.get();
        let w = Rc::downgrade(self);
        glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
            if let Some(p) = w.upgrade()
                && p.generation.get() == generation
            {
                p.reveal_pending.set(false);
            }
        });
    }

    // 決める

    /// 選んだもの（決めるボタンで返すパス）。選べなければ None。つながらなければ、打ち込んだパス。
    fn chosen(&self) -> Option<String> {
        self.chosen_all().into_iter().next()
    }

    /// 選んだものすべて（並びは列の順。Ctrl+クリックで足したものも。macOS 版の `chosen`）。
    fn chosen_all(&self) -> Vec<String> {
        if self.connection_error.borrow().is_some() {
            let typed = self.field.text().trim().to_owned();
            let ok = match self.kind {
                Kind::Folder => !typed.is_empty(),
                _ => is_absolute(&typed),
            };
            return if ok { vec![typed] } else { Vec::new() };
        }
        let columns = self.columns.borrow();
        let Some(last) = columns.last() else {
            return Vec::new();
        };
        // リポジトリの木の根は選べない（すべてのフォルダになる）。
        let tree_root = matches!(*self.reader.borrow(), Some(Reader::Tree(..))) && last.path == "/";
        match self.kind {
            Kind::Folder if last.error.is_none() && !tree_root => vec![last.path.clone()],
            Kind::Folder => Vec::new(),
            _ => last
                .entries
                .iter()
                .flatten()
                .filter(|e| last.is_selected(&e.name) && self.selectable(e))
                .map(|e| join(&last.path, &e.name))
                .collect(),
        }
    }

    fn finish(&self) {
        let paths = self.chosen_all();
        if paths.is_empty() {
            self.window.error_bell();
            return;
        }
        let home = self.home.borrow().clone();
        let chosen = paths
            .into_iter()
            .map(|path| Chosen {
                host: self.host.borrow().clone(),
                shown: if path.starts_with('~') {
                    path.clone()
                } else {
                    shown(&path, home.as_deref())
                },
                path,
            })
            .collect();
        let _ = self.tx.try_send(Some(chosen));
        self.window.destroy();
    }

    /// 列 `index` の `row` 行目を、選んだものに足す（選んであれば外す）。足せるのは、いくつも選べるときの、最後の列の
    /// 選べて開かないものだけ（macOS 版の `select(extend:)`）。足し外ししたら true。
    fn toggle_mark(&self, index: usize, row: usize) -> bool {
        let Some(view) = self.views.borrow().get(index).cloned() else {
            return false;
        };
        let Some(entry) = view.shown.borrow().get(row).cloned() else {
            return false;
        };
        {
            let mut columns = self.columns.borrow_mut();
            let last = columns.len().saturating_sub(1);
            if !self.multiple || index != last || !self.selectable(&entry) || self.opens(&entry) {
                return false;
            }
            let column = &mut columns[index];
            if column.is_selected(&entry.name) {
                column.marked.retain(|m| *m != entry.name);
                if column.selected.as_deref() == Some(entry.name.as_str()) {
                    column.selected = (!column.marked.is_empty()).then(|| column.marked.remove(0));
                }
            } else {
                let selected_usable = column.selected.as_ref().is_some_and(|s| {
                    column
                        .entries
                        .iter()
                        .flatten()
                        .any(|e| &e.name == s && self.selectable(e))
                });
                if selected_usable && let Some(s) = column.selected.take() {
                    column.marked.push(s);
                }
                column.selected = Some(entry.name.clone());
            }
        }
        self.fill(index);
        self.update_field();
        true
    }

    /// 開いているフォルダ（最後の列。読めていて、エラーがないもの）。
    fn open_folder(&self) -> Option<String> {
        let columns = self.columns.borrow();
        let last = columns.last()?;
        (last.error.is_none() && last.entries.is_some()).then(|| last.path.clone())
    }

    /// New Folder…: 名前を聞いて、開いているフォルダの中に作り、そこへ行く（この機械だけ。macOS 版の `createFolder`）。
    fn ask_new_folder(self: &Rc<Self>) {
        let local = matches!(*self.reader.borrow(), Some(Reader::Local));
        let Some(folder) = self.open_folder().filter(|_| local) else {
            self.window.error_bell();
            return;
        };
        let w = Rc::downgrade(self);
        let parent = self.window.clone();
        glib::spawn_future_local(async move {
            let shown_folder = w
                .upgrade()
                .map(|p| shown(&folder, p.home.borrow().as_deref()))
                .unwrap_or_else(|| folder.clone());
            let sheet = crate::sheets::Sheet::new(
                Some(&parent),
                &tr!("New Folder"),
                &tr!("In {path}", path = shown_folder),
            );
            let entry = gtk::Entry::builder()
                .placeholder_text(tr!("Folder name"))
                .activates_default(true)
                .hexpand(true)
                .build();
            sheet.add(&entry);
            sheet.buttons(&tr!("Create"), false, None);
            entry.grab_focus();
            if sheet.run().await != crate::sheets::Response::Ok {
                return;
            }
            let Some(p) = w.upgrade() else { return };
            let name = entry.text().trim().to_owned();
            if name.is_empty() {
                return;
            }
            if name.contains(['/', '\\']) || name == "." || name == ".." {
                p.set_status(Some(tr!("A folder name can’t contain “/”.")), true);
                return;
            }
            let path = join(&folder, &name);
            match std::fs::create_dir(&path) {
                Ok(()) => p.go(&path, false),
                Err(e) => p.set_status(Some(e.to_string()), true),
            }
        });
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
        self.setting_field.set(true);
        self.field.set_text(&text);
        self.setting_field.set(false);
        self.field.set_position(-1);
        self.update_buttons();
    }

    fn update_buttons(&self) {
        let usable = self.reader.borrow().is_some();
        self.choose.set_sensitive(self.chosen().is_some());
        if let Some(b) = &self.new_folder {
            let local = matches!(*self.reader.borrow(), Some(Reader::Local));
            b.set_sensitive(local && self.open_folder().is_some());
        }
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

/// 打った文字 `typed` を頭に持つ最初の候補（大文字小文字は見ない。打ったものより長いものだけ）。
fn completion<'a>(typed: &str, candidates: &'a [String]) -> Option<&'a str> {
    if typed.is_empty() {
        return None;
    }
    let lower = typed.to_lowercase();
    candidates
        .iter()
        .find(|c| c.len() > typed.len() && c.to_lowercase().starts_with(&lower))
        .map(String::as_str)
}

/// 欄に打つと、候補の残りを足して選んでおく（macOS 版の NSComboBox の completes）。消したときは足さない。
fn complete_from(entry: &gtk::Entry, candidates: Vec<String>) {
    let busy = Rc::new(Cell::new(false));
    let before = Rc::new(Cell::new(entry.text().chars().count()));
    entry.connect_changed(move |e| {
        if busy.get() {
            return;
        }
        let text = e.text().to_string();
        let count = text.chars().count();
        let grew = count > before.replace(count);
        if !grew || e.position() != count as i32 {
            return;
        }
        let Some(full) = completion(&text, &candidates) else {
            return;
        };
        // 打ったところはそのまま、残りだけを候補から足す。
        let rest: String = full.chars().skip(count).collect();
        busy.set(true);
        let mut at = count as i32;
        e.insert_text(&rest, &mut at);
        busy.set(false);
        e.select_region(count as i32, -1);
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
    fn typed_paths_must_start_at_a_root() {
        assert!(is_absolute("/usr/bin/git"));
        assert!(is_absolute("C:\\Program Files"));
        assert!(is_absolute("C:/x"));
        assert!(is_absolute("C:"));
        assert!(is_absolute("\\\\server\\share"));
        assert!(!is_absolute("\\\\server"));
        assert!(!is_absolute("~/bin/git"));
        assert!(!is_absolute("bin/git"));
        assert!(!is_absolute("C:x"));
    }

    #[test]
    fn completes_a_host_from_the_first_candidate() {
        let hosts = [
            "build-box".to_owned(),
            "Builder".to_owned(),
            "web".to_owned(),
        ];
        assert_eq!(completion("bu", &hosts), Some("build-box"));
        assert_eq!(completion("BUILDE", &hosts), Some("Builder"));
        assert_eq!(completion("web", &hosts), None);
        assert_eq!(completion("", &hosts), None);
        assert_eq!(completion("x", &hosts), None);
    }

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
    fn steps_over_the_candidates() {
        assert_eq!(step_target(None, 1, 0), None);
        assert_eq!(step_target(None, 1, 3), Some(0));
        assert_eq!(step_target(None, -1, 3), Some(2));
        assert_eq!(step_target(Some(1), 1, 3), Some(2));
        assert_eq!(step_target(Some(2), 1, 3), Some(2));
        assert_eq!(step_target(Some(0), -1, 3), Some(0));
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
        assert_eq!(c("~/src/").ssh_location(), "ssh://build/~/src");
        assert_eq!(c("~").ssh_location(), "ssh://build/~");
        assert_eq!(c("src/repo").ssh_location(), "ssh://build/~/src/repo");
        assert_eq!(c("/srv/git//").ssh_location(), "ssh://build/srv/git");
        assert_eq!(c("/").ssh_location(), "ssh://build/");
        assert_eq!(c("C:\\").ssh_location(), "ssh://build/C:/");
        assert_eq!(ssh_location("", "~/a"), None);
        assert_eq!(ssh_location("a b", "~/a"), None);
        assert_eq!(ssh_location("build", " "), None);
    }
}
