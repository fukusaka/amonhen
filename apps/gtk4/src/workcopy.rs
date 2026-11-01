//! Working Copy: 左の上にコミットの入力、その下に変更のあるファイルの一覧（チェックで次のコミットに含めるか）。
//! 右に選んだファイルの差分（行を選んで含める・外す・捨てる）。項目と文言は macOS 版の
//! `WorkingCopyViewController` と同じ。
//!
//! チェックは index の状態そのもので、FE だけの選択状態は持たない（ターミナルで `git add` しても揃う）。
//! 差分は、できれば HEAD と作業ツリーの 1 本の差分に行ごとの印を付けて見せ（`working_diff_combined`）、
//! それで表せないファイルは、作業ツリーの側（Unstaged）か stage 済みの側（Staged）の差分を見せる。
//! コンフリクトのファイルは、差分の代わりにマージエディタ（`merge.rs`）を埋め込み、上にファイルごとの解決のボタンを出す。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use amonhen_core::diff::{self, DiffView};
use amonhen_core::{Repository, StatusSnapshot, WorkingDiff};
use amonhen_layout::DiffRow;
use amonhen_model::{StatusEntry, StatusKind, tr};
use amonhen_projection::FrontendProfile;
use gtk::prelude::*;
use gtk::{gio, glib};
use serde_json::{Value, json};

use crate::actions::Actions;
use crate::diff::{DiffWidget, View};

type CountCallback = Box<dyn Fn(usize)>;

thread_local! {
    /// 確認用に押したことにするキー（`--keys`）。
    static KEYS: RefCell<std::collections::VecDeque<String>> = RefCell::default();
}

/// 確認用（`--keys list:space,diff:j,…`）: Working Copy で差分を出すたびに、キーを 1 つずつ押したことにする。
pub fn press_keys_once(keys: &str) {
    KEYS.with(|k| {
        k.borrow_mut()
            .extend(keys.split(',').map(|s| s.trim().to_owned()));
    });
}

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
    /// 一覧のファイルの行の、行数を出す場所（パスごと）。行数が届いたら、一覧を作り直さずにここに入れる。
    count_boxes: RefCell<HashMap<String, gtk::Box>>,
    list: gtk::ListBox,
    all: gtk::CheckButton,
    count: gtk::Label,
    filter: gtk::SearchEntry,
    subject: gtk::Entry,
    body: gtk::TextView,
    /// メッセージの入力の補助（下書き・最近のメッセージ・型とトレーラー・fixup!）。
    aids: Rc<crate::commitmsg::MessageAids>,
    amend: gtk::CheckButton,
    commit: gtk::Button,
    /// 件名の文字数。
    summary_count: gtk::Label,
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
    /// 差分（`diff`）か、コンフリクトのファイルのマージエディタ（`merge`）。
    right_stack: gtk::Stack,
    merge_holder: gtk::Box,
    /// 作ったマージエディタ（パスごと。ファイルを選び直しても、編集中の Result を保つ）と、埋め込んでいるもの。
    editors: RefCell<HashMap<String, Rc<crate::merge::Editor>>>,
    embedded: RefCell<Option<Rc<crate::merge::Editor>>>,
    /// 改行を変換している・改行が混ざっているファイルの印（普段のファイルでは出さない）。
    line_endings: gtk::Label,
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
    /// ファイルの行の右クリックのメニューに足す項目（パス、Blame できるか、File History を出せるか）。
    on_file_menu: RefCell<Option<FileMenuCallback>>,
}

type FileMenuCallback = Box<dyn Fn(&str, bool, bool) -> Vec<crate::menu::Item>>;

impl WorkingCopy {
    pub fn new(pending: Rc<Cell<u32>>) -> Self {
        // 左の上: コミットの入力。
        let subject = gtk::Entry::builder().placeholder_text("Summary").build();
        // 本文は等幅（macOS 版と同じ。72 桁の目安の縦線が 72 桁に合うように）。
        let body = gtk::TextView::builder()
            .monospace(true)
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
        // 本文の 72 桁の目安の縦線（72 桁で改行する慣習。macOS 版と同じ）。
        let body_box = gtk::Overlay::new();
        body_box.set_child(Some(&body_scrolled));
        body_box.add_overlay(&body_guide(&body));
        let amend = gtk::CheckButton::with_label("Amend");
        let commit = gtk::Button::with_label("Commit");
        commit.add_css_class("suggested-action");
        commit.set_tooltip_text(Some(&tr!("Commit the checked files (Ctrl+Return)")));
        let commit_menu = gtk::MenuButton::builder()
            .tooltip_text(tr!("Commit and Push (Ctrl+Shift+Return), Amend"))
            .build();
        commit_menu.add_css_class("suggested-action");
        let menu = gio::Menu::new();
        menu.append(Some("Commit and Push"), Some("win.commit-and-push"));
        menu.append(Some("Amend Last Commit"), Some("win.amend-last-commit"));
        commit_menu.set_menu_model(Some(&menu));
        let aids = crate::commitmsg::MessageAids::new(&subject, &body, &amend);
        aids.install_fixups(&commit_menu, menu);
        let commit_group = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        commit_group.add_css_class("linked");
        commit_group.append(&commit);
        commit_group.append(&commit_menu);
        let commit_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        commit_row.append(&amend);
        commit_row.append(&aids.history);
        commit_row.append(&aids.insert);
        // 件名の文字数（50 を超えたら黄、72 を超えたら橙。git の慣習）。
        let summary_count = gtk::Label::new(None);
        summary_count.set_hexpand(true);
        summary_count.set_xalign(1.0);
        summary_count.add_css_class("numeric");
        summary_count.add_css_class("caption");
        summary_count.set_tooltip_text(Some(&tr!(
            "Length of the summary (yellow over 50, orange over 72)"
        )));
        commit_row.append(&summary_count);
        // AI: stage 済みの変更からメッセージを提案する。
        let suggest = gtk::Button::with_label(&tr!("Suggest"));
        crate::assist::track(
            &suggest,
            tr!("AI: suggest a commit message from the staged changes"),
        );
        commit_row.append(&suggest);
        commit_row.append(&commit_group);
        let message = gtk::Label::new(None);
        message.set_xalign(0.0);
        message.set_wrap(true);
        message.add_css_class("error");
        message.set_visible(false);

        // 左の下: 変更のあるファイルの一覧。
        let all = gtk::CheckButton::with_label(&tr!("Changes"));
        all.set_tooltip_text(Some(&tr!(
            "Include or exclude all listed files in the next commit"
        )));
        let count = gtk::Label::new(None);
        count.set_hexpand(true);
        count.set_xalign(1.0);
        count.add_css_class("dim-label");
        count.add_css_class("numeric");
        count.set_tooltip_text(Some(&tr!("Files in the next commit / changed files")));
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
                "Show files as a list of paths",
                Layout::List,
            ),
            (
                crate::icon::Kind::Tree,
                "Show files as a directory tree",
                Layout::Tree,
            ),
            (
                crate::icon::Kind::Doc,
                "Show the file name first and the parent directory dimmed",
                Layout::Flat,
            ),
        ] {
            let icon = crate::icon::Icon::default();
            icon.set(kind, false);
            let button = gtk::ToggleButton::new();
            button.set_child(Some(&icon));
            button.set_tooltip_text(Some(&amonhen_model::i18n::translate(tip)));
            if let Some(first) = layout_buttons.first() {
                button.set_group(Some(&first.0));
            }
            layouts.append(&button);
            layout_buttons.push((button, layout));
        }
        layout_buttons[0].0.set_active(true);
        list_header.append(&layouts);
        let filter = gtk::SearchEntry::builder()
            .placeholder_text(tr!("Filter by path"))
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
        left.append(&body_box);
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
        discard.set_tooltip_text(Some(&tr!(
            "Discard the working tree changes (return to the content of the index)"
        )));
        let worktree_side = gtk::ToggleButton::with_label("Unstaged");
        worktree_side.set_tooltip_text(Some(&tr!(
            "Changes in the working tree that are not staged yet"
        )));
        let staged_side = gtk::ToggleButton::with_label("Staged");
        staged_side.set_tooltip_text(Some(&tr!("Staged changes (included in the next commit)")));
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
        let line_endings = gtk::Label::new(None);
        line_endings.add_css_class("caption");
        line_endings.set_visible(false);
        bar.append(&line_endings);
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
        // コンフリクトのファイルは、差分の代わりにマージエディタを出す（macOS 版と同じ）。
        let merge_holder = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let right_stack = gtk::Stack::new();
        right_stack.set_vexpand(true);
        right_stack.add_named(&diff_scrolled, Some("diff"));
        right_stack.add_named(&merge_holder, Some("merge"));
        right.append(&right_stack);

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
            count_boxes: RefCell::default(),
            list,
            all,
            count,
            filter,
            subject,
            body,
            aids,
            amend,
            commit,
            summary_count,
            message,
            diff,
            path,
            lines,
            discard,
            sides,
            staged_side,
            conflict_buttons,
            right_stack,
            merge_holder,
            editors: RefCell::default(),
            embedded: RefCell::default(),
            line_endings,
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
        let weak = Rc::downgrade(&inner);
        suggest.connect_clicked(move |_| {
            if let Some(this) = weak.upgrade() {
                Inner::suggest_message(&this);
            }
        });
        // 確認用（`--ai suggest`）: 状態を読んでから押す。
        let (b, w) = (suggest.downgrade(), Rc::downgrade(&inner));
        crate::assist::auto(
            "suggest",
            move || {
                b.upgrade().is_some_and(|b| b.is_mapped())
                    && w.upgrade().is_some_and(|w| w.status.borrow().is_some())
            },
            {
                let b = suggest.downgrade();
                move || {
                    if let Some(b) = b.upgrade() {
                        b.emit_clicked();
                    }
                }
            },
        );
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
        self.inner.repo.replace(Some(repo.clone()));
        self.inner.aids.set_repo(repo);
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

    /// `path` の行の右クリックのメニューの項目の名前（確認用。区切りは `---`）。
    pub fn file_menu_titles(&self, path: &str) -> Option<Vec<String>> {
        let entry = self
            .inner
            .all_entries
            .borrow()
            .iter()
            .find(|e| e.path == path)
            .cloned()?;
        Some(crate::menu::titles(&Inner::file_menu(&self.inner, &entry)))
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

    /// ファイルの行の右クリックのメニューに、`f(パス, Blame できるか, File History を出せるか)` の項目
    /// （Blame・File History）を足す。
    pub fn connect_file_menu(
        &self,
        f: impl Fn(&str, bool, bool) -> Vec<crate::menu::Item> + 'static,
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
        // 一覧のキー（macOS 版と同じ）: Space・Enter で選んだファイルを含める・外す、⌫・Delete で捨てる（確かめて）。
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(this);
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            let Some(this) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if modifiers.intersects(
                gtk::gdk::ModifierType::CONTROL_MASK
                    | gtk::gdk::ModifierType::ALT_MASK
                    | gtk::gdk::ModifierType::SUPER_MASK,
            ) || this.filter.has_focus()
            {
                return glib::Propagation::Proceed;
            }
            if Inner::list_key(&this, key) {
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        this.list.add_controller(keys);
        // 差分のキー: Space で選んだ行を含める・外す、⌥J・⌥K で次・前のファイル。
        let weak = Rc::downgrade(this);
        this.diff.connect_toggle_rows(move |rows| {
            let Some(this) = weak.upgrade() else { return };
            let (staged, total) = this.diff.selected_staged();
            Inner::set_rows(&this, rows, Some(staged < total));
        });
        let weak = Rc::downgrade(this);
        this.diff.connect_move_file(move |step| {
            if let Some(this) = weak.upgrade() {
                Inner::move_file(&this, step);
            }
        });
        let weak = Rc::downgrade(this);
        this.diff.connect_selection(move || {
            if let Some(this) = weak.upgrade() {
                this.update_buttons();
            }
        });
        // 差分の行の右クリック: AI でこの hunk を解説。
        let weak = Rc::downgrade(this);
        this.diff.connect_row_menu(move |row, x, y| {
            let Some(this) = weak.upgrade() else { return };
            if let Some(item) = this.hunk_item(row) {
                crate::menu::popup(&this.diff, x, y, vec![item]);
            }
        });
        let (a, b) = (Rc::downgrade(this), Rc::downgrade(this));
        crate::assist::auto(
            "working-hunk",
            move || {
                a.upgrade()
                    .is_some_and(|t| t.diff.is_mapped() && t.diff.first_hunk().is_some())
            },
            move || {
                let Some(this) = b.upgrade() else { return };
                if let Some((_, hunk)) = this.diff.first_hunk() {
                    crate::assist::explain_hunk(
                        this.diff.root().and_downcast(),
                        this.repo().and_then(|r| r.paths().workdir),
                        this.selected.borrow().clone().unwrap_or_default(),
                        hunk,
                        String::new(),
                    );
                }
            },
        );
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
            let button = gtk::Button::with_label(&amonhen_model::i18n::translate(title));
            let weak = Rc::downgrade(this);
            button.connect_clicked(move |_| {
                if let Some(this) = weak.upgrade() {
                    Inner::take_side(&this, ours, title);
                }
            });
            this.conflict_buttons.append(&button);
        }
        let resolved = gtk::Button::with_label(&tr!("Mark Resolved"));
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
            if amend.is_active() && this.message_is_empty() {
                Inner::fill_head_message(&this);
            }
            this.update_buttons();
        });
        // Summary の欄の Enter ではコミットしない（macOS 版と同じ。コミットは Ctrl+Return か Commit のボタン）。
        let weak = Rc::downgrade(this);
        this.commit.connect_clicked(move |_| {
            if let Some(this) = weak.upgrade() {
                Inner::commit(&this, None);
            }
        });
    }

    fn repo(&self) -> Option<Arc<Repository>> {
        self.repo.borrow().clone()
    }

    /// 差分の `row` 行目を含む hunk の「AI でこの hunk を解説」（hunk の外か、AI を使えなければ None）。
    fn hunk_item(&self, row: usize) -> Option<crate::menu::Item> {
        let (_, hunk) = self.diff.hunk_text(row)?;
        crate::assist::hunk_item(
            self.diff.root().and_downcast(),
            self.repo().and_then(|r| r.paths().workdir),
            self.selected.borrow().clone().unwrap_or_default(),
            hunk,
            String::new(),
        )
    }

    /// AI: stage 済みの変更からメッセージを提案し、「メッセージに入れる」で Summary と Description に入れる。
    fn suggest_message(this: &Rc<Self>) {
        let (Some(repo), Some(status)) = (this.repo(), this.status.borrow().clone()) else {
            return;
        };
        let staged: Vec<StatusEntry> = status
            .status
            .entries
            .iter()
            .filter(|e| e.is_staged() && e.kind != StatusKind::Unmerged)
            .cloned()
            .collect();
        if staged.is_empty() {
            this.subject.error_bell();
            return;
        }
        let parent = this.list.root().and_downcast::<gtk::Window>();
        // 書き方の手本: 最近のコミットのメッセージ（まだ読めていなければ直前のもの）。
        let examples = this.aids.recent_messages(10).join("\n---\n");
        let weak = Rc::downgrade(this);
        glib::spawn_future_local(async move {
            let workdir = repo.paths().workdir;
            let key = crate::assist::repository_key(workdir.as_deref());
            let summary = tr!("The diff of {count} staged files", count = staged.len());
            if !crate::assist::confirm(parent.as_ref(), &key, &summary).await {
                return;
            }
            let on_use: Rc<dyn Fn(&str)> = Rc::new(move |text: &str| {
                let Some(this) = weak.upgrade() else { return };
                let text = text.trim();
                let (subject, body) = text.split_once('\n').unwrap_or((text, ""));
                this.subject.set_text(subject.trim());
                this.body.buffer().set_text(body.trim());
                this.update_buttons();
            });
            let p = parent.clone();
            crate::assist::panel(
                parent.as_ref(),
                &tr!("Suggested Commit Message"),
                Some(&tr!("Use Message")),
                Some(on_use),
                crate::assist::work(move |run: crate::assist::Run| {
                    let (repo, staged, examples, workdir, p) = (
                        repo.clone(),
                        staged.clone(),
                        examples.clone(),
                        workdir.clone(),
                        p.clone(),
                    );
                    async move {
                        let files: Vec<String> = staged.iter().map(|e| e.path.clone()).collect();
                        // 差分の文字は裏で作る（大きければ切る）。
                        let (diff, recent) = gio::spawn_blocking(move || {
                            let mut out = amonhen_assist::DiffText::new();
                            for e in &staged {
                                if out.is_full() {
                                    break;
                                }
                                if let Ok(d) = repo.working_diff(e, true, &FrontendProfile::GTK) {
                                    crate::diff::ai_text(&d.view, Some(&e.path), &mut out);
                                }
                            }
                            let recent = if examples.is_empty() {
                                let reply: Value = serde_json::from_str(
                                    &repo.run_json(r#"{"op":"commitMessage","oid":"HEAD"}"#),
                                )
                                .unwrap_or_default();
                                reply["text"].as_str().unwrap_or_default().to_owned()
                            } else {
                                examples
                            };
                            (out.finish(), recent)
                        })
                        .await
                        .unwrap_or_default();
                        let vars = crate::assist::variables([
                            ("diff", diff),
                            ("files", files.join("\n")),
                            ("recent_messages", recent),
                        ]);
                        crate::assist::run_with(
                            p,
                            amonhen_assist::Purpose::SuggestMessage,
                            vars,
                            workdir,
                            run,
                        )
                        .await
                    }
                }),
            );
        });
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
        this.prune_editors();
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
        this.count_boxes.borrow_mut().clear();
        this.list.remove_all();
        let mut row_entries = Vec::new();
        match this.layout.get() {
            Layout::Tree => {
                for item in tree(&entries) {
                    match item {
                        TreeItem::Dir { name, path, depth } => {
                            let collapsed = this.collapsed.borrow().contains(&path);
                            this.list.append(&Inner::dir_row(
                                this, &entries, &name, &path, depth, collapsed,
                            ));
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
        // すべてのチェック: 一覧に出しているファイルのうちコンフリクト以外の状態（押すとそれらを含める・外す）。
        // コンフリクトのファイルは index では stage 済みに見えるが、含めることも外すこともできない（macOS 版と同じ）。
        let candidates: Vec<&StatusEntry> = entries
            .iter()
            .filter(|e| e.kind != StatusKind::Unmerged)
            .collect();
        let on = candidates
            .iter()
            .filter(|e| e.is_staged() && !e.is_unstaged())
            .count();
        let off = candidates.iter().filter(|e| !e.is_staged()).count();
        let mixed = on != candidates.len() && off != candidates.len();
        this.all.set_inconsistent(mixed);
        this.all
            .set_active(!candidates.is_empty() && on == candidates.len());
        this.all.set_sensitive(!candidates.is_empty());
        drop(candidates);
        // 次のコミットに含めるファイルの数 / 変更のあるファイルの数。
        let all = this.all_entries.borrow();
        if all.is_empty() {
            this.count.set_text("");
        } else {
            this.count
                .set_text(&format!("{} / {}", staged_count(&all), all.len()));
        }
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
            // 一覧と差分は作り直さない（行数の場所を埋めるだけ）。
            let stats = this.stats.borrow();
            for (path, counts) in this.count_boxes.borrow().iter() {
                fill_counts(counts, stats.get(path).copied());
            }
        });
    }

    /// 木で、畳んだディレクトリの中のファイルか。
    fn is_hidden(&self, path: &str) -> bool {
        let collapsed = self.collapsed.borrow();
        collapsed
            .iter()
            .any(|dir| path.starts_with(dir.as_str()) && path[dir.len()..].starts_with('/'))
    }

    /// 木のディレクトリの行（クリックで畳む・開く）。チェックは中のファイル（`entries` のうちコンフリクト以外）を
    /// まとめて含める・外す: すべて含めていれば ☑、一部なら ▣（macOS 版と同じ）。
    fn dir_row(
        this: &Rc<Self>,
        entries: &[StatusEntry],
        name: &str,
        path: &str,
        depth: usize,
        collapsed: bool,
    ) -> gtk::ListBoxRow {
        let arrow = gtk::Label::new(Some(if collapsed { "▸" } else { "▾" }));
        arrow.add_css_class("dim-label");
        let inside: Vec<&StatusEntry> = entries
            .iter()
            .filter(|e| {
                e.kind != StatusKind::Unmerged
                    && e.path.starts_with(path)
                    && e.path[path.len()..].starts_with('/')
            })
            .collect();
        let fully = inside
            .iter()
            .filter(|e| e.is_staged() && !e.is_unstaged())
            .count();
        let some = inside.iter().any(|e| e.is_staged());
        let all = !inside.is_empty() && fully == inside.len();
        let check = gtk::CheckButton::new();
        check.set_active(all);
        check.set_inconsistent(some && !all);
        check.set_sensitive(!inside.is_empty());
        let paths: Vec<String> = inside.iter().map(|e| e.path.clone()).collect();
        let weak = Rc::downgrade(this);
        check.connect_toggled(move |_| {
            let Some(this) = weak.upgrade() else { return };
            if this.rebuilding.get() {
                return;
            }
            let paths = paths.clone();
            Inner::run(&this, move |repo| {
                let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
                if all {
                    repo.unstage(&paths)
                } else {
                    repo.stage(&paths)
                }
            });
        });
        let icon = crate::icon::Icon::default();
        icon.set(crate::icon::Kind::Folder, false);
        let label = gtk::Label::new(Some(name));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_ellipsize(gtk::pango::EllipsizeMode::Start);
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        line.set_margin_start((depth * 16) as i32);
        line.append(&arrow);
        line.append(&check);
        line.append(&icon);
        line.append(&label);
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&line));
        row.set_selectable(false);
        let click = gtk::GestureClick::new();
        let (weak, path) = (Rc::downgrade(this), path.to_owned());
        let (r, c) = (row.downgrade(), check.downgrade());
        click.connect_released(move |_, _, x, y| {
            let Some(this) = weak.upgrade() else { return };
            // チェックの上なら、畳まない。
            if let (Some(row), Some(check)) = (r.upgrade(), c.upgrade())
                && row
                    .pick(x, y, gtk::PickFlags::DEFAULT)
                    .is_some_and(|w| w == check || w.is_ancestor(&check))
            {
                return;
            }
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
        let (weak, e) = (Rc::downgrade(this), entry.clone());
        check.connect_toggled(move |_| {
            let Some(this) = weak.upgrade() else { return };
            if this.rebuilding.get() {
                return;
            }
            Inner::toggle_entry(&this, &e);
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
        // 行数（+ 追加 − 削除）。まだ読めていなければ、届いたときに入れる。
        let counts = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        fill_counts(&counts, this.stats.borrow().get(&entry.path).copied());
        line.append(&counts);
        this.count_boxes
            .borrow_mut()
            .insert(entry.path.clone(), counts);
        // 変更を捨てるボタン（マウスを載せたときだけ出す。コンフリクトと、stage 済みだけのファイルにはない）。
        if !conflict && entry.is_unstaged() {
            let discard = gtk::Button::new();
            let icon = crate::icon::Icon::default();
            icon.set(crate::icon::Kind::Undo, false);
            discard.set_child(Some(&icon));
            discard.add_css_class("flat");
            discard.set_tooltip_text(Some(&tr!(
                "Discard the working tree changes (return to the content of the index)"
            )));
            // 隠している間は、透明なだけでなくクリックも Tab のフォーカスも受けない（macOS 版は隠す）。
            // 幅は取ったままにして、出し入れで行数の位置が動かないようにする。
            let reveal = |d: &gtk::Button, shown: bool| {
                d.set_opacity(if shown { 1.0 } else { 0.0 });
                d.set_can_target(shown);
                d.set_focusable(shown);
            };
            reveal(&discard, false);
            let (weak, e) = (Rc::downgrade(this), entry.clone());
            discard.connect_clicked(move |_| {
                if let Some(this) = weak.upgrade() {
                    Inner::discard_file(&this, &e);
                }
            });
            let motion = gtk::EventControllerMotion::new();
            let d = discard.clone();
            motion.connect_enter(move |_, _, _| reveal(&d, true));
            let d = discard.clone();
            motion.connect_leave(move |_| reveal(&d, false));
            line.add_controller(motion);
            line.append(&discard);
        }
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&line));
        // 右クリック（macOS 版と同じ）: 含める・外す・捨てる、.gitignore に足す（untracked）、Blame（消した
        // ファイルにはない）と File History（untracked にはまだ履歴がない）、パスのコピー、ファイルで開く。
        let menu = gtk::GestureClick::new();
        menu.set_button(gtk::gdk::BUTTON_SECONDARY);
        let (weak, e, r) = (Rc::downgrade(this), entry.clone(), row.downgrade());
        menu.connect_pressed(move |_, _, x, y| {
            let (Some(this), Some(row)) = (weak.upgrade(), r.upgrade()) else {
                return;
            };
            let items = Inner::file_menu(&this, &e);
            crate::menu::popup(row.upcast_ref::<gtk::Widget>(), x, y, items);
        });
        row.add_controller(menu);
        row
    }

    /// 確認用（`--keys`）: 差分を出すたびに、残っているキーを 1 つ押したことにする。`list:space`・`list:delete`
    /// は一覧、`diff:space`・`diff:j`・`diff:k`・`diff:alt+j`・`diff:alt+k` は差分のキー、`commit` は Commit。
    fn press_next_key(this: &Rc<Self>) {
        let Some(key) = KEYS.with(|k| k.borrow_mut().pop_front()) else {
            return;
        };
        let this = this.clone();
        this.pending.set(this.pending.get() + 1);
        // 差分を出し終えてから（選び直しの後に）押す。
        glib::idle_add_local_once(move || {
            use gtk::gdk::{Key, ModifierType};
            let (place, name) = key.split_once(':').unwrap_or(("diff", key.as_str()));
            let (modifiers, name) = match name.strip_prefix("alt+") {
                Some(n) => (ModifierType::ALT_MASK, n),
                None => (ModifierType::empty(), name),
            };
            let key = Key::from_name(name).unwrap_or(Key::VoidSymbol);
            if name == "commit" {
                if this.commit.is_sensitive() {
                    Inner::commit(&this, None);
                }
            } else if place == "list" {
                Inner::list_key(&this, key);
            } else {
                this.diff.key_pressed(key, modifiers);
                // 次のキーは、差分を出し直さなくても押す（J・K はスクロールと選択だけ）。
                if !matches!(key, Key::space) {
                    Inner::press_next_key(&this);
                }
            }
            this.pending.set(this.pending.get() - 1);
        });
    }

    /// 一覧のキー: Space・Enter で選んだファイルを含める・外す、⌫・Delete で捨てる（確かめて）。扱ったら true。
    fn list_key(this: &Rc<Self>, key: gtk::gdk::Key) -> bool {
        use gtk::gdk::Key;
        let Some(entry) = this.selected_entry() else {
            return false;
        };
        match key {
            Key::space | Key::Return | Key::KP_Enter => {
                if entry.kind != StatusKind::Unmerged {
                    Inner::toggle_entry(this, &entry);
                }
            }
            Key::BackSpace | Key::Delete | Key::KP_Delete => {
                if entry.kind != StatusKind::Unmerged && entry.is_unstaged() {
                    Inner::discard_file(this, &entry);
                } else {
                    this.list.error_bell();
                }
            }
            _ => return false,
        }
        true
    }

    /// ファイルを次のコミットに含める・外す（すべて含めていれば外し、そうでなければ含める）。
    fn toggle_entry(this: &Rc<Self>, entry: &StatusEntry) {
        let path = entry.path.clone();
        let fully = entry.is_staged() && !entry.is_unstaged();
        Inner::run(this, move |repo| {
            if fully {
                repo.unstage(&[&path])
            } else {
                repo.stage(&[&path])
            }
        });
    }

    /// ⌥J・⌥K: 一覧で次・前のファイルを選ぶ（ディレクトリの行は飛ばす）。
    fn move_file(this: &Rc<Self>, step: i32) {
        let rows = this.row_entries.borrow().clone();
        let current = this.list.selected_row().map_or(-1, |r| r.index());
        let mut i = current + step;
        while i >= 0 && (i as usize) < rows.len() {
            if rows[i as usize].is_some() {
                if let Some(row) = this.list.row_at_index(i) {
                    this.list.select_row(Some(&row));
                    this.diff.grab_focus();
                }
                return;
            }
            i += step;
        }
        this.list.error_bell();
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

    /// 1 ファイルの差分を裏で読んで出す。`same` なら同じファイルの読み直し（位置を保つ）。
    fn load_diff(this: &Rc<Self>, entry: StatusEntry, same: bool) {
        let Some(repo) = this.repo() else { return };
        let prefer_staged = this.staged_side.is_active();
        let generation = this.generation.get() + 1;
        this.generation.set(generation);
        this.path.set_text(&entry.path);
        if entry.kind == StatusKind::Unmerged {
            Inner::show_conflict(this, &entry.path, generation);
            return;
        }
        this.unembed();
        this.pending.set(this.pending.get() + 1);
        let weak = Rc::downgrade(this);
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                let profile = FrontendProfile::GTK;
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
                Ok::<_, amonhen_model::Error>((Arc::new(Working(Some(working))), View::new(view)))
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
                    this.show_line_endings(w.and_then(|w| w.line_endings.as_ref()));
                    this.sides.set_visible(w.is_some_and(|w| {
                        w.combined.is_none() && w.entry.is_staged() && w.entry.is_unstaged()
                    }));
                    this.conflict_buttons.set_visible(false);
                    this.current.replace(Some(working));
                    this.diff.show_working(view, same);
                    if let Some(row) = this.toggle_once.take() {
                        Inner::set_rows(&this, vec![row as usize], None);
                    }
                    Inner::press_next_key(&this);
                }
                Ok(Err(e)) => this.show_error(&tr!("Can’t read the diff: {e}", e = e)),
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

    /// Discard Lines・Discard File（macOS 版と同じ）: 選んだ行があれば行を、なければファイルを捨てる
    /// （ファイルは確かめてから。untracked のファイルは消す）。
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
        // ファイル全体を一度に捨てるときは確かめる（行の単位は選んで押すので、そのまま。macOS 版と同じ）。
        let entry = w.entry.clone();
        Inner::discard_file(this, &entry);
    }

    /// 一覧の行のボタン: 1 ファイルの変更を捨てる（確かめてから）。作業ツリーを index の内容に戻すので、
    /// 含めている分は残る。untracked のファイルは消す。
    fn discard_file(this: &Rc<Self>, entry: &StatusEntry) {
        let (Some(actions), path) = (this.actions.borrow().clone(), entry.path.clone()) else {
            return;
        };
        if entry.kind == StatusKind::Untracked {
            crate::sheets::confirm_then(
                &actions,
                tr!("Delete {path}?", path = path),
                format!("{} {}", tr!("It’s untracked."), crate::sheets::cant_undo()),
                tr!("Delete"),
                true,
                vec![json!({ "op": "deleteUntracked", "paths": [path] })],
            );
        } else {
            crate::sheets::confirm_then(
                &actions,
                tr!("Discard changes to {path}?", path = path),
                tr!(
                    "This discards the working tree changes and returns to the content of the next commit (the index). You can Undo this."
                ),
                "Discard".to_owned(),
                false,
                vec![json!({ "op": "discard", "paths": [path] })],
            );
        }
    }

    /// ファイルの行の右クリックのメニュー。
    fn file_menu(this: &Rc<Self>, entry: &StatusEntry) -> Vec<crate::menu::Item> {
        use crate::menu::{Item, action};
        let mut items = Vec::new();
        let path = entry.path.clone();
        let untracked = entry.kind == StatusKind::Untracked;
        let conflict = entry.kind == StatusKind::Unmerged;
        let name = path.rsplit('/').next().unwrap_or(&path).to_owned();
        if !conflict {
            let fully = entry.is_staged() && !entry.is_unstaged();
            let (w, p) = (Rc::downgrade(this), path.clone());
            items.push(action(if fully { "Unstage" } else { "Stage" }, move || {
                let Some(this) = w.upgrade() else { return };
                let p = p.clone();
                Inner::run(&this, move |repo| {
                    if fully {
                        repo.unstage(&[&p])
                    } else {
                        repo.stage(&[&p])
                    }
                });
            }));
            if untracked || entry.is_unstaged() {
                let (w, e) = (Rc::downgrade(this), entry.clone());
                let title = if untracked {
                    tr!("Delete {name}…", name = name)
                } else {
                    "Discard…".to_owned()
                };
                items.push(action(title, move || {
                    if let Some(this) = w.upgrade() {
                        Inner::discard_file(&this, &e);
                    }
                }));
            }
            items.push(Item::Separator);
        }
        // untracked のファイルにはまだ履歴がない。代わりに .gitignore に足せる（そのパスか、拡張子のパターン）。
        if untracked {
            let mut patterns = vec![path.clone()];
            if let Some((_, ext)) = name.rsplit_once('.')
                && !ext.is_empty()
            {
                patterns.push(format!("*.{ext}"));
            }
            for pattern in patterns {
                let w = Rc::downgrade(this);
                items.push(action(
                    tr!("Add “{pattern}” to .gitignore", pattern = pattern),
                    move || {
                        if let Some(this) = w.upgrade() {
                            Inner::add_to_gitignore(&this, &pattern);
                        }
                    },
                ));
            }
        } else if let Some(f) = this.on_file_menu.borrow().as_ref() {
            items.extend(f(&path, entry.worktree != b'D', true));
        }
        items.push(Item::Separator);
        let (w, p) = (this.list.downgrade(), path.clone());
        items.push(action(tr!("Copy Path"), move || {
            if let Some(w) = w.upgrade() {
                w.clipboard().set_text(&p);
            }
        }));
        // 手元のリポジトリなら、ファイルマネージャーとほかのアプリで開ける（daemon・SSH 越しのファイルは開けない）。
        let local = this
            .repo()
            .filter(|r| r.connection_state().is_none())
            .and_then(|r| r.paths().workdir);
        if let Some(workdir) = local
            && entry.worktree != b'D'
        {
            let file = gio::File::for_path(workdir.join(&path));
            let (w, f) = (this.list.downgrade(), file.clone());
            items.push(action(tr!("Show in Files"), move || {
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
            items.push(action(tr!("Open with Default App"), move || {
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

    /// `pattern` を作業ツリーの直下の .gitignore の最後に足す（コアで読み書きするので、SSH 先でも同じ）。
    fn add_to_gitignore(this: &Rc<Self>, pattern: &str) {
        let (Some(repo), Some(actions)) = (this.repo(), this.actions.borrow().clone()) else {
            return;
        };
        let pattern = pattern.to_owned();
        glib::spawn_future_local(async move {
            let files = crate::sheets::run(&repo, json!({ "op": "ignoreFiles" })).await;
            let mut gitignore = files["info"]["gitignore"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            if gitignore.lines().any(|l| l.trim() == pattern) {
                return;
            }
            if !gitignore.is_empty() && !gitignore.ends_with('\n') {
                gitignore.push('\n');
            }
            gitignore.push_str(&pattern);
            gitignore.push('\n');
            actions.perform(json!({
                "op": "saveIgnoreFiles",
                "gitignore": gitignore,
                "exclude": files["info"]["exclude"].as_str().unwrap_or_default(),
            }));
        });
    }

    /// コンフリクトのファイルを選んだ: 差分の代わりに、そのファイルのマージエディタを埋め込む（作ったものが
    /// あれば編集中のまま、なければ裏で読んで作る）。別の窓で開いていれば、そう案内する。
    fn show_conflict(this: &Rc<Self>, path: &str, generation: u64) {
        this.current.replace(Some(Arc::new(Working(None))));
        this.show_line_endings(None);
        this.sides.set_visible(false);
        this.conflict_buttons.set_visible(true);
        this.update_buttons();
        if crate::merge::open_window(path).is_some() {
            this.embed_message(&tr!("The merge editor is open in a separate window"));
            return;
        }
        let existing = this.editors.borrow().get(path).cloned();
        if let Some(editor) = existing {
            Inner::embed(this, &editor);
            return;
        }
        let (Some(repo), Some(actions)) = (this.repo(), this.actions.borrow().clone()) else {
            return;
        };
        this.pending.set(this.pending.get() + 1);
        let (weak, path) = (Rc::downgrade(this), path.to_owned());
        glib::spawn_future_local(async move {
            let loaded = crate::merge::load(repo, actions, &path).await;
            let Some(this) = weak.upgrade() else { return };
            this.pending.set(this.pending.get() - 1);
            if this.generation.get() != generation {
                return;
            }
            match loaded {
                Ok(editor) => {
                    this.editors.borrow_mut().insert(path, editor.clone());
                    Inner::embed(&this, &editor);
                    crate::merge::Editor::opened(&editor);
                }
                Err(e) => this.embed_message(&tr!("Can’t read: {error}", error = e)),
            }
        });
    }

    /// 右の欄の差分の代わりに、マージエディタを出す。
    fn embed(this: &Rc<Self>, editor: &Rc<crate::merge::Editor>) {
        let same = this
            .embedded
            .borrow()
            .as_ref()
            .is_some_and(|e| Rc::ptr_eq(e, editor));
        if !same {
            this.clear_holder();
            this.merge_holder.append(editor.widget());
            editor.widget().set_vexpand(true);
            this.embedded.replace(Some(editor.clone()));
        }
        this.right_stack.set_visible_child_name("merge");
        // 別の窓に移す（編集中の内容のまま）。
        let weak = Rc::downgrade(this);
        let e = Rc::downgrade(editor);
        editor.set_on_pop_out(Some(Rc::new(move || {
            let (Some(this), Some(editor)) = (weak.upgrade(), e.upgrade()) else {
                return;
            };
            let Some(window) = this.merge_holder.root().and_downcast::<gtk::Window>() else {
                return;
            };
            this.unembed();
            this.editors.borrow_mut().remove(editor.path());
            crate::merge::show_in_window(&editor, &window);
            this.embed_message(&tr!("The merge editor is open in a separate window"));
        })));
        // 保存して解決済みにしたら外す（status を読み直すと、普段の差分になる）。
        let weak = Rc::downgrade(this);
        let e = Rc::downgrade(editor);
        editor.set_on_resolved(move || {
            let (Some(this), Some(editor)) = (weak.upgrade(), e.upgrade()) else {
                return;
            };
            if this
                .embedded
                .borrow()
                .as_ref()
                .is_some_and(|x| Rc::ptr_eq(x, &editor))
            {
                this.unembed();
            }
            this.editors.borrow_mut().remove(editor.path());
        });
    }

    /// マージエディタを外して、差分に戻す。
    fn unembed(&self) {
        self.clear_holder();
        self.embedded.replace(None);
        self.right_stack.set_visible_child_name("diff");
    }

    fn clear_holder(&self) {
        while let Some(child) = self.merge_holder.first_child() {
            self.merge_holder.remove(&child);
        }
    }

    /// マージエディタの場所に、案内（別の窓で開いている・読めない）を出す。
    fn embed_message(&self, text: &str) {
        self.unembed();
        let label = gtk::Label::new(Some(text));
        label.add_css_class("dim-label");
        label.set_wrap(true);
        label.set_vexpand(true);
        label.set_valign(gtk::Align::Center);
        self.merge_holder.append(&label);
        self.right_stack.set_visible_child_name("merge");
    }

    /// コンフリクトでなくなったファイルのマージエディタを捨てる（埋め込んでいれば外す）。
    fn prune_editors(&self) {
        let conflicted: std::collections::HashSet<String> = self
            .all_entries
            .borrow()
            .iter()
            .filter(|e| e.kind == StatusKind::Unmerged)
            .map(|e| e.path.clone())
            .collect();
        self.editors
            .borrow_mut()
            .retain(|path, _| conflicted.contains(path));
        let stale = self
            .embedded
            .borrow()
            .as_ref()
            .is_some_and(|e| !conflicted.contains(e.path()));
        if stale {
            self.unembed();
        }
    }

    /// Take Current File・Take Incoming File: コンフリクトのファイルを片側のファイルそのものにする。マーカーが
    /// 残っていなければ、解決した内容（rerere・ほかのエディタ）を上書きしてしまうので確かめる（macOS 版と同じ）。
    fn take_side(this: &Rc<Self>, ours: bool, title: &'static str) {
        let (Some(path), Some(actions), Some(repo)) = (
            this.selected.borrow().clone(),
            this.actions.borrow().clone(),
            this.repo(),
        ) else {
            return;
        };
        glib::spawn_future_local(async move {
            // 内容のコンフリクトのファイルだけが載る（消した・足したの食い違いは載らないので、確かめない）。
            let files = crate::sheets::run(&repo, json!({ "op": "conflictFiles" })).await;
            let resolved = files["info"]["files"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|f| {
                    f["path"].as_str() == Some(path.as_str()) && f["markers"].as_u64() == Some(0)
                });
            if resolved
                && !actions
                    .confirm(
                        &tr!("Replace the resolved content of {path}?", path = path),
                        &tr!(
                            "No conflict markers are left, so it may already be resolved. It becomes one side’s whole file."
                        ),
                        &amonhen_model::i18n::translate(title),
                    )
                    .await
            {
                return;
            }
            actions.perform(json!({ "op": "takeSide", "path": path, "ours": ours }));
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
                        &tr!(
                            "Mark {path} as resolved with {count} conflicts left?",
                            path = path,
                            count = remaining
                        ),
                        &tr!("The conflict markers stay in the file."),
                        &tr!("Mark Resolved"),
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
        // hooks の出力を探すため、始めた時刻（Activity の記録と同じ UNIX 時刻のミリ秒）を覚えておく。
        let started = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        this.pending.set(this.pending.get() + 1);
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                repo.commit(&message, amend)
                    .map_err(|e| e.to_string())
                    .and_then(|()| repo.load_status().map_err(|e| e.to_string()))
            })
            .await;
            let Some(this) = weak.upgrade() else { return };
            let parent = this.list.root().and_downcast::<gtk::Window>();
            match result {
                Ok(Ok(status)) => {
                    // hooks（pre-commit・commit-msg など）が何か書いていれば見せる（macOS 版と同じ）。
                    let output = amonhen_model::activity::records()
                        .into_iter()
                        .rev()
                        .find(|r| {
                            r.kind == "git"
                                && r.label.starts_with("git commit")
                                && r.started >= started
                                && r.ok == Some(true)
                        })
                        .and_then(|r| r.output)
                        .filter(|o| !o.trim().is_empty());
                    if let Some(output) = output {
                        hooks_sheet(parent.as_ref(), &tr!("Committed (hooks output)"), &output);
                    }
                    this.aids.clear_draft();
                    this.subject.set_text("");
                    this.body.buffer().set_text("");
                    this.amend.set_active(false);
                    Inner::show(&this, status);
                    if let Some(then) = then {
                        then();
                    }
                }
                // hooks で止まったときは出力が長いので、読めるシートで出す。
                Ok(Err(e)) => hooks_sheet(parent.as_ref(), &tr!("Couldn’t Commit"), &e),
                Err(_) => {}
            }
            this.update_buttons();
            // 出したシートが描かれるまで（`--snapshot` が撮れるように）少し待ってから終えたことにする。
            let pending = this.pending.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || {
                pending.set(pending.get() - 1);
            });
        });
    }

    /// Summary も Description も空か（書きかけのメッセージを直前のコミットのもので上書きしない）。
    fn message_is_empty(&self) -> bool {
        self.subject.text().is_empty() && self.body.buffer().char_count() == 0
    }

    /// 直前のコミットを直すとき、そのメッセージを入れておく（1 行目を Summary、空行の後を本文に）。
    /// Summary と Description のどちらかに書いてあれば入れない（macOS 版と同じ）。
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
            if !this.message_is_empty() || !this.amend.is_active() {
                return;
            }
            let message = reply["text"].as_str().unwrap_or_default().trim();
            let (subject, body) = message.split_once('\n').unwrap_or((message, ""));
            this.subject.set_text(subject.trim());
            this.body.buffer().set_text(body.trim());
            this.update_buttons();
        });
    }

    /// 改行の印: 作業ツリーの形 → git に入る形。説明に、作業ツリー・git・checkout の形と変換を出す。
    fn show_line_endings(&self, le: Option<&amonhen_model::LineEndings>) {
        use amonhen_model::Eol;
        let Some(le) = le.filter(|le| le.notable()) else {
            self.line_endings.set_visible(false);
            return;
        };
        let name = |e: Eol| match e {
            Eol::Lf => "LF".to_owned(),
            Eol::Crlf => "CRLF".to_owned(),
            Eol::Mixed => tr!("Mixed"),
            Eol::None => tr!("None"),
        };
        let mut text = if le.worktree != le.index && le.index != Eol::None {
            format!("{} → {}", name(le.worktree), name(le.index))
        } else {
            name(le.worktree)
        };
        if le.warning() {
            text = format!("⚠︎ {text}");
            self.line_endings.add_css_class("warning");
        } else {
            self.line_endings.remove_css_class("warning");
        }
        self.line_endings.set_text(&text);
        let mut tip = vec![tr!(
            "Line endings: working tree {worktree}, git {git}, checkout {checkout}",
            worktree = name(le.worktree),
            git = name(le.index),
            checkout = name(le.checkout)
        )];
        if !le.rule.is_empty() {
            tip.push(tr!("Conversion: {rule}", rule = le.rule));
        }
        if le.warning() {
            tip.push(tr!(
                "The next commit or checkout may change the line endings of the whole file"
            ));
        }
        self.line_endings.set_tooltip_text(Some(&tip.join("\n")));
        self.line_endings.set_visible(true);
    }

    fn show_error(&self, e: &str) {
        self.message.set_text(e);
        self.message.set_visible(true);
    }

    fn update_buttons(&self) {
        let staged = staged_count(&self.all_entries.borrow()) > 0;
        let has_subject = !self.subject.text().trim().is_empty();
        let amend = self.amend.is_active();
        self.commit.set_sensitive(has_subject && (staged || amend));
        self.commit
            .set_label(if amend { "Amend Commit" } else { "Commit" });
        // 件名の文字数（空なら出さない）。
        let length = self.subject.text().chars().count();
        let color = if length > 72 {
            Some("#e66100")
        } else if length > 50 {
            Some("#c49000")
        } else {
            None
        };
        match (length, color) {
            (0, _) => self.summary_count.set_text(""),
            (n, Some(c)) => self.summary_count.set_markup(&format!(
                "<span foreground=\"{c}\" weight=\"bold\">{n}</span>"
            )),
            (n, None) => self
                .summary_count
                .set_markup(&format!("<span alpha=\"45%\">{n}</span>")),
        }

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

/// hooks の出力（git が標準エラーに書いたもの）を等幅の字体で読めるシート（macOS 版の `HookOutputSheet`）。
fn hooks_sheet(parent: Option<&gtk::Window>, title: &str, text: &str) {
    let sheet = crate::sheets::Sheet::unnamed(parent, title, "");
    let view = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .top_margin(6)
        .bottom_margin(6)
        .left_margin(6)
        .right_margin(6)
        .build();
    view.buffer().set_text(text.trim_end());
    let scrolled = gtk::ScrolledWindow::builder()
        .child(&view)
        .min_content_width(560)
        .min_content_height(260)
        .has_frame(true)
        .build();
    sheet.add(&scrolled);
    sheet.button(
        "OK",
        crate::sheets::Response::Ok,
        Some(crate::sheets::Style::Suggested),
    );
    glib::spawn_future_local(sheet.run());
}

/// 本文の欄に重ねる、72 桁の目安の縦線（`body` の字の「0」の幅から位置を決める）。
fn body_guide(body: &gtk::TextView) -> gtk::DrawingArea {
    let guide = gtk::DrawingArea::new();
    guide.set_can_target(false);
    let weak = body.downgrade();
    guide.set_draw_func(move |area, cr, _, height| {
        let Some(body) = weak.upgrade() else { return };
        let zero = body.create_pango_layout(Some("0")).pixel_size().0 as f64;
        // 欄の枠（1 px）と左の余白の後ろから数える。
        let x = 1.0 + body.left_margin() as f64 + zero * 72.0;
        if x >= area.width() as f64 - 2.0 {
            return;
        }
        let fg = area.color();
        cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 0.15);
        cr.rectangle(x.round(), 1.0, 1.0, height as f64 - 2.0);
        let _ = cr.fill();
    });
    guide
}

/// 一覧の行の行数（+ 追加 − 削除）を入れ直す（0 の側は出さない。なければ隠す）。
fn fill_counts(counts: &gtk::Box, stats: Option<LineCounts>) {
    while let Some(child) = counts.first_child() {
        counts.remove(&child);
    }
    let (added, removed) = stats.unwrap_or((None, None));
    for (n, mark, css) in [(added, "+", "success"), (removed, "−", "error")] {
        if let Some(n) = n.filter(|&n| n > 0) {
            let l = gtk::Label::new(Some(&format!("{mark}{n}")));
            l.add_css_class("caption");
            l.add_css_class("numeric");
            l.add_css_class(css);
            counts.append(&l);
        }
    }
    counts.set_visible(counts.first_child().is_some());
}

/// 次のコミットに含めるファイルの数（コンフリクトのファイルは index では stage 済みに見えるので除く）。
fn staged_count(entries: &[StatusEntry]) -> usize {
    entries
        .iter()
        .filter(|e| e.is_staged() && e.kind != StatusKind::Unmerged)
        .count()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: StatusKind, path: &str, index: u8, worktree: u8) -> StatusEntry {
        StatusEntry {
            kind,
            path: path.to_owned(),
            orig_path: None,
            index,
            worktree,
            head_id: None,
            index_id: None,
            index_mode: 0,
            submodule: false,
        }
    }

    /// コンフリクトのファイル（index が U）は、次のコミットに含める数に入れない。
    #[test]
    fn staged_count_skips_conflicts() {
        let entries = [
            entry(StatusKind::Unmerged, "a", b'U', b'U'),
            entry(StatusKind::Changed, "b", b'M', b'.'),
            entry(StatusKind::Changed, "c", b'.', b'M'),
            entry(StatusKind::Untracked, "d", b'?', b'?'),
        ];
        assert_eq!(staged_count(&entries), 1);
        assert_eq!(staged_count(&entries[..1]), 0);
    }
}
