//! History の編集モード（macOS 版の `HistoryEdit.swift`。docs/ux.md の「History's edit mode」）。選んだコミット
//! から HEAD までを並べ、行ごとに何をするか（そのまま・メッセージを直す・ひとつ古いコミットとまとめる・取り消す・
//! 止めて直す）を選び、ドラッグで並べ替える。Apply Changes を押すまで何も変わらず、まとめて 1 回の rebase
//! （コアの `rewrite` の `plan`）で行う。Undo 1 回で全部戻る。行は History と同じく新しい順（上が HEAD）。
//! 開いている間は History の画面と入れ替え、ステータスバーに要約・見通し（`rewritePreview`）・Cancel・Apply を出す。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use amonhen_model::{Oid, tr};
use gtk::prelude::*;
use gtk::{gdk, gio, glib, pango};
use serde_json::{Value, json};

use crate::actions::Actions;
use crate::sidebar::{Page, Sidebar};
use crate::statusbar::{self, StatusBar};

/// 行ですること。git の対話的な rebase の言葉は、ツールチップにだけ出す。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Pick,
    Reword,
    Squash,
    Fixup,
    Drop,
    Edit,
}

impl Action {
    const ALL: [Action; 6] = [
        Action::Pick,
        Action::Reword,
        Action::Squash,
        Action::Fixup,
        Action::Drop,
        Action::Edit,
    ];

    /// コアの `plan` の `action`（git の言葉）。
    pub fn name(self) -> &'static str {
        match self {
            Action::Pick => "pick",
            Action::Reword => "reword",
            Action::Squash => "squash",
            Action::Fixup => "fixup",
            Action::Drop => "drop",
            Action::Edit => "edit",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.name() == name)
    }

    fn title(self) -> String {
        match self {
            Action::Pick => tr!("Keep"),
            Action::Reword => tr!("Edit Message"),
            Action::Squash => tr!("Combine with Older"),
            Action::Fixup => tr!("Combine with Older, Keep Its Message"),
            Action::Drop => "Drop".to_owned(),
            Action::Edit => tr!("Stop to Change Files"),
        }
    }

    fn tip(self) -> String {
        match self {
            Action::Pick => tr!("Leave the commit as it is (git: pick)."),
            Action::Reword => tr!("Give the commit a new message (git: reword)."),
            Action::Squash => tr!(
                "Make this and the older commit below one commit, with a message you can edit (git: squash)."
            ),
            Action::Fixup => tr!(
                "Make this and the older commit below one commit, keeping the older one’s message (git: fixup)."
            ),
            Action::Drop => tr!("Remove the commit and its changes (git: drop)."),
            Action::Edit => tr!(
                "Stop after this commit to change its files and amend it, then Continue (git: edit)."
            ),
        }
    }

    fn combines(self) -> bool {
        matches!(self, Action::Squash | Action::Fixup)
    }
}

/// 1 行（1 コミット）。
#[derive(Clone)]
struct Row {
    oid: String,
    message: String,
    author: String,
    /// merge のコミット（作り直すだけで、することは選べない）。
    merge: bool,
    action: Action,
    /// 直したメッセージ（reword のとき）、またはまとめたコミットのメッセージ（squash のとき）。
    new_message: Option<String>,
}

impl Row {
    fn subject(&self) -> String {
        self.new_message
            .as_deref()
            .unwrap_or(&self.message)
            .lines()
            .next()
            .unwrap_or_default()
            .to_owned()
    }
}

/// 開くときの最初の変更（右クリックの近道から）。
#[derive(Clone)]
pub enum Preset {
    Reword(String),
    /// 選んだもののうちいちばん古いものに、ほかをまとめる（1 つなら、そのひとつ古いコミットに）。
    Combine(Vec<String>),
    Drop(String),
}

/// 行で選べること。いちばん古い行と merge のすぐ上の行は、まとめる相手がない。merge の行は作り直すだけ。
fn choices(rows: &[Row], i: usize) -> Vec<Action> {
    if rows[i].merge {
        return vec![Action::Pick];
    }
    let no_older = i + 1 >= rows.len() || rows[i + 1].merge;
    Action::ALL
        .into_iter()
        .filter(|a| !(no_older && a.combines()))
        .collect()
}

/// `i` の行（squash）までのまとまりのメッセージを、古い順につないだもの（git の squash の既定と同じ）。
fn combined_message(rows: &[Row], i: usize) -> String {
    // 新しい順の並びで、i から下（古い方）へ、まとめる先の行まで。
    let mut j = i;
    while j + 1 < rows.len() && rows[j].action.combines() {
        j += 1;
    }
    rows[i..=j]
        .iter()
        .rev()
        .filter(|r| r.action != Action::Fixup)
        .map(|r| r.new_message.as_deref().unwrap_or(&r.message))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// コアに渡す並び（古い順）。まとまり（squash を含む）の最後の squash に、まとめたメッセージを付ける。
fn plan_steps(rows: &[Row]) -> Vec<Value> {
    let ordered: Vec<&Row> = rows.iter().rev().collect();
    let mut steps = Vec::new();
    for (i, row) in ordered.iter().enumerate() {
        let mut step = json!({ "oid": row.oid, "action": row.action.name() });
        match row.action {
            Action::Reword => {
                step["message"] = json!(row.new_message.as_deref().unwrap_or(&row.message));
            }
            Action::Squash => {
                let later = &ordered[i + 1..];
                let last_squash = !later
                    .iter()
                    .take_while(|r| r.action.combines())
                    .any(|r| r.action == Action::Squash);
                if last_squash || later.is_empty() {
                    step["message"] = json!(
                        row.new_message
                            .clone()
                            .unwrap_or_else(|| combined_message(rows, rows.len() - 1 - i))
                    );
                }
            }
            _ => {}
        }
        steps.push(step);
    }
    steps
}

/// 編集モードの中身。
struct Mode {
    actions: Rc<Actions>,
    rows: RefCell<Vec<Row>>,
    original: Vec<String>,
    /// 一覧のモデル（行のコミット ID。並べ替えたら作り直す）。
    model: gtk::StringList,
    root: gtk::Box,
    /// ステータスバーに出す行（要約・見通し・Cancel・Apply）。
    bar: gtk::Box,
    summary: gtk::Label,
    preview: gtk::Label,
    move_stacked: gtk::CheckButton,
    /// git の設定（`rebase.updateRefs`）と、この編集で選び直したか。
    configured_update_refs: Cell<Option<bool>>,
    touched_update_refs: Cell<bool>,
    /// git の設定に合わせてチェックを付けている間（選び直したことにしない）。
    syncing_update_refs: Cell<bool>,
    /// コンフリクトしそうなコミットとファイル（行に印を付ける）。
    conflicting: RefCell<HashMap<String, Vec<String>>>,
    preview_generation: Cell<u64>,
    /// 終えたとき（Apply・Cancel）に呼ぶ。
    on_finish: RefCell<Option<Rc<dyn Fn()>>>,
}

impl Mode {
    fn new(actions: Rc<Actions>, rows: Vec<Row>) -> Rc<Self> {
        let original = rows.iter().map(|r| r.oid.clone()).collect();
        let can_reorder = !rows.iter().any(|r| r.merge);
        let heading = gtk::Label::new(Some(&tr!("Edit Commits")));
        heading.add_css_class("title-4");
        heading.set_xalign(0.0);
        let hint = gtk::Label::new(Some(&if can_reorder {
            tr!(
                "Choose what to do with each commit and drag rows to reorder them. Nothing changes until Apply Changes."
            )
        } else {
            tr!(
                "Choose what to do with each commit. Merges are recreated as they are, so rows can’t be reordered across them. Nothing changes until Apply Changes."
            )
        }));
        hint.add_css_class("dim-label");
        hint.add_css_class("caption");
        hint.set_xalign(0.0);
        hint.set_wrap(true);
        let top = gtk::Box::new(gtk::Orientation::Vertical, 2);
        top.set_margin_top(12);
        top.set_margin_bottom(8);
        top.set_margin_start(16);
        top.set_margin_end(16);
        top.append(&heading);
        top.append(&hint);
        let model = gtk::StringList::new(&[]);
        let list = gtk::ListView::new(
            Some(gtk::NoSelection::new(Some(model.clone()))),
            None::<gtk::ListItemFactory>,
        );
        list.add_css_class("rich-list");
        let scrolled = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("view");
        root.append(&top);
        root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        root.append(&scrolled);

        // ステータスバーの行。
        let icon = crate::icon::Icon::default();
        icon.set(crate::icon::Kind::WorkingCopy, true);
        icon.set_valign(gtk::Align::Center);
        let summary = gtk::Label::new(None);
        summary.add_css_class("caption");
        summary.set_xalign(0.0);
        summary.set_ellipsize(pango::EllipsizeMode::End);
        let preview = gtk::Label::new(None);
        preview.add_css_class("caption");
        preview.add_css_class("dim-label");
        preview.set_xalign(0.0);
        preview.set_wrap(true);
        preview.set_lines(3);
        preview.set_ellipsize(pango::EllipsizeMode::End);
        preview.set_visible(false);
        let texts = gtk::Box::new(gtk::Orientation::Vertical, 2);
        texts.set_hexpand(true);
        texts.set_valign(gtk::Align::Center);
        texts.append(&summary);
        texts.append(&preview);
        let move_stacked = gtk::CheckButton::with_label(&tr!("Also move the stacked branches"));
        move_stacked.set_tooltip_text(Some(&tr!(
            "For this edit only (git’s update-ref). Starts from rebase.updateRefs."
        )));
        move_stacked.set_visible(false);
        move_stacked.set_valign(gtk::Align::Center);
        let cancel = statusbar::button(&tr!("Cancel"), false);
        let apply = statusbar::button(&tr!("Apply Changes"), true);
        let bar = statusbar::line(
            &[
                icon.upcast(),
                texts.upcast(),
                move_stacked.clone().upcast(),
                cancel.clone().upcast(),
                apply.clone().upcast(),
            ],
            Some("status-editing"),
        );

        let mode = Rc::new(Self {
            actions,
            rows: RefCell::new(rows),
            original,
            model,
            root,
            bar,
            summary,
            preview,
            move_stacked,
            configured_update_refs: Cell::new(None),
            touched_update_refs: Cell::new(false),
            syncing_update_refs: Cell::new(false),
            conflicting: RefCell::default(),
            preview_generation: Cell::new(0),
            on_finish: RefCell::default(),
        });
        list.set_factory(Some(&Self::factory(&mode)));
        let weak = Rc::downgrade(&mode);
        cancel.connect_clicked(move |_| {
            if let Some(m) = weak.upgrade() {
                m.finish();
            }
        });
        let weak = Rc::downgrade(&mode);
        apply.connect_clicked(move |_| {
            if let Some(m) = weak.upgrade() {
                m.apply_clicked();
            }
        });
        let weak = Rc::downgrade(&mode);
        mode.move_stacked.connect_toggled(move |_| {
            if let Some(m) = weak.upgrade()
                && !m.syncing_update_refs.get()
            {
                m.touched_update_refs.set(true);
                m.update_preview();
            }
        });
        mode.reload();
        mode
    }

    fn can_reorder(&self) -> bool {
        !self.rows.borrow().iter().any(|r| r.merge)
    }

    /// 一覧の行を作る。中身は、行のコミットか位置が変わるたびに入れ直す。
    fn factory(mode: &Rc<Self>) -> gtk::SignalListItemFactory {
        let factory = gtk::SignalListItemFactory::new();
        let weak = Rc::downgrade(mode);
        factory.connect_setup(move |_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let Some(mode) = weak.upgrade() else { return };
            item.set_activatable(false);
            let mark = gtk::Label::new(None);
            mark.set_width_chars(2);
            let merge_icon = crate::icon::Icon::default();
            merge_icon.set(crate::icon::Kind::Merge, true);
            merge_icon.set_valign(gtk::Align::Center);
            let subject = gtk::Label::new(None);
            subject.set_xalign(0.0);
            subject.set_ellipsize(pango::EllipsizeMode::End);
            let detail = gtk::Label::new(None);
            detail.set_xalign(0.0);
            detail.add_css_class("caption");
            detail.add_css_class("dim-label");
            detail.set_ellipsize(pango::EllipsizeMode::End);
            let texts = gtk::Box::new(gtk::Orientation::Vertical, 1);
            texts.set_hexpand(true);
            texts.set_valign(gtk::Align::Center);
            texts.append(&subject);
            texts.append(&detail);
            let choice = gtk::DropDown::from_strings(&[]);
            choice.set_valign(gtk::Align::Center);
            choice.set_size_request(290, -1);
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            row.set_margin_start(8);
            row.set_margin_end(8);
            row.set_margin_top(3);
            row.set_margin_bottom(3);
            row.append(&mark);
            row.append(&merge_icon);
            row.append(&texts);
            row.append(&choice);
            item.set_child(Some(&row));

            // 行の中身を入れる。選択肢を入れ替える間は、選んだことにしない。
            let filling = Rc::new(Cell::new(false));
            let fill = {
                let (weak, filling) = (Rc::downgrade(&mode), filling.clone());
                let (mark, merge_icon, subject, detail, choice) = (
                    mark.clone(),
                    merge_icon.clone(),
                    subject.clone(),
                    detail.clone(),
                    choice.clone(),
                );
                move |item: &gtk::ListItem| {
                    let Some(mode) = weak.upgrade() else { return };
                    let rows = mode.rows.borrow();
                    let i = item.position() as usize;
                    let Some(r) = rows.get(i) else { return };
                    filling.set(true);
                    let allowed = choices(&rows, i);
                    let titles: Vec<String> = allowed.iter().map(|a| a.title()).collect();
                    let refs: Vec<&str> = titles.iter().map(String::as_str).collect();
                    choice.set_model(Some(&gtk::StringList::new(&refs)));
                    choice.set_selected(
                        allowed.iter().position(|a| *a == r.action).unwrap_or(0) as u32
                    );
                    choice.set_sensitive(!r.merge);
                    choice.set_tooltip_text(Some(&r.action.tip()));
                    filling.set(false);
                    // 件名: 取り消す行は線で消し、まとめる行は薄く、直したメッセージは斜体。
                    subject.set_text(&r.subject());
                    let attrs = pango::AttrList::new();
                    match r.action {
                        Action::Drop => {
                            attrs.insert(pango::AttrInt::new_strikethrough(true));
                            attrs.insert(pango::AttrInt::new_foreground_alpha(u16::MAX / 3));
                        }
                        Action::Squash | Action::Fixup => {
                            attrs.insert(pango::AttrInt::new_foreground_alpha(u16::MAX / 5 * 3));
                        }
                        _ => {}
                    }
                    if r.new_message.is_some()
                        && matches!(r.action, Action::Reword | Action::Squash)
                    {
                        attrs.insert(pango::AttrInt::new_style(pango::Style::Italic));
                    }
                    subject.set_attributes(Some(&attrs));
                    detail.set_text(&format!("{} · {}", &r.oid[..r.oid.len().min(8)], r.author));
                    // 印: まとめる行は下へ向かう矢印、merge は merge の印、コンフリクトしそうなら警告。
                    let conflict = mode.conflicting.borrow().get(&r.oid).cloned();
                    merge_icon.set_visible(r.merge && conflict.is_none());
                    mark.set_visible(!r.merge || conflict.is_some());
                    mark.remove_css_class("warning");
                    mark.remove_css_class("dim-label");
                    mark.remove_css_class("accent");
                    match &conflict {
                        Some(files) => {
                            mark.set_text("⚠");
                            mark.add_css_class("warning");
                            mark.set_tooltip_text(Some(&tr!(
                                "Conflicts expected in {files}",
                                files = files.join(", ")
                            )));
                        }
                        None => {
                            mark.set_text(if r.action.combines() { "↳" } else { "●" });
                            mark.add_css_class(if r.action == Action::Drop {
                                "dim-label"
                            } else {
                                "accent"
                            });
                            mark.set_tooltip_text(None);
                        }
                    }
                }
            };
            let f = fill.clone();
            item.connect_item_notify(move |item| f(item));
            item.connect_position_notify(move |item| fill(item));

            let (w, it) = (Rc::downgrade(&mode), item.downgrade());
            choice.connect_selected_notify(move |choice| {
                let (Some(mode), Some(item)) = (w.upgrade(), it.upgrade()) else {
                    return;
                };
                if filling.get() {
                    return;
                }
                let i = item.position() as usize;
                let action = {
                    let rows = mode.rows.borrow();
                    if i >= rows.len() {
                        return;
                    }
                    let allowed = choices(&rows, i);
                    let Some(&action) = allowed.get(choice.selected() as usize) else {
                        return;
                    };
                    if action == rows[i].action {
                        return;
                    }
                    action
                };
                if action == Action::Reword {
                    mode.edit_message(i);
                    return;
                }
                mode.set(action, i);
                mode.reload();
            });

            // ダブルクリックでメッセージを直す。
            let click = gtk::GestureClick::new();
            let (w, it) = (Rc::downgrade(&mode), item.downgrade());
            click.connect_pressed(move |_, n, _, _| {
                if n == 2
                    && let (Some(mode), Some(item)) = (w.upgrade(), it.upgrade())
                {
                    mode.edit_message(item.position() as usize);
                }
            });
            row.add_controller(click);

            // ドラッグで並べ替える（merge をまたいでは並べ替えられない）。
            let source = gtk::DragSource::new();
            source.set_actions(gdk::DragAction::MOVE);
            let (w, it) = (Rc::downgrade(&mode), item.downgrade());
            source.connect_prepare(move |_, _, _| {
                let (mode, item) = (w.upgrade()?, it.upgrade()?);
                if !mode.can_reorder() {
                    return None;
                }
                Some(gdk::ContentProvider::for_value(&item.position().to_value()))
            });
            row.add_controller(source);
            let target = gtk::DropTarget::new(u32::static_type(), gdk::DragAction::MOVE);
            let (w, it) = (Rc::downgrade(&mode), item.downgrade());
            target.connect_drop(move |target, value, _, y| {
                let (Some(mode), Some(item)) = (w.upgrade(), it.upgrade()) else {
                    return false;
                };
                let Ok(from) = value.get::<u32>() else {
                    return false;
                };
                let height = target.widget().map_or(1, |w| w.height()) as f64;
                let above = item.position() + u32::from(y >= height / 2.0);
                mode.move_row(from as usize, above as usize)
            });
            row.add_controller(target);
        });
        factory
    }

    /// `from` の行を、新しい順の並びの `above` 番目の行の上へ動かす。
    fn move_row(self: &Rc<Self>, from: usize, above: usize) -> bool {
        {
            let mut rows = self.rows.borrow_mut();
            if from >= rows.len() || above > rows.len() {
                return false;
            }
            let moved = rows.remove(from);
            rows.insert(if from < above { above - 1 } else { above }, moved);
            // いちばん古い行には、まとめる相手がない。
            if rows.last().is_some_and(|r| r.action.combines()) {
                let last = rows.len() - 1;
                rows[last].action = Action::Pick;
                rows[last].new_message = None;
            }
        }
        self.reload();
        true
    }

    fn set(&self, action: Action, i: usize) {
        let mut rows = self.rows.borrow_mut();
        rows[i].action = action;
        if matches!(
            action,
            Action::Pick | Action::Drop | Action::Fixup | Action::Edit
        ) {
            rows[i].new_message = None;
        }
    }

    /// 最初の変更を入れる（メッセージを直すときは、そのままメッセージの欄を開く）。
    fn apply_preset(self: &Rc<Self>, preset: Preset) {
        let index = |oid: &str| self.rows.borrow().iter().position(|r| r.oid == oid);
        match preset {
            Preset::Reword(oid) => {
                if let Some(i) = index(&oid) {
                    self.edit_message(i);
                }
            }
            Preset::Combine(oids) => {
                let mut indices: Vec<usize> = oids.iter().filter_map(|o| index(o)).collect();
                indices.sort_unstable();
                // 1 つなら、そのコミットをひとつ古いコミットへ。いくつかなら、いちばん古いものへほかを。
                if indices.len() > 1 {
                    indices.pop();
                }
                for i in indices {
                    if choices(&self.rows.borrow(), i).contains(&Action::Squash) {
                        self.set(Action::Squash, i);
                    }
                }
            }
            Preset::Drop(oid) => {
                if let Some(i) = index(&oid) {
                    self.set(Action::Drop, i);
                }
            }
        }
        self.reload();
    }

    /// メッセージの欄を開く。まとめる行なら、まとめたコミットのメッセージ（最初は、まとめるものを古い順につないだもの）。
    fn edit_message(self: &Rc<Self>, i: usize) {
        let (combining, initial) = {
            let rows = self.rows.borrow();
            let Some(r) = rows.get(i) else { return };
            if r.merge {
                return;
            }
            let combining = r.action == Action::Squash;
            let initial = r.new_message.clone().unwrap_or_else(|| {
                if combining {
                    combined_message(&rows, i)
                } else {
                    r.message.clone()
                }
            });
            (combining, initial)
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let title = if combining {
                tr!("Message of the Combined Commit")
            } else {
                tr!("Edit Message")
            };
            let edited = this
                .actions
                .edit_message(&title, &initial, &tr!("Change"))
                .await;
            if let Some(edited) = edited {
                let mut rows = this.rows.borrow_mut();
                if let Some(r) = rows.get_mut(i) {
                    if !combining {
                        r.action = Action::Reword;
                    }
                    r.new_message = Some(edited);
                }
            }
            this.reload();
        });
    }

    fn has_changes(&self) -> bool {
        let rows = self.rows.borrow();
        rows.iter().map(|r| &r.oid).ne(self.original.iter())
            || rows.iter().any(|r| r.action != Action::Pick)
    }

    /// 一覧と要約を作り直す。
    fn reload(self: &Rc<Self>) {
        let oids: Vec<String> = self.rows.borrow().iter().map(|r| r.oid.clone()).collect();
        let refs: Vec<&str> = oids.iter().map(String::as_str).collect();
        self.model.splice(0, self.model.n_items(), &refs);
        self.update_summary();
        self.update_preview();
    }

    fn update_summary(&self) {
        let rows = self.rows.borrow();
        let count = |a: Action| rows.iter().filter(|r| r.action == a).count();
        let mut parts = Vec::new();
        if rows.iter().map(|r| &r.oid).ne(self.original.iter()) {
            parts.push(tr!("reordered"));
        }
        if count(Action::Reword) > 0 {
            parts.push(tr!("{count} messages", count = count(Action::Reword)));
        }
        let combined = count(Action::Squash) + count(Action::Fixup);
        if combined > 0 {
            parts.push(tr!("{count} combined", count = combined));
        }
        if count(Action::Drop) > 0 {
            parts.push(tr!("{count} dropped", count = count(Action::Drop)));
        }
        if count(Action::Edit) > 0 {
            parts.push(tr!("{count} to change files", count = count(Action::Edit)));
        }
        self.summary.set_text(&if parts.is_empty() {
            tr!("No changes yet.")
        } else {
            format!(
                "{} {}",
                tr!("Changes: {list}.", list = parts.join(&tr!(", "))),
                crate::sheets::can_undo()
            )
        });
    }

    /// 変えるたびに、一緒に動くブランチとコンフリクトの見込みを読み直す（少し待ってから。続けて変えたら前のものは
    /// 捨てる）。
    fn update_preview(self: &Rc<Self>) {
        let generation = self.preview_generation.get() + 1;
        self.preview_generation.set(generation);
        if !self.has_changes() {
            self.preview.set_visible(false);
            self.preview.set_text("");
            if !self.conflicting.borrow().is_empty() {
                self.conflicting.borrow_mut().clear();
            }
            return;
        }
        let Some(repo) = self.actions.repo() else {
            return;
        };
        let mut command = json!({
            "op": "rewritePreview",
            "edit": { "kind": "plan", "steps": plan_steps(&self.rows.borrow()) },
        });
        if self.touched_update_refs.get() {
            command["updateRefs"] = json!(self.move_stacked.is_active());
        }
        // 終えた後に答えが来ても何もしないよう、弱い参照で持つ。
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            glib::timeout_future(std::time::Duration::from_millis(300)).await;
            if weak
                .upgrade()
                .is_none_or(|m| m.preview_generation.get() != generation)
            {
                return;
            }
            let info = crate::sheets::run(&repo, command).await["info"].clone();
            if let Some(mode) = weak.upgrade()
                && mode.preview_generation.get() == generation
            {
                mode.show_preview(&info);
            }
        });
    }

    fn show_preview(&self, info: &Value) {
        if !self.touched_update_refs.get()
            && let Some(configured) = info["updateRefs"]["configured"].as_bool()
        {
            self.configured_update_refs.set(Some(configured));
            self.syncing_update_refs.set(true);
            self.move_stacked.set_active(configured);
            self.syncing_update_refs.set(false);
            self.move_stacked
                .set_sensitive(info["updateRefs"]["available"].as_bool() == Some(true));
        }
        // 積み重ねたブランチがあるときだけ選ばせる。
        let stacked = info["moving"].as_array().is_some_and(|m| !m.is_empty())
            || info["staying"].as_array().is_some_and(|s| {
                s.iter()
                    .any(|b| matches!(b["reason"].as_str(), Some("setting" | "off")))
            });
        self.move_stacked.set_visible(stacked);
        let (lines, conflicts) = crate::sheets::rewrite_lines(info);
        self.preview.set_text(&lines.join(" "));
        self.preview.set_visible(!lines.is_empty());
        if conflicts {
            self.preview.remove_css_class("dim-label");
            self.preview.add_css_class("warning");
        } else {
            self.preview.remove_css_class("warning");
            self.preview.add_css_class("dim-label");
        }
        let marks: HashMap<String, Vec<String>> = info["conflicts"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|c| {
                        let oid = c["oid"].as_str()?.to_owned();
                        let files = c["files"]
                            .as_array()
                            .map(|f| {
                                f.iter()
                                    .filter_map(|p| p.as_str().map(str::to_owned))
                                    .collect()
                            })
                            .unwrap_or_default();
                        Some((oid, files))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let changed = {
            let current = self.conflicting.borrow();
            let mut a: Vec<_> = current.keys().collect();
            let mut b: Vec<_> = marks.keys().collect();
            a.sort();
            b.sort();
            a != b
        };
        if changed {
            self.conflicting.replace(marks);
            // 印を付け直す（要約と見通しは読み直さない）。
            let n = self.model.n_items();
            let oids: Vec<String> = self.rows.borrow().iter().map(|r| r.oid.clone()).collect();
            let refs: Vec<&str> = oids.iter().map(String::as_str).collect();
            self.model.splice(0, n, &refs);
        }
    }

    /// この編集での指定（git の設定と同じなら None）。
    fn update_refs(&self) -> Option<bool> {
        let configured = self.configured_update_refs.get()?;
        if !self.touched_update_refs.get() || !self.move_stacked.is_visible() {
            return None;
        }
        let on = self.move_stacked.is_active();
        (on != configured).then_some(on)
    }

    fn apply_clicked(&self) {
        if !self.has_changes() {
            self.finish();
            return;
        }
        let (count, steps) = {
            let rows = self.rows.borrow();
            (rows.len(), plan_steps(&rows))
        };
        let mut command = json!({
            "op": "rewrite",
            "label": tr!("Edit {count} Commits", count = count),
            "edit": { "kind": "plan", "steps": steps },
        });
        if let Some(on) = self.update_refs() {
            command["updateRefs"] = json!(on);
        }
        self.actions.perform(command);
        self.finish();
    }

    fn finish(&self) {
        let f = self.on_finish.borrow().clone();
        if let Some(f) = f {
            f();
        }
    }
}

/// 編集モードを開く・閉じる（History の画面と入れ替える）。
#[derive(Clone)]
pub struct Opener(Rc<OpenerInner>);

struct OpenerInner {
    actions: Rc<Actions>,
    pages: gtk::Stack,
    /// History の画面（編集モードの間は外す）。
    history: gtk::Widget,
    sidebar: Rc<Sidebar>,
    statusbar: Rc<StatusBar>,
    current: RefCell<Option<Rc<Mode>>>,
    /// 読んでいる間は 1 つ増やす（`--snapshot` は開き終えるまで待つ）。
    pending: Rc<Cell<u32>>,
}

/// 生きている間、数を 1 つ増やしておく。
struct Busy(Rc<Cell<u32>>);

impl Busy {
    fn new(count: &Rc<Cell<u32>>) -> Self {
        count.set(count.get() + 1);
        Self(count.clone())
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}

impl Opener {
    pub fn new(
        actions: Rc<Actions>,
        pages: &gtk::Stack,
        history: &impl IsA<gtk::Widget>,
        sidebar: Rc<Sidebar>,
        statusbar: Rc<StatusBar>,
        pending: Rc<Cell<u32>>,
    ) -> Self {
        Self(Rc::new(OpenerInner {
            actions,
            pages: pages.clone(),
            history: history.clone().upcast(),
            sidebar,
            statusbar,
            current: RefCell::default(),
            pending,
        }))
    }

    /// 編集モードを開いている。
    pub fn is_open(&self) -> bool {
        self.0.current.borrow().is_some()
    }

    /// History の編集モードに入る（`start` から HEAD まで）。`preset` は最初の変更（右クリックの近道から）。
    pub fn open(&self, start: &str, preset: Option<Preset>) {
        let inner = self.0.clone();
        if inner.actions.in_sequence() || self.is_open() {
            if let Some(w) = inner.actions.window() {
                w.error_bell();
            }
            return;
        }
        let Some(repo) = inner.actions.repo() else {
            return;
        };
        let including_older = matches!(&preset, Some(Preset::Combine(oids)) if oids.len() == 1);
        let start = start.to_owned();
        let this = self.clone();
        let busy = Busy::new(&inner.pending);
        glib::spawn_future_local(async move {
            let _busy = busy;
            // merge を越えて first-parent で辿る（merge は git の --rebase-merges で作り直す）。
            let reply = crate::sheets::run(
                &repo,
                json!({ "op": "editableCommits", "throughMerges": true }),
            )
            .await;
            let line: Vec<(String, bool)> = reply["text"]
                .as_str()
                .unwrap_or_default()
                .lines()
                .map(|l| {
                    let mut parts = l.split(' ');
                    let oid = parts.next().unwrap_or_default().to_owned();
                    (oid, parts.next().is_some())
                })
                .filter(|(o, _)| !o.is_empty())
                .collect();
            let end = line
                .iter()
                .position(|(o, _)| o.starts_with(&start) || start.starts_with(o.as_str()))
                .map(|e| e + usize::from(including_older))
                .filter(|&e| e < line.len());
            let Some(end) = end else {
                crate::sheets::inform(
                    crate::sheets::parent(&inner.actions).as_ref(),
                    &tr!("Edit Commits"),
                    &tr!("Only commits on HEAD’s first-parent line can be edited."),
                )
                .await;
                return;
            };
            let chosen: Vec<(String, bool)> = line[..=end].to_vec();
            let rows = gio::spawn_blocking(move || {
                let ids: Vec<Oid> = chosen
                    .iter()
                    .filter_map(|(o, _)| Oid::from_hex(o))
                    .collect();
                let texts = repo.commit_texts(&ids).unwrap_or_default();
                chosen
                    .iter()
                    .enumerate()
                    .map(|(i, (oid, merge))| {
                        let id = Oid::from_hex(oid);
                        let message = id
                            .and_then(|id| repo.commit_message(&id).ok())
                            .unwrap_or_default();
                        let author = texts
                            .get(i)
                            .map(|t| String::from_utf8_lossy(t.author_name()).into_owned())
                            .unwrap_or_default();
                        Row {
                            oid: oid.clone(),
                            message,
                            author,
                            merge: *merge,
                            action: Action::Pick,
                            new_message: None,
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .await
            .unwrap_or_default();
            if rows.is_empty() || this.is_open() {
                return;
            }
            let mode = Mode::new(inner.actions.clone(), rows);
            let weak = Rc::downgrade(&this.0);
            mode.on_finish.replace(Some(Rc::new(move || {
                if let Some(inner) = weak.upgrade() {
                    Opener(inner).close();
                }
            })));
            inner.current.replace(Some(mode.clone()));
            // History の画面と入れ替える（サイドバーの History で開いたまま）。
            inner.pages.remove(&inner.history);
            inner.pages.add_named(&mode.root, Some("history"));
            inner.pages.set_visible_child_name("history");
            inner.sidebar.select_page(Page::History);
            inner.statusbar.show(statusbar::Row::Editing, &mode.bar);
            if let Some(preset) = preset {
                mode.apply_preset(preset);
            }
        });
    }

    fn close(&self) {
        let inner = &self.0;
        let Some(mode) = inner.current.take() else {
            return;
        };
        inner.statusbar.hide(statusbar::Row::Editing);
        inner.pages.remove(&mode.root);
        inner.pages.add_named(&inner.history, Some("history"));
        inner.pages.set_visible_child_name("history");
    }

    /// 確認用（`--edit-plan`）: 行ですることを新しい順に並べたもの（`pick,squash,drop` など）を入れる。
    pub fn set_plan(&self, plan: &str) {
        let Some(mode) = self.0.current.borrow().clone() else {
            return;
        };
        for (i, name) in plan.split(',').enumerate() {
            if let Some(action) = Action::from_name(name.trim())
                && i < mode.rows.borrow().len()
            {
                mode.set(action, i);
            }
        }
        mode.reload();
    }

    /// 確認用（`--edit-move`）: `from` の行を `above` 番目の行の上へ動かす。
    pub fn move_row(&self, from: usize, above: usize) {
        if let Some(mode) = self.0.current.borrow().clone() {
            mode.move_row(from, above);
        }
    }

    /// 確認用（`--edit-apply`）: Apply Changes を押す。
    pub fn apply(&self) {
        let mode = self.0.current.borrow().clone();
        if let Some(mode) = mode {
            mode.apply_clicked();
        }
    }
}
