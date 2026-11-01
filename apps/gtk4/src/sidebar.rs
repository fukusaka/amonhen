//! サイドバー: Workspace（Working Copy・History・あるときだけ Conflicts と Stashes・開いている間だけ Range Diff、最後に Reflog と Settings）/ Branches / あるときだけ Pull Requests / Remotes / Tags / Submodules。
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
use serde_json::{Value, json};

use crate::actions::Actions;
use crate::github::{Ci, PullRequest};
use crate::icon::{self, Icon};
use crate::menu;
use crate::reffacts::{self, BranchGroups, TagRemotes};

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
    /// リポジトリの設定（いつも Workspace の最後）。
    Settings,
}

#[derive(Clone, Debug, Default)]
pub enum Item {
    /// 見出し（Branches など）。選べない。
    #[default]
    Group,
    Page(Page),
    Folder,
    Ref(RefEntry),
    /// Working Copy の下の worktree（選ぶと書き込む先にする）。
    Worktree(crate::worktrees::Info),
    /// Pull Requests の下のプルリクエストと、そのブランチ（手元にあれば）が指すコミット。
    PullRequest(Box<PullRequest>, Option<Oid>),
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
    /// アイコンと右端の文字を緑にする（追加した worktree が checkout しているブランチ）。
    pub green: bool,
    /// worktree が 2 つ以上あり、書き込む先の worktree が checkout している今のブランチ（✎ の印）。
    pub editing: bool,
    /// 右端の文字の色の CSS のクラス（プルリクエストの CI の `success`・`error`）。
    pub badge_css: Option<&'static str>,
    /// タグの署名の印（右端の印の先頭。見えている行を出すときに、確かめた結果から入れる）。
    pub signature: Option<Mark>,
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
type SelectManyCallback = Box<dyn Fn(&[RefEntry])>;
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
    /// タグがどのリモートにあるか（fetch のときにコアが読んで覚えたもの。まだ読んでいなければ None）。
    pub tags: Option<TagRemotes>,
    /// リモートブランチのあるリモートの名前（タグの様子をまだ読んでいないときの、タグの送り先）。
    pub branch_remotes: Vec<String>,
    /// 開いているプルリクエスト（GitHub から読んだもの）。
    pub pull_requests: Vec<PullRequest>,
    /// プルリクエストを作れる（GitHub のリポジトリで gh が使える）。
    pub can_create_pull_request: bool,
}

impl Known {
    /// `branch` から開いているプルリクエスト。
    pub fn pull_request(&self, branch: &str) -> Option<&PullRequest> {
        self.pull_requests
            .iter()
            .find(|p| p.head_ref_name == branch)
    }

    /// `branch` からプルリクエストを作れるか（既定のブランチからは作らない）。
    pub fn can_create_pull_request_from(&self, branch: &str) -> bool {
        let base = self.groups.as_ref().map_or("main", |g| g.base_name());
        self.can_create_pull_request && branch != base && !base.ends_with(&format!("/{branch}"))
    }
}

impl Known {
    /// タグの push・削除の送り先（すべてのリモート）。
    pub fn remote_names(&self) -> Vec<String> {
        match &self.tags {
            Some(t) => t.remotes.iter().map(|(n, _)| n.clone()).collect(),
            None => self.branch_remotes.clone(),
        }
    }
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
    /// Conflicts の行（コンフリクトのファイルがあるときだけ、History の下に数を添えて出す）。
    conflicts: Node,
    generation: RefCell<Option<u64>>,
    /// 最後に出した ref（分け方が届いたら作り直す）。
    refs: RefCell<Option<Refs>>,
    groups: RefCell<Option<BranchGroups>>,
    tags: RefCell<Option<TagRemotes>>,
    /// お気に入りのブランチ（上から順。作り直すたびに求める）。
    pinned: RefCell<Vec<String>>,
    /// 畳んだ、積んだブランチのあるブランチ（初めはすべて開いておく）。
    folded: RefCell<HashSet<String>>,
    actions: RefCell<Option<Rc<Actions>>>,
    /// 分け方を読む番号（古い答えは使わない）。
    loading: Cell<u64>,
    on_select: RefCell<Option<SelectCallback>>,
    /// ブランチ・タグを 2 つ以上選んだら呼ぶ（macOS 版の `onSelectMany`）。
    on_select_many: RefCell<Option<SelectManyCallback>>,
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
    /// リポジトリの worktree（2 つ以上あるときだけ Working Copy の下に並べる）。
    worktrees: RefCell<crate::worktrees::View>,
    /// Working Copy の下に worktree を並べている（並べ始めたら Working Copy を開く）。
    working_children: Cell<bool>,
    /// 開いているプルリクエスト（あれば Pull Requests の見出しを出す）。
    pull_requests: RefCell<Vec<PullRequest>>,
    can_create_pull_request: Cell<bool>,
    /// Pull Requests の見出しをまだ出していない（初めて出すときは開いておく）。
    pull_requests_first: Cell<bool>,
    /// タグの署名の検証の結果（名前 → 状態と、吹き出しに出す中身と、確かめたときの refs の番号）。refs が
    /// 変わったら（タグを付け直したかもしれない）、見えている行から確かめ直す（コアが ID で覚えているので速い）。
    tag_signatures: RefCell<HashMap<String, TagSignature>>,
    tag_epoch: Cell<u64>,
    /// 確かめる予定のタグと、確かめている途中のタグ。
    tags_to_verify: RefCell<HashSet<String>>,
    verifying_tags: RefCell<HashSet<String>>,
    verify_timer: RefCell<Option<glib::SourceId>>,
    /// 確認用（`--sidebar-mark signature:<タグ>`）: 確かめ終えたら署名の吹き出しを出すタグ。
    debug_signature: RefCell<Option<String>>,
    /// タグの署名を検証した場所（吹き出しに出す）。
    signature_place: RefCell<crate::signature::Place>,
}

/// タグの署名を確かめた結果（状態、コアの `tagSignatures` の 1 件、確かめたときの refs の番号）。
type TagSignature = (Option<crate::tagsign::Status>, Value, u64);

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
        let o = owner.clone();
        factory.connect_bind(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            // タグの行は、署名の印を入れてから（見えている行だけが来るので、見えているタグだけを確かめる）。
            if let (Some(inner), Some(node)) = (
                o.borrow().upgrade(),
                item.item()
                    .and_downcast::<gtk::TreeListRow>()
                    .and_then(|r| r.item())
                    .and_downcast::<Node>(),
            ) {
                inner.decorate_tag(&node);
            }
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
        // 選ぶと Working Copy で最初のコンフリクトのファイルを開く（macOS 版と同じく Working Copy の項目の 1 つ）。
        let conflicts = Node::new(
            Data {
                title: tr!("Conflicts"),
                key: CONFLICTS_KEY.into(),
                item: Item::Page(Page::WorkingCopy),
                icon: icon::Kind::Warning,
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
            conflicts,
            generation: RefCell::default(),
            refs: RefCell::default(),
            groups: RefCell::default(),
            tags: RefCell::default(),
            pinned: RefCell::default(),
            folded: RefCell::default(),
            actions: RefCell::default(),
            loading: Cell::new(0),
            on_select: RefCell::default(),
            on_select_many: RefCell::default(),
            on_menu,
            on_drop,
            rebuilding: Cell::new(false),
            debug: RefCell::default(),
            anchor: RefCell::default(),
            filter: RefCell::default(),
            unfiltered: RefCell::default(),
            worktrees: RefCell::default(),
            working_children: Cell::new(false),
            pull_requests: RefCell::default(),
            can_create_pull_request: Cell::new(false),
            pull_requests_first: Cell::new(true),
            tag_signatures: RefCell::default(),
            tag_epoch: Cell::new(0),
            tags_to_verify: RefCell::default(),
            verifying_tags: RefCell::default(),
            verify_timer: RefCell::default(),
            debug_signature: RefCell::default(),
            signature_place: RefCell::default(),
        });
        inner.anchor.borrow().set(Some(scrolled.upcast_ref()));
        owner.replace(Rc::downgrade(&inner));
        let weak = Rc::downgrade(&inner);
        inner.selection.connect_selection_changed(move |_, _, _| {
            let Some(inner) = weak.upgrade() else { return };
            if inner.rebuilding.get() {
                return;
            }
            let nodes = inner.selected_nodes();
            // ブランチ・タグを 2 つ以上選んだら、それぞれから辿れる履歴をまとめて出す（macOS 版と同じ）。
            if nodes.len() > 1 {
                let refs: Vec<RefEntry> = nodes
                    .iter()
                    .filter_map(|n| match n.data().item {
                        Item::Ref(e)
                            if matches!(
                                e.kind,
                                RefKind::Local | RefKind::Remote | RefKind::Tag
                            ) && e.target.is_some() =>
                        {
                            Some(e)
                        }
                        _ => None,
                    })
                    .collect();
                if refs.len() > 1
                    && let Some(f) = inner.on_select_many.borrow().as_ref()
                {
                    f(&refs);
                }
                return;
            }
            // 1 つだけ選んだときに知らせる（複数選んだら、右クリックでまとめて扱う）。
            if let [node] = &nodes[..]
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

    /// 開いたリポジトリの場所（タグの署名を検証した場所を、署名の吹き出しに出すため）。
    pub fn set_location(&self, location: &str) {
        self.inner
            .signature_place
            .replace(crate::signature::Place::of(location));
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

    /// ブランチ・タグを 2 つ以上選んだら `f(選んだもの)` を呼ぶ。
    pub fn connect_select_many(&self, f: impl Fn(&[RefEntry]) + 'static) {
        self.inner.on_select_many.replace(Some(Box::new(f)));
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
        self.inner.tag_epoch.set(self.inner.tag_epoch.get() + 1);
        self.inner.rebuild(first);
        self.inner.load_facts();
    }

    /// 開いているプルリクエストを出す（なければ Pull Requests の見出しを出さない）。前と同じなら何もしない。
    pub fn show_pull_requests(&self, list: Vec<PullRequest>) {
        if *self.inner.pull_requests.borrow() == list {
            return;
        }
        self.inner.pull_requests.replace(list);
        self.inner.rebuild(false);
        // 確認用のメニュー・吹き出しが、プルリクエストの行を待っていれば。
        if self.inner.groups.borrow().is_some() {
            self.inner.run_debug();
        }
    }

    /// プルリクエストを作れるか（ブランチのメニューと吹き出しに出す）。
    pub fn set_can_create_pull_request(&self, yes: bool) {
        self.inner.can_create_pull_request.set(yes);
    }

    /// Working Copy の変更の数を出す。
    pub fn set_working_count(&self, count: usize) {
        let node = &self.inner.working;
        node.imp().data.borrow_mut().badge = if count > 0 {
            count.to_string()
        } else {
            String::new()
        };
        // 行を描き直させる（同じ項目を入れ替える。下の worktree の開き方と選んだ行は戻す）。
        let workspace = self.inner.root.item(0).and_downcast::<Node>();
        if let Some(store) = workspace.and_then(|w| w.children()) {
            self.inner.replace_keeping(&store, 0, node);
        }
    }

    /// リポジトリの worktree と変更の数を出す。並びが変われば作り直し、数だけなら行を描き直す。
    pub fn set_worktrees(&self, view: crate::worktrees::View) {
        let before = self.inner.worktrees.replace(view.clone());
        if before == view {
            return;
        }
        if before.list.is_empty() && self.inner.debug.borrow().is_some() {
            self.inner.rebuild(false);
            // 確認用の手順は、ほかの一覧の受け手（ほかの worktree が checkout しているブランチをアクションに
            // 渡すもの）が済んでから。同じ知らせの中で動くと、メニューや印が一覧を知らないまま作られる。
            let inner = Rc::downgrade(&self.inner);
            glib::idle_add_local_once(move || {
                if let Some(inner) = inner.upgrade() {
                    inner.run_debug();
                }
            });
            return;
        }
        let keys =
            |l: &[crate::worktrees::Info]| l.iter().map(|w| w.key.clone()).collect::<Vec<_>>();
        let (a, b) = (before.layout(), view.layout());
        if before.list != view.list
            || before.editing != view.editing
            || keys(&a.0) != keys(&b.0)
            || keys(&a.1) != keys(&b.1)
        {
            self.inner.rebuild(false);
            return;
        }
        // 数だけ変わった: worktree の行を描き直す。
        let Some(store) = self.inner.working.children() else {
            return;
        };
        let clean = store
            .item(store.n_items().saturating_sub(1))
            .and_downcast::<Node>()
            .and_then(|n| n.children());
        for store in [Some(store), clean].into_iter().flatten() {
            for i in 0..store.n_items() {
                let Some(node) = store.item(i).and_downcast::<Node>() else {
                    continue;
                };
                let Item::Worktree(w) = node.data().item else {
                    continue;
                };
                let data = worktree_data(&w, &view);
                if data.badge != node.data().badge {
                    node.imp().data.replace(data);
                    self.inner.replace_keeping(&store, i, &node);
                }
            }
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
            Page::Settings => "page/settings",
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

    /// 名前が `names` のブランチ・タグの行（見えているもの）を選ぶ（選んだことを知らせる。確認用）。
    pub fn select_refs(&self, names: &[String]) {
        let inner = &self.inner;
        let mut first = true;
        for i in 0..inner.model.n_items() {
            if let Some(row) = inner.model.row(i)
                && let Some(node) = row.item().and_downcast::<Node>()
                && let Item::Ref(e) = &node.data().item
                && names.contains(&e.name)
            {
                inner.selection.select_item(i, first);
                first = false;
            }
        }
    }

    /// Conflicts の行を選ぶ（選んだことを知らせる。確認用）。
    pub fn select_conflicts(&self) {
        self.inner.select_key(CONFLICTS_KEY);
    }

    /// コンフリクトのファイルの数（0 なら Conflicts の行を出さない。知らせは出さない）。
    pub fn set_conflict_count(&self, count: usize) {
        let inner = &self.inner;
        let badge = if count > 0 {
            count.to_string()
        } else {
            String::new()
        };
        if inner.conflicts.data().badge == badge {
            return;
        }
        inner.conflicts.imp().data.borrow_mut().badge = badge;
        let workspace = inner.root.item(0).and_downcast::<Node>();
        let Some(store) = workspace.and_then(|w| w.children()) else {
            return;
        };
        match (store.find(&inner.conflicts), count > 0) {
            // 数だけ変わった: 行を描き直す。
            (Some(at), true) => inner.replace_keeping(&store, at, &inner.conflicts),
            (Some(at), false) => {
                inner.rebuilding.set(true);
                store.remove(at);
                inner.rebuilding.set(false);
            }
            (None, true) => {
                inner.rebuilding.set(true);
                store.insert(2.min(store.n_items()), &inner.conflicts);
                inner.rebuilding.set(false);
            }
            (None, false) => {}
        }
    }
}

/// Conflicts の行の鍵（選んだら最初のコンフリクトのファイルを開く）。
pub const CONFLICTS_KEY: &str = "page/conflicts";

/// 行（鍵が `key`）を選んだときに出す画面の行の鍵（`main.rs` の `connect_select` と同じ対応）。
fn page_of_key(key: &str) -> &'static str {
    if key.starts_with("worktree/") || key == CONFLICTS_KEY || key == "page/working" {
        "page/working"
    } else if key == "page/stashes" {
        "page/stashes"
    } else if key == "page/reflog" {
        "page/reflog"
    } else if key == "page/settings" {
        "page/settings"
    } else {
        "page/history"
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
    press.connect_pressed(move |gesture, _, x, y| {
        let (Some(expander), Some(inner)) = (weak.upgrade(), owner.borrow().upgrade()) else {
            return;
        };
        let Some(node) = node_of(&expander) else {
            return;
        };
        let data = node.data();
        if !data.mark_click && data.signature.is_none() {
            return;
        }
        gesture.set_state(gtk::EventSequenceState::Claimed);
        if let Some(trail) = expander
            .child()
            .and_then(|l| l.last_child())
            .and_downcast::<gtk::Box>()
        {
            // 署名の印（左）を押すと署名の吹き出し、ほかの印はその吹き出し。
            let first_mark = trail.first_child().and_then(|m| m.first_child());
            let picked = trail.pick(x, y, gtk::PickFlags::DEFAULT);
            let on_signature = data.signature.is_some()
                && picked
                    .zip(first_mark)
                    .is_some_and(|(p, m)| p == m || p.is_ancestor(&m));
            if on_signature || !data.mark_click {
                inner.show_tag_signature(&data, trail.upcast_ref());
            } else {
                inner.show_marks(&data, trail.upcast_ref());
            }
        }
    });
    trail.add_controller(press);

    // 見出しと、選べない行（ブランチ・タグのフォルダ、リモート、No changes）は、行のどこを押しても開く・閉じる
    // （矢印のほかでも。macOS 版と同じ）。矢印は行の部品の外なので、二度は動かない。
    let toggle = gtk::GestureClick::new();
    toggle.set_button(gtk::gdk::BUTTON_PRIMARY);
    let weak = expander.downgrade();
    toggle.connect_pressed(move |_, n_press, _, _| {
        if n_press != 1 {
            return;
        }
        let Some(row) = weak.upgrade().and_then(|e| e.list_row()) else {
            return;
        };
        let foldable = row
            .item()
            .and_downcast::<Node>()
            .is_some_and(|n| matches!(n.data().item, Item::Group | Item::Folder));
        if foldable && row.is_expandable() {
            row.set_expanded(!row.is_expanded());
        }
    });
    line.add_controller(toggle);

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
    // 落とせない行（Workspace・タグ・名前のフォルダなど）の上では、落とせないことを先に見せる（macOS 版と同じ）。
    // History から運んだコミットは受けない（History の中で並べ替えるもの）。
    target.set_preload(true);
    let weak = expander.downgrade();
    target.connect_motion(move |target, _, _| {
        let accepts = weak
            .upgrade()
            .and_then(|e| node_of(&e))
            .is_some_and(|node| {
                target
                    .value()
                    .and_then(|v| v.get::<String>().ok())
                    .is_some_and(|d| crate::refops::accepts_drop(&d, &node.data()))
            });
        if accepts {
            gtk::gdk::DragAction::COPY
        } else {
            gtk::gdk::DragAction::empty()
        }
    });
    let (weak, cb, owner) = (expander.downgrade(), drop_cb.clone(), inner.clone());
    target.connect_drop(move |_, value, x, y| {
        let (Some(expander), Ok(dragged)) = (weak.upgrade(), value.get::<String>()) else {
            return false;
        };
        if node_of(&expander).is_none_or(|n| !crate::refops::accepts_drop(&dragged, &n.data())) {
            return false;
        }
        // お気に入りのブランチを、お気に入りの行の上端・下端に落とす: 並べ替える（真ん中は merge・rebase）。
        if let (Some(node), Some(inner)) = (node_of(&expander), owner.borrow().upgrade())
            && inner.reorder_favorite(&dragged, &node.data(), y, expander.height() as f64)
        {
            return true;
        }
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
    // 見出しと名前のフォルダ（No changes も）は選べない。選ぶと、出している画面の行の選択が外れる。
    item.set_selectable(!matches!(data.item, Item::Group | Item::Folder));
    icon.set(data.icon, data.head);
    for w in [icon.upcast_ref::<gtk::Widget>(), badge.upcast_ref()] {
        if data.green {
            w.add_css_class("success");
        } else {
            w.remove_css_class("success");
        }
    }
    // 緑の印は薄くしない。
    if data.green {
        badge.remove_css_class("dim-label");
    } else {
        badge.add_css_class("dim-label");
    }
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
    // 見出しの数は、名前のすぐ後ろに薄く小さく（右端には出さない）。
    if group && !data.badge.is_empty() {
        let start = data.title.len() as u32;
        let mut alpha = gtk::pango::AttrInt::new_foreground_alpha(u16::MAX / 2);
        alpha.set_start_index(start);
        attrs.insert(alpha);
        let mut scale = gtk::pango::AttrFloat::new_scale(gtk::pango::SCALE_SMALL);
        scale.set_start_index(start);
        attrs.insert(scale);
        let mut weight = gtk::pango::AttrInt::new_weight(gtk::pango::Weight::Normal);
        weight.set_start_index(start);
        attrs.insert(weight);
        title.set_text(&format!("{}  {}", data.title, data.badge));
    } else {
        title.set_text(&data.title);
    }
    title.set_attributes(Some(&attrs));
    let mut child = marks.first_child();
    let mut i = 0;
    while let Some(w) = child {
        child = w.next_sibling();
        let Ok(mark_icon) = w.downcast::<Icon>() else {
            continue;
        };
        for css in ["warning", "success", "error", "caution"] {
            mark_icon.remove_css_class(css);
        }
        match data.signature.iter().chain(&data.marks).nth(i) {
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
    let has_marks = !data.marks.is_empty() || data.signature.is_some();
    marks.set_visible(has_marks);
    // 見出しの数は名前の後ろに出した。
    let badge_text = if group { "" } else { data.badge.as_str() };
    badge.set_text(badge_text);
    badge.set_visible(!badge_text.is_empty());
    for css in ["success", "error"] {
        badge.remove_css_class(css);
    }
    match data.badge_css {
        Some(css) => {
            badge.remove_css_class("dim-label");
            badge.add_css_class(css);
        }
        None => badge.add_css_class("dim-label"),
    }
    trail.set_visible(has_marks || !badge_text.is_empty());
    // 署名の印の意味は、ツールチップの先頭に。
    let tooltip = match (&data.signature, &data.tooltip) {
        (Some(s), Some(t)) => Some(format!("{}\n{t}", s.tooltip)),
        (Some(s), None) => Some(s.tooltip.clone()),
        (None, t) => t.clone(),
    };
    line.set_tooltip_text(tooltip.as_deref());
}

impl Inner {
    fn known(&self) -> Known {
        Known {
            groups: self.groups.borrow().clone(),
            pinned: self.pinned.borrow().clone(),
            selected: Vec::new(),
            tags: self.tags.borrow().clone(),
            branch_remotes: self
                .refs
                .borrow()
                .as_ref()
                .map_or_else(Vec::new, branch_remotes),
            pull_requests: self.pull_requests.borrow().clone(),
            can_create_pull_request: self.can_create_pull_request.get(),
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
        let items = self.menu_items(data, &known);
        if !items.is_empty() {
            menu::popup(widget, x, y, items);
        }
    }

    /// `data` の行の右クリックのメニューの項目。
    fn menu_items(&self, data: &Data, known: &Known) -> Vec<menu::Item> {
        // プルリクエストの行は、ここで作る（Checkout・ブラウザで開く・URL のコピー）。
        if let Item::PullRequest(pr, _) = &data.item {
            return match self.actions.borrow().as_ref() {
                Some(actions) => crate::github::pull_request_menu(actions, pr),
                None => Vec::new(),
            };
        }
        match self.on_menu.borrow().as_ref() {
            Some(f) => f(data, known),
            None => Vec::new(),
        }
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

    /// お気に入りのブランチ `dragged`（`local:<名前>`）を、お気に入りの行 `target` の上端（`y` が高さの 1/3 より上）なら
    /// その上へ、下端なら下へ動かす（macOS 版の、お気に入りの間に落とす並べ替え）。並べ替えたら true。
    fn reorder_favorite(&self, dragged: &str, target: &Data, y: f64, height: f64) -> bool {
        let Some(name) = dragged.strip_prefix("local:") else {
            return false;
        };
        let Item::Ref(e) = &target.item else {
            return false;
        };
        if !target.pinned
            || e.kind != RefKind::Local
            || (y > height / 3.0 && y < height * 2.0 / 3.0)
        {
            return false;
        }
        let Some(order) = reordered(&self.pinned.borrow(), name, &e.name, y <= height / 3.0) else {
            return false;
        };
        if order != *self.pinned.borrow()
            && let Some(actions) = self.actions.borrow().as_ref()
        {
            actions.perform_sequence(crate::refops::pinned_order(&order));
        }
        true
    }

    /// タグの行の署名の印を入れる。結果がない（古い）ときは、確かめる予定に入れる。
    fn decorate_tag(self: &Rc<Self>, node: &Node) {
        let name = match &node.imp().data.borrow().item {
            Item::Ref(e) if e.kind == RefKind::Tag => e.name.clone(),
            _ => return,
        };
        let known = self.tag_signatures.borrow().get(&name).cloned();
        if known.as_ref().map(|k| k.2) != Some(self.tag_epoch.get()) {
            self.verify_tags_soon(name);
        }
        node.imp().data.borrow_mut().signature =
            known.and_then(|(status, _, _)| status).map(|s| s.mark());
    }

    /// 少し待ってから、予定に入れたタグの署名を 8 件ずつ裏で確かめ、終わった分から印を出す（タグごとに検証の
    /// プログラムを起動するので 1 件 25 ms ほど。History の署名の印と同じ）。
    fn verify_tags_soon(self: &Rc<Self>, name: String) {
        if self.verifying_tags.borrow().contains(&name) {
            return;
        }
        self.tags_to_verify.borrow_mut().insert(name);
        if let Some(timer) = self.verify_timer.take() {
            timer.remove();
        }
        let weak = Rc::downgrade(self);
        let timer =
            glib::timeout_add_local_once(std::time::Duration::from_millis(150), move || {
                let Some(inner) = weak.upgrade() else { return };
                inner.verify_timer.take();
                let Some(repo) = inner.actions.borrow().as_ref().and_then(|a| a.repo()) else {
                    return;
                };
                let mut pending: Vec<String> = inner.tags_to_verify.take().into_iter().collect();
                pending.sort();
                inner
                    .verifying_tags
                    .borrow_mut()
                    .extend(pending.iter().cloned());
                let (epoch, weak) = (inner.tag_epoch.get(), Rc::downgrade(&inner));
                glib::spawn_future_local(async move {
                    for batch in pending.chunks(8) {
                        let reply = crate::sheets::run(
                            &repo,
                            json!({ "op": "tagSignatures", "names": batch }),
                        )
                        .await;
                        let Some(inner) = weak.upgrade() else { return };
                        {
                            let mut known = inner.tag_signatures.borrow_mut();
                            for tag in reply["info"]["tags"].as_array().into_iter().flatten() {
                                if let Some(name) = tag["name"].as_str() {
                                    known.insert(
                                        name.to_owned(),
                                        (crate::tagsign::Status::of(tag), tag.clone(), epoch),
                                    );
                                }
                            }
                            let mut verifying = inner.verifying_tags.borrow_mut();
                            for name in batch {
                                verifying.remove(name);
                                // 答えがなかった（確かめられなかった）ものも、この番号では確かめ直さない。
                                known
                                    .entry(name.clone())
                                    .or_insert((None, Value::Null, epoch));
                            }
                        }
                        inner.reload_tag_rows(batch);
                        let shown = inner.debug_signature.borrow().clone();
                        if let Some(name) = shown.filter(|n| batch.contains(n)) {
                            inner.debug_signature.take();
                            inner.show_debug_signature(&name);
                        }
                    }
                });
            });
        self.verify_timer.replace(Some(timer));
    }

    /// `names` のタグの行を描き直す（印を入れ直す）。
    fn reload_tag_rows(&self, names: &[String]) {
        let mut stores = vec![self.root.clone()];
        while let Some(store) = stores.pop() {
            for i in 0..store.n_items() {
                let Some(node) = store.item(i).and_downcast::<Node>() else {
                    continue;
                };
                if let Some(children) = node.children() {
                    stores.push(children);
                }
                let hit = matches!(&node.imp().data.borrow().item,
                    Item::Ref(e) if e.kind == RefKind::Tag && names.contains(&e.name));
                if hit {
                    self.replace_keeping(&store, i, &node);
                }
            }
        }
    }

    /// 確認用: `name` のタグの行の署名の吹き出しを出す。
    fn show_debug_signature(&self, name: &str) {
        let Some(data) = self.find(name) else {
            eprintln!("サイドバーの行がない: {name}");
            return;
        };
        let anchor = self.row_trail(&data.key).or_else(|| self.root_widget());
        if let Some(anchor) = anchor {
            self.show_tag_signature(&data, &anchor);
        }
    }

    /// タグの行の署名の印を押した: 署名の吹き出し。
    fn show_tag_signature(&self, data: &Data, anchor: &gtk::Widget) {
        let Item::Ref(entry) = &data.item else { return };
        let known = self.tag_signatures.borrow().get(&entry.name).cloned();
        if let Some((Some(status), tag, _)) = known {
            let place = self.signature_place.borrow().clone();
            crate::tagsign::popover(anchor, &entry.name, status, &tag, &place);
        }
    }

    /// 行の右端の印を押した: 詳しいことの吹き出し。
    fn show_marks(&self, data: &Data, anchor: &gtk::Widget) {
        let Some(actions) = self.actions.borrow().clone() else {
            return;
        };
        match &data.item {
            Item::Ref(entry) if entry.kind == RefKind::Local => {
                crate::refmarks::branch(
                    &actions,
                    entry,
                    &self.known(),
                    data.head,
                    data.editing,
                    anchor,
                );
            }
            Item::Ref(entry) if entry.kind == RefKind::Tag => {
                crate::refmarks::tag(&actions, entry, &self.known(), anchor);
            }
            // CI の印（History の行の印と同じ吹き出し）。
            Item::PullRequest(pr, _) => {
                if let (Some(sha), Some(github)) =
                    (&pr.head_ref_oid, crate::github::github_of(&actions))
                {
                    crate::cipopover::show(anchor, None, pr.ci(), sha, github);
                }
            }
            _ => {}
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
            let mut changed = false;
            if groups.is_some() && groups != *inner.groups.borrow() {
                inner.groups.replace(groups);
                changed = true;
            }
            // タグがどのリモートにあるか（変わるのはタグの行の印とツールチップだけ）。
            let reply = crate::sheets::run(&repo, json!({ "op": "tagRemotes" })).await;
            if inner.loading.get() != token {
                return;
            }
            let tags = TagRemotes::from(&reply["info"]);
            if tags.is_some() && tags != *inner.tags.borrow() {
                inner.tags.replace(tags);
                changed = true;
            }
            if changed {
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
                    || matches!(&data.item, Item::Ref(e) if e.name == spec)
                    || matches!(&data.item, Item::Worktree(w) if w.name == spec);
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
        // worktree の行は、一覧を読んでから（まだなら [`Sidebar::set_worktrees`] がもう一度呼ぶ）。
        let spec = debug
            .menu
            .as_deref()
            .or(debug.mark.as_deref())
            .unwrap_or_default();
        let worktree_waiting = self.worktrees.borrow().list.is_empty()
            && spec.split(',').any(|s| self.find(s).is_none());
        // プルリクエストの行（`pr/<番号>`）は、GitHub から読むまで待つ。
        let waiting = worktree_waiting
            || [&debug.menu, &debug.mark]
                .into_iter()
                .flatten()
                .any(|s| s.starts_with("pr/") && self.find(s).is_none());
        if waiting {
            self.debug.replace(Some(debug));
            return;
        }
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
                    let items = self.menu_items(&data, &known);
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
        if let Some(name) = debug
            .mark
            .as_deref()
            .and_then(|s| s.strip_prefix("signature:"))
        {
            // タグの署名の吹き出し（確かめ終えてから）。
            if self
                .tag_signatures
                .borrow()
                .get(name)
                .is_some_and(|k| k.2 == self.tag_epoch.get())
            {
                self.show_debug_signature(name);
            } else {
                self.debug_signature.replace(Some(name.to_owned()));
            }
            wait = 3000;
        } else if let Some(spec) = &debug.mark {
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
        let settings = Node::new(
            Data {
                title: "Settings".into(),
                key: "page/settings".into(),
                item: Item::Page(Page::Settings),
                icon: icon::Kind::Gear,
                ..Data::default()
            },
            None,
        );
        // worktree が 2 つ以上なら、Working Copy の下に並べる（並べ始めたら開いておく）。
        let worktrees = self.worktrees.borrow().clone();
        let (shown, clean) = worktrees.layout();
        let mut expanded = expanded;
        if shown.is_empty() {
            self.working.imp().children.replace(None);
            self.working_children.set(false);
        } else {
            let store = gio::ListStore::new::<Node>();
            for w in &shown {
                store.append(&Node::new(worktree_data(w, &worktrees), None));
            }
            if !clean.is_empty() {
                let folded = gio::ListStore::new::<Node>();
                for w in &clean {
                    folded.append(&Node::new(worktree_data(w, &worktrees), None));
                }
                store.append(&Node::new(
                    Data {
                        title: tr!("No changes"),
                        key: "worktrees/clean".into(),
                        item: Item::Folder,
                        icon: icon::Kind::CheckCircle,
                        badge: clean.len().to_string(),
                        tooltip: Some(tr!("Worktrees with no changes (click to show)")),
                        ..Data::default()
                    },
                    Some(folded),
                ));
            }
            self.working.imp().children.replace(Some(store));
            if !self.working_children.replace(true) {
                expanded.insert("page/working".to_owned());
            }
        }
        let workspace = gio::ListStore::new::<Node>();
        workspace.append(&self.working);
        workspace.append(&history);
        if !self.conflicts.data().badge.is_empty() {
            workspace.append(&self.conflicts);
        }
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
        if self.range_diff_shown.get() {
            workspace.append(&self.range_diff);
        }
        workspace.append(&reflog);
        workspace.append(&settings);
        let mut groups = vec![("Workspace", workspace)];
        if let Some(refs) = &refs {
            let tags = self.tags.borrow();
            if filtering {
                groups.extend(filtered_groups(refs, &self.filter.borrow(), tags.as_ref()));
            } else {
                let branch_groups = self.groups.borrow();
                // ほかの worktree が checkout しているブランチ（Inactive に畳まない）。
                let elsewhere: HashSet<&str> = worktrees
                    .list
                    .iter()
                    .filter(|w| w.key != worktrees.editing)
                    .filter_map(|w| w.branch.as_deref())
                    .collect();
                let (built, pinned) =
                    groups_of(refs, branch_groups.as_ref(), tags.as_ref(), &elsewhere);
                self.pinned.replace(pinned);
                groups.extend(built);
            }
        }
        // ほかの worktree が checkout しているブランチと、書き込む先のブランチに印を付ける。
        if worktrees.list.len() > 1 {
            for (_, store) in &groups {
                mark_worktree_branches(store, &worktrees);
            }
        }
        // Pull Requests は Branches の後に（開いているものがあるときだけ）。
        let pull_requests = self.pull_requests.borrow();
        let pr_shown = !filtering && !pull_requests.is_empty();
        if pr_shown {
            let at = groups
                .iter()
                .position(|(t, _)| *t == "Branches")
                .map_or(1, |i| i + 1);
            groups.insert(
                at,
                (
                    "Pull Requests",
                    pull_request_rows(&pull_requests, refs.as_ref()),
                ),
            );
        }
        drop(pull_requests);
        // 初めて出すときは開いておく。
        let open_pull_requests = pr_shown && self.pull_requests_first.replace(false);
        // 閉じていても中身があると分かるよう、見出しに数を添える（Workspace は除く。macOS 版と同じ）。
        let nodes: Vec<Node> = groups
            .into_iter()
            .map(|(title, children)| {
                let badge = if title == "Workspace" {
                    String::new()
                } else {
                    leaf_count(&children).to_string()
                };
                Node::new(
                    Data {
                        title: title.into(),
                        key: format!("group/{title}"),
                        badge,
                        ..Data::default()
                    },
                    Some(children),
                )
            })
            .collect();
        self.root.splice(0, self.root.n_items(), &nodes);

        // 見出しは、最初は Workspace と Branches だけを開いておく（macOS 版と同じ）。あとは前に開いていたものを開く
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
                    && (filtering || data.key == "group/Workspace" || data.key == "group/Branches");
                let stack = matches!(data.item, Item::Ref(_)) && !folded.contains(&data.key);
                let new_prs = open_pull_requests && data.key == "group/Pull Requests";
                if ((first || filtering) && group)
                    || expanded.contains(&data.key)
                    || stack
                    || new_prs
                {
                    row.set_expanded(true);
                }
            }
            i += 1;
        }
        drop(folded);
        // 作り直す前に選んでいた行を選び直す。なくなっていれば（消した worktree・stash など）、その行で出していた
        // 画面の行を選ぶ（画面は替わらないので、ほかの行を選んで見せない）。
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
            let page = selected.first().map_or("page/history", |k| page_of_key(k));
            self.select_key(page);
        }
        self.rebuilding.set(false);
    }

    /// `store` の `at` の項目を `node` に入れ替える。入れ替えた行の下の開き方と、選んでいた行は戻す。
    fn replace_keeping(&self, store: &gio::ListStore, at: u32, node: &Node) {
        let mut expanded = HashSet::new();
        for i in 0..self.model.n_items() {
            if let Some(row) = self.model.row(i)
                && row.is_expanded()
                && let Some(n) = row.item().and_downcast::<Node>()
            {
                expanded.insert(n.data().key);
            }
        }
        let selected: Vec<String> = self.selected_nodes().iter().map(|n| n.data().key).collect();
        self.rebuilding.set(true);
        store.splice(at, 1, std::slice::from_ref(node));
        let mut i = 0;
        while i < self.model.n_items() {
            if let Some(row) = self.model.row(i)
                && !row.is_expanded()
                && let Some(n) = row.item().and_downcast::<Node>()
                && expanded.contains(&n.data().key)
            {
                row.set_expanded(true);
            }
            i += 1;
        }
        for i in 0..self.model.n_items() {
            if let Some(n) = self
                .model
                .row(i)
                .and_then(|r| r.item().and_downcast::<Node>())
                && selected.contains(&n.data().key)
                && !self.selection.is_selected(i)
            {
                self.selection.select_item(i, false);
            }
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

/// 見出しの下の項目の数（子のない行を数える。リモートはブランチの数。macOS 版の `leafCount`）。
fn leaf_count(store: &gio::ListStore) -> usize {
    (0..store.n_items())
        .filter_map(|i| store.item(i).and_downcast::<Node>())
        .map(|node| match node.children() {
            Some(children) if children.n_items() > 0 => leaf_count(&children),
            _ => 1,
        })
        .sum()
}

/// ブランチの行の worktree の印: ほかの worktree が checkout しているもの（git はここでは checkout させない）は、
/// 右端に ⌂（メインの作業ツリー）か ⑂（追加した worktree。アイコンと一緒に緑）を出し、worktree の名前は
/// ツールチップに（ブランチ名を隠さない）。今のブランチ（書き込む先のもの）には ✎ を添える。
fn mark_worktree_branches(store: &gio::ListStore, view: &crate::worktrees::View) {
    for i in 0..store.n_items() {
        let Some(node) = store.item(i).and_downcast::<Node>() else {
            continue;
        };
        if let Some(children) = node.children() {
            mark_worktree_branches(&children, view);
        }
        let mut data = node.imp().data.borrow_mut();
        let Item::Ref(entry) = &data.item else {
            continue;
        };
        if entry.kind != RefKind::Local {
            continue;
        }
        let other = view
            .list
            .iter()
            .find(|w| w.key != view.editing && w.branch.as_deref() == Some(entry.name.as_str()));
        if let Some(w) = other {
            data.tooltip = Some(tr!(
                "Checked out in the worktree “{name}” ({path})",
                name = &w.name,
                path = &w.path
            ));
            data.badge = w.mark(false).to_owned();
            data.marks.clear();
            // 押すと、そのことと Open Worktree の吹き出し（macOS 版と同じ）。
            data.mark_click = true;
            data.green = !w.main;
        } else if data.head {
            data.badge = if data.badge.is_empty() {
                "✎".to_owned()
            } else {
                format!("✎ {}", data.badge)
            };
            // 押すと、書き込む先のブランチであることの吹き出し（macOS 版と同じ）。
            data.editing = true;
            data.mark_click = true;
        }
    }
}

/// worktree の行: 名前、印のアイコン（✎ 書き込む先・⌂ メイン・⑂ 追加したもの）、右端に変更の数（フォルダが
/// ない・ロック中ならそのこと）。ツールチップにパスとブランチ。
fn worktree_data(w: &crate::worktrees::Info, view: &crate::worktrees::View) -> Data {
    let editing = w.key == view.editing;
    let count = view.counts.get(&w.key).copied().unwrap_or(0);
    let badge = if w.prunable.is_some() {
        tr!("missing")
    } else if w.locked.is_some() {
        tr!("locked")
    } else if count > 0 {
        count.to_string()
    } else {
        String::new()
    };
    let lines = [
        Some(w.path.clone()),
        Some(match &w.branch {
            Some(b) => tr!("Branch: {name}", name = b),
            None => tr!("detached HEAD"),
        }),
        Some(if editing {
            tr!("Editing this worktree")
        } else {
            tr!("Click to edit this worktree")
        }),
        w.locked.as_ref().map(|l| {
            if l.is_empty() {
                tr!("Locked")
            } else {
                tr!("Locked: {reason}", reason = l)
            }
        }),
        w.prunable.as_ref().map(|p| {
            tr!(
                "Its folder is gone (Prune removes it): {reason}",
                reason = p
            )
        }),
    ];
    Data {
        title: w.name.clone(),
        key: format!("worktree/{}", w.key),
        item: Item::Worktree(w.clone()),
        icon: if editing {
            icon::Kind::WorkingCopy
        } else if w.main {
            icon::Kind::House
        } else {
            icon::Kind::Branch
        },
        head: editing,
        badge,
        tooltip: Some(lines.into_iter().flatten().collect::<Vec<_>>().join("\n")),
        ..Data::default()
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
/// - Inactive（最後のコミットから 90 日以上。畳んでおく。ほかの worktree が checkout しているもの（`elsewhere`）は
///   入れない）
/// - Archived（手でアーカイブしたもの。畳んでおく）
///
/// ほかのブランチの上に積んだブランチは、その土台のブランチの行の下に入れ子で出す（↳ の印）。
fn branch_rows(
    locals: &[&RefEntry],
    head: Option<&str>,
    groups: Option<&BranchGroups>,
    elsewhere: &HashSet<&str>,
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
        } else if !elsewhere.contains(e.name.as_str())
            && fact(&e.name)
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
    tags: Option<&TagRemotes>,
    elsewhere: &HashSet<&str>,
) -> (Vec<(&'static str, gio::ListStore)>, Vec<String>) {
    let tag_remotes = tags.map_or_else(
        || branch_remotes(refs),
        |t| t.remotes.iter().map(|(n, _)| n.clone()).collect(),
    );
    let of = |kind: RefKind| refs.entries.iter().filter(move |e| e.kind == kind);
    let head = refs.head_branch.as_deref();
    let mut groups = Vec::new();

    let locals: Vec<&RefEntry> = of(RefKind::Local).collect();
    let (rows, pinned) = branch_rows(&locals, head, branch_groups, elsewhere);
    if !rows.is_empty() {
        groups.push(("Branches", store(rows)));
    }

    // リモートごとにまとめる（origin/feature/x → origin の下の feature/x。その下はフォルダにしない。macOS 版と同じ）。
    let mut by_remote: Vec<(String, Vec<Row>)> = Vec::new();
    for e in of(RefKind::Remote) {
        let (remote, branch) = e.name.split_once('/').unwrap_or((&e.name, &e.name));
        let row = Row::leaf(ref_data(e, branch, icon::Kind::Branch, head, None));
        match by_remote.iter_mut().find(|(r, _)| r == remote) {
            Some((_, rows)) => rows.push(row),
            None => by_remote.push((remote.to_owned(), vec![row])),
        }
    }
    by_remote.sort_by(|a, b| a.0.cmp(&b.0));
    let remotes: Vec<Row> = by_remote
        .into_iter()
        .map(|(remote, children)| Row {
            data: Data {
                title: remote.clone(),
                key: format!("Remotes/{remote}"),
                item: Item::Folder,
                icon: icon::Kind::Remote,
                remote: Some(remote),
                ..Data::default()
            },
            children,
            folder: true,
        })
        .collect();
    if !remotes.is_empty() {
        groups.push(("Remotes", store(remotes)));
    }

    // タグも `v1/x` のような名前はフォルダにまとめる（macOS 版と同じ）。
    let mut tag_tree = Tree::default();
    for e in of(RefKind::Tag) {
        let parts: Vec<&str> = e.name.split('/').collect();
        let mut d = ref_data(e, parts[parts.len() - 1], icon::Kind::Tag, head, None);
        finish_tag(&mut d, tags, &tag_remotes);
        tag_tree.insert(&parts, d);
    }
    let tag_rows = tag_tree.into_rows("Tags");
    if !tag_rows.is_empty() {
        groups.push(("Tags", store(tag_rows)));
    }
    // stash は見出しにしない（Workspace の Stashes の行に数だけ。macOS 版と同じ）。
    let submodules = gio::ListStore::new::<Node>();
    for e in of(RefKind::Submodule) {
        let d = ref_data(e, &e.name, icon::Kind::Submodule, head, None);
        submodules.append(&Node::new(d, None));
    }
    if submodules.n_items() > 0 {
        groups.push(("Submodules", submodules));
    }
    (groups, pinned)
}

/// 絞り込み中の見出し: 名前が `words` をすべて含むブランチ・リモートブランチ・タグを、名前のまま平らに
/// （フォルダや積み重ねにはしない）。
fn filtered_groups(
    refs: &Refs,
    words: &[String],
    tags: Option<&TagRemotes>,
) -> Vec<(&'static str, gio::ListStore)> {
    let head = refs.head_branch.as_deref();
    let remotes = tags.map_or_else(
        || branch_remotes(refs),
        |t| t.remotes.iter().map(|(n, _)| n.clone()).collect(),
    );
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
            let mut d = ref_data(e, &e.name, icon, head, None);
            if kind == RefKind::Tag {
                finish_tag(&mut d, tags, &remotes);
            }
            store.append(&Node::new(d, None));
        }
        groups.push((title, store));
    }
    groups
}

/// お気に入りの順 `order` で、`name` を `target` の上（`above`）か下へ動かした順。どちらかがなければ None。
fn reordered(order: &[String], name: &str, target: &str, above: bool) -> Option<Vec<String>> {
    let from = order.iter().position(|n| n == name)?;
    let at = order.iter().position(|n| n == target)?;
    let index = if above { at } else { at + 1 };
    let mut order = order.to_vec();
    let moved = order.remove(from);
    order.insert(if index > from { index - 1 } else { index }, moved);
    Some(order)
}

/// リモートブランチのあるリモートの名前（名前順）。
fn branch_remotes(refs: &Refs) -> Vec<String> {
    let mut names: Vec<String> = refs
        .entries
        .iter()
        .filter(|e| e.kind == RefKind::Remote)
        .filter_map(|e| e.name.split_once('/').map(|(r, _)| r.to_owned()))
        .collect();
    names.sort();
    names.dedup();
    names
}

/// タグの行: どの（読んだ）リモートにもない・別のタグがあるリモートがあれば右端に印を出し、押すと吹き出し。
/// ツールチップには、すべてのリモートでの様子。
fn finish_tag(data: &mut Data, tags: Option<&TagRemotes>, remotes: &[String]) {
    // 行の名前はフォルダの下の部分だけのことがあるので、タグの名前は項目から。
    let name = match &data.item {
        Item::Ref(e) => e.name.clone(),
        _ => data.title.clone(),
    };
    let mark = tags.and_then(|t| t.mark(&name));
    let mut lines: Vec<String> = mark.iter().map(|(_, text, _)| text.clone()).collect();
    lines.extend(
        remotes
            .iter()
            .map(|r| reffacts::tag_state_text(tags, &name, r)),
    );
    if let Some((icon, tooltip, css)) = mark {
        data.marks.push(Mark { icon, tooltip, css });
        data.mark_click = true;
    }
    if !lines.is_empty() {
        data.tooltip = Some(lines.join("\n"));
    }
}

/// ref が指すコミット（History で選ぶ）。
pub fn target(data: &Data) -> Option<Oid> {
    match &data.item {
        Item::Ref(e) => e.target,
        // プルリクエストのブランチが手元にあれば、その先頭。
        Item::PullRequest(_, target) => *target,
        _ => None,
    }
}

/// Pull Requests の見出しの中身（macOS 版と同じ: `#番号 題`、右端に draft と CI の印）。
fn pull_request_rows(list: &[PullRequest], refs: Option<&Refs>) -> gio::ListStore {
    let store = gio::ListStore::new::<Node>();
    for pr in list {
        // ブランチが手元にあれば（リモートブランチ、なければローカルブランチ）、選ぶとその先頭へ。
        let entries = refs.map_or(&[][..], |r| &r.entries[..]);
        let target = entries
            .iter()
            .find(|e| {
                e.kind == RefKind::Remote && e.name.ends_with(&format!("/{}", pr.head_ref_name))
            })
            .or_else(|| {
                entries
                    .iter()
                    .find(|e| e.kind == RefKind::Local && e.name == pr.head_ref_name)
            })
            .and_then(|e| e.target);
        let ci = pr.ci();
        let badge: Vec<&str> = [pr.is_draft.then_some("draft"), Some(pr.ci_mark())]
            .into_iter()
            .flatten()
            .filter(|s| !s.is_empty())
            .collect();
        let data = Data {
            title: format!("#{} {}", pr.number, pr.title),
            key: format!("pr/{}", pr.number),
            icon: icon::Kind::Rebase,
            badge: badge.join(" "),
            tooltip: Some(pr.tooltip()),
            // CI の印は押せる（History の行の印と同じ吹き出し）。
            mark_click: ci != Ci::None && pr.head_ref_oid.is_some(),
            badge_css: match ci {
                Ci::Failing => Some("error"),
                Ci::Passing => Some("success"),
                _ => None,
            },
            item: Item::PullRequest(Box::new(pr.clone()), target),
            ..Data::default()
        };
        store.append(&Node::new(data, None));
    }
    store
}

#[cfg(test)]
mod tests {
    use super::reordered;

    #[test]
    fn reorders_favorites() {
        let order: Vec<String> = ["a", "b", "c"].map(String::from).to_vec();
        let names = |v: Option<Vec<String>>| v.unwrap().join(",");
        assert_eq!(names(reordered(&order, "a", "c", false)), "b,c,a");
        assert_eq!(names(reordered(&order, "a", "c", true)), "b,a,c");
        assert_eq!(names(reordered(&order, "c", "a", true)), "c,a,b");
        assert_eq!(names(reordered(&order, "c", "a", false)), "a,c,b");
        assert_eq!(names(reordered(&order, "b", "b", true)), "a,b,c");
        assert!(reordered(&order, "x", "a", true).is_none());
    }
}
