//! History の右の列の下（macOS 版の `CommitDetailViewController` の差分の側）: 上の帯と、1 つの差分。変更ファイルの
//! 一覧は差分のファイルの見出しそのもの（最初はすべて畳み、押すとその場で開く・畳む）で、リストか木で見せる
//! （木ならフォルダの行が見出しの間に入り、押すと中の見出しを畳む）。
//!
//! 上の帯: 変更／すべてのファイル、ファイルの数と行数の合計、リスト／木、フォルダ・差分をすべて開く、無視する空白
//! （`whitespace.rs`）、差分の中の検索、表示の設定（詳細の見せ方・詳細を広げる・AI）。見出しの ⋯（右クリックと同じ）に
//! ファイル全体を表示・この時点のファイルを開く・Blame などを置く。hunk の見出しの左の印で前後を 20 行広げる。
//! 画像は見出しの下に比べる欄を出す（`imagediff.rs`）。「すべてのファイル」はその時点の木と中身
//! （`commitbrowser.rs`）。
//!
//! 変更ファイルの多いコミット（200 を超える）は、コアが見出しだけを並べ、開いたファイルの差分をそのときに取る
//! （`Repository::set_stacked_context`。macOS 版と同じ）。

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use amonhen_core::diff::{StackedDiff, Whitespace};
use amonhen_core::{HistorySnapshot, Repository};
use amonhen_model::{ChangeKind, FileChange, Oid, tr};
use gtk::prelude::*;
use gtk::{gio, glib};

use crate::aiactions::CurrentRepo;
use crate::commitbrowser::Browser;
use crate::diff::{Decor, DiffWidget, FileInfo, Folder, Source, View};
use crate::imagediff;
use crate::menu::{self, Item};

/// 変更ファイルを木で見せるか（既定はパスの一覧）。
pub const FILE_TREE: &str = "historyFileTree";
/// 1 ファイルの変更がこの行数を超えたら、「差分をすべて開く」でも開かない（1 つずつなら開ける）。
const EXPAND_ALL_LIMIT: u32 = 1000;
/// hunk の見出しの印で広げる行数。
const WIDEN_BY: u32 = 20;

/// 変更ファイルを木で見せるか。
pub fn file_tree() -> bool {
    crate::settings::bool(FILE_TREE)
}

/// 裏のスレッドで用意した差分の表示（すべてのファイルを畳んだもの）。
pub struct Prepared {
    source: Arc<Source>,
    view: View,
    names: Vec<String>,
    generated: Vec<bool>,
    /// コミットの変更ファイル（変更の種類ともとのパス。`source` の並びと同じ）。複数のコミットの差分では None。
    changes: Option<Arc<Vec<FileChange>>>,
}

impl Prepared {
    /// `stacked` を、すべてのファイルを畳んだ表示にする。裏のスレッドで呼ぶ。`repo` はまだ取っていない差分を取る
    /// リポジトリ。
    pub fn new(
        stacked: StackedDiff,
        repo: Option<Arc<Repository>>,
        changes: Option<Vec<FileChange>>,
    ) -> Self {
        let count = stacked.files.len();
        let names = stacked.files.iter().map(|(n, _)| n.clone()).collect();
        let generated = (0..count).map(|i| stacked.generated(i)).collect();
        let view = View::build(&stacked, &vec![true; count]);
        let changes = changes.filter(|c| c.len() == count).map(Arc::new);
        Self {
            source: Arc::new(Source::new(stacked, repo)),
            view,
            names,
            generated,
            changes,
        }
    }

    /// History で選んだ 1 つのコミット（行 `row`）の差分（`whitespace` の空白を無視して行を比べる）。裏のスレッドで
    /// 呼ぶ。
    pub fn commit(
        repo: &Arc<Repository>,
        history: &HistorySnapshot,
        row: u32,
        whitespace: Whitespace,
    ) -> Result<Self, String> {
        // 本当の親と比べる（「一致したコミットだけ」の History では、親が次に一致したコミットに替わっている）。
        let Some(id) = history.graph.ids().get(row as usize) else {
            return Err(tr!("No commit selected"));
        };
        let changes = repo.commit_changes(id).map_err(|e| e.to_string())?;
        Self::from_changes(repo.clone(), changes, whitespace)
    }

    /// コミットの変更ファイル `changes` から作る（Stashes・Reflog・Range Diff の詳細も）。裏のスレッドで呼ぶ。
    pub fn from_changes(
        repo: Arc<Repository>,
        changes: Vec<FileChange>,
        whitespace: Whitespace,
    ) -> Result<Self, String> {
        let stacked = repo
            .stacked_diff_ws(&changes, whitespace)
            .map_err(|e| e.to_string())?;
        Ok(Self::new(stacked, Some(repo), Some(changes)))
    }
}

/// 変更の種類の印（macOS 版と同じ）。
pub fn kind_mark(kind: ChangeKind) -> &'static str {
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

/// 木で見せるときの、フォルダの行・ファイルの深さと名前・畳んだフォルダの中のファイル（並びはパスの順のまま。
/// macOS 版の `treeLayout`）。
#[derive(Default)]
struct TreeLayout {
    folders: HashMap<usize, Vec<Folder>>,
    depths: HashMap<usize, usize>,
    titles: HashMap<usize, String>,
    hidden: HashSet<usize>,
}

/// `paths` のファイルを木に並べる（`labels` は名前を変えたファイルの `src/{old → new}` など、見出しに出す文字）。
fn tree_layout(paths: &[String], labels: &[String], collapsed: &HashSet<String>) -> TreeLayout {
    let mut out = TreeLayout::default();
    let mut previous: Vec<&str> = Vec::new();
    for (file, path) in paths.iter().enumerate() {
        let mut parts: Vec<&str> = path.split('/').collect();
        let name = parts.pop().unwrap_or_default();
        let common = previous
            .iter()
            .zip(&parts)
            .take_while(|(a, b)| a == b)
            .count();
        // 畳んだフォルダ（いちばん外側）。その中は見出しを出さない。
        let outer = (0..parts.len()).find(|&l| collapsed.contains(&parts[..=l].join("/")));
        let mut folders = Vec::new();
        for level in common..parts.len() {
            if outer.is_some_and(|o| level > o) {
                break;
            }
            let folder = parts[..=level].join("/");
            folders.push(Folder {
                name: parts[level].to_owned(),
                depth: level,
                collapsed: collapsed.contains(&folder),
                path: folder,
            });
        }
        if outer.is_some() {
            out.hidden.insert(file);
        }
        if !folders.is_empty() {
            out.folders.insert(file, folders);
        }
        out.depths.insert(file, parts.len());
        // 名前を変えたファイルは、同じフォルダの中なら `old → new`。
        let folder = parts.join("/");
        let label = &labels[file];
        let title = if folder.is_empty() {
            label.clone()
        } else if let Some(rest) = label.strip_prefix(&format!("{folder}/")) {
            rest.to_owned()
        } else {
            name.to_owned()
        };
        out.titles.insert(file, title);
        previous = parts;
    }
    out
}

/// 変更ファイルの右クリックのメニューに足す項目を作る（パス、差分の中の位置 y。見出しからなら None。Blame を
/// 出すか、差分の行の上か）。
type FileItems = Rc<dyn Fn(&str, Option<f64>, bool, bool) -> Vec<Item>>;
/// 変更ファイルの右クリックの「この版に戻す…」など（そのファイルの変更から作る）。
type RestoreItems = Rc<dyn Fn(&FileChange) -> Vec<Item>>;
/// 表示の設定のメニューに足す項目（詳細の見せ方と、AI）。
type ViewOptions = Rc<dyn Fn() -> (Vec<Item>, Vec<Item>)>;
/// 表示を作り直した後にすること。
type Then = Box<dyn FnOnce(&Rc<Inner>)>;

/// 空白の扱いを変えて読み直すとき、開いていたファイルなどを戻す（macOS 版の `restoreAfterReload`）。読み直した
/// 差分のファイルの並びが同じときだけ戻す。
struct Restore {
    names: Vec<String>,
    open: HashSet<usize>,
    whole: HashSet<usize>,
    contexts: HashMap<usize, u32>,
    svg_text: HashSet<usize>,
    collapsed_folders: HashSet<String>,
    /// 見えていた一番上のファイル。
    top: Option<usize>,
}

pub struct CommitDiff {
    pub root: gtk::Box,
    inner: Rc<Inner>,
}

/// 出しているコミットの、見せ方の様子。
#[derive(Default)]
struct St {
    source: Option<Arc<Source>>,
    names: Vec<String>,
    generated: Vec<bool>,
    changes: Option<Arc<Vec<FileChange>>>,
    infos: Vec<FileInfo>,
    /// 「すべてのファイル」を開けるコミット（1 つのコミット。stash・複数のコミットでは None）。
    commit: Option<Oid>,
    /// 開いている（差分を出す）ファイル。最初はすべて畳む。
    open: HashSet<usize>,
    /// ファイル全体を出しているファイルと、前後の行を広げたファイルの行数。
    whole: HashSet<usize>,
    contexts: HashMap<usize, u32>,
    /// SVG をテキストで見るファイル。
    svg_text: HashSet<usize>,
    /// 木で畳んだフォルダ。
    collapsed_folders: HashSet<String>,
    /// 画像の欄（ファイルの番号 → 部品。開き直しても比べ方とスワイプの位置を保つ）。
    panels: HashMap<usize, gtk::Widget>,
    /// 「すべてのファイル」を出しているか。
    files_mode: bool,
}

/// 上の帯の部品。
struct Bar {
    changes: gtk::ToggleButton,
    all_files: gtk::ToggleButton,
    totals: gtk::Label,
    list: gtk::ToggleButton,
    tree: gtk::ToggleButton,
    folders: gtk::Button,
    folders_icon: crate::icon::Icon,
    expand_all: gtk::Button,
    expand_icon: crate::icon::Icon,
    /// 行を比べるときに無視する空白。覚えない。
    whitespace: crate::whitespace::Picker,
    /// 長い行を折り返す（アプリ全体の設定）。
    wrap: gtk::Button,
    /// 表示の設定のメニュー。
    options: gtk::Button,
}

struct Inner {
    root: gtk::Box,
    diff: DiffWidget,
    /// 差分の側と、すべてのファイルの側を切り替える。
    pages: gtk::Stack,
    bar: Bar,
    st: RefCell<St>,
    browser: RefCell<Option<Rc<Browser>>>,
    /// 帯の切り替えを、こちらから合わせている間（変化の知らせで動かない）。
    syncing: Cell<bool>,
    generation: Cell<u64>,
    pending: Rc<Cell<u32>>,
    current_repo: CurrentRepo,
    file_items: RefCell<Option<FileItems>>,
    restore_items: RefCell<Option<RestoreItems>>,
    view_options: RefCell<Option<ViewOptions>>,
    /// 詳細を広げるボタン（詳細の右上。広げている間だけ見せる）。
    expand: RefCell<Option<gtk::ToggleButton>>,
    /// sparse checkout なら、その様子（取り出していないフォルダのファイルは、ディスクにないので開けない）。
    sparse: RefCell<Option<crate::repoform::Sparse>>,
    /// partial clone か（すべてのファイルでファイルを開く前に、中身が手元にあるかを確かめる）。
    partial: Cell<bool>,
    /// 差分の側の列（上に「取ってくる」の帯を置く）。
    diff_column: gtk::Box,
    /// 「取ってくる」の帯と、それを出した差分の元・中身が手元にないファイルの数（なければ None）。
    missing_band: RefCell<Option<MissingBand>>,
    find: FindBar,
    /// 出しているコミットのファイルごとの行数（`commit_extras` の `stats`。パス → 追加・削除、両方 None は
    /// バイナリ）。まだ差分を取っていないファイルの見出しと合計に使う（macOS 版と同じ）。
    stats: RefCell<Stats>,
    /// 画像の比べ方（コミットを変えても保つ）。
    image_mode: Cell<imagediff::Mode>,
    /// 出しているコミットの GitHub のページ（`commit_extras` の origin から。Markdown のリンク先）。
    github: RefCell<Option<String>>,
    /// 無視する空白を選び直したら呼ぶ（持ち主が、出しているものを読み直す）。
    on_whitespace: RefCell<Option<Rc<dyn Fn()>>>,
    /// 空白の扱いを変えて読み直した差分に戻す、開き方。
    restore: RefCell<Option<Restore>>,
}

/// 差分の上の「取ってくる」の帯（`Inner::update_missing`）。
struct MissingBand {
    source: Arc<Source>,
    count: usize,
    widget: gtk::Widget,
}

/// ファイルごとの行数（パス → 追加・削除。両方 None はバイナリ）。
type Stats = HashMap<String, (Option<u32>, Option<u32>)>;

/// 差分の中を探す帯（差分の上）。
struct FindBar {
    bar: gtk::Box,
    entry: gtk::SearchEntry,
    count: gtk::Label,
}

thread_local! {
    /// 確認用: 最初に差分を出したら探す語（`--find-diff`）、メニューを出すファイル（`--diff-file-menu`）。
    static FIND_ONCE: RefCell<Option<String>> = const { RefCell::new(None) };
    static MENU_ONCE: Cell<Option<usize>> = const { Cell::new(None) };
    /// 確認用（`AMH_VIEW_OPTIONS_MENU=1`）: 見えている差分の列で最初に差分を出したら、帯の表示の設定のメニューを
    /// 出し、項目の名前を標準出力に書く（出したら true）。
    static OPTIONS_SHOWN: Cell<bool> = const { Cell::new(false) };
    /// 開いている差分の列（Ctrl+F を、フォーカスのある差分の検索の帯に回すため）。
    static OPEN: RefCell<Vec<std::rc::Weak<Inner>>> = RefCell::default();
}

/// 確認用（`--find-diff <語>`）: 最初に差分を出したら、検索の帯を開いてこの語を探す。
pub fn find_once(query: &str) {
    FIND_ONCE.with(|f| f.replace(Some(query.to_owned())));
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
/// フォーカスが検索の帯の中なら移らない（macOS 版の ⌘G は差分の部品だけが受け、帯の欄からは History の検索の
/// 一致へ行く）。
pub fn find_next_in_focused(window: &gtk::Window, step: i32) -> bool {
    match focused(window).filter(|inner| inner.find.bar.is_visible() && !in_bar(window, inner)) {
        Some(inner) => {
            Inner::move_match(&inner, step);
            true
        }
        None => false,
    }
}

/// `window` でフォーカスのある差分の検索の帯が閉じていれば、帯を開いて次・前の一致へ（History を探していない
/// ときの Ctrl+G。macOS 版の差分の `findNext`）。移ったら true。
pub fn find_next_opening(window: &gtk::Window, step: i32) -> bool {
    match focused(window).filter(|inner| !in_bar(window, inner)) {
        Some(inner) => {
            Inner::move_match(&inner, step);
            true
        }
        None => false,
    }
}

/// `window` で Ctrl+G が差分の一致に移れるか（フォーカスが差分にあり、検索の帯の中ではない）。
pub fn find_next_possible(window: &gtk::Window) -> bool {
    focused(window).is_some_and(|inner| !in_bar(window, &inner))
}

fn in_bar(window: &gtk::Window, inner: &Inner) -> bool {
    gtk::prelude::GtkWindowExt::focus(window).is_some_and(|f| f.is_ancestor(&inner.find.bar))
}

/// `window` で、フォーカスが差分か検索の帯にある差分の列。
fn focused(window: &gtk::Window) -> Option<Rc<Inner>> {
    let focus = gtk::prelude::GtkWindowExt::focus(window)?;
    OPEN.with(|open| {
        let mut open = open.borrow_mut();
        open.retain(|w| w.strong_count() > 0);
        open.iter().filter_map(|w| w.upgrade()).find(|inner| {
            inner.root.is_mapped()
                && (focus == *inner.diff.upcast_ref::<gtk::Widget>()
                    || focus.is_ancestor(&inner.diff)
                    || focus.is_ancestor(&inner.find.bar))
        })
    })
}

/// 帯の小さなアイコンのボタン。
pub(crate) fn icon_button(kind: crate::icon::Kind, tip: &str) -> (gtk::Button, crate::icon::Icon) {
    let icon = crate::icon::Icon::default();
    icon.set(kind, false);
    let button = gtk::Button::new();
    button.set_child(Some(&icon));
    button.add_css_class("flat");
    button.set_tooltip_text(Some(tip));
    button.update_property(&[gtk::accessible::Property::Label(tip)]);
    (button, icon)
}

/// 帯の、アイコンで切り替える押しボタン（つないで 1 つの切り替えに見せる）。
fn icon_toggle(kind: crate::icon::Kind, tip: &str) -> gtk::ToggleButton {
    let icon = crate::icon::Icon::default();
    icon.set(kind, false);
    let button = gtk::ToggleButton::new();
    button.set_child(Some(&icon));
    button.set_tooltip_text(Some(tip));
    button.update_property(&[gtk::accessible::Property::Label(tip)]);
    button
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
        // ツールチップは出さず、読み上げの名前だけ（macOS 版と同じ）。
        previous.update_property(&[gtk::accessible::Property::Label(&tr!("Previous"))]);
        next.update_property(&[gtk::accessible::Property::Label(&tr!("Next"))]);
        let done = gtk::Button::with_label(&tr!("Done"));
        for b in [&previous, &next, &done] {
            b.add_css_class("flat");
        }
        let find_bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        find_bar.set_margin_top(4);
        find_bar.set_margin_bottom(4);
        find_bar.set_margin_start(8);
        find_bar.set_margin_end(8);
        find_bar.append(&entry);
        find_bar.append(&count);
        find_bar.append(&previous);
        find_bar.append(&next);
        find_bar.append(&done);
        find_bar.set_visible(false);

        // 上の帯: 変更／すべてのファイル、合計、リスト／木、フォルダ・差分をすべて開く、空白、検索、表示の設定。
        let changes = gtk::ToggleButton::with_label(&tr!("Changes"));
        let all_files = gtk::ToggleButton::with_label(&tr!("All Files"));
        all_files.set_group(Some(&changes));
        changes.set_active(true);
        let view_switch = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        view_switch.add_css_class("linked");
        view_switch.append(&changes);
        view_switch.append(&all_files);
        let totals = gtk::Label::new(None);
        totals.set_xalign(0.0);
        // 合計は省かない（狭いときは右の側の部品から切れる）。
        totals.set_single_line_mode(true);
        totals.add_css_class("caption");
        totals.add_css_class("numeric");
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        let list = icon_toggle(
            crate::icon::Kind::List,
            &tr!("Show the changed files as a list"),
        );
        let tree = icon_toggle(
            crate::icon::Kind::Tree,
            &tr!("Show the changed files as a tree"),
        );
        tree.set_group(Some(&list));
        if file_tree() {
            tree.set_active(true);
        } else {
            list.set_active(true);
        }
        let tree_switch = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        tree_switch.add_css_class("linked");
        tree_switch.append(&list);
        tree_switch.append(&tree);
        let (folders, folders_icon) =
            icon_button(crate::icon::Kind::Folder, &tr!("Expand All Folders"));
        let (expand_all, expand_icon) =
            icon_button(crate::icon::Kind::ExpandVertical, &tr!("Expand All Diffs"));
        let whitespace = crate::whitespace::Picker::new();
        let wrap = crate::diff::wrap_button();
        let (find, _) = icon_button(crate::icon::Kind::Search, &tr!("Find in the diff (Ctrl+F)"));
        let (options, _) = icon_button(crate::icon::Kind::Sliders, &tr!("View Options"));
        let bar_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        bar_box.set_margin_start(8);
        bar_box.set_margin_end(8);
        bar_box.set_margin_top(3);
        bar_box.set_margin_bottom(3);
        for w in [
            view_switch.upcast_ref::<gtk::Widget>(),
            totals.upcast_ref(),
            spacer.upcast_ref(),
            tree_switch.upcast_ref(),
            folders.upcast_ref(),
            expand_all.upcast_ref(),
            whitespace.widget().upcast_ref(),
            wrap.upcast_ref(),
            find.upcast_ref(),
            options.upcast_ref(),
        ] {
            bar_box.append(w);
        }
        // 狭いときは右の側から切れる（右の列の幅を押し広げない。macOS 版と同じ）。
        let bar_scroll = gtk::ScrolledWindow::builder()
            .child(&bar_box)
            .hscrollbar_policy(gtk::PolicyType::External)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .build();

        let diff_column = gtk::Box::new(gtk::Orientation::Vertical, 0);
        diff_column.append(&diff_scrolled);
        let pages = gtk::Stack::new();
        pages.set_vexpand(true);
        pages.add_named(&diff_column, Some("changes"));
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.append(&find_bar);
        root.append(&bar_scroll);
        root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        root.append(&pages);
        let inner = Rc::new(Inner {
            root: root.clone(),
            diff,
            pages,
            bar: Bar {
                changes,
                all_files,
                totals,
                list,
                tree,
                folders,
                folders_icon,
                expand_all,
                expand_icon,
                whitespace,
                wrap,
                options: options.clone(),
            },
            st: RefCell::default(),
            browser: RefCell::default(),
            syncing: Cell::new(false),
            generation: Cell::new(0),
            pending,
            current_repo,
            file_items: RefCell::default(),
            restore_items: RefCell::default(),
            view_options: RefCell::default(),
            expand: RefCell::default(),
            sparse: RefCell::default(),
            partial: Cell::new(false),
            diff_column,
            missing_band: RefCell::default(),
            find: FindBar {
                bar: find_bar,
                entry,
                count,
            },
            stats: RefCell::default(),
            image_mode: Cell::new(imagediff::Mode::default()),
            github: RefCell::default(),
            on_whitespace: RefCell::default(),
            restore: RefCell::default(),
        });
        OPEN.with(|open| open.borrow_mut().push(Rc::downgrade(&inner)));
        Inner::connect(&inner, &previous, &next, &done, &find, &options);
        Inner::update_bar(&inner);
        inner
            .diff
            .show_message(&tr!("Select a commit to see its changes."));
        Self { root, inner }
    }

    /// 差分の部品（AI の解説などが読む）。
    pub fn diff(&self) -> &DiffWidget {
        &self.inner.diff
    }

    /// 行を比べるときに無視する空白（帯で選んでいるもの）。差分はこれで取る。
    pub fn whitespace(&self) -> Whitespace {
        self.inner.bar.whitespace.whitespace()
    }

    /// 帯で無視する空白を選び直したら `f()` を呼ぶ（出しているコミット・選び方・範囲を [`Self::whitespace`] で
    /// 読み直す）。読み直した差分では、開いていたファイル・広げた前後・ファイル全体・畳んだフォルダ・一番上の
    /// ファイルを戻す（macOS 版と同じ）。
    pub fn connect_whitespace_changed(&self, f: impl Fn() + 'static) {
        self.inner.on_whitespace.replace(Some(Rc::new(f)));
    }

    /// 次を読んでいる間: 差分を消す。
    pub fn clear(&self) {
        let inner = &self.inner;
        inner.generation.set(inner.generation.get() + 1);
        *inner.st.borrow_mut() = St::default();
        Inner::drop_browser(inner);
        Inner::update_missing(inner);
        inner.diff.clear();
        Inner::sync_switches(inner);
        Inner::update_bar(inner);
    }

    /// 別のコミットを読み始める: 前のコミットの行数を捨てる（`clear` は読んでいる途中にも呼ぶので、そこでは捨てない）。
    pub fn reset_stats(&self) {
        self.inner.stats.borrow_mut().clear();
        self.inner.github.replace(None);
    }

    /// 出しているコミットの `commit_extras` の答えから、ファイルごとの行数を受け取る（まだ差分を取っていない
    /// ファイルの見出しと、合計に出す）。
    pub fn set_stats(&self, extras: &serde_json::Value) {
        let inner = &self.inner;
        let stats: Stats = extras["stats"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| {
                let count = |k: &str| s[k].as_u64().map(|n| n as u32);
                Some((
                    s["path"].as_str()?.to_owned(),
                    (count("added"), count("removed")),
                ))
            })
            .collect();
        inner.stats.replace(stats);
        // Markdown の #123 などのリンク先（GitHub の remote のときだけ）。
        let github = extras["originUrl"]
            .as_str()
            .and_then(crate::github::GitHubRepository::parse)
            .map(|g| g.web_url());
        if let Some(browser) = inner.browser.borrow().as_ref() {
            browser.set_github(github.clone());
        }
        inner.github.replace(github);
        let counts = Inner::pending_counts(inner);
        inner.diff.set_counts(counts);
        Inner::update_bar(inner);
    }

    /// 差分の代わりに知らせ `text` を出す（「Loading the diff…」など）。
    pub fn show_message(&self, text: &str) {
        self.clear();
        self.inner.diff.show_message(text);
    }

    /// 用意した差分を出す。`commit` は「すべてのファイル」で木を読むコミット（stash・複数のコミットでは None）。
    pub fn show(&self, prepared: Prepared, commit: Option<Oid>) {
        Inner::show(&self.inner, prepared, commit);
    }

    /// sparse checkout の様子（ファイルのメニューで、取り出していないフォルダのファイルを開けないわけを添える）。
    pub fn set_sparse(&self, sparse: Option<crate::repoform::Sparse>) {
        self.inner.sparse.replace(sparse);
    }

    /// partial clone か（すべてのファイルでファイルを開く前に、中身が手元にあるかを確かめる）。
    pub fn set_partial(&self, partial: bool) {
        self.inner.partial.set(partial);
        if let Some(browser) = self.inner.browser.borrow().as_ref() {
            browser.set_partial(partial);
        }
    }

    /// 変更ファイルの右クリックのメニューに足す項目（Blame・File History など）を作る
    /// `f(パス, 差分の中の y, Blame を出すか, 差分の行の上か)`。差分の行の上（見出しでない所）では、AI でこの hunk を
    /// 解説だけ（macOS 版と同じ）。
    pub fn connect_file_items(
        &self,
        f: impl Fn(&str, Option<f64>, bool, bool) -> Vec<Item> + 'static,
    ) {
        self.inner.file_items.replace(Some(Rc::new(f)));
    }

    /// 変更ファイルの右クリックに足す、作業ツリーのファイルを戻す項目（`f(そのファイルの変更)`）。コミットを
    /// 出しているときだけ呼ぶ。
    pub fn connect_restore_items(&self, f: impl Fn(&FileChange) -> Vec<Item> + 'static) {
        self.inner.restore_items.replace(Some(Rc::new(f)));
    }

    /// 帯の表示の設定に足す項目を作る `f() -> (詳細の見せ方, AI)`（開くたびに作る）。
    pub fn connect_view_options(&self, f: impl Fn() -> (Vec<Item>, Vec<Item>) + 'static) {
        self.inner.view_options.replace(Some(Rc::new(f)));
    }

    /// 詳細の欄だけを右の列いっぱいに広げるボタン（差分を隠す。表示の設定の「詳細を広げる」で押す）。広げている
    /// 間だけ詳細の右上に出し、押すと戻す（隠した差分の側の帯からは戻せないので）。
    pub fn expand_button(&self) -> gtk::ToggleButton {
        let icon = crate::icon::Icon::default();
        icon.set(crate::icon::Kind::Restore, false);
        let button = gtk::ToggleButton::new();
        // 詳細の見せ方を変えたときに戻せるように（`Details::show_style_switch`）。
        button.set_widget_name(crate::details::EXPAND_BUTTON);
        button.set_child(Some(&icon));
        button.update_property(&[gtk::accessible::Property::Label(&tr!(
            "Restore Commit Details"
        ))]);
        button.add_css_class("flat");
        button.add_css_class("image-button");
        button.set_valign(gtk::Align::Start);
        button.set_tooltip_text(Some(&tr!(
            "Expand the commit details to fill the right column (click again to restore)."
        )));
        button.set_visible(false);
        let root = self.root.downgrade();
        button.connect_toggled(move |b| {
            b.set_visible(b.is_active());
            if let Some(root) = root.upgrade() {
                root.set_visible(!b.is_active());
            }
        });
        self.inner.expand.replace(Some(button.clone()));
        button
    }
}

impl Inner {
    fn connect(
        this: &Rc<Self>,
        previous: &gtk::Button,
        next: &gtk::Button,
        done: &gtk::Button,
        find: &gtk::Button,
        options: &gtk::Button,
    ) {
        // 見出しを押す: その場で開く・畳む。
        let weak = Rc::downgrade(this);
        this.diff.connect_toggle_file(move |file| {
            if let Some(this) = weak.upgrade() {
                Inner::toggle_open(&this, file);
            }
        });
        // フォルダの行を押す: 中の見出しを畳む・出す。
        let weak = Rc::downgrade(this);
        this.diff.connect_toggle_folder(move |folder| {
            let Some(this) = weak.upgrade() else { return };
            {
                let mut st = this.st.borrow_mut();
                if !st.collapsed_folders.remove(&folder) {
                    st.collapsed_folders.insert(folder);
                }
            }
            Inner::reshow(&this, true, Vec::new(), None);
        });
        // hunk の見出しの印: そのファイルの前後を広げる。
        let weak = Rc::downgrade(this);
        this.diff.connect_expand_hunk(move |file| {
            if let Some(this) = weak.upgrade() {
                Inner::widen_context(&this, file);
            }
        });
        // 見出しの ⋯・右クリック、差分の行の右クリック。
        let weak = Rc::downgrade(this);
        this.diff.connect_file_menu(move |file, path, x, y| {
            let Some(this) = weak.upgrade() else { return };
            // 文字のコピーは Ctrl+C で（メニューには出さない。macOS 版と同じ）。
            let items = match file {
                Some(file) => Inner::menu_items(&this, file),
                None => Inner::row_items(&this, path, y),
            };
            if !items.is_empty() {
                menu::popup(&this.diff, x, y, items);
            }
        });
        // 帯: 変更／すべてのファイル。
        let weak = Rc::downgrade(this);
        this.bar.all_files.connect_toggled(move |b| {
            if let Some(this) = weak.upgrade()
                && !this.syncing.get()
            {
                Inner::set_files_mode(&this, b.is_active());
            }
        });
        // リスト／木（History の表示のメニューと同じ設定）。
        let weak = Rc::downgrade(this);
        this.bar.tree.connect_toggled(move |b| {
            if let Some(this) = weak.upgrade()
                && !this.syncing.get()
                && b.is_active() != file_tree()
            {
                crate::settings::set_bool(FILE_TREE, b.is_active());
            }
        });
        let weak = Rc::downgrade(this);
        this.bar.folders.connect_clicked(move |_| {
            if let Some(this) = weak.upgrade() {
                Inner::toggle_all_folders(&this);
            }
        });
        let weak = Rc::downgrade(this);
        this.bar.expand_all.connect_clicked(move |_| {
            if let Some(this) = weak.upgrade() {
                Inner::toggle_all_diffs(&this);
            }
        });
        let weak = Rc::downgrade(this);
        this.bar.whitespace.connect_changed(move || {
            if let Some(this) = weak.upgrade() {
                Inner::whitespace_changed(&this);
            }
        });
        let weak = Rc::downgrade(this);
        find.connect_clicked(move |_| {
            if let Some(this) = weak.upgrade() {
                Inner::show_find(&this);
            }
        });
        let weak = Rc::downgrade(this);
        options.connect_clicked(move |b| {
            if let Some(this) = weak.upgrade() {
                let items = Inner::view_option_items(&this);
                if !items.is_empty() {
                    menu::popup(b, b.width() as f64 / 2.0, b.height() as f64, items);
                }
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
        // リストか木かを変えたら（History の表示のメニューからも）、置き直す。
        let weak = Rc::downgrade(this);
        crate::settings::connect_changed_while(&this.root, move |key| {
            if key == FILE_TREE
                && let Some(this) = weak.upgrade()
            {
                Inner::tree_changed(&this);
            }
        });
    }

    fn show(this: &Rc<Self>, prepared: Prepared, commit: Option<Oid>) {
        let Prepared {
            source,
            view,
            names,
            generated,
            changes,
        } = prepared;
        this.generation.set(this.generation.get() + 1);
        Inner::drop_browser(this);
        let count = names.len();
        let restore = this.restore.take().filter(|r| r.names == names);
        {
            let mut st = this.st.borrow_mut();
            *st = St {
                commit: commit.filter(|_| source.repo.is_some()),
                infos: view.file_infos(),
                source: Some(source.clone()),
                names,
                generated,
                changes,
                ..St::default()
            };
        }
        // 最初はすべて畳む。
        let decor = Inner::decor(this, &HashSet::new(), HashMap::new());
        this.diff
            .show_stacked(source, view, vec![true; count], decor, false);
        Inner::update_missing(this);
        Inner::sync_switches(this);
        Inner::update_bar(this);
        // 確認用（`--find-diff`）。
        if let Some(query) = FIND_ONCE.with(|f| f.take()) {
            Inner::show_find(this);
            this.find.entry.set_text(&query);
        } else if this.find.bar.is_visible() {
            let (current, count) = this.diff.recount(&this.find.entry.text());
            this.show_count(current, count);
        }
        if this.root.is_mapped()
            && std::env::var("AMH_VIEW_OPTIONS_MENU").is_ok_and(|v| v == "1")
            && !OPTIONS_SHOWN.with(|s| s.replace(true))
        {
            // 項目の名前も書き出す（メニューは別の面に出るので、撮った絵には写らないことがある）。
            for title in menu::titles(&Inner::view_option_items(this)) {
                println!("view options | {title}");
            }
            let button = this.bar.options.clone();
            glib::idle_add_local_once(move || button.emit_clicked());
        }
        if let Some(file) = MENU_ONCE.with(|v| v.take()) {
            let items = Inner::menu_items(this, file);
            let diff = this.diff.clone();
            // 差分が描かれてから出す。
            glib::idle_add_local_once(move || menu::popup(&diff, 60.0, 10.0, items));
        }
        match restore {
            Some(restore) => Inner::apply_restore(this, restore),
            None => Inner::apply_snapshot_options(this),
        }
    }

    /// 帯で無視する空白を選び直した: 開き方を覚えて、持ち主に読み直してもらう（macOS 版の `setWhitespace`）。
    fn whitespace_changed(this: &Rc<Self>) {
        let restore = {
            let st = this.st.borrow();
            st.source.is_some().then(|| Restore {
                names: st.names.clone(),
                open: st.open.clone(),
                whole: st.whole.clone(),
                contexts: st.contexts.clone(),
                svg_text: st.svg_text.clone(),
                collapsed_folders: st.collapsed_folders.clone(),
                top: this.diff.visible_top_file(),
            })
        };
        this.restore.replace(restore);
        let f = this.on_whitespace.borrow().clone();
        match f {
            Some(f) => f(),
            None => {
                this.restore.take();
            }
        }
    }

    /// 空白の扱いを変えて読み直した差分に、前の開き方を戻す。
    fn apply_restore(this: &Rc<Self>, restore: Restore) {
        let Restore {
            open,
            whole,
            contexts,
            svg_text,
            collapsed_folders,
            top,
            ..
        } = restore;
        let ops: Vec<(usize, u32)> = {
            let mut st = this.st.borrow_mut();
            let count = st.names.len();
            let mut files: Vec<usize> = open
                .iter()
                .chain(&whole)
                .chain(contexts.keys())
                .copied()
                .filter(|&f| f < count)
                .collect();
            files.sort_unstable();
            files.dedup();
            let ops = files
                .into_iter()
                .filter_map(|f| {
                    let context = if whole.contains(&f) {
                        u32::MAX
                    } else {
                        contexts.get(&f).copied().unwrap_or(3)
                    };
                    let pending = st.infos.get(f).is_some_and(|i| i.pending);
                    (context != 3 || pending).then_some((f, context))
                })
                .collect();
            st.open = open.into_iter().filter(|&f| f < count).collect();
            st.whole = whole.into_iter().filter(|&f| f < count).collect();
            st.contexts = contexts.into_iter().filter(|(f, _)| *f < count).collect();
            st.svg_text = svg_text.into_iter().filter(|&f| f < count).collect();
            st.collapsed_folders = collapsed_folders;
            ops
        };
        Inner::reshow(
            this,
            false,
            ops,
            Some(Box::new(move |this| {
                if let Some(top) = top {
                    this.diff.scroll_to_file(top);
                }
            })),
        );
    }

    /// 前のコミットの「すべてのファイル」を捨てる。
    fn drop_browser(this: &Rc<Self>) {
        if let Some(browser) = this.browser.take() {
            this.pages.remove(&browser.root);
        }
        this.pages.set_visible_child_name("changes");
    }

    /// `file` 番目のファイルのパス（とびとびに選んだコミットの見出し「パス — 短い ID」からはパスを取り出す）。
    fn path(st: &St, file: usize) -> String {
        match st.changes.as_ref().and_then(|c| c.get(file)) {
            Some(change) => change.path.clone(),
            None => path_of(st.names.get(file).map_or("", String::as_str)).to_owned(),
        }
    }

    /// 見出しに出す名前（名前を変えたファイルは `src/{old → new}`）。
    fn label(st: &St, file: usize) -> String {
        match st.changes.as_ref().and_then(|c| c.get(file)) {
            Some(FileChange {
                path,
                old_path: Some(old),
                ..
            }) if old != path => amonhen_model::rename_label(old, path),
            Some(change) => change.path.clone(),
            None => st.names.get(file).cloned().unwrap_or_default(),
        }
    }

    fn change(st: &St, file: usize) -> Option<FileChange> {
        st.changes.as_ref()?.get(file).cloned()
    }

    /// まだ差分を取っていないファイルの行数（詳細の行数から）。
    fn pending_counts(this: &Rc<Self>) -> HashMap<usize, (u32, u32)> {
        let st = this.st.borrow();
        let stats = this.stats.borrow();
        st.infos
            .iter()
            .enumerate()
            .filter(|(_, info)| info.pending)
            .filter_map(|(i, _)| {
                let (a, r) = stats.get(&Inner::path(&st, i))?;
                Some((i, (a.unwrap_or(0), r.unwrap_or(0))))
            })
            .collect()
    }

    /// 木の様子（木で見せていなければ None）。
    fn tree_layout(st: &St) -> Option<TreeLayout> {
        if !file_tree() {
            return None;
        }
        let paths: Vec<String> = (0..st.names.len()).map(|i| Inner::path(st, i)).collect();
        let labels: Vec<String> = (0..st.names.len()).map(|i| Inner::label(st, i)).collect();
        Some(tree_layout(&paths, &labels, &st.collapsed_folders))
    }

    /// 見出しの飾り（`panel_files` は欄を出すファイル、`panels` はその部品）。
    fn decor(
        this: &Rc<Self>,
        panel_files: &HashSet<usize>,
        panels: HashMap<usize, gtk::Widget>,
    ) -> Decor {
        let counts = Inner::pending_counts(this);
        let st = this.st.borrow();
        let mut decor = Decor {
            counts,
            open: panel_files.clone(),
            panels,
            ..Decor::default()
        };
        for file in 0..st.names.len() {
            if let Some(change) = st.changes.as_ref().and_then(|c| c.get(file)) {
                decor.marks.insert(file, kind_mark(change.kind));
            }
            let path = Inner::path(&st, file);
            if st.infos.get(file).is_some_and(|i| i.missing) {
                decor.notes.insert(file, tr!("Not here yet"));
            } else if imagediff::is_image(&path) && !imagediff::is_svg(&path) {
                decor.notes.insert(file, tr!("Image"));
            }
        }
        match Inner::tree_layout(&st) {
            Some(tree) => {
                decor.folders = tree.folders;
                decor.depths = tree.depths;
                decor.titles = tree.titles;
                decor.hidden = tree.hidden;
            }
            None => {
                // リストでは、名前を変えたファイルだけ `src/{old → new}` にする。
                for file in 0..st.names.len() {
                    let label = Inner::label(&st, file);
                    if label != st.names[file] {
                        decor.titles.insert(file, label);
                    }
                }
            }
        }
        decor
    }

    /// 開いている・畳んでいるファイル、木、画像の欄から、差分の表示を作り直す。`ops`（ファイル、前後の行数）は、
    /// 先にそのファイルの差分を取り直す（まだ取っていない差分もここで取る）。作り直したら `then` を呼ぶ。
    fn reshow(this: &Rc<Self>, keep_scroll: bool, ops: Vec<(usize, u32)>, then: Option<Then>) {
        let (source, collapsed, panel_files, blobs) = {
            let mut st = this.st.borrow_mut();
            let Some(source) = st.source.clone() else {
                return;
            };
            let count = st.names.len();
            let hidden = Inner::tree_layout(&st)
                .map(|t| t.hidden)
                .unwrap_or_default();
            let panel_files: HashSet<usize> = st
                .open
                .iter()
                .copied()
                .filter(|&f| {
                    // 中身が手元にない画像は、絵の欄を作らない（帯で取ってくるまで）。
                    imagediff::is_image(&Inner::path(&st, f))
                        && !st.svg_text.contains(&f)
                        && !hidden.contains(&f)
                        && !st.infos.get(f).is_some_and(|i| i.missing)
                })
                .collect();
            let collapsed: Vec<bool> = (0..count)
                .map(|f| !st.open.contains(&f) || panel_files.contains(&f) || hidden.contains(&f))
                .collect();
            // まだ部品のない画像の欄の、前と後の blob。
            st.panels.retain(|f, _| panel_files.contains(f));
            let blobs: Vec<(usize, Option<Oid>, Option<Oid>)> = panel_files
                .iter()
                .filter(|f| !st.panels.contains_key(f))
                .map(|&f| {
                    let change = Inner::change(&st, f);
                    (
                        f,
                        change.as_ref().and_then(|c| c.old_id),
                        change.as_ref().and_then(|c| c.new_id),
                    )
                })
                .collect();
            (source, collapsed, panel_files, blobs)
        };
        let generation = this.generation.get() + 1;
        this.generation.set(generation);
        this.pending.set(this.pending.get() + 1);
        let weak = Rc::downgrade(this);
        let s = source.clone();
        let c = collapsed.clone();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                let mut failed = false;
                let mut stacked = s.lock();
                if let Some(repo) = &s.repo {
                    for (file, context) in ops {
                        if let Err(e) = repo.set_stacked_context(&mut stacked, file, context) {
                            eprintln!("差分を読めない: {e}");
                            failed = true;
                        }
                    }
                }
                let view = View::build(&stacked, &c);
                drop(stacked);
                type Bytes = Option<Arc<Vec<u8>>>;
                let blobs: Vec<(usize, Bytes, Bytes)> = blobs
                    .into_iter()
                    .map(|(file, old, new)| {
                        let read = |id: Option<Oid>| {
                            id.and_then(|id| s.repo.as_ref().and_then(|r| r.blob(&id).ok()))
                        };
                        (file, read(old), read(new))
                    })
                    .collect();
                (view, failed, blobs)
            })
            .await;
            let Some(this) = weak.upgrade() else { return };
            this.pending.set(this.pending.get() - 1);
            let Ok((view, failed, blobs)) = result else {
                return;
            };
            if this.generation.get() != generation {
                return;
            }
            // 開いたときに中身が手元にないと分かった画像も、絵の欄を作らない。
            let infos = view.file_infos();
            let mut panel_files = panel_files;
            panel_files.retain(|&f| !infos.get(f).is_some_and(|i| i.missing));
            for (file, old, new) in blobs {
                if !panel_files.contains(&file) {
                    continue;
                }
                let panel = Inner::image_panel(
                    &this,
                    file,
                    old.as_ref().map(|d| d.as_slice()),
                    new.as_ref().map(|d| d.as_slice()),
                );
                this.st.borrow_mut().panels.insert(file, panel);
            }
            this.st.borrow_mut().infos = infos;
            let panels: HashMap<usize, gtk::Widget> = this
                .st
                .borrow()
                .panels
                .iter()
                .filter(|(f, _)| panel_files.contains(f))
                .map(|(f, w)| (*f, w.clone()))
                .collect();
            let decor = Inner::decor(&this, &panel_files, panels);
            this.diff
                .show_stacked(source, view, collapsed, decor, keep_scroll);
            // 開いたファイルの中身が手元になかった（partial clone）なら、取りに行かずに帯で聞く。
            Inner::update_missing(&this);
            Inner::update_bar(&this);
            if failed {
                this.diff.error_bell();
            }
            if let Some(then) = then {
                then(&this);
            }
        });
    }

    /// partial clone で中身が手元になく、差分を取っていないファイルがあれば、差分の上に「取ってくる」の帯を出す
    /// （なければ消す。数が同じなら、取っている途中の帯をそのまま残す）。
    fn update_missing(this: &Rc<Self>) {
        let (count, source) = {
            let st = this.st.borrow();
            (
                st.infos.iter().filter(|i| i.missing).count(),
                st.source.clone(),
            )
        };
        if this.missing_band.borrow().as_ref().is_some_and(|b| {
            b.count == count && source.as_ref().is_some_and(|s| Arc::ptr_eq(s, &b.source))
        }) {
            return;
        }
        if let Some(band) = this.missing_band.take() {
            this.diff_column.remove(&band.widget);
        }
        let Some(source) = source.filter(|s| count > 0 && s.repo.is_some()) else {
            return;
        };
        let s = source.clone();
        let fetch: crate::missingband::Fetch = Arc::new(move || {
            let Some(repo) = &s.repo else { return Ok(()) };
            // 取っている間は差分を止めない（取り終えてから、そのファイルの差分を取る）。
            let ids = s.lock().missing_ids();
            repo.fetch_contents(&ids).map_err(|e| e.to_string())?;
            repo.fetch_stacked_missing(&mut s.lock())
                .map_err(|e| e.to_string())
        });
        let weak = Rc::downgrade(this);
        let shown = source.clone();
        let widget = crate::missingband::files(count, fetch, move || {
            let Some(this) = weak.upgrade() else { return };
            // 別のコミットを出していれば何もしない。
            if !this
                .st
                .borrow()
                .source
                .as_ref()
                .is_some_and(|s| Arc::ptr_eq(s, &source))
            {
                return;
            }
            if let Some(band) = this.missing_band.take() {
                this.diff_column.remove(&band.widget);
            }
            Inner::reshow(&this, true, Vec::new(), None);
        });
        this.diff_column.prepend(&widget);
        this.missing_band.replace(Some(MissingBand {
            source: shown,
            count,
            widget,
        }));
    }

    /// 画像の比べる欄。
    fn image_panel(
        this: &Rc<Self>,
        file: usize,
        before: Option<&[u8]>,
        after: Option<&[u8]>,
    ) -> gtk::Widget {
        let svg = imagediff::is_svg(&Inner::path(&this.st.borrow(), file));
        let weak = Rc::downgrade(this);
        let on_mode: Rc<dyn Fn(imagediff::Mode)> = Rc::new(move |mode| {
            if let Some(this) = weak.upgrade() {
                this.image_mode.set(mode);
            }
        });
        let weak = Rc::downgrade(this);
        let on_text: Rc<dyn Fn()> = Rc::new(move || {
            let Some(this) = weak.upgrade() else { return };
            let ops = Inner::load_ops(&this, &[file]);
            this.st.borrow_mut().svg_text.insert(file);
            Inner::reshow(&this, true, ops, None);
        });
        imagediff::panel(before, after, svg, this.image_mode.get(), on_mode, on_text)
    }

    /// まだ差分を取っていないファイルを取る（`reshow` に渡す）。
    fn load_ops(this: &Rc<Self>, files: &[usize]) -> Vec<(usize, u32)> {
        let st = this.st.borrow();
        files
            .iter()
            .filter(|&&f| st.infos.get(f).is_some_and(|i| i.pending))
            .map(|&f| (f, 3))
            .collect()
    }

    /// 見出しを押した: そのファイルの差分を開く・畳む。まだ差分を取っていなければ、ここで取る。
    fn toggle_open(this: &Rc<Self>, file: usize) {
        let ops = if this.st.borrow_mut().open.remove(&file) {
            Vec::new()
        } else {
            this.st.borrow_mut().open.insert(file);
            Inner::load_ops(this, &[file])
        };
        Inner::reshow(
            this,
            true,
            ops,
            Some(Box::new(move |this| this.diff.reveal_file_header(file))),
        );
    }

    /// hunk の見出しの印: そのファイルの前後の行を 20 行ずつ広げる。
    fn widen_context(this: &Rc<Self>, file: usize) {
        let context = {
            let mut st = this.st.borrow_mut();
            if st.whole.contains(&file) {
                return;
            }
            let context = st.contexts.get(&file).copied().unwrap_or(3) + WIDEN_BY;
            st.contexts.insert(file, context);
            context
        };
        Inner::reshow(this, true, vec![(file, context)], None);
    }

    /// 見出しの ⋯ > ファイル全体を表示: そのファイルの差分の前後の行を、ファイル全体にする・戻す。
    fn toggle_whole_file(this: &Rc<Self>, file: usize) {
        let mut ops = Inner::load_ops(this, &[file]);
        {
            let mut st = this.st.borrow_mut();
            let whole = !st.whole.contains(&file);
            let context = if whole {
                st.whole.insert(file);
                u32::MAX
            } else {
                st.whole.remove(&file);
                st.contexts.get(&file).copied().unwrap_or(3)
            };
            ops.push((file, context));
            st.open.insert(file);
        }
        Inner::reshow(
            this,
            true,
            ops,
            Some(Box::new(move |this| this.diff.reveal_file_header(file))),
        );
    }

    /// 「差分をすべて開く」で開くファイル（生成物と、変更の多いファイルは除く）。
    fn expandable_files(this: &Rc<Self>) -> HashSet<usize> {
        let st = this.st.borrow();
        let stats = this.stats.borrow();
        (0..st.names.len())
            .filter(|&f| {
                let info = st.infos.get(f).copied().unwrap_or_default();
                let stat_ok = stats
                    .get(&Inner::path(&st, f))
                    .is_none_or(|(a, r)| a.unwrap_or(0) + r.unwrap_or(0) <= EXPAND_ALL_LIMIT);
                !st.generated.get(f).copied().unwrap_or(false)
                    && info.added + info.removed <= EXPAND_ALL_LIMIT
                    && stat_ok
            })
            .collect()
    }

    /// 生成物などのほかは、すべて開いているか。
    fn all_diffs_open(this: &Rc<Self>) -> bool {
        let targets = Inner::expandable_files(this);
        !targets.is_empty() && targets.is_subset(&this.st.borrow().open)
    }

    fn toggle_all_diffs(this: &Rc<Self>) {
        let ops = if Inner::all_diffs_open(this) {
            this.st.borrow_mut().open.clear();
            Vec::new()
        } else {
            let targets: Vec<usize> = Inner::expandable_files(this).into_iter().collect();
            let ops = Inner::load_ops(this, &targets);
            this.st.borrow_mut().open.extend(targets);
            ops
        };
        Inner::reshow(this, false, ops, None);
    }

    /// すべてのフォルダを開く（どれか畳んでいれば）・畳む。差分を開いているかは変えない。
    fn toggle_all_folders(this: &Rc<Self>) {
        let browser = this.browser.borrow().clone();
        if this.st.borrow().files_mode {
            if let Some(browser) = browser {
                browser.set_all_folders(!browser.any_folder_expanded());
                Inner::update_bar(this);
            }
            return;
        }
        {
            let mut st = this.st.borrow_mut();
            if st.collapsed_folders.is_empty() {
                let mut all = HashSet::new();
                for file in 0..st.names.len() {
                    let path = Inner::path(&st, file);
                    let parts: Vec<&str> = path.split('/').collect();
                    for level in 0..parts.len().saturating_sub(1) {
                        all.insert(parts[..=level].join("/"));
                    }
                }
                st.collapsed_folders = all;
            } else {
                st.collapsed_folders.clear();
            }
        }
        Inner::reshow(this, true, Vec::new(), None);
    }

    /// リストか木かの設定が変わった。
    fn tree_changed(this: &Rc<Self>) {
        Inner::sync_switches(this);
        if this.st.borrow().files_mode {
            Inner::update_bar(this);
        } else {
            Inner::reshow(this, true, Vec::new(), None);
        }
    }

    /// 帯の切り替えを、今の様子に合わせる。
    fn sync_switches(this: &Rc<Self>) {
        this.syncing.set(true);
        let (files, can) = {
            let st = this.st.borrow();
            (st.files_mode, st.commit.is_some())
        };
        this.bar.all_files.set_sensitive(can);
        if files {
            this.bar.all_files.set_active(true);
        } else {
            this.bar.changes.set_active(true);
        }
        if file_tree() {
            this.bar.tree.set_active(true);
        } else {
            this.bar.list.set_active(true);
        }
        this.syncing.set(false);
    }

    /// 帯の部品を、今の見せ方で効くものだけにする（すべてのファイルでは差分にしか効かないものを隠す、フォルダの
    /// 操作は木のとき）。合計も出し直す。
    fn update_bar(this: &Rc<Self>) {
        let files = this.st.borrow().files_mode;
        let tree = file_tree();
        let bar = &this.bar;
        bar.expand_all.set_visible(!files);
        bar.whitespace.widget().set_visible(!files);
        bar.wrap.set_visible(!files);
        bar.folders.set_visible(tree);
        let (kind, tip) = if Inner::all_diffs_open(this) {
            (
                crate::icon::Kind::CompressVertical,
                tr!("Collapse All Diffs"),
            )
        } else {
            (crate::icon::Kind::ExpandVertical, tr!("Expand All Diffs"))
        };
        bar.expand_icon.set(kind, false);
        bar.expand_all.set_tooltip_text(Some(&tip));
        bar.expand_all
            .update_property(&[gtk::accessible::Property::Label(&tip)]);
        let browser = this.browser.borrow().clone();
        if files && let Some(browser) = &browser {
            browser.set_flat(!tree);
        }
        let expanded = if files {
            browser.as_ref().is_some_and(|b| b.any_folder_expanded())
        } else {
            this.st.borrow().collapsed_folders.is_empty()
        };
        let (kind, tip) = if expanded {
            (crate::icon::Kind::FolderFill, tr!("Collapse All Folders"))
        } else {
            (crate::icon::Kind::Folder, tr!("Expand All Folders"))
        };
        bar.folders_icon.set(kind, false);
        bar.folders.set_tooltip_text(Some(&tip));
        bar.folders
            .update_property(&[gtk::accessible::Property::Label(&tip)]);
        Inner::show_totals(this);
    }

    /// 帯の合計（「10 ファイル +9 −8」）。まだ差分を取っていないファイルの行数は、詳細の行数で数える。
    fn show_totals(this: &Rc<Self>) {
        let st = this.st.borrow();
        if st.names.is_empty() {
            this.bar.totals.set_text("");
            return;
        }
        let stats = this.stats.borrow();
        let (mut added, mut removed) = (0, 0);
        for (file, info) in st.infos.iter().enumerate() {
            if info.pending {
                if let Some((a, r)) = stats.get(&Inner::path(&st, file)) {
                    added += a.unwrap_or(0);
                    removed += r.unwrap_or(0);
                }
            } else {
                added += info.added;
                removed += info.removed;
            }
        }
        this.bar.totals.set_markup(&format!(
            "<span alpha=\"55%\">{}</span>  <span foreground=\"#2ec27e\">+{added}</span> <span foreground=\"#e01b24\">−{removed}</span>",
            glib::markup_escape_text(&tr!("{count} files", count = st.names.len()))
        ));
    }

    /// 右の列を「すべてのファイル」（その時点の木と中身）にする・変更に戻す。
    fn set_files_mode(this: &Rc<Self>, on: bool) {
        let (commit, repo, changes) = {
            let st = this.st.borrow();
            (
                st.commit,
                st.source.as_ref().and_then(|s| s.repo.clone()),
                st.changes.clone(),
            )
        };
        let on = on && commit.is_some();
        if on {
            if this.browser.borrow().is_none()
                && let (Some(commit), Some(repo)) = (commit, repo)
            {
                let browser =
                    Browser::new(repo, commit, changes, !file_tree(), this.pending.clone());
                browser.set_github(this.github.borrow().clone());
                browser.set_partial(this.partial.get());
                let weak = Rc::downgrade(this);
                browser.connect_show_change(move |file| {
                    let Some(this) = weak.upgrade() else { return };
                    Inner::set_files_mode(&this, false);
                    let ops = Inner::load_ops(&this, &[file]);
                    this.st.borrow_mut().open.insert(file);
                    Inner::reshow(
                        &this,
                        true,
                        ops,
                        Some(Box::new(move |this| this.diff.scroll_to_file(file))),
                    );
                });
                let weak = Rc::downgrade(this);
                browser.connect_folders_change(move || {
                    if let Some(this) = weak.upgrade() {
                        Inner::update_bar(&this);
                    }
                });
                this.pages.add_named(&browser.root, Some("files"));
                this.browser.replace(Some(browser));
            }
            this.pages.set_visible_child_name("files");
        } else {
            this.pages.set_visible_child_name("changes");
        }
        this.st.borrow_mut().files_mode = on;
        Inner::sync_switches(this);
        Inner::update_bar(this);
    }

    /// その時点のファイルを開く（見出しの ⋯・行の右クリック）。
    fn open_at_commit(this: &Rc<Self>, file: usize, line: Option<u32>) {
        Inner::set_files_mode(this, true);
        let path = Inner::path(&this.st.borrow(), file);
        let browser = this.browser.borrow().clone();
        if let Some(browser) = browser {
            browser.open(&path, line);
        }
    }

    /// `file` 番目のファイルを、その時点のファイルとして開けるか（1 つのコミットで、そのコミットで消していない）。
    fn can_open_at_commit(st: &St, file: usize) -> bool {
        st.commit.is_some()
            && st
                .changes
                .as_ref()
                .and_then(|c| c.get(file))
                .is_some_and(|c| c.kind != ChangeKind::Deleted)
    }

    /// 確かめる用（`--snapshot`）: 開くファイル・木で畳むフォルダ・差分をすべて開く・すべてのファイル
    /// （`AMH_OPEN_FILES=1,6`・`AMH_COLLAPSED_FOLDERS=assets,src/old`・`AMH_EXPAND_ALL=1`・
    /// `AMH_FILES_MODE=tree|path[:line]`。macOS 版の `AMHOpenFiles` などと同じ）。
    fn apply_snapshot_options(this: &Rc<Self>) {
        let open = std::env::var("AMH_OPEN_FILES").ok();
        let folders = std::env::var("AMH_COLLAPSED_FOLDERS").ok();
        let mut ops = Vec::new();
        if let Some(open) = &open {
            let files: Vec<usize> = open
                .split(',')
                .filter_map(|f| f.trim().parse().ok())
                .collect();
            ops = Inner::load_ops(this, &files);
            this.st.borrow_mut().open = files.into_iter().collect();
        }
        if let Some(folders) = &folders {
            this.st.borrow_mut().collapsed_folders = folders
                .split(',')
                .filter(|f| !f.is_empty())
                .map(str::to_owned)
                .collect();
        }
        let expand_all = std::env::var("AMH_EXPAND_ALL").is_ok_and(|v| v == "1");
        let files_mode = std::env::var("AMH_FILES_MODE").ok();
        if open.is_none() && folders.is_none() && !expand_all && files_mode.is_none() {
            return;
        }
        Inner::reshow(
            this,
            false,
            ops,
            Some(Box::new(move |this| {
                if expand_all {
                    Inner::toggle_all_diffs(this);
                }
                if let Some(target) = files_mode {
                    Inner::set_files_mode(this, true);
                    if target != "tree" {
                        let (path, line) = match target.split_once(':') {
                            Some((p, l)) => (p.to_owned(), l.parse().ok()),
                            None => (target.clone(), None),
                        };
                        let browser = this.browser.borrow().clone();
                        if let Some(browser) = browser {
                            browser.open(&path, line);
                        }
                    }
                }
            })),
        );
    }

    /// 表示の設定のメニュー（開くたびに作る）: コミット情報（表示項目・高さ・コミット情報だけを表示）、コード（長い行を
    /// 折り返す）、AI。
    fn view_option_items(this: &Rc<Self>) -> Vec<Item> {
        let (styles, ai) = this
            .view_options
            .borrow()
            .clone()
            .map(|f| f())
            .unwrap_or_default();
        // コミット情報（右の列の上の欄）に効くもの: 表示項目（簡潔・標準・詳細）、高さを自動で決める、コミット
        // 情報だけを表示。コード（差分と「すべてのファイル」の中身）に効くもの: 長い行を折り返す（macOS 版と同じ）。
        let mut items = vec![Item::Heading(tr!("Commit Details"))];
        if !styles.is_empty() {
            items.push(Item::Disabled(tr!("Details Shown")));
            items.extend(styles);
        }
        let expand = this.expand.borrow().clone();
        if let Some(button) = expand {
            let on = button.is_active();
            items.push(menu::choice_with_tip(
                tr!("Show Only Commit Details"),
                on,
                tr!("Expand the commit details to fill the right column (click again to restore)."),
                move || button.set_active(!on),
            ));
        }
        if items.len() == 1 {
            items.clear();
        }
        items.push(Item::Heading(tr!("Code")));
        items.push(crate::diff::wrap_item());
        if !ai.is_empty() {
            items.push(Item::Separator);
            items.extend(ai);
        }
        items
    }

    /// 差分の行の右クリック: この時点のファイルをその行で開く、AI でこの hunk を解説（持ち主が作る）。
    fn row_items(this: &Rc<Self>, path: &str, y: f64) -> Vec<Item> {
        let mut items = Vec::new();
        if let Some((file, line)) = this.diff.line_at(y)
            && Inner::can_open_at_commit(&this.st.borrow(), file)
        {
            let w = Rc::downgrade(this);
            items.push(menu::action(tr!("Open File at This Commit"), move || {
                if let Some(this) = w.upgrade() {
                    Inner::open_at_commit(&this, file, (line > 0).then_some(line));
                }
            }));
        }
        let f = this.file_items.borrow().clone();
        if let Some(f) = f {
            let ai = f(path, Some(y), true, true);
            if !ai.is_empty() && !items.is_empty() {
                items.push(Item::Separator);
            }
            items.extend(ai);
        }
        items
    }

    /// ファイルの見出しの ⋯・右クリックのメニュー（macOS 版と同じ）: 開く・畳む、ファイル全体を表示、この時点の
    /// ファイルを開く、Blame・File History・戻す、パスのコピー・ファイルマネージャーで見る・既定のアプリで開く。
    fn menu_items(this: &Rc<Self>, file: usize) -> Vec<Item> {
        let (path, open, whole, deleted, at_commit, change) = {
            let st = this.st.borrow();
            if file >= st.names.len() {
                return Vec::new();
            }
            let change = Inner::change(&st, file);
            (
                Inner::path(&st, file),
                st.open.contains(&file),
                st.whole.contains(&file),
                change
                    .as_ref()
                    .is_some_and(|c| c.kind == ChangeKind::Deleted),
                Inner::can_open_at_commit(&st, file),
                change,
            )
        };
        let mut items = Vec::new();
        let w = Rc::downgrade(this);
        items.push(menu::action(
            if open { tr!("Collapse") } else { tr!("Expand") },
            move || {
                if let Some(this) = w.upgrade() {
                    Inner::toggle_open(&this, file);
                }
            },
        ));
        let w = Rc::downgrade(this);
        items.push(menu::choice(tr!("Show Whole File"), whole, move || {
            if let Some(this) = w.upgrade() {
                Inner::toggle_whole_file(&this, file);
            }
        }));
        if at_commit {
            let w = Rc::downgrade(this);
            items.push(menu::action(tr!("Open File at This Commit"), move || {
                if let Some(this) = w.upgrade() {
                    Inner::open_at_commit(&this, file, None);
                }
            }));
        }
        items.push(Item::Separator);
        if let Some(f) = this.file_items.borrow().clone() {
            // 消したファイルは、そのコミットにはない（Blame を出さない）。
            items.extend(f(&path, None, !deleted, false));
            let restore = this.restore_items.borrow().clone();
            if let (Some(restore), Some(change)) = (restore, &change) {
                let more = restore(change);
                if !more.is_empty() {
                    items.push(Item::Separator);
                    items.extend(more);
                }
            }
            items.push(Item::Separator);
        }
        let (w, p) = (this.diff.downgrade(), path.clone());
        items.push(menu::action(tr!("Copy Path"), move || {
            if let Some(w) = w.upgrade() {
                w.clipboard().set_text(&p);
            }
        }));
        // 手元のリポジトリのファイルがあれば、ファイルマネージャーとほかのアプリで開ける。
        let workdir = (this.current_repo)()
            .filter(|r| r.connection_state().is_none())
            .and_then(|r| r.paths().workdir);
        let local = workdir
            .as_ref()
            .map(|w| w.join(&path))
            .filter(|p| p.exists());
        let unchecked = workdir.is_some()
            && local.is_none()
            && this
                .sparse
                .borrow()
                .as_ref()
                .is_some_and(|s| !s.contains(&path));
        if unchecked {
            // sparse checkout で取り出していないフォルダのファイル。項目は出すが、押せないわけを添える（macOS 版と同じ）。
            for title in [
                tr!("Show in Files"),
                tr!("Open in Editor"),
                tr!("Open with Default App"),
            ] {
                items.push(menu::disabled_hint(
                    title,
                    tr!("This folder isn’t checked out"),
                ));
            }
        }
        if let Some(full) = local {
            let window = || {
                this.diff
                    .root()
                    .and_downcast::<gtk::Window>()
                    .map(|w| w.downgrade())
            };
            let file = gio::File::for_path(&full);
            let (w, f) = (window(), file.clone());
            items.push(menu::action(tr!("Show in Files"), move || {
                let window = w.as_ref().and_then(|w| w.upgrade());
                gtk::FileLauncher::new(Some(&f)).open_containing_folder(
                    window.as_ref(),
                    gio::Cancellable::NONE,
                    |_| {},
                );
            }));
            let (w, f) = (window(), file.clone());
            items.push(menu::hint(
                tr!("Open in Editor"),
                crate::editor::tooltip(),
                move || {
                    let window = w.as_ref().and_then(|w| w.upgrade());
                    let path = f.path().into_iter().collect::<Vec<_>>();
                    crate::editor::open(&path, None, window.as_ref());
                },
            ));
            let w = window();
            items.push(menu::action(tr!("Open with Default App"), move || {
                let window = w.as_ref().and_then(|w| w.upgrade());
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
    fn lays_out_folders_between_the_headers_and_hides_folded_ones() {
        let paths: Vec<String> = [
            "README",
            "src/a.rs",
            "src/old/x.rs",
            "src/old/y.rs",
            "src/z.rs",
        ]
        .iter()
        .map(|p| (*p).to_owned())
        .collect();
        let mut labels = paths.clone();
        labels[4] = "src/{b.rs → z.rs}".to_owned();
        let names = |tree: &TreeLayout, file: usize| -> Vec<String> {
            tree.folders
                .get(&file)
                .map(|f| f.iter().map(|f| f.path.clone()).collect())
                .unwrap_or_default()
        };
        let tree = tree_layout(&paths, &labels, &HashSet::new());
        assert_eq!(names(&tree, 0), Vec::<String>::new());
        assert_eq!(names(&tree, 1), ["src"]);
        assert_eq!(names(&tree, 2), ["src/old"]);
        assert_eq!(names(&tree, 3), Vec::<String>::new());
        assert_eq!(names(&tree, 4), Vec::<String>::new());
        assert_eq!(tree.depths[&2], 2);
        assert_eq!(tree.titles[&2], "x.rs");
        assert_eq!(tree.titles[&4], "{b.rs → z.rs}");
        assert!(tree.hidden.is_empty());

        // 畳んだフォルダの中は見出しを出さず、そのフォルダの行だけ残す。
        let folded: HashSet<String> = ["src/old".to_owned()].into_iter().collect();
        let tree = tree_layout(&paths, &labels, &folded);
        assert_eq!(tree.hidden, [2, 3].into_iter().collect());
        assert!(tree.folders[&2][0].collapsed);
        let folded: HashSet<String> = ["src".to_owned()].into_iter().collect();
        let tree = tree_layout(&paths, &labels, &folded);
        assert_eq!(tree.hidden, [1, 2, 3, 4].into_iter().collect());
        assert_eq!(names(&tree, 1), ["src"]);
        assert!(!tree.folders.contains_key(&2));
    }

    #[test]
    fn shows_every_file_folded_at_first() {
        let dir = std::env::temp_dir().join(format!("amonhen-gtk-stacked-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        commit_three_files(&dir);
        let backend = amonhen_backend_gix::GixBackend::open(&dir).unwrap();
        let repo = Arc::new(Repository::new(backend));
        let history = repo.load_history().unwrap();
        let prepared = Prepared::commit(&repo, &history, 0, Whitespace::Exact).unwrap();
        assert_eq!(prepared.names, ["a.txt", "b.txt", "c.txt"]);
        // 見出しだけ（すべて畳んでいる）。行数は見出しの行にある。
        assert_eq!(prepared.view.diff_view().rows.len(), 3);
        let infos = prepared.view.file_infos();
        assert!(infos.iter().all(|i| i.added == 1 && !i.pending));
        let kinds: Vec<&str> = prepared
            .changes
            .iter()
            .flat_map(|c| c.iter())
            .map(|c| kind_mark(c.kind))
            .collect();
        assert_eq!(kinds, ["A", "A", "A"]);
        // AI の解説など、全ファイルが要るときはそのまま読める。
        let added = prepared
            .source
            .with_all(|all| all.files.iter().map(|(_, d)| d.added).sum::<u32>());
        assert_eq!(added, 3);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
