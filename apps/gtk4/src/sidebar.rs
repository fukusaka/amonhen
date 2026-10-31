//! サイドバー: Workspace（Working Copy・History・あるときだけ Stashes・Reflog・開いている間だけ Range Diff）/ Branches / Remotes / Tags / Stashes / Submodules。
//!
//! `GtkListView` と `GtkTreeListModel` の木（項目が多くても、見えている行の部品しか作らない）。
//! `feature/x` のような名前はフォルダにまとめる。ref を選ぶと、指すコミットを History で選ぶ。
//! 右クリックとドラッグ＆ドロップ（ブランチ・タグを別のブランチやリモートに落とす）は、窓の側が
//! 操作のメニューにする。
//!
//! Branches はコアの `branchGroups` で並べる（macOS 版の `SidebarViewController.branchesNode` と同じ）: お気に入り（★）、
//! 今のブランチと既定のブランチ、動いているブランチ（新しい順）、Inactive・Archived のフォルダ。ほかのブランチの上に
//! 積んだブランチは、土台の行の下に入れ子で出す（↳）。行の右端の印（取り込み済み・gone・↑↓）を押すと、詳しいことの
//! 吹き出し（`refmarks`）。

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use amonhen_core::RefsSnapshot;
use amonhen_model::{Oid, RefEntry, RefKind, Refs, tr};
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gio, glib};
use serde_json::json;

use crate::actions::Actions;
use crate::icon::{self, Icon};
use crate::menu;
use crate::reffacts::BranchGroups;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    History,
    WorkingCopy,
    /// stash の画面（stash があるときだけ出す）。
    Stashes,
    /// 開いている間だけ出す。
    RangeDiff,
    Reflog,
}

#[derive(Clone, Debug, Default)]
pub enum Item {
    /// 見出し（Branches など）。選べない。
    #[default]
    Group,
    Page(Page),
    Folder,
    Ref(RefEntry),
}

/// 行の右端のアイコン（取り込み済み・gone など）。意味はツールチップの先頭に出す。
#[derive(Clone, Debug)]
pub struct Mark {
    pub icon: icon::Kind,
    pub tooltip: String,
    /// アイコンの色の CSS のクラス（`warning` など）。
    pub css: Option<&'static str>,
}

#[derive(Clone, Debug, Default)]
pub struct Data {
    pub title: String,
    /// 開いた状態と選んだ項目を、作り直した後に戻すための鍵。
    pub key: String,
    pub item: Item,
    pub icon: icon::Kind,
    pub badge: String,
    pub head: bool,
    pub tooltip: Option<String>,
    /// Remotes の下のリモートそのもの（その名前）。
    pub remote: Option<String>,
    /// お気に入り（Branches の一番上）。
    pub pinned: bool,
    /// ほかのブランチの上に積んだブランチなら、その土台。
    pub stacked_on: Option<String>,
    pub marks: Vec<Mark>,
    /// 右端の印・数を押すと吹き出しを出す。
    pub mark_click: bool,
}

mod node_imp {
    use super::*;

    #[derive(Default)]
    pub struct Node {
        pub data: RefCell<Data>,
        pub children: RefCell<Option<gio::ListStore>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Node {
        const NAME: &'static str = "AmonhenSidebarNode";
        type Type = super::Node;
    }

    impl ObjectImpl for Node {}
}

glib::wrapper! {
    pub struct Node(ObjectSubclass<node_imp::Node>);
}

impl Node {
    fn new(data: Data, children: Option<gio::ListStore>) -> Self {
        let node: Self = glib::Object::new();
        node.imp().data.replace(data);
        node.imp().children.replace(children);
        node
    }

    pub fn data(&self) -> Data {
        self.imp().data.borrow().clone()
    }

    fn children(&self) -> Option<gio::ListStore> {
        self.imp().children.borrow().clone()
    }
}

/// 作りかけの行（子の行つき）。最後に [`store`] で木の部品にする。
struct Row {
    data: Data,
    children: Vec<Row>,
    /// 子がなくても開ける行（フォルダ）。
    folder: bool,
}

impl Row {
    fn leaf(data: Data) -> Self {
        Self {
            data,
            children: Vec::new(),
            folder: false,
        }
    }
}

fn store(rows: Vec<Row>) -> gio::ListStore {
    let store = gio::ListStore::new::<Node>();
    for row in rows {
        let children = (row.folder || !row.children.is_empty()).then(|| self::store(row.children));
        store.append(&Node::new(row.data, children));
    }
    store
}

/// 名前を `/` で分けてフォルダにまとめながら、項目を並べる。
#[derive(Default)]
struct Tree {
    children: Vec<(String, Child)>,
}

enum Child {
    Leaf(Box<Data>),
    Folder(Tree),
}

impl Tree {
    fn insert(&mut self, parts: &[&str], data: Data) {
        match parts {
            [] | [_] => self
                .children
                .push((data.title.clone(), Child::Leaf(Box::new(data)))),
            [first, rest @ ..] => {
                let at = self
                    .children
                    .iter()
                    .position(|(n, c)| n == first && matches!(c, Child::Folder(_)));
                let at = at.unwrap_or_else(|| {
                    self.children
                        .push(((*first).to_owned(), Child::Folder(Tree::default())));
                    self.children.len() - 1
                });
                if let Child::Folder(tree) = &mut self.children[at].1 {
                    tree.insert(rest, data);
                }
            }
        }
    }

    fn into_rows(self, key: &str) -> Vec<Row> {
        self.children
            .into_iter()
            .map(|(name, child)| match child {
                Child::Leaf(data) => Row::leaf(*data),
                Child::Folder(tree) => {
                    let key = format!("{key}/{name}");
                    let children = tree.into_rows(&key);
                    Row {
                        data: Data {
                            title: name,
                            key,
                            item: Item::Folder,
                            icon: icon::Kind::Folder,
                            ..Data::default()
                        },
                        children,
                        folder: true,
                    }
                }
            })
            .collect()
    }
}

/// 右クリックのメニューを作る（項目、サイドバーが知っていること）。
type MenuCallback = Box<dyn Fn(&Data, &Known) -> Vec<menu::Item>>;
type SelectCallback = Box<dyn Fn(&Data)>;
/// 落としたもの（`local:<名前>` か `other:<名前>`）、落とした先の項目、メニューを出す場所。
type DropCallback = Box<dyn Fn(&str, &Data, &gtk::Widget, f64, f64)>;

/// サイドバーが ref のほかに知っていること（右クリックのメニューと印の吹き出しが使う）。
#[derive(Clone, Default)]
pub struct Known {
    /// ローカルブランチの分け方（まだ求めていなければ None）。
    pub groups: Option<BranchGroups>,
    /// お気に入りのブランチ（上から順）。
    pub pinned: Vec<String>,
    /// 右クリックした行を含めて、ブランチ・タグを 2 つ以上選んでいれば、その ref（上から順）。
    pub selected: Vec<RefEntry>,
}

/// ドラッグで運ぶ文字列（ローカルブランチの名前にも `/` は入りうるので、種類も付ける）。
pub fn drag_value(data: &Data) -> Option<String> {
    match &data.item {
        Item::Ref(e) if matches!(e.kind, RefKind::Local | RefKind::Remote | RefKind::Tag) => {
            let kind = if e.kind == RefKind::Local {
                "local"
            } else {
                "other"
            };
            Some(format!("{kind}:{}", e.name))
        }
        _ => None,
    }
}

/// 行の部品から、その行の項目。
fn node_of(expander: &gtk::TreeExpander) -> Option<Node> {
    expander.list_row()?.item().and_downcast::<Node>()
}

/// 最初に開いておく見出しの、項目の数の上限。
const MANY: u32 = 30;
/// Inactive とみなす日数（最後のコミットからこれだけ経ったブランチ）。
const STALE_DAYS: i64 = 90;

pub struct Sidebar {
    /// 絞り込みの欄と、項目の一覧。
    pub root: gtk::Box,
    filter: gtk::SearchEntry,
    inner: Rc<Inner>,
}

struct Inner {
    root: gio::ListStore,
    model: gtk::TreeListModel,
    /// ブランチ・タグは複数選べる（まとめて消す・アーカイブする）。
    selection: gtk::MultiSelection,
    working: Node,
    /// Range Diff の行（開いている間だけ Workspace の最後に出す）。
    range_diff: Node,
    range_diff_shown: Cell<bool>,
    generation: RefCell<Option<u64>>,
    /// 最後に出した ref（分け方が届いたら作り直す）。
    refs: RefCell<Option<Refs>>,
    groups: RefCell<Option<BranchGroups>>,
    /// お気に入りのブランチ（上から順。作り直すたびに求める）。
    pinned: RefCell<Vec<String>>,
    /// 畳んだ、積んだブランチのあるブランチ（初めはすべて開いておく）。
    folded: RefCell<HashSet<String>>,
    actions: RefCell<Option<Rc<Actions>>>,
    /// 分け方を読む番号（古い答えは使わない）。
    loading: Cell<u64>,
    on_select: RefCell<Option<SelectCallback>>,
    on_menu: Rc<RefCell<Option<MenuCallback>>>,
    on_drop: Rc<RefCell<Option<DropCallback>>>,
    /// 作り直している間は、選択の変化を知らせない。
    rebuilding: Cell<bool>,
    /// 分け方を読んだら一度だけ行う確認（`--sidebar-menu`・`--sidebar-mark`）。
    debug: RefCell<Option<Debug>>,
    /// サイドバーの部品（確認用のメニューと吹き出しを出す先）。
    anchor: RefCell<glib::WeakRef<gtk::Widget>>,
    /// 絞り込みの語（空白で区切る。小文字。名前がすべてを含む行だけ出す）。空なら絞り込まない。
    filter: RefCell<Vec<String>>,
    /// 絞り込む前に開いていた項目（絞り込みをやめたら戻す）。絞り込んでいる間だけ Some。
    unfiltered: RefCell<Option<HashSet<String>>>,
}

/// 確認用: 分け方を読んだら、この行のメニューか印の吹き出しを出す。
struct Debug {
    menu: Option<String>,
    mark: Option<String>,
    item: Option<String>,
    pending: Rc<Cell<u32>>,
}

impl Sidebar {
    pub fn new() -> Self {
        let root = gio::ListStore::new::<Node>();
        let model = gtk::TreeListModel::new(root.clone(), false, false, |item| {
            item.downcast_ref::<Node>()
                .and_then(Node::children)
                .map(|s| s.upcast())
        });
        let selection = gtk::MultiSelection::new(Some(model.clone()));

        let on_menu: Rc<RefCell<Option<MenuCallback>>> = Rc::default();
        let on_drop: Rc<RefCell<Option<DropCallback>>> = Rc::default();
        // 行の部品から Inner に届くように（Inner を作ったあとで入れる）。
        let owner: Owner = Rc::default();
        let factory = gtk::SignalListItemFactory::new();
        let (drop_cb, o) = (on_drop.clone(), owner.clone());
        factory.connect_setup(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            setup_row(item, &o, &drop_cb);
        });
        factory.connect_bind(|_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            bind_row(item);
        });
        let list = gtk::ListView::new(Some(selection.clone()), Some(factory));
        list.add_css_class("navigation-sidebar");
        let scrolled = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .width_request(180)
            .build();

        let working = Node::new(
            Data {
                title: "Working Copy".into(),
                key: "page/working".into(),
                item: Item::Page(Page::WorkingCopy),
                icon: icon::Kind::WorkingCopy,
                ..Data::default()
            },
            None,
        );
        let range_diff = Node::new(
            Data {
                title: "Range Diff".into(),
                key: "page/range-diff".into(),
                item: Item::Page(Page::RangeDiff),
                icon: icon::Kind::RangeDiff,
                ..Data::default()
            },
            None,
        );
        let inner = Rc::new(Inner {
            root,
            model,
            selection,
            working,
            range_diff,
            range_diff_shown: Cell::new(false),
            generation: RefCell::default(),
            refs: RefCell::default(),
            groups: RefCell::default(),
            pinned: RefCell::default(),
            folded: RefCell::default(),
            actions: RefCell::default(),
            loading: Cell::new(0),
            on_select: RefCell::default(),
            on_menu,
            on_drop,
            rebuilding: Cell::new(false),
            debug: RefCell::default(),
            anchor: RefCell::default(),
            filter: RefCell::default(),
            unfiltered: RefCell::default(),
        });
        inner.anchor.borrow().set(Some(scrolled.upcast_ref()));
        owner.replace(Rc::downgrade(&inner));
        let weak = Rc::downgrade(&inner);
        inner.selection.connect_selection_changed(move |_, _, _| {
            let Some(inner) = weak.upgrade() else { return };
            if inner.rebuilding.get() {
                return;
            }
            // 1 つだけ選んだときに知らせる（複数選んだら、右クリックでまとめて扱う）。
            if let [node] = &inner.selected_nodes()[..]
                && let Some(f) = inner.on_select.borrow().as_ref()
            {
                f(&node.data());
            }
        });
        inner.rebuild(true);

        // 絞り込みの欄は一覧の上に。打つたびに絞り込む（Esc で消す）。
        let filter = gtk::SearchEntry::new();
        filter.set_placeholder_text(Some(&tr!("Filter branches")));
        filter.set_margin_top(6);
        filter.set_margin_bottom(4);
        filter.set_margin_start(8);
        filter.set_margin_end(8);
        let weak = Rc::downgrade(&inner);
        filter.connect_search_changed(move |entry| {
            if let Some(inner) = weak.upgrade() {
                inner.set_filter(&entry.text());
            }
        });
        filter.connect_stop_search(|entry| entry.set_text(""));
        scrolled.set_vexpand(true);
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.append(&filter);
        root.append(&scrolled);
        Self {
            root,
            filter,
            inner,
        }
    }

    /// Navigate > Filter Branches…: 絞り込みの欄にカーソルを移す。
    pub fn focus_filter(&self) {
        self.filter.grab_focus();
    }

    /// 絞り込みの語を入れる（`--filter-sidebar` の確認用）。
    pub fn filter(&self, text: &str) {
        self.filter.set_text(text);
        self.inner.set_filter(text);
    }

    /// 操作に使う（ブランチの分け方を読む、吹き出しのボタン）。操作の後には分け方を読み直す（お気に入り・
    /// アーカイブは ref を変えない）。
    pub fn set_actions(&self, actions: Rc<Actions>) {
        let weak = Rc::downgrade(&self.inner);
        actions.connect_after(move || {
            if let Some(inner) = weak.upgrade() {
                inner.load_facts();
            }
        });
        self.inner.actions.replace(Some(actions));
    }

    /// 項目を選んだら `f(項目)` を呼ぶ。
    pub fn connect_select(&self, f: impl Fn(&Data) + 'static) {
        self.inner.on_select.replace(Some(Box::new(f)));
    }

    /// 右クリックしたら `f(項目, 知っていること)` でメニューの項目を求めて出す。
    pub fn connect_menu(&self, f: impl Fn(&Data, &Known) -> Vec<menu::Item> + 'static) {
        self.inner.on_menu.replace(Some(Box::new(f)));
    }

    /// ブランチ・タグを落としたら `f(運んだもの, 落とした先, 部品, x, y)` を呼ぶ。
    pub fn connect_drop(&self, f: impl Fn(&str, &Data, &gtk::Widget, f64, f64) + 'static) {
        self.inner.on_drop.replace(Some(Box::new(f)));
    }

    /// 確認用: 分け方を読んだら、`menu` の行（ref の名前・リモートの名前・見出しの名前・行の鍵）の右クリックの
    /// メニューを出す（`item` があればその項目を選ぶ）か、`mark` の行の右端の印の吹き出しを出す。撮るのは
    /// 終わってから（`pending`）。
    pub fn debug_once(
        &self,
        menu: Option<String>,
        mark: Option<String>,
        item: Option<String>,
        pending: Rc<Cell<u32>>,
    ) {
        if menu.is_none() && mark.is_none() {
            return;
        }
        pending.set(pending.get() + 1);
        self.inner.debug.replace(Some(Debug {
            menu,
            mark,
            item,
            pending,
        }));
    }

    /// サイドバーが知っていること（右クリックのメニューに渡す）。
    pub fn known(&self) -> Known {
        self.inner.known()
    }

    /// ref の一覧を出す。前と同じ世代なら何もしない。
    pub fn show(&self, refs: &RefsSnapshot) {
        if *self.inner.generation.borrow() == Some(refs.generation) {
            return;
        }
        let first = self
            .inner
            .generation
            .replace(Some(refs.generation))
            .is_none();
        self.inner.refs.replace(Some(refs.refs.clone()));
        self.inner.rebuild(first);
        self.inner.load_facts();
    }

    /// Working Copy の変更の数を出す。
    pub fn set_working_count(&self, count: usize) {
        let node = &self.inner.working;
        node.imp().data.borrow_mut().badge = if count > 0 {
            count.to_string()
        } else {
            String::new()
        };
        // 行を描き直させる（同じ項目を入れ替える）。
        let workspace = self.inner.root.item(0).and_downcast::<Node>();
        if let Some(store) = workspace.and_then(|w| w.children()) {
            store.splice(0, 1, std::slice::from_ref(node));
        }
    }

    /// `page` の項目を選んだ状態にする（知らせは出さない）。
    pub fn select_page(&self, page: Page) {
        let key = match page {
            Page::WorkingCopy => "page/working",
            Page::History => "page/history",
            Page::Stashes => "page/stashes",
            Page::RangeDiff => "page/range-diff",
            Page::Reflog => "page/reflog",
        };
        self.inner.rebuilding.set(true);
        self.inner.select_key(key);
        self.inner.rebuilding.set(false);
    }

    /// Range Diff の行を出す・消す（知らせは出さない）。
    pub fn set_range_diff_shown(&self, shown: bool) {
        if self.inner.range_diff_shown.replace(shown) == shown {
            return;
        }
        let workspace = self.inner.root.item(0).and_downcast::<Node>();
        let Some(store) = workspace.and_then(|w| w.children()) else {
            return;
        };
        self.inner.rebuilding.set(true);
        if shown {
            store.append(&self.inner.range_diff);
        } else if let Some(at) = store.find(&self.inner.range_diff) {
            store.remove(at);
        }
        self.inner.rebuilding.set(false);
    }
}

/// 行の部品から届く Inner。
type Owner = Rc<RefCell<std::rc::Weak<Inner>>>;

/// 行の部品を作る: アイコン、名前、右端の印と数。右クリック・ドラッグ・印のクリック。
fn setup_row(item: &gtk::ListItem, inner: &Owner, drop_cb: &Rc<RefCell<Option<DropCallback>>>) {
    let icon = Icon::default();
    icon.set_valign(gtk::Align::Center);
    let title = gtk::Label::new(None);
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    // 右端: 印のアイコン（いくつか）と、数などの文字。押せる行は押すと吹き出し。
    let marks = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    for _ in 0..2 {
        marks.append(&Icon::default());
    }
    let badge = gtk::Label::new(None);
    badge.add_css_class("dim-label");
    badge.add_css_class("caption");
    let trail = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    trail.append(&marks);
    trail.append(&badge);
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    line.append(&icon);
    line.append(&title);
    line.append(&trail);
    let expander = gtk::TreeExpander::new();
    expander.set_child(Some(&line));
    item.set_child(Some(&expander));

    // 右端の印を押す: 行を選ばずに、詳しいことの吹き出し。
    let press = gtk::GestureClick::new();
    press.set_button(gtk::gdk::BUTTON_PRIMARY);
    let (weak, owner) = (expander.downgrade(), inner.clone());
    press.connect_pressed(move |gesture, _, _, _| {
        let (Some(expander), Some(inner)) = (weak.upgrade(), owner.borrow().upgrade()) else {
            return;
        };
        let Some(node) = node_of(&expander) else {
            return;
        };
        let data = node.data();
        if !data.mark_click {
            return;
        }
        gesture.set_state(gtk::EventSequenceState::Claimed);
        if let Some(trail) = expander
            .child()
            .and_then(|l| l.last_child())
            .and_downcast::<gtk::Box>()
        {
            inner.show_marks(&data, trail.upcast_ref());
        }
    });
    trail.add_controller(press);

    // 右クリック: 操作のメニュー。
    let click = gtk::GestureClick::new();
    click.set_button(gtk::gdk::BUTTON_SECONDARY);
    let (weak, owner) = (expander.downgrade(), inner.clone());
    click.connect_pressed(move |_, _, x, y| {
        let (Some(expander), Some(inner)) = (weak.upgrade(), owner.borrow().upgrade()) else {
            return;
        };
        if let Some(node) = node_of(&expander) {
            inner.popup_menu(&node.data(), expander.upcast_ref(), x, y);
        }
    });
    expander.add_controller(click);

    // ブランチ・タグは運べる。
    let drag = gtk::DragSource::new();
    drag.set_actions(gtk::gdk::DragAction::COPY | gtk::gdk::DragAction::MOVE);
    let weak = expander.downgrade();
    drag.connect_prepare(move |_, _, _| {
        let value = drag_value(&node_of(&weak.upgrade()?)?.data())?;
        Some(gtk::gdk::ContentProvider::for_value(&value.to_value()))
    });
    expander.add_controller(drag);

    let target = gtk::DropTarget::new(
        String::static_type(),
        gtk::gdk::DragAction::COPY | gtk::gdk::DragAction::MOVE,
    );
    let (weak, cb) = (expander.downgrade(), drop_cb.clone());
    target.connect_drop(move |_, value, x, y| {
        let (Some(expander), Ok(dragged)) = (weak.upgrade(), value.get::<String>()) else {
            return false;
        };
        match (node_of(&expander), cb.borrow().as_ref()) {
            (Some(node), Some(f)) => {
                f(&dragged, &node.data(), expander.upcast_ref(), x, y);
                true
            }
            _ => false,
        }
    });
    expander.add_controller(target);
}

fn bind_row(item: &gtk::ListItem) {
    let Some(row) = item.item().and_downcast::<gtk::TreeListRow>() else {
        return;
    };
    let Some(node) = row.item().and_downcast::<Node>() else {
        return;
    };
    let expander = item
        .child()
        .and_downcast::<gtk::TreeExpander>()
        .expect("expander");
    expander.set_list_row(Some(&row));
    let line = expander.child().and_downcast::<gtk::Box>().expect("box");
    let icon = line.first_child().and_downcast::<Icon>().expect("icon");
    let title = icon
        .next_sibling()
        .and_downcast::<gtk::Label>()
        .expect("title");
    let trail = title
        .next_sibling()
        .and_downcast::<gtk::Box>()
        .expect("trail");
    let marks = trail
        .first_child()
        .and_downcast::<gtk::Box>()
        .expect("marks");
    let badge = marks
        .next_sibling()
        .and_downcast::<gtk::Label>()
        .expect("badge");
    let data = node.data();
    let group = matches!(data.item, Item::Group);
    item.set_selectable(!group);
    icon.set(data.icon, data.head);
    title.set_text(&data.title);
    for css in ["heading", "dim-label"] {
        if group {
            title.add_css_class(css);
        } else {
            title.remove_css_class(css);
        }
    }
    let attrs = gtk::pango::AttrList::new();
    if data.head {
        attrs.insert(gtk::pango::AttrInt::new_weight(gtk::pango::Weight::Bold));
    }
    title.set_attributes(Some(&attrs));
    let mut child = marks.first_child();
    let mut i = 0;
    while let Some(w) = child {
        child = w.next_sibling();
        let Ok(mark_icon) = w.downcast::<Icon>() else {
            continue;
        };
        for css in ["warning", "success"] {
            mark_icon.remove_css_class(css);
        }
        match data.marks.get(i) {
            Some(mark) => {
                mark_icon.set(mark.icon, false);
                if let Some(css) = mark.css {
                    mark_icon.add_css_class(css);
                }
            }
            None => mark_icon.set(icon::Kind::None, false),
        }
        i += 1;
    }
    marks.set_visible(!data.marks.is_empty());
    badge.set_text(&data.badge);
    badge.set_visible(!data.badge.is_empty());
    trail.set_visible(!data.marks.is_empty() || !data.badge.is_empty());
    line.set_tooltip_text(data.tooltip.as_deref());
}

impl Inner {
    fn known(&self) -> Known {
        Known {
            groups: self.groups.borrow().clone(),
            pinned: self.pinned.borrow().clone(),
            selected: Vec::new(),
        }
    }

    /// 選んでいる行の項目（上から順）。
    fn selected_nodes(&self) -> Vec<Node> {
        let set = self.selection.selection();
        let mut out = Vec::new();
        for i in 0..set.size() {
            let at = set.nth(i as u32);
            if let Some(node) = self
                .model
                .row(at)
                .and_then(|r| r.item().and_downcast::<Node>())
            {
                out.push(node);
            }
        }
        out
    }

    /// `data` の行で開くメニューが扱う ref: 選んだ行の上で開いたら選んだブランチ・タグ（2 つ以上のときだけ）。
    fn selected_refs(&self, data: &Data) -> Vec<RefEntry> {
        let nodes = self.selected_nodes();
        if !nodes.iter().any(|n| n.data().key == data.key) {
            return Vec::new();
        }
        let refs: Vec<RefEntry> = nodes
            .iter()
            .filter_map(|n| match n.data().item {
                Item::Ref(e)
                    if matches!(e.kind, RefKind::Local | RefKind::Remote | RefKind::Tag) =>
                {
                    Some(e)
                }
                _ => None,
            })
            .collect();
        if refs.len() > 1 { refs } else { Vec::new() }
    }

    /// 右クリックのメニューを出す。
    fn popup_menu(&self, data: &Data, widget: &gtk::Widget, x: f64, y: f64) {
        let known = Known {
            selected: self.selected_refs(data),
            ..self.known()
        };
        let items = match self.on_menu.borrow().as_ref() {
            Some(f) => f(data, &known),
            None => return,
        };
        menu::popup(widget, x, y, items);
    }

    /// 絞り込みの語を変える（変わったら作り直す）。
    fn set_filter(&self, text: &str) {
        let words: Vec<String> = text.split_whitespace().map(str::to_lowercase).collect();
        if words == *self.filter.borrow() {
            return;
        }
        self.filter.replace(words);
        self.rebuild(false);
    }

    /// 行の右端の印を押した: 詳しいことの吹き出し。
    fn show_marks(&self, data: &Data, anchor: &gtk::Widget) {
        let Some(actions) = self.actions.borrow().clone() else {
            return;
        };
        if let Item::Ref(entry) = &data.item
            && entry.kind == RefKind::Local
        {
            crate::refmarks::branch(&actions, entry, &self.known(), data.head, anchor);
        }
    }

    /// ローカルブランチの分け方を裏で読み、変わっていたら作り直す。
    fn load_facts(self: &Rc<Self>) {
        let Some(repo) = self.actions.borrow().as_ref().and_then(|a| a.repo()) else {
            return;
        };
        let token = self.loading.get() + 1;
        self.loading.set(token);
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let reply = crate::sheets::run(&repo, json!({ "op": "branchGroups" })).await;
            let Some(inner) = weak.upgrade() else { return };
            if inner.loading.get() != token {
                return;
            }
            let groups = BranchGroups::from(&reply["info"]);
            if groups.is_some() && groups != *inner.groups.borrow() {
                inner.groups.replace(groups);
                inner.rebuild(false);
            }
            inner.run_debug();
        });
    }

    /// 木の中から `spec`（ref の名前・リモートの名前・見出しの名前・行の鍵）の項目を探す。
    fn find(&self, spec: &str) -> Option<Data> {
        fn walk(store: &gio::ListStore, spec: &str) -> Option<Data> {
            for i in 0..store.n_items() {
                let node = store.item(i).and_downcast::<Node>()?;
                let data = node.data();
                let hit = data.key == spec
                    || data.remote.as_deref() == Some(spec)
                    || (matches!(data.item, Item::Group) && data.title == spec)
                    || matches!(&data.item, Item::Ref(e) if e.name == spec);
                if hit {
                    return Some(data);
                }
                if let Some(found) = node.children().and_then(|c| walk(&c, spec)) {
                    return Some(found);
                }
            }
            None
        }
        walk(&self.root, spec)
    }

    fn run_debug(&self) {
        let Some(debug) = self.debug.take() else {
            return;
        };
        let anchor = self
            .root_widget()
            .unwrap_or_else(|| gtk::Label::new(None).upcast());
        let mut wait = 300;
        if let Some(spec) = &debug.menu {
            // `a,b` は、選んだ 2 つの上で開いたメニュー。
            let found: Vec<Data> = spec.split(',').filter_map(|s| self.find(s)).collect();
            let selected: Vec<RefEntry> = found
                .iter()
                .filter_map(|d| match &d.item {
                    Item::Ref(e) => Some(e.clone()),
                    _ => None,
                })
                .collect();
            // 見えている行なら、選んだ状態にしておく。
            for i in 0..self.model.n_items() {
                if let Some(node) = self
                    .model
                    .row(i)
                    .and_then(|r| r.item().and_downcast::<Node>())
                    && found.iter().any(|d| d.key == node.data().key)
                {
                    self.selection.select_item(i, false);
                }
            }
            match found.into_iter().next() {
                Some(data) => {
                    let known = Known {
                        selected: if selected.len() > 1 {
                            selected
                        } else {
                            Vec::new()
                        },
                        ..self.known()
                    };
                    let items = match self.on_menu.borrow().as_ref() {
                        Some(f) => f(&data, &known),
                        None => Vec::new(),
                    };
                    eprintln!("{}", menu::titles(&items).join("\n"));
                    match &debug.item {
                        Some(path) => {
                            if !menu::activate(&items, path) {
                                eprintln!("メニューの項目がない: {path}");
                            }
                            wait = 2000;
                        }
                        None => menu::popup(&anchor, 24.0, 120.0, items),
                    }
                }
                None => eprintln!("サイドバーの行がない: {spec}"),
            }
        }
        if let Some(spec) = &debug.mark {
            match self.find(spec) {
                Some(data) => {
                    let trail = self.row_trail(&data.key).unwrap_or(anchor);
                    self.show_marks(&data, &trail);
                }
                None => eprintln!("サイドバーの行がない: {spec}"),
            }
            // 吹き出しの中で、コンフリクトの見込みを読み終えるまで待つ。
            wait = 1500;
        }
        let pending = debug.pending;
        glib::timeout_add_local_once(std::time::Duration::from_millis(wait), move || {
            pending.set(pending.get() - 1)
        });
    }

    /// 見えている行のうち `key` の行の、右端の印と数の部品。
    fn row_trail(&self, key: &str) -> Option<gtk::Widget> {
        fn walk(w: &gtk::Widget, key: &str) -> Option<gtk::Widget> {
            if let Some(expander) = w.downcast_ref::<gtk::TreeExpander>() {
                return node_of(expander)
                    .filter(|n| n.data().key == key)
                    .and_then(|_| expander.child()?.last_child());
            }
            let mut child = w.first_child();
            while let Some(c) = child {
                if let Some(found) = walk(&c, key) {
                    return Some(found);
                }
                child = c.next_sibling();
            }
            None
        }
        walk(&self.root_widget()?, key)
    }

    /// サイドバーの部品（確認用のメニューと吹き出しを出す先）。
    fn root_widget(&self) -> Option<gtk::Widget> {
        self.anchor.borrow().upgrade()
    }

    fn rebuild(&self, first: bool) {
        self.rebuilding.set(true);
        // 開いていた項目と、選んでいた項目を覚えておく。積んだブランチのある行は、畳んだものを覚える。
        let mut expanded = HashSet::new();
        {
            let mut folded = self.folded.borrow_mut();
            for i in 0..self.model.n_items() {
                let Some(row) = self.model.row(i) else {
                    continue;
                };
                let Some(node) = row.item().and_downcast::<Node>() else {
                    continue;
                };
                let data = node.data();
                if matches!(data.item, Item::Ref(_)) && row.is_expandable() {
                    if row.is_expanded() {
                        folded.remove(&data.key);
                    } else {
                        folded.insert(data.key.clone());
                    }
                }
                if row.is_expanded() {
                    expanded.insert(data.key);
                }
            }
        }
        let selected: Vec<String> = self.selected_nodes().iter().map(|n| n.data().key).collect();
        // 絞り込んでいる間に開いた見出しは覚えない（絞り込みをやめたら、前の開き方に戻す）。
        let filtering = !self.filter.borrow().is_empty();
        let expanded = match (self.unfiltered.take(), filtering) {
            (Some(before), true) => {
                self.unfiltered.replace(Some(before.clone()));
                before
            }
            (Some(before), false) => before,
            (None, true) => {
                self.unfiltered.replace(Some(expanded.clone()));
                expanded
            }
            (None, false) => expanded,
        };

        let refs = self.refs.borrow().clone();
        let history = Node::new(
            Data {
                title: "History".into(),
                key: "page/history".into(),
                item: Item::Page(Page::History),
                icon: icon::Kind::History,
                ..Data::default()
            },
            None,
        );
        // いつも Workspace の最後に出す。選ぶと Reflog の画面を開く。
        let reflog = Node::new(
            Data {
                title: "Reflog".into(),
                key: "page/reflog".into(),
                item: Item::Page(Page::Reflog),
                icon: icon::Kind::Reflog,
                ..Data::default()
            },
            None,
        );
        let workspace = gio::ListStore::new::<Node>();
        workspace.append(&self.working);
        workspace.append(&history);
        // stash があるときだけ、数を添えて出す（macOS 版と同じ）。
        let stashes = refs.as_ref().map_or(0, |r| {
            r.entries
                .iter()
                .filter(|e| e.kind == RefKind::Stash)
                .count()
        });
        if stashes > 0 {
            workspace.append(&Node::new(
                Data {
                    title: "Stashes".into(),
                    key: "page/stashes".into(),
                    item: Item::Page(Page::Stashes),
                    icon: icon::Kind::Stash,
                    badge: stashes.to_string(),
                    ..Data::default()
                },
                None,
            ));
        }
        workspace.append(&reflog);
        if self.range_diff_shown.get() {
            workspace.append(&self.range_diff);
        }
        let mut groups = vec![("Workspace", workspace)];
        if let Some(refs) = &refs {
            if filtering {
                groups.extend(filtered_groups(refs, &self.filter.borrow()));
            } else {
                let branch_groups = self.groups.borrow();
                let (built, pinned) = groups_of(refs, branch_groups.as_ref());
                self.pinned.replace(pinned);
                groups.extend(built);
            }
        }
        let nodes: Vec<Node> = groups
            .into_iter()
            .map(|(title, children)| {
                Node::new(
                    Data {
                        title: title.into(),
                        key: format!("group/{title}"),
                        ..Data::default()
                    },
                    Some(children),
                )
            })
            .collect();
        self.root.splice(0, self.root.n_items(), &nodes);

        // 見出しは最初は開いておく（項目の多い Tags は閉じておく）。あとは前に開いていたものを開く
        // （開くと子の行が後ろに増える）。積んだブランチのある行は、畳んだもののほかは開く。
        let folded = self.folded.borrow();
        let mut i = 0;
        while i < self.model.n_items() {
            if let Some(row) = self.model.row(i)
                && let Some(node) = row.item().and_downcast::<Node>()
            {
                let data = node.data();
                // 絞り込み中は、見出しを開いて出す。
                let group = matches!(data.item, Item::Group)
                    && (filtering || node.children().is_some_and(|c| c.n_items() <= MANY));
                let stack = matches!(data.item, Item::Ref(_)) && !folded.contains(&data.key);
                if ((first || filtering) && group) || expanded.contains(&data.key) || stack {
                    row.set_expanded(true);
                }
            }
            i += 1;
        }
        drop(folded);
        // 作り直す前に選んでいた行を選び直す（なければ History）。
        self.selection.unselect_all();
        let mut found = false;
        for i in 0..self.model.n_items() {
            if let Some(node) = self
                .model
                .row(i)
                .and_then(|r| r.item().and_downcast::<Node>())
                && selected.contains(&node.data().key)
            {
                self.selection.select_item(i, false);
                found = true;
            }
        }
        if !found {
            self.select_key("page/history");
        }
        self.rebuilding.set(false);
    }

    fn select_key(&self, key: &str) {
        for i in 0..self.model.n_items() {
            if let Some(row) = self.model.row(i)
                && let Some(node) = row.item().and_downcast::<Node>()
                && node.data().key == key
            {
                self.selection.select_item(i, true);
                return;
            }
        }
    }
}

/// 分け方のフォルダの説明（ツールチップ）。
fn branch_group_help(key: &str) -> Option<String> {
    match key {
        "inactive" => Some(tr!("Branches with no new commit for 90 days. Folded.")),
        "archived" => Some(tr!(
            "Branches you archived from their menu. Folded. Archiving is kept in git’s config, so it follows a rename."
        )),
        _ => None,
    }
}

/// ref の行の中身。ローカルブランチは右端に印（取り込み済み・gone）と上流との差（↑ 先にある、↓ 取り込んでいない）。
fn ref_data(
    e: &RefEntry,
    title: &str,
    icon: icon::Kind,
    head: Option<&str>,
    groups: Option<&BranchGroups>,
) -> Data {
    let is_head = e.kind == RefKind::Local && Some(e.name.as_str()) == head;
    let mut data = Data {
        title: title.to_owned(),
        key: format!("{:?}/{}", e.kind, e.name),
        item: Item::Ref(e.clone()),
        icon,
        head: is_head,
        tooltip: e.detail.clone(),
        ..Data::default()
    };
    if e.kind != RefKind::Local {
        return data;
    }
    if !e.gone {
        let mut parts = Vec::new();
        if e.ahead > 0 {
            parts.push(format!("↑{}", e.ahead));
        }
        if e.behind > 0 {
            parts.push(format!("↓{}", e.behind));
        }
        data.badge = parts.join(" ");
    }
    // 取り込み済み（既定のブランチの先端と同じ、作ったばかりのものは除く）と gone は、文字でなくアイコンで。
    if let Some(groups) = groups
        && groups.is_merged(&e.name)
        && !is_head
        && Some(e.name.as_str()) != groups.base.as_deref()
    {
        data.marks.push(Mark {
            icon: icon::Kind::Merge,
            tooltip: tr!("Merged into {base}", base = groups.base_name()),
            css: None,
        });
    }
    if e.gone {
        data.marks.push(Mark {
            icon: icon::Kind::CloudSlash,
            tooltip: tr!(
                "Its upstream {upstream} no longer exists on the remote",
                upstream = e.detail.as_deref().unwrap_or_default()
            ),
            css: None,
        });
    }
    data
}

/// 印のある・数のあるローカルブランチは押せるようにし、印の意味をツールチップの先頭に置く。
fn finish_branch(data: &mut Data) {
    if !matches!(&data.item, Item::Ref(e) if e.kind == RefKind::Local) {
        return;
    }
    // お気に入りに出した積んだブランチは入れ子にならないので、↳ を右に添える（土台はツールチップ）。
    if data.pinned && data.stacked_on.is_some() {
        data.badge = if data.badge.is_empty() {
            "↳".into()
        } else {
            format!("↳ {}", data.badge)
        };
    }
    if let Some(parent) = &data.stacked_on {
        data.tooltip = Some(tr!(
            "Built on {branch} (its commits are on top of that branch)",
            branch = parent
        ));
    }
    if !data.marks.is_empty() {
        let mut lines: Vec<String> = data.marks.iter().map(|m| m.tooltip.clone()).collect();
        lines.extend(data.tooltip.take());
        data.tooltip = Some(lines.join("\n"));
    }
    data.mark_click = !data.marks.is_empty() || !data.badge.is_empty();
}

/// Branches の中身と、お気に入りの順。分け方が分かっていれば、上から順に:
/// - お気に入り（★。決めた順。積んだブランチでも入れ子にせず、ここに平らに出す）
/// - 今のブランチ（その積み重ねの根元ごと）と既定のブランチ
/// - 動いているブランチ（新しい順。`/` の名前はフォルダ）
/// - Inactive（最後のコミットから 90 日以上。畳んでおく）
/// - Archived（手でアーカイブしたもの。畳んでおく）
///
/// ほかのブランチの上に積んだブランチは、その土台のブランチの行の下に入れ子で出す（↳ の印）。
fn branch_rows(
    locals: &[&RefEntry],
    head: Option<&str>,
    groups: Option<&BranchGroups>,
) -> (Vec<Row>, Vec<String>) {
    let leaf = |e: &RefEntry, title: &str, icon: icon::Kind| {
        let mut d = ref_data(e, title, icon, head, groups);
        finish_branch(&mut d);
        d
    };
    let folders = |list: &[&RefEntry]| {
        let mut tree = Tree::default();
        for e in list {
            let parts: Vec<&str> = e.name.split('/').collect();
            tree.insert(&parts, leaf(e, parts[parts.len() - 1], icon::Kind::Branch));
        }
        tree.into_rows("Branches")
    };
    let Some(groups) = groups else {
        return (folders(locals), Vec::new());
    };
    let fact = |name: &str| groups.get(name);
    let by_name: HashMap<&str, &RefEntry> = locals.iter().map(|e| (e.name.as_str(), *e)).collect();
    let base_name = groups.base.as_deref().map(|name| {
        if by_name.contains_key(name) {
            name
        } else {
            name.split_once('/').map_or(name, |(_, b)| b)
        }
    });
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let time = |name: &str| fact(name).and_then(|f| f.time).unwrap_or(0);
    let newest = |list: &mut Vec<&RefEntry>| list.sort_by_key(|e| std::cmp::Reverse(time(&e.name)));
    let archived = |name: &str| fact(name).is_some_and(|f| f.archived);
    let pinned = |name: &str| fact(name).and_then(|f| f.pinned).is_some();
    // 今のブランチがほかのブランチの上に積んであれば、その積み重ねの根元ごと一番上に出す（積み重ねを切らない）。
    let mut stack_root = head.map(str::to_owned);
    let mut seen = HashSet::new();
    while let Some(name) = stack_root.clone() {
        let Some(parent) = fact(&name).and_then(|f| f.parent.clone()) else {
            break;
        };
        if seen.contains(&parent) || !by_name.contains_key(parent.as_str()) || archived(&parent) {
            break;
        }
        seen.insert(name);
        stack_root = Some(parent);
    }
    // 根元がお気に入りなら、そのお気に入りの下に入れ子で出す（お気に入りはもう一番上にある）。
    let is_top = |name: &str| {
        (Some(name) == stack_root.as_deref() && !pinned(name)) || Some(name) == base_name
    };
    // 土台のブランチ → その上に積んだブランチ（土台がアーカイブ済みなら、積んだほうは自分の場所に出す。
    // お気に入りは入れ子にしない）。
    let mut stacked: HashMap<String, Vec<&RefEntry>> = HashMap::new();
    let mut nested = HashSet::new();
    for e in locals {
        if is_top(&e.name) || archived(&e.name) || pinned(&e.name) {
            continue;
        }
        let Some(parent) = fact(&e.name).and_then(|f| f.parent.as_deref()) else {
            continue;
        };
        if !by_name.contains_key(parent) || archived(parent) {
            continue;
        }
        stacked.entry(parent.to_owned()).or_default().push(e);
        nested.insert(e.name.clone());
    }
    for list in stacked.values_mut() {
        newest(list);
    }
    // 積んだブランチを、土台の行の下に（さらに積んだものはその下に）付ける。積んだブランチの行は土台とは別の名前
    // なので、名前を全部出す。
    fn attach(
        row: &mut Row,
        stacked: &HashMap<String, Vec<&RefEntry>>,
        make: &dyn Fn(&RefEntry, Option<&str>) -> Data,
        depth: usize,
    ) {
        if depth > 32 {
            return;
        }
        match &row.data.item {
            Item::Ref(e) => {
                let name = e.name.clone();
                row.children = stacked
                    .get(&name)
                    .into_iter()
                    .flatten()
                    .map(|child| {
                        let mut r = Row::leaf(make(child, Some(&name)));
                        attach(&mut r, stacked, make, depth + 1);
                        r
                    })
                    .collect();
            }
            _ => {
                for child in &mut row.children {
                    attach(child, stacked, make, depth + 1);
                }
            }
        }
    }
    let make = |e: &RefEntry, on: Option<&str>| {
        let mut d = ref_data(
            e,
            &e.name,
            if on.is_some() {
                icon::Kind::Stacked
            } else {
                icon::Kind::Branch
            },
            head,
            Some(groups),
        );
        d.stacked_on = on.map(str::to_owned);
        finish_branch(&mut d);
        d
    };
    let tree = |e: &RefEntry| {
        let mut r = Row::leaf(make(e, None));
        attach(&mut r, &stacked, &make, 0);
        r
    };
    let (mut stars, mut top, mut active, mut inactive, mut archive) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for e in locals.iter().copied() {
        if nested.contains(&e.name) {
            continue;
        }
        if pinned(&e.name) {
            stars.push(e);
        } else if is_top(&e.name) {
            top.push(e);
        } else if archived(&e.name) {
            archive.push(e);
        } else if fact(&e.name)
            .and_then(|f| f.time)
            .is_some_and(|t| now - t > STALE_DAYS * 86_400)
        {
            inactive.push(e);
        } else {
            active.push(e);
        }
    }
    // お気に入りは決めた順（順位が同じなら名前順）。積んだブランチでも平らに出し、土台は行の右に書く。
    stars.sort_by_key(|e| {
        (
            fact(&e.name).and_then(|f| f.pinned).unwrap_or(0),
            e.name.clone(),
        )
    });
    let order: Vec<String> = stars.iter().map(|e| e.name.clone()).collect();
    let mut rows: Vec<Row> = stars
        .iter()
        .map(|e| {
            let mut d = ref_data(e, &e.name, icon::Kind::Star, head, Some(groups));
            d.pinned = true;
            d.stacked_on = fact(&e.name)
                .and_then(|f| f.parent.clone())
                .filter(|p| by_name.contains_key(p.as_str()));
            finish_branch(&mut d);
            let mut r = Row::leaf(d);
            // お気に入りの上に積んだブランチは、お気に入りの下に入れ子で出す。
            attach(&mut r, &stacked, &make, 0);
            r
        })
        .collect();
    top.sort_by_key(|e| Some(e.name.as_str()) != stack_root.as_deref());
    rows.extend(top.iter().map(|e| tree(e)));
    // 動いているブランチは新しい順。`/` のフォルダは、中でいちばん新しいブランチの位置に出る。
    newest(&mut active);
    let mut named = folders(&active);
    for r in &mut named {
        attach(r, &stacked, &make, 0);
    }
    rows.extend(named);
    let group = |key: &str, title: &str, list: &[&RefEntry]| -> Option<Row> {
        if list.is_empty() {
            return None;
        }
        Some(Row {
            data: Data {
                title: title.to_owned(),
                key: format!("Branches/#{key}"),
                item: Item::Folder,
                icon: icon::Kind::Folder,
                badge: list.len().to_string(),
                tooltip: branch_group_help(key),
                ..Data::default()
            },
            children: list.iter().map(|e| tree(e)).collect(),
            folder: true,
        })
    };
    newest(&mut inactive);
    rows.extend(group("inactive", "Inactive", &inactive));
    archive.sort_by(|a, b| a.name.cmp(&b.name));
    rows.extend(group("archived", "Archived", &archive));
    (rows, order)
}

/// ref の種類ごとの見出しと、その中の項目（空の見出しは出さない）と、お気に入りの順。
fn groups_of(
    refs: &Refs,
    branch_groups: Option<&BranchGroups>,
) -> (Vec<(&'static str, gio::ListStore)>, Vec<String>) {
    let of = |kind: RefKind| refs.entries.iter().filter(move |e| e.kind == kind);
    let head = refs.head_branch.as_deref();
    let mut groups = Vec::new();

    let locals: Vec<&RefEntry> = of(RefKind::Local).collect();
    let (rows, pinned) = branch_rows(&locals, head, branch_groups);
    if !rows.is_empty() {
        groups.push(("Branches", store(rows)));
    }

    // リモートごとにまとめ、その中もフォルダにまとめる。
    let mut remotes = Tree::default();
    for e in of(RefKind::Remote) {
        let parts: Vec<&str> = e.name.split('/').collect();
        remotes.insert(
            &parts,
            ref_data(e, parts[parts.len() - 1], icon::Kind::Branch, head, None),
        );
    }
    let mut remotes = remotes.into_rows("Remotes");
    for row in &mut remotes {
        if matches!(row.data.item, Item::Folder) {
            row.data.icon = icon::Kind::Remote;
            row.data.remote = Some(row.data.title.clone());
        }
    }
    if !remotes.is_empty() {
        groups.push(("Remotes", store(remotes)));
    }

    let mut simple = |title: &'static str, kind: RefKind, icon: icon::Kind| {
        let store = gio::ListStore::new::<Node>();
        for e in of(kind) {
            // stash はメッセージを名前に、名前は説明に。
            let mut d = ref_data(e, &e.name, icon, head, None);
            if kind == RefKind::Stash
                && let Some(message) = &e.detail
            {
                d.title = message.clone();
                d.tooltip = Some(e.name.clone());
            }
            store.append(&Node::new(d, None));
        }
        if store.n_items() > 0 {
            groups.push((title, store));
        }
    };
    simple("Tags", RefKind::Tag, icon::Kind::Tag);
    simple("Stashes", RefKind::Stash, icon::Kind::Stash);
    simple("Submodules", RefKind::Submodule, icon::Kind::Submodule);
    (groups, pinned)
}

/// 絞り込み中の見出し: 名前が `words` をすべて含むブランチ・リモートブランチ・タグを、名前のまま平らに
/// （フォルダや積み重ねにはしない）。
fn filtered_groups(refs: &Refs, words: &[String]) -> Vec<(&'static str, gio::ListStore)> {
    let head = refs.head_branch.as_deref();
    let mut groups = Vec::new();
    for (title, kind, icon) in [
        ("Branches", RefKind::Local, icon::Kind::Branch),
        ("Remotes", RefKind::Remote, icon::Kind::Branch),
        ("Tags", RefKind::Tag, icon::Kind::Tag),
    ] {
        let mut matched: Vec<&RefEntry> = refs
            .entries
            .iter()
            .filter(|e| e.kind == kind)
            .filter(|e| {
                let name = e.name.to_lowercase();
                words.iter().all(|w| name.contains(w.as_str()))
            })
            .collect();
        if matched.is_empty() {
            continue;
        }
        matched.sort_by(|a, b| a.name.cmp(&b.name));
        let store = gio::ListStore::new::<Node>();
        for e in matched {
            store.append(&Node::new(ref_data(e, &e.name, icon, head, None), None));
        }
        groups.push((title, store));
    }
    groups
}

/// ref が指すコミット（History で選ぶ）。
pub fn target(data: &Data) -> Option<Oid> {
    match &data.item {
        Item::Ref(e) => e.target,
        _ => None,
    }
}
