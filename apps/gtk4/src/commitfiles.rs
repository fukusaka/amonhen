//! History の右の列の下（macOS 版の `CommitDetailViewController` の変更ファイルと差分）: 変更ファイルの一覧と差分。
//! 変更ファイルは、木なら差分の左に、そうでなければメッセージと差分の間に並べる。全ファイルを 1 つの差分に並べる
//! か（オフなら一覧で選んだファイルだけ）と、変更ファイルを木で出すかは、History の表示のメニューで切り替える
//! （macOS 版と同じ設定の鍵）。
//!
//! 全ファイルを並べるときは、一覧で選んだファイルの見出しまでスクロールし、スクロールしたら一覧の選択を合わせる。
//! 見たことにしたファイル（Viewed）は畳み、コミットごとに覚えておく（古いものから捨てて 500 コミットまで）。
//! lock ファイルなど生成されたファイルと、1000 行を超える変更のファイルは、最初は畳んでおく。
//!
//! 1 つのコミットで全ファイルを並べないとき（設定がオフか、変更ファイルが 200 を超える）は、最初のファイルの
//! 差分だけを読み、ほかは一覧で選んだときに読む（macOS 版と同じ）。

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use amonhen_core::diff::{DiffView, StackedDiff};
use amonhen_core::{HistorySnapshot, Repository, Selection};
use amonhen_model::{ChangeKind, FileChange, tr};
use amonhen_projection::FrontendProfile;
use gtk::prelude::*;
use gtk::{gio, glib};
use serde_json::{Value, json};

use crate::aiactions::CurrentRepo;
use crate::diff::{DiffWidget, Lazy, Source, View};
use crate::menu::{self, Item};

/// 全ファイルを 1 つの差分に並べるか（既定は並べる）。
pub const STACKED: &str = "stackedCommitDiff";
/// 変更ファイルを差分の左に木で出すか（既定は差分の上に一覧）。
pub const FILE_TREE: &str = "historyFileTree";
/// 見たことにしたファイル（`{"order": [鍵], "files": {鍵: [パス]}}`）。
const VIEWED: &str = "viewedFiles";
/// 覚えておくコミットの数。
const VIEWED_KEEP: usize = 500;
/// 1 ファイルの変更がこの行数を超えたら、最初は畳む。
const COLLAPSE_ABOVE: u32 = 1000;
/// 1 つのコミットで全ファイルを並べるのは、変更ファイルがこの数まで（多ければ 1 ファイルずつ。macOS 版と同じ）。
const STACK_LIMIT: usize = 200;

/// 全ファイルを 1 つの差分に並べるか。
pub fn stacked() -> bool {
    crate::settings::string(STACKED).is_none_or(|v| v == "true")
}

/// 変更ファイルを木で出すか。
pub fn file_tree() -> bool {
    crate::settings::bool(FILE_TREE)
}

fn viewed_store() -> Value {
    crate::settings::string(VIEWED)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({ "order": [], "files": {} }))
}

/// `key`（コミット ID。複数のコミットを選んでいるときはその ID を並べたもの）で見たことにしたパス。
pub fn viewed_paths(key: &str) -> HashSet<String> {
    viewed_store()["files"][key]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| p.as_str().map(str::to_owned))
        .collect()
}

/// `key` で見たことにしたパスを覚える（古いコミットのものから捨てる）。
fn save_viewed(key: &str, paths: &HashSet<String>) {
    let store = viewed_store();
    let mut order: Vec<String> = store["order"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|k| k.as_str().map(str::to_owned))
        .filter(|k| k != key)
        .collect();
    let mut files = store["files"].as_object().cloned().unwrap_or_default();
    if paths.is_empty() {
        files.remove(key);
    } else {
        let mut list: Vec<&String> = paths.iter().collect();
        list.sort();
        files.insert(key.to_owned(), json!(list));
        order.push(key.to_owned());
    }
    while order.len() > VIEWED_KEEP {
        let old = order.remove(0);
        files.remove(&old);
    }
    let store = json!({ "order": order, "files": files });
    crate::settings::set_string(VIEWED, &store.to_string());
}

/// lock ファイルや縮めたファイルなど、生成されたファイル（最初は畳む。macOS 版と同じ）。
fn is_generated(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    const LOCKFILES: [&str; 12] = [
        "Cargo.lock",
        "package-lock.json",
        "yarn.lock",
        "pnpm-lock.yaml",
        "Gemfile.lock",
        "poetry.lock",
        "composer.lock",
        "go.sum",
        "Package.resolved",
        "Podfile.lock",
        "bun.lockb",
        "uv.lock",
    ];
    LOCKFILES.contains(&name) || name.ends_with(".min.js") || name.ends_with(".min.css")
}

/// 裏のスレッドで用意した差分の表示。
pub struct Prepared {
    source: Arc<Source>,
    view: View,
    collapsed: Vec<bool>,
    /// 1 ファイルずつ見せるなら、出しているファイル。
    single: Option<usize>,
    /// コミットの変更ファイル（全ファイルを読んだときの、変更の種類ともとのパス。`source` の並びと同じ）。
    changes: Option<Arc<Vec<FileChange>>>,
}

impl Prepared {
    /// `stacked` を、`viewed` のファイル・生成されたファイル・大きなファイルを畳んで（`all` でなければ最初の
    /// ファイルだけを）表示にする。裏のスレッドで呼ぶ。
    pub fn new(stacked: StackedDiff, viewed: &HashSet<String>, all: bool) -> Self {
        Self::with_source(Arc::new(Source(stacked, None)), viewed, all)
    }

    /// History で選んだ 1 つのコミット（行 `row`）の差分。全ファイルを並べるなら（`all` で、変更ファイルが 1〜200）
    /// 全ファイルの差分を、そうでなければ最初のファイルの差分だけを読む。裏のスレッドで呼ぶ。
    pub fn commit(
        repo: &Arc<Repository>,
        history: &HistorySnapshot,
        row: u32,
        viewed: &HashSet<String>,
        all: bool,
    ) -> Result<Self, String> {
        let Selection::Range { base, tip, .. } =
            Selection::of(&history.graph, &[row]).map_err(|e| e.to_string())?
        else {
            return Err(tr!("No commit selected"));
        };
        let changes = repo
            .changes_between(base.as_ref(), &tip)
            .map_err(|e| e.to_string())?;
        Self::from_changes(repo.clone(), changes, viewed, all)
    }

    /// コミットの変更ファイル `changes` から作る（`commit` と、見せ方を変えたとき、Stashes・Reflog・Range Diff の
    /// 詳細）。裏のスレッドで呼ぶ。
    pub fn from_changes(
        repo: Arc<Repository>,
        changes: Vec<FileChange>,
        viewed: &HashSet<String>,
        all: bool,
    ) -> Result<Self, String> {
        if all && (1..=STACK_LIMIT).contains(&changes.len()) {
            let stacked = repo.stacked_diff(&changes).map_err(|e| e.to_string())?;
            let mut prepared = Self::new(stacked, viewed, true);
            prepared.changes = Some(Arc::new(changes));
            return Ok(prepared);
        }
        // 一覧に出す名前だけを並べ、差分は選んだときに読む。
        let empty = || {
            amonhen_core::diff::file_diff(Arc::<Vec<u8>>::default(), Arc::<Vec<u8>>::default(), 0)
        };
        let files = changes.iter().map(|c| (c.path.clone(), empty())).collect();
        let count = changes.len();
        let source = Arc::new(Source(StackedDiff { files }, Some(Lazy { repo, changes })));
        let (view, single) = if count == 0 {
            (View::build(&source.0, &[]), None)
        } else {
            (single_view(&source, 0)?, Some(0))
        };
        Ok(Self {
            source,
            view,
            collapsed: vec![false; count],
            single,
            changes: None,
        })
    }

    /// 読んである差分 `source` から作る（見せ方を変えたとき）。裏のスレッドで呼ぶ。
    fn with_source(source: Arc<Source>, viewed: &HashSet<String>, all: bool) -> Self {
        let collapsed: Vec<bool> = source
            .0
            .files
            .iter()
            .map(|(name, diff)| {
                is_generated(path_of(name))
                    || viewed.contains(name)
                    || diff.added + diff.removed > COLLAPSE_ABOVE
            })
            .collect();
        let (view, single) = if all || source.0.files.is_empty() {
            (View::build(&source.0, &collapsed), None)
        } else {
            (read_view(&source, 0), Some(0))
        };
        Self {
            source,
            view,
            collapsed,
            single,
            changes: None,
        }
    }
}

/// 1 ファイルの差分の表示（選んだファイルだけを読む元なら、ここで読む）。裏のスレッドで呼ぶ。
fn single_view(source: &Source, file: usize) -> Result<View, String> {
    match &source.1 {
        Some(lazy) => {
            let change = lazy
                .changes
                .get(file)
                .ok_or_else(|| tr!("Couldn’t load the diff"))?;
            lazy.repo
                .file_diff(change, &FrontendProfile::GTK)
                .map(View::new)
                .map_err(|e| e.to_string())
        }
        None => Ok(read_view(source, file)),
    }
}

/// 読んである差分の、1 ファイルの表示（裏のスレッドで呼ぶ）。
fn read_view(source: &Source, file: usize) -> View {
    let (name, diff) = &source.0.files[file];
    View::new(DiffView::new(diff, &FrontendProfile::GTK).with_path(path_of(name)))
}

/// 変更の種類の印（macOS 版と同じ）。
fn kind_mark(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Added => "A",
        ChangeKind::Deleted => "D",
        ChangeKind::Modified => "M",
        ChangeKind::Renamed => "R",
        ChangeKind::Copied => "C",
    }
}

/// とびとびに選んだときの見出し（「パス — 短い ID」）のパス。
fn path_of(name: &str) -> &str {
    name.split(" — ").next().unwrap_or(name)
}

/// 変更ファイルの一覧の 1 行。
struct FileRow {
    /// フォルダの名前か、ファイルの名前（木では名前だけ、一覧ではパス）。
    name: String,
    depth: usize,
    /// ファイルの番号（フォルダの行は None）。
    file: Option<usize>,
    /// 変更の種類（A・D・M・R・C）と、行数。
    mark: &'static str,
    counts: Counts,
    /// 説明（パス。名前を変えたファイルは「もとのパス → 今のパス」）。
    tip: String,
}

/// 一覧の行の行数。
#[derive(Clone, Copy)]
enum Counts {
    /// まだ読んでいない（選んだファイルだけを読むとき）か、フォルダの行。
    Unknown,
    Binary,
    Lines(u32, u32),
}

/// 変更ファイルの一覧の行。木なら、フォルダごとにまとめ（子が 1 つだけのフォルダはつなげる）、各階層でフォルダを
/// 先に、名前の順に並べる（macOS 版と同じ）。
fn file_rows(source: &Source, changes: Option<&[FileChange]>, tree: bool) -> Vec<FileRow> {
    // 変更の種類ともとのパスは、コミットの変更ファイルから（macOS 版と同じ）。
    let changes = source
        .1
        .as_ref()
        .map(|lazy| lazy.changes.as_slice())
        .or(changes)
        .filter(|c| c.len() == source.0.files.len());
    let file = |i: usize, name: String, depth: usize| {
        let diff = &source.0.files[i].1;
        let change = changes.map(|c| &c[i]);
        let mark = match change {
            Some(change) => kind_mark(change.kind),
            // 変更ファイルがわからない差分（stash など）は、中身の有無から。
            None if diff.old.is_empty() && !diff.new.is_empty() => "A",
            None if diff.new.is_empty() && !diff.old.is_empty() => "D",
            None => "M",
        };
        // 選んだファイルだけを読む元では、行数はまだわからない。
        let counts = if source.1.is_some() {
            Counts::Unknown
        } else if diff.binary {
            Counts::Binary
        } else {
            Counts::Lines(diff.added, diff.removed)
        };
        let path = path_of(source.0.files[i].0.as_str());
        let tip = match change.and_then(|c| c.old_path.as_deref()) {
            Some(old) => format!("{old} → {path}"),
            None => path.to_owned(),
        };
        FileRow {
            name,
            depth,
            file: Some(i),
            mark,
            counts,
            tip,
        }
    };
    let names: Vec<&str> = source.0.files.iter().map(|(n, _)| n.as_str()).collect();
    if !tree {
        return names
            .iter()
            .enumerate()
            .map(|(i, n)| file(i, (*n).to_owned(), 0))
            .collect();
    }
    #[derive(Default)]
    struct Folder {
        folders: std::collections::BTreeMap<String, Folder>,
        files: Vec<(String, usize)>,
    }
    let mut root = Folder::default();
    for (i, name) in names.iter().enumerate() {
        let mut parts: Vec<&str> = path_of(name).split('/').collect();
        let leaf = parts.pop().unwrap_or(name);
        // とびとびの見出しの短い ID は、名前の後ろに残す。
        let leaf = match name.split_once(" — ") {
            Some((_, id)) => format!("{leaf} — {id}"),
            None => leaf.to_owned(),
        };
        let mut folder = &mut root;
        for part in parts {
            folder = folder.folders.entry(part.to_owned()).or_default();
        }
        folder.files.push((leaf, i));
    }
    fn add(
        folder: &Folder,
        depth: usize,
        out: &mut Vec<FileRow>,
        file: &dyn Fn(usize, String, usize) -> FileRow,
    ) {
        for (name, child) in &folder.folders {
            let (mut label, mut child) = (name.clone(), child);
            while child.files.is_empty() && child.folders.len() == 1 {
                let (next, grand) = child.folders.iter().next().expect("one");
                label = format!("{label}/{next}");
                child = grand;
            }
            out.push(FileRow {
                tip: label.clone(),
                name: label,
                depth,
                file: None,
                mark: "",
                counts: Counts::Unknown,
            });
            add(child, depth + 1, out, file);
        }
        let mut files = folder.files.clone();
        files.sort();
        for (name, index) in files {
            out.push(file(index, name, depth));
        }
    }
    let mut out = Vec::new();
    add(&root, 0, &mut out, &file);
    out
}

/// 変更ファイルの右クリックのメニューに足す項目を作る（パス、差分の中の位置 y。一覧からなら None）。
type FileItems = Rc<dyn Fn(&str, Option<f64>) -> Vec<Item>>;

pub struct CommitDiff {
    pub root: gtk::Box,
    inner: Rc<Inner>,
}

struct Inner {
    root: gtk::Box,
    diff: DiffWidget,
    /// 差分（の枠）。
    diff_column: gtk::Box,
    /// 変更ファイルの一覧（スクロールの枠）。
    files: gtk::ScrolledWindow,
    list: gtk::ListView,
    store: gio::ListStore,
    selection: gtk::SingleSelection,
    /// 一覧と差分を並べている枠（置き方を変えたら作り直す）。
    holder: gtk::Box,
    paned: RefCell<Option<gtk::Paned>>,
    /// ファイルの番号 → 一覧の行。
    row_of_file: RefCell<Vec<u32>>,
    source: RefCell<Option<Arc<Source>>>,
    /// Viewed を覚える鍵。
    key: RefCell<String>,
    /// 1 ファイルずつ見せているか。
    single: Cell<bool>,
    /// 1 ファイルずつ見せているとき、差分に出しているファイル（一覧でフォルダの行を選んでも替わらない）。
    shown_file: Cell<Option<usize>>,
    /// 差分のスクロールに合わせて一覧の選択を変えている間（選択の変化でスクロールし返さない）。
    syncing: Cell<bool>,
    generation: Cell<u64>,
    pending: Rc<Cell<u32>>,
    current_repo: CurrentRepo,
    file_items: RefCell<Option<FileItems>>,
    find: FindBar,
    /// 出しているコミットの変更ファイル（`Prepared::changes`）。
    changes: RefCell<Option<Arc<Vec<FileChange>>>>,
}

/// 差分の中を探す帯（差分と一覧の下）。
struct FindBar {
    bar: gtk::Box,
    entry: gtk::SearchEntry,
    count: gtk::Label,
}

thread_local! {
    /// 確認用: 最初に差分を出したら探す語（`--find-diff`）、Viewed にするファイル（`--toggle-viewed`）、メニューを
    /// 出すファイル（`--diff-file-menu`）。
    static FIND_ONCE: RefCell<Option<String>> = const { RefCell::new(None) };
    static VIEWED_ONCE: Cell<Option<usize>> = const { Cell::new(None) };
    static MENU_ONCE: Cell<Option<usize>> = const { Cell::new(None) };
    /// 開いている差分の列（Ctrl+F を、フォーカスのある差分の検索の帯に回すため）。
    static OPEN: RefCell<Vec<std::rc::Weak<Inner>>> = RefCell::default();
}

/// 確認用（`--find-diff <語>`）: 最初に差分を出したら、検索の帯を開いてこの語を探す。
pub fn find_once(query: &str) {
    FIND_ONCE.with(|f| f.replace(Some(query.to_owned())));
}

/// 確認用（`--toggle-viewed <n>`）: 最初に差分を出したら、n 番目のファイルを Viewed にする。
pub fn toggle_viewed_once(file: usize) {
    VIEWED_ONCE.with(|v| v.set(Some(file)));
}

/// 確認用（`--diff-file-menu <n>`）: 最初に差分を出したら、n 番目のファイルの見出しのメニューを出す。
pub fn file_menu_once(file: usize) {
    MENU_ONCE.with(|v| v.set(Some(file)));
}

/// `window` でフォーカスのある差分の列があれば、その検索の帯を開く（Ctrl+F。開いたら true）。
pub fn find_in_focused(window: &gtk::Window) -> bool {
    match focused(window) {
        Some(inner) => {
            Inner::show_find(&inner);
            true
        }
        None => false,
    }
}

/// `window` でフォーカスのある差分の列の検索の帯が開いていれば、次・前の一致へ（Ctrl+G。移ったら true）。
pub fn find_next_in_focused(window: &gtk::Window, step: i32) -> bool {
    match focused(window).filter(|inner| inner.find.bar.is_visible()) {
        Some(inner) => {
            Inner::move_match(&inner, step);
            true
        }
        None => false,
    }
}

/// `window` で、フォーカスが中にある差分の列。
fn focused(window: &gtk::Window) -> Option<Rc<Inner>> {
    let focus = gtk::prelude::GtkWindowExt::focus(window)?;
    OPEN.with(|open| {
        let mut open = open.borrow_mut();
        open.retain(|w| w.strong_count() > 0);
        open.iter().filter_map(|w| w.upgrade()).find(|inner| {
            inner.root.is_mapped()
                && (focus == *inner.root.upcast_ref::<gtk::Widget>()
                    || focus.is_ancestor(&inner.root))
        })
    })
}

impl CommitDiff {
    pub fn new(pending: Rc<Cell<u32>>, current_repo: CurrentRepo) -> Self {
        let diff = DiffWidget::default();
        let diff_scrolled = gtk::ScrolledWindow::builder()
            .child(&diff)
            .vexpand(true)
            .build();
        // 検索の帯。
        let entry = gtk::SearchEntry::builder()
            .placeholder_text(tr!("Search Diff"))
            .hexpand(true)
            .build();
        let count = gtk::Label::new(None);
        count.add_css_class("dim-label");
        count.add_css_class("caption");
        count.add_css_class("numeric");
        let previous = gtk::Button::from_icon_name("go-up-symbolic");
        let next = gtk::Button::from_icon_name("go-down-symbolic");
        if !gtk::IconTheme::for_display(&diff.display()).has_icon("go-up-symbolic") {
            previous.set_label("∧");
            next.set_label("∨");
        }
        previous.set_tooltip_text(Some(&tr!("Previous")));
        next.set_tooltip_text(Some(&tr!("Next")));
        let done = gtk::Button::with_label(&tr!("Done"));
        for b in [&previous, &next, &done] {
            b.add_css_class("flat");
        }
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        bar.set_margin_top(4);
        bar.set_margin_bottom(4);
        bar.set_margin_start(8);
        bar.set_margin_end(8);
        bar.append(&entry);
        bar.append(&count);
        bar.append(&previous);
        bar.append(&next);
        bar.append(&done);
        bar.set_visible(false);
        let diff_column = gtk::Box::new(gtk::Orientation::Vertical, 0);
        diff_column.append(&diff_scrolled);

        // 変更ファイルの一覧（見えている行だけに部品を作る）。
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let selection = gtk::SingleSelection::new(Some(store.clone()));
        selection.set_autoselect(false);
        selection.set_can_unselect(true);
        let factory = gtk::SignalListItemFactory::new();
        let list = gtk::ListView::new(Some(selection.clone()), Some(factory.clone()));
        let files = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let holder = gtk::Box::new(gtk::Orientation::Vertical, 0);
        holder.set_vexpand(true);
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.append(&holder);
        root.append(&bar);
        let inner = Rc::new(Inner {
            root: root.clone(),
            diff,
            diff_column,
            files,
            list,
            store,
            selection,
            holder,
            paned: RefCell::default(),
            row_of_file: RefCell::default(),
            source: RefCell::default(),
            key: RefCell::default(),
            single: Cell::new(false),
            shown_file: Cell::new(None),
            syncing: Cell::new(false),
            generation: Cell::new(0),
            pending,
            current_repo,
            file_items: RefCell::default(),
            find: FindBar { bar, entry, count },
            changes: RefCell::default(),
        });
        OPEN.with(|open| open.borrow_mut().push(Rc::downgrade(&inner)));
        Inner::setup_factory(&inner, &factory);
        Inner::connect(&inner, &previous, &next, &done);
        Inner::arrange(&inner);
        inner
            .diff
            .show_message(&tr!("Select a commit to see its changes."));
        Self { root, inner }
    }

    /// 差分の部品（AI の解説などが読む）。
    pub fn diff(&self) -> &DiffWidget {
        &self.inner.diff
    }

    /// 次を読んでいる間: 一覧と差分を消す。
    pub fn clear(&self) {
        let inner = &self.inner;
        inner.generation.set(inner.generation.get() + 1);
        inner.source.replace(None);
        inner.shown_file.set(None);
        inner.syncing.set(true);
        inner.store.remove_all();
        inner.syncing.set(false);
        inner.row_of_file.borrow_mut().clear();
        inner.diff.clear();
    }

    /// 一覧を消し、差分の代わりに知らせ `text` を出す（「Loading the diff…」など）。
    pub fn show_message(&self, text: &str) {
        self.clear();
        self.inner.diff.show_message(text);
    }

    /// 用意した差分を出す。`key` は Viewed を覚える鍵。
    pub fn show(&self, prepared: Prepared, key: String) {
        Inner::show(&self.inner, prepared, key);
    }

    /// 変更ファイルの右クリックのメニューに足す項目（Blame・File History など）を作る `f(パス, 差分の中の y)`。
    pub fn connect_file_items(&self, f: impl Fn(&str, Option<f64>) -> Vec<Item> + 'static) {
        self.inner.file_items.replace(Some(Rc::new(f)));
    }

    /// 詳細の欄だけを右の列いっぱいに広げるボタン（⤢。変更ファイルと差分を隠す。もう一度押すと戻す）。
    pub fn expand_button(&self) -> gtk::ToggleButton {
        let label = gtk::Label::new(None);
        label.set_markup("<span size=\"large\">⤢</span>");
        let button = gtk::ToggleButton::new();
        button.set_child(Some(&label));
        button.add_css_class("flat");
        button.add_css_class("image-button");
        button.set_valign(gtk::Align::Start);
        button.set_tooltip_text(Some(&tr!(
            "Expand the commit details to fill the right column (click again to restore)."
        )));
        let root = self.root.downgrade();
        button.connect_toggled(move |b| {
            if let Some(root) = root.upgrade() {
                root.set_visible(!b.is_active());
            }
        });
        button
    }

    /// 差分の中を探す帯を開くボタン（詳細の右上に置く。メニューバーを使わずに届くように）。
    pub fn find_button(&self) -> gtk::Button {
        let icon = crate::icon::Icon::default();
        icon.set(crate::icon::Kind::Search, false);
        let button = gtk::Button::new();
        button.set_child(Some(&icon));
        button.add_css_class("flat");
        button.set_valign(gtk::Align::Start);
        button.set_tooltip_text(Some(&tr!("Find in the diff (Ctrl+F)")));
        let weak = Rc::downgrade(&self.inner);
        button.connect_clicked(move |_| {
            if let Some(inner) = weak.upgrade() {
                Inner::show_find(&inner);
            }
        });
        button
    }
}

impl Inner {
    fn setup_factory(this: &Rc<Self>, factory: &gtk::SignalListItemFactory) {
        let weak = Rc::downgrade(this);
        factory.connect_setup(move |_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let mark = gtk::Label::new(None);
            mark.set_width_chars(2);
            mark.add_css_class("monospace");
            let icon = crate::icon::Icon::default();
            icon.set(crate::icon::Kind::Folder, false);
            let name = gtk::Label::new(None);
            name.set_xalign(0.0);
            name.set_hexpand(true);
            name.set_ellipsize(gtk::pango::EllipsizeMode::Start);
            let counts = gtk::Label::new(None);
            counts.add_css_class("caption");
            counts.add_css_class("numeric");
            let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            line.set_margin_top(2);
            line.set_margin_bottom(2);
            line.set_margin_end(6);
            line.append(&mark);
            line.append(&icon);
            line.append(&name);
            line.append(&counts);
            // 右クリック: Blame・File History・パスのコピーなど（差分の見出しと同じ）。
            let click = gtk::GestureClick::new();
            click.set_button(gtk::gdk::BUTTON_SECONDARY);
            let (w, i, l) = (weak.clone(), item.downgrade(), line.downgrade());
            click.connect_pressed(move |_, _, x, y| {
                let (Some(this), Some(item), Some(line)) = (w.upgrade(), i.upgrade(), l.upgrade())
                else {
                    return;
                };
                let Some(file) = item
                    .item()
                    .and_downcast::<glib::BoxedAnyObject>()
                    .and_then(|o| o.borrow::<FileRow>().file)
                else {
                    return;
                };
                let items = Inner::menu_items(&this, Some(file), None);
                menu::popup(&line, x, y, items);
            });
            line.add_controller(click);
            item.set_child(Some(&line));
        });
        factory.connect_bind(|_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let (Some(line), Some(object)) = (
                item.child().and_downcast::<gtk::Box>(),
                item.item().and_downcast::<glib::BoxedAnyObject>(),
            ) else {
                return;
            };
            let row = object.borrow::<FileRow>();
            let children: Vec<gtk::Widget> = {
                let mut out = Vec::new();
                let mut child = line.first_child();
                while let Some(c) = child {
                    child = c.next_sibling();
                    out.push(c);
                }
                out
            };
            let [mark, icon, name, counts] = &children[..] else {
                return;
            };
            let (mark, name, counts) = (
                mark.downcast_ref::<gtk::Label>().expect("mark"),
                name.downcast_ref::<gtk::Label>().expect("name"),
                counts.downcast_ref::<gtk::Label>().expect("counts"),
            );
            line.set_margin_start(6 + (row.depth * 14) as i32);
            let folder = row.file.is_none();
            icon.set_visible(folder);
            mark.set_visible(!folder);
            mark.set_text(row.mark);
            for css in ["success", "error", "warning"] {
                mark.remove_css_class(css);
            }
            mark.add_css_class(match row.mark {
                "A" => "success",
                "D" => "error",
                _ => "warning",
            });
            name.set_text(&row.name);
            name.set_tooltip_text(Some(&row.tip));
            // 行数（0 の側は出さない）。バイナリは「Binary」（macOS 版と同じ）。
            match row.counts {
                Counts::Lines(added, removed) if !folder => {
                    let mut parts = Vec::new();
                    if added > 0 {
                        parts.push(format!("<span foreground=\"#2ec27e\">+{added}</span>"));
                    }
                    if removed > 0 {
                        parts.push(format!("<span foreground=\"#e01b24\">−{removed}</span>"));
                    }
                    counts.set_markup(&parts.join("  "));
                }
                Counts::Binary if !folder => counts.set_markup(&format!(
                    "<span alpha=\"55%\">{}</span>",
                    glib::markup_escape_text(&tr!("Binary"))
                )),
                _ => counts.set_text(""),
            }
            // フォルダの行は選べない。
            item.set_selectable(!folder);
            item.set_activatable(!folder);
        });
    }

    fn connect(this: &Rc<Self>, previous: &gtk::Button, next: &gtk::Button, done: &gtk::Button) {
        // 一覧で選んだファイル: 全ファイルを並べているなら、その見出しまでスクロールし、そうでなければそのファイルの
        // 差分を出す。
        let weak = Rc::downgrade(this);
        this.selection.connect_selected_notify(move |selection| {
            let Some(this) = weak.upgrade() else { return };
            if this.syncing.get() {
                return;
            }
            let Some(file) = selection
                .selected_item()
                .and_downcast::<glib::BoxedAnyObject>()
                .and_then(|o| o.borrow::<FileRow>().file)
            else {
                return;
            };
            if this.single.get() {
                Inner::show_single(&this, file);
            } else {
                this.diff.scroll_to_file(file);
            }
        });
        // 差分をスクロールしたら、一番上のファイルを一覧で選ぶ。
        let weak = Rc::downgrade(this);
        this.diff.connect_top_file(move |file| {
            let Some(this) = weak.upgrade() else { return };
            if this.single.get() {
                return;
            }
            let Some(&row) = this.row_of_file.borrow().get(file) else {
                return;
            };
            if this.selection.selected() != row {
                this.syncing.set(true);
                this.selection.set_selected(row);
                this.list.scroll_to(row, gtk::ListScrollFlags::NONE, None);
                this.syncing.set(false);
            }
        });
        // ⌥J・⌥K（1 ファイルずつ見せているとき）: 一覧で次・前のファイルを選ぶ。
        let weak = Rc::downgrade(this);
        this.diff.connect_move_file(move |step| {
            if let Some(this) = weak.upgrade() {
                Inner::move_file(&this, step);
            }
        });
        // Viewed: 見たことにしたら畳み、戻したら開く（GitHub と同じ）。
        let weak = Rc::downgrade(this);
        this.diff.connect_toggle_viewed(move |file| {
            if let Some(this) = weak.upgrade() {
                Inner::toggle_viewed(&this, file);
            }
        });
        // 見出し・差分の行の右クリック。
        let weak = Rc::downgrade(this);
        this.diff.connect_file_menu(move |file, path, x, y| {
            let Some(this) = weak.upgrade() else { return };
            // 文字を選んでいれば、先頭に Copy。
            let mut items = this.diff.copy_items();
            items.extend(match file {
                // 見出しの上: 畳む・Viewed・Blame・パスのコピーなど。
                Some(file) => Inner::menu_items(&this, Some(file), Some(y)),
                // 差分の行の上: AI でこの hunk を解説・Blame・File History（持ち主が作る）。
                None => this
                    .file_items
                    .borrow()
                    .clone()
                    .map(|f| f(path, Some(y)))
                    .unwrap_or_default(),
            });
            menu::popup(&this.diff, x, y, items);
        });
        // 1 ファイルずつ見せているときは、差分にファイルの見出しがない。差分の行の右クリックは、全ファイルを
        // 並べたときの見出しの下の行と同じ（AI でこの hunk を解説・Blame・File History）。
        let weak = Rc::downgrade(this);
        this.diff.connect_row_menu(move |_, x, y| {
            let Some(this) = weak.upgrade() else { return };
            let mut items = this.diff.copy_items();
            if let Some(path) = this.single_path() {
                items.extend(
                    this.file_items
                        .borrow()
                        .clone()
                        .map(|f| f(&path, Some(y)))
                        .unwrap_or_default(),
                );
            }
            if !items.is_empty() {
                menu::popup(&this.diff, x, y, items);
            }
        });
        // 検索の帯。
        let weak = Rc::downgrade(this);
        this.find.entry.connect_search_changed(move |entry| {
            if let Some(this) = weak.upgrade() {
                let (current, count) = this.diff.find(&entry.text());
                this.show_count(current, count);
            }
        });
        let weak = Rc::downgrade(this);
        this.find.entry.connect_activate(move |_| {
            if let Some(this) = weak.upgrade() {
                Inner::move_match(&this, 1);
            }
        });
        let weak = Rc::downgrade(this);
        this.find.entry.connect_stop_search(move |_| {
            if let Some(this) = weak.upgrade() {
                Inner::hide_find(&this);
            }
        });
        // Shift+Enter: 前の一致。
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(this);
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            let Some(this) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if matches!(key, gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter)
                && modifiers.contains(gtk::gdk::ModifierType::SHIFT_MASK)
            {
                Inner::move_match(&this, -1);
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        this.find.entry.add_controller(keys);
        for (button, step) in [(previous, -1), (next, 1)] {
            let weak = Rc::downgrade(this);
            button.connect_clicked(move |_| {
                if let Some(this) = weak.upgrade() {
                    Inner::move_match(&this, step);
                }
            });
        }
        let weak = Rc::downgrade(this);
        done.connect_clicked(move |_| {
            if let Some(this) = weak.upgrade() {
                Inner::hide_find(&this);
            }
        });
        // 一覧の置き方と、全ファイルを並べるかを変えたら、置き直す・読み直す。
        let weak = Rc::downgrade(this);
        crate::settings::connect_changed(move |key| {
            let Some(this) = weak.upgrade() else { return };
            match key {
                FILE_TREE => {
                    Inner::arrange(&this);
                    Inner::fill_list(&this);
                }
                STACKED => Inner::restack(&this),
                _ => {}
            }
        });
    }

    /// 変更ファイルの一覧を、差分の左（木）か上（一覧）に置く。
    fn arrange(this: &Rc<Self>) {
        if let Some(old) = this.paned.take() {
            old.set_start_child(gtk::Widget::NONE);
            old.set_end_child(gtk::Widget::NONE);
            this.holder.remove(&old);
        }
        let tree = file_tree();
        let paned = gtk::Paned::new(if tree {
            gtk::Orientation::Horizontal
        } else {
            gtk::Orientation::Vertical
        });
        paned.set_vexpand(true);
        paned.set_start_child(Some(&this.files));
        paned.set_end_child(Some(&this.diff_column));
        paned.set_resize_start_child(false);
        paned.set_shrink_start_child(false);
        paned.set_shrink_end_child(false);
        if tree {
            this.files.set_size_request(140, -1);
            paned.set_position(200);
        } else {
            this.files.set_size_request(-1, 60);
            paned.set_position(110);
        }
        this.holder.append(&paned);
        this.paned.replace(Some(paned));
    }

    fn show(this: &Rc<Self>, prepared: Prepared, key: String) {
        let Prepared {
            source,
            view,
            collapsed,
            single,
            changes,
        } = prepared;
        this.changes.replace(changes);
        this.generation.set(this.generation.get() + 1);
        let viewed = viewed_paths(&key);
        let viewed_files: HashSet<usize> = source
            .0
            .files
            .iter()
            .enumerate()
            .filter(|(_, (n, _))| viewed.contains(n))
            .map(|(i, _)| i)
            .collect();
        this.key.replace(key);
        this.single.set(single.is_some());
        this.shown_file.set(single);
        this.source.replace(Some(source.clone()));
        Inner::fill_list(this);
        match single {
            Some(_) => this.diff.show(source, view),
            None => this.diff.show_files(source, view, collapsed, viewed_files),
        }
        // 確認用（`--find-diff`・`--toggle-viewed`）。
        if let Some(query) = FIND_ONCE.with(|f| f.take()) {
            Inner::show_find(this);
            this.find.entry.set_text(&query);
        } else if this.find.bar.is_visible() {
            let (current, count) = this.diff.find(&this.find.entry.text());
            this.show_count(current, count);
        }
        if let Some(file) = VIEWED_ONCE.with(|v| v.take()) {
            Inner::toggle_viewed(this, file);
        }
        if let Some(file) = MENU_ONCE.with(|v| v.take()) {
            let items = Inner::menu_items(this, Some(file), None);
            let diff = this.diff.clone();
            // 差分が描かれてから出す。
            glib::idle_add_local_once(move || menu::popup(&diff, 60.0, 10.0, items));
        }
    }

    /// 変更ファイルの一覧を作り直し、最初のファイル（1 ファイルずつなら出しているファイル）を選ぶ。
    fn fill_list(this: &Rc<Self>) {
        let source = this.source.borrow().clone();
        let selected_file = this
            .selection
            .selected_item()
            .and_downcast::<glib::BoxedAnyObject>()
            .and_then(|o| o.borrow::<FileRow>().file);
        this.syncing.set(true);
        this.store.remove_all();
        let Some(source) = source else {
            this.syncing.set(false);
            return;
        };
        let changes = this.changes.borrow().clone();
        let rows = file_rows(&source, changes.as_deref().map(Vec::as_slice), file_tree());
        let mut row_of_file = vec![0; source.0.files.len()];
        for (r, row) in rows.iter().enumerate() {
            if let Some(i) = row.file {
                row_of_file[i] = r as u32;
            }
        }
        let objects: Vec<glib::BoxedAnyObject> =
            rows.into_iter().map(glib::BoxedAnyObject::new).collect();
        this.store.extend_from_slice(&objects);
        let first = selected_file
            .or_else(|| (!row_of_file.is_empty()).then_some(0))
            .and_then(|f| row_of_file.get(f).copied());
        this.row_of_file.replace(row_of_file);
        if let Some(row) = first {
            this.selection.set_selected(row);
        }
        this.syncing.set(false);
    }

    /// 1 ファイルずつ見せているときの、差分に出しているファイルのパス。
    fn single_path(&self) -> Option<String> {
        if !self.single.get() {
            return None;
        }
        let source = self.source.borrow().clone()?;
        let file = self.shown_file.get()?;
        let (name, _) = source.0.files.get(file)?;
        Some(path_of(name).to_owned())
    }

    /// 1 ファイルずつ見せているとき: `file` の差分を裏で作って出す。
    fn show_single(this: &Rc<Self>, file: usize) {
        let Some(source) = this.source.borrow().clone() else {
            return;
        };
        let generation = this.generation.get() + 1;
        this.generation.set(generation);
        this.pending.set(this.pending.get() + 1);
        let weak = Rc::downgrade(this);
        let s = source.clone();
        glib::spawn_future_local(async move {
            let view = gio::spawn_blocking(move || single_view(&s, file)).await;
            let Some(this) = weak.upgrade() else { return };
            this.pending.set(this.pending.get() - 1);
            if this.generation.get() != generation {
                return;
            }
            match view {
                Ok(Ok(view)) => {
                    this.diff.show(source, view);
                    this.shown_file.set(Some(file));
                    if this.find.bar.is_visible() {
                        let (current, count) = this.diff.find(&this.find.entry.text());
                        this.show_count(current, count);
                    }
                }
                Ok(Err(e)) => {
                    eprintln!("差分を読めない: {e}");
                    this.diff.show_message(&tr!("Couldn’t load the diff"));
                    this.shown_file.set(Some(file));
                }
                Err(_) => {}
            }
        });
    }

    /// 全ファイルを並べるかを変えた: 出しているコミットを、その見せ方で出し直す。
    fn restack(this: &Rc<Self>) {
        let Some(source) = this.source.borrow().clone() else {
            return;
        };
        let changes = this.changes.borrow().clone();
        let key = this.key.borrow().clone();
        let viewed = viewed_paths(&key);
        let all = stacked();
        let generation = this.generation.get() + 1;
        this.generation.set(generation);
        this.pending.set(this.pending.get() + 1);
        let weak = Rc::downgrade(this);
        glib::spawn_future_local(async move {
            let prepared = gio::spawn_blocking(move || match &source.1 {
                // 選んだファイルだけを読んでいたなら、その見せ方で読み直す。
                Some(lazy) => {
                    Prepared::from_changes(lazy.repo.clone(), lazy.changes.clone(), &viewed, all)
                }
                None => {
                    let mut prepared = Prepared::with_source(source, &viewed, all);
                    prepared.changes = changes;
                    Ok(prepared)
                }
            })
            .await;
            let Some(this) = weak.upgrade() else { return };
            this.pending.set(this.pending.get() - 1);
            if this.generation.get() != generation {
                return;
            }
            match prepared {
                Ok(Ok(prepared)) => Inner::show(&this, prepared, key),
                Ok(Err(e)) => {
                    eprintln!("差分を読めない: {e}");
                    this.diff.show_message(&tr!("Couldn’t load the diff"));
                }
                Err(_) => {}
            }
        });
    }

    /// ⌥J・⌥K（1 ファイルずつ見せているとき）: 一覧で次・前のファイルを選ぶ。
    fn move_file(this: &Rc<Self>, step: i32) {
        let n = this.store.n_items() as i64;
        let mut row = this.selection.selected() as i64;
        if this.selection.selected() == gtk::INVALID_LIST_POSITION {
            row = -1;
        }
        loop {
            row += step as i64;
            if row < 0 || row >= n {
                this.diff.error_bell();
                return;
            }
            let is_file = this
                .store
                .item(row as u32)
                .and_downcast::<glib::BoxedAnyObject>()
                .is_some_and(|o| o.borrow::<FileRow>().file.is_some());
            if is_file {
                this.selection.set_selected(row as u32);
                this.list
                    .scroll_to(row as u32, gtk::ListScrollFlags::NONE, None);
                return;
            }
        }
    }

    /// `file` を見たことにする・戻す（見たら畳み、戻したら開く）。
    fn toggle_viewed(this: &Rc<Self>, file: usize) {
        let Some(source) = this.source.borrow().clone() else {
            return;
        };
        let Some((name, _)) = source.0.files.get(file) else {
            return;
        };
        let key = this.key.borrow().clone();
        let mut paths = viewed_paths(&key);
        let now = !paths.remove(name);
        if now {
            paths.insert(name.clone());
        }
        save_viewed(&key, &paths);
        let viewed: HashSet<usize> = source
            .0
            .files
            .iter()
            .enumerate()
            .filter(|(_, (n, _))| paths.contains(n))
            .map(|(i, _)| i)
            .collect();
        this.diff.set_viewed(viewed);
        this.diff.set_file_collapsed(file, now);
    }

    /// 変更ファイルの右クリックのメニュー（macOS 版と同じ）: 開く・畳む・Viewed、Blame・File History、パスの
    /// コピー・ファイルマネージャーで見る・既定のアプリで開く。`y` は差分の中の位置（一覧からなら None）。
    fn menu_items(this: &Rc<Self>, file: Option<usize>, y: Option<f64>) -> Vec<Item> {
        let Some(source) = this.source.borrow().clone() else {
            return Vec::new();
        };
        let Some(file) = file.filter(|&f| f < source.0.files.len()) else {
            return Vec::new();
        };
        let path = path_of(&source.0.files[file].0).to_owned();
        let mut items = Vec::new();
        if !this.single.get() {
            let collapsed = this.diff.file_collapsed(file);
            let w = Rc::downgrade(this);
            items.push(menu::action(
                if collapsed {
                    tr!("Expand")
                } else {
                    tr!("Collapse")
                },
                move || {
                    if let Some(this) = w.upgrade() {
                        this.diff.set_file_collapsed(file, !collapsed);
                    }
                },
            ));
            let w = Rc::downgrade(this);
            items.push(menu::action(tr!("Toggle Viewed"), move || {
                if let Some(this) = w.upgrade() {
                    Inner::toggle_viewed(&this, file);
                }
            }));
            items.push(Item::Separator);
        }
        if let Some(f) = this.file_items.borrow().clone() {
            items.extend(f(&path, y));
            items.push(Item::Separator);
        }
        let (w, p) = (this.list.downgrade(), path.clone());
        items.push(menu::action(tr!("Copy Path"), move || {
            if let Some(w) = w.upgrade() {
                w.clipboard().set_text(&p);
            }
        }));
        // 手元のリポジトリのファイルがあれば、ファイルマネージャーとほかのアプリで開ける。
        let local = (this.current_repo)()
            .filter(|r| r.connection_state().is_none())
            .and_then(|r| r.paths().workdir)
            .map(|w| w.join(&path))
            .filter(|p| p.exists());
        if let Some(full) = local {
            let file = gio::File::for_path(&full);
            let (w, f) = (this.list.downgrade(), file.clone());
            items.push(menu::action(tr!("Show in Files"), move || {
                let window = w
                    .upgrade()
                    .and_then(|w| w.root())
                    .and_downcast::<gtk::Window>();
                gtk::FileLauncher::new(Some(&f)).open_containing_folder(
                    window.as_ref(),
                    gio::Cancellable::NONE,
                    |_| {},
                );
            }));
            let w = this.list.downgrade();
            items.push(menu::action(tr!("Open with Default App"), move || {
                let window = w
                    .upgrade()
                    .and_then(|w| w.root())
                    .and_downcast::<gtk::Window>();
                gtk::FileLauncher::new(Some(&file)).launch(
                    window.as_ref(),
                    gio::Cancellable::NONE,
                    |_| {},
                );
            }));
        }
        items
    }

    fn show_find(this: &Rc<Self>) {
        // 選んだ文字（1 行）があれば、それを探す（macOS 版と同じ）。
        if let Some(selected) = this
            .diff
            .selected_text()
            .filter(|t| !t.is_empty() && !t.contains('\n'))
        {
            this.find.entry.set_text(&selected);
        }
        this.find.bar.set_visible(true);
        this.find.entry.grab_focus();
        this.find.entry.select_region(0, -1);
        let (current, count) = this.diff.find(&this.find.entry.text());
        this.show_count(current, count);
    }

    fn hide_find(this: &Rc<Self>) {
        this.find.bar.set_visible(false);
        this.diff.clear_find();
        this.diff.grab_focus();
    }

    fn move_match(this: &Rc<Self>, step: i32) {
        if !this.find.bar.is_visible() {
            Inner::show_find(this);
        }
        let (current, count) = this.diff.find_move(step);
        this.show_count(current, count);
    }

    fn show_count(&self, current: Option<usize>, count: usize) {
        let text = if self.find.entry.text().is_empty() {
            String::new()
        } else if let Some(c) = current {
            format!("{} / {count}", c + 1)
        } else {
            tr!("Not found")
        };
        self.find.count.set_text(&text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 手元の設定を読まない git で、`dir` に 3 ファイルのコミットを 1 つ作る。
    fn commit_three_files(dir: &std::path::Path) {
        std::fs::create_dir_all(dir).unwrap();
        for name in ["a.txt", "b.txt", "c.txt"] {
            std::fs::write(dir.join(name), format!("{name}\n")).unwrap();
        }
        let git = |args: &[&str]| {
            let ok = amonhen_model::process::command("git")
                .args(["-c", "commit.gpgsign=false"])
                .args(["-c", "user.name=T", "-c", "user.email=t@example.com"])
                .args(args)
                .current_dir(dir)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .status()
                .is_ok_and(|s| s.success());
            assert!(ok, "git {args:?}");
        };
        git(&["init", "-q"]);
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "three"]);
    }

    #[test]
    fn reads_only_the_first_file_unless_stacked() {
        let dir = std::env::temp_dir().join(format!("amonhen-gtk-lazy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        commit_three_files(&dir);
        let backend = amonhen_backend_gix::GixBackend::open(&dir).unwrap();
        let repo = Arc::new(Repository::new(backend));
        let history = repo.load_history().unwrap();
        let none = HashSet::new();

        // 全ファイルを並べないなら、最初のファイルだけ読み、ほかは選んだときに読む。
        let lazy = Prepared::commit(&repo, &history, 0, &none, false).unwrap();
        assert_eq!(lazy.single, Some(0));
        assert!(lazy.source.1.is_some());
        assert_eq!(lazy.source.0.files.len(), 3);
        assert!(lazy.source.0.files.iter().all(|(_, d)| d.added == 0));
        assert!(!lazy.view.diff_view().rows.is_empty());
        let marks: Vec<&str> = file_rows(&lazy.source, None, false)
            .iter()
            .map(|r| r.mark)
            .collect();
        assert_eq!(marks, ["A", "A", "A"]);
        let third = single_view(&lazy.source, 2).unwrap();
        assert_eq!(third.diff_view().added, 1);
        // AI の解説など、全ファイルが要るときはその場で読む。
        let added = lazy
            .source
            .with_all(|all| all.files.iter().map(|(_, d)| d.added).sum::<u32>());
        assert_eq!(added, 3);

        // 全ファイルを並べるなら（200 ファイルまで）、全部読む。
        let stacked = Prepared::commit(&repo, &history, 0, &none, true).unwrap();
        assert_eq!(stacked.single, None);
        assert!(stacked.source.1.is_none());
        assert!(stacked.source.0.files.iter().all(|(_, d)| d.added == 1));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
