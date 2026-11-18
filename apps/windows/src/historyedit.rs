//! History の編集モード（GTK4 版の `historyedit.rs`、macOS 版の `HistoryEdit.swift`）。選んだコミットから HEAD まで
//! を並べ、行ごとに何をするか（そのまま・メッセージを直す・ひとつ古いコミットとまとめる・取り消す・止めて直す）を
//! 選び、↑・↓ で並べ替える。Apply Changes を押すまで何も変わらず、まとめて 1 回の rebase（コアの `rewrite` の
//! `plan`）で行う。Undo 1 回で全部戻る。行は History と同じく新しい順（上が HEAD）。

use amonhen_model::tr;
use serde_json::{Value, json};
use windows_reactor::*;

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
    fn name(self) -> &'static str {
        match self {
            Action::Pick => "pick",
            Action::Reword => "reword",
            Action::Squash => "squash",
            Action::Fixup => "fixup",
            Action::Drop => "drop",
            Action::Edit => "edit",
        }
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
#[derive(Clone, Debug)]
pub struct Row {
    pub oid: String,
    pub message: String,
    pub author: String,
    /// merge のコミット（作り直すだけで、することは選べない）。
    pub merge: bool,
    pub action: Action,
    /// 直したメッセージ（reword のとき）、またはまとめたコミットのメッセージ（squash のとき）。
    pub new_message: Option<String>,
}

impl Row {
    pub fn new(oid: String, message: String, author: String, merge: bool) -> Self {
        Self {
            oid,
            message,
            author,
            merge,
            action: Action::Pick,
            new_message: None,
        }
    }

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

/// 開くときの最初の変更（右クリックの近道から）。
#[derive(Clone, Debug, PartialEq)]
pub enum Preset {
    Reword(String),
    Combine(String),
    Drop(String),
}

/// 編集モードの状態。
pub struct State {
    pub rows: Vec<Row>,
    original: Vec<String>,
    /// 見通し（`rewritePreview` の行）と、コンフリクトしそうか。読むたびに数を進め、古い答えは捨てる。
    pub preview: Option<(Vec<String>, bool)>,
    pub generation: u64,
    /// メッセージを直している行と、打っている文。
    pub message_row: Option<usize>,
    pub message_text: String,
    /// 書き換えている間（Apply を押せない）。
    pub busy: bool,
}

impl State {
    pub fn new(rows: Vec<Row>, preset: Option<Preset>) -> Self {
        let original = rows.iter().map(|r| r.oid.clone()).collect();
        let mut state = Self {
            rows,
            original,
            preview: None,
            generation: 0,
            message_row: None,
            message_text: String::new(),
            busy: false,
        };
        let at = |oid: &str, rows: &[Row]| rows.iter().position(|r| r.oid.starts_with(oid));
        match preset {
            Some(Preset::Reword(oid)) => {
                if let Some(i) = at(&oid, &state.rows) {
                    state.set_action(i, Action::Reword);
                }
            }
            Some(Preset::Combine(oid)) => {
                if let Some(i) = at(&oid, &state.rows) {
                    state.set_action(i, Action::Squash);
                }
            }
            Some(Preset::Drop(oid)) => {
                if let Some(i) = at(&oid, &state.rows) {
                    state.set_action(i, Action::Drop);
                }
            }
            None => {}
        }
        state
    }

    pub fn has_changes(&self) -> bool {
        self.rows.iter().map(|r| &r.oid).ne(self.original.iter())
            || self.rows.iter().any(|r| r.action != Action::Pick)
    }

    /// `i` の行ですることを決める。メッセージを直すもの（reword・squash）なら、メッセージの欄を開く。
    pub fn set_action(&mut self, i: usize, action: Action) {
        if !choices(&self.rows, i).contains(&action) {
            return;
        }
        self.rows[i].action = action;
        match action {
            Action::Reword => {
                self.message_row = Some(i);
                self.message_text = self.rows[i]
                    .new_message
                    .clone()
                    .unwrap_or_else(|| self.rows[i].message.clone());
            }
            Action::Squash => {
                self.message_row = Some(i);
                self.message_text = self.rows[i]
                    .new_message
                    .clone()
                    .unwrap_or_else(|| combined_message(&self.rows, i));
            }
            _ => {
                self.rows[i].new_message = None;
                if self.message_row == Some(i) {
                    self.message_row = None;
                }
            }
        }
    }

    /// メッセージの欄を閉じる（`keep` なら打った文を使う）。
    pub fn close_message(&mut self, keep: bool) {
        if let Some(i) = self.message_row.take()
            && keep
            && let Some(row) = self.rows.get_mut(i)
        {
            let text = self.message_text.replace("\r\n", "\n").replace('\r', "\n");
            row.new_message = (!text.trim().is_empty()).then(|| text.trim_end().to_owned());
        }
    }

    /// `i` の行を 1 つ上（新しい方）か下（古い方）へ動かす。merge を越えては動かせない。まとめる相手がなくなった
    /// 行は、そのままに戻す。
    pub fn move_row(&mut self, i: usize, up: bool) -> bool {
        if self.rows.iter().any(|r| r.merge) {
            return false;
        }
        let j = if up {
            match i.checked_sub(1) {
                Some(j) => j,
                None => return false,
            }
        } else if i + 1 < self.rows.len() {
            i + 1
        } else {
            return false;
        };
        self.rows.swap(i, j);
        self.message_row = None;
        let last = self.rows.len() - 1;
        if self.rows[last].action.combines() {
            self.rows[last].action = Action::Pick;
            self.rows[last].new_message = None;
        }
        true
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
        json!({
            "op": "rewrite",
            "label": tr!("Edit {count} Commits", count = self.rows.len()),
            "edit": { "kind": "plan", "steps": plan_steps(&self.rows) },
        })
    }

    /// 見通しを読む命令。
    pub fn preview_command(&self) -> Value {
        json!({ "op": "rewritePreview", "edit": { "kind": "plan", "steps": plan_steps(&self.rows) } })
    }
}

/// 押したもの。
#[derive(Clone, Debug)]
pub enum Event {
    Move(usize, bool),
    MessageDone(bool),
    Cancel,
    Apply,
}

pub struct Callbacks<'a> {
    pub event: Box<dyn Fn(Event) -> Callback<()> + 'a>,
    pub choice: Box<dyn Fn(usize) -> Callback<Option<usize>> + 'a>,
    pub text: Callback<String>,
}

/// 選ぶ欄で `index` 番目を選んだときの、すること。
pub fn chosen(state: &State, row: usize, index: usize) -> Option<Action> {
    choices(&state.rows, row).get(index).copied()
}

pub fn view(state: &State, callbacks: Callbacks) -> View {
    let reorder = !state.rows.iter().any(|r| r.merge);
    let hint = if reorder {
        tr!(
            "Choose what to do with each commit and drag rows to reorder them. Nothing changes until Apply Changes."
        )
    } else {
        tr!(
            "Choose what to do with each commit. Merges are recreated as they are, so rows can’t be reordered across them. Nothing changes until Apply Changes."
        )
    };
    let rows = state.rows.iter().enumerate().map(|(i, row)| {
        let list = choices(&state.rows, i);
        let selected = list.iter().position(|a| *a == row.action).unwrap_or(0);
        let short = &row.oid[..row.oid.len().min(8)];
        let editing: View = if state.message_row == Some(i) {
            StackPanel::new()
                .spacing(6.0)
                .margin(Thickness::new(0.0, 6.0, 0.0, 6.0))
                .children((
                    TextBlock::new()
                        .text(if row.action == Action::Squash {
                            tr!("Message of the Combined Commit")
                        } else {
                            tr!("Edit Message")
                        })
                        .font_size(12.0)
                        .opacity(0.7),
                    TextBox::new()
                        .text(state.message_text.clone())
                        .accepts_return(true)
                        .min_height(80.0)
                        .on_text_changed(callbacks.text.clone()),
                    StackPanel::new()
                        .orientation(Orientation::Horizontal)
                        .spacing(8.0)
                        .horizontal_alignment(HorizontalAlignment::Right)
                        .children((
                            Button::new()
                                .on_click((callbacks.event)(Event::MessageDone(false)))
                                .content(TextBlock::new().text(tr!("Cancel"))),
                            Button::new()
                                .style(ButtonStyle::Accent)
                                .on_click((callbacks.event)(Event::MessageDone(true)))
                                .content(TextBlock::new().text(tr!("Change"))),
                        )),
                ))
        } else {
            View::empty()
        };
        let line = Grid::new()
            .columns([
                GridLength::Pixel(250.0),
                GridLength::Pixel(80.0),
                GridLength::Star(1.0),
                GridLength::Pixel(140.0),
                GridLength::Auto,
            ])
            .column_spacing(8.0)
            .children((
                ComboBox::new()
                    .items_source(list.iter().map(|a| a.title()).collect::<Vec<_>>())
                    .selected_index(selected)
                    .is_enabled(!row.merge && !state.busy)
                    .horizontal_alignment(HorizontalAlignment::Stretch)
                    .on_selection_changed((callbacks.choice)(i))
                    .grid_column(0)
                    .tooltip(row.action.tip()),
                TextBlock::new()
                    .text(short.to_owned())
                    .font_size(12.0)
                    .opacity(0.6)
                    .vertical_alignment(VerticalAlignment::Center)
                    .grid_column(1),
                TextBlock::new()
                    .text(row.subject())
                    .text_trimming(TextTrimming::CharacterEllipsis)
                    .opacity(if row.action == Action::Drop {
                        0.45
                    } else {
                        1.0
                    })
                    .vertical_alignment(VerticalAlignment::Center)
                    .grid_column(2),
                TextBlock::new()
                    .text(row.author.clone())
                    .font_size(12.0)
                    .opacity(0.6)
                    .text_trimming(TextTrimming::CharacterEllipsis)
                    .vertical_alignment(VerticalAlignment::Center)
                    .grid_column(3),
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(4.0)
                    .grid_column(4)
                    .children((
                        Button::new()
                            .is_enabled(reorder && i > 0 && !state.busy)
                            .on_click((callbacks.event)(Event::Move(i, true)))
                            .content(TextBlock::new().text("↑")),
                        Button::new()
                            .is_enabled(reorder && i + 1 < state.rows.len() && !state.busy)
                            .on_click((callbacks.event)(Event::Move(i, false)))
                            .content(TextBlock::new().text("↓")),
                    )),
            ));
        (
            format!("{}:{i}:{}", row.oid, list.len()),
            StackPanel::new()
                .margin(Thickness::new(0.0, 2.0, 0.0, 2.0))
                .children((line, editing)),
        )
    });
    let (lines, conflicts) = state.preview.clone().unwrap_or_default();
    let bar = Grid::new()
        .grid_row(2)
        .columns([GridLength::Star(1.0), GridLength::Auto])
        .column_spacing(8.0)
        .background(Brush::Solid(Color::rgb(0xf3, 0xf3, 0xf3)))
        .children((
            StackPanel::new()
                .spacing(2.0)
                .margin(Thickness::new(16.0, 8.0, 0.0, 8.0))
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
                            .opacity(if conflicts { 1.0 } else { 0.65 })
                            .foreground(if conflicts {
                                Brush::Solid(Color::rgb(0xb2, 0x5f, 0x00))
                            } else {
                                Brush::Solid(Color::rgb(0, 0, 0))
                            })
                            .text_wrapping(TextWrapping::Wrap)
                            .into()
                    },
                )),
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(8.0)
                .grid_column(1)
                .margin(Thickness::new(0.0, 8.0, 16.0, 8.0))
                .vertical_alignment(VerticalAlignment::Center)
                .children((
                    Button::new()
                        .is_enabled(!state.busy)
                        .on_click((callbacks.event)(Event::Cancel))
                        .content(TextBlock::new().text(tr!("Cancel"))),
                    Button::new()
                        .style(ButtonStyle::Accent)
                        .is_enabled(!state.busy && state.message_row.is_none())
                        .on_click((callbacks.event)(Event::Apply))
                        .content(TextBlock::new().text(tr!("Apply Changes"))),
                )),
        ));
    Grid::new()
        .rows([GridLength::Auto, GridLength::Star(1.0), GridLength::Auto])
        .children((
            StackPanel::new()
                .spacing(4.0)
                .margin(Thickness::new(16.0, 10.0, 16.0, 6.0))
                .children((
                    TextBlock::new().text(tr!("Edit Commits")).font_size(15.0),
                    TextBlock::new()
                        .text(hint)
                        .font_size(12.0)
                        .opacity(0.65)
                        .text_wrapping(TextWrapping::Wrap),
                )),
            ScrollViewer::new().grid_row(1).content(
                StackPanel::new()
                    .margin(Thickness::new(16.0, 0.0, 16.0, 8.0))
                    .children((View::keyed_fragment(rows.collect::<Vec<_>>()),)),
            ),
            bar,
        ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(n: usize) -> Vec<Row> {
        (0..n)
            .map(|i| Row::new(format!("{i:040}"), format!("m{i}"), "a".into(), false))
            .collect()
    }

    #[test]
    fn plan_is_oldest_first_with_the_combined_message() {
        let mut s = State::new(rows(3), None);
        assert!(!s.has_changes());
        // 上（新しい）の行を古い方とまとめる。
        s.set_action(0, Action::Squash);
        s.close_message(false);
        let steps = plan_steps(&s.rows);
        assert_eq!(steps[0]["oid"], s.rows[2].oid);
        assert_eq!(steps[2]["action"], "squash");
        assert_eq!(steps[2]["message"], "m1\n\nm0");
        // いちばん古い行は、まとめる相手がない。
        assert!(!choices(&s.rows, 2).contains(&Action::Squash));
    }

    #[test]
    fn moving_resets_a_combine_without_an_older_row() {
        let mut s = State::new(rows(2), Some(Preset::Combine("0".repeat(40))));
        assert_eq!(s.rows[0].action, Action::Squash);
        assert!(s.move_row(0, false));
        assert_eq!(s.rows[1].action, Action::Pick);
        assert!(s.has_changes());
        // 直したメッセージを使う。
        s.set_action(0, Action::Reword);
        s.message_text = "new\r\nbody".into();
        s.close_message(true);
        assert_eq!(plan_steps(&s.rows)[1]["message"], "new\nbody");
    }
}
