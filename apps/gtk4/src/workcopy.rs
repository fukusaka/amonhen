//! Working Copy: 左の上にコミットの入力、その下に変更のあるファイルの一覧（チェックで次のコミットに含めるか）。
//! 右に選んだファイルの差分（行を選んで含める・外す・捨てる）。項目と文言は macOS 版の
//! `WorkingCopyViewController` と同じ。
//!
//! チェックは index の状態そのもので、FE だけの選択状態は持たない（ターミナルで `git add` しても揃う）。
//! 差分は、できれば HEAD と作業ツリーの 1 本の差分に行ごとの印を付けて見せ（`working_diff_combined`）、
//! それで表せないファイルは、作業ツリーの側（Unstaged）か stage 済みの側（Staged）の差分を見せる。
//! コンフリクトのファイルは、マーカー付きの中身（`conflict_view`）と、ファイルごとの解決のボタンを出す。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use amonhen_core::diff::{self, DiffView};
use amonhen_core::{Repository, StatusSnapshot, WorkingDiff};
use amonhen_layout::DiffRow;
use amonhen_model::{StatusEntry, StatusKind};
use amonhen_projection::FrontendProfile;
use gtk::prelude::*;
use gtk::{gio, glib};
use serde_json::{Value, json};

use crate::actions::Actions;
use crate::diff::{DiffWidget, View};

type CountCallback = Box<dyn Fn(usize)>;

/// 一覧の見せ方（macOS 版と同じ 3 つ）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layout {
    /// パスの一覧。
    List,
    /// ディレクトリの木。
    Tree,
    /// ファイル名を先に、親のディレクトリを薄く。
    Flat,
}

impl Layout {
    /// 設定に覚える名前（`--layout` と同じ）。
    fn name(self) -> &'static str {
        match self {
            Layout::List => "list",
            Layout::Tree => "tree",
            Layout::Flat => "flat",
        }
    }
}
/// 追加と削除の行数（バイナリなどで数えられなければ None）。
type LineCounts = (Option<u64>, Option<u64>);

/// 裏のスレッドで作った 1 ファイルの差分。行の操作に使う。コンフリクトなら None（行の操作はしない）。
struct Working(Option<WorkingDiff>);

// SAFETY: `WorkingDiff` の表示（`DiffView`）のポインタは、自分が持つバッファの先頭を指すだけ。
unsafe impl Send for Working {}
unsafe impl Sync for Working {}

pub struct WorkingCopy {
    pub root: gtk::Paned,
    inner: Rc<Inner>,
    /// 一覧の見せ方の切り替え（`--layout`）。
    layouts: Vec<(gtk::ToggleButton, Layout)>,
    /// 見せ方を変えたら設定に覚えるか。
    remember: Rc<Cell<bool>>,
}

struct Inner {
    repo: RefCell<Option<Arc<Repository>>>,
    actions: RefCell<Option<Rc<Actions>>>,
    status: RefCell<Option<Arc<StatusSnapshot>>>,
    /// 一覧に出しているファイル（絞り込み済み）。
    entries: RefCell<Vec<StatusEntry>>,
    /// 一覧の行ごとの、`entries` の番号（ディレクトリの行は None）。
    row_entries: RefCell<Vec<Option<usize>>>,
    layout: Cell<Layout>,
    /// 木で畳んでいるディレクトリ。
    collapsed: RefCell<std::collections::HashSet<String>>,
    /// 絞り込む前のすべてのファイル。
    all_entries: RefCell<Vec<StatusEntry>>,
    /// ファイルごとの行数（追加, 削除）。HEAD と作業ツリーの比べ。
    stats: RefCell<HashMap<String, LineCounts>>,
    list: gtk::ListBox,
    all: gtk::CheckButton,
    count: gtk::Label,
    filter: gtk::SearchEntry,
    subject: gtk::Entry,
    body: gtk::TextView,
    amend: gtk::CheckButton,
    commit: gtk::Button,
    message: gtk::Label,
    diff: DiffWidget,
    path: gtk::Label,
    lines: gtk::Button,
    discard: gtk::Button,
    /// 1 本の差分で表せないファイルで、作業ツリーの側と stage 済みの側を切り替える。
    sides: gtk::Box,
    staged_side: gtk::ToggleButton,
    /// コンフリクトのファイルの解決のボタン。
    conflict_buttons: gtk::Box,
    /// 選んでいるファイル。
    selected: RefCell<Option<String>>,
    /// 出している差分。
    current: RefCell<Option<Arc<Working>>>,
    generation: Cell<u64>,
    /// 一覧を作り直している間は、チェックの変化を操作として扱わない。
    rebuilding: Cell<bool>,
    /// 裏で読んでいる数（`--snapshot` はなくなるまで待つ）。
    pending: Rc<Cell<u32>>,
    /// 次に差分を出したとき、この行の印をクリックしたことにする（`--toggle-row`）。
    toggle_once: Cell<Option<u32>>,
    /// 変更のあるファイルの数が変わったときに呼ぶ。
    on_count: RefCell<Option<CountCallback>>,
    /// ファイルの行を右クリックしたときに呼ぶ（パス、Blame できるか、File History を出せるか、行、x、y）。
    on_file_menu: RefCell<Option<FileMenuCallback>>,
}

type FileMenuCallback = Box<dyn Fn(&str, bool, bool, &gtk::Widget, f64, f64)>;

impl WorkingCopy {
    pub fn new(pending: Rc<Cell<u32>>) -> Self {
        // 左の上: コミットの入力。
        let subject = gtk::Entry::builder().placeholder_text("Summary").build();
        let body = gtk::TextView::builder()
            .wrap_mode(gtk::WrapMode::WordChar)
            .top_margin(6)
            .bottom_margin(6)
            .left_margin(6)
            .right_margin(6)
            .build();
        let body_scrolled = gtk::ScrolledWindow::builder()
            .child(&body)
            .min_content_height(80)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .has_frame(true)
            .build();
        let amend = gtk::CheckButton::with_label("Amend");
        let commit = gtk::Button::with_label("Commit");
        commit.add_css_class("suggested-action");
        commit.set_tooltip_text(Some("チェックしたファイルをコミット（Ctrl+Return）"));
        let commit_menu = gtk::MenuButton::builder()
            .tooltip_text("Commit and Push（Ctrl+Shift+Return）、Amend")
            .build();
        commit_menu.add_css_class("suggested-action");
        let menu = gio::Menu::new();
        menu.append(Some("Commit and Push"), Some("win.commit-and-push"));
        menu.append(Some("Amend Last Commit"), Some("win.amend-last-commit"));
        commit_menu.set_menu_model(Some(&menu));
        let commit_group = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        commit_group.add_css_class("linked");
        commit_group.append(&commit);
        commit_group.append(&commit_menu);
        commit_group.set_hexpand(true);
        commit_group.set_halign(gtk::Align::End);
        let commit_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        commit_row.append(&amend);
        commit_row.append(&commit_group);
        let message = gtk::Label::new(None);
        message.set_xalign(0.0);
        message.set_wrap(true);
        message.add_css_class("error");
        message.set_visible(false);

        // 左の下: 変更のあるファイルの一覧。
        let all = gtk::CheckButton::with_label("Changes");
        all.set_tooltip_text(Some("一覧のファイルをすべて次のコミットに含める・外す"));
        let count = gtk::Label::new(None);
        count.set_hexpand(true);
        count.set_xalign(1.0);
        count.add_css_class("dim-label");
        count.add_css_class("numeric");
        count.set_tooltip_text(Some("次のコミットに含めるファイル / 変更のあるファイル"));
        let list_header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        list_header.append(&all);
        list_header.append(&count);
        // 一覧の見せ方の切り替え。
        let layouts = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        layouts.add_css_class("linked");
        let mut layout_buttons: Vec<(gtk::ToggleButton, Layout)> = Vec::new();
        for (kind, tip, layout) in [
            (
                crate::icon::Kind::List,
                "ファイルをパスの一覧で表示",
                Layout::List,
            ),
            (
                crate::icon::Kind::Tree,
                "ファイルをディレクトリの木で表示",
                Layout::Tree,
            ),
            (
                crate::icon::Kind::Doc,
                "ファイル名を先に、親のディレクトリを薄く表示",
                Layout::Flat,
            ),
        ] {
            let icon = crate::icon::Icon::default();
            icon.set(kind, false);
            let button = gtk::ToggleButton::new();
            button.set_child(Some(&icon));
            button.set_tooltip_text(Some(tip));
            if let Some(first) = layout_buttons.first() {
                button.set_group(Some(&first.0));
            }
            layouts.append(&button);
            layout_buttons.push((button, layout));
        }
        layout_buttons[0].0.set_active(true);
        list_header.append(&layouts);
        let filter = gtk::SearchEntry::builder()
            .placeholder_text("パスで絞り込む")
            .build();
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.add_css_class("navigation-sidebar");
        let list_scrolled = gtk::ScrolledWindow::builder()
            .child(&list)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();

        let left = gtk::Box::new(gtk::Orientation::Vertical, 8);
        left.set_margin_top(10);
        left.set_margin_start(10);
        left.set_margin_end(10);
        left.append(&subject);
        left.append(&body_scrolled);
        left.append(&commit_row);
        left.append(&message);
        left.append(&list_header);
        left.append(&filter);
        left.append(&list_scrolled);

        // 右: 操作のボタンと差分。
        let path = gtk::Label::new(None);
        path.set_xalign(0.0);
        path.set_hexpand(true);
        path.set_ellipsize(gtk::pango::EllipsizeMode::Start);
        path.add_css_class("heading");
        let lines = gtk::Button::with_label("Stage Lines");
        let discard = gtk::Button::with_label("Discard File");
        discard.set_tooltip_text(Some("作業ツリーの変更を捨てる（index の内容に戻す）"));
        let worktree_side = gtk::ToggleButton::with_label("Unstaged");
        worktree_side.set_tooltip_text(Some("stage していない（作業ツリーの）変更"));
        let staged_side = gtk::ToggleButton::with_label("Staged");
        staged_side.set_tooltip_text(Some("stage 済み（次のコミットに含める）の変更"));
        staged_side.set_group(Some(&worktree_side));
        worktree_side.set_active(true);
        let sides = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        sides.add_css_class("linked");
        sides.append(&worktree_side);
        sides.append(&staged_side);
        sides.set_visible(false);
        let conflict_buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        conflict_buttons.set_visible(false);
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        bar.set_margin_top(6);
        bar.set_margin_bottom(6);
        bar.set_margin_start(10);
        bar.set_margin_end(10);
        bar.append(&path);
        bar.append(&sides);
        bar.append(&conflict_buttons);
        bar.append(&lines);
        bar.append(&discard);
        let diff = DiffWidget::default();
        let diff_scrolled = gtk::ScrolledWindow::builder()
            .child(&diff)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let right = gtk::Box::new(gtk::Orientation::Vertical, 0);
        right.append(&bar);
        right.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        right.append(&diff_scrolled);

        let root = gtk::Paned::new(gtk::Orientation::Horizontal);
        root.set_start_child(Some(&left));
        root.set_end_child(Some(&right));
        root.set_resize_start_child(false);
        root.set_position(340);

        let inner = Rc::new(Inner {
            repo: RefCell::default(),
            actions: RefCell::default(),
            status: RefCell::default(),
            entries: RefCell::default(),
            row_entries: RefCell::default(),
            layout: Cell::new(Layout::List),
            collapsed: RefCell::default(),
            all_entries: RefCell::default(),
            stats: RefCell::default(),
            list,
            all,
            count,
            filter,
            subject,
            body,
            amend,
            commit,
            message,
            diff,
            path,
            lines,
            discard,
            sides,
            staged_side,
            conflict_buttons,
            selected: RefCell::default(),
            current: RefCell::default(),
            generation: Cell::new(0),
            rebuilding: Cell::new(false),
            pending,
            toggle_once: Cell::new(None),
            on_count: RefCell::default(),
            on_file_menu: RefCell::default(),
        });
        Inner::connect(&inner);
        let layouts = layout_buttons.clone();
        let remember = Rc::new(Cell::new(true));
        for (button, layout) in layout_buttons {
            let weak = Rc::downgrade(&inner);
            let remember = remember.clone();
            button.connect_toggled(move |b| {
                if let Some(this) = weak.upgrade()
                    && b.is_active()
                {
                    this.layout.set(layout);
                    Inner::rebuild_list(&this);
                    // 次に開く窓も同じ見せ方にする（`--layout` では覚えない）。
                    if remember.get() {
                        crate::settings::set_string(
                            crate::settings::WORKING_COPY_LAYOUT,
                            layout.name(),
                        );
                    }
                }
            });
        }
        // 前に選んだ見せ方。
        let saved = crate::settings::string(crate::settings::WORKING_COPY_LAYOUT);
        if let Some((button, _)) = layouts
            .iter()
            .find(|(_, l)| Some(l.name()) == saved.as_deref())
        {
            remember.set(false);
            button.set_active(true);
            remember.set(true);
        }
        inner.update_buttons();
        Self {
            root,
            inner,
            layouts,
            remember,
        }
    }

    pub fn set_repo(&self, repo: Arc<Repository>) {
        self.inner.repo.replace(Some(repo));
    }

    /// 確かめる窓や、知らせに使う。
    pub fn set_actions(&self, actions: Rc<Actions>) {
        self.inner.actions.replace(Some(actions));
    }

    /// 新しい status を出す。同じスナップショットなら何もしない。
    pub fn show(&self, status: Arc<StatusSnapshot>) {
        Inner::show(&self.inner, status);
    }

    /// 次に差分を出したとき、`row` 行の印をクリックしたことにする（確認用）。
    pub fn toggle_row_once(&self, row: u32) {
        self.inner.toggle_once.set(Some(row));
    }

    /// 一覧の見せ方を `list`・`tree`・`flat` にする。
    pub fn set_layout(&self, name: &str) {
        let wanted = match name {
            "tree" => Layout::Tree,
            "flat" => Layout::Flat,
            _ => Layout::List,
        };
        if let Some((button, _)) = self.layouts.iter().find(|(_, l)| *l == wanted) {
            self.remember.set(false);
            button.set_active(true);
            self.remember.set(true);
        }
    }

    /// 次に status が届いたとき、`path` を選ぶ。
    pub fn select_path(&self, path: &str) {
        self.inner.selected.replace(Some(path.to_owned()));
    }

    /// Commit メニューの Commit（コミットできるときだけ）。`then` はコミットできた後に呼ぶ。
    pub fn commit_from_menu(&self, then: Option<Box<dyn FnOnce()>>) {
        if self.inner.commit.is_sensitive() {
            Inner::commit(&self.inner, then);
        }
    }

    /// Amend Last Commit: 直前のコミットを直す（メッセージを入れておく）。
    pub fn amend_last_commit(&self) {
        self.inner.amend.set_active(true);
    }

    /// Commit メニューの Stage All（コンフリクト以外の、stage していない変更をすべて含める）。
    pub fn stage_all(&self) {
        let paths: Vec<String> = self
            .inner
            .all_entries
            .borrow()
            .iter()
            .filter(|e| e.kind != StatusKind::Unmerged && e.is_unstaged())
            .map(|e| e.path.clone())
            .collect();
        if paths.is_empty() {
            return;
        }
        Inner::run(&self.inner, move |repo| {
            let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
            repo.stage(&paths)
        });
    }

    /// ファイルの行を右クリックしたら `f(パス, Blame できるか, File History を出せるか, 行, x, y)` を呼ぶ。
    pub fn connect_file_menu(
        &self,
        f: impl Fn(&str, bool, bool, &gtk::Widget, f64, f64) + 'static,
    ) {
        self.inner.on_file_menu.replace(Some(Box::new(f)));
    }

    pub fn connect_count(&self, f: impl Fn(usize) + 'static) {
        self.inner.on_count.replace(Some(Box::new(f)));
    }
}

impl Inner {
    fn connect(this: &Rc<Self>) {
        let weak = Rc::downgrade(this);
        this.list.connect_row_selected(move |_, row| {
            let (Some(this), Some(row)) = (weak.upgrade(), row) else {
                return;
            };
            if this.rebuilding.get() {
                return;
            }
            let index = this
                .row_entries
                .borrow()
                .get(row.index() as usize)
                .copied()
                .flatten();
            let entry = index.and_then(|i| this.entries.borrow().get(i).cloned());
            if let Some(entry) = entry {
                let same = this.selected.borrow().as_deref() == Some(entry.path.as_str());
                this.selected.replace(Some(entry.path.clone()));
                Inner::load_diff(&this, entry, same);
            }
        });
        let weak = Rc::downgrade(this);
        this.all.connect_toggled(move |all| {
            let Some(this) = weak.upgrade() else { return };
            if this.rebuilding.get() {
                return;
            }
            let paths: Vec<String> = this
                .entries
                .borrow()
                .iter()
                .filter(|e| e.kind != StatusKind::Unmerged)
                .map(|e| e.path.clone())
                .collect();
            let stage = all.is_active();
            Inner::run(&this, move |repo| {
                let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
                if stage {
                    repo.stage(&paths)
                } else {
                    repo.unstage(&paths)
                }
            });
        });
        let weak = Rc::downgrade(this);
        this.filter.connect_search_changed(move |_| {
            if let Some(this) = weak.upgrade() {
                Inner::rebuild_list(&this);
            }
        });
        let weak = Rc::downgrade(this);
        this.diff.connect_mark(move |row| {
            let Some(this) = weak.upgrade() else { return };
            Inner::set_rows(&this, vec![row as usize], None);
        });
        let weak = Rc::downgrade(this);
        this.diff.connect_selection(move || {
            if let Some(this) = weak.upgrade() {
                this.update_buttons();
            }
        });
        let weak = Rc::downgrade(this);
        this.lines.connect_clicked(move |_| {
            let Some(this) = weak.upgrade() else { return };
            // 1 本の差分では、選んだ行がすべて含まれていれば外す、そうでなければ含める。
            let rows = this.diff.selected_rows();
            let (staged, total) = this.diff.selected_staged();
            Inner::set_rows(&this, rows, Some(staged < total));
        });
        let weak = Rc::downgrade(this);
        this.discard.connect_clicked(move |_| {
            if let Some(this) = weak.upgrade() {
                Inner::discard(&this);
            }
        });
        let weak = Rc::downgrade(this);
        this.staged_side.connect_toggled(move |_| {
            let Some(this) = weak.upgrade() else { return };
            let entry = this.selected_entry();
            if let Some(entry) = entry {
                Inner::load_diff(&this, entry, false);
            }
        });
        for (title, ours) in [("Take Current File", true), ("Take Incoming File", false)] {
            let button = gtk::Button::with_label(title);
            let weak = Rc::downgrade(this);
            button.connect_clicked(move |_| {
                let Some(this) = weak.upgrade() else { return };
                let (Some(path), Some(actions)) = (
                    this.selected.borrow().clone(),
                    this.actions.borrow().clone(),
                ) else {
                    return;
                };
                actions.perform(json!({ "op": "takeSide", "path": path, "ours": ours }));
            });
            this.conflict_buttons.append(&button);
        }
        let resolved = gtk::Button::with_label("Mark Resolved");
        let weak = Rc::downgrade(this);
        resolved.connect_clicked(move |_| {
            if let Some(this) = weak.upgrade() {
                Inner::mark_resolved(&this);
            }
        });
        this.conflict_buttons.append(&resolved);
        let weak = Rc::downgrade(this);
        this.subject.connect_changed(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update_buttons();
            }
        });
        let weak = Rc::downgrade(this);
        this.amend.connect_toggled(move |amend| {
            let Some(this) = weak.upgrade() else { return };
            if amend.is_active() && this.subject.text().is_empty() {
                Inner::fill_head_message(&this);
            }
            this.update_buttons();
        });
        let weak = Rc::downgrade(this);
        this.commit.connect_clicked(move |_| {
            if let Some(this) = weak.upgrade() {
                Inner::commit(&this, None);
            }
        });
        let weak = Rc::downgrade(this);
        this.subject.connect_activate(move |_| {
            if let Some(this) = weak.upgrade()
                && this.commit.is_sensitive()
            {
                Inner::commit(&this, None);
            }
        });
    }

    fn repo(&self) -> Option<Arc<Repository>> {
        self.repo.borrow().clone()
    }

    fn show(this: &Rc<Self>, status: Arc<StatusSnapshot>) {
        if this
            .status
            .borrow()
            .as_ref()
            .is_some_and(|s| Arc::ptr_eq(s, &status))
        {
            return;
        }
        // コンフリクトを先に、あとはパスの順。
        let mut entries = status.status.entries.clone();
        entries.sort_by(|a, b| {
            (a.kind != StatusKind::Unmerged, &a.path)
                .cmp(&(b.kind != StatusKind::Unmerged, &b.path))
        });
        this.status.replace(Some(status));
        let count = entries.len();
        this.all_entries.replace(entries);
        Inner::rebuild_list(this);
        Inner::load_stats(this);
        if let Some(f) = this.on_count.borrow().as_ref() {
            f(count);
        }
    }

    /// 絞り込みに合うファイルで一覧を作り直し、選んでいたファイルを選び直す（なければ最初のもの）。
    fn rebuild_list(this: &Rc<Self>) {
        let filter = this.filter.text().to_lowercase();
        let entries: Vec<StatusEntry> = this
            .all_entries
            .borrow()
            .iter()
            .filter(|e| filter.is_empty() || e.path.to_lowercase().contains(&filter))
            .cloned()
            .collect();
        this.rebuilding.set(true);
        this.list.remove_all();
        let mut row_entries = Vec::new();
        match this.layout.get() {
            Layout::Tree => {
                for item in tree(&entries) {
                    match item {
                        TreeItem::Dir { name, path, depth } => {
                            let collapsed = this.collapsed.borrow().contains(&path);
                            this.list
                                .append(&Inner::dir_row(this, &name, &path, depth, collapsed));
                            row_entries.push(None);
                        }
                        TreeItem::File { index, depth } => {
                            if this.is_hidden(&entries[index].path) {
                                continue;
                            }
                            this.list.append(&Inner::row(
                                this,
                                &entries[index],
                                Layout::Tree,
                                depth,
                            ));
                            row_entries.push(Some(index));
                        }
                    }
                }
            }
            layout => {
                for (i, entry) in entries.iter().enumerate() {
                    this.list.append(&Inner::row(this, entry, layout, 0));
                    row_entries.push(Some(i));
                }
            }
        }
        this.row_entries.replace(row_entries);
        let all = this.all_entries.borrow();
        let staged = all.iter().filter(|e| e.is_staged()).count();
        let partial = all.iter().any(|e| e.is_staged() && e.is_unstaged());
        this.all
            .set_inconsistent(partial || (staged > 0 && staged < all.len()));
        this.all
            .set_active(!all.is_empty() && staged == all.len() && !partial);
        this.all.set_sensitive(!all.is_empty());
        this.count.set_text(&format!("{staged} / {}", all.len()));
        drop(all);
        let wanted = this.selected.borrow().clone();
        let index = wanted
            .and_then(|p| entries.iter().position(|e| e.path == p))
            .unwrap_or(0);
        this.entries.replace(entries);
        // そのファイルの行（畳んだディレクトリの中なら、最初のファイルの行）。
        let row_of = |i: usize| this.row_entries.borrow().iter().position(|&r| r == Some(i));
        let row_index =
            row_of(index).or_else(|| this.row_entries.borrow().iter().position(Option::is_some));
        let index = row_index
            .and_then(|r| this.row_entries.borrow()[r])
            .unwrap_or(index);
        if let Some(row) = row_index.and_then(|r| this.list.row_at_index(r as i32)) {
            this.list.select_row(Some(&row));
            this.rebuilding.set(false);
            // 選び直しでは `row-selected` を扱わないので、差分は自分で読み直す。
            let entry = this.entries.borrow()[index].clone();
            let same = this.selected.borrow().as_deref() == Some(entry.path.as_str());
            this.selected.replace(Some(entry.path.clone()));
            Inner::load_diff(this, entry, same);
        } else {
            this.rebuilding.set(false);
            this.selected.replace(None);
            this.current.replace(None);
            this.path.set_text("");
            this.diff.clear();
        }
        this.update_buttons();
    }

    /// ファイルごとの行数を裏で読み、一覧に出す（数がなくても一覧は先に出す）。
    fn load_stats(this: &Rc<Self>) {
        let Some(repo) = this.repo() else { return };
        let weak = Rc::downgrade(this);
        glib::spawn_future_local(async move {
            let reply: Value = gio::spawn_blocking(move || {
                serde_json::from_str(&repo.run_json(r#"{"op":"workingStats"}"#)).unwrap_or_default()
            })
            .await
            .unwrap_or_default();
            let Some(this) = weak.upgrade() else { return };
            let stats = reply["info"]["stats"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|s| {
                    Some((
                        s["path"].as_str()?.to_owned(),
                        (s["added"].as_u64(), s["removed"].as_u64()),
                    ))
                })
                .collect();
            this.stats.replace(stats);
            Inner::rebuild_list(&this);
        });
    }

    /// 木で、畳んだディレクトリの中のファイルか。
    fn is_hidden(&self, path: &str) -> bool {
        let collapsed = self.collapsed.borrow();
        collapsed
            .iter()
            .any(|dir| path.starts_with(dir.as_str()) && path[dir.len()..].starts_with('/'))
    }

    /// 木のディレクトリの行（クリックで畳む・開く）。
    fn dir_row(
        this: &Rc<Self>,
        name: &str,
        path: &str,
        depth: usize,
        collapsed: bool,
    ) -> gtk::ListBoxRow {
        let arrow = gtk::Label::new(Some(if collapsed { "▸" } else { "▾" }));
        arrow.add_css_class("dim-label");
        let icon = crate::icon::Icon::default();
        icon.set(crate::icon::Kind::Folder, false);
        let label = gtk::Label::new(Some(name));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_ellipsize(gtk::pango::EllipsizeMode::Start);
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        line.set_margin_start((depth * 16) as i32);
        line.append(&arrow);
        line.append(&icon);
        line.append(&label);
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&line));
        row.set_selectable(false);
        let click = gtk::GestureClick::new();
        let (weak, path) = (Rc::downgrade(this), path.to_owned());
        click.connect_released(move |_, _, _, _| {
            let Some(this) = weak.upgrade() else { return };
            {
                let mut collapsed = this.collapsed.borrow_mut();
                if !collapsed.remove(&path) {
                    collapsed.insert(path.clone());
                }
            }
            Inner::rebuild_list(&this);
        });
        row.add_controller(click);
        row
    }

    /// 一覧の 1 行: チェック（次のコミットに含めるか）、状態の文字、パス、行数、変更を捨てるボタン。
    /// 木ならファイル名を `depth` 段下げ、ファイル名を先にするならパスの親を薄く添える。
    fn row(this: &Rc<Self>, entry: &StatusEntry, layout: Layout, depth: usize) -> gtk::ListBoxRow {
        let conflict = entry.kind == StatusKind::Unmerged;
        let check = gtk::CheckButton::new();
        check.set_active(entry.is_staged() && !entry.is_unstaged());
        check.set_inconsistent(entry.is_staged() && entry.is_unstaged());
        check.set_sensitive(!conflict);
        let path = entry.path.clone();
        let fully = entry.is_staged() && !entry.is_unstaged();
        let weak = Rc::downgrade(this);
        check.connect_toggled(move |_| {
            let Some(this) = weak.upgrade() else { return };
            if this.rebuilding.get() {
                return;
            }
            let path = path.clone();
            Inner::run(&this, move |repo| {
                if fully {
                    repo.unstage(&[&path])
                } else {
                    repo.stage(&[&path])
                }
            });
        });
        let (letter, css) = state_letter(entry);
        let state = gtk::Label::new(Some(letter));
        state.set_width_chars(2);
        state.add_css_class(css);
        state.add_css_class("monospace");
        let (parent, file) = match entry.path.rsplit_once('/') {
            Some((p, f)) => (p, f),
            None => ("", entry.path.as_str()),
        };
        let shown = match (layout, &entry.orig_path) {
            (Layout::List, Some(orig)) => format!("{orig} → {}", entry.path),
            (Layout::List, None) => entry.path.clone(),
            _ => file.to_owned(),
        };
        let name = gtk::Label::new(Some(&shown));
        name.set_xalign(0.0);
        name.set_ellipsize(gtk::pango::EllipsizeMode::Start);
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        line.set_margin_start((depth * 16) as i32);
        line.append(&check);
        line.append(&state);
        line.append(&name);
        if layout == Layout::Flat && !parent.is_empty() {
            let dir = gtk::Label::new(Some(parent));
            dir.set_xalign(0.0);
            dir.set_ellipsize(gtk::pango::EllipsizeMode::Start);
            dir.add_css_class("dim-label");
            dir.add_css_class("caption");
            dir.set_hexpand(true);
            line.append(&dir);
        } else {
            name.set_hexpand(true);
        }
        // 行数（+ 追加 − 削除）。
        if let Some(&(added, removed)) = this.stats.borrow().get(&entry.path) {
            let counts = gtk::Box::new(gtk::Orientation::Horizontal, 4);
            for (n, mark, css) in [(added, "+", "success"), (removed, "−", "error")] {
                if let Some(n) = n.filter(|&n| n > 0) {
                    let l = gtk::Label::new(Some(&format!("{mark}{n}")));
                    l.add_css_class("caption");
                    l.add_css_class("numeric");
                    l.add_css_class(css);
                    counts.append(&l);
                }
            }
            line.append(&counts);
        }
        // 変更を捨てるボタン（マウスを載せたときだけ出す。コンフリクトと、stage 済みだけのファイルにはない）。
        if !conflict && entry.is_unstaged() {
            let discard = gtk::Button::new();
            let icon = crate::icon::Icon::default();
            icon.set(crate::icon::Kind::Undo, false);
            discard.set_child(Some(&icon));
            discard.add_css_class("flat");
            discard.set_tooltip_text(Some("作業ツリーの変更を捨てる（index の内容に戻す）"));
            discard.set_opacity(0.0);
            let (weak, e) = (Rc::downgrade(this), entry.clone());
            discard.connect_clicked(move |_| {
                if let Some(this) = weak.upgrade() {
                    Inner::discard_file(&this, &e);
                }
            });
            let motion = gtk::EventControllerMotion::new();
            let d = discard.clone();
            motion.connect_enter(move |_, _, _| d.set_opacity(1.0));
            let d = discard.clone();
            motion.connect_leave(move |_| d.set_opacity(0.0));
            line.add_controller(motion);
            line.append(&discard);
        }
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&line));
        // 右クリック: Blame（消したファイルにはない）と File History（untracked にはまだ履歴がない）。
        let menu = gtk::GestureClick::new();
        menu.set_button(gtk::gdk::BUTTON_SECONDARY);
        let (weak, path, r) = (Rc::downgrade(this), entry.path.clone(), row.downgrade());
        let untracked = entry.kind == StatusKind::Untracked;
        let deleted = entry.worktree == b'D';
        menu.connect_pressed(move |_, _, x, y| {
            let (Some(this), Some(row)) = (weak.upgrade(), r.upgrade()) else {
                return;
            };
            if let Some(f) = this.on_file_menu.borrow().as_ref() {
                f(
                    &path,
                    !untracked && !deleted,
                    !untracked,
                    row.upcast_ref(),
                    x,
                    y,
                );
            }
        });
        row.add_controller(menu);
        row
    }

    fn selected_entry(&self) -> Option<StatusEntry> {
        let selected = self.selected.borrow();
        let path = selected.as_deref()?;
        self.all_entries
            .borrow()
            .iter()
            .find(|e| e.path == path)
            .cloned()
    }

    /// 1 ファイルの差分を裏で読んで出す。`same` なら同じファイルの読み直し（位置と選んだ行を保つ）。
    fn load_diff(this: &Rc<Self>, entry: StatusEntry, same: bool) {
        let Some(repo) = this.repo() else { return };
        let prefer_staged = this.staged_side.is_active();
        let generation = this.generation.get() + 1;
        this.generation.set(generation);
        this.path.set_text(&entry.path);
        this.pending.set(this.pending.get() + 1);
        let conflict = entry.kind == StatusKind::Unmerged;
        let weak = Rc::downgrade(this);
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                let profile = FrontendProfile::GTK;
                if conflict {
                    // コンフリクト: マーカー付きの中身（Current・Incoming・Base の行に分ける）。
                    let view = repo.conflict_view(&entry.path, &profile)?;
                    return Ok::<_, amonhen_model::Error>((
                        Arc::new(Working(None)),
                        View::new(view),
                    ));
                }
                // 1 本の差分で表せなければ、選んだ側（片側にしか変更がなければその側）。
                let mut working = match repo.working_diff_combined(&entry, &profile)? {
                    Some(d) => d,
                    None => {
                        let staged = if entry.is_staged() && entry.is_unstaged() {
                            prefer_staged
                        } else {
                            !entry.is_unstaged()
                        };
                        repo.working_diff(&entry, staged, &profile)?
                    }
                };
                let empty =
                    diff::file_diff(Arc::<Vec<u8>>::default(), Arc::<Vec<u8>>::default(), 0);
                let mut view =
                    std::mem::replace(&mut working.view, DiffView::new(&empty, &profile));
                // stage 済みの側の変更は、どれも index に入っている。
                if working.staged {
                    for row in &mut view.rows {
                        if matches!(row.kind, DiffRow::ADDED | DiffRow::REMOVED) {
                            row.flags |= DiffRow::STAGED;
                        }
                    }
                }
                Ok((Arc::new(Working(Some(working))), View::new(view)))
            })
            .await;
            let Some(this) = weak.upgrade() else { return };
            this.pending.set(this.pending.get() - 1);
            if this.generation.get() != generation {
                return;
            }
            match result {
                Ok(Ok((working, view))) => {
                    let w = working.0.as_ref();
                    this.sides.set_visible(w.is_some_and(|w| {
                        w.combined.is_none() && w.entry.is_staged() && w.entry.is_unstaged()
                    }));
                    this.conflict_buttons.set_visible(w.is_none());
                    let marks = w.is_some();
                    this.current.replace(Some(working));
                    if marks {
                        this.diff.show_working(view, same);
                    } else {
                        this.diff.show_plain(view);
                    }
                    if let Some(row) = this.toggle_once.take() {
                        Inner::set_rows(&this, vec![row as usize], None);
                    }
                }
                Ok(Err(e)) => this.show_error(&format!("差分を読めない: {e}")),
                Err(_) => {}
            }
            this.update_buttons();
        });
    }

    /// 選んだ行（`rows`）を含める・外す。`stage` が None なら、1 行の今の状態を反対にする。
    fn set_rows(this: &Rc<Self>, rows: Vec<usize>, stage: Option<bool>) {
        let Some(current) = this.current.borrow().clone() else {
            return;
        };
        if rows.is_empty() || current.0.is_none() {
            return;
        }
        Inner::run(this, move |repo| {
            let w = current.0.as_ref().expect("working diff");
            match &w.combined {
                Some(staged) => {
                    let stage = stage.unwrap_or(!staged[rows[0]]);
                    repo.set_rows_staged(w, &rows, stage)
                }
                // 片側だけの差分: 作業ツリーの側なら含める、stage 済みの側なら外す。
                None => repo.stage_rows(w, &rows),
            }
        });
    }

    /// Discard Lines・Discard File（macOS 版と同じ）: 選んだ行があれば行を、なければファイルを捨てる。
    /// untracked のファイルは消す（Undo では戻せないので確かめる）。
    fn discard(this: &Rc<Self>) {
        let Some(current) = this.current.borrow().clone() else {
            return;
        };
        let Some(w) = current.0.as_ref() else { return };
        if w.staged {
            return;
        }
        let rows = this.diff.selected_rows();
        if !rows.is_empty() {
            Inner::run(this, move |repo| {
                repo.discard_rows(current.0.as_ref().expect("working diff"), &rows)
            });
            return;
        }
        let entry = w.entry.clone();
        let (Some(actions), path) = (this.actions.borrow().clone(), entry.path.clone()) else {
            return;
        };
        if entry.kind == StatusKind::Untracked {
            glib::spawn_future_local(async move {
                if actions
                    .confirm(
                        &format!("{path} を削除しますか？"),
                        "untracked のファイルなので、Undo では戻せません。",
                        "削除",
                    )
                    .await
                {
                    actions.perform(json!({ "op": "deleteUntracked", "paths": [path] }));
                }
            });
        } else {
            actions.perform(json!({ "op": "discard", "paths": [path] }));
        }
    }

    /// 一覧の行のボタン: 1 ファイルの変更を捨てる（確かめてから）。作業ツリーを index の内容に戻すので、
    /// 含めている分は残る。untracked のファイルは消す。
    fn discard_file(this: &Rc<Self>, entry: &StatusEntry) {
        let (Some(actions), path) = (this.actions.borrow().clone(), entry.path.clone()) else {
            return;
        };
        let untracked = entry.kind == StatusKind::Untracked;
        glib::spawn_future_local(async move {
            let (title, message, ok, command) = if untracked {
                (
                    format!("{path} を削除しますか？"),
                    "untracked のファイルなので、Undo では戻せません。",
                    "削除",
                    json!({ "op": "deleteUntracked", "paths": [path] }),
                )
            } else {
                (
                    format!("{path} の変更を捨てますか？"),
                    "作業ツリーの変更を捨て、次のコミットに含める内容（index）に戻します。Undo で戻せます。",
                    "捨てる",
                    json!({ "op": "discard", "paths": [path] }),
                )
            };
            if actions.confirm(&title, message, ok).await {
                actions.perform(command);
            }
        });
    }

    /// Mark Resolved: 残っているコンフリクトがあれば確かめてから、解決済みにする（index に入れる）。
    fn mark_resolved(this: &Rc<Self>) {
        let (Some(path), Some(actions), Some(repo)) = (
            this.selected.borrow().clone(),
            this.actions.borrow().clone(),
            this.repo(),
        ) else {
            return;
        };
        let weak = Rc::downgrade(this);
        glib::spawn_future_local(async move {
            let p = path.clone();
            let reply: Value = gio::spawn_blocking(move || {
                let command = json!({ "op": "conflictCount", "path": p });
                serde_json::from_str(&repo.run_json(&command.to_string())).unwrap_or_default()
            })
            .await
            .unwrap_or_default();
            let remaining = reply["info"]["count"].as_u64().unwrap_or(0);
            if remaining > 0
                && !actions
                    .confirm(
                        &format!("{path} にコンフリクトが {remaining} 個残っています"),
                        "このまま解決済みにしますか？",
                        "解決済みにする",
                    )
                    .await
            {
                return;
            }
            if let Some(this) = weak.upgrade() {
                Inner::run(&this, move |repo| repo.stage(&[&path]));
            }
        });
    }

    /// `op` を裏で行い、status を読み直す。
    fn run(
        this: &Rc<Self>,
        op: impl FnOnce(&Repository) -> amonhen_model::Result<()> + Send + 'static,
    ) {
        let Some(repo) = this.repo() else { return };
        this.message.set_visible(false);
        this.pending.set(this.pending.get() + 1);
        let weak = Rc::downgrade(this);
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                op(&repo)
                    .map_err(|e| e.to_string())
                    .and_then(|()| repo.load_status().map_err(|e| e.to_string()))
            })
            .await;
            let Some(this) = weak.upgrade() else { return };
            this.pending.set(this.pending.get() - 1);
            match result {
                Ok(Ok(status)) => Inner::show(&this, status),
                Ok(Err(e)) => this.show_error(&e),
                Err(_) => {}
            }
        });
    }

    /// コミットする。できたら `then` を呼ぶ（Commit and Push の push）。
    fn commit(this: &Rc<Self>, then: Option<Box<dyn FnOnce()>>) {
        let subject = this.subject.text().trim().to_owned();
        let buffer = this.body.buffer();
        let body = buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .trim()
            .to_owned();
        let message = if body.is_empty() {
            subject
        } else {
            format!("{subject}\n\n{body}")
        };
        let amend = this.amend.is_active();
        let weak = Rc::downgrade(this);
        this.commit.set_sensitive(false);
        let Some(repo) = this.repo() else { return };
        this.message.set_visible(false);
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                repo.commit(&message, amend)
                    .map_err(|e| e.to_string())
                    .and_then(|()| repo.load_status().map_err(|e| e.to_string()))
            })
            .await;
            let Some(this) = weak.upgrade() else { return };
            match result {
                Ok(Ok(status)) => {
                    this.subject.set_text("");
                    this.body.buffer().set_text("");
                    this.amend.set_active(false);
                    Inner::show(&this, status);
                    if let Some(then) = then {
                        then();
                    }
                }
                Ok(Err(e)) => this.show_error(&e),
                Err(_) => {}
            }
            this.update_buttons();
        });
    }

    /// 直前のコミットを直すとき、そのメッセージを入れておく（1 行目を Summary、空行の後を本文に）。
    fn fill_head_message(this: &Rc<Self>) {
        let Some(repo) = this.repo() else { return };
        let weak = Rc::downgrade(this);
        glib::spawn_future_local(async move {
            let reply: Value = gio::spawn_blocking(move || {
                serde_json::from_str(&repo.run_json(r#"{"op":"commitMessage","oid":"HEAD"}"#))
                    .unwrap_or_default()
            })
            .await
            .unwrap_or_default();
            let Some(this) = weak.upgrade() else { return };
            if !this.subject.text().is_empty() || !this.amend.is_active() {
                return;
            }
            let message = reply["text"].as_str().unwrap_or_default().trim();
            let (subject, body) = message.split_once('\n').unwrap_or((message, ""));
            this.subject.set_text(subject.trim());
            this.body.buffer().set_text(body.trim());
            this.update_buttons();
        });
    }

    fn show_error(&self, e: &str) {
        self.message.set_text(e);
        self.message.set_visible(true);
    }

    fn update_buttons(&self) {
        let staged = self.all_entries.borrow().iter().any(StatusEntry::is_staged);
        let has_subject = !self.subject.text().trim().is_empty();
        let amend = self.amend.is_active();
        self.commit.set_sensitive(has_subject && (staged || amend));
        self.commit
            .set_label(if amend { "Amend Commit" } else { "Commit" });

        let current = self.current.borrow();
        let working = current.as_ref().and_then(|c| c.0.as_ref());
        let conflict = current.is_some() && working.is_none();
        let (selected_staged, selected) = self.diff.selected_staged();
        let staged_side = working.is_some_and(|w| w.staged);
        // 1 本の差分: 選んだ行がすべて含まれていれば Unstage、そうでなければ Stage。片側だけの差分では側で決まる。
        let unstage = if working.is_some_and(|w| w.combined.is_some()) {
            selected > 0 && selected_staged == selected
        } else {
            staged_side
        };
        self.lines.set_visible(!conflict);
        self.lines.set_label(if unstage {
            "Unstage Lines"
        } else {
            "Stage Lines"
        });
        self.lines.set_sensitive(selected > 0);
        // stage 済みの側では捨てない。選んだ行がなければファイルを捨てる。
        self.discard.set_visible(!conflict);
        self.discard.set_label(if selected > 0 {
            "Discard Lines"
        } else {
            "Discard File"
        });
        self.discard.set_sensitive(
            working.is_some_and(|w| !w.staged && w.entry.is_unstaged()) && selected_staged == 0,
        );
    }
}

/// 一覧に出す状態の文字と、その色の CSS のクラス。
fn state_letter(entry: &StatusEntry) -> (&'static str, &'static str) {
    match entry.kind {
        StatusKind::Unmerged => ("U", "error"),
        StatusKind::Untracked => ("?", "success"),
        StatusKind::Renamed => ("R", "accent"),
        StatusKind::Changed => {
            let c = if entry.is_unstaged() {
                entry.worktree
            } else {
                entry.index
            };
            match c {
                b'A' => ("A", "success"),
                b'D' => ("D", "error"),
                _ => ("M", "warning"),
            }
        }
    }
}

/// 木の行（ディレクトリかファイル）。
enum TreeItem {
    Dir {
        name: String,
        path: String,
        depth: usize,
    },
    File {
        index: usize,
        depth: usize,
    },
}

/// ファイルをディレクトリの木の順に並べる（macOS 版と同じく、ファイルを持たず子が 1 つだけのディレクトリは
/// 名前をつないで 1 段にする。ディレクトリはファイルより先、どちらも名前の順）。
fn tree(entries: &[StatusEntry]) -> Vec<TreeItem> {
    #[derive(Default)]
    struct Dir {
        dirs: std::collections::BTreeMap<String, Dir>,
        files: Vec<usize>,
    }
    let mut root = Dir::default();
    for (i, e) in entries.iter().enumerate() {
        let mut dir = &mut root;
        let parts: Vec<&str> = e.path.split('/').collect();
        for part in &parts[..parts.len() - 1] {
            dir = dir.dirs.entry((*part).to_owned()).or_default();
        }
        dir.files.push(i);
    }
    fn walk(
        dir: &Dir,
        prefix: &str,
        depth: usize,
        entries: &[StatusEntry],
        out: &mut Vec<TreeItem>,
    ) {
        for (name, child) in &dir.dirs {
            // 子が 1 つのディレクトリだけなら、名前をつなぐ。
            let (mut name, mut child) = (name.clone(), child);
            while child.files.is_empty() && child.dirs.len() == 1 {
                let (n, c) = child.dirs.iter().next().expect("one");
                name = format!("{name}/{n}");
                child = c;
            }
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            out.push(TreeItem::Dir {
                name,
                path: path.clone(),
                depth,
            });
            walk(child, &path, depth + 1, entries, out);
        }
        let mut files = dir.files.clone();
        files.sort_by(|&a, &b| entries[a].path.cmp(&entries[b].path));
        for index in files {
            out.push(TreeItem::File { index, depth });
        }
    }
    let mut out = Vec::new();
    walk(&root, "", 0, entries, &mut out);
    out
}
