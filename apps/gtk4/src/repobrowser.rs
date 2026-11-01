//! Repositories（Ctrl+Shift+R。macOS 版の `RepositoryBrowser.swift` のページと `RepositoryHome.swift`）: ブックマーク・
//! 最近開いたもの・スキャンで見つけたものを見渡して開くページ。左に分類（すべて・お気に入り・場所（このコンピュータと
//! SSH のホストごと。その下にスキャンするフォルダとフォルダの木）・手で作ったグループ）と件数、右にその中のリポジトリを
//! 列（名前とパス・ブランチ・状態・最後のコミット・開いた日時）で並べる。リポジトリを開いていないタブ
//! （`tabs::show_home`）の中身で、開く（ダブルクリック・Enter）とそのタブが選んだリポジトリになる。
//!
//! ツールバーのリポジトリ名から出す小さな一覧（[`compact`]）も、ここで作る。

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gio, glib};

use crate::bookmarks;
use crate::filepicker::{self, Kind, Request, Source};
use crate::menu::{self, Item};
use crate::sheets::{Response, Sheet};

/// リポジトリを新しい窓で開く。
/// リポジトリを開く（場所と、開く先: 前の窓のタブか新しい窓か）。
pub type Open = Rc<dyn Fn(PathBuf, crate::tabs::Place)>;

/// 左の分類。
#[derive(Clone, Debug, PartialEq, Eq)]
enum Category {
    All,
    Favorites,
    /// この機械のリポジトリ（手元の共有の daemon 越しも含む）。
    Local,
    /// SSH のホストごと。
    Host(String),
    Group(String),
    /// スキャンするフォルダ。
    Scan(String),
    /// この機械のフォルダの下（スキャンするフォルダの外のリポジトリ。場所の木）。
    Folder(String),
    /// SSH のホストのフォルダの下（ホストと、ホストの上のパス）。
    HostFolder(String, String),
    /// スキャンするフォルダの中のフォルダの下（スキャンするフォルダと、その中のパス）。
    ScanFolder(String, String),
}

impl Category {
    /// 覚えておくための文字列（macOS 版の `Category.key` と同じ形）。
    fn key(&self) -> String {
        match self {
            Self::All => "all".into(),
            Self::Favorites => "favorites".into(),
            Self::Local => "local".into(),
            Self::Host(h) => format!("host:{h}"),
            Self::Group(g) => format!("group:{g}"),
            Self::Scan(p) => format!("scan:{p}"),
            Self::Folder(p) => format!("folder:{p}"),
            Self::HostFolder(h, p) => format!("hostfolder:{h}\n{p}"),
            Self::ScanFolder(r, p) => format!("scanfolder:{r}\n{p}"),
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        Some(match key {
            "all" => Self::All,
            "favorites" => Self::Favorites,
            "local" => Self::Local,
            _ => {
                let (kind, rest) = key.split_once(':')?;
                match kind {
                    "host" => Self::Host(rest.into()),
                    "group" => Self::Group(rest.into()),
                    "scan" => Self::Scan(rest.into()),
                    "folder" => Self::Folder(rest.into()),
                    "hostfolder" => {
                        let (h, p) = rest.split_once('\n')?;
                        Self::HostFolder(h.into(), p.into())
                    }
                    "scanfolder" => {
                        let (r, p) = rest.split_once('\n')?;
                        Self::ScanFolder(r.into(), p.into())
                    }
                    _ => return None,
                }
            }
        })
    }
}

const CATEGORY_KEY: &str = "repositoryPageCategory";
/// 手元のリポジトリを選んだ、前のフォルダ。
const LAST_LOCAL_FOLDER_KEY: &str = "lastLocalRepositoryFolder";
/// SSH 先でスキャンするフォルダを加えた、前のホスト。
const LAST_HOST_KEY: &str = "lastScanHost";
/// 開いている左の節（鍵を改行で区切る）。
const EXPANDED_KEY: &str = "repositoryPageExpanded";

/// 覚えている、開いている左の節（初めは、このコンピュータだけ。macOS 版と同じ）。
fn saved_expanded() -> std::collections::HashSet<String> {
    match crate::settings::string(EXPANDED_KEY) {
        Some(s) => s
            .split('\n')
            .filter(|k| !k.is_empty())
            .map(str::to_owned)
            .collect(),
        None => [Category::Local.key()].into_iter().collect(),
    }
}

/// 左の行: 見出し（場所・グループ）か分類。
#[derive(Clone, Debug, PartialEq, Eq)]
enum Side {
    Places,
    Groups,
    Category(Category),
}

impl Side {
    fn category(&self) -> Option<&Category> {
        match self {
            Self::Category(c) => Some(c),
            _ => None,
        }
    }
}

/// 左に並べる 1 行（開いた節の中だけ）。
struct SideRow {
    side: Side,
    title: String,
    /// 木の深さ（字下げ）。
    depth: i32,
    tip: Option<String>,
    /// 子のある節か（開く・閉じるの印を出す）。
    parent: bool,
}

/// 分類の中のリポジトリ（絞り込み・並べ替えの前）。
fn paths_in(category: &Category) -> Vec<String> {
    let data = bookmarks::data();
    // 場所の下のフォルダは、スキャンするフォルダの外のものだけ（左の木と同じ。スキャンした分は 🔍 の下に出す）。
    let outside_under = |host: Option<&str>, prefix: &str| -> Vec<String> {
        let roots = Roots::new(&data);
        let under = format!("{prefix}/");
        bookmarks::all_locations()
            .into_iter()
            .filter(|l| {
                bookmarks::ssh_host(l).as_deref() == host
                    && !roots.contains(l)
                    && bookmarks::path_part(l).starts_with(&under)
            })
            .collect()
    };
    match category {
        Category::All => bookmarks::all_locations(),
        Category::Local => bookmarks::all_locations()
            .into_iter()
            .filter(|l| bookmarks::ssh_host(l).is_none())
            .collect(),
        Category::Host(h) => bookmarks::all_locations()
            .into_iter()
            .filter(|l| bookmarks::ssh_host(l).as_ref() == Some(h))
            .collect(),
        Category::Favorites => data
            .items
            .iter()
            .filter(|b| b.favorite)
            .map(|b| b.path.clone())
            .collect(),
        Category::Group(g) => data
            .items
            .iter()
            .filter(|b| b.group.as_ref() == Some(g))
            .map(|b| b.path.clone())
            .collect(),
        Category::Scan(root) => bookmarks::scanned(root),
        Category::Folder(prefix) => outside_under(None, prefix),
        Category::HostFolder(host, prefix) => outside_under(Some(host), prefix),
        Category::ScanFolder(root, prefix) => {
            let under = format!("{prefix}/");
            bookmarks::scanned(root)
                .into_iter()
                .filter(|l| bookmarks::path_part(l).starts_with(&under))
                .collect()
        }
    }
}

/// スキャンするフォルダ（どれかの下にあるか、を引く）。
struct Roots {
    /// (ホスト, フォルダのパス, 見つけたもの)。
    roots: Vec<(Option<String>, String, std::collections::HashSet<String>)>,
}

impl Roots {
    fn new(data: &bookmarks::Data) -> Self {
        Self {
            roots: data
                .scan_roots
                .iter()
                .map(|r| {
                    (
                        bookmarks::ssh_host(&r.path),
                        root_base(&r.path),
                        bookmarks::scanned(&r.path).into_iter().collect(),
                    )
                })
                .collect(),
        }
    }

    /// `location` がスキャンするフォルダの下にあるか（見つけたもの、またはパスがその下のもの）。
    fn contains(&self, location: &str) -> bool {
        let host = bookmarks::ssh_host(location);
        let path = bookmarks::path_part(location);
        self.roots.iter().any(|(h, base, found)| {
            found.contains(location)
                || (*h == host && (path == *base || path.starts_with(&format!("{base}/"))))
        })
    }
}

/// スキャンするフォルダのパス（手元なら `~` を広げる。比べるため）。
fn root_base(root: &str) -> String {
    match bookmarks::ssh_host(root) {
        Some(_) => bookmarks::path_part(root).trim_end_matches('/').to_owned(),
        None => bookmarks::expand(&bookmarks::path_part(root))
            .to_string_lossy()
            .trim_end_matches('/')
            .to_owned(),
    }
}

/// SSH 先のパスの見せ方（`/~/prj` は `~/prj`、Windows の SSH 先の `/C:/src` は `C:/src`）。
pub(crate) fn remote_title(path: &str) -> String {
    let rest = &path[path.len().min(1)..];
    let b = rest.as_bytes();
    if path.starts_with("/~") || (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':') {
        rest.to_owned()
    } else {
        path.to_owned()
    }
}

/// 左の木の節（分類と、その下の節）。
struct Node {
    title: String,
    category: Category,
    /// 短くした名前なら、元のパス（ツールチップ）。
    tip: Option<String>,
    children: Vec<Node>,
}

/// リポジトリのあるフォルダの階層（macOS 版の `folderTree`）。`repositories` はリポジトリのパス（`base` の下）。
/// リポジトリの入っているフォルダとその上のフォルダを節にし、枝分かれもリポジトリもない途中のフォルダは 1 つに
/// まとめる（`a/b`）。一番上の節の名前は `title`（フルパスから）、その下はひとつ上からの相対。
fn folder_tree(
    repositories: &[String],
    base: &str,
    category: &dyn Fn(String) -> Category,
    title: &dyn Fn(&str) -> String,
) -> Vec<Node> {
    #[derive(Default)]
    struct Trie {
        children: std::collections::BTreeMap<String, Trie>,
        repositories: usize,
    }
    let mut root = Trie::default();
    for path in repositories {
        let Some(i) = path.rfind('/') else { continue };
        let parent = &path[..i];
        if !(base.is_empty() || parent == base || parent.starts_with(&format!("{base}/"))) {
            continue;
        }
        let mut node = &mut root;
        for part in parent[base.len()..].split('/').filter(|p| !p.is_empty()) {
            node = node.children.entry(part.to_owned()).or_default();
        }
        node.repositories += 1;
    }
    /// 枝分かれもリポジトリもない途中のフォルダを降りる。
    fn only_child(node: &Trie) -> Option<(&String, &Trie)> {
        (node.repositories == 0 && node.children.len() == 1)
            .then(|| node.children.iter().next())
            .flatten()
    }
    fn make(
        node: &Trie,
        prefix: &str,
        top: bool,
        category: &dyn Fn(String) -> Category,
        title: &dyn Fn(&str) -> String,
    ) -> Vec<Node> {
        let mut names: Vec<&String> = node.children.keys().collect();
        names.sort_by_key(|n| n.to_lowercase());
        names
            .into_iter()
            .map(|name| {
                let mut child = &node.children[name];
                let mut path = format!("{prefix}/{name}");
                let mut label = name.clone();
                while let Some((next, grand)) = only_child(child) {
                    path.push('/');
                    path.push_str(next);
                    label.push('/');
                    label.push_str(next);
                    child = grand;
                }
                let full = title(&path);
                Node {
                    title: if top { full.clone() } else { label },
                    category: category(path.clone()),
                    tip: Some(full),
                    children: make(child, &path, false, category, title),
                }
            })
            .collect()
    }
    // 一番上（`base` の直下）は、枝分かれがなければそのまま下の節に降りる。
    let mut start = &root;
    let mut prefix = base.to_owned();
    while let Some((next, child)) = only_child(start) {
        prefix.push('/');
        prefix.push_str(next);
        start = child;
    }
    if prefix == base {
        return make(start, base, base.is_empty(), category, title);
    }
    // まとめた途中のフォルダを 1 つの節にする（リポジトリが散っていれば、その下を枝に）。
    let full = title(&prefix);
    let label = if base.is_empty() {
        full.clone()
    } else {
        prefix[base.len() + 1..].to_owned()
    };
    vec![Node {
        title: label,
        category: category(prefix.clone()),
        tip: Some(full),
        children: make(start, &prefix, false, category, title),
    }]
}

// 行の中身（Quick Open と共通）

fn esc(s: &str) -> String {
    glib::markup_escape_text(s).to_string()
}

/// 名前の行: 色ラベルの丸、お気に入りの ★、表示名（見つからなければ薄く）。
pub fn name_markup(location: &str) -> String {
    let bookmark = bookmarks::bookmark(location);
    let mut out = String::new();
    if let Some(color) = bookmarks::color(bookmark.as_ref().and_then(|b| b.color.as_deref())) {
        out.push_str(&format!("<span foreground=\"{color}\">●</span> "));
    }
    if bookmark.as_ref().is_some_and(|b| b.favorite) {
        out.push_str("★ ");
    }
    let name = esc(&bookmarks::display_name(location));
    if bookmarks::is_repository(location) {
        out.push_str(&format!("<b>{name}</b>"));
    } else {
        out.push_str(&format!("<b><span fgalpha=\"45%\">{name}</span></b>"));
    }
    out
}

/// 名前の下の行: パス（ホームの下は ~。見つからなければそう書く）。
pub fn path_text(location: &str) -> String {
    if amonhen_remote::location::is_remote(location) {
        location.to_owned()
    } else if bookmarks::is_repository(location) {
        bookmarks::tilde(location)
    } else {
        tr!("Not found — {path}", path = location)
    }
}

/// ブランチの列。
pub fn branch_text(location: &str) -> String {
    if amonhen_remote::location::is_remote(location) {
        return "—".into();
    }
    if !bookmarks::is_repository(location) {
        return String::new();
    }
    match bookmarks::peek(location) {
        Some(peek) => peek.branch.unwrap_or_else(|| "detached".into()),
        None => bookmarks::head_branch(location).unwrap_or_default(),
    }
}

/// 状態の列（マークアップ）と、その説明（ツールチップ）。`check` は変更も差もないときの ✓（一覧の列だけ）。
pub fn state_markup(location: &str, check: bool) -> (String, Option<String>) {
    if amonhen_remote::location::is_remote(location) {
        // SSH 先・daemon 越しは、開くまで状態を読まない。
        let text = if bookmarks::ssh_host(location).is_some() {
            "SSH"
        } else {
            "daemon"
        };
        return (
            format!("<span fgalpha=\"60%\">{text}</span>"),
            Some(tr!("The status isn’t read until you open it")),
        );
    }
    if !bookmarks::is_repository(location) {
        return (
            format!(
                "<span foreground=\"#e01b24\">{}</span>",
                esc(&tr!("Not found"))
            ),
            None,
        );
    }
    let peek = bookmarks::peek(location);
    let mut parts = Vec::new();
    let fetch_error = bookmarks::fetch_error(location);
    if fetch_error.is_some() {
        parts.push(tr!("Fetch failed"));
    }
    if bookmarks::is_fetching(location) {
        parts.push(tr!("Fetching…"));
    }
    if let Some(peek) = &peek {
        if peek.changed > 0 {
            parts.push(tr!("Changes {count}", count = peek.changed));
        }
        if peek.ahead > 0 {
            parts.push(format!("↑{}", peek.ahead));
        }
        if peek.behind > 0 {
            parts.push(format!("↓{}", peek.behind));
        }
        if parts.is_empty() && check {
            parts.push("✓".into());
        }
    }
    let color = if fetch_error.is_some() {
        Some("#e01b24")
    } else if peek.as_ref().is_some_and(|p| p.changed > 0) {
        Some("#e66100")
    } else {
        None
    };
    let text = esc(&parts.join("  "));
    let markup = match color {
        Some(c) => format!("<span foreground=\"{c}\">{text}</span>"),
        None => format!("<span fgalpha=\"60%\">{text}</span>"),
    };
    let mut tip = peek.map(|p| {
        [
            if p.changed > 0 {
                tr!(
                    "{count} files with changes (untracked files aren’t counted)",
                    count = p.changed
                )
            } else {
                tr!("No changes")
            },
            if p.upstream {
                tr!(
                    "{ahead} ahead of the upstream, {behind} behind",
                    ahead = p.ahead,
                    behind = p.behind
                )
            } else {
                tr!("No upstream")
            },
        ]
        .join("\n")
    });
    if let Some(e) = fetch_error {
        let head = tr!("Fetch failed: {error}", error = e);
        tip = Some(match tip {
            Some(t) => format!("{head}\n{t}"),
            None => head,
        });
    }
    (markup, tip)
}

/// 最後のコミットの列（いつ — 件名）とツールチップ（件名と日時）。
fn commit_text(location: &str) -> (String, Option<String>) {
    if amonhen_remote::location::is_remote(location) {
        return ("—".into(), None);
    }
    match bookmarks::peek(location).and_then(|p| p.last_commit) {
        Some((time, subject)) => {
            let date = glib::DateTime::from_unix_local(time)
                .and_then(|d| d.format("%Y-%m-%d %H:%M"))
                .map(|s| s.to_string())
                .unwrap_or_default();
            (
                format!("{} — {subject}", crate::stashes::ago(time)),
                Some(format!("{subject}\n{date}")),
            )
        }
        None => (String::new(), None),
    }
}

fn opened_text(location: &str) -> String {
    crate::repos::opened_at(location).map_or_else(|| "—".into(), crate::stashes::ago)
}

/// 列。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Column {
    Name,
    Branch,
    Status,
    Commit,
    Opened,
}

/// 表のセル（行の状態が届いたら、同じ場所のセルだけを描き直す）。
struct CellView {
    column: Column,
    title: gtk::Label,
    sub: gtk::Label,
    location: RefCell<String>,
}

impl CellView {
    fn render(&self) {
        let location = self.location.borrow().clone();
        match self.column {
            Column::Name => {
                self.title.set_markup(&name_markup(&location));
                self.sub.set_text(&path_text(&location));
                self.title.set_tooltip_text(Some(&location));
            }
            Column::Branch => self.title.set_text(&branch_text(&location)),
            Column::Status => {
                let (markup, tip) = state_markup(&location, true);
                self.title.set_markup(&markup);
                self.title.set_tooltip_text(tip.as_deref());
            }
            Column::Commit => {
                let (text, tip) = commit_text(&location);
                self.title.set_text(&text);
                self.title.set_tooltip_text(tip.as_deref());
            }
            Column::Opened => self.title.set_text(&opened_text(&location)),
        }
    }
}

pub struct Browser {
    /// ページの中身（タブに入れる）。
    pub root: gtk::Paned,
    open: Open,
    category: RefCell<Category>,
    side: gtk::ListBox,
    /// 左の行ごとの分類と見出し。
    side_rows: RefCell<Vec<Side>>,
    /// 開いている節の鍵。
    expanded: RefCell<std::collections::HashSet<String>>,
    rebuilding: Cell<bool>,
    filter: gtk::SearchEntry,
    model: gtk::StringList,
    selection: gtk::MultiSelection,
    view: gtk::ColumnView,
    cells: RefCell<Vec<Rc<CellView>>>,
    scanning: gtk::Spinner,
    add: gtk::Button,
}

thread_local! {
    /// 最後に作ったページ（確認用の `--menu-row`）。
    static BROWSER: RefCell<std::rc::Weak<Browser>> = RefCell::default();
}

/// `--repositories --menu-row <row> [--menu-item <title>]`: 行の右クリックのメニューを出すか、その項目を選ぶ
/// （確認用。下に開くメニューの中は ` > ` でつなぐ）。
pub fn debug_menu(row: u32, item: Option<String>) {
    let Some(b) = BROWSER.with(|w| w.borrow().upgrade()) else {
        return;
    };
    b.selection.select_item(row, true);
    let items = b.row_menu(row);
    match item {
        Some(title) => {
            if !menu::activate(&items, &title) {
                eprintln!("項目がない: {title}\n{}", menu::titles(&items).join("\n"));
            }
        }
        None => menu::popup(&b.view, 200.0, 140.0 + row as f64 * 52.0, items),
    }
}

/// リポジトリのページの中身を作る。`group` にページのアクション（Quick Open・Clone）を入れる。返したものは
/// ページを閉じるまで持っておく（ブックマークの知らせは、それを手放すと止まる）。
pub fn page(app: &gtk::Application, open: Open, group: &gio::SimpleActionGroup) -> Rc<Browser> {
    let browser = Browser::new(open);
    BROWSER.with(|w| *w.borrow_mut() = Rc::downgrade(&browser));
    browser.install_actions(app, group);
    bookmarks::refresh();
    bookmarks::forget_peeks();
    bookmarks::rescan();
    browser.reload();
    // ページを閉じるまで知らせを受ける。
    let weak = Rc::downgrade(&browser);
    bookmarks::connect(move |path| {
        let Some(b) = weak.upgrade() else {
            return false;
        };
        match path {
            Some(p) => b.row_changed(p),
            None => b.bookmarks_changed(),
        }
        true
    });
    browser
}

impl Browser {
    fn new(open: Open) -> Rc<Self> {
        // 左: 分類と、下の ＋。
        let side = gtk::ListBox::new();
        side.add_css_class("navigation-sidebar");
        side.set_selection_mode(gtk::SelectionMode::Single);
        let side_scrolled = gtk::ScrolledWindow::builder()
            .child(&side)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();
        let add = gtk::Button::with_label(&format!("＋ {}", tr!("Add")));
        add.add_css_class("flat");
        add.set_tooltip_text(Some(&tr!(
            "Add repositories, SSH connections, groups and folders to scan"
        )));
        let scanning = gtk::Spinner::new();
        scanning.set_visible(false);
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        bar.set_margin_start(6);
        bar.set_margin_end(6);
        bar.set_margin_top(4);
        bar.set_margin_bottom(6);
        bar.append(&add);
        bar.append(&scanning);
        let left = gtk::Box::new(gtk::Orientation::Vertical, 0);
        left.append(&side_scrolled);
        left.append(&bar);

        // 右: 絞り込みと一覧だけ（開くのはダブルクリックと Enter、加えるのは左の ＋ と右クリック）。
        let filter = gtk::SearchEntry::new();
        filter.set_placeholder_text(Some(&tr!("Filter by name or path")));
        filter.set_margin_top(10);
        filter.set_margin_bottom(8);
        filter.set_margin_start(12);
        filter.set_margin_end(12);
        let model = gtk::StringList::new(&[]);
        let view = gtk::ColumnView::new(None::<gtk::MultiSelection>);
        view.set_show_row_separators(false);
        view.set_reorderable(false);
        let sorted = gtk::SortListModel::new(Some(model.clone()), view.sorter());
        let selection = gtk::MultiSelection::new(Some(sorted));
        view.set_model(Some(&selection));
        let table = gtk::ScrolledWindow::builder()
            .child(&view)
            .vexpand(true)
            .hexpand(true)
            .build();
        let right = gtk::Box::new(gtk::Orientation::Vertical, 0);
        right.append(&filter);
        right.append(&table);

        let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
        paned.set_start_child(Some(&left));
        paned.set_end_child(Some(&right));
        paned.set_resize_start_child(false);
        paned.set_shrink_start_child(false);
        paned.set_position(240);

        let saved = crate::settings::string(CATEGORY_KEY)
            .and_then(|k| Category::from_key(&k))
            .unwrap_or(Category::All);
        let browser = Rc::new(Self {
            root: paned,
            open,
            category: RefCell::new(saved),
            side,
            side_rows: RefCell::default(),
            expanded: RefCell::new(saved_expanded()),
            rebuilding: Cell::new(false),
            filter,
            model,
            selection,
            view,
            cells: RefCell::default(),
            scanning,
            add,
        });
        browser.build_columns();
        browser.connect();
        browser
    }

    /// ページのある窓（ページは窓にある間にしか操作できない）。
    fn window(&self) -> gtk::Window {
        self.root.root().and_downcast().expect("ページが窓にない")
    }

    /// 絞り込みの欄に入る（ページを前に出したとき）。
    pub fn focus_filter(&self) {
        self.filter.grab_focus();
    }

    /// 列（名前とパス・ブランチ・状態・最後のコミット・開いた日時）。見出しを押すと並べ替える（既定は開いた順）。
    fn build_columns(self: &Rc<Self>) {
        let columns = [
            (Column::Name, tr!("Name"), None),
            (Column::Branch, tr!("Branch"), Some(130)),
            (Column::Status, tr!("Status"), Some(100)),
            (Column::Commit, tr!("Last Commit"), Some(260)),
            (Column::Opened, tr!("Opened"), Some(100)),
        ];
        let mut opened = None;
        for (kind, title, width) in columns {
            let factory = gtk::SignalListItemFactory::new();
            let weak = Rc::downgrade(self);
            factory.connect_setup(move |_, item| {
                let Some(b) = weak.upgrade() else { return };
                let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
                let title = gtk::Label::new(None);
                title.set_xalign(0.0);
                title.set_ellipsize(gtk::pango::EllipsizeMode::End);
                let sub = gtk::Label::new(None);
                sub.set_xalign(0.0);
                sub.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
                sub.add_css_class("caption");
                sub.add_css_class("dim-label");
                sub.set_visible(kind == Column::Name);
                if kind != Column::Name {
                    title.add_css_class("caption");
                }
                let cell = gtk::Box::new(gtk::Orientation::Vertical, 1);
                cell.set_valign(gtk::Align::Center);
                cell.set_height_request(36);
                cell.set_margin_start(4);
                cell.set_margin_end(4);
                cell.append(&title);
                cell.append(&sub);
                // 右クリックで、その行のメニュー（選んでいなければ選ぶ）。
                let click = gtk::GestureClick::new();
                click.set_button(gtk::gdk::BUTTON_SECONDARY);
                let (w, list_item) = (Rc::downgrade(&b), item.downgrade());
                click.connect_pressed(move |gesture, _, x, y| {
                    let (Some(b), Some(item)) = (w.upgrade(), list_item.upgrade()) else {
                        return;
                    };
                    let row = item.position();
                    if row == gtk::INVALID_LIST_POSITION {
                        return;
                    }
                    if !b.selection.is_selected(row) {
                        b.selection.select_item(row, true);
                    }
                    if let Some(widget) = gesture.widget() {
                        menu::popup(&widget, x, y, b.row_menu(row));
                    }
                });
                cell.add_controller(click);
                item.set_child(Some(&cell));
                let view = Rc::new(CellView {
                    column: kind,
                    title,
                    sub,
                    location: RefCell::default(),
                });
                // 消えたセルの分は、作るたびに片づける。
                let mut cells = b.cells.borrow_mut();
                cells.retain(|c| c.title.parent().is_some());
                cells.push(view);
            });
            let weak = Rc::downgrade(self);
            factory.connect_bind(move |_, item| {
                let Some(b) = weak.upgrade() else { return };
                let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
                let Some(location) = item
                    .item()
                    .and_downcast::<gtk::StringObject>()
                    .map(|s| s.string().to_string())
                else {
                    return;
                };
                // setup で作ったセル（見出しの文字の部品で見分ける）。
                let label = item
                    .child()
                    .and_then(|c| c.first_child())
                    .and_downcast::<gtk::Label>();
                let cell =
                    label.and_then(|l| b.cells.borrow().iter().find(|c| c.title == l).cloned());
                if let Some(cell) = cell {
                    *cell.location.borrow_mut() = location;
                    cell.render();
                }
            });
            let column = gtk::ColumnViewColumn::new(Some(&title), Some(factory));
            match width {
                Some(w) => {
                    column.set_fixed_width(w);
                    column.set_resizable(true);
                }
                None => column.set_expand(true),
            }
            column.set_sorter(Some(&sorter(kind, self.view.downgrade())));
            if kind == Column::Opened {
                opened = Some(column.clone());
            }
            self.view.append_column(&column);
        }
        self.view
            .sort_by_column(opened.as_ref(), gtk::SortType::Descending);
    }

    fn connect(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.side.connect_row_selected(move |_, row| {
            let (Some(b), Some(row)) = (weak.upgrade(), row) else {
                return;
            };
            if b.rebuilding.get() {
                return;
            }
            let chosen = b
                .side_rows
                .borrow()
                .get(row.index() as usize)
                .and_then(|s| s.category().cloned());
            if let Some(chosen) = chosen
                && chosen != *b.category.borrow()
            {
                crate::settings::set_string(CATEGORY_KEY, &chosen.key());
                *b.category.borrow_mut() = chosen;
                b.reload_table();
            }
        });
        // 左の右クリック: その分類のメニュー。
        let click = gtk::GestureClick::new();
        click.set_button(gtk::gdk::BUTTON_SECONDARY);
        let weak = Rc::downgrade(self);
        click.connect_pressed(move |_, _, x, y| {
            let Some(b) = weak.upgrade() else { return };
            let Some(row) = b.side.row_at_y(y as i32) else {
                return;
            };
            let index = row.index() as usize;
            let items = b.side_menu(index);
            menu::popup(&b.side, x, y, items);
        });
        self.side.add_controller(click);

        let weak = Rc::downgrade(self);
        self.add.connect_clicked(move |button| {
            let Some(b) = weak.upgrade() else { return };
            let h = button.height() as f64;
            menu::popup(button, 0.0, h, b.add_menu());
        });

        let weak = Rc::downgrade(self);
        self.filter.connect_search_changed(move |_| {
            if let Some(b) = weak.upgrade() {
                b.reload_table();
            }
        });
        // 絞り込みの欄から、↑↓ で行を選び、Enter で開く。
        let weak = Rc::downgrade(self);
        self.filter.connect_activate(move |_| {
            if let Some(b) = weak.upgrade() {
                let rows = b.selected_rows();
                if let Some(&row) = rows.first() {
                    b.open_row(row, true);
                } else if b.selection.n_items() > 0 {
                    b.open_row(0, true);
                }
            }
        });
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, _| {
            let Some(b) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let step = match key {
                gtk::gdk::Key::Down => 1i64,
                gtk::gdk::Key::Up => -1,
                _ => return glib::Propagation::Proceed,
            };
            let n = b.selection.n_items() as i64;
            if n == 0 {
                return glib::Propagation::Stop;
            }
            let current = b.selected_rows().first().map_or(-1, |&r| r as i64);
            let next = (current + step).clamp(0, n - 1) as u32;
            b.selection.select_item(next, true);
            b.view
                .scroll_to(next, None, gtk::ListScrollFlags::NONE, None);
            glib::Propagation::Stop
        });
        self.filter.add_controller(keys);
        // ダブルクリック・Enter で開く。
        let weak = Rc::downgrade(self);
        self.view.connect_activate(move |_, row| {
            if let Some(b) = weak.upgrade() {
                b.open_row(row, true);
            }
        });
    }

    /// ページのアクション（Quick Open・Clone）。窓のメニューから来るショートカットは、このページにも効く。
    fn install_actions(self: &Rc<Self>, app: &gtk::Application, group: &gio::SimpleActionGroup) {
        let quick = gio::SimpleAction::new("quick-open", None);
        let weak = Rc::downgrade(self);
        quick.connect_activate(move |_, _| {
            if let Some(b) = weak.upgrade() {
                crate::quickopen::show(Some(&b.window()), b.open.clone());
            }
        });
        group.add_action(&quick);
        let clone = gio::SimpleAction::new("clone", None);
        let weak = Rc::downgrade(self);
        clone.connect_activate(move |_, _| {
            if let Some(b) = weak.upgrade() {
                b.clone_repository();
            }
        });
        group.add_action(&clone);
        // Ctrl+Shift+R: このページにいれば、絞り込みの欄へ。
        let repositories = gio::SimpleAction::new("repositories", None);
        let weak = Rc::downgrade(self);
        repositories.connect_activate(move |_, _| {
            if let Some(b) = weak.upgrade() {
                b.focus_filter();
            }
        });
        group.add_action(&repositories);
        let settings = gio::SimpleAction::new("settings", None);
        let weak = Rc::downgrade(self);
        settings.connect_activate(move |_, _| {
            if let Some(w) = weak.upgrade().and_then(|b| b.window().downcast().ok()) {
                crate::settings::show(&w);
            }
        });
        group.add_action(&settings);
        // 起動したときにこのページしかなければ、ショートカットはまだ登録されていない。
        for (action, accel) in [
            ("win.quick-open", "<Control><Shift>o"),
            ("win.clone", "<Control><Alt>c"),
            ("win.repositories", "<Control><Shift>r"),
            ("win.settings", "<Control>comma"),
        ] {
            if app.accels_for_action(action).is_empty() {
                app.set_accels_for_action(action, &[accel]);
            }
        }
    }

    // 作り直す

    fn bookmarks_changed(self: &Rc<Self>) {
        self.scanning.set_visible(bookmarks::is_scanning());
        self.scanning.set_spinning(bookmarks::is_scanning());
        self.reload();
    }

    /// 行の状態が変わった。その場所のセルだけ描き直す（並びは変えない。読めるたびに行が動くと選びにくい）。
    fn row_changed(&self, location: &str) {
        for cell in self.cells.borrow().iter() {
            if *cell.location.borrow() == location {
                cell.render();
            }
        }
    }

    fn reload(self: &Rc<Self>) {
        self.reload_sidebar();
        self.reload_table();
    }

    /// 左の木: すべて・お気に入り、場所（このコンピュータ・SSH のホストごと。その下にスキャンするフォルダ（🔍。その下は
    /// フォルダの階層）と、それ以外のリポジトリ（開いた・手で入れたもの）のあるフォルダの木）、グループ。
    fn side_tree(&self) -> Vec<(Side, Vec<Node>)> {
        let data = bookmarks::data();
        let all = bookmarks::all_locations();
        let roots = Roots::new(&data);
        let node = |title: String, category: Category, children: Vec<Node>| Node {
            title,
            category,
            tip: None,
            children,
        };
        // スキャンするフォルダ（🔍）と、その中のフォルダの木。
        let scan_nodes = |host: Option<&str>| -> Vec<Node> {
            data.scan_roots
                .iter()
                .filter(|r| bookmarks::ssh_host(&r.path).as_deref() == host)
                .map(|root| {
                    let base = root_base(&root.path);
                    let found: Vec<String> = bookmarks::scanned(&root.path)
                        .iter()
                        .map(|l| bookmarks::path_part(l))
                        .collect();
                    // SSH 先は `~` のまま探すので、見つけたものが絶対パスで返ることがある。そのときは根から木にする。
                    let inside = format!("{base}/");
                    let under = found.iter().all(|p| p.starts_with(&inside));
                    let r = root.path.clone();
                    let children = folder_tree(
                        &found,
                        if under { &base } else { "" },
                        &|p| Category::ScanFolder(r.clone(), p),
                        &|p| match host {
                            Some(_) => remote_title(p),
                            None => bookmarks::tilde(p),
                        },
                    );
                    let title = match host {
                        Some(_) => remote_title(&bookmarks::path_part(&root.path)),
                        None => bookmarks::tilde(&base),
                    };
                    Node {
                        tip: Some(tr!(
                            "Searches {path}",
                            path = host.map(|h| format!("{h}:")).unwrap_or_default() + &title
                        )),
                        title: format!("🔍 {title}"),
                        category: Category::Scan(root.path.clone()),
                        children,
                    }
                })
                .collect()
        };
        let outside = |host: Option<&str>| -> Vec<String> {
            all.iter()
                .filter(|l| bookmarks::ssh_host(l).as_deref() == host && !roots.contains(l))
                .map(|l| bookmarks::path_part(l))
                .collect()
        };
        let mut local = scan_nodes(None);
        local.extend(folder_tree(
            &outside(None),
            "",
            &Category::Folder,
            &bookmarks::tilde,
        ));
        let mut places = vec![node(tr!("This Computer"), Category::Local, local)];
        let mut hosts: Vec<String> = all
            .iter()
            .filter_map(|l| bookmarks::ssh_host(l))
            .chain(
                data.scan_roots
                    .iter()
                    .filter_map(|r| bookmarks::ssh_host(&r.path)),
            )
            .collect();
        hosts.sort();
        hosts.dedup();
        for host in hosts {
            let mut children = scan_nodes(Some(&host));
            let h = host.clone();
            children.extend(folder_tree(
                &outside(Some(&host)),
                "",
                &move |p| Category::HostFolder(h.clone(), p),
                &remote_title,
            ));
            places.push(node(host.clone(), Category::Host(host), children));
        }
        let mut tree = vec![
            (
                Side::Category(Category::All),
                vec![node(tr!("All"), Category::All, Vec::new())],
            ),
            (
                Side::Category(Category::Favorites),
                vec![node(tr!("Favorites"), Category::Favorites, Vec::new())],
            ),
            (Side::Places, places),
        ];
        // グループは作ってから出す（空の見出しを並べない）。
        if !data.groups.is_empty() {
            tree.push((
                Side::Groups,
                data.groups
                    .iter()
                    .map(|g| node(g.clone(), Category::Group(g.clone()), Vec::new()))
                    .collect(),
            ));
        }
        tree
    }

    /// 左: [`Self::side_tree`] を、開いた節の中だけ並べる（選んでいる分類が閉じた節の中なら、そこまで開く）。
    fn reload_sidebar(self: &Rc<Self>) {
        let tree = self.side_tree();
        /// `category` の節までの道の、上の節の鍵。
        fn path_to(category: &Category, nodes: &[Node]) -> Option<Vec<String>> {
            for n in nodes {
                if n.category == *category {
                    return Some(Vec::new());
                }
                if let Some(mut rest) = path_to(category, &n.children) {
                    rest.insert(0, n.category.key());
                    return Some(rest);
                }
            }
            None
        }
        let found = tree
            .iter()
            .find_map(|(_, nodes)| path_to(&self.category.borrow(), nodes));
        match found {
            Some(ancestors) => self.expanded.borrow_mut().extend(ancestors),
            // 消えたグループ・フォルダを選んでいたら、すべてに戻す。
            None => *self.category.borrow_mut() = Category::All,
        }
        let mut rows: Vec<SideRow> = Vec::new();
        fn flatten(
            nodes: &[Node],
            depth: i32,
            expanded: &std::collections::HashSet<String>,
            rows: &mut Vec<SideRow>,
        ) {
            for n in nodes {
                rows.push(SideRow {
                    side: Side::Category(n.category.clone()),
                    title: n.title.clone(),
                    depth,
                    tip: n.tip.clone(),
                    parent: !n.children.is_empty(),
                });
                if expanded.contains(&n.category.key()) {
                    flatten(&n.children, depth + 1, expanded, rows);
                }
            }
        }
        for (side, nodes) in &tree {
            let title = match side {
                Side::Places => Some(tr!("Places")),
                Side::Groups => Some(tr!("Groups")),
                Side::Category(_) => None,
            };
            if let Some(title) = title {
                rows.push(SideRow {
                    side: side.clone(),
                    title,
                    depth: 0,
                    tip: None,
                    parent: false,
                });
            }
            flatten(nodes, 0, &self.expanded.borrow(), &mut rows);
        }
        self.rebuilding.set(true);
        self.side.remove_all();
        let current = self.category.borrow().clone();
        let mut selected = None;
        for r in &rows {
            let row = gtk::ListBoxRow::new();
            let line = gtk::Box::new(gtk::Orientation::Horizontal, 2);
            let label = gtk::Label::new(Some(&r.title));
            label.set_xalign(0.0);
            label.set_hexpand(true);
            label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            match r.side.category() {
                None => {
                    // 見出し。
                    line.append(&label);
                    label.add_css_class("caption-heading");
                    label.add_css_class("dim-label");
                    line.set_margin_top(8);
                    row.set_selectable(false);
                    row.set_activatable(false);
                }
                Some(c) => {
                    line.set_margin_start(r.depth * 14);
                    // 開く・閉じる（子のない節は、同じ幅を空ける）。
                    let open = self.expanded.borrow().contains(&c.key());
                    let disclosure = gtk::Image::from_icon_name(if open {
                        "pan-down-symbolic"
                    } else {
                        "pan-end-symbolic"
                    });
                    disclosure.add_css_class("dim-label");
                    disclosure.set_size_request(16, -1);
                    if r.parent {
                        let click = gtk::GestureClick::new();
                        let (w, key) = (Rc::downgrade(self), c.key());
                        click.connect_released(move |gesture, _, _, _| {
                            gesture.set_state(gtk::EventSequenceState::Claimed);
                            if let Some(b) = w.upgrade() {
                                b.toggle_expanded(&key);
                            }
                        });
                        disclosure.add_controller(click);
                    } else {
                        disclosure.set_opacity(0.0);
                    }
                    line.append(&disclosure);
                    line.append(&label);
                    let count = gtk::Label::new(Some(&self.count_text(c)));
                    count.add_css_class("dim-label");
                    count.add_css_class("caption");
                    line.append(&count);
                    row.set_tooltip_text(r.tip.as_deref());
                }
            }
            row.set_child(Some(&line));
            self.side.append(&row);
            if r.side.category() == Some(&current) {
                selected = Some(row);
            }
        }
        self.side.select_row(selected.as_ref());
        *self.side_rows.borrow_mut() = rows.into_iter().map(|r| r.side).collect();
        self.rebuilding.set(false);
    }

    /// 節を開く・閉じる（覚えておく）。
    fn toggle_expanded(self: &Rc<Self>, key: &str) {
        {
            let mut expanded = self.expanded.borrow_mut();
            if !expanded.remove(key) {
                expanded.insert(key.to_owned());
            }
            let mut keys: Vec<&str> = expanded.iter().map(String::as_str).collect();
            keys.sort();
            crate::settings::set_string(EXPANDED_KEY, &keys.join("\n"));
        }
        self.reload_sidebar();
    }

    fn count_text(&self, category: &Category) -> String {
        if let Category::Scan(root) = category {
            if bookmarks::is_scanning() {
                return "…".into();
            }
            if bookmarks::scan_error(root).is_some() {
                return "⚠︎".into();
            }
        }
        paths_in(category).len().to_string()
    }

    /// 右の一覧を作り直す（選んでいたものは選び直す）。
    fn reload_table(&self) {
        let selected: Vec<String> = self
            .selected_rows()
            .iter()
            .filter_map(|&r| self.location(r))
            .collect();
        let query = self.filter.text().trim().to_lowercase();
        let list: Vec<String> = paths_in(&self.category.borrow())
            .into_iter()
            .filter(|l| {
                query.is_empty()
                    || bookmarks::display_name(l).to_lowercase().contains(&query)
                    || l.to_lowercase().contains(&query)
            })
            .collect();
        let refs: Vec<&str> = list.iter().map(String::as_str).collect();
        self.model.splice(0, self.model.n_items(), &refs);
        self.selection.unselect_all();
        let mut any = false;
        if query.is_empty() {
            for row in 0..self.selection.n_items() {
                if self.location(row).is_some_and(|l| selected.contains(&l)) {
                    self.selection.select_item(row, false);
                    any = true;
                }
            }
        }
        if !any && self.selection.n_items() > 0 {
            self.selection.select_item(0, true);
        }
    }

    fn location(&self, row: u32) -> Option<String> {
        self.selection
            .item(row)
            .and_downcast::<gtk::StringObject>()
            .map(|s| s.string().to_string())
    }

    fn selected_rows(&self) -> Vec<u32> {
        let set = self.selection.selection();
        (0..set.size() as u32).map(|i| set.nth(i)).collect()
    }

    // 開く

    /// 開く。`close` ならこのタブがそのリポジトリになり（macOS 版と同じ）、でなければ新しい窓に開く。
    fn open_row(self: &Rc<Self>, row: u32, close: bool) {
        if let Some(location) = self.location(row) {
            self.open_location(&location, close);
        }
    }

    fn open_location(self: &Rc<Self>, location: &str, close: bool) {
        if !bookmarks::is_repository(location) {
            let (b, location) = (self.clone(), location.to_owned());
            glib::spawn_future_local(async move {
                let sheet = Sheet::new(
                    Some(&b.window()),
                    &tr!(
                        "{name} not found",
                        name = bookmarks::display_name(&location)
                    ),
                    &tr!("{path}\nRemove it from the list?", path = location),
                );
                sheet.buttons(&tr!("Remove"), false, None);
                if sheet.run().await == Response::Ok {
                    crate::repos::forget(&location);
                    bookmarks::remove(&location);
                    b.reload();
                }
            });
            return;
        }
        let place = if close {
            crate::tabs::Place::Tab
        } else {
            crate::tabs::Place::NewWindow
        };
        (self.open)(PathBuf::from(location), place);
    }

    // メニュー

    fn targets(&self, row: u32) -> Vec<String> {
        let rows = if self.selection.is_selected(row) {
            self.selected_rows()
        } else {
            vec![row]
        };
        rows.iter().filter_map(|&r| self.location(r)).collect()
    }

    /// 行の右クリック（macOS 版の `rowMenu`）。
    fn row_menu(self: &Rc<Self>, row: u32) -> Vec<Item> {
        let Some(path) = self.location(row) else {
            return Vec::new();
        };
        let targets = self.targets(row);
        let bookmark = bookmarks::bookmark(&path);
        let mut items = Vec::new();
        let (b, p) = (self.clone(), path.clone());
        items.push(menu::action(tr!("Open"), move || b.open_location(&p, true)));
        let (b, p) = (self.clone(), path.clone());
        items.push(menu::action(tr!("Open in New Window"), move || {
            b.open_location(&p, false)
        }));
        if !path.starts_with("daemon:")
            && !path.starts_with("ssh://")
            && !crate::settings::bool(crate::settings::OPEN_THROUGH_DAEMON)
        {
            let (b, p) = (self.clone(), path.clone());
            items.push(menu::action(
                tr!("Open Through the Shared daemon"),
                move || b.open_location(&format!("daemon:{p}"), true),
            ));
        }
        items.push(Item::Separator);
        let t = targets.clone();
        let title = if targets.len() > 1 {
            crate::titles::fetch_several(targets.len())
        } else {
            "Fetch".to_owned()
        };
        items.push(menu::action(title, move || bookmarks::fetch(&t)));
        items.push(Item::Separator);
        let p = path.clone();
        let favorite = bookmark.as_ref().is_some_and(|b| b.favorite);
        items.push(menu::action(
            if favorite {
                tr!("Remove from Favorites")
            } else {
                tr!("Add to Favorites")
            },
            move || bookmarks::toggle_favorite(&p),
        ));
        let mut groups = Vec::new();
        let t = targets.clone();
        groups.push(menu::action(tr!("None"), move || {
            for p in &t {
                bookmarks::move_to(p, None);
            }
        }));
        for g in bookmarks::data().groups {
            let t = targets.clone();
            groups.push(menu::action(g.clone(), move || {
                for p in &t {
                    bookmarks::move_to(p, Some(&g));
                }
            }));
        }
        groups.push(Item::Separator);
        let (b, t) = (self.clone(), targets.clone());
        groups.push(menu::action(tr!("New Group…"), move || {
            let (b, t) = (b.clone(), t.clone());
            glib::spawn_future_local(async move {
                if let Some(name) = b.ask(&tr!("New Group"), &tr!("Group name"), "").await {
                    bookmarks::add_group(&name);
                    for p in &t {
                        bookmarks::move_to(p, Some(&name));
                    }
                }
            });
        }));
        items.push(Item::Submenu(tr!("Group"), groups));
        let mut colors = Vec::new();
        let t = targets.clone();
        colors.push(menu::action(tr!("None"), move || {
            for p in &t {
                bookmarks::set_color(p, None);
            }
        }));
        for (name, title, _) in bookmarks::COLORS {
            let t = targets.clone();
            colors.push(menu::action(
                format!("● {}", amonhen_model::i18n::translate(title)),
                move || {
                    for p in &t {
                        bookmarks::set_color(p, Some(name));
                    }
                },
            ));
        }
        items.push(Item::Submenu(tr!("Color Label"), colors));
        let (b, p) = (self.clone(), path.clone());
        items.push(menu::action(tr!("Rename…"), move || {
            let (b, p) = (b.clone(), p.clone());
            glib::spawn_future_local(async move {
                let current = bookmarks::display_name(&p);
                // 空にするとフォルダの名前に戻す。
                if let Some(name) = b
                    .ask_allowing_empty(
                        &tr!("Rename"),
                        &tr!("Name shown in the list (leave empty for the folder name)"),
                        &current,
                    )
                    .await
                {
                    bookmarks::rename(&p, Some(name));
                }
            });
        }));
        if !amonhen_remote::location::is_remote(&path) {
            items.push(Item::Separator);
            let (b, p) = (self.clone(), path.clone());
            items.push(menu::action(tr!("Show in Files"), move || {
                crate::appmenu::reveal(&b.window(), std::path::Path::new(&p))
            }));
            let (b, p) = (self.clone(), path.clone());
            items.push(menu::action(tr!("Open in Terminal"), move || {
                if !crate::appmenu::terminal_at(std::path::Path::new(&p)) {
                    b.root.error_bell();
                }
            }));
        }
        let category = self.category.borrow().clone();
        let remove = match category {
            Category::All
            | Category::Local
            | Category::Host(_)
            | Category::Folder(_)
            | Category::HostFolder(..) => Some(tr!("Remove from List")),
            Category::Favorites => Some(tr!("Remove from Favorites")),
            Category::Group(_) => Some(tr!("Remove from Group")),
            Category::Scan(_) | Category::ScanFolder(..) => None,
        };
        if let Some(title) = remove {
            items.push(Item::Separator);
            let b = self.clone();
            items.push(menu::action(title, move || b.remove(&targets)));
        }
        items
    }

    /// 今の分類から外す: すべて・場所なら一覧から（ブックマークと開いた記録を消す）、お気に入りなら印を外す、
    /// グループならグループから出す。スキャンで見つけたものは外せない（フォルダを外す）。
    fn remove(self: &Rc<Self>, paths: &[String]) {
        let category = self.category.borrow().clone();
        for path in paths {
            match &category {
                Category::All
                | Category::Local
                | Category::Host(_)
                | Category::Folder(_)
                | Category::HostFolder(..) => {
                    crate::repos::forget(path);
                    bookmarks::remove(path);
                }
                Category::Favorites => bookmarks::set_favorite(path, false),
                Category::Group(_) => bookmarks::move_to(path, None),
                Category::Scan(_) | Category::ScanFolder(..) => self.root.error_bell(),
            }
        }
        self.reload();
    }

    /// 左の右クリック（macOS 版の `sidebarMenu`）。
    fn side_menu(self: &Rc<Self>, index: usize) -> Vec<Item> {
        let rows = self.side_rows.borrow().clone();
        let mut items = Vec::new();
        let target = match rows.get(index) {
            Some(Side::Category(c)) => c.clone(),
            // 見出し: そこに加えるもの。
            Some(Side::Groups) => {
                let b = self.clone();
                items.push(menu::action(tr!("New Group…"), move || b.new_group()));
                return items;
            }
            Some(Side::Places) => return self.place_items(),
            None => return items,
        };
        let paths = paths_in(&target);
        if !paths.is_empty() {
            items.push(menu::action(
                crate::titles::fetch_all(paths.len()),
                move || bookmarks::fetch(&paths),
            ));
            items.push(Item::Separator);
        }
        match &target {
            Category::Host(name) => {
                let (b, h) = (self.clone(), name.clone());
                items.push(menu::action(
                    tr!("Add Repository on This Host…"),
                    move || b.add_remote(Some(&h)),
                ));
                let (b, h) = (self.clone(), name.clone());
                items.push(menu::action(
                    tr!("Add Folder to Scan on This Host…"),
                    move || b.add_remote_scan_folder(Some(&h)),
                ));
                items.push(Item::Separator);
                let (b, h) = (self.clone(), name.clone());
                items.push(menu::action(
                    crate::titles::remove_from_list(name),
                    move || b.remove_host(&h),
                ));
            }
            Category::Local => {
                let b = self.clone();
                items.push(menu::action(tr!("Add Local Repository…"), move || {
                    b.add_repository()
                }));
                let b = self.clone();
                items.push(menu::action(tr!("Add Folder to Scan…"), move || {
                    b.add_scan_folder()
                }));
            }
            Category::Group(name) => {
                let (b, n) = (self.clone(), name.clone());
                items.push(menu::action(tr!("Rename…"), move || {
                    let (b, n) = (b.clone(), n.clone());
                    glib::spawn_future_local(async move {
                        if let Some(new) = b.ask(&tr!("Rename Group"), &tr!("Group name"), &n).await
                        {
                            bookmarks::rename_group(&n, &new);
                            *b.category.borrow_mut() = Category::Group(new);
                            b.reload();
                        }
                    });
                }));
                let n = name.clone();
                items.push(menu::action(tr!("Delete Group"), move || {
                    bookmarks::remove_group(&n)
                }));
            }
            Category::Scan(path) => {
                if let Some(error) = bookmarks::scan_error(path) {
                    items.push(menu::action(
                        tr!("Couldn’t search: {error}", error = error),
                        || {},
                    ));
                    items.push(Item::Separator);
                }
                let mut depths = Vec::new();
                for d in 1..=6u32 {
                    let p = path.clone();
                    depths.push(menu::action(d.to_string(), move || {
                        bookmarks::set_depth(&p, d)
                    }));
                }
                items.push(Item::Submenu(tr!("Search Depth"), depths));
                items.push(menu::action(tr!("Search Again"), bookmarks::rescan));
                if bookmarks::ssh_host(path).is_none() {
                    let (b, p) = (self.clone(), path.clone());
                    items.push(menu::action(tr!("Show in Files"), move || {
                        crate::appmenu::reveal(&b.window(), &bookmarks::expand(&p))
                    }));
                }
                let p = path.clone();
                items.push(menu::action(tr!("Stop Scanning"), move || {
                    bookmarks::remove_scan_root(&p)
                }));
            }
            Category::All
            | Category::Favorites
            | Category::Folder(_)
            | Category::HostFolder(..)
            | Category::ScanFolder(..) => {}
        }
        items
    }

    /// 場所の見出しの、加える項目。
    fn place_items(self: &Rc<Self>) -> Vec<Item> {
        let mut items = Vec::new();
        let b = self.clone();
        items.push(menu::action(tr!("Add Local Repository…"), move || {
            b.add_repository()
        }));
        let b = self.clone();
        items.push(menu::action(tr!("Add SSH Connection…"), move || {
            b.add_remote(None)
        }));
        let b = self.clone();
        items.push(menu::action(tr!("Add Folder to Scan…"), move || {
            b.add_scan_folder()
        }));
        let b = self.clone();
        items.push(menu::action(
            tr!("Add Folder to Scan over SSH…"),
            move || b.add_remote_scan_folder(None),
        ));
        items
    }

    /// 左の下の ＋（macOS 版と同じ並び）。
    fn add_menu(self: &Rc<Self>) -> Vec<Item> {
        let mut items = Vec::new();
        let b = self.clone();
        items.push(menu::action(tr!("Local Repository…"), move || {
            b.add_repository()
        }));
        let b = self.clone();
        items.push(menu::action("Clone…", move || b.clone_repository()));
        let b = self.clone();
        items.push(menu::action(tr!("SSH Connection…"), move || {
            b.add_remote(None)
        }));
        items.push(Item::Separator);
        let b = self.clone();
        items.push(menu::action(tr!("Group…"), move || b.new_group()));
        let b = self.clone();
        items.push(menu::action(tr!("Folder to Scan…"), move || {
            b.add_scan_folder()
        }));
        let b = self.clone();
        items.push(menu::action(tr!("Folder to Scan over SSH…"), move || {
            b.add_remote_scan_folder(None)
        }));
        items
    }

    // 加える

    /// 左で手で作ったグループを選んでいれば、その名前（加えるものをそこに入れる）。
    fn selected_group(&self) -> Option<String> {
        match &*self.category.borrow() {
            Category::Group(g) => Some(g.clone()),
            _ => None,
        }
    }

    /// 手元のリポジトリを選んで加える（列で選ぶシート。すぐ行ける所はスキャンするフォルダ）。
    fn add_repository(self: &Rc<Self>) {
        let places = bookmarks::data()
            .scan_roots
            .iter()
            .filter(|r| bookmarks::ssh_host(&r.path).is_none())
            .map(|r| bookmarks::tilde(&r.path))
            .collect();
        let request = Request {
            title: tr!("Add Repositories"),
            message: tr!("Choose a repository to add to the list (green)."),
            kind: Kind::Repository,
            initial: crate::settings::string(LAST_LOCAL_FOLDER_KEY).unwrap_or_else(|| "~".into()),
            places,
        };
        let b = self.clone();
        glib::spawn_future_local(async move {
            let Some(chosen) = filepicker::choose(Some(&b.window()), request, Source::Local).await
            else {
                return;
            };
            if let Some(parent) = std::path::Path::new(&chosen.path).parent() {
                crate::settings::set_string(LAST_LOCAL_FOLDER_KEY, &parent.to_string_lossy());
            }
            if bookmarks::is_repository(&chosen.path) {
                bookmarks::add(&chosen.path, b.selected_group().as_deref());
            } else {
                b.root.error_bell();
            }
        });
    }

    fn clone_repository(self: &Rc<Self>) {
        let b = self.clone();
        // clone したものはブックマークにも入れて開く（macOS 版と同じ）。
        let open: Rc<dyn Fn(PathBuf)> = Rc::new(move |path: PathBuf| {
            bookmarks::add(&path.to_string_lossy(), b.selected_group().as_deref());
            b.open_location(&path.to_string_lossy(), true);
        });
        crate::repos::clone(&self.window(), open);
    }

    fn add_scan_folder(self: &Rc<Self>) {
        let request = Request {
            title: tr!("Add a Folder to Scan"),
            message: tr!("Choose a folder to search for repositories (up to 3 levels deep)."),
            kind: Kind::Folder,
            initial: "~".into(),
            places: Vec::new(),
        };
        let b = self.clone();
        glib::spawn_future_local(async move {
            if let Some(chosen) =
                filepicker::choose(Some(&b.window()), request, Source::Local).await
            {
                bookmarks::add_scan_root(&chosen.path);
            }
        });
    }

    /// SSH 先のリポジトリを加えて開く。`host` があれば、そのホストの場所を最初に入れておく。Choose… で、
    /// そのホストのフォルダを列で見て選べる。
    fn add_remote(self: &Rc<Self>, host: Option<&str>) {
        let mut initial = host.map_or_else(|| "ssh://".to_owned(), |h| format!("ssh://{h}/"));
        let b = self.clone();
        glib::spawn_future_local(async move {
            loop {
                let sheet = Sheet::new(
                    Some(&b.window()),
                    &tr!("Add SSH Connection"),
                    &tr!(
                        "ssh://[user@]host[:port]/path (a daemon on an SSH host) or daemon:/path (the local shared daemon)"
                    ),
                );
                let entry = gtk::Entry::builder()
                    .text(&initial)
                    .activates_default(true)
                    .hexpand(true)
                    .build();
                sheet.add(&entry);
                let ok = sheet.buttons("OK", false, Some(&tr!("Choose…")));
                // ホストとパスがそろうまで OK を押せない（macOS 版と同じ）。
                let check = move |e: &gtk::Entry| {
                    ok.set_sensitive(bookmarks::is_connection(e.text().trim()));
                };
                check(&entry);
                entry.connect_changed(check);
                entry.grab_focus();
                let response = sheet.run().await;
                let location = entry.text().trim().to_owned();
                match response {
                    Response::Cancel => return,
                    Response::Other => {
                        // ホストのフォルダを見て選ぶ（選んだら、欄に入れて聞き直す）。
                        let Some(host) = bookmarks::ssh_host(&location).filter(|h| !h.is_empty())
                        else {
                            b.root.error_bell();
                            initial = location;
                            continue;
                        };
                        let path = bookmarks::path_part(&location);
                        let path = path
                            .strip_prefix('/')
                            .filter(|p| p.starts_with('~'))
                            .map_or(path.clone(), str::to_owned);
                        let request = Request {
                            title: tr!("Choose a Repository on {host}", host = host),
                            message: tr!(
                                "Click a folder to open it, and choose a repository (green)."
                            ),
                            kind: Kind::Folder,
                            initial: if path.is_empty() { "~".into() } else { path },
                            places: Vec::new(),
                        };
                        initial = match filepicker::choose(
                            Some(&b.window()),
                            request,
                            Source::Host(host),
                        )
                        .await
                        {
                            Some(chosen) => chosen.ssh_location(),
                            None => location,
                        };
                    }
                    Response::Ok => {
                        // Enter は押せない OK でも届くので、ここでも確かめる（書いたものは残して聞き直す）。
                        if !bookmarks::is_connection(&location) {
                            b.root.error_bell();
                            initial = location;
                            continue;
                        }
                        bookmarks::add(&location, b.selected_group().as_deref());
                        b.open_location(&location, true);
                        return;
                    }
                }
            }
        });
    }

    /// SSH 先でスキャンするフォルダを加える（そのホストの上で、エンジンが探す）。`host` がなければ、シートで
    /// ホストも選ぶ。
    fn add_remote_scan_folder(self: &Rc<Self>, host: Option<&str>) {
        let b = self.clone();
        let host = host.map(str::to_owned);
        glib::spawn_future_local(async move {
            if let Some(location) = choose_remote_scan_folder(&b.window(), host, None).await {
                bookmarks::add_scan_root(&location);
                if let Some(h) = bookmarks::ssh_host(&location) {
                    *b.category.borrow_mut() = Category::Host(h);
                    b.reload();
                }
            }
        });
    }

    fn new_group(self: &Rc<Self>) {
        let b = self.clone();
        glib::spawn_future_local(async move {
            if let Some(name) = b.ask(&tr!("New Group"), &tr!("Group name"), "").await {
                bookmarks::add_group(&name);
                *b.category.borrow_mut() = Category::Group(name);
                b.reload();
            }
        });
    }

    /// 接続先ごと一覧から外す（そのホストのリポジトリと、スキャンするフォルダ）。
    fn remove_host(self: &Rc<Self>, name: &str) {
        let b = self.clone();
        let name = name.to_owned();
        glib::spawn_future_local(async move {
            let paths = paths_in(&Category::Host(name.clone()));
            let roots: Vec<String> = bookmarks::data()
                .scan_roots
                .into_iter()
                .map(|r| r.path)
                .filter(|p| bookmarks::ssh_host(p).as_ref() == Some(&name))
                .collect();
            let sheet = Sheet::new(
                Some(&b.window()),
                &tr!("Remove {host}?", host = name),
                &tr!(
                    "This removes its {repositories} repositories and {folders} folders to scan from the list. Nothing on the host is touched.",
                    repositories = paths.len(),
                    folders = roots.len()
                ),
            );
            sheet.buttons(&tr!("Delete"), false, None);
            if sheet.run().await != Response::Ok {
                return;
            }
            for p in &paths {
                crate::repos::forget(p);
                bookmarks::remove(p);
            }
            for r in &roots {
                bookmarks::remove_scan_root(r);
            }
            *b.category.borrow_mut() = Category::All;
            b.reload();
        });
    }

    /// 文字を入れてもらう。キャンセルか空なら None。
    async fn ask(&self, title: &str, message: &str, initial: &str) -> Option<String> {
        self.ask_allowing_empty(title, message, initial)
            .await
            .filter(|s| !s.is_empty())
    }

    async fn ask_allowing_empty(
        &self,
        title: &str,
        message: &str,
        initial: &str,
    ) -> Option<String> {
        let sheet = Sheet::new(Some(&self.window()), title, message);
        let entry = gtk::Entry::builder()
            .text(initial)
            .activates_default(true)
            .hexpand(true)
            .build();
        sheet.add(&entry);
        sheet.buttons("OK", false, None);
        entry.grab_focus();
        (sheet.run().await == Response::Ok).then(|| entry.text().trim().to_owned())
    }
}

/// SSH 先でスキャンするフォルダを列で選んでもらい、その場所（`ssh://host/~/src`）を返す。`host` がなければ、
/// シートでホストも選ぶ（最後に選んだホストから）。Settings > SSH Hosts の Connection と共通。
pub async fn choose_remote_scan_folder(
    parent: &gtk::Window,
    host: Option<String>,
    repo: Option<std::sync::Arc<amonhen_core::Repository>>,
) -> Option<String> {
    let request = Request {
        title: match &host {
            Some(h) => tr!("Add a Folder to Scan on {host}", host = h),
            None => tr!("Add a Folder to Scan over SSH"),
        },
        message: tr!("The engine on the host searches up to 3 levels below the folder."),
        kind: Kind::Folder,
        initial: "~/prj".into(),
        places: Vec::new(),
    };
    let source = match (host, repo) {
        (Some(h), Some(repo)) => Source::Connected(h, repo),
        (Some(h), None) => Source::Host(h),
        (None, _) => Source::AnyHost(crate::settings::string(LAST_HOST_KEY).unwrap_or_default()),
    };
    let chosen = filepicker::choose(Some(parent), request, source).await?;
    if chosen.host.is_empty() {
        return None;
    }
    crate::settings::set_string(LAST_HOST_KEY, &chosen.host);
    Some(chosen.ssh_location())
}

/// 列の並べ替え（同じ値なら名前の順。並びが毎回変わらないように）。
fn sorter(column: Column, view: glib::WeakRef<gtk::ColumnView>) -> gtk::CustomSorter {
    gtk::CustomSorter::new(move |a, b| {
        let a = a.downcast_ref::<gtk::StringObject>().map(|s| s.string());
        let b = b.downcast_ref::<gtk::StringObject>().map(|s| s.string());
        let (Some(a), Some(b)) = (a, b) else {
            return gtk::Ordering::Equal;
        };
        let name = |l: &str| bookmarks::display_name(l).to_lowercase();
        let number = |l: &str| -> i64 {
            match column {
                Column::Status => bookmarks::peek(l).map_or(-1, |p| {
                    p.changed as i64 * 10_000 + p.ahead as i64 + p.behind as i64
                }),
                Column::Commit => bookmarks::peek(l)
                    .and_then(|p| p.last_commit)
                    .map_or(i64::MIN, |c| c.0),
                Column::Opened => crate::repos::opened_at(l).unwrap_or(i64::MIN),
                _ => 0,
            }
        };
        // 逆順に並べるときも、同じ値の中は名前の昇順にする（並べ替えは比べた結果ごと逆にするので、先に逆にしておく）。
        let descending = view
            .upgrade()
            .and_then(|v| v.sorter())
            .and_downcast::<gtk::ColumnViewSorter>()
            .is_some_and(|s| s.primary_sort_order() == gtk::SortType::Descending);
        let tie = name(&a).cmp(&name(&b));
        let order = match column {
            Column::Name => name(&a).cmp(&name(&b)),
            Column::Branch => branch_text(&a).cmp(&branch_text(&b)),
            _ => number(&a).cmp(&number(&b)),
        }
        .then(if descending { tie.reverse() } else { tie });
        order.into()
    })
}

/// `--repositories --snapshot <png>`: 裏で読むものがなくなってから撮って終わる（確認用）。
pub fn snapshot_when_ready(window: &gtk::Window, png: PathBuf) {
    let w = window.downgrade();
    let calm = Rc::new(Cell::new(0u32));
    glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        let Some(window) = w.upgrade() else {
            return glib::ControlFlow::Break;
        };
        // 読み終えてから少し（描き直すまで）待つ。
        if !window.is_mapped() || !bookmarks::idle() {
            calm.set(0);
            return glib::ControlFlow::Continue;
        }
        calm.set(calm.get() + 1);
        if calm.get() < 5 {
            return glib::ControlFlow::Continue;
        }
        // 開いているシート・メニューがあれば、そちらを撮る。
        let target = gtk::Window::list_toplevels()
            .into_iter()
            .filter_map(|t| t.downcast::<gtk::Window>().ok())
            .find(|t| t.transient_for().as_ref() == Some(&window) && t.is_visible())
            .unwrap_or_else(|| window.clone());
        match crate::snapshot(&target, &png) {
            Ok(()) => eprintln!("{}", png.display()),
            Err(e) => eprintln!("撮れない: {e}"),
        }
        if let Some(app) = window.application() {
            app.quit();
        }
        glib::ControlFlow::Break
    });
}
