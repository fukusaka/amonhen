//! History の編集モード（アプリの「History's edit mode」、docs/ux.md）。選んだコミットから HEAD までを並べ、行ごとに
//! 何をするか（そのまま・メッセージを直す・ひとつ古いコミットとまとめる・取り消す・止めて直す）を選び、並べ替える。
//! 当てるまで何も変わらず、まとめて 1 回の書き換え（コアの `rewrite` の `plan`）で行う。Undo 1 回で全部戻る。行は
//! History と同じく新しい順（上が HEAD）。

use amonhen_model::tr;
use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Modifier, Style};
use serde_json::{Value, json};

/// 行ですること。git の対話的な rebase の言葉（`name`）は、コアに渡すときと一覧の頭に使う。
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
    /// コアの `plan` の言葉（git の todo と同じ）。
    pub fn name(self) -> &'static str {
        match self {
            Self::Pick => "pick",
            Self::Reword => "reword",
            Self::Squash => "squash",
            Self::Fixup => "fixup",
            Self::Drop => "drop",
            Self::Edit => "edit",
        }
    }

    /// アプリと同じ名前。
    pub fn title(self) -> String {
        match self {
            Self::Pick => tr!("Keep"),
            Self::Reword => tr!("Edit Message"),
            Self::Squash => tr!("Combine with Older"),
            Self::Fixup => tr!("Combine with Older, Keep Its Message"),
            Self::Drop => "Drop".to_owned(),
            Self::Edit => tr!("Stop to Change Files"),
        }
    }
}

/// 1 行（1 コミット）。
pub struct Row {
    pub oid: String,
    pub merge: bool,
    /// 今のメッセージ（1 行目が件名）。
    pub message: String,
    pub action: Action,
    /// 直したメッセージ（reword の新しいメッセージ、squash ならまとめたコミットのメッセージ）。
    pub new_message: Option<String>,
}

impl Row {
    pub fn new(oid: String, merge: bool, message: String) -> Self {
        Self {
            oid,
            merge,
            message,
            action: Action::Pick,
            new_message: None,
        }
    }

    fn subject(&self) -> &str {
        let message = self.new_message.as_deref().unwrap_or(&self.message);
        message.lines().next().unwrap_or("")
    }
}

pub struct EditMode {
    pub rows: Vec<Row>,
    original: Vec<String>,
    pub selected: usize,
}

impl EditMode {
    pub fn new(rows: Vec<Row>) -> Self {
        let original = rows.iter().map(|r| r.oid.clone()).collect();
        Self {
            rows,
            original,
            selected: 0,
        }
    }

    /// `i` の行を、ひとつ古い行とまとめられるか（いちばん古い行と merge のすぐ上の行は、まとめる相手がない）。
    fn can_combine(&self, i: usize) -> bool {
        !self.rows[i].merge && i + 1 < self.rows.len() && !self.rows[i + 1].merge
    }

    /// 一番古い行から続く `oids`（新しい順）を 1 つにまとめる行にする（History の複数選択の Combine）。`oids` が
    /// 一番古い行で終わる、続いた merge でない行でなければ、何もせずに false。
    pub fn combine_last(&mut self, oids: &[String]) -> bool {
        let Some(start) = self.rows.len().checked_sub(oids.len()) else {
            return false;
        };
        let tail = &self.rows[start..];
        if oids.len() < 2
            || tail
                .iter()
                .zip(oids)
                .any(|(r, oid)| r.oid != *oid || r.merge)
        {
            return false;
        }
        let last = self.rows.len() - 1;
        for row in &mut self.rows[start..last] {
            row.action = Action::Squash;
        }
        self.selected = start;
        true
    }

    /// 選んでいる行ですることを決める。できなければ、その理由。
    pub fn set(&mut self, action: Action) -> Result<(), String> {
        let i = self.selected;
        if self.rows[i].merge {
            return Err(tr!("A merge is recreated as it is"));
        }
        if matches!(action, Action::Squash | Action::Fixup) && !self.can_combine(i) {
            return Err(tr!("There is no older commit to combine it with"));
        }
        self.rows[i].action = action;
        if matches!(
            action,
            Action::Pick | Action::Drop | Action::Fixup | Action::Edit
        ) {
            self.rows[i].new_message = None;
        }
        Ok(())
    }

    /// 選んでいる行を `delta`（負なら新しい方へ）だけ動かす。merge をまたいで並べ替えはできない。
    pub fn move_selected(&mut self, delta: i64) -> Result<(), String> {
        if self.rows.iter().any(|r| r.merge) {
            return Err(tr!(
                "Merges are recreated as they are, so rows can’t be reordered across them"
            ));
        }
        let to = self.selected as i64 + delta;
        if to < 0 || to >= self.rows.len() as i64 {
            return Ok(());
        }
        let row = self.rows.remove(self.selected);
        self.rows.insert(to as usize, row);
        self.selected = to as usize;
        // いちばん古い行には、まとめる相手がない。
        if let Some(last) = self.rows.last_mut()
            && matches!(last.action, Action::Squash | Action::Fixup)
        {
            last.action = Action::Pick;
            last.new_message = None;
        }
        Ok(())
    }

    pub fn has_changes(&self) -> bool {
        self.rows.iter().map(|r| &r.oid).ne(self.original.iter())
            || self.rows.iter().any(|r| r.action != Action::Pick)
    }

    /// 選んだことの要約（アプリの帯と同じ）。
    pub fn summary(&self) -> String {
        let count = |a: Action| self.rows.iter().filter(|r| r.action == a).count();
        let mut parts = Vec::new();
        if self.rows.iter().map(|r| &r.oid).ne(self.original.iter()) {
            parts.push(tr!("reordered"));
        }
        let rewords = count(Action::Reword);
        if rewords > 0 {
            parts.push(tr!("{count} messages", count = rewords));
        }
        let combined = count(Action::Squash) + count(Action::Fixup);
        if combined > 0 {
            parts.push(tr!("{count} combined", count = combined));
        }
        let dropped = count(Action::Drop);
        if dropped > 0 {
            parts.push(tr!("{count} dropped", count = dropped));
        }
        let edits = count(Action::Edit);
        if edits > 0 {
            parts.push(tr!("{count} to change files", count = edits));
        }
        match parts.is_empty() {
            true => tr!("No changes yet."),
            false => tr!(
                "Changes: {parts}.",
                parts = amonhen_model::i18n::list(
                    &parts.iter().map(String::as_str).collect::<Vec<_>>()
                )
            ),
        }
    }

    /// `i` の行（squash）までのまとまりのメッセージを、古い順につないだもの（git の squash の既定と同じ）。
    fn combined_message(&self, i: usize) -> String {
        let mut j = i;
        while j + 1 < self.rows.len()
            && matches!(self.rows[j].action, Action::Squash | Action::Fixup)
        {
            j += 1;
        }
        self.rows[i..=j]
            .iter()
            .rev()
            .filter(|r| r.action != Action::Fixup)
            .map(|r| r.new_message.as_deref().unwrap_or(&r.message).trim_end())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// 選んでいる行のメッセージを直すときに、エディタに入れるもの。merge の行は直せない。
    pub fn message_to_edit(&self) -> Option<String> {
        let row = &self.rows[self.selected];
        if row.merge {
            return None;
        }
        Some(match (&row.new_message, row.action) {
            (Some(message), _) => message.clone(),
            (None, Action::Squash) => self.combined_message(self.selected),
            (None, _) => row.message.clone(),
        })
    }

    /// 選んでいる行のメッセージを直した。squash ならまとめたコミットのメッセージ、ほかは reword にする。
    pub fn set_message(&mut self, message: String) {
        let row = &mut self.rows[self.selected];
        if row.action != Action::Squash {
            row.action = Action::Reword;
        }
        row.new_message = Some(message);
    }

    /// コアに渡す並び（古い順）。まとまり（squash を含む）の最後の squash に、まとめたメッセージを付ける。
    pub fn plan(&self) -> Value {
        let n = self.rows.len();
        let ordered: Vec<&Row> = self.rows.iter().rev().collect();
        let steps: Vec<Value> = ordered
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let mut step = json!({"oid": row.oid, "action": row.action.name()});
                match row.action {
                    Action::Reword => {
                        step["message"] = json!(row.new_message.as_deref().unwrap_or(&row.message));
                    }
                    Action::Squash => {
                        let later_squash = ordered[i + 1..]
                            .iter()
                            .take_while(|r| matches!(r.action, Action::Squash | Action::Fixup))
                            .any(|r| r.action == Action::Squash);
                        if !later_squash {
                            step["message"] = json!(
                                row.new_message
                                    .clone()
                                    .unwrap_or_else(|| self.combined_message(n - 1 - i))
                            );
                        }
                    }
                    _ => {}
                }
                step
            })
            .collect();
        json!({"kind": "plan", "steps": steps})
    }

    /// 枠（題は呼ぶ側）の中に描く。上の行は案内、下の 2 行は要約とキー。
    pub fn draw(&self, area: Rect, buf: &mut Buffer) {
        if area.height < 4 || area.width < 20 {
            return;
        }
        let width = usize::from(area.width);
        let dim = Style::new().fg(Color::Indexed(244));
        let hint = match self.rows.iter().any(|r| r.merge) {
            false => tr!(
                "Choose what to do with each commit and move rows to reorder them. Nothing changes until you apply."
            ),
            true => tr!(
                "Choose what to do with each commit. Merges are recreated as they are, so rows can’t be reordered across them. Nothing changes until you apply."
            ),
        };
        buf.set_stringn(area.x, area.y, hint, width, dim);
        let list = Rect::new(area.x, area.y + 1, area.width, area.height - 3);
        let rows = usize::from(list.height);
        let top = (self.selected + 1).saturating_sub(rows);
        let label_width = 38.min(width / 2);
        for (y, i) in (list.y..list.bottom()).zip(top..self.rows.len()) {
            let row = &self.rows[i];
            let selected = i == self.selected;
            let base = match selected {
                true => Style::new().add_modifier(Modifier::REVERSED),
                false => Style::new(),
            };
            if selected {
                for x in list.x..list.right() {
                    buf[(x, y)].set_style(base);
                }
            }
            let label = match row.merge {
                true => tr!("Recreate the merge"),
                false => row.action.title(),
            };
            let label_style = match (selected, row.action) {
                (true, _) => base,
                (false, Action::Pick) => dim,
                (false, _) => Style::new().fg(Color::Cyan),
            };
            buf.set_stringn(list.x, y, &label, label_width, label_style);
            let x = list.x + label_width as u16 + 1;
            let id = &row.oid[..row.oid.len().min(8)];
            buf.set_stringn(x, y, id, 8, if selected { base } else { dim });
            let text_style = match (selected, row.action) {
                (true, _) => base,
                (false, Action::Drop) => dim.add_modifier(Modifier::CROSSED_OUT),
                (false, Action::Squash | Action::Fixup) => dim,
                (false, _) => Style::new(),
            };
            let room = width.saturating_sub(label_width + 10);
            buf.set_stringn(x + 9, y, row.subject(), room, text_style);
        }
        let bottom = area.bottom();
        buf.set_stringn(area.x, bottom - 2, self.summary(), width, Style::new());
        buf.set_stringn(
            area.x,
            bottom - 1,
            tr!(
                "p keep  r message  s combine  f combine, keep message  d drop  e stop  J K move  Enter apply  Esc cancel"
            ),
            width,
            dim,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode() -> EditMode {
        // 新しい順: c（HEAD）、b、a。
        EditMode::new(vec![
            Row::new("c".into(), false, "Third\n".into()),
            Row::new("b".into(), false, "Second\n".into()),
            Row::new("a".into(), false, "First\n".into()),
        ])
    }

    fn steps(mode: &EditMode) -> Vec<(String, String, Option<String>)> {
        mode.plan()["steps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| {
                (
                    s["oid"].as_str().unwrap().to_owned(),
                    s["action"].as_str().unwrap().to_owned(),
                    s["message"].as_str().map(str::to_owned),
                )
            })
            .collect()
    }

    /// まとめると、まとまりの最後の squash に、古い順につないだメッセージが付く（fixup のものは入らない）。
    #[test]
    fn combining_joins_the_messages_oldest_first() {
        let mut mode = mode();
        mode.selected = 1;
        mode.set(Action::Squash).unwrap();
        mode.selected = 0;
        mode.set(Action::Fixup).unwrap();
        assert_eq!(
            steps(&mode),
            vec![
                ("a".into(), "pick".into(), None),
                ("b".into(), "squash".into(), Some("First\n\nSecond".into())),
                ("c".into(), "fixup".into(), None),
            ]
        );
        assert_eq!(mode.summary(), "Changes: 2 combined.");
        // いちばん古い行には、まとめる相手がない。
        mode.selected = 2;
        assert!(mode.set(Action::Squash).is_err());
    }

    /// 並べ替えると、いちばん古い行になったまとめる行は、そのままに戻る。
    #[test]
    fn moving_keeps_the_oldest_row_from_combining() {
        let mut mode = mode();
        mode.selected = 1;
        mode.set(Action::Fixup).unwrap();
        mode.move_selected(1).unwrap();
        assert_eq!(mode.selected, 2);
        assert_eq!(
            steps(&mode),
            vec![
                ("b".into(), "pick".into(), None),
                ("a".into(), "pick".into(), None),
                ("c".into(), "pick".into(), None),
            ]
        );
        assert!(mode.has_changes());
        assert_eq!(mode.summary(), "Changes: reordered.");
        // 戻せば、変更はない。
        mode.move_selected(-1).unwrap();
        assert!(!mode.has_changes());
    }

    /// メッセージを直すと reword になる。squash の行では、まとめたコミットのメッセージになる。
    #[test]
    fn editing_a_message() {
        let mut mode = mode();
        assert_eq!(mode.message_to_edit().unwrap(), "Third\n");
        mode.set_message("Better third".into());
        mode.selected = 1;
        mode.set(Action::Squash).unwrap();
        assert_eq!(mode.message_to_edit().unwrap(), "First\n\nSecond");
        mode.set_message("Both".into());
        assert_eq!(
            steps(&mode),
            vec![
                ("a".into(), "pick".into(), None),
                ("b".into(), "squash".into(), Some("Both".into())),
                ("c".into(), "reword".into(), Some("Better third".into())),
            ]
        );
    }

    /// merge がある並びは、並べ替えられず、merge の行ですることも変えられない。
    #[test]
    fn merges_stay_as_they_are() {
        let mut mode = EditMode::new(vec![
            Row::new("c".into(), false, "Third\n".into()),
            Row::new("m".into(), true, "Merge\n".into()),
            Row::new("a".into(), false, "First\n".into()),
        ]);
        assert!(mode.move_selected(1).is_err());
        // merge のすぐ上の行には、まとめる相手がない。
        assert!(mode.set(Action::Squash).is_err());
        mode.selected = 1;
        assert!(mode.set(Action::Drop).is_err());
        assert!(mode.message_to_edit().is_none());
    }
}
