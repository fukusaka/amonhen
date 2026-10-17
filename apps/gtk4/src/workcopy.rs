//! Working Copy: 左の上にコミットの入力、その下に変更のあるファイルの一覧（チェックで次のコミットに含めるか）。
//! 右に選んだファイルの差分（行を選んで含める・外す・破棄する）。
//!
//! チェックは index の状態そのもので、FE だけの選択状態は持たない（ターミナルで `git add` しても揃う）。
//! 差分は、できれば HEAD と作業ツリーの 1 本の差分に行ごとの印を付けて見せ（`working_diff_combined`）、
//! それで表せないファイルは、作業ツリーの側か stage 済みの側の差分を見せる。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use amonhen_core::diff::{self, DiffView};
use amonhen_core::{Repository, StatusSnapshot, WorkingDiff};
use amonhen_layout::DiffRow;
use amonhen_model::{StatusEntry, StatusKind};
use amonhen_projection::FrontendProfile;
use gtk::prelude::*;
use gtk::{gio, glib};

use crate::diff::{DiffWidget, View};

type CountCallback = Box<dyn Fn(usize)>;

/// 裏のスレッドで作った 1 ファイルの差分。行の操作に使う。
struct Working(WorkingDiff);

// SAFETY: `WorkingDiff` の表示（`DiffView`）のポインタは、自分が持つバッファの先頭を指すだけ。
unsafe impl Send for Working {}
unsafe impl Sync for Working {}

pub struct WorkingCopy {
    pub root: gtk::Paned,
    inner: Rc<Inner>,
}

struct Inner {
    repo: RefCell<Option<Arc<Repository>>>,
    status: RefCell<Option<Arc<StatusSnapshot>>>,
    /// 一覧に出しているファイル（一覧の行と同じ順）。
    entries: RefCell<Vec<StatusEntry>>,
    list: gtk::ListBox,
    all: gtk::CheckButton,
    count: gtk::Label,
    subject: gtk::Entry,
    body: gtk::TextView,
    amend: gtk::CheckButton,
    commit: gtk::Button,
    message: gtk::Label,
    diff: DiffWidget,
    path: gtk::Label,
    stage: gtk::Button,
    unstage: gtk::Button,
    discard: gtk::Button,
    /// 1 本の差分で表せないファイルで、作業ツリーの側と stage 済みの側を切り替える。
    sides: gtk::Box,
    staged_side: gtk::ToggleButton,
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
}

impl WorkingCopy {
    pub fn new(pending: Rc<Cell<u32>>) -> Self {
        // 左の上: コミットの入力。
        let subject = gtk::Entry::builder()
            .placeholder_text("コミットの件名")
            .build();
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
        let amend = gtk::CheckButton::with_label("直前のコミットを修正");
        let commit = gtk::Button::with_label("コミット");
        commit.add_css_class("suggested-action");
        commit.set_hexpand(true);
        commit.set_halign(gtk::Align::End);
        let commit_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        commit_row.append(&amend);
        commit_row.append(&commit);
        let message = gtk::Label::new(None);
        message.set_xalign(0.0);
        message.set_wrap(true);
        message.add_css_class("error");
        message.set_visible(false);

        // 左の下: 変更のあるファイルの一覧。
        let all = gtk::CheckButton::new();
        all.set_tooltip_text(Some("すべてを含める・外す"));
        let count = gtk::Label::new(None);
        count.set_xalign(0.0);
        count.add_css_class("dim-label");
        let list_header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        list_header.append(&all);
        list_header.append(&count);
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
        left.append(&list_scrolled);

        // 右: 操作のボタンと差分。
        let path = gtk::Label::new(None);
        path.set_xalign(0.0);
        path.set_hexpand(true);
        path.set_ellipsize(gtk::pango::EllipsizeMode::Start);
        path.add_css_class("heading");
        let stage = gtk::Button::with_label("含める");
        let unstage = gtk::Button::with_label("外す");
        let discard = gtk::Button::with_label("破棄");
        let worktree_side = gtk::ToggleButton::with_label("作業ツリー");
        let staged_side = gtk::ToggleButton::with_label("stage 済み");
        staged_side.set_group(Some(&worktree_side));
        worktree_side.set_active(true);
        let sides = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        sides.add_css_class("linked");
        sides.append(&worktree_side);
        sides.append(&staged_side);
        sides.set_visible(false);
        discard.add_css_class("destructive-action");
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        bar.set_margin_top(6);
        bar.set_margin_bottom(6);
        bar.set_margin_start(10);
        bar.set_margin_end(10);
        bar.append(&path);
        bar.append(&sides);
        bar.append(&stage);
        bar.append(&unstage);
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
            status: RefCell::default(),
            entries: RefCell::default(),
            list,
            all,
            count,
            subject,
            body,
            amend,
            commit,
            message,
            diff,
            path,
            stage,
            unstage,
            discard,
            sides,
            staged_side,
            selected: RefCell::default(),
            current: RefCell::default(),
            generation: Cell::new(0),
            rebuilding: Cell::new(false),
            pending,
            toggle_once: Cell::new(None),
            on_count: RefCell::default(),
        });
        Inner::connect(&inner);
        inner.update_buttons();
        Self { root, inner }
    }

    pub fn set_repo(&self, repo: Arc<Repository>) {
        self.inner.repo.replace(Some(repo));
    }

    /// 新しい status を出す。同じスナップショットなら何もしない。
    pub fn show(&self, status: Arc<StatusSnapshot>) {
        Inner::show(&self.inner, status);
    }

    /// 次に status が届いたとき、`path` を選ぶ。
    /// 次に差分を出したとき、`row` 行の印をクリックしたことにする（確認用）。
    pub fn toggle_row_once(&self, row: u32) {
        self.inner.toggle_once.set(Some(row));
    }

    pub fn select_path(&self, path: &str) {
        self.inner.selected.replace(Some(path.to_owned()));
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
            let entry = this.entries.borrow().get(row.index() as usize).cloned();
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
        for (button, stage) in [(&this.stage, true), (&this.unstage, false)] {
            let weak = Rc::downgrade(this);
            button.connect_clicked(move |_| {
                let Some(this) = weak.upgrade() else { return };
                let rows = this.diff.selected_rows();
                Inner::set_rows(&this, rows, Some(stage));
            });
        }
        let weak = Rc::downgrade(this);
        this.discard.connect_clicked(move |_| {
            let Some(this) = weak.upgrade() else { return };
            let Some(current) = this.current.borrow().clone() else {
                return;
            };
            let rows = this.diff.selected_rows();
            Inner::run(&this, move |repo| repo.discard_rows(&current.0, &rows));
        });
        let weak = Rc::downgrade(this);
        this.staged_side.connect_toggled(move |_| {
            let Some(this) = weak.upgrade() else { return };
            let entry = this.selected_entry();
            if let Some(entry) = entry {
                Inner::load_diff(&this, entry, false);
            }
        });
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
                Inner::commit(&this);
            }
        });
        let weak = Rc::downgrade(this);
        this.subject.connect_activate(move |_| {
            if let Some(this) = weak.upgrade()
                && this.commit.is_sensitive()
            {
                Inner::commit(&this);
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

        this.rebuilding.set(true);
        this.list.remove_all();
        for entry in &entries {
            this.list.append(&Inner::row(this, entry));
        }
        let staged = entries.iter().filter(|e| e.is_staged()).count();
        let partial = entries.iter().any(|e| e.is_staged() && e.is_unstaged());
        this.all
            .set_inconsistent(partial || (staged > 0 && staged < entries.len()));
        this.all
            .set_active(!entries.is_empty() && staged == entries.len() && !partial);
        this.all.set_sensitive(!entries.is_empty());
        this.count.set_text(&if entries.is_empty() {
            "変更はありません".to_owned()
        } else {
            format!("{staged} / {} を含める", entries.len())
        });
        let count = entries.len();
        // 同じファイルを選び直す。なければ最初のもの。
        let wanted = this.selected.borrow().clone();
        let index = wanted
            .and_then(|p| entries.iter().position(|e| e.path == p))
            .unwrap_or(0);
        this.entries.replace(entries);
        this.rebuilding.set(false);
        if let Some(row) = this.list.row_at_index(index as i32) {
            // 選び直すと `row-selected` が来ないことがあるので、差分は自分で読み直す。
            this.list.select_row(Some(&row));
            let entry = this.entries.borrow()[index].clone();
            let same = this.selected.borrow().as_deref() == Some(entry.path.as_str());
            this.selected.replace(Some(entry.path.clone()));
            Inner::load_diff(this, entry, same);
        } else {
            this.selected.replace(None);
            this.current.replace(None);
            this.path.set_text("");
            this.diff.clear();
        }
        this.update_buttons();
        if let Some(f) = this.on_count.borrow().as_ref() {
            f(count);
        }
    }

    /// 一覧の 1 行: チェック（次のコミットに含めるか）、状態の文字、パス。
    fn row(this: &Rc<Self>, entry: &StatusEntry) -> gtk::ListBoxRow {
        let check = gtk::CheckButton::new();
        check.set_active(entry.is_staged() && !entry.is_unstaged());
        check.set_inconsistent(entry.is_staged() && entry.is_unstaged());
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
        let name = gtk::Label::new(Some(&match &entry.orig_path {
            Some(orig) => format!("{orig} → {}", entry.path),
            None => entry.path.clone(),
        }));
        name.set_xalign(0.0);
        name.set_hexpand(true);
        name.set_ellipsize(gtk::pango::EllipsizeMode::Start);
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        line.append(&check);
        line.append(&state);
        line.append(&name);
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&line));
        row
    }

    fn selected_entry(&self) -> Option<StatusEntry> {
        let selected = self.selected.borrow();
        let path = selected.as_deref()?;
        self.entries
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
                Ok::<_, amonhen_model::Error>((Arc::new(Working(working)), View::new(view)))
            })
            .await;
            let Some(this) = weak.upgrade() else { return };
            this.pending.set(this.pending.get() - 1);
            if this.generation.get() != generation {
                return;
            }
            match result {
                Ok(Ok((working, view))) => {
                    let w = &working.0;
                    this.sides.set_visible(
                        w.combined.is_none() && w.entry.is_staged() && w.entry.is_unstaged(),
                    );
                    this.current.replace(Some(working));
                    this.diff.show_working(view, same);
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
        if rows.is_empty() {
            return;
        }
        Inner::run(this, move |repo| {
            let w = &current.0;
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

    fn commit(this: &Rc<Self>) {
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
                }
                Ok(Err(e)) => this.show_error(&e),
                Err(_) => {}
            }
            this.update_buttons();
        });
    }

    /// 直前のコミットを修正するとき、そのメッセージを入れておく。
    fn fill_head_message(this: &Rc<Self>) {
        let Some(repo) = this.repo() else { return };
        let weak = Rc::downgrade(this);
        glib::spawn_future_local(async move {
            let info = gio::spawn_blocking(move || {
                let head = repo.load_refs().ok()?.refs.head?;
                repo.commit_info(&head).ok()
            })
            .await;
            let (Some(this), Ok(Some(info))) = (weak.upgrade(), info) else {
                return;
            };
            if !this.subject.text().is_empty() {
                return;
            }
            let message = info["message"].as_str().unwrap_or_default();
            let (subject, body) = message.split_once('\n').unwrap_or((message, ""));
            this.subject.set_text(subject.trim());
            this.body.buffer().set_text(body.trim());
        });
    }

    fn show_error(&self, e: &str) {
        self.message.set_text(e);
        self.message.set_visible(true);
    }

    fn update_buttons(&self) {
        let staged = self.entries.borrow().iter().any(StatusEntry::is_staged);
        let has_subject = !self.subject.text().trim().is_empty();
        self.commit
            .set_sensitive(has_subject && (staged || self.amend.is_active()));

        let current = self.current.borrow();
        let (selected_staged, selected) = self.diff.selected_staged();
        let (combined, staged_side) = current
            .as_ref()
            .map_or((false, false), |c| (c.0.combined.is_some(), c.0.staged));
        let any = selected > 0;
        // 1 本の差分: 選んだ行に含めていないものがあれば「含める」、含めているものがあれば「外す」。
        // 片側だけの差分: 作業ツリーの側なら「含める」、stage 済みの側なら「外す」だけ。
        self.stage.set_sensitive(
            any && if combined {
                selected_staged < selected
            } else {
                !staged_side
            },
        );
        self.unstage.set_sensitive(
            any && if combined {
                selected_staged > 0
            } else {
                staged_side
            },
        );
        // stage 済みの変更は破棄しない（先に外す）。
        self.discard
            .set_sensitive(any && selected_staged == 0 && !staged_side);
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
