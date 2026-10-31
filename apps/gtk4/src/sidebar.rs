//! サイドバー: Workspace（Working Copy・History・あるときだけ Stashes）/ Branches / Remotes / Tags / Stashes / Submodules。
//!
//! `GtkListView` と `GtkTreeListModel` の木（項目が多くても、見えている行の部品しか作らない）。
//! `feature/x` のような名前はフォルダにまとめる。ref を選ぶと、指すコミットを History で選ぶ。
//! 右クリックとドラッグ＆ドロップ（ブランチ・タグを別のブランチやリモートに落とす）は、窓の側が
//! 操作のメニューにする。

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use amonhen_core::RefsSnapshot;
use amonhen_model::{Oid, RefEntry, RefKind};
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gio, glib};

use crate::icon::{self, Icon};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    History,
    WorkingCopy,
    /// stash の画面（stash があるときだけ出す）。
    Stashes,
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

/// 名前を `/` で分けてフォルダにまとめながら、項目を並べる。
#[derive(Default)]
struct Tree {
    children: Vec<(String, Child)>,
}

enum Child {
    Leaf(Data),
    Folder(Tree),
}

impl Tree {
    fn insert(&mut self, parts: &[&str], data: Data) {
        match parts {
            [] | [_] => self.children.push((data.title.clone(), Child::Leaf(data))),
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

    fn into_store(self, key: &str) -> gio::ListStore {
        let store = gio::ListStore::new::<Node>();
        for (name, child) in self.children {
            match child {
                Child::Leaf(data) => store.append(&Node::new(data, None)),
                Child::Folder(tree) => {
                    let key = format!("{key}/{name}");
                    let children = tree.into_store(&key);
                    let data = Data {
                        title: name,
                        key,
                        item: Item::Folder,
                        icon: icon::Kind::Folder,
                        ..Data::default()
                    };
                    store.append(&Node::new(data, Some(children)));
                }
            }
        }
        store
    }
}

type SelectCallback = Box<dyn Fn(&Data)>;
/// 右クリックした項目と、メニューを出す場所。
type MenuCallback = Box<dyn Fn(&Data, &gtk::Widget, f64, f64)>;
/// 落としたもの（`local:<名前>` か `other:<名前>`）、落とした先の項目、メニューを出す場所。
type DropCallback = Box<dyn Fn(&str, &Data, &gtk::Widget, f64, f64)>;

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

pub struct Sidebar {
    pub root: gtk::ScrolledWindow,
    inner: Rc<Inner>,
}

struct Inner {
    root: gio::ListStore,
    model: gtk::TreeListModel,
    selection: gtk::SingleSelection,
    working: Node,
    generation: RefCell<Option<u64>>,
    on_select: RefCell<Option<SelectCallback>>,
    on_menu: Rc<RefCell<Option<MenuCallback>>>,
    on_drop: Rc<RefCell<Option<DropCallback>>>,
    /// 作り直している間は、選択の変化を知らせない。
    rebuilding: std::cell::Cell<bool>,
}

impl Sidebar {
    pub fn new() -> Self {
        let root = gio::ListStore::new::<Node>();
        let model = gtk::TreeListModel::new(root.clone(), false, false, |item| {
            item.downcast_ref::<Node>()
                .and_then(Node::children)
                .map(|s| s.upcast())
        });
        let selection = gtk::SingleSelection::new(Some(model.clone()));
        selection.set_autoselect(false);
        selection.set_can_unselect(true);

        let on_menu: Rc<RefCell<Option<MenuCallback>>> = Rc::default();
        let on_drop: Rc<RefCell<Option<DropCallback>>> = Rc::default();
        let factory = gtk::SignalListItemFactory::new();
        let (menu_cb, drop_cb) = (on_menu.clone(), on_drop.clone());
        factory.connect_setup(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            let icon = Icon::default();
            icon.set_valign(gtk::Align::Center);
            let title = gtk::Label::new(None);
            title.set_xalign(0.0);
            title.set_hexpand(true);
            title.set_ellipsize(gtk::pango::EllipsizeMode::End);
            let badge = gtk::Label::new(None);
            badge.add_css_class("dim-label");
            badge.add_css_class("caption");
            let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            line.append(&icon);
            line.append(&title);
            line.append(&badge);
            let expander = gtk::TreeExpander::new();
            expander.set_child(Some(&line));
            item.set_child(Some(&expander));

            // 右クリック: 操作のメニュー。
            let click = gtk::GestureClick::new();
            click.set_button(gtk::gdk::BUTTON_SECONDARY);
            let (weak, cb) = (expander.downgrade(), menu_cb.clone());
            click.connect_pressed(move |_, _, x, y| {
                let Some(expander) = weak.upgrade() else {
                    return;
                };
                if let (Some(node), Some(f)) = (node_of(&expander), cb.borrow().as_ref()) {
                    f(&node.data(), expander.upcast_ref(), x, y);
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
        });
        factory.connect_bind(|_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
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
            let badge = title
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
            badge.set_text(&data.badge);
            badge.set_visible(!data.badge.is_empty());
            line.set_tooltip_text(data.tooltip.as_deref());
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
        let inner = Rc::new(Inner {
            root,
            model,
            selection,
            working,
            generation: RefCell::default(),
            on_select: RefCell::default(),
            on_menu,
            on_drop,
            rebuilding: std::cell::Cell::new(false),
        });
        let weak = Rc::downgrade(&inner);
        inner
            .selection
            .connect_selected_item_notify(move |selection| {
                let Some(inner) = weak.upgrade() else { return };
                if inner.rebuilding.get() {
                    return;
                }
                let node = selection
                    .selected_item()
                    .and_downcast::<gtk::TreeListRow>()
                    .and_then(|r| r.item().and_downcast::<Node>());
                if let (Some(node), Some(f)) = (node, inner.on_select.borrow().as_ref()) {
                    f(&node.data());
                }
            });
        inner.rebuild(None, true);
        Self {
            root: scrolled,
            inner,
        }
    }

    /// 項目を選んだら `f(項目)` を呼ぶ。
    pub fn connect_select(&self, f: impl Fn(&Data) + 'static) {
        self.inner.on_select.replace(Some(Box::new(f)));
    }

    /// 右クリックしたら `f(項目, 部品, x, y)` を呼ぶ。
    pub fn connect_menu(&self, f: impl Fn(&Data, &gtk::Widget, f64, f64) + 'static) {
        self.inner.on_menu.replace(Some(Box::new(f)));
    }

    /// ブランチ・タグを落としたら `f(運んだもの, 落とした先, 部品, x, y)` を呼ぶ。
    pub fn connect_drop(&self, f: impl Fn(&str, &Data, &gtk::Widget, f64, f64) + 'static) {
        self.inner.on_drop.replace(Some(Box::new(f)));
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
        self.inner.rebuild(Some(refs), first);
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
        };
        self.inner.rebuilding.set(true);
        self.inner.select_key(key);
        self.inner.rebuilding.set(false);
    }
}

impl Inner {
    fn rebuild(&self, refs: Option<&RefsSnapshot>, first: bool) {
        self.rebuilding.set(true);
        // 開いていた項目と、選んでいた項目を覚えておく。
        let mut expanded = HashSet::new();
        for i in 0..self.model.n_items() {
            if let Some(row) = self.model.row(i)
                && row.is_expanded()
                && let Some(node) = row.item().and_downcast::<Node>()
            {
                expanded.insert(node.data().key);
            }
        }
        let selected = self
            .selection
            .selected_item()
            .and_downcast::<gtk::TreeListRow>()
            .and_then(|r| r.item().and_downcast::<Node>())
            .map(|n| n.data().key);

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
        let workspace = gio::ListStore::new::<Node>();
        workspace.append(&self.working);
        workspace.append(&history);
        // stash があるときだけ、数を添えて出す（macOS 版と同じ）。
        let stashes = refs.map_or(0, |r| {
            r.refs
                .entries
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
        let mut groups = vec![("Workspace", workspace)];
        if let Some(refs) = refs {
            groups.extend(groups_of(refs));
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
        // （開くと子の行が後ろに増える）。
        let mut i = 0;
        while i < self.model.n_items() {
            if let Some(row) = self.model.row(i)
                && let Some(node) = row.item().and_downcast::<Node>()
            {
                let data = node.data();
                let group = matches!(data.item, Item::Group)
                    && node.children().is_some_and(|c| c.n_items() <= MANY);
                if (first && group) || expanded.contains(&data.key) {
                    row.set_expanded(true);
                }
            }
            i += 1;
        }
        self.select_key(selected.as_deref().unwrap_or("page/history"));
        self.rebuilding.set(false);
    }

    fn select_key(&self, key: &str) {
        for i in 0..self.model.n_items() {
            if let Some(row) = self.model.row(i)
                && let Some(node) = row.item().and_downcast::<Node>()
                && node.data().key == key
            {
                self.selection.set_selected(i);
                return;
            }
        }
    }
}

/// ref の種類ごとの見出しと、その中の項目。空の見出しは出さない。
fn groups_of(refs: &RefsSnapshot) -> Vec<(&'static str, gio::ListStore)> {
    let refs = &refs.refs;
    let of = |kind: RefKind| refs.entries.iter().filter(move |e| e.kind == kind);
    let data = |e: &RefEntry, title: &str, icon: icon::Kind| Data {
        title: title.to_owned(),
        key: format!("{:?}/{}", e.kind, e.name),
        item: Item::Ref(e.clone()),
        icon,
        head: e.kind == RefKind::Local && Some(e.name.as_str()) == refs.head_branch.as_deref(),
        badge: badge(e),
        tooltip: e.detail.clone(),
    };
    let mut groups = Vec::new();

    let mut branches = Tree::default();
    for e in of(RefKind::Local) {
        let parts: Vec<&str> = e.name.split('/').collect();
        branches.insert(&parts, data(e, parts[parts.len() - 1], icon::Kind::Branch));
    }
    let branches = branches.into_store("Branches");
    if branches.n_items() > 0 {
        groups.push(("Branches", branches));
    }

    // リモートごとにまとめ、その中もフォルダにまとめる。
    let mut remotes = Tree::default();
    for e in of(RefKind::Remote) {
        let parts: Vec<&str> = e.name.split('/').collect();
        remotes.insert(&parts, data(e, parts[parts.len() - 1], icon::Kind::Branch));
    }
    let remotes = remotes.into_store("Remotes");
    for i in 0..remotes.n_items() {
        if let Some(node) = remotes.item(i).and_downcast::<Node>() {
            let mut d = node.imp().data.borrow_mut();
            if matches!(d.item, Item::Folder) {
                d.icon = icon::Kind::Remote;
            }
        }
    }
    if remotes.n_items() > 0 {
        groups.push(("Remotes", remotes));
    }

    let mut simple = |title: &'static str, kind: RefKind, icon: icon::Kind| {
        let store = gio::ListStore::new::<Node>();
        for e in of(kind) {
            // stash はメッセージを名前に、名前は説明に。
            let mut d = data(e, &e.name, icon);
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
    groups
}

/// ローカルブランチの、上流との差（↑ 先にある、↓ 取り込んでいない）。上流が消えていれば gone。
fn badge(e: &RefEntry) -> String {
    if e.kind != RefKind::Local {
        return String::new();
    }
    if e.gone {
        return "gone".into();
    }
    let mut parts = Vec::new();
    if e.ahead > 0 {
        parts.push(format!("↑{}", e.ahead));
    }
    if e.behind > 0 {
        parts.push(format!("↓{}", e.behind));
    }
    parts.join(" ")
}

/// ref が指すコミット（History で選ぶ）。
pub fn target(data: &Data) -> Option<Oid> {
    match &data.item {
        Item::Ref(e) => e.target,
        _ => None,
    }
}
