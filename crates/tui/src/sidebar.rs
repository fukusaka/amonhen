//! サイドバー: メインの画面を切り替える Workspace の行と、Branches・Remotes・Tags。

use amonhen_model::tr;
use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Modifier, Style};
use serde_json::Value;
use unicode_width::UnicodeWidthStr;

use crate::diff::DIM;
use crate::history::fit;

/// サイドバーの幅。
pub const WIDTH: u16 = 26;

const SELECTED: Color = Color::Indexed(238);
const SELECTED_UNFOCUSED: Color = Color::Indexed(236);

/// 選んだ行が指すもの。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Picked {
    Changes,
    History,
    Reflog,
    /// ref が指すコミット（16 進の ID）。
    Commit(String),
}

enum Row {
    /// 節の見出し（選べない）。
    Header(String),
    Changes,
    History,
    Reflog,
    Ref {
        /// `branch`・`remote`・`tag`・`stash`・`worktree`（名前はその場所）。
        kind: &'static str,
        name: String,
        /// 出す文字（stash はメッセージ、ほかは名前）。
        label: String,
        target: String,
        /// 今のブランチ。
        current: bool,
        ahead: u64,
        behind: u64,
    },
}

impl Row {
    /// 読み直しても同じ行を選び続けるための名前。
    fn key(&self) -> Option<String> {
        match self {
            Row::Header(_) => None,
            Row::Changes => Some("view:changes".to_owned()),
            Row::History => Some("view:history".to_owned()),
            Row::Reflog => Some("view:reflog".to_owned()),
            Row::Ref { name, .. } => Some(format!("ref:{name}")),
        }
    }
}

pub struct Sidebar {
    /// 最後に届いた `refs` の答え（絞り込みを替えたときに、行を作り直す）。
    refs: Value,
    /// 名前にこれを含む ref だけを出す（大文字小文字は区別しない）。空なら、すべて。
    filter: String,
    rows: Vec<Row>,
    selected: usize,
    top: usize,
    height: u16,
}

impl Default for Sidebar {
    fn default() -> Self {
        Self {
            refs: Value::Null,
            filter: String::new(),
            rows: vec![
                Row::Header(tr!("Workspace")),
                Row::Changes,
                Row::History,
                Row::Reflog,
            ],
            // 開いたときのメインの画面は History。
            selected: 2,
            top: 0,
            height: 0,
        }
    }
}

impl Sidebar {
    /// `refs` の答えから、行を作り直す。選んでいた行は名前で引き継ぐ。
    pub fn set_refs(&mut self, refs: &Value) {
        self.refs = refs.clone();
        self.rebuild();
    }

    /// 絞り込みの語を替える。
    pub fn set_filter(&mut self, filter: &str) {
        self.filter = filter.to_lowercase();
        self.rebuild();
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    fn rebuild(&mut self) {
        let refs = &self.refs;
        let selected = self.rows.get(self.selected).and_then(Row::key);
        let head = refs["headBranch"].as_str();
        let mut rows = vec![
            Row::Header(tr!("Workspace")),
            Row::Changes,
            Row::History,
            Row::Reflog,
        ];
        for (kind, title) in [
            ("branch", tr!("Branches")),
            ("remote", tr!("Remotes")),
            ("tag", tr!("Tags")),
            ("stash", tr!("Stashes")),
        ] {
            let mut of_kind = refs["entries"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|e| e["kind"] == kind)
                .filter_map(|e| {
                    let name = e["name"].as_str()?;
                    // stash は、名前（`stash@{0}`）ではなくメッセージを出す。
                    let label = match kind {
                        "stash" => e["detail"].as_str().unwrap_or(name),
                        _ => name,
                    };
                    if !label.to_lowercase().contains(&self.filter) {
                        return None;
                    }
                    Some(Row::Ref {
                        kind,
                        name: name.to_owned(),
                        label: label.to_owned(),
                        target: e["target"].as_str()?.to_owned(),
                        current: kind == "branch" && Some(name) == head,
                        ahead: e["ahead"].as_u64().unwrap_or(0),
                        behind: e["behind"].as_u64().unwrap_or(0),
                    })
                })
                .peekable();
            if of_kind.peek().is_some() {
                rows.push(Row::Header(title));
                rows.extend(of_kind);
            }
        }
        // worktree は、2 つ以上あるときだけ並べる（フォルダの名前と、そこのブランチ）。
        let worktrees: Vec<&Value> = refs["worktrees"].as_array().into_iter().flatten().collect();
        if worktrees.len() > 1 {
            let mut of_kind = worktrees
                .iter()
                .filter_map(|w| {
                    let path = w["path"].as_str()?;
                    let folder = path.rsplit(['/', '\\']).next().unwrap_or(path);
                    let label = match w["branch"].as_str() {
                        Some(branch) => format!("{folder} ({branch})"),
                        None => folder.to_owned(),
                    };
                    if !label.to_lowercase().contains(&self.filter) {
                        return None;
                    }
                    Some(Row::Ref {
                        kind: "worktree",
                        name: path.to_owned(),
                        label,
                        target: w["head"].as_str().unwrap_or("").to_owned(),
                        current: w["current"] == true,
                        ahead: 0,
                        behind: 0,
                    })
                })
                .peekable();
            if of_kind.peek().is_some() {
                rows.push(Row::Header(tr!("Worktrees")));
                rows.extend(of_kind);
            }
        }
        self.rows = rows;
        self.selected = selected
            .and_then(|key| {
                self.rows
                    .iter()
                    .position(|r| r.key().as_ref() == Some(&key))
            })
            .unwrap_or(2);
        self.clamp_top();
    }

    /// メインの画面が替わった（数字のキーなど）ら、その行を選ぶ。ref を選んでいる間はそのまま。
    pub fn show_view(&mut self, picked: Picked) {
        // History で ref を選んでいる間は、そのまま。
        if matches!(self.rows.get(self.selected), Some(Row::Ref { .. }))
            && picked == Picked::History
        {
            return;
        }
        self.selected = match picked {
            Picked::Changes => 1,
            Picked::Reflog => 3,
            _ => 2,
        };
        self.clamp_top();
    }

    pub fn set_height(&mut self, height: u16) {
        self.height = height;
        self.clamp_top();
    }

    fn clamp_top(&mut self) {
        let height = usize::from(self.height.max(1));
        if self.selected < self.top {
            // 節の最初の行なら、見出しも見せる。
            self.top = match self.selected.checked_sub(1) {
                Some(above) if matches!(self.rows[above], Row::Header(_)) => above,
                _ => self.selected,
            };
        } else if self.selected >= self.top + height {
            self.top = self.selected + 1 - height;
        }
        self.top = self.top.min(self.rows.len().saturating_sub(height));
    }

    pub fn page(&self) -> i64 {
        i64::from(self.height.max(2)) - 1
    }

    /// 選択を動かす（見出しは飛ばす）。
    pub fn move_selection(&mut self, delta: i64) -> bool {
        let step: i64 = if delta < 0 { -1 } else { 1 };
        let mut at = self.selected as i64;
        let mut last = at;
        for _ in 0..delta.unsigned_abs() {
            let mut next = at + step;
            while next >= 0
                && (next as usize) < self.rows.len()
                && matches!(self.rows[next as usize], Row::Header(_))
            {
                next += step;
            }
            if next < 0 || next as usize >= self.rows.len() {
                break;
            }
            at = next;
            last = at;
        }
        let changed = last as usize != self.selected;
        self.selected = last as usize;
        self.clamp_top();
        changed
    }

    pub fn scroll(&mut self, delta: i64) -> bool {
        let last = self
            .rows
            .len()
            .saturating_sub(usize::from(self.height.max(1)));
        let top = (self.top as i64 + delta).clamp(0, last as i64) as usize;
        let changed = top != self.top;
        self.top = top;
        changed
    }

    /// `line` 行目（見えている中で）を選ぶ。選べる行なら true。
    pub fn click(&mut self, line: u16) -> bool {
        let index = self.top + usize::from(line);
        match self.rows.get(index) {
            Some(Row::Header(_)) | None => false,
            Some(_) => {
                self.selected = index;
                true
            }
        }
    }

    /// 選んでいる行が指すもの。
    pub fn picked(&self) -> Option<Picked> {
        match self.rows.get(self.selected)? {
            Row::Header(_) => None,
            Row::Changes => Some(Picked::Changes),
            Row::History => Some(Picked::History),
            Row::Reflog => Some(Picked::Reflog),
            Row::Ref { target, .. } => Some(Picked::Commit(target.clone())),
        }
    }

    /// 選んでいる ref の種類（`branch`・`remote`・`tag`）・名前・指すコミット。
    /// ローカルのブランチ `name` の上流（`origin/main` など）。なければ、なくなっていれば None。
    pub fn upstream_of(&self, name: &str) -> Option<String> {
        let entry = self.refs["entries"]
            .as_array()?
            .iter()
            .find(|e| e["kind"] == "branch" && e["name"] == name)?;
        (entry["gone"] != true)
            .then(|| entry["detail"].as_str().map(str::to_owned))
            .flatten()
    }

    pub fn picked_ref(&self) -> Option<(&'static str, &str, &str)> {
        match self.rows.get(self.selected)? {
            Row::Ref {
                kind, name, target, ..
            } => Some((kind, name, target)),
            _ => None,
        }
    }

    /// 選んでいる行が、今のもの（今のブランチ、開いている worktree）か。
    pub fn picked_is_current(&self) -> bool {
        matches!(
            self.rows.get(self.selected),
            Some(Row::Ref { current: true, .. })
        )
    }

    pub fn draw(&self, area: Rect, buf: &mut Buffer, focused: bool, changes: usize) {
        if area.width < 4 || area.height == 0 {
            return;
        }
        let width = usize::from(area.width);
        for (y, (index, row)) in (area.y..area.bottom()).zip(
            self.rows
                .iter()
                .enumerate()
                .skip(self.top)
                .take(usize::from(area.height)),
        ) {
            match row {
                Row::Header(title) => {
                    buf.set_stringn(
                        area.x,
                        y,
                        title,
                        width,
                        Style::new().fg(DIM).add_modifier(Modifier::BOLD),
                    );
                }
                Row::Changes => {
                    buf.set_stringn(area.x + 2, y, tr!("Changes"), width - 2, Style::new());
                    if changes > 0 {
                        right(area, y, buf, &changes.to_string(), Style::new().fg(DIM));
                    }
                }
                Row::History => {
                    buf.set_stringn(area.x + 2, y, tr!("History"), width - 2, Style::new());
                }
                Row::Reflog => {
                    buf.set_stringn(area.x + 2, y, tr!("Reflog"), width - 2, Style::new());
                }
                Row::Ref {
                    label: name,
                    current,
                    ahead,
                    behind,
                    ..
                } => {
                    let mut counts = String::new();
                    if *ahead > 0 {
                        counts.push_str(&format!("↑{ahead}"));
                    }
                    if *behind > 0 {
                        counts.push_str(&format!("↓{behind}"));
                    }
                    if *current {
                        buf[(area.x, y)].set_char('●').set_fg(Color::Green);
                    }
                    let room = width.saturating_sub(3 + counts.width());
                    let style = match current {
                        true => Style::new().add_modifier(Modifier::BOLD),
                        false => Style::new(),
                    };
                    buf.set_stringn(area.x + 2, y, fit(name, room), room, style);
                    if !counts.is_empty() {
                        right(area, y, buf, &counts, Style::new().fg(DIM));
                    }
                }
            }
            if index == self.selected {
                buf.set_style(
                    Rect::new(area.x, y, area.width, 1),
                    Style::new().bg(if focused {
                        SELECTED
                    } else {
                        SELECTED_UNFOCUSED
                    }),
                );
            }
        }
    }
}

/// 行の右端に寄せて書く。
fn right(area: Rect, y: u16, buf: &mut Buffer, text: &str, style: Style) {
    let width = text.width() as u16 + 1;
    if width < area.width {
        buf.set_string(area.right() - width, y, text, style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sidebar() -> Sidebar {
        let mut sidebar = Sidebar::default();
        sidebar.set_height(20);
        sidebar.set_refs(&json!({
            "headBranch": "main",
            "entries": [
                {"kind": "branch", "name": "main", "target": "aa", "ahead": 2, "behind": 0},
                {"kind": "branch", "name": "topic", "target": "bb"},
                {"kind": "tag", "name": "v1", "target": "cc"},
            ],
        }));
        sidebar
    }

    /// 選択は見出しを飛ばして動く
    #[test]
    fn selection_skips_headers() {
        let mut sidebar = sidebar();
        assert_eq!(sidebar.picked(), Some(Picked::History));
        // History の次は Reflog、その次は Branches の見出しを飛ばして main。
        assert!(sidebar.move_selection(2));
        assert_eq!(sidebar.picked(), Some(Picked::Commit("aa".to_owned())));
        sidebar.move_selection(2);
        assert_eq!(sidebar.picked(), Some(Picked::Commit("cc".to_owned())));
        // 端では止まる。
        assert!(!sidebar.move_selection(1));
        sidebar.move_selection(-10);
        assert_eq!(sidebar.picked(), Some(Picked::Changes));
    }

    /// ref を読み直しても、同じ名前の行を選び続ける
    #[test]
    fn selection_survives_a_reload() {
        let mut sidebar = sidebar();
        sidebar.move_selection(3);
        assert_eq!(sidebar.picked(), Some(Picked::Commit("bb".to_owned())));
        sidebar.set_refs(&json!({
            "headBranch": "main",
            "entries": [
                {"kind": "branch", "name": "another", "target": "dd"},
                {"kind": "branch", "name": "topic", "target": "ee"},
            ],
        }));
        assert_eq!(sidebar.picked(), Some(Picked::Commit("ee".to_owned())));
    }
}
