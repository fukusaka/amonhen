//! History の右の列の「すべてのファイル」（macOS 版の `CommitFilesBrowser`）: その時点のリポジトリ全体の木
//! （ツリーかリスト）と、開いたファイルの中身（右の列の幅いっぱい）。フォルダの中は開いたときに読む
//! （`treeEntries`）。変更ファイルには種類の印、変更を含むフォルダには橙の点を付け、このコミットで消したファイルは
//! 赤の取り消し線で足す。中身は差分と同じ部品で、その時点のファイルとして見せる（`DiffWidget::set_file_mode`）。

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use amonhen_core::Repository;
use amonhen_model::{ChangeKind, FileChange, Oid, tr};
use amonhen_projection::FrontendProfile;
use gtk::prelude::*;
use gtk::{gio, glib};

use crate::diff::{DiffWidget, View};

/// 木の 1 つ（フォルダかファイル）。
struct Node {
    name: String,
    path: String,
    is_dir: bool,
    /// blob・木の ID（このコミットで消したファイル・フォルダにはない）。
    oid: Option<Oid>,
    /// 読んだ子（フォルダで、まだ読んでいなければ None）。
    children: RefCell<Option<Vec<Rc<Node>>>>,
    kind: Option<ChangeKind>,
    contains_changes: bool,
}

/// 木を読む元（コミットと、その変更ファイル）。
struct Tree {
    repo: Arc<Repository>,
    commit: Oid,
    changes: Option<Arc<Vec<FileChange>>>,
    /// 変更ファイル（パス → 番号）と、変更を含むフォルダ。
    changed_files: HashMap<String, usize>,
    changed_folders: HashSet<String>,
    /// このコミットで消したファイル（親のフォルダ → パス。木にはないので足して赤で出す）。
    deleted_files: HashMap<String, Vec<String>>,
}

fn parent_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(p, _)| p)
}

fn name_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

impl Tree {
    /// `working` なら Working Copy の「すべてのファイル」: `commit` は HEAD、`changes` は作業ツリーの変更で、HEAD の
    /// 木にない足したファイル（untracked を含む）を足す（消したファイルは木にあるので、印を付けるだけ）。
    fn new(
        repo: Arc<Repository>,
        commit: Oid,
        changes: Option<Arc<Vec<FileChange>>>,
        working: bool,
    ) -> Self {
        let mut tree = Self {
            repo,
            commit,
            changes: changes.clone(),
            changed_files: HashMap::new(),
            changed_folders: HashSet::new(),
            deleted_files: HashMap::new(),
        };
        for (i, change) in changes.iter().flat_map(|c| c.iter()).enumerate() {
            tree.changed_files.insert(change.path.clone(), i);
            let mut folder = parent_of(&change.path);
            while !folder.is_empty() {
                tree.changed_folders.insert(folder.to_owned());
                folder = parent_of(folder);
            }
            tree.changed_folders.insert(String::new());
            let missing = if working {
                matches!(
                    change.kind,
                    ChangeKind::Added | ChangeKind::Renamed | ChangeKind::Copied
                )
            } else {
                change.kind == ChangeKind::Deleted
            };
            if missing {
                tree.deleted_files
                    .entry(parent_of(&change.path).to_owned())
                    .or_default()
                    .push(change.path.clone());
            }
        }
        tree
    }

    fn change(&self, path: &str) -> Option<(usize, &FileChange)> {
        let index = *self.changed_files.get(path)?;
        Some((index, self.changes.as_ref()?.get(index)?))
    }

    /// フォルダの中を読む（まだなら）。このコミットで消したファイルも足す。
    fn load(&self, node: &Node) {
        if !node.is_dir || node.children.borrow().is_some() {
            return;
        }
        let request = serde_json::json!({
            "op": "treeEntries",
            "oid": self.commit.to_string(),
            "path": node.path,
        });
        let response: serde_json::Value =
            serde_json::from_str(&self.repo.run_json(&request.to_string())).unwrap_or_default();
        let mut children: Vec<Rc<Node>> = response["info"]["entries"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|e| {
                let name = e["name"].as_str().unwrap_or_default().to_owned();
                let path = if node.path.is_empty() {
                    name.clone()
                } else {
                    format!("{}/{name}", node.path)
                };
                let is_dir = e["kind"] == "folder";
                let kind = self.change(&path).map(|(_, c)| c.kind);
                let contains_changes =
                    kind.is_some() || (is_dir && self.changed_folders.contains(&path));
                Rc::new(Node {
                    name,
                    path,
                    is_dir,
                    oid: e["oid"].as_str().and_then(Oid::from_hex),
                    children: RefCell::new(None),
                    kind,
                    contains_changes,
                })
            })
            .collect();
        for path in self.deleted_files.get(&node.path).into_iter().flatten() {
            children.push(Rc::new(Node {
                name: name_of(path).to_owned(),
                path: path.clone(),
                is_dir: false,
                oid: None,
                children: RefCell::new(None),
                kind: Some(
                    self.change(path)
                        .map_or(ChangeKind::Deleted, |(_, c)| c.kind),
                ),
                contains_changes: true,
            }));
        }
        // 消したフォルダ（このコミットで中のファイルをすべて消した）も、変更を含むフォルダとして足す。
        let names: HashSet<String> = children.iter().map(|c| c.name.clone()).collect();
        for folder in &self.changed_folders {
            if !folder.is_empty()
                && parent_of(folder) == node.path
                && !names.contains(name_of(folder))
            {
                children.push(Rc::new(Node {
                    name: name_of(folder).to_owned(),
                    path: folder.clone(),
                    is_dir: true,
                    oid: None,
                    children: RefCell::new(None),
                    kind: None,
                    contains_changes: true,
                }));
            }
        }
        children.sort_by(|a, b| {
            (!a.is_dir, a.name.to_lowercase()).cmp(&(!b.is_dir, b.name.to_lowercase()))
        });
        node.children.replace(Some(children));
    }

    /// リストのために、すべてのフォルダを読む。
    fn load_all(&self, root: &Rc<Node>) {
        let mut stack = vec![root.clone()];
        while let Some(node) = stack.pop() {
            self.load(&node);
            stack.extend(
                node.children
                    .borrow()
                    .iter()
                    .flatten()
                    .filter(|c| c.is_dir)
                    .cloned(),
            );
        }
    }
}

fn object(node: &Rc<Node>) -> glib::BoxedAnyObject {
    glib::BoxedAnyObject::new(node.clone())
}

fn node_of(item: &glib::Object) -> Option<Rc<Node>> {
    let item = match item.downcast_ref::<gtk::TreeListRow>() {
        Some(row) => row.item()?,
        None => item.clone(),
    };
    let boxed = item.downcast::<glib::BoxedAnyObject>().ok()?;
    let node = boxed.borrow::<Rc<Node>>().clone();
    Some(node)
}

/// ファイルの番号を受け取る知らせ先。
type OnFile = Rc<dyn Fn(usize)>;
/// ファイルを開いたときに、中身の欄の代わりに呼ぶもの（パス）。
type OnOpenFile = Rc<dyn Fn(&str)>;

/// SVG の見せ方（macOS 版の `SVGView`）: 絵（librsvg で描く）・モバイル（絵と、モバイル向けの描画部品で崩れる
/// 機能の一覧）・テキスト。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SvgView {
    Picture,
    Mobile,
    Text,
}

impl SvgView {
    const ALL: [SvgView; 3] = [SvgView::Picture, SvgView::Mobile, SvgView::Text];

    /// 最初の見せ方。ふだんは絵で、確かめる用（`--snapshot`）に `AMH_SVG_VIEW=picture|mobile|text` で変えられる。
    fn initial() -> Self {
        match std::env::var("AMH_SVG_VIEW").as_deref() {
            Ok("mobile") => SvgView::Mobile,
            Ok("text") => SvgView::Text,
            _ => SvgView::Picture,
        }
    }

    fn title(self) -> String {
        match self {
            SvgView::Picture => tr!("Picture"),
            SvgView::Mobile => tr!("Mobile"),
            SvgView::Text => tr!("Text"),
        }
    }

    fn tooltip(self) -> String {
        match self {
            SvgView::Picture => tr!("Draw the SVG"),
            SvgView::Mobile => {
                tr!("Check for mobile: list what breaks on Android, iOS, Flutter and React Native")
            }
            SvgView::Text => tr!("Show the SVG source"),
        }
    }
}

/// コミット `commit` の木の `path` のファイルの blob（なければ None）。裏のスレッドで呼ぶ。
fn tree_blob(repo: &Repository, commit: &Oid, path: &str) -> Option<Oid> {
    let (folder, name) = path.rsplit_once('/').unwrap_or(("", path));
    let request = serde_json::json!({
        "op": "treeEntries",
        "oid": commit.to_string(),
        "path": folder,
    });
    let response: serde_json::Value =
        serde_json::from_str(&repo.run_json(&request.to_string())).ok()?;
    response["info"]["entries"]
        .as_array()?
        .iter()
        .find(|e| e["name"] == name && e["kind"] != "folder")
        .and_then(|e| e["oid"].as_str())
        .and_then(Oid::from_hex)
}

/// モバイルで見る部品の並び（切り替えのボタンの順）と、ボタンの名前。
const RENDERERS: [(amonhen_core::svglint::Target, &str); 4] = [
    (amonhen_core::svglint::Target::Android, "Android"),
    (amonhen_core::svglint::Target::Ios, "iOS"),
    (amonhen_core::svglint::Target::Flutter, "Flutter"),
    (amonhen_core::svglint::Target::ReactNative, "React Native"),
];

/// 最初にモバイルで見る部品。ふだんは Android で、確かめる用（`--snapshot`）に
/// `AMH_MOBILE_RENDERER=android|ios|flutter|reactNative` で変えられる（macOS 版の `AMHMobileRenderer`）。
fn initial_renderer() -> amonhen_core::svglint::Target {
    let key = std::env::var("AMH_MOBILE_RENDERER").unwrap_or_default();
    RENDERERS
        .iter()
        .map(|(t, _)| *t)
        .find(|t| t.key() == key)
        .unwrap_or(amonhen_core::svglint::Target::Android)
}

/// モバイル向けの描画部品ごとの、崩れる機能の一覧（macOS 版の `mobileNotes`）。
/// 上に、見ている部品の取り込みが読めなかった理由・警告（`preview`）を置く。
fn mobile_notes(
    issues: &[amonhen_core::svglint::Issue],
    preview: &amonhen_core::svgmobile::Preview,
) -> gtk::Widget {
    use amonhen_core::svglint::Target;
    let label = gtk::Label::new(None);
    label.set_xalign(0.0);
    label.set_yalign(0.0);
    label.set_wrap(true);
    label.set_selectable(true);
    label.set_margin_top(4);
    let esc = |s: &str| glib::markup_escape_text(s).to_string();
    let mut head = String::new();
    if let Some(error) = &preview.error {
        head += &format!("<b><span foreground=\"#d33\">{}</span></b>\n", esc(error));
    }
    if !preview.messages.is_empty() {
        head += &format!(
            "<b>{}</b>\n",
            esc(&tr!("What {name} reports", name = preview.title))
        );
        for message in &preview.messages {
            head += &format!(
                "    <span font_family=\"monospace\" size=\"small\" alpha=\"60%\">{}</span>\n",
                esc(message)
            );
        }
    }
    if !head.is_empty() {
        head.push('\n');
    }
    let mut out = String::new();
    if issues.is_empty() {
        out += &esc(&tr!(
            "Nothing known to break on Android VectorDrawable, iOS (CoreSVG), flutter_svg or react-native-svg."
        ));
    } else {
        for target in [
            Target::Android,
            Target::Ios,
            Target::Flutter,
            Target::ReactNative,
        ] {
            if !out.is_empty() {
                out.push('\n');
            }
            out += &format!("<b>{}</b>", esc(target.name()));
            let own: Vec<_> = issues
                .iter()
                .filter(|i| i.targets.contains(&target))
                .collect();
            if own.is_empty() {
                out += &format!("\n    {}", esc(&tr!("Nothing known to break")));
            }
            for issue in own {
                let count = if issue.count > 1 {
                    format!(" ×{}", issue.count)
                } else {
                    String::new()
                };
                out += &format!(
                    "\n    <span font_family=\"monospace\" size=\"small\" alpha=\"60%\">{}{count}</span>  {}",
                    esc(&issue.feature),
                    esc(&issue.message)
                );
            }
        }
    }
    label.set_markup(&(head + &out));
    label.upcast()
}

pub struct Browser {
    pub root: gtk::Box,
    tree: Rc<Tree>,
    top: Rc<Node>,
    model: gtk::TreeListModel,
    flat_store: gio::ListStore,
    selection: gtk::SingleSelection,
    /// リストで見せているか（行の書き方でも使う）。
    flat: Rc<Cell<bool>>,
    pages: gtk::Stack,
    contents: gtk::Stack,
    path_bar: gtk::Box,
    path_label: gtk::Label,
    change_button: gtk::Button,
    content: DiffWidget,
    /// 開いた画像（拡大縮小できる 1 枚の表示。`imagediff::single`）を置く所。
    image_holder: gtk::Box,
    /// SVG の見せ方の切り替え（SVG を開いたときだけ出す）と、今の見せ方（この木を出している間は覚える）。
    svg_switch: gtk::Box,
    svg_buttons: Vec<(SvgView, gtk::ToggleButton)>,
    /// モバイルで見る部品（切り替えは比べる欄の帯の左端。`renderer_switch`）。
    renderer: Cell<amonhen_core::svglint::Target>,
    /// Markdown の見せ方の切り替え（整形・テキスト。Markdown を開いたときだけ出す）と、テキストで見るか。
    markdown_switch: gtk::Box,
    markdown_buttons: [gtk::ToggleButton; 2],
    markdown_text: Cell<bool>,
    /// Android の VectorDrawable の見せ方の切り替え（絵・テキスト。VectorDrawable を開いたときだけ出す）と、
    /// テキストで見るか。
    drawable_switch: gtk::Box,
    drawable_buttons: [gtk::ToggleButton; 2],
    drawable_text: Cell<bool>,
    /// Markdown の整形表示を置く所。
    markdown_holder: gtk::Box,
    /// Markdown の #123・@user・コミットの ID をリンクにする先（`https://github.com/owner/repo`。GitHub の
    /// remote のときだけ）。
    github: RefCell<Option<String>>,
    svg_view: Cell<SvgView>,
    /// 切り替えを、こちらから合わせている間（押したことにしない）。
    syncing: Cell<bool>,
    open_path: RefCell<Option<String>>,
    request: Cell<u64>,
    pending: Rc<Cell<u32>>,
    on_show_change: RefCell<Option<OnFile>>,
    on_folders_change: RefCell<Option<Rc<dyn Fn()>>>,
    /// ファイルを開いたときに、自分の中身の欄の代わりに呼ぶ（Working Copy の「すべてのファイル」。パス）。
    on_open_file: RefCell<Option<OnOpenFile>>,
}

impl Browser {
    pub fn new(
        repo: Arc<Repository>,
        commit: Oid,
        changes: Option<Arc<Vec<FileChange>>>,
        flat: bool,
        pending: Rc<Cell<u32>>,
    ) -> Rc<Self> {
        Self::build(repo, commit, changes, flat, pending, false)
    }

    /// Working Copy の「すべてのファイル」の木（HEAD の木に作業ツリーの変更を重ねる。足したファイルも出す）。
    /// ファイルを開くと、中身の欄を出さずに `open` を呼ぶ。
    pub fn new_working(
        repo: Arc<Repository>,
        head: Oid,
        changes: Vec<FileChange>,
        flat: bool,
        pending: Rc<Cell<u32>>,
        open: impl Fn(&str) + 'static,
    ) -> Rc<Self> {
        let this = Self::build(repo, head, Some(Arc::new(changes)), flat, pending, true);
        this.on_open_file.replace(Some(Rc::new(open)));
        this
    }

    fn build(
        repo: Arc<Repository>,
        commit: Oid,
        changes: Option<Arc<Vec<FileChange>>>,
        flat: bool,
        pending: Rc<Cell<u32>>,
        working: bool,
    ) -> Rc<Self> {
        let tree = Rc::new(Tree::new(repo, commit, changes, working));
        let top = Rc::new(Node {
            name: String::new(),
            path: String::new(),
            is_dir: true,
            oid: None,
            children: RefCell::new(None),
            kind: None,
            contains_changes: true,
        });
        tree.load(&top);
        let roots = gio::ListStore::new::<glib::BoxedAnyObject>();
        for child in top.children.borrow().iter().flatten() {
            roots.append(&object(child));
        }
        let t = tree.clone();
        let model = gtk::TreeListModel::new(roots, false, false, move |item| {
            let node = node_of(item)?;
            if !node.is_dir {
                return None;
            }
            t.load(&node);
            let store = gio::ListStore::new::<glib::BoxedAnyObject>();
            for child in node.children.borrow().iter().flatten() {
                store.append(&object(child));
            }
            Some(store.upcast())
        });
        let selection = gtk::SingleSelection::new(Some(model.clone()));
        selection.set_autoselect(false);
        selection.set_can_unselect(true);
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(|_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let icon = crate::icon::Icon::default();
            let label = gtk::Label::new(None);
            label.set_xalign(0.0);
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            let line = gtk::Box::new(gtk::Orientation::Horizontal, 5);
            line.set_margin_top(2);
            line.set_margin_bottom(2);
            line.append(&icon);
            line.append(&label);
            let expander = gtk::TreeExpander::new();
            expander.set_child(Some(&line));
            item.set_child(Some(&expander));
        });
        let flat_flag = Rc::new(Cell::new(false));
        let f = flat_flag.clone();
        factory.connect_bind(move |_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let (Some(expander), Some(object)) = (
                item.child().and_downcast::<gtk::TreeExpander>(),
                item.item(),
            ) else {
                return;
            };
            expander.set_list_row(object.downcast_ref::<gtk::TreeListRow>());
            let Some(node) = node_of(&object) else { return };
            let Some(line) = expander.child().and_downcast::<gtk::Box>() else {
                return;
            };
            let (Some(icon), Some(label)) = (
                line.first_child().and_downcast::<crate::icon::Icon>(),
                line.last_child().and_downcast::<gtk::Label>(),
            ) else {
                return;
            };
            icon.set(
                if node.is_dir {
                    crate::icon::Kind::Folder
                } else {
                    crate::icon::Kind::Doc
                },
                false,
            );
            if node.is_dir {
                icon.remove_css_class("dim-label");
            } else {
                icon.add_css_class("dim-label");
            }
            label.set_markup(&row_markup(&node, f.get()));
        });
        let list = gtk::ListView::new(Some(selection.clone()), Some(factory));
        list.set_single_click_activate(true);
        let tree_scroll = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();

        // 開いたファイルの、パスの帯と中身。
        let back_icon = crate::icon::Icon::default();
        back_icon.set(crate::icon::Kind::ChevronLeft, false);
        let back = gtk::Button::new();
        back.set_child(Some(&back_icon));
        back.add_css_class("flat");
        back.set_tooltip_text(Some(&tr!("Back to the files")));
        back.update_property(&[gtk::accessible::Property::Label(&tr!("Back"))]);
        let path_label = gtk::Label::new(None);
        path_label.set_xalign(0.0);
        path_label.set_hexpand(true);
        path_label.set_ellipsize(gtk::pango::EllipsizeMode::Start);
        let change_button = gtk::Button::with_label(&tr!("Show This Change"));
        change_button.add_css_class("flat");
        // プレビューで開く（macOS 版の Quick Look のボタン）。
        let preview_icon = crate::icon::Icon::default();
        preview_icon.set(crate::icon::Kind::Eye, false);
        let preview_button = gtk::Button::new();
        preview_button.set_child(Some(&preview_icon));
        preview_button.add_css_class("flat");
        preview_button.set_tooltip_text(Some(&tr!("Open this file at the commit in a preview")));
        preview_button.update_property(&[gtk::accessible::Property::Label(&tr!("Preview"))]);
        let path_bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        path_bar.set_margin_start(6);
        path_bar.set_margin_end(10);
        path_bar.set_margin_top(2);
        path_bar.set_margin_bottom(2);
        path_bar.append(&back);
        path_bar.append(&path_label);
        let svg_switch = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        svg_switch.add_css_class("linked");
        svg_switch.set_visible(false);
        let mut svg_buttons: Vec<(SvgView, gtk::ToggleButton)> = Vec::new();
        for view in SvgView::ALL {
            let button = gtk::ToggleButton::with_label(&view.title());
            button.set_tooltip_text(Some(&view.tooltip()));
            if let Some((_, first)) = svg_buttons.first() {
                button.set_group(Some(first));
            }
            svg_switch.append(&button);
            svg_buttons.push((view, button));
        }
        let markdown_switch = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        markdown_switch.add_css_class("linked");
        markdown_switch.set_visible(false);
        let markdown_buttons = [
            (tr!("Preview"), tr!("Show the Markdown formatted")),
            (tr!("Text"), tr!("Show the Markdown source")),
        ]
        .map(|(title, tip)| {
            let button = gtk::ToggleButton::with_label(&title);
            button.set_tooltip_text(Some(&tip));
            markdown_switch.append(&button);
            button
        });
        markdown_buttons[1].set_group(Some(&markdown_buttons[0]));
        let drawable_switch = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        drawable_switch.add_css_class("linked");
        drawable_switch.set_visible(false);
        let drawable_buttons = [
            (tr!("Picture"), tr!("Draw the VectorDrawable")),
            (tr!("Text"), tr!("Show the XML source")),
        ]
        .map(|(title, tip)| {
            let button = gtk::ToggleButton::with_label(&title);
            button.set_tooltip_text(Some(&tip));
            drawable_switch.append(&button);
            button
        });
        drawable_buttons[1].set_group(Some(&drawable_buttons[0]));
        path_bar.append(&markdown_switch);
        path_bar.append(&svg_switch);
        path_bar.append(&drawable_switch);
        path_bar.append(&preview_button);
        path_bar.append(&change_button);
        path_bar.set_visible(false);
        let content = DiffWidget::default();
        content.set_file_mode(true);
        let content_scroll = gtk::ScrolledWindow::builder()
            .child(&content)
            .vexpand(true)
            .build();
        let image_holder = gtk::Box::new(gtk::Orientation::Vertical, 0);
        image_holder.set_vexpand(true);
        // バイナリのファイルの案内と、プレビューで開くボタン（アプリで描けないもの）。
        let binary_label = gtk::Label::new(Some(&tr!("Binary file")));
        binary_label.add_css_class("dim-label");
        let binary_button = gtk::Button::with_label(&tr!("Preview"));
        binary_button.set_halign(gtk::Align::Center);
        let binary_box = gtk::Box::new(gtk::Orientation::Vertical, 10);
        binary_box.set_halign(gtk::Align::Center);
        binary_box.set_valign(gtk::Align::Center);
        binary_box.append(&binary_label);
        binary_box.append(&binary_button);
        let contents = gtk::Stack::new();
        contents.add_named(&content_scroll, Some("text"));
        contents.add_named(&image_holder, Some("image"));
        contents.add_named(&binary_box, Some("binary"));
        let markdown_holder = gtk::Box::new(gtk::Orientation::Vertical, 0);
        markdown_holder.set_vexpand(true);
        contents.add_named(&markdown_holder, Some("markdown"));
        let pages = gtk::Stack::new();
        pages.set_vexpand(true);
        pages.add_named(&tree_scroll, Some("tree"));
        pages.add_named(&contents, Some("file"));
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.append(&path_bar);
        root.append(&pages);

        let flat_store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let this = Rc::new(Self {
            root,
            tree,
            top,
            model,
            flat_store,
            selection,
            flat: flat_flag,
            pages,
            contents,
            path_bar,
            path_label,
            change_button,
            content,
            image_holder,
            svg_switch,
            svg_buttons,
            renderer: Cell::new(initial_renderer()),
            markdown_switch,
            markdown_buttons,
            // 確かめる用（`--snapshot`）に `AMH_MARKDOWN_VIEW=text` でテキストから（macOS 版の `AMHMarkdownView`）。
            markdown_text: Cell::new(std::env::var("AMH_MARKDOWN_VIEW").is_ok_and(|v| v == "text")),
            markdown_holder,
            drawable_switch,
            drawable_buttons,
            // 確かめる用（`--snapshot`）に `AMH_DRAWABLE_VIEW=text` でテキストから（macOS 版の `AMHDrawableView`）。
            drawable_text: Cell::new(std::env::var("AMH_DRAWABLE_VIEW").is_ok_and(|v| v == "text")),
            github: RefCell::default(),
            svg_view: Cell::new(SvgView::initial()),
            syncing: Cell::new(false),
            open_path: RefCell::default(),
            request: Cell::new(0),
            pending,
            on_show_change: RefCell::default(),
            on_folders_change: RefCell::default(),
            on_open_file: RefCell::default(),
        });
        this.expand_changed();
        this.set_flat(flat);
        let weak = Rc::downgrade(&this);
        list.connect_activate(move |list, position| {
            let Some(this) = weak.upgrade() else { return };
            let Some(item) = list.model().and_then(|m| m.item(position)) else {
                return;
            };
            if let Some(row) = item.downcast_ref::<gtk::TreeListRow>()
                && row.is_expandable()
            {
                row.set_expanded(!row.is_expanded());
                return;
            }
            if let Some(node) = node_of(&item)
                && !node.is_dir
            {
                let open = this.on_open_file.borrow().clone();
                if let Some(open) = open {
                    open(&node.path);
                    return;
                }
                this.open(&node.path, None);
            }
        });
        let weak = Rc::downgrade(&this);
        this.model.connect_items_changed(move |_, _, _, _| {
            let f = weak
                .upgrade()
                .and_then(|this| this.on_folders_change.borrow().clone());
            if let Some(f) = f {
                f();
            }
        });
        let weak = Rc::downgrade(&this);
        back.connect_clicked(move |_| {
            if let Some(this) = weak.upgrade() {
                this.back_to_tree();
            }
        });
        // gantt の時間帯・数式の設定を変えたら、整形した Markdown を読み直す（日付の位置・式の大きさが変わる）
        {
            let weak = Rc::downgrade(&this);
            crate::settings::connect_changed_while(&this.root, move |key| {
                let Some(this) = weak.upgrade() else { return };
                let watched = [
                    crate::theme::GANTT_TIME_ZONE,
                    crate::theme::MATH_SCALE,
                    crate::theme::MATH_TIME_LIMIT,
                ];
                if !watched.contains(&key) || this.markdown_text.get() {
                    return;
                }
                let path = this.open_path.borrow().clone();
                if let Some(path) = path
                    && amonhen_core::markdown::is_markdown(&path)
                {
                    this.open(&path, None);
                }
            });
        }
        for (i, button) in this.markdown_buttons.iter().enumerate() {
            let weak = Rc::downgrade(&this);
            button.connect_toggled(move |b| {
                let Some(this) = weak.upgrade() else { return };
                let text = i == 1;
                if !b.is_active() || this.syncing.get() || this.markdown_text.get() == text {
                    return;
                }
                this.markdown_text.set(text);
                let path = this.open_path.borrow().clone();
                if let Some(path) = path {
                    this.open(&path, None);
                }
            });
        }
        // VectorDrawable の絵は引けない色を外観の文字の色で描くので、外観が変わったら描き直す
        {
            let weak = Rc::downgrade(&this);
            let id = adw::StyleManager::default().connect_dark_notify(move |_| {
                let Some(this) = weak.upgrade() else { return };
                if !this.drawable_switch.is_visible() || this.drawable_text.get() {
                    return;
                }
                let path = this.open_path.borrow().clone();
                if let Some(path) = path {
                    this.open(&path, None);
                }
            });
            let id = RefCell::new(Some(id));
            this.root.connect_destroy(move |_| {
                if let Some(id) = id.take() {
                    adw::StyleManager::default().disconnect(id);
                }
            });
        }
        for (i, button) in this.drawable_buttons.iter().enumerate() {
            let weak = Rc::downgrade(&this);
            button.connect_toggled(move |b| {
                let Some(this) = weak.upgrade() else { return };
                let text = i == 1;
                if !b.is_active() || this.syncing.get() || this.drawable_text.get() == text {
                    return;
                }
                this.drawable_text.set(text);
                let path = this.open_path.borrow().clone();
                if let Some(path) = path {
                    this.open(&path, None);
                }
            });
        }
        for (view, button) in &this.svg_buttons {
            let (weak, view) = (Rc::downgrade(&this), *view);
            button.connect_toggled(move |b| {
                let Some(this) = weak.upgrade() else { return };
                if !b.is_active() || this.syncing.get() || this.svg_view.get() == view {
                    return;
                }
                this.svg_view.set(view);
                let path = this.open_path.borrow().clone();
                if let Some(path) = path {
                    this.open(&path, None);
                }
            });
        }
        for button in [&preview_button, &binary_button] {
            let weak = Rc::downgrade(&this);
            button.connect_clicked(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.preview();
                }
            });
        }
        let weak = Rc::downgrade(&this);
        this.change_button.connect_clicked(move |_| {
            let Some(this) = weak.upgrade() else { return };
            let file = this
                .open_path
                .borrow()
                .as_deref()
                .and_then(|p| this.tree.changed_files.get(p).copied());
            let f = this.on_show_change.borrow().clone();
            if let (Some(file), Some(f)) = (file, f) {
                f(file);
            }
        });
        this
    }

    /// 「この変更を見る」: 変更の表示に戻り、そのファイルを開く（ファイルの番号）。
    pub fn connect_show_change(&self, f: impl Fn(usize) + 'static) {
        self.on_show_change.replace(Some(Rc::new(f)));
    }

    /// フォルダを開いた・畳んだ（帯のフォルダのボタンの向き）。
    pub fn connect_folders_change(&self, f: impl Fn() + 'static) {
        self.on_folders_change.replace(Some(Rc::new(f)));
    }

    /// リストで見せる（読んだフォルダのファイルをすべてパスで並べる）か、ツリーで見せる。
    pub fn set_flat(&self, flat: bool) {
        if flat == self.flat.get() {
            return;
        }
        self.flat.set(flat);
        if flat {
            self.tree.load_all(&self.top);
            let mut files = Vec::new();
            let mut stack = vec![self.top.clone()];
            while let Some(node) = stack.pop() {
                for child in node.children.borrow().iter().flatten() {
                    if child.is_dir {
                        stack.push(child.clone());
                    } else {
                        files.push(child.clone());
                    }
                }
            }
            files.sort_by_key(|n| n.path.to_lowercase());
            self.flat_store.remove_all();
            let objects: Vec<glib::BoxedAnyObject> = files.iter().map(object).collect();
            self.flat_store.extend_from_slice(&objects);
            self.selection.set_model(Some(&self.flat_store));
        } else {
            self.selection.set_model(Some(&self.model));
            self.expand_changed();
        }
    }

    /// 変更を含むフォルダを開く（中を読みながら）。
    fn expand_changed(&self) {
        let mut i = 0;
        while i < self.model.n_items() {
            if let Some(row) = self.model.row(i)
                && let Some(node) = row.item().and_then(|o| node_of(&o))
                && node.is_dir
                && node.contains_changes
                && !row.is_expanded()
            {
                row.set_expanded(true);
            }
            i += 1;
        }
    }

    /// 開いているフォルダのパス（作り直しで保つ）。
    pub fn expanded_paths(&self) -> HashSet<String> {
        (0..self.model.n_items())
            .filter_map(|i| self.model.row(i))
            .filter(|r| r.is_expandable() && r.is_expanded())
            .filter_map(|r| r.item().and_then(|o| node_of(&o)).map(|n| n.path.clone()))
            .collect()
    }

    /// 作り直した木で、前に開いていたフォルダを開き直し、`selected` のファイルの行を選ぶ。
    pub fn restore(&self, expanded: &HashSet<String>, selected: Option<&str>) {
        if !self.flat.get() && !expanded.is_empty() {
            let mut i = 0;
            while i < self.model.n_items() {
                if let Some(row) = self.model.row(i)
                    && let Some(node) = row.item().and_then(|o| node_of(&o))
                    && node.is_dir
                {
                    row.set_expanded(expanded.contains(&node.path));
                }
                i += 1;
            }
        }
        let Some(selected) = selected else { return };
        let model = self.selection.model();
        let Some(model) = model else { return };
        let found = (0..model.n_items()).find(|&i| {
            model.item(i).is_some_and(|o| {
                let o = o
                    .downcast_ref::<gtk::TreeListRow>()
                    .and_then(|r| r.item())
                    .unwrap_or(o);
                node_of(&o).is_some_and(|n| n.path == selected)
            })
        });
        if let Some(i) = found {
            self.selection.set_selected(i);
        }
    }

    /// 開いているフォルダがあるか（フォルダのボタンの向き）。
    pub fn any_folder_expanded(&self) -> bool {
        !self.flat.get()
            && (0..self.model.n_items()).any(|i| {
                self.model
                    .row(i)
                    .is_some_and(|r| r.depth() == 0 && r.is_expandable() && r.is_expanded())
            })
    }

    /// すべてのフォルダを開く・畳む（開くときは、中を読む）。
    pub fn set_all_folders(&self, expanded: bool) {
        if self.flat.get() {
            return;
        }
        if expanded {
            self.tree.load_all(&self.top);
            let mut i = 0;
            while i < self.model.n_items() {
                if let Some(row) = self.model.row(i)
                    && row.is_expandable()
                {
                    row.set_expanded(true);
                }
                i += 1;
            }
        } else {
            let mut i = 0;
            while i < self.model.n_items() {
                if let Some(row) = self.model.row(i)
                    && row.depth() == 0
                {
                    row.set_expanded(false);
                }
                i += 1;
            }
        }
    }

    /// 前に開いた画像の欄を捨てる。
    fn clear_image(&self) {
        for holder in [&self.image_holder, &self.markdown_holder] {
            while let Some(child) = holder.first_child() {
                holder.remove(&child);
            }
        }
    }

    /// Markdown の #123・@user・コミットの ID をリンクにする先（GitHub の remote のときだけ）。
    pub fn set_github(&self, url: Option<String>) {
        self.github.replace(url);
    }

    fn back_to_tree(&self) {
        self.open_path.replace(None);
        self.clear_image();
        self.request.set(self.request.get() + 1);
        self.path_bar.set_visible(false);
        self.pages.set_visible_child_name("tree");
    }

    /// 読んだ木の中の `path` のもの。
    fn node_at(&self, path: &str) -> Option<Rc<Node>> {
        let mut node = self.top.clone();
        for part in path.split('/') {
            let next = node
                .children
                .borrow()
                .iter()
                .flatten()
                .find(|c| c.name == part)
                .cloned()?;
            node = next;
        }
        Some(node)
    }

    /// モバイルで見る部品の切り替え（比べる欄の帯の左端に置く。帯にまとめるとファイルの帯が狭くなり、ファイルの
    /// 名前が読めなくなるため。macOS 版と同じ）。選び直すと開き直す。
    fn renderer_switch(self: &Rc<Self>) -> gtk::Widget {
        let switch = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        switch.add_css_class("linked");
        let mut first: Option<gtk::ToggleButton> = None;
        for (target, title) in RENDERERS {
            let button = gtk::ToggleButton::with_label(title);
            button.set_tooltip_text(Some(&tr!(
                "How the SVG looks on {name}",
                name = target.name()
            )));
            if let Some(first) = &first {
                button.set_group(Some(first));
            }
            button.set_active(target == self.renderer.get());
            let weak = Rc::downgrade(self);
            button.connect_toggled(move |b| {
                let Some(this) = weak.upgrade() else { return };
                if !b.is_active() || this.renderer.get() == target {
                    return;
                }
                this.renderer.set(target);
                let path = this.open_path.borrow().clone();
                if let Some(path) = path {
                    this.open(&path, None);
                }
            });
            switch.append(&button);
            first.get_or_insert(button);
        }
        switch.upcast()
    }

    /// Markdown を整形して出す（中身・画像は裏で読む。画像はこのコミットの中のものだけ）。
    fn show_markdown(self: &Rc<Self>, path: &str, request: u64) {
        self.contents.set_visible_child_name("markdown");
        let oid = self.blob_id(path);
        let github = self.github.borrow().clone();
        let zone = crate::theme::gantt_zone();
        // 変えたファイルは変更後の blob（消したファイルは None）。ほかはその時点の木から。
        let changed: HashMap<String, Option<Oid>> = self
            .tree
            .changes
            .iter()
            .flat_map(|c| c.iter())
            .map(|c| (c.path.clone(), c.new_id))
            .collect();
        let (repo, commit, from) = (self.tree.repo.clone(), self.tree.commit, path.to_owned());
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let f = from.clone();
            let result = gio::spawn_blocking(move || {
                let data = oid.and_then(|oid| repo.blob(&oid).ok())?;
                let text = String::from_utf8_lossy(&data);
                // このコミットのファイルの中身（リポジトリの根からのパス。変えたファイルは変更後）
                let changed = Arc::new(changed);
                let read = {
                    let (repo, changed) = (repo.clone(), changed.clone());
                    move |path: &str| -> Option<Arc<Vec<u8>>> {
                        let id = match changed.get(path) {
                            Some(id) => (*id)?,
                            None => tree_blob(&repo, &commit, path)?,
                        };
                        repo.blob(&id).ok()
                    }
                };
                let read = Arc::new(read);
                // 図（DOT）の画像も同じコミットから
                let images = amonhen_core::diagram::ImageSource {
                    from: f.clone(),
                    read: {
                        let read = read.clone();
                        Arc::new(move |path: &str| read(path).map(|b| b.to_vec()))
                    },
                };
                let blocks = amonhen_core::markdown::parse_with_images(
                    &text,
                    github.as_deref(),
                    Some(zone),
                    Some(images),
                );
                let images: Vec<(String, Arc<Vec<u8>>)> =
                    crate::markdownview::image_sources(&blocks)
                        .into_iter()
                        .filter_map(|src| {
                            let path = crate::markdownview::resolve(&src, &f)?;
                            Some((src, read(&path)?))
                        })
                        .collect();
                Some((blocks, images))
            })
            .await;
            let Some(this) = weak.upgrade() else { return };
            this.pending.set(this.pending.get() - 1);
            if this.request.get() != request {
                return;
            }
            let Ok(Some((blocks, images))) = result else {
                this.contents.set_visible_child_name("binary");
                return;
            };
            let images: HashMap<String, gtk::gdk::Texture> = images
                .into_iter()
                .filter_map(|(src, data)| Some((src, crate::imagediff::texture(Some(&data))?)))
                .collect();
            let w = Rc::downgrade(&this);
            let view = crate::markdownview::view(
                &blocks,
                &from,
                &images,
                Rc::new(move |target| {
                    if let Some(this) = w.upgrade() {
                        this.open(&target, None);
                    }
                }),
            );
            this.markdown_holder.append(&view);
        });
    }

    /// `path` のその時点の中身の blob（変えたファイルは変更後、消したファイルは変更前、変えていないファイルは木の
    /// もの）。
    fn blob_id(&self, path: &str) -> Option<Oid> {
        match self.tree.change(path).map(|(_, c)| c) {
            Some(c) if c.kind == ChangeKind::Deleted => c.old_id,
            Some(c) => c.new_id,
            None => self.node_at(path).and_then(|n| n.oid),
        }
    }

    /// 開いているファイルをプレビューで開く（macOS 版の Quick Look）。中身は裏で読む。
    fn preview(self: &Rc<Self>) {
        let Some(path) = self.open_path.borrow().clone() else {
            return;
        };
        let oid = self.blob_id(&path);
        let repo = self.tree.repo.clone();
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let data = gio::spawn_blocking(move || oid.and_then(|oid| repo.blob(&oid).ok()))
                .await
                .ok()
                .flatten();
            let Some(this) = weak.upgrade() else { return };
            if this.open_path.borrow().as_deref() != Some(path.as_str()) {
                return;
            }
            match data {
                Some(data) => crate::preview::show(&this.root, &data, name_of(&path)),
                None => this.root.error_bell(),
            }
        });
    }

    /// `path` の中身を出す（`line` があればその行へ）。変えたファイルは前後をファイル全体にした差分、変えていない
    /// ファイルはその時点の中身。画像（SVG のほか）は絵で出す。
    pub fn open(self: &Rc<Self>, path: &str, line: Option<u32>) {
        self.open_path.replace(Some(path.to_owned()));
        self.clear_image();
        let change = self.tree.change(path).map(|(_, c)| c.clone());
        let deleted = change
            .as_ref()
            .is_some_and(|c| c.kind == ChangeKind::Deleted);
        let folder = parent_of(path);
        let mut title = String::new();
        if !folder.is_empty() {
            title += &format!(
                "<span alpha=\"60%\">{} / </span>",
                glib::markup_escape_text(&folder.replace('/', " / "))
            );
        }
        let hex = self.tree.commit.to_string();
        title += &format!(
            "<b>{}</b><span alpha=\"60%\" font_family=\"monospace\" size=\"small\">  @ {}</span>",
            glib::markup_escape_text(name_of(path)),
            &hex[..8]
        );
        if deleted {
            title += &format!(
                "<span foreground=\"#e01b24\" size=\"small\">  {}</span>",
                glib::markup_escape_text(&tr!("(deleted in this commit)"))
            );
        }
        self.path_label.set_markup(&title);
        // 帯が狭くても、ファイルの名前（と「@ 短い ID」）は読めるだけの幅を取る。足りなければフォルダの側から
        // 省く（macOS 版の truncating head と同じ）。
        self.path_label
            .set_width_chars((name_of(path).chars().count() + 20).min(60) as i32);
        self.change_button.set_visible(change.is_some());
        self.path_bar.set_visible(true);
        self.pages.set_visible_child_name("file");
        let request = self.request.get() + 1;
        self.request.set(request);
        let repo = self.tree.repo.clone();
        self.pending.set(self.pending.get() + 1);
        let weak = Rc::downgrade(self);
        // SVG は、見せ方の切り替えを出す（絵・モバイルなら絵、テキストなら中身）。
        let svg = crate::imagediff::is_svg(path);
        self.svg_switch.set_visible(svg);
        self.syncing.set(true);
        for (view, button) in &self.svg_buttons {
            button.set_active(*view == self.svg_view.get());
        }
        let mobile = svg && self.svg_view.get() == SvgView::Mobile;
        let renderer = self.renderer.get();
        let markdown = amonhen_core::markdown::is_markdown(path);
        self.markdown_switch.set_visible(markdown);
        self.markdown_buttons[usize::from(self.markdown_text.get())].set_active(true);
        self.drawable_switch.set_visible(false);
        self.drawable_buttons[usize::from(self.drawable_text.get())].set_active(true);
        self.syncing.set(false);
        if markdown && !self.markdown_text.get() {
            self.show_markdown(path, request);
            return;
        }
        // .xml は中身の頭を見て、Android の VectorDrawable なら絵にする（ほかの XML はテキスト）。
        if amonhen_core::vectordrawable::is_xml(path) {
            self.show_drawable(path, line, change, request);
            return;
        }
        let picture = if svg {
            self.svg_view.get() != SvgView::Text
        } else {
            crate::imagediff::is_image(path)
        };
        if picture {
            self.contents.set_visible_child_name("image");
            let oid = self.blob_id(path);
            glib::spawn_future_local(async move {
                let data = gio::spawn_blocking(move || {
                    let data = oid.and_then(|oid| repo.blob(&oid).ok());
                    // モバイル向けの確認: 崩れる機能と、その部品での見え方（Android と flutter_svg は本物の
                    // 取り込みを移したもの、ほかは近似）。
                    let mobile = data.as_ref().filter(|_| mobile).map(|d| {
                        (
                            amonhen_core::svglint::lint(d),
                            amonhen_core::svgmobile::preview(d, renderer),
                        )
                    });
                    (data, mobile)
                })
                .await;
                let Some(this) = weak.upgrade() else { return };
                this.pending.set(this.pending.get() - 1);
                if this.request.get() != request {
                    return;
                }
                let (data, mobile) = data.unwrap_or_default();
                let panel = match (data, mobile) {
                    // 元の絵と、その部品での絵を並べ、崩れる機能の一覧を下に（macOS 版と同じ）。
                    (Some(data), Some((issues, preview)))
                        if crate::imagediff::texture(Some(&data)).is_some() =>
                    {
                        let caption = format!(
                            "{}    {}",
                            crate::imagediff::size_caption(&data),
                            preview.caption
                        );
                        Some(crate::imagediff::compare(
                            &data,
                            preview.svg.as_deref().unwrap_or_default(),
                            (tr!("Original"), preview.title.clone()),
                            caption,
                            Some(mobile_notes(&issues, &preview)),
                            Some(this.renderer_switch()),
                        ))
                    }
                    (data, _) => data.and_then(|d| crate::imagediff::single(&d, None)),
                };
                match panel {
                    Some(panel) => this.image_holder.append(&panel),
                    // 読めない形式（拡張子は画像でも）は、バイナリと同じくプレビューに。
                    None => this.contents.set_visible_child_name("binary"),
                }
            });
            return;
        }
        self.show_text(path, line, change, request);
    }

    /// Android の VectorDrawable かを中身の頭で見て、そうなら絵（テキストで見るならテキスト）で、違えばテキストで
    /// 出す。描けないもの（参照した drawable がないなど）もテキストで出す。
    fn show_drawable(
        self: &Rc<Self>,
        path: &str,
        line: Option<u32>,
        change: Option<FileChange>,
        request: u64,
    ) {
        self.contents.set_visible_child_name("image");
        let oid = self.blob_id(path);
        let (repo, commit, p) = (self.tree.repo.clone(), self.tree.commit, path.to_owned());
        let text = self.drawable_text.get();
        // 引けない色（Android のテーマ属性など）は、今の外観の文字の色（図の配色と同じ libadwaita の本文の色）で
        // 描く。色の資源も、暗い外観なら values-night などを先に引く。外観が変わったら描き直す
        let dark = adw::StyleManager::default().is_dark();
        let fallback = if dark { "#ffffff" } else { "#000000" };
        let weak = Rc::downgrade(self);
        let path = path.to_owned();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                let data = oid.and_then(|oid| repo.blob(&oid).ok())?;
                if !amonhen_core::vectordrawable::is_vector_drawable(&p, &data) {
                    return None;
                }
                let picture =
                    (!text).then(|| repo.vector_drawable(&commit, &p, &data, Some(fallback), dark));
                Some((data.len(), picture))
            })
            .await
            .ok()
            .flatten();
            let Some(this) = weak.upgrade() else { return };
            if this.request.get() != request {
                this.pending.set(this.pending.get() - 1);
                return;
            }
            let Some((len, picture)) = result else {
                return this.show_text(&path, line, change, request);
            };
            this.drawable_switch.set_visible(true);
            let picture = match picture {
                Some(Ok(picture)) => picture,
                Some(Err(e)) => {
                    eprintln!("VectorDrawable を描けない: {e}");
                    this.drawable_switch.set_visible(false);
                    return this.show_text(&path, line, change, request);
                }
                None => return this.show_text(&path, line, change, request),
            };
            let size = crate::imagediff::svg_size(&picture.svg);
            let bytes = glib::format_size(len as u64).to_string();
            let caption = match size {
                Some((w, h)) => format!("{w} × {h} dp · {bytes}"),
                None => bytes,
            };
            match crate::imagediff::drawable(picture.svg.as_bytes(), caption, &picture.notes) {
                Some(panel) => {
                    this.pending.set(this.pending.get() - 1);
                    this.image_holder.append(&panel);
                }
                None => {
                    this.drawable_switch.set_visible(false);
                    this.show_text(&path, line, change, request);
                }
            }
        });
    }

    /// `path` の中身をテキストで出す（変えたファイルは前後をファイル全体にした差分、変えていないファイルはその時点
    /// の中身。`line` があればその行へ）。
    fn show_text(
        self: &Rc<Self>,
        path: &str,
        line: Option<u32>,
        change: Option<FileChange>,
        request: u64,
    ) {
        self.contents.set_visible_child_name("text");
        self.content.show_message(&tr!("Loading the diff…"));
        let path = path.to_owned();
        let (repo, commit) = (self.tree.repo.clone(), self.tree.commit);
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let view = gio::spawn_blocking(move || {
                let view = match &change {
                    Some(c) => repo.file_diff_with_context(c, u32::MAX, &FrontendProfile::GTK),
                    None => repo.tree_file(&commit, &path, &FrontendProfile::GTK),
                }
                .map_err(|e| e.to_string())?;
                let binary = view.binary;
                Ok::<_, String>((binary, View::new(view)))
            })
            .await;
            let Some(this) = weak.upgrade() else { return };
            this.pending.set(this.pending.get() - 1);
            if this.request.get() != request {
                return;
            }
            match view {
                Ok(Ok((true, _))) => this.contents.set_visible_child_name("binary"),
                Ok(Ok((false, view))) => {
                    this.content.show_plain(view);
                    if let Some(line) = line {
                        // 大きさが決まってから、その行まで送る。
                        let content = this.content.clone();
                        glib::idle_add_local_once(move || content.scroll_to_line(line));
                    }
                }
                Ok(Err(e)) => {
                    eprintln!("その時点のファイルを読めない: {e}");
                    this.content.show_message(&tr!("Couldn’t load the diff"));
                }
                Err(_) => {}
            }
        });
    }
}

/// 木・リストの行の文字: リストならフォルダを薄く添え、名前（変更を含まないものは薄く、消したものは赤の取り消し線）、
/// 変更ファイルは種類の印、変更を含むフォルダは橙の点。
fn row_markup(node: &Node, flat: bool) -> String {
    let mut out = String::new();
    let folder = parent_of(&node.path);
    if flat && !folder.is_empty() {
        out += &format!(
            "<span alpha=\"45%\">{}/</span>",
            glib::markup_escape_text(folder)
        );
    }
    let name = glib::markup_escape_text(&node.name);
    if node.kind == Some(ChangeKind::Deleted) {
        out += &format!("<span foreground=\"#e01b24\" strikethrough=\"true\">{name}</span>");
    } else if node.contains_changes {
        out += &name;
    } else {
        out += &format!("<span alpha=\"60%\">{name}</span>");
    }
    if let Some(kind) = node.kind {
        let mark = crate::commitfiles::kind_mark(kind);
        out += &format!(
            "  <span foreground=\"{}\" font_family=\"monospace\" weight=\"bold\" size=\"small\">{mark}</span>",
            crate::diff::mark_color(mark)
        );
    } else if node.is_dir && node.contains_changes {
        out += "  <span foreground=\"#e66100\" size=\"x-small\">●</span>";
    }
    out
}
