//! History の編集モード（GTK4 版の `historyedit.rs`、macOS 版の `HistoryEdit.swift`。docs/ux.md の「History's
//! edit mode」）。選んだコミットから HEAD までの範囲で、行ごとに何をするか（pick・reword・squash・fixup・drop・
//! edit）を決め、並べ替える。行は History がそのまま描き（範囲の行に操作の印、外は薄く。[`Overlay`] を
//! `render.rs` が描く）、ここは範囲と決めたこと、ステータスバーの帯（要約・見通し（`rewritePreview`）・Cancel・
//! Apply）を持つ。Apply Changes を押すまで何も変わらず、まとめて 1 回の rebase（コアの `rewrite` の `plan`）で
//! 行う。Undo 1 回で全部戻る。並びは History と同じく新しい順（上が HEAD）。

use std::collections::HashMap;
use std::rc::Rc;

use amonhen_core::HistorySnapshot;
use amonhen_model::{Oid, tr};
use serde_json::{Value, json};
use windows_reactor::*;

/// History の上で編集している間は止める操作（HEAD・作業ツリーを動かす。GTK4 版の `MOVES_HEAD`）。
pub const MOVES_HEAD: &[&str] = &[
    "pull",
    "merge",
    "rebase",
    "pop-stash",
    "switch-branch",
    "apply-patch",
    "undo",
];

/// 編集の間に止めた操作のわけ（macOS 版の `editingHint`）。
pub fn editing_hint() -> String {
    tr!("Apply or cancel the commit edit first.")
}

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

    /// コアの `plan` の `action`（git の言葉。行の札にも出す）。
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

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.name() == name)
    }

    /// History で範囲の行を選んで押すキー（p・r・s・f・d・e。git の言葉の頭文字）。
    pub fn from_key(key: char) -> Option<Self> {
        let key = key.to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|a| key.is_ascii_lowercase() && a.name().starts_with(key))
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

/// 1 行（1 コミット）。件名・作者は History が描く。
#[derive(Clone, Debug)]
pub struct Row {
    pub oid: String,
    pub message: String,
    /// merge のコミット（作り直すだけで、することは選べない）。
    pub merge: bool,
    pub action: Action,
    /// 直したメッセージ（reword のとき）、またはまとめたコミットのメッセージ（squash のとき）。
    pub new_message: Option<String>,
}

impl Row {
    pub fn new(oid: String, message: String, merge: bool) -> Self {
        Self {
            oid,
            message,
            merge,
            action: Action::Pick,
            new_message: None,
        }
    }
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
pub fn plan_steps(rows: &[Row]) -> Vec<Value> {
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
    if from >= rows.len() || above > rows.len() || above == from || above == from + 1 {
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

/// 開くときの最初の変更（右クリックの近道・History のドラッグから）。
#[derive(Clone, Debug, PartialEq)]
pub enum Preset {
    /// メッセージを直す（開いたらメッセージのダイアログを出す）。
    Reword(String),
    /// 1 つならそのコミットをひとつ古いコミットへ、いくつかならいちばん古いものへほかをまとめる。
    Combine(Vec<String>),
    Drop(String),
    /// コミットを、並び（新しい順）の `to` 番目へ動かす（History でコミットを行の間に落としたとき）。
    Move(String, usize),
    /// コミット `0` を `1` のすぐ上へ動かし、`1` にまとめる（History で行に重ねて落としたとき）。
    Squash(String, String),
}

/// History に渡す、範囲の 1 行。
#[derive(Clone, Debug, PartialEq)]
pub struct MarkRow {
    pub oid: Oid,
    pub action: Action,
    /// merge（作り直すだけで、操作の印を出さない）。
    pub merge: bool,
    /// 直したメッセージの件名（直していなければ None）。
    pub subject: Option<String>,
    /// コンフリクトしそう（印を橙に）。
    pub conflict: bool,
}

/// History に渡す、編集の範囲と決めたこと（GTK4 版の `EditMarks`）。
#[derive(Clone, Debug)]
pub struct Marks {
    /// 範囲の行（新しい順。並べ替えた後の並び）。
    pub rows: Vec<MarkRow>,
    /// HEAD から first-parent で辿った並び（境目を引く先）。
    pub line: Rc<Vec<Oid>>,
    /// 境目を上へ引いて狭められる、いちばん少ない数（外れる行がそのままのときだけ狭められる）。
    pub min_count: usize,
    /// 並べ替えられる（merge を含まない）。
    pub reorderable: bool,
}

/// することを選んだ結果。
#[derive(Debug, PartialEq, Eq)]
pub enum Chosen {
    /// 変えた。
    Changed,
    /// メッセージを直す（ダイアログを開く）。
    Message,
    /// 選べない・変わらない。
    Nothing,
}

/// 編集モードの状態。
pub struct State {
    pub rows: Vec<Row>,
    original: Vec<String>,
    /// HEAD から first-parent で辿った並び（範囲を広げる・狭める先）と、History に渡すその ID。
    pub line: Vec<(String, bool)>,
    line_ids: Rc<Vec<Oid>>,
    /// 範囲を広げて読んでいる途中（読み終えるまで次の変更を受けない）。
    pub resizing: bool,
    /// 見通し（`rewritePreview` の行）と、コンフリクトしそうか。読むたびに数を進め、古い答えは捨てる。
    pub preview: Option<(Vec<String>, bool)>,
    pub generation: u64,
    /// 書き換えている間（Apply を押せない）。
    pub busy: bool,
    /// コンフリクトしそうなコミットとファイル（行の印を橙にする）。
    pub conflicting: HashMap<String, Vec<String>>,
    /// 積み重ねたブランチがあるか、git の設定（`rebase.updateRefs`）、選べるか、この編集で選び直した値。
    pub stacked: bool,
    pub configured_update_refs: Option<bool>,
    pub update_refs_available: bool,
    pub update_refs: Option<bool>,
}

impl State {
    /// `rows`（範囲。`line` の先頭から）で始める。`preset` は最初の変更（Reword はここでは何もしない。開いた側が
    /// メッセージのダイアログを出す）。
    pub fn new(rows: Vec<Row>, line: Vec<(String, bool)>, preset: Option<&Preset>) -> Self {
        let original = rows.iter().map(|r| r.oid.clone()).collect();
        let line_ids = Rc::new(line.iter().filter_map(|(o, _)| Oid::from_hex(o)).collect());
        let mut state = Self {
            rows,
            original,
            line,
            line_ids,
            resizing: false,
            preview: None,
            generation: 0,
            busy: false,
            conflicting: HashMap::new(),
            stacked: false,
            configured_update_refs: None,
            update_refs_available: true,
            update_refs: None,
        };
        match preset {
            Some(Preset::Combine(oids)) => {
                let mut indices: Vec<usize> = oids.iter().filter_map(|o| state.index(o)).collect();
                indices.sort_unstable();
                if indices.len() > 1 {
                    indices.pop();
                }
                for i in indices {
                    if choices(&state.rows, i).contains(&Action::Squash) {
                        state.set(i, Action::Squash);
                    }
                }
            }
            Some(Preset::Drop(oid)) => {
                if let Some(i) = state.index(oid) {
                    state.set(i, Action::Drop);
                }
            }
            Some(Preset::Move(oid, to)) => {
                if let Some(from) = state.index(oid)
                    && *to < state.rows.len()
                {
                    state.move_row(from, if from < *to { to + 1 } else { *to });
                }
            }
            Some(Preset::Squash(oid, onto)) => {
                if let (Some(from), Some(onto)) = (state.index(oid), state.index(onto)) {
                    squash_rows(&mut state.rows, from, onto);
                }
            }
            Some(Preset::Reword(_)) | None => {}
        }
        state
    }

    /// コミット `oid`（短くてもよい）の行。
    pub fn index(&self, oid: &str) -> Option<usize> {
        self.rows.iter().position(|r| r.oid.starts_with(oid))
    }

    pub fn has_changes(&self) -> bool {
        self.rows.iter().map(|r| &r.oid).ne(self.original.iter())
            || self.rows.iter().any(|r| r.action != Action::Pick)
    }

    fn set(&mut self, i: usize, action: Action) {
        self.rows[i].action = action;
        if matches!(
            action,
            Action::Pick | Action::Drop | Action::Fixup | Action::Edit
        ) {
            self.rows[i].new_message = None;
        }
    }

    /// `i` の行のすることを変える（reword はメッセージのダイアログを開いてから）。
    pub fn choose(&mut self, i: usize, action: Action) -> Chosen {
        if i >= self.rows.len() || !choices(&self.rows, i).contains(&action) {
            return Chosen::Nothing;
        }
        if action == Action::Reword {
            return Chosen::Message;
        }
        if self.rows[i].action == action {
            return Chosen::Nothing;
        }
        self.set(i, action);
        Chosen::Changed
    }

    /// メッセージを直すときの見出しと最初の文。まとめる行なら、まとめたコミットのメッセージ（最初は、まとめるものを
    /// 古い順につないだもの）。merge の行は直せない。
    pub fn message(&self, i: usize) -> Option<(bool, String)> {
        let row = self.rows.get(i).filter(|r| !r.merge)?;
        let combining = row.action == Action::Squash;
        let initial = row.new_message.clone().unwrap_or_else(|| {
            if combining {
                combined_message(&self.rows, i)
            } else {
                row.message.clone()
            }
        });
        Some((combining, initial))
    }

    /// 直したメッセージを入れる（まとめる行でなければ reword にする）。
    pub fn set_message(&mut self, i: usize, text: &str) {
        let Some(row) = self.rows.get_mut(i) else {
            return;
        };
        if row.action != Action::Squash {
            row.action = Action::Reword;
        }
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        row.new_message = (!text.trim().is_empty()).then(|| text.trim_end().to_owned());
    }

    /// 並べ替えられるか（merge をまたいで並べ替えると、何をどこへ動かすかが一意でない）。
    pub fn reorderable(&self) -> bool {
        !self.rows.iter().any(|r| r.merge)
    }

    /// `from` の行を、新しい順の並びの `above` 番目の行の上へ動かす（merge をまたいでは並べ替えられない）。
    pub fn move_row(&mut self, from: usize, above: usize) -> bool {
        self.reorderable() && move_rows(&mut self.rows, from, above)
    }

    /// `from` の行を `onto` の行のすぐ上へ動かし、squash にする。
    pub fn squash(&mut self, from: usize, onto: usize) -> bool {
        squash_rows(&mut self.rows, from, onto)
    }

    /// 範囲を `count` 個に狭める（外れる行がそのままのときだけ）。狭めたら true。
    pub fn shrink(&mut self, count: usize) -> bool {
        if count == 0 || count >= self.rows.len() || count < min_count(&self.rows, &self.line) {
            return false;
        }
        self.rows.truncate(count);
        self.original.truncate(count);
        fix_oldest(&mut self.rows);
        true
    }

    /// 範囲を広げて読み足した行（`line` の続き）を足す。読んでいる間に範囲が変わっていたら false。
    pub fn extend(&mut self, from: usize, more: Vec<Row>) -> bool {
        if self.rows.len() != from || more.is_empty() {
            return false;
        }
        self.original.extend(more.iter().map(|r| r.oid.clone()));
        self.rows.extend(more);
        fix_oldest(&mut self.rows);
        true
    }

    /// History に渡す、範囲と決めたこと。
    pub fn marks(&self) -> Marks {
        Marks {
            rows: self
                .rows
                .iter()
                .filter_map(|r| {
                    Some(MarkRow {
                        oid: Oid::from_hex(&r.oid)?,
                        action: r.action,
                        merge: r.merge,
                        subject: r
                            .new_message
                            .as_deref()
                            .filter(|_| r.action == Action::Reword || r.action == Action::Squash)
                            .map(|m| m.lines().next().unwrap_or_default().to_owned()),
                        conflict: self.conflicting.contains_key(&r.oid),
                    })
                })
                .collect(),
            line: self.line_ids.clone(),
            min_count: min_count(&self.rows, &self.line),
            reorderable: self.reorderable(),
        }
    }

    /// 選ぶ一覧（`i` 番目の行で選べること。今のものに印、説明はツールチップ）。押すと
    /// `edit-choose:<行>:<言葉>`。
    pub fn choice_items(&self, i: usize) -> Vec<MenuItem> {
        let Some(row) = self.rows.get(i).filter(|r| !r.merge) else {
            return Vec::new();
        };
        choices(&self.rows, i)
            .into_iter()
            .map(|action| {
                let mark = if row.action == action { "✓ " } else { "" };
                MenuItem::item(
                    format!("edit-choose:{i}:{}", action.name()),
                    format!("{mark}{}", action.menu_title()),
                )
                .with_tooltip(action.tip())
            })
            .collect()
    }

    /// 見通し（`rewritePreview` の info）を取り込む: 帯の行、行のコンフリクトの印、積み重ねたブランチの選択。
    pub fn take_preview(&mut self, info: &Value) {
        self.preview = Some(crate::forecast::rewrite_lines(info));
        self.conflicting = info["conflicts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| {
                let files = c["files"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|f| f.as_str().map(str::to_owned))
                    .collect();
                Some((c["oid"].as_str()?.to_owned(), files))
            })
            .collect();
        if let Some(configured) = info["updateRefs"]["configured"].as_bool() {
            self.configured_update_refs = Some(configured);
            self.update_refs_available = info["updateRefs"]["available"].as_bool() == Some(true);
        }
        // 積み重ねたブランチがあるときだけ選ばせる（GTK4 版と同じ）。
        self.stacked = info["moving"].as_array().is_some_and(|m| !m.is_empty())
            || info["staying"].as_array().is_some_and(|s| {
                s.iter()
                    .any(|b| matches!(b["reason"].as_str(), Some("setting" | "off")))
            });
    }

    /// 積み重ねたブランチも動かすか（選び直していなければ git の設定）。
    pub fn moves_stacked(&self) -> bool {
        self.update_refs
            .or(self.configured_update_refs)
            .unwrap_or(false)
    }

    /// 要約（帯に出す）。
    pub fn summary(&self) -> String {
        let count = |a: Action| self.rows.iter().filter(|r| r.action == a).count();
        let mut parts = Vec::new();
        if self.rows.iter().map(|r| &r.oid).ne(self.original.iter()) {
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
        if parts.is_empty() {
            tr!("No changes yet.")
        } else {
            format!(
                "{} {}",
                tr!("Changes: {list}.", list = amonhen_model::i18n::list(&parts)),
                tr!("You can Undo this (Ctrl+Alt+Z).")
            )
        }
    }

    /// 書き換える命令。
    pub fn command(&self) -> Value {
        let mut command = json!({
            "op": "rewrite",
            "label": tr!("Edit {count} Commits", count = self.rows.len()),
            "edit": { "kind": "plan", "steps": plan_steps(&self.rows) },
        });
        // この編集で選び直して、git の設定と違うときだけ渡す。
        if let Some(on) = self
            .update_refs
            .filter(|on| self.stacked && Some(*on) != self.configured_update_refs)
        {
            command["updateRefs"] = json!(on);
        }
        command
    }

    /// 見通しを読む命令。
    pub fn preview_command(&self) -> Value {
        let mut command = json!({
            "op": "rewritePreview",
            "edit": { "kind": "plan", "steps": plan_steps(&self.rows) },
        });
        if let Some(on) = self.update_refs {
            command["updateRefs"] = json!(on);
        }
        command
    }
}

/// History の側で持つ、編集の範囲の描き方（GTK4 版の `history.rs` の `Edit`）。範囲の行は History の上から順に
/// 並び、並べ替えても行の場所は変わらず、描くコミットが替わる。
pub struct Overlay {
    pub marks: Marks,
    /// History の行 → 範囲の何番目（新しい順）。
    pub slots: HashMap<u32, usize>,
    /// History の行 → そこに描くコミットの行（並べ替えた分）。
    pub source: HashMap<u32, u32>,
    /// first-parent の並びのうち History にあるもの（History の行、並びの何番目。行の順）。境目を引く先。
    line_rows: Vec<(u32, usize)>,
    /// `line_rows` を求めた History の世代と並び（変わらなければ求め直さない）。
    line_key: Option<(u64, *const Vec<Oid>)>,
    /// `slots` を求めた History の世代（None なら求め直す）。
    synced: Option<u64>,
    /// 見えている行の、操作の印の横の範囲（描くたびに入れ直す）。押すとすることの一覧を出す。
    pub chips: HashMap<u32, (f32, f32)>,
    /// 境目を引いている間の、範囲の数。
    pub preview: Option<usize>,
    /// 境目を引いている。
    pub dragging_boundary: bool,
}

impl Overlay {
    /// `marks` で描く（前のものがあれば、境目の先の求めたものを引き継ぐ）。
    pub fn new(marks: Marks, previous: Option<Overlay>) -> Self {
        let (line_rows, line_key) = previous
            .map(|p| (p.line_rows, p.line_key))
            .unwrap_or_default();
        Self {
            marks,
            slots: HashMap::new(),
            source: HashMap::new(),
            line_rows,
            line_key,
            synced: None,
            chips: HashMap::new(),
            preview: None,
            dragging_boundary: false,
        }
    }

    /// 範囲の行と描くコミット、境目を引く先を、`history` の行で求め直す（世代が同じなら何もしない）。
    pub fn sync(&mut self, history: &HistorySnapshot) {
        if self.synced == Some(history.generation) {
            return;
        }
        self.synced = Some(history.generation);
        let rows_of: Vec<Option<u32>> = self
            .marks
            .rows
            .iter()
            .map(|r| history.row_of(&r.oid))
            .collect();
        (self.slots, self.source) = edit_map(&rows_of);
        // first-parent の並びは長いことがあるので、History か並びが替わったときだけ引き直す。
        let key = (history.generation, Rc::as_ptr(&self.marks.line));
        if self.line_key != Some(key) {
            self.line_rows = self
                .marks
                .line
                .iter()
                .enumerate()
                .filter_map(|(k, id)| Some((history.row_of(id)?, k)))
                .collect();
            self.line_rows.sort_unstable();
            self.line_key = Some(key);
        }
    }

    /// History の行に描くコミットの行（並べ替えていなければ同じ行）。
    pub fn shown_row(&self, row: u32) -> u32 {
        self.source.get(&row).copied().unwrap_or(row)
    }

    /// 範囲のいちばん下の行（その下に境目を引く）。
    pub fn boundary_row(&self) -> Option<u32> {
        let count = self.preview.unwrap_or(self.marks.rows.len());
        // 並びは History の上から順に出る（行の順に並べたものは、並びの順でもある）。
        self.line_rows
            .binary_search_by_key(&count.wrapping_sub(1), |&(_, k)| k)
            .ok()
            .map(|i| self.line_rows[i].0)
            .or_else(|| self.slots.keys().max().copied())
    }

    /// 境目を中身の高さ `y` へ引いた: いちばん近い並びのコミットの下へ（狭められないところへは動かさない）。
    /// 動いたら true。
    pub fn drag_boundary(&mut self, y: f32, row_height: f32) -> bool {
        let Some(k) = nearest_line_row(&self.line_rows, y, row_height) else {
            return false;
        };
        let count = k + 1;
        if (count >= self.marks.rows.len() || count >= self.marks.min_count)
            && self.preview != Some(count)
        {
            self.preview = Some(count);
            return true;
        }
        false
    }

    /// 範囲の行 `row` を運べるか（並べ替えられるときだけ）。
    pub fn can_drag(&self, row: u32) -> bool {
        !self.dragging_boundary && self.marks.reorderable && self.slots.contains_key(&row)
    }

    /// 範囲の `from` 番目の行を History の行 `row` に落とした先: 行に重ねたら `Squash`、行の上の境目なら `Move`
    /// （範囲のすぐ下の行の上なら最後へ）。落とせなければ None。
    pub fn drop_at(&self, from: usize, row: u32, onto: bool) -> Option<Dropped> {
        if onto {
            let &to = self.slots.get(&row)?;
            let target = self.marks.rows.get(to)?;
            return (to != from && !target.merge).then_some(Dropped::Squash(from, to));
        }
        let last = self.slots.keys().max().copied()?;
        let above = match self.slots.get(&row) {
            Some(&i) => i,
            None if row == last + 1 => self.marks.rows.len(),
            None => return None,
        };
        (above != from && above != from + 1).then_some(Dropped::Move(from, above))
    }
}

/// 範囲の行を落とした先。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dropped {
    /// `0` 番目の行を `1` 番目の行の上へ（範囲の数なら最後の行の下）。
    Move(usize, usize),
    /// `0` 番目の行を `1` 番目の行に重ねた（そのすぐ上へ動かしてまとめる）。
    Squash(usize, usize),
}

/// 範囲のコミットの History の行（範囲の順。History になければ None）から、History の行 → 範囲の何番目と、
/// History の行 → そこに描くコミットの行を作る。範囲の行は History の上から順に並ぶ。
fn edit_map(rows_of: &[Option<u32>]) -> (HashMap<u32, usize>, HashMap<u32, u32>) {
    let mut slots: Vec<u32> = rows_of.iter().flatten().copied().collect();
    slots.sort_unstable();
    let mut index = HashMap::new();
    let mut source = HashMap::new();
    for (i, slot) in slots.into_iter().enumerate() {
        index.insert(slot, i);
        if let Some(Some(row)) = rows_of.get(i) {
            source.insert(slot, *row);
        }
    }
    (index, source)
}

/// 境目の下端 `bottom(行)` が `y` にいちばん近い、並びの行（`line_rows` の 1 つ）の並びの何番目。
fn nearest_line_row(line_rows: &[(u32, usize)], y: f32, row_height: f32) -> Option<usize> {
    if line_rows.is_empty() || row_height <= 0.0 {
        return None;
    }
    let bottom = |row: u32| (row + 1) as f32 * row_height;
    let at = line_rows.partition_point(|&(row, _)| bottom(row) < y);
    [at.checked_sub(1), Some(at)]
        .into_iter()
        .flatten()
        .filter_map(|i| line_rows.get(i))
        .min_by(|a, b| (bottom(a.0) - y).abs().total_cmp(&(bottom(b.0) - y).abs()))
        .map(|&(_, k)| k)
}

/// 帯で押したもの。
#[derive(Clone, Debug)]
pub enum Event {
    Cancel,
    Apply,
}

pub struct Callbacks<'a> {
    pub event: Box<dyn Fn(Event) -> Callback<()> + 'a>,
    /// 積み重ねたブランチも動かすか。
    pub stacked: Callback<bool>,
}

/// ステータスバーの帯（要約・見通し・積み重ねたブランチ・Cancel・Apply Changes。GTK4 版の `Mode::bar`）。
pub fn bar(state: &State, callbacks: Callbacks) -> View {
    let (lines, conflicts) = state.preview.clone().unwrap_or_default();
    let texts = StackPanel::new()
        .spacing(2.0)
        .vertical_alignment(VerticalAlignment::Center)
        .children((
            TextBlock::new()
                .text(state.summary())
                .text_wrapping(TextWrapping::Wrap),
            if lines.is_empty() || !state.has_changes() {
                View::empty()
            } else {
                TextBlock::new()
                    .text(lines.join(" "))
                    .font_size(12.0)
                    .opacity(if conflicts { 1.0 } else { 0.75 })
                    .foreground(if conflicts {
                        Brush::Solid(Color::rgb(0xb2, 0x5f, 0x00))
                    } else {
                        Brush::Solid(crate::palette::foreground())
                    })
                    .max_lines(3)
                    .text_wrapping(TextWrapping::Wrap)
                    .into()
            },
        ));
    let buttons = StackPanel::new()
        .orientation(Orientation::Horizontal)
        .spacing(8.0)
        .grid_column(2)
        .vertical_alignment(VerticalAlignment::Center)
        .children((
            if state.stacked {
                let check = CheckBox::new()
                    .is_checked(state.moves_stacked())
                    .is_enabled(state.update_refs_available && !state.busy)
                    .on_is_checked_changed(callbacks.stacked.clone())
                    .content(TextBlock::new().text(tr!("Also move the stacked branches")));
                let tip = if state.update_refs_available {
                    tr!("For this edit only (git’s update-ref). Starts from rebase.updateRefs.")
                } else {
                    tr!("Moving stacked branches needs git 2.38 or newer.")
                };
                crate::tip::holder(check, tip, 0)
            } else {
                View::empty()
            },
            Button::new()
                .is_enabled(!state.busy)
                .on_click((callbacks.event)(Event::Cancel))
                .content(TextBlock::new().text(tr!("Cancel"))),
            Button::new()
                .style(ButtonStyle::Accent)
                .is_enabled(!state.busy && !state.resizing)
                .on_click((callbacks.event)(Event::Apply))
                .content(TextBlock::new().text(tr!("Apply Changes"))),
        ));
    Border::new()
        .background(Brush::Solid(crate::palette::note_band()))
        .padding(Thickness::new(10.0, 6.0, 10.0, 6.0))
        .content(
            Grid::new()
                .columns([GridLength::Auto, GridLength::Star(1.0), GridLength::Auto])
                .column_spacing(10.0)
                .children((
                    // 編集の印（macOS 版の pencil.and.list.clipboard、GTK4 版の EditList）。
                    Grid::new()
                        .vertical_alignment(VerticalAlignment::Center)
                        .children((crate::icons::icon("edit-mode", 20.0),)),
                    Grid::new().grid_column(1).children((texts,)),
                    buttons,
                )),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(n: usize) -> Vec<Row> {
        (0..n)
            .map(|i| Row::new(format!("{i:040}"), format!("m{i}"), false))
            .collect()
    }

    fn line(rows: &[Row]) -> Vec<(String, bool)> {
        rows.iter().map(|r| (r.oid.clone(), false)).collect()
    }

    fn state(n: usize, preset: Option<&Preset>) -> State {
        let r = rows(n);
        let l = line(&r);
        State::new(r, l, preset)
    }

    #[test]
    fn keys_are_the_first_letters_of_gits_words() {
        let keys: Vec<Option<Action>> = "prsfdeXq".chars().map(Action::from_key).collect();
        assert_eq!(
            keys,
            [
                Some(Action::Pick),
                Some(Action::Reword),
                Some(Action::Squash),
                Some(Action::Fixup),
                Some(Action::Drop),
                Some(Action::Edit),
                None,
                None,
            ]
        );
    }

    #[test]
    fn plan_is_oldest_first_with_the_combined_message() {
        let mut s = state(3, None);
        assert!(!s.has_changes());
        // 上（新しい）の行を古い方とまとめる。
        assert_eq!(s.choose(0, Action::Squash), Chosen::Changed);
        let steps = plan_steps(&s.rows);
        assert_eq!(steps[0]["oid"], s.rows[2].oid);
        assert_eq!(steps[2]["action"], "squash");
        assert_eq!(steps[2]["message"], "m1\n\nm0");
        // いちばん古い行は、まとめる相手がない。
        assert_eq!(s.choose(2, Action::Squash), Chosen::Nothing);
        // reword はダイアログを開いてから。
        assert_eq!(s.choose(1, Action::Reword), Chosen::Message);
        assert_eq!(s.message(0), Some((true, "m1\n\nm0".to_owned())));
    }

    #[test]
    fn moving_puts_the_row_above_the_target_and_resets_the_oldest_squash() {
        let mut s = state(3, Some(&Preset::Combine(vec!["0".repeat(40)])));
        assert_eq!(s.rows[0].action, Action::Squash);
        // いちばん上を最後の行の下へ: まとめる相手がなくなるので pick に戻す。
        assert!(s.move_row(0, 3));
        assert_eq!(s.rows[2].oid, "0".repeat(40));
        assert_eq!(s.rows[2].action, Action::Pick);
        // 同じ場所へは動かさない。
        assert!(!s.move_row(0, 1));
        // 直したメッセージを使う。
        s.set_message(0, "new\r\nbody");
        assert_eq!(s.rows[0].action, Action::Reword);
        assert_eq!(plan_steps(&s.rows)[2]["message"], "new\nbody");
        assert_eq!(s.marks().rows[0].subject.as_deref(), Some("new"));
    }

    #[test]
    fn squashing_by_drop_puts_the_row_above_the_target() {
        let mut s = state(4, None);
        let (a, c) = (s.rows[0].oid.clone(), s.rows[2].oid.clone());
        assert!(s.squash(0, 2));
        let oids: Vec<&str> = s.rows.iter().map(|r| r.oid.as_str()).collect();
        assert_eq!(oids[1], a);
        assert_eq!(oids[2], c);
        assert_eq!(s.rows[1].action, Action::Squash);
        assert!(!s.squash(1, 1));
    }

    #[test]
    fn the_range_shrinks_only_while_leaving_rows_are_untouched() {
        let mut s = state(4, None);
        assert_eq!(s.marks().min_count, 1);
        s.choose(2, Action::Drop);
        assert_eq!(s.marks().min_count, 3);
        assert!(!s.shrink(2));
        assert!(s.shrink(3));
        assert_eq!(s.rows.len(), 3);
        // 広げて読み足す（読んでいる間に変わっていれば捨てる）。
        let more = vec![Row::new("9".repeat(40), "m9".into(), false)];
        assert!(!s.extend(2, more.clone()));
        assert!(s.extend(3, more));
        assert_eq!(s.rows.len(), 4);
    }

    #[test]
    fn preview_marks_conflicts_and_stacked_branches() {
        let mut s = state(3, None);
        s.take_preview(&json!({
            "conflicts": [{ "oid": s.rows[0].oid, "files": ["a.txt"] }],
            "moving": [{ "name": "topic" }],
            "staying": [],
            "updateRefs": { "configured": false, "available": true },
        }));
        assert_eq!(s.conflicting[&s.rows[0].oid], ["a.txt"]);
        assert!(s.marks().rows[0].conflict);
        assert!(s.stacked && !s.moves_stacked());
        // 選び直した値だけを渡す。
        assert!(s.command().get("updateRefs").is_none());
        s.update_refs = Some(true);
        assert_eq!(s.command()["updateRefs"], true);
    }

    #[test]
    fn combining_several_squashes_into_the_oldest() {
        let r = rows(4);
        let ids = vec![r[0].oid.clone(), r[1].oid.clone(), r[2].oid.clone()];
        let l = line(&r);
        let s = State::new(r, l, Some(&Preset::Combine(ids)));
        let actions: Vec<Action> = s.rows.iter().map(|r| r.action).collect();
        assert_eq!(
            actions,
            [Action::Squash, Action::Squash, Action::Pick, Action::Pick]
        );
        // 3 つが 1 つのコミットになり、メッセージは古い順につなぐ。
        assert_eq!(plan_steps(&s.rows)[3]["message"], "m2\n\nm1\n\nm0");
    }

    #[test]
    fn a_dropped_commit_opens_moved() {
        let r = rows(3);
        let oid = r[2].oid.clone();
        let l = line(&r);
        let s = State::new(r, l, Some(&Preset::Move(oid.clone(), 0)));
        assert_eq!(s.rows[0].oid, oid);
        assert!(s.has_changes());
    }

    #[test]
    fn edit_rows_keep_their_places_and_draw_the_reordered_commits() {
        // 範囲のコミットが History の行 5・2・3 にある（並べ替えた後の並び）。
        let (slots, source) = edit_map(&[Some(5), Some(2), Some(3)]);
        assert_eq!(slots[&2], 0);
        assert_eq!(slots[&3], 1);
        assert_eq!(slots[&5], 2);
        // 行 2 には範囲の 0 番目（History の行 5 のコミット）を描く。
        assert_eq!(source[&2], 5);
        assert_eq!(source[&3], 2);
        assert_eq!(source[&5], 3);
    }

    #[test]
    fn the_boundary_snaps_to_the_nearest_commit_on_the_line() {
        let rows = [(0, 0), (1, 1), (4, 2)];
        // 行の高さ 10: 下端は 10・20・50。
        assert_eq!(nearest_line_row(&rows, 12.0, 10.0), Some(0));
        assert_eq!(nearest_line_row(&rows, 24.0, 10.0), Some(1));
        assert_eq!(nearest_line_row(&rows, 44.0, 10.0), Some(2));
        assert_eq!(nearest_line_row(&[], 44.0, 10.0), None);
    }

    #[test]
    fn drops_move_between_rows_and_squash_onto_them() {
        let s = state(3, None);
        let mut overlay = Overlay::new(s.marks(), None);
        (overlay.slots, overlay.source) = edit_map(&[Some(0), Some(1), Some(2)]);
        // 行 2 の上（行 1 の下）へ: 0 番目を 2 番目の上へ。
        assert_eq!(overlay.drop_at(0, 2, false), Some(Dropped::Move(0, 2)));
        // 範囲のすぐ下の行の上: 最後へ。
        assert_eq!(overlay.drop_at(0, 3, false), Some(Dropped::Move(0, 3)));
        // 自分のすぐ上・すぐ下は動かない、範囲の外は落とせない。
        assert_eq!(overlay.drop_at(1, 1, false), None);
        assert_eq!(overlay.drop_at(1, 2, false), None);
        assert_eq!(overlay.drop_at(0, 7, false), None);
        // 行に重ねる。
        assert_eq!(overlay.drop_at(0, 2, true), Some(Dropped::Squash(0, 2)));
        assert_eq!(overlay.drop_at(0, 0, true), None);
    }
}
