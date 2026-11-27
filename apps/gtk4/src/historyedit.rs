//! History の編集モード（macOS 版の `HistoryEdit.swift`。docs/ux.md の「History's edit mode」）。選んだコミット
//! から HEAD までの範囲で、行ごとに何をするか（pick・reword・squash・fixup・drop・edit）を決め、並べ替える。
//! 行は History の表がそのまま描き（範囲の行に操作の印、外は薄く。`history.rs` の [`EditMarks`]）、ここは範囲と
//! 決めたこと、ステータスバーの帯（要約・見通し（`rewritePreview`）・Cancel・Apply）を持つ。Apply Changes を
//! 押すまで何も変わらず、まとめて 1 回の rebase（コアの `rewrite` の `plan`）で行う。Undo 1 回で全部戻る。並びは
//! History と同じく新しい順（上が HEAD）。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use amonhen_model::{Oid, tr};
use gtk::prelude::*;
use gtk::{gdk, gio, glib, pango};
use serde_json::{Value, json};

use crate::actions::Actions;
use crate::history::{EditEvent, EditMarks, EditRow, HistoryView};
use crate::historydisplay::Shaper;
use crate::menu::{self, Item};
use crate::sidebar::{Page, Sidebar};
use crate::statusbar::{self, StatusBar};

/// History の上で編集している間は止める操作（HEAD・作業ツリーを動かす。macOS 版の `movesHead`）。
pub const MOVES_HEAD: &[&str] = &[
    "pull",
    "merge",
    "rebase",
    "pop-stash",
    "switch-branch",
    "commit",
    "commit-and-push",
    "apply-patch",
    "undo",
];

/// 行ですること。札は git の対話的な rebase の言葉、何をするかはツールチップと選ぶ一覧の説明に。
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

    /// History で範囲の行を選んで押すキー（p・r・s・f・d・e。git の言葉の頭文字）。
    pub fn from_key(key: char) -> Option<Self> {
        let key = key.to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|a| a.name().starts_with(key) && key.is_ascii_lowercase())
    }

    /// 選ぶ一覧で、言葉に添える説明。
    fn summary(self) -> String {
        match self {
            Action::Pick => tr!("Keep"),
            Action::Reword => tr!("Edit Message"),
            Action::Squash => tr!("Combine with Older"),
            Action::Fixup => tr!("Combine with Older, Keep Its Message"),
            Action::Drop => tr!("Remove"),
            Action::Edit => tr!("Stop to Change Files"),
        }
    }

    /// 選ぶ一覧の項目（言葉と説明）。
    fn menu_title(self) -> String {
        format!("{} — {}", self.name(), self.summary())
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
    /// コミットを、並び（新しい順）の `to` 番目へ動かす（History でコミットを運んで落としたとき）。
    Move(String, usize),
    /// コミット `0` を `1` のすぐ上へ動かし、`1` にまとめる（History で重ねて落としたとき）。
    Squash(String, String),
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

/// `from` の行を、新しい順の並びの `above` 番目の行の上へ動かす（`above` が行の数なら最後の行の下）。動かした
/// ら true。
fn move_rows(rows: &mut Vec<Row>, from: usize, above: usize) -> bool {
    if from >= rows.len() || above > rows.len() {
        return false;
    }
    let moved = rows.remove(from);
    rows.insert(if from < above { above - 1 } else { above }, moved);
    fix_oldest(rows);
    true
}

/// `from` の行を `onto` の行のすぐ上（新しい側）へ動かし、squash にする（History で重ねて落とした）。並べ替えが
/// 要るのに merge を含む、merge の行、同じ行なら false（macOS 版の `place(_:squashingInto:)`）。
fn squash_rows(rows: &mut Vec<Row>, from: usize, onto: usize) -> bool {
    if from == onto
        || from >= rows.len()
        || onto >= rows.len()
        || rows[from].merge
        || rows[onto].merge
    {
        return false;
    }
    if from + 1 != onto && rows.iter().any(|r| r.merge) {
        return false;
    }
    let mut moved = rows.remove(from);
    moved.action = Action::Squash;
    moved.new_message = None;
    let to = if from < onto { onto - 1 } else { onto };
    rows.insert(to, moved);
    fix_oldest(rows);
    true
}

/// いちばん古い行には、まとめる相手がない（squash・fixup なら pick に戻す）。
fn fix_oldest(rows: &mut [Row]) {
    if let Some(last) = rows.last_mut()
        && last.action.combines()
    {
        last.action = Action::Pick;
        last.new_message = None;
    }
}

/// 範囲を狭めて残せる、いちばん少ない数: 外れる行がどれもそのまま（pick で、元の位置。`line` は HEAD から
/// first-parent で辿った並び）のところまで（macOS 版の `canShrink`）。
fn min_count(rows: &[Row], line: &[(String, bool)]) -> usize {
    let mut count = rows.len();
    while count > 1
        && rows[count - 1].action == Action::Pick
        && line
            .get(count - 1)
            .is_some_and(|(oid, _)| *oid == rows[count - 1].oid)
    {
        count -= 1;
    }
    count
}

/// 範囲を `count` 個に狭める（外れる行がそのままのときだけ。macOS 版の `resize(to:)` の狭める側）。狭めたら
/// true。
fn shrink(
    rows: &mut Vec<Row>,
    original: &mut Vec<String>,
    line: &[(String, bool)],
    count: usize,
) -> bool {
    if count == 0 || count >= rows.len() || count < min_count(rows, line) {
        return false;
    }
    rows.truncate(count);
    original.truncate(count);
    fix_oldest(rows);
    true
}

/// コミット `chosen`（16 進と merge か）のメッセージを読んで行にする（件名・作者は History の表が描く）。
async fn read_rows(
    repo: std::sync::Arc<amonhen_core::Repository>,
    chosen: Vec<(String, bool)>,
) -> Vec<Row> {
    gio::spawn_blocking(move || {
        chosen
            .iter()
            .map(|(oid, merge)| {
                let message = Oid::from_hex(oid)
                    .and_then(|id| repo.commit_message(&id).ok())
                    .unwrap_or_default();
                Row {
                    oid: oid.clone(),
                    message,
                    merge: *merge,
                    action: Action::Pick,
                    new_message: None,
                }
            })
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default()
}

/// 編集モードの中身。
struct Mode {
    actions: Rc<Actions>,
    /// 編集を始めた worktree（見通しを読むのも書き換えるのもここ。編集の間に書き込む先を替えても、読んだ
    /// コミットの並びを、ほかの worktree の HEAD に当てない）。
    repo: std::sync::Arc<amonhen_core::Repository>,
    /// 行を描く History の表。
    view: HistoryView,
    rows: RefCell<Vec<Row>>,
    original: RefCell<Vec<String>>,
    /// HEAD から first-parent で辿った並び（範囲を広げる・狭める先）。
    line: Vec<(String, bool)>,
    /// 表に渡す並び（同じものを渡し続け、表が引き直さずに済むように）。
    line_ids: Rc<Vec<Oid>>,
    /// 範囲を変えている途中（読み終えるまで次の変更を受けない）。
    resizing: Cell<bool>,
    /// ステータスバーに出す行（要約・見通し・Cancel・Apply）。
    bar: gtk::Box,
    summary: gtk::Label,
    preview: gtk::Label,
    move_stacked: gtk::CheckButton,
    /// 帯の Cancel と Apply Changes（書き換えている間は押せない）。
    buttons: [gtk::Button; 2],
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
    fn new(
        actions: Rc<Actions>,
        repo: std::sync::Arc<amonhen_core::Repository>,
        view: HistoryView,
        rows: Vec<Row>,
        line: Vec<(String, bool)>,
    ) -> Rc<Self> {
        let original = rows.iter().map(|r| r.oid.clone()).collect();
        let line_ids = Rc::new(line.iter().filter_map(|(o, _)| Oid::from_hex(o)).collect());

        // ステータスバーの行。
        let icon = crate::icon::Icon::default();
        icon.set(crate::icon::Kind::EditList, true);
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
        // macOS 版の小さな文字のチェックボックス。
        move_stacked.add_css_class("caption");
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
            repo,
            view,
            rows: RefCell::new(rows),
            original: RefCell::new(original),
            line,
            line_ids,
            resizing: Cell::new(false),
            bar,
            summary,
            preview,
            move_stacked,
            buttons: [cancel.clone(), apply.clone()],
            configured_update_refs: Cell::new(None),
            touched_update_refs: Cell::new(false),
            syncing_update_refs: Cell::new(false),
            conflicting: RefCell::default(),
            preview_generation: Cell::new(0),
            on_finish: RefCell::default(),
        });
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
        mode
    }

    /// 表で起きたこと（操作の印・キー・ダブルクリック・並べ替え・境目）。
    fn handle(self: &Rc<Self>, event: EditEvent) {
        match event {
            EditEvent::Choices(i, x, y) => {
                let items = self.choice_items(i);
                menu::popup(&self.view, x, y, items);
            }
            EditEvent::Choose(i, action) => self.choose(i, action),
            EditEvent::Reword(i) => self.edit_message(i),
            EditEvent::Move(from, above) => {
                self.move_row(from, above);
            }
            EditEvent::Squash(from, onto) => {
                if squash_rows(&mut self.rows.borrow_mut(), from, onto) {
                    self.reload();
                }
            }
            EditEvent::Resize(count) => {
                let this = self.clone();
                glib::spawn_future_local(async move {
                    this.resize(count).await;
                });
            }
        }
    }

    /// `i` 番目の行で選べること（git の言葉と説明。今のものに印、説明はツールチップ）。
    fn choice_items(self: &Rc<Self>, i: usize) -> Vec<Item> {
        let rows = self.rows.borrow();
        let Some(row) = rows.get(i) else {
            return Vec::new();
        };
        choices(&rows, i)
            .into_iter()
            .map(|action| {
                let weak = Rc::downgrade(self);
                menu::choice_with_tip(
                    action.menu_title(),
                    row.action == action,
                    action.tip(),
                    move || {
                        if let Some(m) = weak.upgrade() {
                            m.choose(i, action);
                        }
                    },
                )
            })
            .collect()
    }

    /// 編集の間の右クリック: 範囲の行（merge でないもの）はすることの一覧、どの行もコミットの ID のコピー。
    fn context_items(self: &Rc<Self>, index: Option<usize>, id: Oid) -> Vec<Item> {
        let mut items = Vec::new();
        if let Some(i) = index.filter(|&i| self.rows.borrow().get(i).is_some_and(|r| !r.merge)) {
            items = self.choice_items(i);
            items.push(Item::Separator);
        }
        let view = self.view.clone();
        items.push(menu::action(tr!("Copy Commit ID"), move || {
            view.clipboard().set_text(&id.to_string());
        }));
        items
    }

    /// `i` 番目の行のすることを変える（reword はメッセージの欄を開く）。
    fn choose(self: &Rc<Self>, i: usize, action: Action) {
        let allowed = {
            let rows = self.rows.borrow();
            i < rows.len() && choices(&rows, i).contains(&action)
        };
        if !allowed {
            return;
        }
        if action == Action::Reword {
            self.edit_message(i);
            return;
        }
        self.set(action, i);
        self.reload();
    }

    /// `from` の行を、新しい順の並びの `above` 番目の行の上へ動かす（merge をまたいでは並べ替えられない）。
    fn move_row(self: &Rc<Self>, from: usize, above: usize) -> bool {
        if self.rows.borrow().iter().any(|r| r.merge) {
            return false;
        }
        let moved = move_rows(&mut self.rows.borrow_mut(), from, above);
        self.reload();
        moved
    }

    /// 範囲を HEAD から first-parent で `count` 個にする。広げるときは古い側に読み足し、狭めるときは外れる行が
    /// どれもそのまま（pick で、元の位置）のときだけ（macOS 版の `resize(to:)`）。変えたら true。
    async fn resize(self: Rc<Self>, count: usize) -> bool {
        if self.resizing.get() || self.line.is_empty() {
            self.push();
            return false;
        }
        let count = count.clamp(1, self.line.len());
        let len = self.rows.borrow().len();
        let changed = if count < len {
            let mut rows = self.rows.borrow_mut();
            shrink(
                &mut rows,
                &mut self.original.borrow_mut(),
                &self.line,
                count,
            )
        } else if count > len {
            self.resizing.set(true);
            let more = read_rows(self.repo.clone(), self.line[len..count].to_vec()).await;
            self.resizing.set(false);
            // 読んでいる間に終えた・変わったら捨てる。
            if self.rows.borrow().len() != len || more.len() != count - len {
                self.push();
                return false;
            }
            self.original
                .borrow_mut()
                .extend(more.iter().map(|r| r.oid.clone()));
            let mut rows = self.rows.borrow_mut();
            rows.extend(more);
            fix_oldest(&mut rows);
            true
        } else {
            false
        };
        if changed {
            self.reload();
        } else {
            self.push();
        }
        changed
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
            Preset::Move(oid, to) => {
                if let Some(from) = index(&oid)
                    && to < self.rows.borrow().len()
                {
                    self.move_row(from, if from < to { to + 1 } else { to });
                }
            }
            Preset::Squash(oid, onto) => {
                if let (Some(from), Some(onto)) = (index(&oid), index(&onto)) {
                    squash_rows(&mut self.rows.borrow_mut(), from, onto);
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
        rows.iter()
            .map(|r| &r.oid)
            .ne(self.original.borrow().iter())
            || rows.iter().any(|r| r.action != Action::Pick)
    }

    /// 表の印と要約を作り直す。
    fn reload(self: &Rc<Self>) {
        self.push();
        self.update_summary();
        self.update_preview();
    }

    fn update_summary(&self) {
        let rows = self.rows.borrow();
        let count = |a: Action| rows.iter().filter(|r| r.action == a).count();
        let mut parts = Vec::new();
        if rows
            .iter()
            .map(|r| &r.oid)
            .ne(self.original.borrow().iter())
        {
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
                "{}{}{}",
                tr!("Changes: {list}.", list = crate::titles::list(&parts)),
                amonhen_model::i18n::sentence_gap(),
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
            // 変更をすべて戻したら、前の見込み（コンフリクトの印、積み重ねたブランチの選択）も消す。
            self.preview.set_visible(false);
            self.preview.set_text("");
            self.move_stacked.set_visible(false);
            if !self.conflicting.borrow().is_empty() {
                self.conflicting.borrow_mut().clear();
                self.push();
            }
            return;
        }
        let repo = self.repo.clone();
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
            let available = info["updateRefs"]["available"].as_bool() == Some(true);
            self.move_stacked.set_sensitive(available);
            // 押せないときは理由を出す（Rebase のシートと同じ）。
            if !available {
                self.move_stacked.set_tooltip_text(Some(&tr!(
                    "Moving stacked branches needs git 2.38 or newer."
                )));
            }
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
            self.push();
        }
    }

    /// 範囲の行と決めたことを表に渡して描き直させる（要約と見通しは読み直さない）。
    fn push(&self) {
        let marks = {
            let rows = self.rows.borrow();
            let conflicting = self.conflicting.borrow();
            EditMarks {
                rows: rows
                    .iter()
                    .filter_map(|r| {
                        Some(EditRow {
                            oid: Oid::from_hex(&r.oid)?,
                            action: r.action,
                            merge: r.merge,
                            subject: r.new_message.as_ref().map(|_| r.subject()),
                            conflict: conflicting.contains_key(&r.oid),
                        })
                    })
                    .collect(),
                line: self.line_ids.clone(),
                min_count: min_count(&rows, &self.line),
                reorderable: !rows.iter().any(|r| r.merge),
            }
        };
        self.view.set_edit(Some(marks));
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

    /// 書き換えが終わってから編集モードを閉じる。書き換えられなければ（失敗はステータスバーに出し）編集モードと
    /// 選んだものを残す（閉じてしまうと、組んだ手順がすべて消える）。途中で止まったら（コンフリクト・edit）閉じる。
    fn apply_clicked(self: &Rc<Self>) {
        if !self.has_changes() {
            self.finish();
            return;
        }
        if self.buttons.iter().any(|b| !b.is_sensitive()) {
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
        for b in &self.buttons {
            b.set_sensitive(false);
        }
        let mut preview = json!({ "op": "rewritePreview", "edit": command["edit"].clone() });
        if let Some(on) = command.get("updateRefs") {
            preview["updateRefs"] = on.clone();
        }
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let Some(m) = weak.upgrade() else { return };
            // 前と後の絵を見せて確かめる（merge を越える編集では描けないので、そのまま。macOS 版と同じ）。
            let info = crate::sheets::run(&m.repo, preview).await["info"].clone();
            if let Some(area) = crate::rewritedraw::for_edit(&info) {
                let (lines, _) = crate::sheets::rewrite_lines(&info);
                let sheet = crate::sheets::Sheet::new(
                    crate::sheets::parent(&m.actions).as_ref(),
                    &tr!("Apply These Changes?"),
                    &lines.join(" "),
                );
                sheet.add(&area);
                sheet.buttons(&tr!("Apply Changes"), false, None);
                if sheet.run().await != crate::sheets::Response::Ok {
                    for b in &m.buttons {
                        b.set_sensitive(true);
                    }
                    return;
                }
            }
            let weak = Rc::downgrade(&m);
            m.actions
                .perform_in(m.repo.clone(), vec![command], move |reply| {
                    let Some(m) = weak.upgrade() else { return };
                    if reply["error"].is_string() || reply.is_null() {
                        for b in &m.buttons {
                            b.set_sensitive(true);
                        }
                    } else {
                        m.finish();
                    }
                });
        });
    }

    fn finish(&self) {
        let f = self.on_finish.borrow().clone();
        if let Some(f) = f {
            f();
        }
    }
}

/// Esc で Cancel、Return で Apply Changes（macOS 版のボタンのキー: 窓のどこでも効く）。窓に付け、捕まえる段階で
/// 受けるので、History やチェックボックスに入力があっても先に効く（シートの `return_presses_default` と同じ）。
/// 文字の欄と、開いた吹き出し（することの一覧など）の中の Return・Esc はそのまま。窓はタブで共有するので、この
/// タブの編集モードの帯が出ている間だけ。
fn window_keys(mode: &Rc<Mode>) -> gtk::EventControllerKey {
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = Rc::downgrade(mode);
    keys.connect_key_pressed(move |c, key, _, state| {
        let Some(m) = weak.upgrade() else {
            return glib::Propagation::Proceed;
        };
        let typing = c
            .widget()
            .and_downcast::<gtk::Window>()
            .and_then(|w| GtkWindowExt::focus(&w))
            .is_some_and(|f| {
                f.is::<gtk::Text>()
                    || f.is::<gtk::TextView>()
                    || f.ancestor(gtk::Popover::static_type()).is_some()
            });
        if typing
            || !m.bar.is_mapped()
            || state.intersects(
                gdk::ModifierType::CONTROL_MASK
                    | gdk::ModifierType::ALT_MASK
                    | gdk::ModifierType::SHIFT_MASK,
            )
        {
            return glib::Propagation::Proceed;
        }
        match key {
            // 書き換えている間は Cancel も押せない。
            gdk::Key::Escape if m.buttons[0].is_sensitive() => m.finish(),
            gdk::Key::Escape => {}
            gdk::Key::Return | gdk::Key::KP_Enter => m.apply_clicked(),
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    });
    keys
}

/// 編集モードの Esc・Return のキーを、編集モードのある窓に付けておく（タブをほかの窓に移したら、移した先の窓に
/// 付け替える。macOS 版はタブごとに窓なので、いつもそのタブの窓に効く）。
struct WindowKeys {
    keys: gtk::EventControllerKey,
    /// 今付けている窓。
    window: Rc<RefCell<Option<gtk::Window>>>,
    /// 編集モードの部品の窓が替わるのを見ているもの（閉じたら外す）。
    watch: (gtk::Widget, glib::SignalHandlerId),
}

impl WindowKeys {
    fn new(mode: &Rc<Mode>) -> Self {
        let keys = window_keys(mode);
        let window: Rc<RefCell<Option<gtk::Window>>> = Rc::default();
        let follow = {
            let (keys, window) = (keys.clone(), window.clone());
            move |widget: &gtk::Widget| {
                let now = widget.root().and_downcast::<gtk::Window>();
                if *window.borrow() == now {
                    return;
                }
                if let Some(old) = window.replace(now.clone()) {
                    old.remove_controller(&keys);
                }
                if let Some(now) = now {
                    now.add_controller(keys.clone());
                }
            }
        };
        let widget: gtk::Widget = mode.view.clone().upcast();
        follow(&widget);
        let id = widget.connect_root_notify(move |w| follow(w));
        Self {
            keys,
            window,
            watch: (widget, id),
        }
    }

    fn remove(self) {
        let (widget, id) = self.watch;
        widget.disconnect(id);
        if let Some(window) = self.window.take() {
            window.remove_controller(&self.keys);
        }
    }
}

/// 編集モードを開く・閉じる（History の上でそのまま編集する）。
#[derive(Clone)]
pub struct Opener(Rc<OpenerInner>);

struct OpenerInner {
    actions: Rc<Actions>,
    /// 行を描く History の表と、その範囲・折りたたみを決めるもの（編集の間は今のブランチの全部の行にする）。
    view: HistoryView,
    shaper: Rc<Shaper>,
    sidebar: Rc<Sidebar>,
    statusbar: Rc<StatusBar>,
    /// このタブの見出し（止めた操作のボタンのツールチップを、編集の間だけ替える）。
    headers: Vec<gtk::Widget>,
    /// ツールチップを替えたボタンと、元のツールチップ。
    saved_tips: RefCell<Vec<(glib::WeakRef<gtk::Widget>, Option<glib::GString>)>>,
    current: RefCell<Option<Rc<Mode>>>,
    /// 編集モードの間だけ窓に付ける、Esc・Return のキー。
    keys: RefCell<Option<WindowKeys>>,
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

/// 編集の間に止めた操作のボタンのツールチップ（macOS 版の `editingHint`）。
fn editing_hint() -> String {
    tr!("Apply or cancel the commit edit first.")
}

/// `widget` の下の、止める操作（`win.<MOVES_HEAD>`）のボタン。
fn head_buttons(widget: &gtk::Widget, out: &mut Vec<gtk::Widget>) {
    if let Some(name) = widget
        .dynamic_cast_ref::<gtk::Actionable>()
        .and_then(|a| a.action_name())
        && name
            .strip_prefix("win.")
            .is_some_and(|n| MOVES_HEAD.contains(&n))
    {
        out.push(widget.clone());
    }
    let mut child = widget.first_child();
    while let Some(c) = child {
        head_buttons(&c, out);
        child = c.next_sibling();
    }
}

impl Opener {
    pub fn new(
        actions: Rc<Actions>,
        view: &HistoryView,
        shaper: Rc<Shaper>,
        sidebar: Rc<Sidebar>,
        statusbar: Rc<StatusBar>,
        headers: Vec<gtk::Widget>,
        pending: Rc<Cell<u32>>,
    ) -> Self {
        Self(Rc::new(OpenerInner {
            actions,
            view: view.clone(),
            shaper,
            sidebar,
            statusbar,
            headers,
            saved_tips: RefCell::default(),
            current: RefCell::default(),
            keys: RefCell::default(),
            pending,
        }))
    }

    /// 編集モードを開いている。
    pub fn is_open(&self) -> bool {
        self.0.current.borrow().is_some()
    }

    /// History の編集モードに入る（`start` から HEAD まで）。`preset` は最初の変更（右クリックの近道・ドラッグから）。
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
                // 状態の帯に少し出す（macOS 版と同じ。シートで止めない）。
                inner.statusbar.flash_warning(&tr!(
                    "Only commits on HEAD’s first-parent line can be edited."
                ));
                return;
            };
            // まとめるものに HEAD の first-parent の並びにないものがあれば、黙って減らさずに断る（選んだ数より少なく
            // まとめてしまう）。
            if let Some(Preset::Combine(oids)) = &preset
                && !oids
                    .iter()
                    .all(|o| line[..=end].iter().any(|(l, _)| l == o))
            {
                inner.statusbar.flash_warning(&tr!(
                    "Only commits on HEAD’s first-parent line can be edited."
                ));
                return;
            }
            // 隣り合っていなければ断る（一致したコミットだけの History では、間のコミットが隠れている）。
            if let Some(Preset::Combine(oids)) = &preset {
                let mut at: Vec<usize> = oids
                    .iter()
                    .filter_map(|o| line.iter().position(|(l, _)| l == o))
                    .collect();
                at.sort_unstable();
                if at.windows(2).any(|w| w[1] != w[0] + 1) {
                    inner
                        .statusbar
                        .flash_warning(&tr!("Only commits next to each other can be combined."));
                    return;
                }
            }
            let rows = read_rows(repo.clone(), line[..=end].to_vec()).await;
            if rows.is_empty() || this.is_open() {
                return;
            }
            let mode = Mode::new(inner.actions.clone(), repo, inner.view.clone(), rows, line);
            let weak = Rc::downgrade(&this.0);
            mode.on_finish.replace(Some(Rc::new(move || {
                if let Some(inner) = weak.upgrade() {
                    Opener(inner).close();
                }
            })));
            inner.current.replace(Some(mode.clone()));
            // History の上でそのまま編集する（今のブランチの全部の行にし、範囲の行に操作の印、外は薄く）。
            let weak = Rc::downgrade(&this.0);
            inner.view.connect_edit(move |event| {
                let mode = weak.upgrade().and_then(|i| i.current.borrow().clone());
                if let Some(mode) = mode {
                    mode.handle(event);
                }
            });
            inner.shaper.begin_editing();
            inner.sidebar.select_page(Page::History);
            inner.statusbar.show(statusbar::Row::Editing, &mode.bar);
            inner.keys.replace(Some(WindowKeys::new(&mode)));
            this.block_head_moves(true);
            // 帯の要約はすぐ書く（変更がなくても「まだ変更はありません」）。
            mode.reload();
            if let Some(preset) = preset {
                mode.apply_preset(preset);
            }
        });
    }

    fn close(&self) {
        let inner = &self.0;
        let Some(_mode) = inner.current.take() else {
            return;
        };
        inner.statusbar.hide(statusbar::Row::Editing);
        if let Some(keys) = inner.keys.take() {
            keys.remove();
        }
        // 並べ替えて描いていた行から元の行に戻し（詳細は選んでいる行のコミットに）、元の範囲・折りたたみ・一致だけの
        // 表示に戻す。
        inner.view.connect_edit(|_| {});
        inner.view.set_edit(None);
        inner.shaper.end_editing();
        self.block_head_moves(false);
    }

    /// 編集の間は、HEAD・作業ツリーを動かす操作を止め、見出しのボタンのツールチップにわけを出す（終えたら戻す）。
    fn block_head_moves(&self, on: bool) {
        let inner = &self.0;
        inner.actions.update_facts(|f| f.editing_commits = on);
        let hint = editing_hint();
        if on {
            let mut buttons = Vec::new();
            for header in &inner.headers {
                head_buttons(header, &mut buttons);
            }
            let mut saved = inner.saved_tips.borrow_mut();
            for button in buttons {
                saved.push((button.downgrade(), button.tooltip_text()));
                button.set_tooltip_text(Some(&hint));
            }
        } else {
            for (button, tip) in inner.saved_tips.take() {
                // 編集の間にほかで言い直したもの（Undo の操作の名前など）はそのまま。
                if let Some(button) = button.upgrade()
                    && button.tooltip_text().as_deref() == Some(hint.as_str())
                {
                    button.set_tooltip_text(tip.as_deref());
                }
            }
        }
    }

    /// 編集の間の右クリック（範囲の行はすることの一覧とコピー、ほかの行はコピーだけ）。
    pub fn context_menu(&self, row: u32, x: f64, y: f64) {
        let inner = &self.0;
        let Some(mode) = inner.current.borrow().clone() else {
            return;
        };
        let Some(id) = inner.view.shown_commit(row) else {
            return;
        };
        let items = mode.context_items(inner.view.edit_index(row), id);
        if let Some(popover) = menu::popup_returning(&inner.view, x, y, items) {
            inner.view.mark_menu_row(row, &popover);
        }
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
        let mode = self.0.current.borrow().clone();
        if let Some(mode) = mode {
            mode.move_row(from, above);
        }
    }

    /// 確認用（`--edit-range`）: 範囲を HEAD から `count` 個にする（境目を引いたのと同じ）。読み終えたら返る。
    pub fn resize(&self, count: usize) -> impl std::future::Future<Output = ()> + 'static {
        let mode = self.0.current.borrow().clone();
        async move {
            if let Some(mode) = mode {
                mode.resize(count).await;
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 新しい順の行（`oid` だけ、することは pick）。
    fn rows(oids: &[&str]) -> Vec<Row> {
        oids.iter()
            .map(|o| Row {
                oid: (*o).to_owned(),
                message: format!("{o} subject"),
                merge: false,
                action: Action::Pick,
                new_message: None,
            })
            .collect()
    }

    fn line(oids: &[&str]) -> Vec<(String, bool)> {
        oids.iter().map(|o| ((*o).to_owned(), false)).collect()
    }

    fn oids(rows: &[Row]) -> Vec<&str> {
        rows.iter().map(|r| r.oid.as_str()).collect()
    }

    #[test]
    fn keys_are_the_first_letters_of_gits_words() {
        let keys: Vec<_> = "prsfde".chars().map(Action::from_key).collect();
        assert_eq!(
            keys,
            [
                Some(Action::Pick),
                Some(Action::Reword),
                Some(Action::Squash),
                Some(Action::Fixup),
                Some(Action::Drop),
                Some(Action::Edit),
            ]
        );
        assert_eq!(Action::from_key('R'), Some(Action::Reword));
        assert_eq!(Action::from_key('x'), None);
        assert_eq!(Action::from_key('1'), None);
    }

    #[test]
    fn moving_a_row_puts_it_above_the_target_and_resets_the_oldest_squash() {
        let mut r = rows(&["c", "b", "a"]);
        // c を a の上へ（上から下へ運ぶと、抜けた分だけ前に詰める）。
        assert!(move_rows(&mut r, 0, 2));
        assert_eq!(oids(&r), ["b", "c", "a"]);
        // a をいちばん上へ。
        assert!(move_rows(&mut r, 2, 0));
        assert_eq!(oids(&r), ["a", "b", "c"]);
        // 最後の行の下へ（行の数）。
        assert!(move_rows(&mut r, 0, 3));
        assert_eq!(oids(&r), ["b", "c", "a"]);
        assert!(!move_rows(&mut r, 3, 0));
        assert!(!move_rows(&mut r, 0, 4));
        // まとめる行をいちばん古い所へ運ぶと、まとめる相手がないので pick に戻る。
        r[0].action = Action::Squash;
        r[0].new_message = Some("combined".into());
        assert!(move_rows(&mut r, 0, 3));
        assert_eq!(oids(&r), ["c", "a", "b"]);
        assert_eq!(r[2].action, Action::Pick);
        assert_eq!(r[2].new_message, None);
    }

    #[test]
    fn squashing_by_drop_puts_the_row_above_the_target() {
        // 上から下へ: c を a に重ねる → a のすぐ上で squash。
        let mut r = rows(&["c", "b", "a"]);
        assert!(squash_rows(&mut r, 0, 2));
        assert_eq!(oids(&r), ["b", "c", "a"]);
        assert_eq!(r[1].action, Action::Squash);
        // 下から上へ: a を b に重ねる。動いた跡でいちばん古い行が squash なら pick に戻る。
        let mut r = rows(&["c", "b", "a"]);
        r[0].action = Action::Squash;
        assert!(squash_rows(&mut r, 2, 0));
        assert_eq!(oids(&r), ["a", "c", "b"]);
        assert_eq!(r[0].action, Action::Squash);
        // 隣の古い行へは、並べ替えずにそのまま。
        let mut r = rows(&["c", "b", "a"]);
        r[0].new_message = Some("reworded".into());
        assert!(squash_rows(&mut r, 0, 1));
        assert_eq!(oids(&r), ["c", "b", "a"]);
        assert_eq!(r[0].new_message, None);
        // 同じ行・merge には重ねられない。merge を含むと並べ替えが要るものはできない。
        let mut r = rows(&["c", "b", "a"]);
        assert!(!squash_rows(&mut r, 1, 1));
        r[2].merge = true;
        assert!(!squash_rows(&mut r, 0, 2));
        let mut r = rows(&["d", "c", "b", "a"]);
        r[3].merge = true;
        assert!(!squash_rows(&mut r, 0, 2));
        assert!(squash_rows(&mut r, 1, 2));
    }

    #[test]
    fn the_range_shrinks_only_while_leaving_rows_are_untouched() {
        let l = line(&["e", "d", "c", "b", "a"]);
        let mut r = rows(&["e", "d", "c", "b"]);
        let mut original: Vec<String> = r.iter().map(|r| r.oid.clone()).collect();
        // どれもそのままなら、HEAD の 1 つまで狭められる。
        assert_eq!(min_count(&r, &l), 1);
        // d を取り消すと、d より上までしか狭められない。
        r[1].action = Action::Drop;
        assert_eq!(min_count(&r, &l), 2);
        assert!(!shrink(&mut r, &mut original, &l, 1));
        assert!(shrink(&mut r, &mut original, &l, 2));
        assert_eq!(oids(&r), ["e", "d"]);
        assert_eq!(original, ["e", "d"]);
        // 並べ替えて元の位置にない行は外せない。
        let mut r = rows(&["e", "c", "d", "b"]);
        let mut original = vec!["e".into(), "d".into(), "c".into(), "b".into()];
        assert_eq!(min_count(&r, &l), 3);
        assert!(!shrink(&mut r, &mut original, &l, 2));
        assert!(shrink(&mut r, &mut original, &l, 3));
        assert_eq!(oids(&r), ["e", "c", "d"]);
        // 広げる・同じ数は、ここでは狭めない。
        assert!(!shrink(&mut r, &mut original, &l, 3));
        assert!(!shrink(&mut r, &mut original, &l, 0));
    }

    #[test]
    fn shrinking_resets_a_squash_left_at_the_oldest_row() {
        let l = line(&["c", "b", "a"]);
        let mut r = rows(&["c", "b", "a"]);
        let mut original: Vec<String> = r.iter().map(|r| r.oid.clone()).collect();
        r[1].action = Action::Squash;
        assert!(shrink(&mut r, &mut original, &l, 2));
        assert_eq!(r[1].action, Action::Pick);
    }

    #[test]
    fn plan_steps_are_oldest_first_with_the_combined_message() {
        let mut r = rows(&["c", "b", "a"]);
        r[0].action = Action::Squash;
        r[1].action = Action::Reword;
        r[1].new_message = Some("new b".into());
        let steps = plan_steps(&r);
        let names: Vec<_> = steps
            .iter()
            .map(|s| (s["oid"].as_str().unwrap(), s["action"].as_str().unwrap()))
            .collect();
        assert_eq!(names, [("a", "pick"), ("b", "reword"), ("c", "squash")]);
        assert_eq!(steps[1]["message"], "new b");
        assert_eq!(steps[2]["message"], "new b\n\nc subject");
    }
}
