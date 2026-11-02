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

/// sparse checkout の知らせを押したときに呼ぶもの。
type ShowFolders = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

pub struct WorkingCopy {
    pub root: gtk::Paned,
    inner: Rc<Inner>,
    /// 一覧の見せ方の切り替え（`--layout`）。
    layouts: Vec<(gtk::ToggleButton, Layout)>,
    /// 見せ方を変えたら設定に覚えるか。
    remember: Rc<Cell<bool>>,
    /// sparse checkout なら、一覧の上にそう出す（押すと、リポジトリの設定の「取得とフォルダ」へ）。
    sparse_note: gtk::Button,
    sparse_label: gtk::Label,
    on_show_folders: ShowFolders,
}

struct Inner {
    repo: RefCell<Option<Arc<Repository>>>,
    actions: RefCell<Option<Rc<Actions>>>,
    /// Commit の横の ▾（Commit and Push・Amend Last Commit）。
    commit_menu: gtk::MenuButton,
    status: RefCell<Option<Arc<StatusSnapshot>>>,
    /// 一覧に出しているファイル（絞り込み済み）。
    entries: RefCell<Vec<StatusEntry>>,
    /// 一覧の行ごとの、`entries` の番号（ディレクトリの行は None）。
    row_entries: RefCell<Vec<Option<usize>>>,
    /// 一覧の行（並びのまま。`store` と同じ）。部品は見えている行の分だけ作り、使い回す（変更が何千もあっても、
    /// 最初に出すときも status が届くたびも、全部の行の部品は作らない）。
    rows: RefCell<Vec<Row>>,
    /// 一覧の行の中身（`Row` を入れた BoxedAnyObject）と、その選択。
    store: gio::ListStore,
    selection: gtk::MultiSelection,
    /// 作った行の部品（行の部品の箱ごと）。行数・コンフリクトの状態が届いたら、見えている行だけ直す。
    cells: RefCell<HashMap<gtk::Box, Rc<RowCell>>>,
    /// 選び直しを待っている（選択の変化は、フォーカスが移り終えてから読む）世代。作り直したら捨てる。
    follow: Cell<u64>,
    layout: Cell<Layout>,
    /// 木で畳んでいるディレクトリ。
    collapsed: RefCell<std::collections::HashSet<String>>,
    /// 絞り込む前のすべてのファイル。
    all_entries: RefCell<Vec<StatusEntry>>,
    /// ファイルごとの行数（追加, 削除）。HEAD と作業ツリーの比べ。
    stats: RefCell<HashMap<String, LineCounts>>,
    /// 内容のコンフリクトのファイルに残っているマーカーの数と、rerere が解決を当てたか（パスごと）。
    conflict_states: RefCell<HashMap<String, ConflictState>>,
    list: gtk::ListView,
    /// 一覧が空のときの文（変更がない・絞り込みに合うファイルがない）。
    empty: gtk::Label,
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
    /// 差分の中を探す帯（Ctrl+F）。
    find: crate::difffind::DiffFind,
    path: gtk::Label,
    lines: gtk::Button,
    discard: gtk::Button,
    /// 1 本の差分で表せないファイルで、作業ツリーの側と stage 済みの側を切り替える。
    sides: gtk::Box,
    worktree_side: gtk::ToggleButton,
    staged_side: gtk::ToggleButton,
    /// コンフリクトのファイルの解決のボタン。
    conflict_buttons: gtk::Box,
    /// 差分（`diff`）か、コンフリクトのファイルのマージエディタ（`merge`）。
    right_stack: gtk::Stack,
    merge_holder: gtk::Box,
    /// 作ったマージエディタ（worktree とパスごと。ファイルを選び直しても、書き込む先の worktree を切り替えて
    /// 戻っても、編集中の Result を保つ）と、埋め込んでいるもの。
    editors: RefCell<HashMap<EditorKey, Rc<crate::merge::Editor>>>,
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

/// コンフリクトのファイルの中身の状態（`conflictFiles`）: 残っているマーカーの数と、rerere が解決を当てたか。
#[derive(Clone, Copy, PartialEq, Eq)]
struct ConflictState {
    markers: u64,
    rerere: bool,
}

/// 一覧の 1 行の部品（使い回す。出している行は `item`）。
struct RowCell {
    item: RefCell<Option<Row>>,
    list_item: glib::WeakRef<gtk::ListItem>,
    line: gtk::Box,
    /// 木のディレクトリの ▸・▾（クリックで畳む・開く）。
    arrow: gtk::Label,
    check: gtk::CheckButton,
    state: gtk::Label,
    name: gtk::Label,
    /// 名前の後ろの薄い注記（Flat の親のディレクトリ、コンフリクトの「rerere」「no markers」）。
    detail: gtk::Label,
    counts: gtk::Box,
    discard: gtk::Button,
    /// 変更を捨てるボタンを、マウスを載せたときに出すか（コンフリクトと、stage 済みだけのファイルにはない）。
    can_discard: Cell<bool>,
    /// 中身を入れている間（チェックの変化を操作として扱わない）。
    binding: Cell<bool>,
}

/// マージエディタの鍵: worktree（作業ツリーの場所）とパス。
type EditorKey = (Option<std::path::PathBuf>, String);

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
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .has_frame(true)
            .build();
        // 本文の 72 桁の目安の縦線（72 桁で改行する慣習。macOS 版と同じ）。
        let body_box = gtk::Overlay::new();
        body_box.set_child(Some(&body_scrolled));
        body_box.add_overlay(&body_guide(&body));
        body_box.add_overlay(&body_placeholder(&body));
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
        let suggest = gtk::Button::new();
        // macOS 版と同じ sparkles のアイコン。
        let icon = crate::icon::Icon::default();
        icon.set(crate::icon::Kind::Sparkles, false);
        suggest.set_child(Some(&icon));
        suggest.add_css_class("flat");
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
        // 見えている行だけに部品を作る（変更が何千もあっても、最初に出すときに固まらない）。何行も選べる（Space・
        // Enter で選んだファイルをまとめて含める・外す。macOS 版と同じ）。クリックは選ぶだけ（activate しない）。
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let selection = gtk::MultiSelection::new(Some(store.clone()));
        let factory = gtk::SignalListItemFactory::new();
        let list = gtk::ListView::new(Some(selection.clone()), Some(factory.clone()));
        list.add_css_class("navigation-sidebar");
        let empty = gtk::Label::new(None);
        empty.add_css_class("dim-label");
        empty.set_margin_top(12);
        empty.set_valign(gtk::Align::Start);
        empty.set_can_target(false);
        let list_scrolled = gtk::ScrolledWindow::builder()
            .child(&list)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        // 一覧が空のときの文は、一覧の上に重ねる（GtkListView には placeholder がない）。
        let list_area = gtk::Overlay::new();
        list_area.set_child(Some(&list_scrolled));
        list_area.add_overlay(&empty);

        // 左の列は上下 2 段: 上にコミットの入力、下にファイルの一覧。間の仕切りをドラッグするとメッセージの欄の
        // 高さが変わり、位置は覚えておく。窓の高さが変わったときは、ファイルの一覧の側が伸び縮みする（macOS 版と同じ）。
        let message_part = gtk::Box::new(gtk::Orientation::Vertical, 8);
        message_part.set_margin_top(10);
        message_part.set_margin_bottom(6);
        message_part.append(&subject);
        message_part.append(&body_box);
        message_part.append(&commit_row);
        message_part.append(&message);
        let files_part = gtk::Box::new(gtk::Orientation::Vertical, 8);
        files_part.set_margin_top(6);
        list_scrolled.set_min_content_height(100);
        files_part.append(&list_header);
        // sparse checkout なら、一覧の上にそう出す（取り出していないフォルダのファイルは変更に出ない。macOS 版と同じ）。
        let sparse_label = gtk::Label::new(None);
        sparse_label.add_css_class("dim-label");
        sparse_label.add_css_class("caption");
        sparse_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let sparse_icon = crate::icon::Icon::default();
        sparse_icon.set(crate::icon::Kind::FolderMinus, false);
        let sparse_box = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        sparse_box.append(&sparse_icon);
        sparse_box.append(&sparse_label);
        let sparse_note = gtk::Button::new();
        sparse_note.set_child(Some(&sparse_box));
        sparse_note.add_css_class("flat");
        sparse_note.set_halign(gtk::Align::Start);
        sparse_note.set_visible(false);
        let on_show_folders: ShowFolders = Rc::default();
        let f = on_show_folders.clone();
        sparse_note.connect_clicked(move |_| {
            let show = f.borrow().clone();
            if let Some(show) = show {
                show();
            }
        });
        files_part.append(&sparse_note);
        files_part.append(&filter);
        files_part.append(&list_area);
        let left = gtk::Paned::new(gtk::Orientation::Vertical);
        left.set_margin_start(10);
        left.set_margin_end(10);
        left.set_start_child(Some(&message_part));
        left.set_end_child(Some(&files_part));
        left.set_resize_start_child(false);
        left.set_shrink_start_child(false);
        left.set_shrink_end_child(false);
        remember_message_height(&left);

        // 右: 操作のボタンと差分。
        let path = gtk::Label::new(None);
        path.set_xalign(0.0);
        path.set_hexpand(true);
        path.set_ellipsize(gtk::pango::EllipsizeMode::Start);
        path.add_css_class("heading");
        let lines = gtk::Button::with_label("Stage Lines");
        let discard = gtk::Button::with_label("Discard File");
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
            .build();
        let right = gtk::Box::new(gtk::Orientation::Vertical, 0);
        right.append(&bar);
        right.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        // コンフリクトのファイルは、差分の代わりにマージエディタを出す（macOS 版と同じ）。
        let merge_holder = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let right_stack = gtk::Stack::new();
        right_stack.set_vexpand(true);
        // 差分の上に検索の帯（macOS 版と同じく、どの差分でも Ctrl+F で探せる）。
        let diff_area = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let (find, find_bar) = crate::difffind::DiffFind::new(&diff, &diff_area);
        diff_area.append(&find_bar);
        diff_area.append(&diff_scrolled);
        right_stack.add_named(&diff_area, Some("diff"));
        right_stack.add_named(&merge_holder, Some("merge"));
        right.append(&right_stack);

        let root = gtk::Paned::new(gtk::Orientation::Horizontal);
        root.set_start_child(Some(&left));
        root.set_end_child(Some(&right));
        root.set_resize_start_child(false);
        // 左の列は最小の幅より狭くしない（「Commit 3 Files」などで行が伸びると、左の余白が削られて詰まる）。
        root.set_shrink_start_child(false);
        root.set_position(340);

        let inner = Rc::new(Inner {
            repo: RefCell::default(),
            actions: RefCell::default(),
            commit_menu: commit_menu.clone(),
            status: RefCell::default(),
            entries: RefCell::default(),
            row_entries: RefCell::default(),
            rows: RefCell::default(),
            store,
            selection,
            cells: RefCell::default(),
            follow: Cell::new(0),
            layout: Cell::new(Layout::List),
            collapsed: RefCell::default(),
            all_entries: RefCell::default(),
            stats: RefCell::default(),
            conflict_states: RefCell::default(),
            list,
            empty,
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
            find,
            path,
            lines,
            discard,
            sides,
            worktree_side,
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
        Inner::install_factory(&inner, &factory);
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
            sparse_note,
            sparse_label,
            on_show_folders,
        }
    }

    /// sparse checkout なら、一覧の上にそう出す（取り出していないフォルダのファイルは変更に出ない）。
    pub fn set_sparse(&self, sparse: Option<&crate::repoform::Sparse>) {
        self.sparse_note.set_visible(sparse.is_some());
        let Some(sparse) = sparse else { return };
        self.sparse_label.set_text(&if sparse.cone {
            tr!(
                "Only some folders are checked out ({count})",
                count = sparse.folders.len()
            )
        } else {
            tr!("Only some files are checked out")
        });
        self.sparse_note.set_tooltip_text(Some(&format!(
            "{}\n{}",
            sparse.help(),
            tr!("Click to change them in Settings > Fetching and Folders.")
        )));
    }

    /// sparse checkout の知らせを押したら `f` を呼ぶ（リポジトリの設定の「取得とフォルダ」を開く）。
    pub fn connect_show_folders(&self, f: impl Fn() + 'static) {
        self.on_show_folders.replace(Some(Rc::new(f)));
    }

    pub fn set_repo(&self, repo: Arc<Repository>) {
        let old = self.inner.repo.replace(Some(repo.clone()));
        // 書き込む先の worktree を切り替えた: 前の worktree のマージエディタを外す（作ったものは戻ったときに使う）。
        if old.is_some_and(|old| old.paths().workdir != repo.paths().workdir) {
            self.inner.unembed();
        }
        self.inner.aids.set_repo(repo);
    }

    /// 確かめる窓や、知らせに使う。
    pub fn set_actions(&self, actions: Rc<Actions>) {
        crate::appmenu::retitle_on_show(&self.inner.commit_menu, &actions);
        // 上流がなければ、Commit ▾ のツールチップも Commit and Publish（macOS 版と同じ）。
        let (menu, weak) = (self.inner.commit_menu.downgrade(), Rc::downgrade(&actions));
        let retip = move || {
            let (Some(menu), Some(actions)) = (menu.upgrade(), weak.upgrade()) else {
                return;
            };
            menu.set_tooltip_text(Some(&if actions.publishes() {
                tr!("Commit and Publish (Ctrl+Shift+Return), Amend")
            } else {
                tr!("Commit and Push (Ctrl+Shift+Return), Amend")
            }));
        };
        retip();
        actions.connect_publishes_changed(retip);
        self.inner.actions.replace(Some(actions));
        self.inner.update_buttons();
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
        Inner::perform(&self.inner, move |repo| {
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
        this.selection.connect_selection_changed(move |_, _, _| {
            let Some(this) = weak.upgrade() else { return };
            if this.rebuilding.get() {
                return;
            }
            // クリック・キーでフォーカスが移り終えてから、どの行かを読む（続けて変わったら 1 回だけ）。
            let generation = this.follow.get() + 1;
            this.follow.set(generation);
            let weak = Rc::downgrade(&this);
            glib::idle_add_local_once(move || {
                if let Some(this) = weak.upgrade()
                    && this.follow.get() == generation
                {
                    Inner::follow_selection(&this);
                }
            });
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
        this.diff.connect_mark(move |rows| {
            let Some(this) = weak.upgrade() else { return };
            Inner::set_rows(&this, rows, None);
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
        // 差分の行の右クリック: 選んだ行のコピー、AI でこの hunk を解説。
        let weak = Rc::downgrade(this);
        this.diff.connect_row_menu(move |row, x, y| {
            let Some(this) = weak.upgrade() else { return };
            let mut items = this.diff.copy_items();
            items.extend(this.hunk_item(row));
            if !items.is_empty() {
                crate::menu::popup(&this.diff, x, y, items);
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
        this.staged_side.connect_toggled(move |side| {
            let Some(this) = weak.upgrade() else { return };
            // 出している差分に合わせただけなら、読み直さない。
            let showing = this.current.borrow().as_ref().and_then(|c| {
                c.0.as_ref()
                    .map(|w| (w.entry.path.clone(), w.staged == side.is_active()))
            });
            if let Some((path, true)) = showing
                && this.selected.borrow().as_deref() == Some(path.as_str())
            {
                return;
            }
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
        // カーソルの前が `#` になったら（打ったときも、消して `#` の後ろに戻ったときも）、開いている Issue・PR の
        // 番号を選べる（macOS 版と同じ。全角の ＃ でもよい。前の文字は `offer_issues` が確かめる）。
        let weak = Rc::downgrade(this);
        // Entry の文字は中の GtkText が持ち、insert-text もそこに来る。
        let editable: gtk::Editable = this
            .subject
            .delegate()
            .unwrap_or_else(|| this.subject.clone().upcast());
        editable.connect_insert_text(move |_, _, _| {
            if let Some(this) = weak.upgrade() {
                Inner::offer_issues(&this, false);
            }
        });
        let weak = Rc::downgrade(this);
        editable.connect_delete_text(move |_, _, _| {
            if let Some(this) = weak.upgrade() {
                Inner::offer_issues(&this, false);
            }
        });
        let weak = Rc::downgrade(this);
        this.body.buffer().connect_insert_text(move |_, _, _| {
            if let Some(this) = weak.upgrade() {
                Inner::offer_issues(&this, true);
            }
        });
        let weak = Rc::downgrade(this);
        this.body.buffer().connect_delete_range(move |_, _, _| {
            if let Some(this) = weak.upgrade() {
                Inner::offer_issues(&this, true);
            }
        });
        // Description だけでもコミットできるので、本文の変化でもボタンを直す（macOS 版と同じ）。
        let weak = Rc::downgrade(this);
        this.body.buffer().connect_changed(move |_| {
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

    /// 文字の位置（Summary か Description）の、カーソルの前の 1 文字（文字の数え方の位置と、その文字）。
    fn char_before_cursor(&self, body: bool) -> Option<(i32, char)> {
        let (text, at) = if body {
            let buffer = self.body.buffer();
            let at = buffer.iter_at_mark(&buffer.get_insert()).offset();
            let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
            (text.to_string(), at)
        } else {
            (self.subject.text().to_string(), self.subject.position())
        };
        let c = text.chars().nth(usize::try_from(at - 1).ok()?)?;
        Some((at, c))
    }

    /// `#` を打った直後: 開いている Issue・PR の一覧を出し、選んだら `#番号` を入れる（全角の ＃ は半角に直す）。
    /// 読んでいる間に打ち進めていたら出さない（macOS 版と同じ）。
    fn offer_issues(this: &Rc<Self>, body: bool) {
        let Some(actions) = this.actions.borrow().clone() else {
            return;
        };
        let weak = Rc::downgrade(this);
        // 文字が入り終えてから位置を読む。
        glib::idle_add_local_once(move || {
            let Some(this) = weak.upgrade() else { return };
            let Some((at, _)) = this
                .char_before_cursor(body)
                .filter(|(_, c)| matches!(c, '#' | '＃'))
            else {
                return;
            };
            let weak = Rc::downgrade(&this);
            glib::spawn_future_local(async move {
                let items = crate::github::issue_candidates(&actions).await;
                let Some(this) = weak.upgrade() else { return };
                if items.is_empty() || this.char_before_cursor(body).map(|(p, _)| p) != Some(at) {
                    return;
                }
                let menu: Vec<crate::menu::Item> = items
                    .into_iter()
                    .take(30)
                    .map(|(number, title)| {
                        let w = Rc::downgrade(&this);
                        crate::menu::action(format!("#{number}  {title}"), move || {
                            if let Some(this) = w.upgrade() {
                                this.put_issue(body, at, number);
                            }
                        })
                    })
                    .collect();
                // 文字の位置の下に出す。
                let (x, y) = if body {
                    let buffer = this.body.buffer();
                    let iter = buffer.iter_at_offset(at);
                    let rect = this.body.iter_location(&iter);
                    let (x, y) = this.body.buffer_to_window_coords(
                        gtk::TextWindowType::Widget,
                        rect.x(),
                        rect.y() + rect.height(),
                    );
                    (x as f64, y as f64)
                } else {
                    // 件名の欄でも、文字の位置の下（中の GtkText のカーソルの位置から）。
                    let text = this.subject.delegate().and_downcast::<gtk::Text>();
                    let x = text
                        .and_then(|t| {
                            let (strong, _) = t.compute_cursor_extents(at as usize);
                            let point = gtk::graphene::Point::new(strong.x(), 0.0);
                            t.compute_point(&this.subject, &point)
                        })
                        .map_or(8.0, |p| p.x() as f64);
                    (x, this.subject.height() as f64)
                };
                if body {
                    crate::menu::popup(&this.body, x, y, menu);
                } else {
                    crate::menu::popup(&this.subject, x, y, menu);
                }
            });
        });
    }

    /// `at`（`#` の直後）の `#`（全角でも）を `#番号` にする。
    fn put_issue(&self, body: bool, at: i32, number: u64) {
        if self.char_before_cursor(body).map(|(p, _)| p) != Some(at) {
            return;
        }
        let text = format!("#{number}");
        if body {
            let buffer = self.body.buffer();
            let mut start = buffer.iter_at_offset(at - 1);
            let mut end = buffer.iter_at_offset(at);
            buffer.delete(&mut start, &mut end);
            buffer.insert(&mut start, &text);
            self.body.grab_focus();
        } else {
            self.subject.delete_text(at - 1, at);
            let mut position = at - 1;
            self.subject.insert_text(&text, &mut position);
            self.subject.grab_focus();
            self.subject.set_position(position);
        }
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

    /// `source` で読んだ status を出す。その間に書き込む先の worktree を替えていたら捨てる（古い worktree の
    /// 一覧で、今の worktree のマージエディタを捨ててしまわないように。新しい worktree の status は別に届く）。
    fn show_from(this: &Rc<Self>, source: &Repository, status: Arc<StatusSnapshot>) {
        let workdir = this.repo().and_then(|r| r.paths().workdir);
        if source.paths().workdir != workdir {
            return;
        }
        Inner::show(this, status);
    }

    fn show(this: &Rc<Self>, status: Arc<StatusSnapshot>) {
        if this
            .status
            .borrow()
            .as_ref()
            .is_some_and(|s| Arc::ptr_eq(s, &status))
        {
            // status は同じでも、作業ツリーのファイルが変わった知らせ（ほかのエディタでマーカーを消した、
            // 変更のあるファイルをさらに書き換えた）なので、コンフリクトの中身・行数・出している差分は読み直す
            // （macOS 版は読み直すたびに一覧を作り直し、差分を位置を保って読み直す）。
            Inner::load_conflict_states(this);
            Inner::load_stats(this);
            let shown = this.selected.borrow().as_ref().and_then(|p| {
                this.entries
                    .borrow()
                    .iter()
                    .find(|e| &e.path == p && e.kind != StatusKind::Unmerged)
                    .cloned()
            });
            if let Some(entry) = shown {
                Inner::reload_diff(this, entry);
            }
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
        Inner::load_conflict_states(this);
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
        // 一覧にフォーカスがあれば、作り直した後も選んだ行に戻す（Space・Enter で続けて含める・外せるように）。
        let focused = this.list.root().and_then(|r| r.focus()).is_some_and(|f| {
            &f == this.list.upcast_ref::<gtk::Widget>() || f.is_ancestor(&this.list)
        });
        // ディレクトリの行を選んでいたら、作り直した後もその行に戻す（畳む・開く、Space で含める）。
        let kept_dir = this
            .focused_position()
            .filter(|&p| this.selection.is_selected(p as u32))
            .and_then(|p| match this.rows.borrow().get(p) {
                Some(Row::Dir { path, .. }) => Some(path.clone()),
                _ => None,
            });
        // 選んでいた行（いくつでも）。作り直した後も、一覧に残っているものは選んだままにする。
        let kept: Vec<RowKey> = {
            let rows = this.rows.borrow();
            this.selected_positions()
                .into_iter()
                .filter_map(|p| rows.get(p).map(Row::key))
                .collect()
        };
        this.rebuilding.set(true);
        // 待っている選び直しは捨てる（ここで選び直す）。
        this.follow.set(this.follow.get() + 1);
        let (rows, row_entries) = {
            let collapsed = this.collapsed.borrow();
            list_rows(&entries, this.layout.get(), &collapsed)
        };
        // 中身の変わった所だけを入れ替える（変わらない行は部品も選択もそのまま）。
        let (start, old_end, new_end) = changed_span(&this.rows.borrow(), &rows);
        // 同じファイル・ディレクトリの行は、中身（状態・チェック）だけを入れ替える。入れ替えると GtkListView
        // は行を消して足したと見て、その行がスクロールの基準なら隣の行が代わりに来て一覧がずれる（Space で
        // 含めるたびに 1 行ずつ動いた）。
        let (head, tail) = {
            let old = &this.rows.borrow()[start..old_end];
            let new = &rows[start..new_end];
            let head = old
                .iter()
                .zip(new)
                .take_while(|(a, b)| a.same_place(b))
                .count();
            let tail = old[head..]
                .iter()
                .rev()
                .zip(new[head..].iter().rev())
                .take_while(|(a, b)| a.same_place(b))
                .count();
            (head, tail)
        };
        let replace = |at: usize, row: &Row| {
            if let Some(object) = this
                .store
                .item(at as u32)
                .and_downcast::<glib::BoxedAnyObject>()
            {
                object.replace(row.clone());
            }
        };
        for i in 0..head {
            replace(start + i, &rows[start + i]);
        }
        for j in 0..tail {
            replace(old_end - 1 - j, &rows[new_end - 1 - j]);
        }
        let added: Vec<glib::BoxedAnyObject> = rows[start + head..new_end - tail]
            .iter()
            .cloned()
            .map(glib::BoxedAnyObject::new)
            .collect();
        this.rows.replace(rows);
        this.row_entries.replace(row_entries);
        this.store.splice(
            (start + head) as u32,
            (old_end - tail - start - head) as u32,
            &added,
        );
        if head + tail > 0 {
            this.rebind_cells();
        }
        // 選び直すので、残った行の選択も外す（作り直していたときと同じ）。
        this.selection.unselect_all();
        // 空のときの文（macOS 版と同じ）。
        this.empty.set_visible(this.store.n_items() == 0);
        this.empty
            .set_text(&if this.all_entries.borrow().is_empty() {
                tr!("No changes")
            } else {
                tr!("No matching files")
            });
        this.set_all_check(&entries);
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
        let row_of = |i: usize| this.row_entries.borrow().iter().position(|&r| r == Some(i));
        // 差分に出していたファイルがなくなれば、ほかに選んでいて一覧に残ったファイル。
        let found = wanted
            .and_then(|p| entries.iter().position(|e| e.path == p))
            .or_else(|| {
                entries.iter().enumerate().position(|(i, e)| {
                    row_of(i).is_some() && kept.contains(&RowKey::File(e.path.clone()))
                })
            });
        let index = found.unwrap_or(0);
        this.entries.replace(entries);
        // そのファイルの行。畳んだディレクトリの中に隠れたら、行は選ばず差分もそのまま（macOS 版の
        // NSOutlineView と同じ）。選んでいたファイルがなくなったときだけ、最初のファイルの行に移る。
        let hidden = found.is_some_and(|i| row_of(i).is_none());
        let row_index = if hidden {
            None
        } else {
            row_of(index).or_else(|| this.row_entries.borrow().iter().position(Option::is_some))
        };
        let index = row_index
            .and_then(|r| this.row_entries.borrow()[r])
            .unwrap_or(index);
        if let Some(row) = row_index {
            this.selection.select_item(row as u32, true);
            if focused {
                this.list
                    .scroll_to(row as u32, gtk::ListScrollFlags::FOCUS, None);
            }
            this.rebuilding.set(false);
            // 選び直しでは選択の変化を扱わないので、差分は自分で読み直す。
            let entry = this.entries.borrow()[index].clone();
            let same = this.selected.borrow().as_deref() == Some(entry.path.as_str());
            this.selected.replace(Some(entry.path.clone()));
            Inner::load_diff(this, entry, same);
        } else if hidden {
            this.rebuilding.set(false);
        } else {
            this.rebuilding.set(false);
            this.selected.replace(None);
            this.current.replace(None);
            this.path.set_text("");
            // 変更がひとつもなければその旨（macOS 版と同じ）。
            if this.all_entries.borrow().is_empty() {
                this.diff.show_message(&tr!("No changes"));
            } else {
                this.diff.clear();
            }
        }
        // 選んでいたディレクトリの行があれば、そこを選び直す（差分は選んでいたファイルのまま）。
        let dir_row = kept_dir.and_then(|dir| {
            this.rows
                .borrow()
                .iter()
                .position(|r| matches!(r, Row::Dir { path, .. } if *path == dir))
        });
        if let Some(row) = dir_row {
            this.rebuilding.set(true);
            this.selection.select_item(row as u32, true);
            if focused {
                this.list
                    .scroll_to(row as u32, gtk::ListScrollFlags::FOCUS, None);
            }
            this.rebuilding.set(false);
        }
        // ほかに選んでいた行も選び直す（差分とフォーカスは上で決めた行のまま）。
        this.rebuilding.set(true);
        for row in rows_to_reselect(&kept, &this.rows.borrow()) {
            this.selection.select_item(row as u32, false);
        }
        this.rebuilding.set(false);
        this.update_buttons();
    }

    /// すべてのチェックを、一覧に出しているファイル `entries` の状態に合わせる（押すとそれらを含める・外す）。
    fn set_all_check(&self, entries: &[StatusEntry]) {
        let (on, mixed, enabled) = all_check(entries);
        self.all.set_inconsistent(mixed);
        self.all.set_active(on);
        self.all.set_sensitive(enabled);
    }

    /// チェックを押した後、すべてのチェックと見えている行のチェックを index の状態から入れ直す。
    /// 押した時点で GTK がチェックを進めるが、何も変わらなかった（失敗した・断られた）ときは status が
    /// 同じで一覧を入れ直さないので、押した見た目が残ってしまう（macOS 版の `toggled` と同じく、正しい
    /// 状態は index を読み直して決める）。
    fn restore_checks(&self) {
        self.rebuilding.set(true);
        self.set_all_check(&self.entries.borrow());
        self.rebuilding.set(false);
        self.refresh_cells();
    }

    /// 選んだ行が変わった: 差分に出すのは、クリックした（フォーカスのある）行。なければ選んだ最初の行。
    fn follow_selection(this: &Rc<Self>) {
        let focused = this
            .focused_position()
            .filter(|&p| this.selection.is_selected(p as u32));
        let first = || {
            let bits = this.selection.selection();
            (!bits.is_empty()).then(|| bits.minimum() as usize)
        };
        let Some(row) = focused.or_else(first) else {
            return;
        };
        let index = this.row_entries.borrow().get(row).copied().flatten();
        let entry = index.and_then(|i| this.entries.borrow().get(i).cloned());
        if let Some(entry) = entry {
            let same = this.selected.borrow().as_deref() == Some(entry.path.as_str());
            this.selected.replace(Some(entry.path.clone()));
            Inner::load_diff(this, entry, same);
        }
    }

    /// 一覧でフォーカスのある行（なければ None）。
    fn focused_position(&self) -> Option<usize> {
        let focus = self.list.root().and_then(|r| r.focus())?;
        if &focus == self.list.upcast_ref::<gtk::Widget>() || !focus.is_ancestor(&self.list) {
            return None;
        }
        self.cells.borrow().values().find_map(|cell| {
            let inside = focus == *cell.line.upcast_ref::<gtk::Widget>()
                || focus.is_ancestor(&cell.line)
                || cell.line.is_ancestor(&focus);
            let position = cell.list_item.upgrade()?.position();
            (inside && position != gtk::INVALID_LIST_POSITION).then_some(position as usize)
        })
    }

    /// 選んでいる行（並びの順）。
    fn selected_positions(&self) -> Vec<usize> {
        let bits = self.selection.selection();
        (0..bits.size())
            .map(|i| bits.nth(i as u32) as usize)
            .collect()
    }

    /// 一覧の行の部品を作る・中身を入れる（見えている行の分だけ。スクロールすると使い回す）。
    fn install_factory(this: &Rc<Self>, factory: &gtk::SignalListItemFactory) {
        let weak = Rc::downgrade(this);
        factory.connect_setup(move |_, item| {
            let (Some(this), Some(item)) = (weak.upgrade(), item.downcast_ref::<gtk::ListItem>())
            else {
                return;
            };
            let cell = Inner::new_cell(&this, item);
            item.set_child(Some(&cell.line));
            this.cells.borrow_mut().insert(cell.line.clone(), cell);
        });
        let weak = Rc::downgrade(this);
        factory.connect_bind(move |_, item| {
            let (Some(this), Some(item)) = (weak.upgrade(), item.downcast_ref::<gtk::ListItem>())
            else {
                return;
            };
            let (Some(line), Some(object)) = (
                item.child().and_downcast::<gtk::Box>(),
                item.item().and_downcast::<glib::BoxedAnyObject>(),
            ) else {
                return;
            };
            let Some(cell) = this.cells.borrow().get(&line).cloned() else {
                return;
            };
            let row = object.borrow::<Row>().clone();
            cell.item.replace(Some(row));
            this.fill_cell(&cell);
        });
        let weak = Rc::downgrade(this);
        factory.connect_unbind(move |_, item| {
            let (Some(this), Some(item)) = (weak.upgrade(), item.downcast_ref::<gtk::ListItem>())
            else {
                return;
            };
            if let Some(line) = item.child().and_downcast::<gtk::Box>()
                && let Some(cell) = this.cells.borrow().get(&line)
            {
                cell.item.replace(None);
            }
        });
        let weak = Rc::downgrade(this);
        factory.connect_teardown(move |_, item| {
            let (Some(this), Some(item)) = (weak.upgrade(), item.downcast_ref::<gtk::ListItem>())
            else {
                return;
            };
            if let Some(line) = item.child().and_downcast::<gtk::Box>() {
                this.cells.borrow_mut().remove(&line);
            }
        });
    }

    /// 行の部品: ▸・▾（ディレクトリ）、チェック（次のコミットに含めるか）、状態の文字、名前、薄い
    /// 注記、行数、変更を捨てるボタン。どの行を出しているかは押したときに `item` で見る（使い回すので）。
    fn new_cell(this: &Rc<Self>, list_item: &gtk::ListItem) -> Rc<RowCell> {
        let arrow = gtk::Label::new(None);
        arrow.add_css_class("dim-label");
        let check = gtk::CheckButton::new();
        let state = gtk::Label::new(None);
        state.set_width_chars(2);
        state.add_css_class("monospace");
        let name = gtk::Label::new(None);
        name.set_xalign(0.0);
        name.set_ellipsize(gtk::pango::EllipsizeMode::Start);
        let detail = gtk::Label::new(None);
        detail.set_xalign(0.0);
        detail.set_ellipsize(gtk::pango::EllipsizeMode::Start);
        detail.add_css_class("dim-label");
        detail.add_css_class("caption");
        // 行数（+ 追加 − 削除）。まだ読めていなければ、届いたときに入れる。
        let counts = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let discard = gtk::Button::new();
        let icon = crate::icon::Icon::default();
        icon.set(crate::icon::Kind::Undo, false);
        discard.set_child(Some(&icon));
        discard.add_css_class("flat");
        discard.set_tooltip_text(Some(&tr!(
            "Discard the working tree changes (return to the content of the index)"
        )));
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        for w in [
            arrow.upcast_ref::<gtk::Widget>(),
            check.upcast_ref(),
            state.upcast_ref(),
            name.upcast_ref(),
            detail.upcast_ref(),
            counts.upcast_ref(),
            discard.upcast_ref(),
        ] {
            line.append(w);
        }
        let cell = Rc::new(RowCell {
            item: RefCell::default(),
            list_item: list_item.downgrade(),
            line,
            arrow,
            check,
            state,
            name,
            detail,
            counts,
            discard,
            can_discard: Cell::new(false),
            binding: Cell::new(false),
        });
        reveal_discard(&cell.discard, false);
        // チェック: ファイルなら含める・外す、ディレクトリなら中のファイル（一覧に出しているもののうち
        // コンフリクト以外）をまとめて（macOS 版と同じ）。
        let (weak, c) = (Rc::downgrade(this), Rc::downgrade(&cell));
        cell.check.connect_toggled(move |_| {
            let (Some(this), Some(cell)) = (weak.upgrade(), c.upgrade()) else {
                return;
            };
            if this.rebuilding.get() || cell.binding.get() {
                return;
            }
            let item = cell.item.borrow().clone();
            match item {
                Some(Row::File { entry, .. }) => Inner::toggle_entry(&this, &entry),
                Some(Row::Dir { path, check, .. }) => Inner::toggle_dir(&this, &path, check.all()),
                None => {}
            }
        });
        // ▸・▾ のクリックで畳む・開く（macOS 版の NSOutlineView の三角と同じ）。
        let click = gtk::GestureClick::new();
        let (weak, c) = (Rc::downgrade(this), Rc::downgrade(&cell));
        click.connect_released(move |gesture, _, _, _| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            let (Some(this), Some(cell)) = (weak.upgrade(), c.upgrade()) else {
                return;
            };
            let item = cell.item.borrow().clone();
            if let Some(Row::Dir {
                path, collapsed, ..
            }) = item
            {
                Inner::set_collapsed(&this, &path, !collapsed);
            }
        });
        cell.arrow.add_controller(click);
        // 変更を捨てるボタン（マウスを載せたときだけ出す）。
        let (weak, c) = (Rc::downgrade(this), Rc::downgrade(&cell));
        cell.discard.connect_clicked(move |_| {
            let (Some(this), Some(cell)) = (weak.upgrade(), c.upgrade()) else {
                return;
            };
            let item = cell.item.borrow().clone();
            if let Some(Row::File { entry, .. }) = item {
                Inner::discard_file(&this, &entry);
            }
        });
        let motion = gtk::EventControllerMotion::new();
        let c = Rc::downgrade(&cell);
        motion.connect_enter(move |_, _, _| {
            if let Some(cell) = c.upgrade() {
                reveal_discard(&cell.discard, cell.can_discard.get());
            }
        });
        let c = Rc::downgrade(&cell);
        motion.connect_leave(move |_| {
            if let Some(cell) = c.upgrade() {
                reveal_discard(&cell.discard, false);
            }
        });
        cell.line.add_controller(motion);
        // 右クリック（macOS 版と同じ）: 含める・外す・捨てる、.gitignore に足す（untracked）、Blame（消した
        // ファイルにはない）と File History（untracked にはまだ履歴がない）、パスのコピー、ファイルで開く。
        let menu = gtk::GestureClick::new();
        menu.set_button(gtk::gdk::BUTTON_SECONDARY);
        let (weak, c) = (Rc::downgrade(this), Rc::downgrade(&cell));
        menu.connect_pressed(move |_, _, x, y| {
            let (Some(this), Some(cell)) = (weak.upgrade(), c.upgrade()) else {
                return;
            };
            let item = cell.item.borrow().clone();
            if let Some(Row::File { entry, .. }) = item {
                let items = Inner::file_menu(&this, &entry);
                crate::menu::popup(cell.line.upcast_ref::<gtk::Widget>(), x, y, items);
            }
        });
        cell.line.add_controller(menu);
        cell
    }

    /// 行の部品に、出している行の中身を入れる。
    fn fill_cell(&self, cell: &RowCell) {
        let Some(item) = cell.item.borrow().clone() else {
            return;
        };
        cell.binding.set(true);
        reveal_discard(&cell.discard, false);
        match item {
            Row::Dir {
                name,
                depth,
                collapsed,
                check,
                ..
            } => {
                cell.line.set_margin_start((depth * 16) as i32);
                cell.arrow.set_text(if collapsed { "▸" } else { "▾" });
                cell.arrow.set_visible(true);
                let (all, mixed) = (check.all(), check.mixed());
                cell.check.set_active(all);
                cell.check.set_inconsistent(mixed);
                // 中に含められるファイルがなければ（コンフリクトだけ）チェックを出さない（macOS 版と同じ）。
                cell.check.set_visible(check.files > 0);
                cell.check.set_tooltip_text(Some(&check_tip(all, mixed)));
                cell.state.set_visible(false);
                // 名前は「/」を付けて薄く（macOS 版と同じ）。
                cell.name.set_text(&format!("{name}/"));
                cell.name.add_css_class("dim-label");
                cell.name.set_hexpand(true);
                cell.detail.set_visible(false);
                cell.detail.set_hexpand(false);
                cell.counts.set_visible(false);
                cell.can_discard.set(false);
                cell.discard.set_visible(false);
                cell.line.set_tooltip_text(None);
            }
            Row::File {
                entry,
                layout,
                depth,
            } => {
                let conflict = entry.kind == StatusKind::Unmerged;
                cell.line.set_margin_start((depth * 16) as i32);
                cell.arrow.set_visible(false);
                cell.name.remove_css_class("dim-label");
                let on = entry.is_staged() && !entry.is_unstaged();
                let mixed = entry.is_staged() && entry.is_unstaged();
                cell.check.set_active(on);
                cell.check.set_inconsistent(mixed);
                // コンフリクトのファイルにはチェックを出さない（index は両方の側で「一部」に見える。macOS 版と同じ）。
                cell.check.set_visible(!conflict);
                cell.check
                    .set_tooltip_text((!conflict).then(|| check_tip(on, mixed)).as_deref());
                let (letter, css) = state_letter(&entry);
                cell.state.set_text(&letter);
                for c in ["error", "success", "accent", "warning"] {
                    cell.state.remove_css_class(c);
                }
                cell.state.add_css_class(css);
                cell.state.set_visible(true);
                let (parent, file) = match entry.path.rsplit_once('/') {
                    Some((p, f)) => (p, f),
                    None => ("", entry.path.as_str()),
                };
                // 名前を変えたファイルのもとのパスは、ツールチップだけに出す（macOS 版と同じ）。
                cell.name.set_text(match layout {
                    Layout::List => entry.path.as_str(),
                    _ => file,
                });
                // 名前の後ろの薄い注記: Flat では親のディレクトリ、マーカーの残っていないコンフリクトでは
                // 「rerere」か「no markers」（macOS 版と同じ）。
                let plain_detail = if layout == Layout::Flat { parent } else { "" };
                cell.detail.set_text(plain_detail);
                cell.detail.set_visible(!plain_detail.is_empty());
                let flat_parent = layout == Layout::Flat && !parent.is_empty();
                cell.detail.set_hexpand(flat_parent);
                cell.name.set_hexpand(!flat_parent);
                fill_counts(&cell.counts, self.stats.borrow().get(&entry.path).copied());
                cell.can_discard.set(!conflict && entry.is_unstaged());
                // 捨てられないファイル（すべて含めた・コンフリクト）でも幅は取っておく（行数の位置を揃える）。
                cell.discard.set_visible(true);
                // パス（名前を変えたファイルはもとのパス → 今のパス。macOS 版と同じ）。
                let tip = match &entry.orig_path {
                    Some(orig) => format!("{orig} → {}", entry.path),
                    None => entry.path.clone(),
                };
                cell.line.set_tooltip_text(Some(&tip));
                if conflict {
                    self.apply_conflict_state(cell, &entry.path, plain_detail, &tip);
                }
            }
        }
        cell.binding.set(false);
    }

    /// 見えている行の中身を入れ直す（行数・コンフリクトの状態が届いたとき。一覧は作り直さない）。
    /// 見えている行の部品に、今の行の中身を入れ直す（行を入れ替えずに中身だけを変えたとき）。
    fn rebind_cells(&self) {
        let cells: Vec<Rc<RowCell>> = self.cells.borrow().values().cloned().collect();
        for cell in cells {
            let row = cell
                .list_item
                .upgrade()
                .and_then(|item| item.item())
                .and_downcast::<glib::BoxedAnyObject>()
                .map(|object| object.borrow::<Row>().clone());
            if let Some(row) = row {
                cell.item.replace(Some(row));
                self.fill_cell(&cell);
            }
        }
    }

    fn refresh_cells(&self) {
        let cells: Vec<Rc<RowCell>> = self.cells.borrow().values().cloned().collect();
        for cell in cells {
            self.fill_cell(&cell);
        }
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
            // 一覧と差分は作り直さない（見えている行の行数を埋めるだけ）。
            this.refresh_cells();
        });
    }

    /// コンフリクトのファイルの中身を読み直す（index が変わらなくても、ほかのエディタでマーカーを消せば変わる）。
    /// 変わっていれば、一覧の印を直す（macOS 版と同じ）。
    fn load_conflict_states(this: &Rc<Self>) {
        let any = this
            .all_entries
            .borrow()
            .iter()
            .any(|e| e.kind == StatusKind::Unmerged);
        let Some(repo) = this.repo().filter(|_| any) else {
            this.conflict_states.borrow_mut().clear();
            return;
        };
        let weak = Rc::downgrade(this);
        glib::spawn_future_local(async move {
            let reply = crate::sheets::run(&repo, json!({ "op": "conflictFiles" })).await;
            let Some(files) = reply["info"]["files"].as_array() else {
                return;
            };
            let states: HashMap<String, ConflictState> = files
                .iter()
                .filter_map(|f| {
                    Some((
                        f["path"].as_str()?.to_owned(),
                        ConflictState {
                            markers: f["markers"].as_u64()?,
                            rerere: f["rerere"].as_bool().unwrap_or(false),
                        },
                    ))
                })
                .collect();
            let Some(this) = weak.upgrade() else { return };
            if *this.conflict_states.borrow() == states {
                return;
            }
            let old = this.conflict_states.replace(states.clone());
            this.refresh_cells();
            // ほかで書き換えたファイル（マーカーの数が変わった）のマージエディタは、手を付けていなければ読み直す。
            // 前の中身のまま「保存して解決済みにする」と、ほかでした解決をマーカー付きの中身で上書きしてしまう。
            let changed: Vec<String> = states
                .iter()
                .filter(|(path, state)| old.get(*path).is_some_and(|o| o != *state))
                .map(|(path, _)| path.clone())
                .collect();
            for path in changed {
                let key = this.editor_key(&path);
                let editor = this.editors.borrow().get(&key).cloned();
                let Some(editor) = editor.filter(|e| !e.has_unsaved_changes()) else {
                    continue;
                };
                this.editors.borrow_mut().remove(&key);
                let shown = this
                    .embedded
                    .borrow()
                    .as_ref()
                    .is_some_and(|e| Rc::ptr_eq(e, &editor));
                if shown {
                    this.unembed();
                    Inner::show_conflict(&this, &path, this.generation.get());
                }
            }
        });
    }

    /// コンフリクトの行の印: マーカーが残っていなければ、解決できていそうなことを橙の ✓ と短い注記で示す。
    /// `plain_detail` は印がないときの注記（Flat の親のディレクトリ）、`tip` は行の説明。
    fn apply_conflict_state(&self, cell: &RowCell, path: &str, plain_detail: &str, tip: &str) {
        let state = self
            .conflict_states
            .borrow()
            .get(path)
            .copied()
            .filter(|s| s.markers == 0);
        match state {
            Some(state) => {
                cell.state.set_text("✓");
                cell.state.remove_css_class("error");
                cell.state.add_css_class("warning");
                cell.detail.set_text(&if state.rerere {
                    "rerere".to_owned()
                } else {
                    tr!("no markers")
                });
                cell.detail.set_visible(true);
                let message = if state.rerere {
                    tr!(
                        "git rerere applied the earlier resolution. Check the result, then Mark Resolved."
                    )
                } else {
                    tr!("No conflict markers are left. Check the result, then Mark Resolved.")
                };
                cell.line
                    .set_tooltip_text(Some(&format!("{tip}\n{message}")));
            }
            None => {
                cell.state.set_text("⚠");
                cell.state.remove_css_class("warning");
                cell.state.add_css_class("error");
                cell.detail.set_text(plain_detail);
                cell.detail.set_visible(!plain_detail.is_empty());
                cell.line.set_tooltip_text(Some(tip));
            }
        }
    }

    /// 木のディレクトリ `dir` の中のファイル（一覧に出しているもののうちコンフリクト以外）を、`all`（すべて
    /// 含めている）なら外し、そうでなければ含める（macOS 版と同じ）。
    fn toggle_dir(this: &Rc<Self>, dir: &str, all: bool) {
        let paths: Vec<String> = this
            .entries
            .borrow()
            .iter()
            .filter(|e| e.kind != StatusKind::Unmerged && in_dir(&e.path, dir))
            .map(|e| e.path.clone())
            .collect();
        if paths.is_empty() {
            return;
        }
        Inner::run(this, move |repo| {
            let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
            if all {
                repo.unstage(&paths)
            } else {
                repo.stage(&paths)
            }
        });
    }

    /// 木のディレクトリ `path` を畳む・開く。
    fn set_collapsed(this: &Rc<Self>, path: &str, collapsed: bool) {
        let changed = if collapsed {
            this.collapsed.borrow_mut().insert(path.to_owned())
        } else {
            this.collapsed.borrow_mut().remove(path)
        };
        if changed {
            Inner::rebuild_list(this);
        }
    }

    /// 木の ←・→（macOS 版の NSOutlineView と同じ）: ← は開いたディレクトリを畳み、そうでなければ親の
    /// ディレクトリの行に移る。→ は畳んだディレクトリを開く。扱ったら true。
    fn tree_key(this: &Rc<Self>, key: gtk::gdk::Key) -> bool {
        use gtk::gdk::Key;
        if this.layout.get() != Layout::Tree {
            return false;
        }
        let Some(at) = this.focused_position() else {
            return false;
        };
        let Some((depth, dir)) = this.rows.borrow().get(at).map(Row::tree_place) else {
            return false;
        };
        let folded = dir
            .as_ref()
            .is_some_and(|d| this.collapsed.borrow().contains(d));
        match key {
            Key::Left | Key::KP_Left => match dir {
                Some(dir) if !folded => Inner::set_collapsed(this, &dir, true),
                _ => {
                    let parent = this.rows.borrow()[..at]
                        .iter()
                        .rposition(|r| matches!(r, Row::Dir { depth: d, .. } if *d < depth));
                    match parent {
                        Some(parent) => this.list.scroll_to(
                            parent as u32,
                            gtk::ListScrollFlags::SELECT | gtk::ListScrollFlags::FOCUS,
                            None,
                        ),
                        None => this.list.error_bell(),
                    }
                }
            },
            Key::Right | Key::KP_Right => match dir {
                Some(dir) if folded => Inner::set_collapsed(this, &dir, false),
                _ => return false,
            },
            _ => return false,
        }
        true
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
        if Inner::tree_key(this, key) {
            return true;
        }
        if matches!(key, Key::space | Key::Return | Key::KP_Enter) {
            // ディレクトリの行だけを選んでいても、その中のファイルを含める・外す。
            if this.selection.selection().is_empty() {
                return false;
            }
            Inner::toggle_selected(this);
            return true;
        }
        let Some(entry) = this.selected_entry() else {
            return false;
        };
        match key {
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

    /// Space・Enter: 一覧で選んだファイル（コンフリクトを除く）を、すべて含めていれば外し、そうでなければ含める
    /// （macOS 版と同じ）。含める・外すものがなければ音で知らせる。
    fn toggle_selected(this: &Rc<Self>) {
        let targets: Vec<StatusEntry> = {
            let row_entries = this.row_entries.borrow();
            let rows = this.rows.borrow();
            let entries = this.entries.borrow();
            let mut indices = Vec::new();
            for at in this.selected_positions() {
                if let Some(i) = row_entries.get(at).copied().flatten() {
                    indices.push(i);
                } else if let Some(Row::Dir { path: dir, .. }) = rows.get(at) {
                    // ディレクトリの行: 中のファイル（畳んで見えていないものも。macOS 版と同じ）。
                    indices.extend(
                        entries
                            .iter()
                            .enumerate()
                            .filter_map(|(i, e)| in_dir(&e.path, dir).then_some(i)),
                    );
                }
            }
            // ディレクトリとその中のファイルを一緒に選んでいても、1 回ずつにする。
            indices.sort_unstable();
            indices.dedup();
            indices
                .into_iter()
                .filter_map(|i| entries.get(i).cloned())
                .filter(|e| e.kind != StatusKind::Unmerged)
                .collect()
        };
        if targets.is_empty() {
            this.list.error_bell();
            return;
        }
        let fully = targets.iter().all(|e| e.is_staged() && !e.is_unstaged());
        let paths: Vec<String> = targets.into_iter().map(|e| e.path).collect();
        Inner::run(this, move |repo| {
            let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
            if fully {
                repo.unstage(&paths)
            } else {
                repo.stage(&paths)
            }
        });
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
        // 今の差分のファイルの行から数える（何行も選んでいても）。
        let current = this.selected.borrow().as_deref().and_then(|path| {
            let entries = this.entries.borrow();
            rows.iter()
                .position(|r| r.is_some_and(|i| entries.get(i).is_some_and(|e| e.path == path)))
        });
        let mut i = current.map_or(-1, |c| c as i32) + step;
        while i >= 0 && (i as usize) < rows.len() {
            if rows[i as usize].is_some() {
                this.list
                    .scroll_to(i as u32, gtk::ListScrollFlags::SELECT, None);
                this.diff.grab_focus();
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
        Inner::read_diff(this, entry, same, false);
    }

    /// 出している差分を読み直す。中身（両側・含めた行）が変わったときだけ出し直す（選んだ行を外さない）。
    fn reload_diff(this: &Rc<Self>, entry: StatusEntry) {
        Inner::read_diff(this, entry, true, true);
    }

    fn read_diff(this: &Rc<Self>, entry: StatusEntry, same: bool, only_changed: bool) {
        let Some(repo) = this.repo() else { return };
        // 両側に変更のあるファイルは作業ツリーの側から。同じファイルを読み直すときだけ、今の側のまま
        // （macOS 版の `side(for:)`）。
        let reselect = this
            .current
            .borrow()
            .as_ref()
            .and_then(|c| c.0.as_ref())
            .is_some_and(|w| w.entry.path == entry.path);
        let prefer_staged = reselect && this.staged_side.is_active();
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
                Ok(Ok((working, _)))
                    if only_changed
                        && this
                            .current
                            .borrow()
                            .as_ref()
                            .and_then(|c| c.0.as_ref())
                            .zip(working.0.as_ref())
                            .is_some_and(|(a, b)| {
                                a.entry.path == b.entry.path
                                    && a.staged == b.staged
                                    && a.combined == b.combined
                                    && a.file.old == b.file.old
                                    && a.file.new == b.file.new
                            }) => {}
                Ok(Ok((working, view))) => {
                    let w = working.0.as_ref();
                    this.show_line_endings(w.and_then(|w| w.line_endings.as_ref()));
                    this.sides.set_visible(w.is_some_and(|w| {
                        w.combined.is_none() && w.entry.is_staged() && w.entry.is_unstaged()
                    }));
                    let checks = w.is_some_and(|w| w.combined.is_some());
                    this.conflict_buttons.set_visible(false);
                    let staged = w.is_some_and(|w| w.staged);
                    this.current.replace(Some(working));
                    // 切り替えを、出した側に合わせる（ほかのファイルで Staged にしたままにしない）。
                    if staged {
                        this.staged_side.set_active(true);
                    } else {
                        this.worktree_side.set_active(true);
                    }
                    this.diff.show_working(view, same, checks);
                    this.find.refresh();
                    if let Some(row) = this.toggle_once.take() {
                        Inner::set_rows(&this, vec![row as usize], None);
                    }
                    Inner::press_next_key(&this);
                }
                Ok(Err(e)) => {
                    // 前に出していたもの（コンフリクトのボタンなど）を残さない。理由は差分の場所に。
                    this.conflict_buttons.set_visible(false);
                    this.sides.set_visible(false);
                    this.show_line_endings(None);
                    this.current.replace(None);
                    this.diff
                        .show_message(&tr!("Can’t read the diff: {e}", e = e));
                }
                Err(_) => {}
            }
            this.update_buttons();
        });
    }

    /// 選んだ行（`rows`）を含める・外す。`stage` が None なら、すべて含めていれば外し、そうでなければ含める。
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
                    let stage = stage.unwrap_or_else(|| !rows.iter().all(|&r| staged[r]));
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
            Inner::perform(this, move |repo| {
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
                crate::sheets::can_undo(),
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
        let workdir = this.repo().and_then(|r| r.paths().workdir);
        if crate::merge::open_window(workdir.as_deref(), path).is_some() {
            this.embed_message(&tr!("The merge editor is open in a separate window"));
            return;
        }
        let existing = this.editors.borrow().get(&this.editor_key(path)).cloned();
        if let Some(editor) = existing {
            Inner::embed(this, &editor);
            return;
        }
        let (Some(repo), Some(actions)) = (this.repo(), this.actions.borrow().clone()) else {
            return;
        };
        this.pending.set(this.pending.get() + 1);
        let (weak, path, key) = (Rc::downgrade(this), path.to_owned(), this.editor_key(path));
        glib::spawn_future_local(async move {
            let loaded = crate::merge::load(repo, actions, &path).await;
            let Some(this) = weak.upgrade() else { return };
            this.pending.set(this.pending.get() - 1);
            if this.generation.get() != generation {
                return;
            }
            match loaded {
                Ok(editor) => {
                    this.editors.borrow_mut().insert(key, editor.clone());
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
        // 埋め込むのは、今の worktree のエディタだけ（後で切り替えても、このエディタの鍵はこれ）。
        let key = this.editor_key(editor.path());
        // 別の窓に移す（編集中の内容のまま）。
        let (weak, k) = (Rc::downgrade(this), key.clone());
        let e = Rc::downgrade(editor);
        editor.set_on_pop_out(Some(Rc::new(move || {
            let (Some(this), Some(editor)) = (weak.upgrade(), e.upgrade()) else {
                return;
            };
            let Some(window) = this.merge_holder.root().and_downcast::<gtk::Window>() else {
                return;
            };
            this.unembed();
            this.editors.borrow_mut().remove(&k);
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
            this.editors.borrow_mut().remove(&key);
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

    /// 今の worktree の `path` のマージエディタの鍵。
    fn editor_key(&self, path: &str) -> EditorKey {
        (self.repo().and_then(|r| r.paths().workdir), path.to_owned())
    }

    /// 今の worktree で、コンフリクトでなくなったファイルのマージエディタを捨てる（埋め込んでいれば外す）。
    /// ほかの worktree のものは、その worktree の status が届くまで残す。
    fn prune_editors(&self) {
        let conflicted: std::collections::HashSet<String> = self
            .all_entries
            .borrow()
            .iter()
            .filter(|e| e.kind == StatusKind::Unmerged)
            .map(|e| e.path.clone())
            .collect();
        let workdir = self.repo().and_then(|r| r.paths().workdir);
        self.editors
            .borrow_mut()
            .retain(|(w, path), _| *w != workdir || conflicted.contains(path));
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
                Inner::perform(&this, move |repo| repo.stage(&[&path]));
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
        let source = repo.clone();
        glib::spawn_future_local(async move {
            // 失敗しても status は読み直す（途中まで進んだかもしれない。macOS 版と同じ）。
            let result = gio::spawn_blocking(move || {
                let done = op(&repo).map_err(|e| e.to_string());
                (done, repo.load_status())
            })
            .await;
            let Some(this) = weak.upgrade() else { return };
            this.pending.set(this.pending.get() - 1);
            let Ok((done, status)) = result else { return };
            if let Ok(status) = status {
                Inner::show_from(&this, &source, status);
            }
            this.restore_checks();
            let actions = this.actions.borrow().clone();
            if let Some(actions) = actions {
                // 取り消せる操作が変わったかもしれない（見張りの知らせを待たない）。
                actions.refresh_undo();
                // 失敗はシートで（macOS 版の `run` と同じ）。
                if let Err(e) = done {
                    actions.alert(&e, "").await;
                }
            } else if let Err(e) = done {
                this.show_error(&e);
            }
        });
    }

    /// `op` を操作として行う（macOS 版の `actions.perform`）。失敗は操作の失敗の知らせに出し、読み直しと
    /// Undo の更新は操作の窓口に任せる。
    fn perform(
        this: &Rc<Self>,
        op: impl FnOnce(&Repository) -> amonhen_model::Result<()> + Send + 'static,
    ) {
        let actions = this.actions.borrow().clone();
        match actions {
            Some(actions) => actions.perform_work(op),
            None => Inner::run(this, op),
        }
    }

    /// コミットする。できたら `then` を呼ぶ（Commit and Push の push）。
    fn commit(this: &Rc<Self>, then: Option<Box<dyn FnOnce()>>) {
        let message = this.composed_message();
        let amend = this.amend.is_active();
        let weak = Rc::downgrade(this);
        let Some(repo) = this.repo() else { return };
        this.commit.set_sensitive(false);
        let source = repo.clone();
        this.message.set_visible(false);
        // hooks の出力を探すため、始めた時刻（Activity の記録と同じ UNIX 時刻のミリ秒）を覚えておく。
        let started = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        this.pending.set(this.pending.get() + 1);
        glib::spawn_future_local(async move {
            // できなくても status は読み直す（hooks がファイルを直していることがある。macOS 版と同じ）。
            let result = gio::spawn_blocking(move || {
                let done = repo.commit(&message, amend).map_err(|e| e.to_string());
                (done, repo.load_status().ok())
            })
            .await;
            let Some(this) = weak.upgrade() else { return };
            let parent = this.list.root().and_downcast::<gtk::Window>();
            match result {
                Ok((Ok(()), status)) => {
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
                    if let Some(status) = status {
                        Inner::show_from(&this, &source, status);
                    }
                    if let Some(then) = then {
                        then();
                    }
                }
                // hooks で止まったときは出力が長いので、読めるシートで出す。
                Ok((Err(e), status)) => {
                    hooks_sheet(parent.as_ref(), &tr!("Couldn’t Commit"), &e);
                    if let Some(status) = status {
                        Inner::show_from(&this, &source, status);
                    }
                }
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

    /// Summary と Description をつないだメッセージ（片方だけでもよい。macOS 版と同じ）。
    fn composed_message(&self) -> String {
        let subject = self.subject.text().trim().to_owned();
        let buffer = self.body.buffer();
        let body = buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .trim()
            .to_owned();
        match (subject.is_empty(), body.is_empty()) {
            (_, true) => subject,
            (true, false) => body,
            (false, false) => format!("{subject}\n\n{body}"),
        }
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
        let staged_files = staged_count(&self.all_entries.borrow());
        let staged = staged_files > 0;
        let has_message = !self.composed_message().is_empty();
        let amend = self.amend.is_active();
        let can_commit = has_message && (staged || amend);
        self.commit.set_sensitive(can_commit);
        // 含めるファイルの数も出す（macOS 版と同じ）。
        let label = match (amend, staged_files) {
            (true, _) => "Amend Commit".to_owned(),
            (false, 0) => "Commit".to_owned(),
            (false, 1) => "Commit 1 File".to_owned(),
            (false, n) => format!("Commit {n} Files"),
        };
        self.commit.set_label(&label);
        // Commit メニューの Commit・Commit and Push・Stage All も、押せるときだけ（macOS 版と同じ）。
        if let Some(actions) = self.actions.borrow().as_ref() {
            let can_stage_all = self
                .all_entries
                .borrow()
                .iter()
                .any(|e| e.kind != StatusKind::Unmerged && e.is_unstaged());
            actions.update_facts(|f| {
                f.can_commit = can_commit;
                f.can_stage_all = can_stage_all;
            });
        }
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
        // 選んだファイルがなければ、ファイルの操作は出さない（macOS 版と同じ）。
        let chosen = self.selected.borrow().is_some();
        self.lines.set_visible(!conflict && chosen);
        self.lines.set_label(if unstage {
            "Unstage Lines"
        } else {
            "Stage Lines"
        });
        self.lines.set_sensitive(selected > 0);
        self.lines.set_tooltip_text(
            working
                .is_some_and(|w| w.combined.is_some())
                .then(|| tr!("Include or exclude the selected lines in the next commit (Space)"))
                .as_deref(),
        );
        // stage 済みの側では捨てない。選んだ行がなければファイルを捨てる。
        self.discard
            .set_visible(!conflict && chosen && !staged_side);
        self.discard.set_label(if selected > 0 {
            "Discard Lines"
        } else {
            "Discard File"
        });
        self.discard.set_tooltip_text(Some(&if selected > 0 {
            tr!(
                "Discard the selected lines’ changes from the working tree (included lines are excluded first)"
            )
        } else {
            tr!(
                "Discard the working tree changes and return to the content of the next commit (the index)"
            )
        }));
        self.discard.set_sensitive(
            working.is_some_and(|w| !w.staged && w.entry.is_unstaged()) && selected_staged == 0,
        );
    }
}

/// コミットの入力とファイルの一覧の間の仕切りの位置（上の段の高さ）を覚える設定の鍵。
const MESSAGE_HEIGHT: &str = "workingCopyMessageHeight";

/// 仕切りを覚えた位置に置き、動かしたら覚える（ドラッグの間は書かず、止まってから）。
fn remember_message_height(paned: &gtk::Paned) {
    if let Some(height) = crate::settings::string(MESSAGE_HEIGHT).and_then(|s| s.parse().ok()) {
        paned.set_position(height);
    }
    let pending: Rc<RefCell<Option<glib::SourceId>>> = Rc::default();
    paned.connect_position_notify(move |paned| {
        // 自分で置いたとき・まだ大きさが決まっていないときは覚えない。
        if !paned.is_mapped() {
            return;
        }
        if let Some(id) = pending.borrow_mut().take() {
            id.remove();
        }
        let (weak, slot) = (paned.downgrade(), pending.clone());
        let id = glib::timeout_add_local_once(std::time::Duration::from_millis(400), move || {
            slot.borrow_mut().take();
            if let Some(paned) = weak.upgrade() {
                crate::settings::set_string(MESSAGE_HEIGHT, &paned.position().to_string());
            }
        });
        pending.borrow_mut().replace(id);
    });
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

/// 本文が空のときの薄い文（TextView には placeholder がないので、上に重ねる。macOS 版と同じ文）。
fn body_placeholder(body: &gtk::TextView) -> gtk::Label {
    let label = gtk::Label::new(Some(&tr!("Description (optional)")));
    label.add_css_class("dim-label");
    label.add_css_class("monospace");
    label.set_can_target(false);
    label.set_halign(gtk::Align::Start);
    label.set_valign(gtk::Align::Start);
    // 本文の 1 文字目の位置（欄の枠 1 px と余白）。
    label.set_margin_start(1 + body.left_margin());
    label.set_margin_top(1 + body.top_margin());
    let buffer = body.buffer();
    label.set_visible(buffer.char_count() == 0);
    let weak = label.downgrade();
    buffer.connect_changed(move |b| {
        if let Some(label) = weak.upgrade() {
            label.set_visible(b.char_count() == 0);
        }
    });
    label
}

/// すべてのチェックの状態（含めている, 一部, 押せる）。一覧に出しているファイルのうちコンフリクト以外を見る。
/// コンフリクトのファイルは index では stage 済みに見えるが、含めることも外すこともできない（macOS 版と同じ）。
fn all_check(entries: &[StatusEntry]) -> (bool, bool, bool) {
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
    (
        !candidates.is_empty() && on == candidates.len(),
        mixed,
        !candidates.is_empty(),
    )
}

/// 一覧のチェックの説明（含めている・一部の行だけ・含めていない。macOS 版と同じ）。
fn check_tip(on: bool, mixed: bool) -> String {
    if on {
        tr!("Included in the next commit (click to exclude)")
    } else if mixed {
        tr!("Only some lines are included (click to include all)")
    } else {
        tr!("Not included in the next commit (click to include)")
    }
}

/// 一覧の行の行数（+ 追加 − 削除）を入れ直す（0 の側は出さない。なければ隠す）。
fn fill_counts(counts: &gtk::Box, stats: Option<LineCounts>) {
    while let Some(child) = counts.first_child() {
        counts.remove(&child);
    }
    // 行数のないファイル（バイナリ）は薄く「bin」（macOS 版と同じ）。まだ読めていなければ何も出さない。
    if stats == Some((None, None)) {
        let l = gtk::Label::new(Some("bin"));
        l.add_css_class("caption");
        l.add_css_class("dim-label");
        counts.append(&l);
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

/// 一覧に出す状態の文字と、その色の CSS のクラス（macOS 版と同じ）。作業ツリーの側を優先し、作業ツリーに変更が
/// なければ index の側。untracked は U（緑）、コンフリクトは ⚠（赤）。
fn state_letter(entry: &StatusEntry) -> (String, &'static str) {
    if entry.kind == StatusKind::Unmerged {
        return ("⚠".to_owned(), "error");
    }
    let state = if entry.worktree != b'.' {
        entry.worktree
    } else {
        entry.index
    };
    match state {
        b'?' => ("U".to_owned(), "success"),
        b'A' => ("A".to_owned(), "success"),
        b'D' => ("D".to_owned(), "error"),
        b'R' => ("R".to_owned(), "accent"),
        c => ((c as char).to_string(), "warning"),
    }
}

/// 木の行（ディレクトリかファイル）。
enum TreeItem {
    Dir {
        name: String,
        path: String,
        depth: usize,
        check: DirCheck,
    },
    File {
        index: usize,
        depth: usize,
    },
}

impl TreeItem {
    fn depth(&self) -> usize {
        match self {
            TreeItem::Dir { depth, .. } | TreeItem::File { depth, .. } => *depth,
        }
    }
}

/// ディレクトリのチェックの元: 中の（コンフリクト以外の）ファイルの数、すべて含めている数、一部でも含めている数。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct DirCheck {
    files: usize,
    fully: usize,
    some: usize,
}

impl DirCheck {
    fn add(&mut self, other: DirCheck) {
        self.files += other.files;
        self.fully += other.fully;
        self.some += other.some;
    }

    fn all(self) -> bool {
        self.files > 0 && self.fully == self.files
    }

    fn mixed(self) -> bool {
        self.some > 0 && !self.all()
    }
}

/// 一覧の 1 行の中身（同じなら部品を入れ直さない）。
#[derive(Clone, Debug, PartialEq, Eq)]
enum Row {
    File {
        entry: StatusEntry,
        layout: Layout,
        depth: usize,
    },
    /// 木のディレクトリ（`name` は出す名前、`path` はパス。子が 1 つのディレクトリはつないだもの）。
    Dir {
        name: String,
        path: String,
        depth: usize,
        collapsed: bool,
        check: DirCheck,
    },
}

/// 作り直しをまたいで同じ行を探すための鍵（ファイルかディレクトリかと、そのパス）。
#[derive(Clone, Debug, PartialEq, Eq)]
enum RowKey {
    File(String),
    Dir(String),
}

/// 作り直した一覧 `rows` で、前に選んでいた行 `kept` に当たる行（なくなったものは除く）。
fn rows_to_reselect(kept: &[RowKey], rows: &[Row]) -> Vec<usize> {
    rows.iter()
        .enumerate()
        .filter(|(_, r)| kept.contains(&r.key()))
        .map(|(i, _)| i)
        .collect()
}

impl Row {
    fn key(&self) -> RowKey {
        match self {
            Row::File { entry, .. } => RowKey::File(entry.path.clone()),
            Row::Dir { path, .. } => RowKey::Dir(path.clone()),
        }
    }

    /// 同じファイル・ディレクトリの行か（状態・チェック・畳んだかは問わない）。
    fn same_place(&self, other: &Row) -> bool {
        match (self, other) {
            (
                Row::File {
                    entry: a, depth: x, ..
                },
                Row::File {
                    entry: b, depth: y, ..
                },
            ) => a.path == b.path && x == y,
            (
                Row::Dir {
                    path: a, depth: x, ..
                },
                Row::Dir {
                    path: b, depth: y, ..
                },
            ) => a == b && x == y,
            _ => false,
        }
    }

    /// 木の深さと、ディレクトリの行ならそのパス（← で親に移る・畳むのに使う）。
    fn tree_place(&self) -> (usize, Option<String>) {
        match self {
            Row::File { depth, .. } => (*depth, None),
            Row::Dir { depth, path, .. } => (*depth, Some(path.clone())),
        }
    }
}

/// 一覧の行と、行ごとの `entries` の番号（ディレクトリの行は None）。木では畳んだディレクトリの中を飛ばす。
fn list_rows(
    entries: &[StatusEntry],
    layout: Layout,
    collapsed: &std::collections::HashSet<String>,
) -> (Vec<Row>, Vec<Option<usize>>) {
    let mut rows = Vec::new();
    let mut row_entries = Vec::new();
    if layout != Layout::Tree {
        for (i, entry) in entries.iter().enumerate() {
            rows.push(Row::File {
                entry: entry.clone(),
                layout,
                depth: 0,
            });
            row_entries.push(Some(i));
        }
        return (rows, row_entries);
    }
    let mut folded_at: Option<usize> = None;
    for item in tree(entries) {
        let depth = item.depth();
        if folded_at.is_some_and(|d| depth > d) {
            continue;
        }
        folded_at = None;
        match item {
            TreeItem::Dir {
                name,
                path,
                depth,
                check,
            } => {
                let folded = collapsed.contains(&path);
                if folded {
                    folded_at = Some(depth);
                }
                rows.push(Row::Dir {
                    name,
                    path,
                    depth,
                    collapsed: folded,
                    check,
                });
                row_entries.push(None);
            }
            TreeItem::File { index, depth } => {
                rows.push(Row::File {
                    entry: entries[index].clone(),
                    layout,
                    depth,
                });
                row_entries.push(Some(index));
            }
        }
    }
    (rows, row_entries)
}

/// `old` を `new` にするのに入れ替える所: 前後の同じ所を除いた `(始め, old の終わり, new の終わり)`。
fn changed_span<T: PartialEq>(old: &[T], new: &[T]) -> (usize, usize, usize) {
    let start = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let tail = old[start..]
        .iter()
        .rev()
        .zip(new[start..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    (start, old.len() - tail, new.len() - tail)
}

/// `path` がディレクトリ `dir` の中か。
fn in_dir(path: &str, dir: &str) -> bool {
    path.strip_prefix(dir)
        .is_some_and(|rest| rest.starts_with('/'))
}

/// 行の変更を捨てるボタンを出す・隠す。隠している間は、透明なだけでなくクリックも Tab のフォーカスも受けない
/// （macOS 版は隠す）。幅は取ったままにして、出し入れで行数の位置が動かないようにする。
fn reveal_discard(discard: &gtk::Button, shown: bool) {
    discard.set_opacity(if shown { 1.0 } else { 0.0 });
    discard.set_can_target(shown);
    discard.set_focusable(shown);
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
    /// `dir` の行を `out` に足し、中のファイルのチェックの元を返す（ファイルは 1 回ずつしか見ない）。
    fn walk(
        dir: &Dir,
        prefix: &str,
        depth: usize,
        entries: &[StatusEntry],
        out: &mut Vec<TreeItem>,
    ) -> DirCheck {
        let mut total = DirCheck::default();
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
            let at = out.len();
            out.push(TreeItem::Dir {
                name,
                path: path.clone(),
                depth,
                check: DirCheck::default(),
            });
            let inside = walk(child, &path, depth + 1, entries, out);
            if let TreeItem::Dir { check, .. } = &mut out[at] {
                *check = inside;
            }
            total.add(inside);
        }
        let mut files = dir.files.clone();
        files.sort_by(|&a, &b| entries[a].path.cmp(&entries[b].path));
        for index in files {
            let e = &entries[index];
            if e.kind != StatusKind::Unmerged {
                total.add(DirCheck {
                    files: 1,
                    fully: usize::from(e.is_staged() && !e.is_unstaged()),
                    some: usize::from(e.is_staged()),
                });
            }
            out.push(TreeItem::File { index, depth });
        }
        total
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

    /// 木のディレクトリのチェックは、中の（コンフリクト以外の）ファイルを数える。子が 1 つのディレクトリは
    /// つなぎ、ディレクトリはファイルより先。
    #[test]
    fn tree_counts_files_inside_each_directory() {
        let entries = [
            entry(StatusKind::Changed, "src/a.rs", b'M', b'.'),
            entry(StatusKind::Changed, "src/b.rs", b'M', b'M'),
            entry(StatusKind::Unmerged, "src/c.rs", b'U', b'U'),
            entry(StatusKind::Changed, "src/deep/x/y.rs", b'.', b'M'),
            entry(StatusKind::Changed, "top.txt", b'M', b'.'),
        ];
        let items = tree(&entries);
        let shape: Vec<String> = items
            .iter()
            .map(|item| match item {
                TreeItem::Dir {
                    path, depth, check, ..
                } => format!(
                    "{depth} {path}/ {}:{}:{}",
                    check.files, check.fully, check.some
                ),
                TreeItem::File { index, depth } => format!("{depth} {}", entries[*index].path),
            })
            .collect();
        assert_eq!(
            shape,
            [
                "0 src/ 3:1:2",
                "1 src/deep/x/ 1:0:0",
                "2 src/deep/x/y.rs",
                "1 src/a.rs",
                "1 src/b.rs",
                "1 src/c.rs",
                "0 top.txt",
            ]
        );
        let TreeItem::Dir { check, .. } = &items[0] else {
            panic!("dir")
        };
        assert!(check.mixed() && !check.all());
    }

    /// 木の行: 畳んだディレクトリの中は飛ばし、行ごとのファイルの番号はディレクトリの行で None。
    #[test]
    fn list_rows_skip_folded_directories() {
        let entries = [
            entry(StatusKind::Changed, "a/x.rs", b'M', b'.'),
            entry(StatusKind::Changed, "a/b/y.rs", b'.', b'M'),
            entry(StatusKind::Changed, "a/b/z.rs", b'.', b'M'),
            entry(StatusKind::Changed, "top.txt", b'M', b'.'),
        ];
        let shape = |rows: &[Row]| -> Vec<String> {
            rows.iter()
                .map(|r| match r {
                    Row::Dir {
                        path, collapsed, ..
                    } => format!("{path}/{}", if *collapsed { " ▸" } else { "" }),
                    Row::File { entry, depth, .. } => format!("{depth} {}", entry.path),
                })
                .collect()
        };
        let none = std::collections::HashSet::new();
        let (rows, indices) = list_rows(&entries, Layout::Tree, &none);
        assert_eq!(
            shape(&rows),
            [
                "a/",
                "a/b/",
                "2 a/b/y.rs",
                "2 a/b/z.rs",
                "1 a/x.rs",
                "0 top.txt"
            ]
        );
        assert_eq!(indices, [None, None, Some(1), Some(2), Some(0), Some(3)]);
        let folded: std::collections::HashSet<String> = ["a/b".to_owned()].into();
        let (rows, indices) = list_rows(&entries, Layout::Tree, &folded);
        assert_eq!(shape(&rows), ["a/", "a/b/ ▸", "1 a/x.rs", "0 top.txt"]);
        assert_eq!(indices, [None, None, Some(0), Some(3)]);
        // 一覧と Flat はファイルだけを並びのまま。
        let (rows, indices) = list_rows(&entries, Layout::Flat, &folded);
        assert_eq!(rows.len(), 4);
        assert_eq!(indices, [Some(0), Some(1), Some(2), Some(3)]);
    }

    /// 入れ替える所は、前後の同じ所を除いた真ん中だけ。
    #[test]
    fn changed_span_keeps_common_ends() {
        assert_eq!(changed_span(&[1, 2, 3], &[1, 2, 3]), (3, 3, 3));
        assert_eq!(changed_span(&[1, 2, 3], &[1, 9, 3]), (1, 2, 2));
        assert_eq!(changed_span(&[1, 2, 3], &[1, 3]), (1, 2, 1));
        assert_eq!(changed_span(&[1, 3], &[1, 2, 3]), (1, 1, 2));
        assert_eq!(changed_span::<i32>(&[], &[1, 2]), (0, 0, 2));
        assert_eq!(changed_span(&[1, 2], &[]), (0, 2, 0));
        // 同じ値が続いても、前と後ろで重ならない。
        assert_eq!(changed_span(&[1, 1], &[1, 1, 1]), (2, 2, 3));
    }

    /// 読み直した後は、選んでいたファイル・ディレクトリのうち一覧に残ったものをすべて選び直す。
    #[test]
    fn reselects_every_row_still_listed() {
        let a = entry(StatusKind::Changed, "src/a.rs", b'.', b'M');
        let b = entry(StatusKind::Changed, "src/b.rs", b'.', b'M');
        let c = entry(StatusKind::Changed, "z.txt", b'.', b'M');
        let none = std::collections::HashSet::new();
        let kept = vec![
            RowKey::File("src/a.rs".into()),
            RowKey::File("z.txt".into()),
            RowKey::Dir("src".into()),
        ];
        // 一覧: a・b・z。a と z を選んでいた。
        let (rows, _) = list_rows(&[a.clone(), b.clone(), c.clone()], Layout::List, &none);
        assert_eq!(rows_to_reselect(&kept, &rows), vec![0, 2]);
        // a がなくなった: z だけ残る（行の番号は新しい一覧で）。
        let (rows, _) = list_rows(&[b.clone(), c.clone()], Layout::List, &none);
        assert_eq!(rows_to_reselect(&kept, &rows), vec![1]);
        // 木: src/・a・b・z。ディレクトリの行も選び直す。
        let (rows, _) = list_rows(&[a, b, c], Layout::Tree, &none);
        assert_eq!(rows_to_reselect(&kept, &rows), vec![0, 1, 3]);
    }

    /// ディレクトリの中かは、名前の区切りで見る（`src` に `srcx/a` は入らない）。
    #[test]
    fn in_dir_needs_a_separator() {
        assert!(in_dir("src/a.rs", "src"));
        assert!(in_dir("src/deep/a.rs", "src"));
        assert!(!in_dir("srcx/a.rs", "src"));
        assert!(!in_dir("src", "src"));
    }

    /// コンフリクトのファイル（index が U）は、次のコミットに含める数に入れない。
    #[test]
    fn all_check_follows_files_outside_conflicts() {
        let on = entry(StatusKind::Changed, "a", b'M', b'.');
        let off = entry(StatusKind::Changed, "b", b'.', b'M');
        let part = entry(StatusKind::Changed, "c", b'M', b'M');
        let conflict = entry(StatusKind::Unmerged, "d", b'U', b'U');
        assert_eq!(
            all_check(&[on.clone(), conflict.clone()]),
            (true, false, true)
        );
        assert_eq!(
            all_check(&[off.clone(), conflict.clone()]),
            (false, false, true)
        );
        assert_eq!(all_check(&[on.clone(), off]), (false, true, true));
        assert_eq!(all_check(&[part]), (false, true, true));
        // コンフリクトだけなら押せない。
        assert_eq!(all_check(&[conflict]), (false, false, false));
        assert_eq!(all_check(&[]), (false, false, false));
    }

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

    /// 状態の文字は macOS 版と同じ: untracked は U、コンフリクトは ⚠、作業ツリーの側を先に見る。
    #[test]
    fn state_letters_follow_macos() {
        let letter = |e: StatusEntry| state_letter(&e).0;
        assert_eq!(letter(entry(StatusKind::Untracked, "a", b'?', b'?')), "U");
        assert_eq!(letter(entry(StatusKind::Unmerged, "a", b'U', b'U')), "⚠");
        assert_eq!(letter(entry(StatusKind::Changed, "a", b'A', b'M')), "M");
        assert_eq!(letter(entry(StatusKind::Changed, "a", b'A', b'.')), "A");
        assert_eq!(letter(entry(StatusKind::Changed, "a", b'.', b'D')), "D");
        assert_eq!(letter(entry(StatusKind::Renamed, "a", b'R', b'.')), "R");
        assert_eq!(letter(entry(StatusKind::Changed, "a", b'T', b'.')), "T");
    }
}
