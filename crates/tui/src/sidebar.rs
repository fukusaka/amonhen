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
    /// Range Diff（開いている間だけの行）。
    RangeDiff,
    /// ref が指すコミット（16 進の ID）。
    Commit(String),
}

enum Row {
    /// 節の見出し（選べない）と、右に出す数（0 なら出さない）。
    Header(String, usize),
    Changes,
    /// コンフリクトがある間だけ（選ぶと Working Copy）。
    Conflicts,
    History,
    Reflog,
    /// Range Diff を開いている間だけ（アプリと同じ）。
    RangeDiff,
    /// Branches の中のまとまり（Inactive・Archived）。Enter で開く・畳む。
    Group {
        key: &'static str,
        title: String,
        count: usize,
        open: bool,
    },
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
            Row::Header(..) => None,
            Row::Changes => Some("view:changes".to_owned()),
            Row::Conflicts => Some("view:conflicts".to_owned()),
            Row::History => Some("view:history".to_owned()),
            Row::Reflog => Some("view:reflog".to_owned()),
            Row::RangeDiff => Some("view:range-diff".to_owned()),
            Row::Group { key, .. } => Some(format!("group:{key}")),
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
    /// コンフリクトのあるファイルの数（あれば Workspace に行を出す）。
    conflicts: usize,
    /// 署名のあるタグの印（名前 → 印と色）。
    tag_marks: std::collections::HashMap<String, (char, Color)>,
    /// リモートでのタグの様子の印（名前 → 印と色。⚠ 別のタグがある、∅ どのリモートにもない）。
    tag_remote_marks: std::collections::HashMap<String, (char, Color)>,
    /// submodule の印（パス → 印と色）。
    submodule_marks: std::collections::HashMap<String, (char, Color)>,
    /// 印をつけた ref（ブランチ・リモートのブランチ・タグの名前。History をそれらだけにする）。
    marked: Vec<String>,
    /// Range Diff を開いている（Workspace に行を出す）。
    range_diff: bool,
    /// subtree の一覧（コアの `subtrees` の info。git subtree があって 1 つ以上あれば、Subtrees の節を出す）。
    subtrees: Value,
    /// ブランチの分け方（コアの `branchGroups` の info。読めていなければ null）と、そのときの時刻（Inactive を
    /// 決める）。
    groups: Value,
    now: i64,
    /// 開いている Branches のまとまり（`inactive`・`archived`。初めは畳んでいる）。
    open_groups: Vec<&'static str>,
}

/// Inactive とみなす日数（最後のコミットからこれだけ経ったブランチ。アプリと同じ）。
const STALE_DAYS: i64 = 90;

impl Default for Sidebar {
    fn default() -> Self {
        Self {
            refs: Value::Null,
            filter: String::new(),
            rows: vec![
                Row::Header(tr!("Workspace"), 0),
                Row::Changes,
                Row::History,
                Row::Reflog,
            ],
            // 開いたときのメインの画面は History。
            selected: 2,
            top: 0,
            height: 0,
            conflicts: 0,
            tag_marks: Default::default(),
            tag_remote_marks: Default::default(),
            submodule_marks: Default::default(),
            marked: Vec::new(),
            range_diff: false,
            subtrees: Value::Null,
            groups: Value::Null,
            now: 0,
            open_groups: Vec::new(),
        }
    }
}

impl Sidebar {
    /// `refs` の答えから、行を作り直す。選んでいた行は名前で引き継ぐ。
    pub fn set_refs(&mut self, refs: &Value) {
        self.refs = refs.clone();
        self.rebuild();
    }

    /// コンフリクトのあるファイルの数。あるかないかが変わったら、行を作り直す。
    pub fn set_conflicts(&mut self, conflicts: usize) {
        let changed = (self.conflicts > 0) != (conflicts > 0);
        self.conflicts = conflicts;
        if changed {
            self.rebuild();
        }
    }

    /// ブランチの分け方（`branchGroups` の info）。`now` は Inactive を決める時刻。
    pub fn set_branch_groups(&mut self, groups: Value, now: i64) {
        self.groups = groups;
        self.now = now;
        self.rebuild();
    }

    /// お気に入りのブランチ（上から順）。
    pub fn favorites(&self) -> Vec<String> {
        let mut pinned: Vec<(i64, String)> =
            (self.groups["branches"].as_array().into_iter().flatten())
                .filter_map(|b| Some((b["pinned"].as_i64()?, b["name"].as_str()?.to_owned())))
                .collect();
        pinned.sort();
        pinned.into_iter().map(|(_, name)| name).collect()
    }

    /// ブランチ `name` をアーカイブしてある。
    pub fn is_archived(&self, name: &str) -> bool {
        (self.groups["branches"].as_array().into_iter().flatten())
            .any(|b| b["name"] == name && b["archived"] == true)
    }

    /// 選んでいる行が Branches のまとまりなら、開く・畳む。
    pub fn toggle_group(&mut self) -> bool {
        let Some(Row::Group { key, .. }) = self.rows.get(self.selected) else {
            return false;
        };
        let key = *key;
        match self.open_groups.iter().position(|k| *k == key) {
            Some(i) => {
                self.open_groups.remove(i);
            }
            None => self.open_groups.push(key),
        }
        self.rebuild();
        true
    }

    /// ブランチの行を、アプリと同じく並べる（分け方が分かっていれば）: お気に入り（★、決めた順）、今のブランチ
    /// （その積み重ねの根元ごと）と既定のブランチ、動いているブランチ（新しい順）、Inactive（最後のコミットから
    /// 90 日以上。ほかの worktree が checkout しているものは入れない）、Archived。ほかのブランチの上に積んだ
    /// ブランチは、土台の行の下に ↳ で出す。
    fn arrange_branches(&self, rows: Vec<Row>, head: Option<&str>) -> Vec<Row> {
        let Some(list) = self.groups["branches"].as_array() else {
            return rows;
        };
        let facts: std::collections::HashMap<&str, &Value> = list
            .iter()
            .filter_map(|b| Some((b["name"].as_str()?, b)))
            .collect();
        let mut by_name: std::collections::HashMap<String, Row> = rows
            .into_iter()
            .filter_map(|r| match &r {
                Row::Ref { name, .. } => Some((name.clone(), r)),
                _ => None,
            })
            .collect();
        let archived = |n: &str| facts.get(n).is_some_and(|f| f["archived"] == true);
        let pinned = |n: &str| facts.get(n).and_then(|f| f["pinned"].as_i64());
        let time = |n: &str| facts.get(n).and_then(|f| f["time"].as_i64()).unwrap_or(0);
        let parent = |n: &str| facts.get(n).and_then(|f| f["parent"].as_str());
        let base = self.groups["base"].as_str().map(|b| {
            if by_name.contains_key(b) {
                b.to_owned()
            } else {
                b.split_once('/').map_or(b, |(_, n)| n).to_owned()
            }
        });
        // ほかの worktree が checkout しているブランチ（Inactive に畳まない）。
        let elsewhere: Vec<&str> = (self.refs["worktrees"].as_array().into_iter().flatten())
            .filter(|w| w["current"] != true)
            .filter_map(|w| w["branch"].as_str())
            .collect();
        // 今のブランチが積んであれば、その根元ごと一番上に。
        let mut root = head.map(str::to_owned);
        let mut seen = Vec::new();
        while let Some(name) = root.clone() {
            match parent(&name) {
                Some(p) if !seen.contains(&p) && by_name.contains_key(p) && !archived(p) => {
                    seen.push(p);
                    root = Some(p.to_owned());
                }
                _ => break,
            }
        }
        let is_top = |n: &str| {
            (Some(n) == root.as_deref() && pinned(n).is_none()) || Some(n) == base.as_deref()
        };
        let names: Vec<String> = by_name.keys().cloned().collect();
        let mut children: std::collections::HashMap<String, Vec<String>> = Default::default();
        let mut nested = Vec::new();
        for n in &names {
            if is_top(n) || archived(n) || pinned(n).is_some() {
                continue;
            }
            if let Some(p) = parent(n).filter(|p| by_name.contains_key(*p) && !archived(p)) {
                children.entry(p.to_owned()).or_default().push(n.clone());
                nested.push(n.clone());
            }
        }
        for list in children.values_mut() {
            list.sort_by_key(|n| (std::cmp::Reverse(time(n)), n.clone()));
        }
        let (mut stars, mut top, mut active, mut inactive, mut archive) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for n in names {
            if nested.contains(&n) {
                continue;
            }
            if pinned(&n).is_some() {
                stars.push(n);
            } else if is_top(&n) {
                top.push(n);
            } else if archived(&n) {
                archive.push(n);
            } else if !elsewhere.contains(&n.as_str()) && self.now - time(&n) > STALE_DAYS * 86_400
            {
                inactive.push(n);
            } else {
                active.push(n);
            }
        }
        stars.sort_by_key(|n| (pinned(n).unwrap_or(0), n.clone()));
        top.sort_by_key(|n| (Some(n.as_str()) != root.as_deref(), n.clone()));
        // 同じ時刻なら名前の順。
        active.sort_by_key(|n| (std::cmp::Reverse(time(n)), n.clone()));
        inactive.sort_by_key(|n| (std::cmp::Reverse(time(n)), n.clone()));
        archive.sort();
        // 行に印（★ ↳）と字下げを付けて、積んだブランチを下に続ける。
        // 既定のブランチに取り込み済みのもの（今のブランチと既定のブランチのほか。アプリの印）。
        let merged: Vec<&str> = facts
            .iter()
            .filter(|(n, f)| {
                f["merged"] == true && Some(**n) != head && Some(**n) != base.as_deref()
            })
            .map(|(n, _)| *n)
            .collect();
        fn emit(
            out: &mut Vec<Row>,
            by_name: &mut std::collections::HashMap<String, Row>,
            children: &std::collections::HashMap<String, Vec<String>>,
            merged: &[&str],
            name: &str,
            prefix: String,
            depth: usize,
        ) {
            let Some(mut row) = by_name.remove(name) else {
                return;
            };
            if let Row::Ref { label, .. } = &mut row {
                *label = format!("{prefix}{label}");
                if merged.contains(&name) {
                    label.push_str(&tr!(" · merged"));
                }
            }
            out.push(row);
            if depth > 32 {
                return;
            }
            for child in children.get(name).into_iter().flatten() {
                let indent = "  ".repeat(depth);
                emit(
                    out,
                    by_name,
                    children,
                    merged,
                    child,
                    format!("{indent}↳ "),
                    depth + 1,
                );
            }
        }
        let mut out = Vec::new();
        for n in &stars {
            emit(
                &mut out,
                &mut by_name,
                &children,
                &merged,
                n,
                "★ ".to_owned(),
                1,
            );
        }
        for n in top.iter().chain(&active) {
            emit(
                &mut out,
                &mut by_name,
                &children,
                &merged,
                n,
                String::new(),
                0,
            );
        }
        for (key, title, list) in [
            ("inactive", tr!("Inactive"), &inactive),
            ("archived", tr!("Archived"), &archive),
        ] {
            if list.is_empty() {
                continue;
            }
            let open = self.open_groups.contains(&key);
            out.push(Row::Group {
                key,
                title,
                count: list.len(),
                open,
            });
            for n in list {
                if open {
                    emit(
                        &mut out,
                        &mut by_name,
                        &children,
                        &merged,
                        n,
                        "  ".to_owned(),
                        1,
                    );
                } else {
                    by_name.remove(n.as_str());
                }
            }
        }
        // 分け方に載っていないもの（読んだ後に作ったもの）は、最後に。
        let mut rest: Vec<Row> = by_name.into_values().collect();
        rest.sort_by_key(|a| a.key());
        out.extend(rest);
        out
    }

    /// subtree の一覧（`subtrees` の info）。
    pub fn set_subtrees(&mut self, subtrees: Value) {
        if self.subtrees != subtrees {
            self.subtrees = subtrees;
            self.rebuild();
        }
    }

    /// この機械の git に git subtree があるか（まだ読んでいなければ None）。
    pub fn subtrees_available(&self) -> Option<bool> {
        self.subtrees["available"].as_bool()
    }

    /// subtree `dir` の覚えている相手（url・ref・pushRef・squash）。
    pub fn subtree(&self, dir: &str) -> Option<&Value> {
        (self.subtrees["subtrees"].as_array().into_iter().flatten()).find(|s| s["dir"] == dir)
    }

    /// Range Diff を開いているか。変わったら、行を作り直す。
    pub fn set_range_diff(&mut self, open: bool) {
        if self.range_diff != open {
            self.range_diff = open;
            self.rebuild();
        }
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
        let mut rows = vec![Row::Header(tr!("Workspace"), 0), Row::Changes];
        if self.conflicts > 0 {
            rows.push(Row::Conflicts);
        }
        rows.extend([Row::History, Row::Reflog]);
        if self.range_diff {
            rows.push(Row::RangeDiff);
        }
        for (kind, title) in [
            ("branch", tr!("Branches")),
            ("remote", tr!("Remotes")),
            ("tag", tr!("Tags")),
            ("stash", tr!("Stashes")),
            ("submodule", tr!("Submodules")),
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
                let mut rows_of_kind: Vec<Row> = of_kind.collect();
                if kind == "branch" {
                    rows_of_kind = self.arrange_branches(rows_of_kind, head);
                }
                // stash は数も（アプリの Stashes の行と同じ）。
                let count = match kind {
                    "stash" => rows_of_kind.len(),
                    _ => 0,
                };
                rows.push(Row::Header(title, count));
                rows.extend(rows_of_kind);
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
                    let mut label = match w["branch"].as_str() {
                        Some(branch) => format!("{folder} ({branch})"),
                        None => folder.to_owned(),
                    };
                    // ロックしているもの・フォルダが見つからないもの（アプリの印の代わり）。
                    if !w["locked"].is_null() {
                        label.push_str(&tr!(" · locked"));
                    }
                    if !w["prunable"].is_null() {
                        label.push_str(&tr!(" · missing"));
                    }
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
                rows.push(Row::Header(tr!("Worktrees"), 0));
                rows.extend(of_kind);
            }
        }
        // subtree（取り込んだフォルダと、覚えている相手）。
        if self.subtrees["available"] == true {
            let of_kind: Vec<Row> = (self.subtrees["subtrees"].as_array().into_iter().flatten())
                .filter_map(|s| {
                    let dir = s["dir"].as_str()?;
                    let label = match (s["url"].as_str(), s["ref"].as_str()) {
                        (Some(url), Some(r)) => format!("{dir} ← {url} {r}"),
                        _ => dir.to_owned(),
                    };
                    label
                        .to_lowercase()
                        .contains(&self.filter)
                        .then(|| Row::Ref {
                            kind: "subtree",
                            name: dir.to_owned(),
                            label,
                            target: String::new(),
                            current: false,
                            ahead: 0,
                            behind: 0,
                        })
                })
                .collect();
            if !of_kind.is_empty() {
                rows.push(Row::Header(tr!("Subtrees"), 0));
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
        // 行の位置は、コンフリクトの行があるかで変わるので、名前で引く。
        let key = match picked {
            Picked::Changes => "view:changes",
            Picked::Reflog => "view:reflog",
            Picked::RangeDiff => "view:range-diff",
            _ => "view:history",
        };
        self.selected = self
            .rows
            .iter()
            .position(|r| r.key().as_deref() == Some(key))
            .unwrap_or(2);
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
                Some(above) if matches!(self.rows[above], Row::Header(..)) => above,
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
                && matches!(self.rows[next as usize], Row::Header(..))
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
            Some(Row::Header(..)) | None => false,
            Some(_) => {
                self.selected = index;
                true
            }
        }
    }

    /// 選んでいる行が指すもの。
    pub fn picked(&self) -> Option<Picked> {
        match self.rows.get(self.selected)? {
            Row::Header(..) | Row::Group { .. } => None,
            Row::Changes | Row::Conflicts => Some(Picked::Changes),
            Row::History => Some(Picked::History),
            Row::Reflog => Some(Picked::Reflog),
            Row::RangeDiff => Some(Picked::RangeDiff),
            // subtree の行は、指すコミットを持たない。
            Row::Ref {
                kind: "subtree", ..
            } => None,
            Row::Ref { target, .. } => Some(Picked::Commit(target.clone())),
        }
    }

    /// 選んでいる ref の種類（`branch`・`remote`・`tag`）・名前・指すコミット。
    /// タグの名前（並びの順）。
    pub fn tag_names(&self) -> Vec<String> {
        (self.refs["entries"].as_array().into_iter().flatten())
            .filter(|e| e["kind"] == "tag")
            .filter_map(|e| e["name"].as_str().map(str::to_owned))
            .collect()
    }

    /// 選んでいる ref（ブランチ・リモートのブランチ・タグ）の印をつけ・外す。ref でなければ false。
    pub fn toggle_mark(&mut self) -> bool {
        let Some(("branch" | "remote" | "tag", name, _)) = self.picked_ref() else {
            return false;
        };
        let name = name.to_owned();
        match self.marked.iter().position(|m| *m == name) {
            Some(i) => {
                self.marked.remove(i);
            }
            None => self.marked.push(name),
        }
        true
    }

    pub fn has_marks(&self) -> bool {
        !self.marked.is_empty()
    }

    pub fn clear_marks(&mut self) {
        self.marked.clear();
    }

    /// 印をつけた ref の名前（つけた順）。
    pub fn marked_names(&self) -> Vec<String> {
        self.marked.clone()
    }

    /// 印をつけた ref が今指すコミット（なくなった ref は除く）。
    pub fn marked_targets(&self) -> Vec<String> {
        let entries = self.refs["entries"].as_array();
        self.marked
            .iter()
            .filter_map(|name| {
                entries?.iter().find(|e| {
                    e["name"] == name.as_str()
                        && matches!(e["kind"].as_str(), Some("branch" | "remote" | "tag"))
                })?["target"]
                    .as_str()
                    .map(str::to_owned)
            })
            .collect()
    }

    /// submodule の様子の印（パス → 印と色）。
    pub fn set_submodule_marks(&mut self, marks: std::collections::HashMap<String, (char, Color)>) {
        self.submodule_marks = marks;
    }

    /// submodule のパス。
    pub fn submodule_paths(&self) -> Vec<String> {
        (self.refs["entries"].as_array().into_iter().flatten())
            .filter(|e| e["kind"] == "submodule")
            .filter_map(|e| e["name"].as_str().map(str::to_owned))
            .collect()
    }

    /// 署名のあるタグの印。
    pub fn set_tag_marks(&mut self, marks: std::collections::HashMap<String, (char, Color)>) {
        self.tag_marks = marks;
    }

    pub fn set_tag_remote_marks(
        &mut self,
        marks: std::collections::HashMap<String, (char, Color)>,
    ) {
        self.tag_remote_marks = marks;
    }

    /// ローカルとリモートのブランチ（名前と、リモートのものか）。
    pub fn branches(&self) -> Vec<(String, bool)> {
        (self.refs["entries"].as_array().into_iter().flatten())
            .filter_map(|e| {
                let remote = match e["kind"].as_str()? {
                    "branch" => false,
                    "remote" => true,
                    _ => return None,
                };
                Some((e["name"].as_str()?.to_owned(), remote))
            })
            .collect()
    }

    /// `kind` の ref `name` の添え書き（stash ならメッセージ）。
    pub fn detail_of(&self, kind: &str, name: &str) -> Option<String> {
        self.refs["entries"]
            .as_array()?
            .iter()
            .find(|e| e["kind"] == kind && e["name"] == name)?["detail"]
            .as_str()
            .map(str::to_owned)
    }

    /// worktree `path` がロックされているか、フォルダが見つからないか、最初の worktree か。
    pub fn worktree_state(&self, path: &str) -> (bool, bool, bool) {
        let found =
            (self.refs["worktrees"].as_array().into_iter().flatten()).find(|w| w["path"] == path);
        match found {
            Some(w) => (
                !w["locked"].is_null(),
                !w["prunable"].is_null(),
                w["main"] == true,
            ),
            None => (false, false, false),
        }
    }

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

    /// 選んでいるブランチの、上流より進んでいる数と遅れている数。
    pub fn picked_counts(&self) -> (u64, u64) {
        match self.rows.get(self.selected) {
            Some(Row::Ref { ahead, behind, .. }) => (*ahead, *behind),
            _ => (0, 0),
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
                Row::Header(title, count) => {
                    buf.set_stringn(
                        area.x,
                        y,
                        title,
                        width,
                        Style::new().fg(DIM).add_modifier(Modifier::BOLD),
                    );
                    if *count > 0 {
                        right(area, y, buf, &count.to_string(), Style::new().fg(DIM));
                    }
                }
                Row::Conflicts => {
                    buf.set_stringn(
                        area.x + 2,
                        y,
                        tr!("Conflicts"),
                        width - 2,
                        Style::new().fg(Color::Red),
                    );
                    right(
                        area,
                        y,
                        buf,
                        &self.conflicts.to_string(),
                        Style::new().fg(Color::Red),
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
                Row::RangeDiff => {
                    buf.set_stringn(area.x + 2, y, tr!("Range Diff"), width - 2, Style::new());
                }
                Row::Group {
                    title, count, open, ..
                } => {
                    let mark = if *open { '▾' } else { '▸' };
                    buf.set_stringn(
                        area.x + 2,
                        y,
                        format!("{mark} {title}"),
                        width - 2,
                        Style::new().fg(DIM),
                    );
                    right(area, y, buf, &count.to_string(), Style::new().fg(DIM));
                }
                Row::Ref {
                    kind: kind @ ("tag" | "submodule"),
                    label: name,
                    ..
                } if (self.tag_marks.contains_key(name)
                    || self.tag_remote_marks.contains_key(name))
                    && *kind == "tag"
                    || self.submodule_marks.contains_key(name) && *kind == "submodule" =>
                {
                    // 右端から: 署名の印、その左にリモートでの様子の印（タグ）。
                    let marks: Vec<(char, Color)> = match *kind {
                        "tag" => [self.tag_remote_marks.get(name), self.tag_marks.get(name)]
                            .into_iter()
                            .flatten()
                            .copied()
                            .collect(),
                        _ => vec![self.submodule_marks[name]],
                    };
                    let room = width.saturating_sub(2 + marks.len() * 2);
                    buf.set_stringn(area.x + 2, y, fit(name, room), room, Style::new());
                    let mut x = area.right().saturating_sub(marks.len() as u16 * 2);
                    for (mark, color) in marks {
                        if x >= area.x + 2 {
                            buf[(x, y)].set_char(mark).set_fg(color);
                        }
                        x += 2;
                    }
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
            // 印をつけた ref（History の印をつけた行と同じ地の色）。
            if let Row::Ref { name, kind, .. } = row
                && matches!(*kind, "branch" | "remote" | "tag")
                && self.marked.iter().any(|m| m == name)
            {
                buf.set_style(
                    Rect::new(area.x, y, area.width, 1),
                    Style::new().bg(Color::Indexed(24)),
                );
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
