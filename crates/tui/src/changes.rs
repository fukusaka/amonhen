//! Working Copy: 変わったファイルの 1 つの一覧（ファイルごとに stage の印）と、選んだファイルの差分。
//!
//! 一覧は `status`、差分は `workingDiff`。差分は、できれば HEAD と作業ツリーを比べたもの（`combined`。行ごとに
//! stage 済みの印がつく）で、それが出せないファイルは、stage していない変更（なければ stage 済みの変更）を出す。

use amonhen_front::Reply;
use amonhen_layout::DiffRow;
use amonhen_model::tr;
use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Modifier, Style};
use serde_json::Value;

use crate::diff::{DIM, Diff, KeptRows, Scroll, Theme, counts, draw_row_kept, fit_path};
use crate::editor::Editor;
use crate::loader::Request;

const SELECTED: Color = Color::Indexed(238);
const SELECTED_UNFOCUSED: Color = Color::Indexed(236);
/// 一覧の 1 行の、パスの前（「[x] M 」）の幅。
const BEFORE_PATH: u16 = 6;

struct Entry {
    path: String,
    orig_path: Option<String>,
    /// 状態の 1 文字（`M` `A` `D` `R` `?` `U`）。
    letter: char,
    staged: bool,
    unstaged: bool,
}

/// 一覧の形（アプリの List・Tree・File Name）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ListMode {
    /// パスをそのまま。
    #[default]
    Path,
    /// フォルダの木（フォルダの行でまとめて stage でき、Enter で畳む）。
    Tree,
    /// ファイル名を先に、フォルダを薄く後ろに。
    Name,
}

/// 一覧の 1 行。
enum Row {
    /// `entries` の何番目か、木の深さ。
    File { entry: usize, depth: u16 },
    /// 木のフォルダ（`path` は作業ツリーからの相対パス）。`files` は中の（絞り込みに合った）ファイル。
    Folder {
        path: String,
        depth: u16,
        open: bool,
        files: Vec<usize>,
    },
}

/// 出している差分。
struct Shown {
    path: String,
    /// どの回の `status` のものか。
    serial: u64,
    diff: Diff,
    added: u64,
    removed: u64,
    binary: bool,
    /// 構文の色を頼んだときの名前。
    syntax: String,
    /// 窓口での差分の番号（`stageRows`・`discardRows` に渡す）。
    id: Value,
    /// HEAD と作業ツリーを比べた差分（行ごとに stage の印がある）。
    combined: bool,
    /// stage 済みの変更だけの差分。
    staged_only: bool,
    /// コンフリクトのあるファイルの、マーカーつきの中身。
    conflict: bool,
}

/// 行ごとの操作の相手。
pub struct Target {
    pub rows: Vec<u32>,
    /// hunk の見出しで選んだ（その hunk の変更すべて）。
    pub hunk: bool,
}

#[derive(Default)]
pub struct Changes {
    entries: Vec<Entry>,
    /// 出している行（形・絞り込み・畳んだフォルダから作る）。`selected`・`top` はこの行の番目。
    rows: Vec<Row>,
    mode: ListMode,
    /// パスの絞り込み（小文字。空白で分けた語のすべてを含むもの）。
    filter: String,
    /// 木で畳んだフォルダ。
    collapsed: std::collections::HashSet<String>,
    /// 一度でも `status` が届いた。
    loaded: bool,
    selected: usize,
    top: usize,
    height: u16,
    /// `status` が届くたびに進む。差分は、この回のものだけを出す。
    serial: u64,
    status_loading: bool,
    /// 読み直しが要る（変化の知らせ・stage の後）。
    stale: bool,
    shown: Option<Shown>,
    loading: Option<(String, u64)>,
    /// 読めなかった差分と、その理由。
    failed: Option<(String, u64, String)>,
    scroll: Scroll,
    /// 前のフレームで組み立てた差分の行。
    kept: KeptRows,
    /// 差分の中で選んでいる行（文書の行。0 はパスの行）。
    cursor: u32,
    /// 行の範囲の始まり（あれば、ここから選んでいる行までが相手）。
    mark: Option<u32>,
    /// コミットメッセージの欄。
    pub message: Editor,
    /// 直前のコミットを作り直す。
    pub amend: bool,
    /// 一覧とメッセージの欄を合わせた高さ。
    total_height: u16,
}

/// メッセージの欄の高さ（見出しの 1 行を含む）: 全体の 3 分の 1 で、3 行から 8 行。狭すぎれば出さない。
fn message_height(total: u16) -> u16 {
    if total < 6 {
        return 0;
    }
    (total / 3).clamp(3, 8)
}

impl Changes {
    /// 一覧を読み直す（次の `request` で頼む）。
    pub fn refresh(&mut self) {
        self.stale = true;
    }

    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// コンフリクトのあるファイルの数。
    pub fn conflicts(&self) -> usize {
        self.entries.iter().filter(|e| e.letter == 'U').count()
    }

    fn selected_entry(&self) -> Option<&Entry> {
        match self.rows.get(self.selected)? {
            Row::File { entry, .. } => self.entries.get(*entry),
            Row::Folder { .. } => None,
        }
    }

    /// 選んでいる行の鍵（ファイルかフォルダのパス、フォルダかどうか）。作り直しても同じ行を選ぶため。
    fn selected_key(&self) -> Option<(String, bool)> {
        match self.rows.get(self.selected)? {
            Row::File { entry, .. } => Some((self.entries.get(*entry)?.path.clone(), false)),
            Row::Folder { path, .. } => Some((path.clone(), true)),
        }
    }

    pub fn mode(&self) -> ListMode {
        self.mode
    }

    /// 一覧の形を次のものにする（パス → 木 → ファイル名）。
    pub fn next_mode(&mut self) {
        self.mode = match self.mode {
            ListMode::Path => ListMode::Tree,
            ListMode::Tree => ListMode::Name,
            ListMode::Name => ListMode::Path,
        };
        self.rebuild();
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// パスで絞り込む（空なら、すべて）。
    pub fn set_filter(&mut self, filter: &str) {
        self.filter = filter.to_lowercase();
        self.rebuild();
    }

    /// 木で選んでいるフォルダを畳む・開く。フォルダでなければ false。
    pub fn toggle_folder(&mut self) -> bool {
        let Some(Row::Folder { path, .. }) = self.rows.get(self.selected) else {
            return false;
        };
        let path = path.clone();
        if !self.collapsed.remove(&path) {
            self.collapsed.insert(path);
        }
        self.rebuild();
        true
    }

    /// 形・絞り込み・畳んだフォルダから、出す行を作り直す（選んでいた行は引き継ぐ）。
    fn rebuild(&mut self) {
        let key = self.selected_key();
        self.rebuild_keeping(key);
    }

    /// 出す行を作り直し、`key`（ファイルかフォルダのパス）の行があれば、それを選ぶ。
    fn rebuild_keeping(&mut self, key: Option<(String, bool)>) {
        let words: Vec<&str> = self.filter.split_whitespace().collect();
        let mut matched: Vec<usize> = (0..self.entries.len())
            .filter(|&i| {
                let path = self.entries[i].path.to_lowercase();
                words.iter().all(|w| path.contains(w))
            })
            .collect();
        self.rows = match self.mode {
            ListMode::Path | ListMode::Name => matched
                .into_iter()
                .map(|entry| Row::File { entry, depth: 0 })
                .collect(),
            ListMode::Tree => {
                // フォルダを先に、名前の順。
                let order = |path: &str| {
                    let parts: Vec<&str> = path.split('/').collect();
                    let last = parts.len() - 1;
                    parts
                        .into_iter()
                        .enumerate()
                        .map(|(i, p)| (i == last, p.to_owned()))
                        .collect::<Vec<_>>()
                };
                matched.sort_by_cached_key(|&i| order(&self.entries[i].path));
                let mut rows = Vec::new();
                let mut seen = std::collections::HashSet::new();
                for &i in &matched {
                    let parts: Vec<&str> = self.entries[i].path.split('/').collect();
                    let mut hidden = false;
                    for depth in 0..parts.len() - 1 {
                        let folder = parts[..=depth].join("/");
                        if seen.insert(folder.clone()) {
                            let open = !self.collapsed.contains(&folder);
                            rows.push(Row::Folder {
                                path: folder.clone(),
                                depth: depth as u16,
                                open,
                                files: Vec::new(),
                            });
                        }
                        if self.collapsed.contains(&folder) {
                            hidden = true;
                            break;
                        }
                    }
                    if !hidden {
                        rows.push(Row::File {
                            entry: i,
                            depth: (parts.len() - 1) as u16,
                        });
                    }
                }
                for row in &mut rows {
                    if let Row::Folder { path, files, .. } = row {
                        let prefix = format!("{path}/");
                        files.extend(
                            matched
                                .iter()
                                .copied()
                                .filter(|&i| self.entries[i].path.starts_with(&prefix)),
                        );
                    }
                }
                rows
            }
        };
        if let Some((path, folder)) = key
            && let Some(index) = self.rows.iter().position(|r| match r {
                Row::File { entry, .. } => !folder && self.entries[*entry].path == path,
                Row::Folder { path: p, .. } => folder && *p == path,
            })
        {
            self.selected = index;
        }
        self.selected = self.selected.min(self.rows.len().saturating_sub(1));
        self.clamp_top();
    }

    /// 頼むことがあれば、その中身。`diff_visible` は、差分が画面に出ているか。
    pub fn request(&mut self, diff_visible: bool) -> Option<Request> {
        if self.stale && !self.status_loading {
            self.stale = false;
            self.status_loading = true;
            return Some(Request::Status);
        }
        // 一覧を読んでいる間は、差分を頼まない（届いたら、その回のものを頼む）。
        if !diff_visible || self.status_loading || self.loading.is_some() {
            return None;
        }
        let entry = self.selected_entry()?;
        let key = (entry.path.as_str(), self.serial);
        if self
            .shown
            .as_ref()
            .is_some_and(|s| (s.path.as_str(), s.serial) == key)
            || self
                .failed
                .as_ref()
                .is_some_and(|f| (f.0.as_str(), f.1) == key)
        {
            return None;
        }
        let (path, unstaged, conflict) = (entry.path.clone(), entry.unstaged, entry.letter == 'U');
        self.loading = Some((path.clone(), self.serial));
        Some(Request::WorkingDiff {
            path,
            serial: self.serial,
            unstaged,
            conflict,
        })
    }

    /// `status` の答え。選んでいたファイルは、パスで引き継ぐ。
    pub fn status(&mut self, status: &Value) {
        let key = self.selected_key();
        self.entries = status["entries"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|e| {
                let staged = e["staged"] == true;
                let unstaged = e["unstaged"] == true;
                let one = |key: &str| e[key].as_str().and_then(|s| s.chars().next());
                let letter = match e["kind"].as_str() {
                    Some("untracked") => '?',
                    Some("unmerged") => 'U',
                    // stage していない変更があれば、その状態を出す。
                    _ => match (unstaged, one("worktree"), one("index")) {
                        (true, Some(w), _) if w != '.' && w != ' ' => w,
                        (_, _, Some(i)) if i != '.' && i != ' ' => i,
                        _ => 'M',
                    },
                };
                Entry {
                    path: e["path"].as_str().unwrap_or("").to_owned(),
                    orig_path: e["origPath"].as_str().map(str::to_owned),
                    letter,
                    staged,
                    unstaged,
                }
            })
            .collect();
        self.loaded = true;
        self.status_loading = false;
        self.serial += 1;
        // 選んでいた行（ファイルかフォルダ）は、パスで引き継ぐ。
        self.rebuild_keeping(key);
        // 変更がなくなったら、差分も消す。
        if self.entries.is_empty() {
            self.shown = None;
        }
    }

    /// `status` が読めなかった。
    pub fn status_failed(&mut self) {
        self.status_loading = false;
    }

    /// 差分の答え。今の回の、選んでいるファイルのものなら入れ替え、構文の色を頼む中身を返す。
    pub fn working_diff(
        &mut self,
        path: String,
        serial: u64,
        result: std::result::Result<Reply, String>,
    ) -> Option<Request> {
        self.loading = None;
        if serial != self.serial || self.selected_entry().is_none_or(|e| e.path != path) {
            return None;
        }
        match result {
            Ok(reply) => {
                // 同じファイルを読み直したときは、見ていた位置を保つ。
                if self.shown.as_ref().is_none_or(|s| s.path != path) {
                    self.scroll.top = 0;
                    self.scroll.left = 0;
                    self.cursor = 0;
                    self.mark = None;
                }
                let id = reply.json["diff"].clone();
                let mode = reply.json["mode"].as_str().unwrap_or("");
                self.shown = Some(Shown {
                    diff: Diff::new(&reply, mode == "combined"),
                    combined: mode == "combined",
                    staged_only: mode == "staged",
                    // `conflict` の答えには、差分の種類も番号もない。
                    conflict: reply.json["mode"].is_null(),
                    id: id.clone(),
                    added: reply.json["added"].as_u64().unwrap_or(0),
                    removed: reply.json["removed"].as_u64().unwrap_or(0),
                    binary: reply.json["binary"] == true,
                    syntax: syntax_key(&id),
                    path,
                    serial,
                });
                self.failed = None;
                self.cursor = self.cursor.min(self.lines().saturating_sub(1));
                self.scroll.clamp(self.lines());
                (!id.is_null()).then(|| Request::Syntax {
                    id: syntax_key(&id),
                    of: serde_json::json!({"diff": id}),
                })
            }
            Err(error) => {
                self.failed = Some((path, serial, error));
                None
            }
        }
    }

    pub fn syntax(&mut self, id: &str, reply: &Reply) {
        if let Some(shown) = &mut self.shown
            && shown.syntax == id
        {
            shown.diff.set_syntax(reply);
        }
    }

    /// 選んでいるファイルの stage を切り替える: stage していない変更があれば stage、なければ unstage。
    pub fn toggle(&self) -> Option<Request> {
        // 木のフォルダ: 中に stage していない変更があれば、そのすべてを stage（コンフリクトのファイルは除く）、
        // なければすべてを unstage。
        if let Some(Row::Folder { files, .. }) = self.rows.get(self.selected) {
            let files: Vec<&Entry> = files.iter().map(|&i| &self.entries[i]).collect();
            let stage = files.iter().any(|e| e.unstaged && e.letter != 'U');
            let paths: Vec<String> = files
                .iter()
                .filter(|e| {
                    if stage {
                        e.unstaged && e.letter != 'U'
                    } else {
                        e.staged
                    }
                })
                .map(|e| e.path.clone())
                .collect();
            return (!paths.is_empty()).then_some(Request::Stage { paths, stage });
        }
        let entry = self.selected_entry()?;
        Some(Request::Stage {
            paths: vec![entry.path.clone()],
            stage: entry.unstaged,
        })
    }

    /// Stage All: stage していない変更のあるファイルをすべて（コンフリクトのファイルは除く。アプリと同じ）。
    pub fn stage_all(&self) -> Option<Request> {
        let paths: Vec<String> = (self.entries.iter())
            .filter(|e| e.unstaged && e.letter != 'U')
            .map(|e| e.path.clone())
            .collect();
        (!paths.is_empty()).then_some(Request::Stage { paths, stage: true })
    }

    // MARK: 一覧

    /// stage 済みの変更があるファイルの数。
    pub fn staged_count(&self) -> usize {
        self.entries.iter().filter(|e| e.staged).count()
    }

    /// 一覧の高さ（メッセージの欄を除く）。
    pub fn list_height(&self) -> u16 {
        self.height
    }

    /// メッセージの欄が出ている。
    pub fn has_message_field(&self) -> bool {
        message_height(self.total_height) > 0
    }

    pub fn set_height(&mut self, height: u16) {
        self.total_height = height;
        self.height = height - message_height(height);
        self.scroll.height = height;
        self.clamp_top();
        self.scroll.clamp(self.lines());
    }

    fn clamp_top(&mut self) {
        let height = usize::from(self.height.max(1));
        let last = self.rows.len().saturating_sub(height);
        if self.selected < self.top {
            self.top = self.selected;
        } else if self.selected >= self.top + height {
            self.top = self.selected + 1 - height;
        }
        self.top = self.top.min(last);
    }

    pub fn page(&self) -> i64 {
        i64::from(self.height.max(2)) - 1
    }

    pub fn move_selection(&mut self, delta: i64) -> bool {
        self.select((self.selected as i64 + delta).max(0) as usize)
    }

    pub fn select(&mut self, index: usize) -> bool {
        let index = index.min(self.rows.len().saturating_sub(1));
        let changed = index != self.selected;
        self.selected = index;
        self.clamp_top();
        changed
    }

    /// 一覧を動かす（選択はそのまま）。
    pub fn scroll_list(&mut self, delta: i64) -> bool {
        let last = self
            .rows
            .len()
            .saturating_sub(usize::from(self.height.max(1)));
        let top = (self.top as i64 + delta).clamp(0, last as i64) as usize;
        let changed = top != self.top;
        self.top = top;
        changed
    }

    /// 一覧の `line` 行目（見えている中で）を選ぶ。
    pub fn click(&mut self, line: u16) -> bool {
        let index = self.top + usize::from(line);
        index < self.rows.len() && self.select(index)
    }

    /// 選んでいる位置と、全体の数。
    pub fn list_position(&self) -> (u32, u32) {
        let count = self.rows.len() as u32;
        ((self.selected as u32 + 1).min(count), count)
    }

    /// 一覧と、その下のメッセージの欄。欄で入力しているなら、カーソルを置く位置を返す。
    pub fn draw_main(
        &mut self,
        area: Rect,
        buf: &mut Buffer,
        focused: bool,
        typing: bool,
    ) -> Option<(u16, u16)> {
        let field = message_height(area.height);
        let list = Rect::new(area.x, area.y, area.width, area.height - field);
        self.draw_list(list, buf, focused);
        if field == 0 || area.width == 0 {
            return None;
        }
        // 見出し: 「── Message ───── 23 ─」（右は件名の文字数）。
        let y = list.bottom();
        let line = Style::new().fg(if typing { Color::Cyan } else { DIM });
        for x in area.x..area.right() {
            buf[(x, y)].set_char('─').set_style(line);
        }
        let title = if self.amend {
            tr!(" Message (amend) ")
        } else {
            tr!(" Message ")
        };
        buf.set_stringn(
            area.x + 2,
            y,
            title,
            usize::from(area.width.saturating_sub(2)),
            line,
        );
        let count = format!(" {} ", self.message.summary_length());
        let count_x = area.right().saturating_sub(count.len() as u16 + 1);
        if count_x > area.x + 20 {
            buf.set_string(count_x, y, count, line);
        }
        let body = Rect::new(area.x + 1, y + 1, area.width - 1, field - 1);
        if self.message.is_empty() && !typing {
            buf.set_stringn(
                body.x,
                body.y,
                tr!("c to write the commit message"),
                usize::from(body.width),
                Style::new().fg(DIM),
            );
            return None;
        }
        let cursor = self.message.draw(body, buf);
        cursor.filter(|_| typing)
    }

    fn draw_list(&self, area: Rect, buf: &mut Buffer, focused: bool) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        if self.rows.is_empty() {
            let text = match (self.loaded, self.entries.is_empty()) {
                (false, _) => tr!("Loading…"),
                (true, true) => tr!("No changes"),
                (true, false) => tr!("No file matches “{filter}”", filter = self.filter),
            };
            buf.set_stringn(
                area.x + 1,
                area.y,
                text,
                usize::from(area.width.saturating_sub(1)),
                Style::new().fg(DIM),
            );
            return;
        }
        let check_of = |staged: bool, unstaged: bool| match (staged, unstaged) {
            (true, false) => ("[x]", Style::new().fg(Color::Green)),
            (true, true) => ("[-]", Style::new().fg(Color::Yellow)),
            _ => ("[ ]", Style::new().fg(DIM)),
        };
        for (y, (index, row)) in (area.y..area.bottom()).zip(
            self.rows
                .iter()
                .enumerate()
                .skip(self.top)
                .take(usize::from(area.height)),
        ) {
            match row {
                Row::Folder {
                    path,
                    depth,
                    open,
                    files,
                } => {
                    // フォルダの印: 中がすべて stage 済み、一部、まだ。
                    let staged = files.iter().any(|&i| self.entries[i].staged);
                    let all = files
                        .iter()
                        .all(|&i| self.entries[i].staged && !self.entries[i].unstaged);
                    let (check, check_style) = check_of(staged, !all);
                    buf.set_stringn(area.x, y, check, usize::from(area.width), check_style);
                    let indent = BEFORE_PATH + depth * 2;
                    if area.width > indent {
                        let name = path.rsplit('/').next().unwrap_or(path);
                        let arrow = if *open { "▾" } else { "▸" };
                        let text = format!("{arrow} {name}/");
                        let room = usize::from(area.width - indent);
                        buf.set_stringn(area.x + indent, y, text, room, Style::new().fg(DIM));
                    }
                }
                Row::File { entry, depth } => {
                    let entry = &self.entries[*entry];
                    // stage の印: すべて stage 済み、一部だけ、まだ。
                    let (check, check_style) = check_of(entry.staged, entry.unstaged);
                    buf.set_stringn(area.x, y, check, usize::from(area.width), check_style);
                    let color = match entry.letter {
                        'A' => Color::Green,
                        'D' => Color::Red,
                        'R' | 'C' => Color::Cyan,
                        'U' => Color::LightRed,
                        '?' => DIM,
                        _ => Color::Yellow,
                    };
                    if area.width > 4 {
                        buf[(area.x + 4, y)].set_char(entry.letter).set_fg(color);
                    }
                    let indent = BEFORE_PATH + depth * 2;
                    if area.width > indent {
                        let room = usize::from(area.width - indent);
                        let x = area.x + indent;
                        let name = entry.path.rsplit('/').next().unwrap_or(&entry.path);
                        let folder = entry.path.len() - name.len();
                        match self.mode {
                            ListMode::Path => {
                                let path = match &entry.orig_path {
                                    Some(old) if old != &entry.path => {
                                        format!("{old} → {}", entry.path)
                                    }
                                    _ => entry.path.clone(),
                                };
                                buf.set_stringn(x, y, fit_path(&path, room), room, Style::new());
                            }
                            ListMode::Tree => {
                                buf.set_stringn(x, y, name, room, Style::new());
                            }
                            ListMode::Name => {
                                let end = buf.set_stringn(x, y, name, room, Style::new()).0;
                                let dir = entry.path[..folder].trim_end_matches('/');
                                let left = usize::from(area.right().saturating_sub(end + 2));
                                if !dir.is_empty() && left > 0 {
                                    let dim = Style::new().fg(DIM);
                                    buf.set_stringn(end + 2, y, fit_path(dir, left), left, dim);
                                }
                            }
                        }
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

    // MARK: 差分

    /// 差分の行数（1 行目はパス）。
    fn lines(&self) -> u32 {
        self.shown.as_ref().map_or(0, |s| 1 + s.diff.len())
    }

    /// 選んでいる行と、全体の行数。
    pub fn diff_position(&self) -> (u32, u32) {
        let lines = self.lines();
        ((self.cursor + 1).min(lines), lines)
    }

    /// 最後に描いてから、差分が縦に何行ずれたか（正なら、中身が上へ）。
    pub fn diff_scrolled(&self) -> i32 {
        self.scroll.scrolled()
    }

    /// 前のフレームで組み立てた行を捨てる。
    pub fn forget_drawn(&mut self) {
        self.kept.forget();
    }

    /// 選んでいる行を、`down` の向きにはもう動かせない。
    pub fn at_end(&self, down: bool) -> bool {
        self.cursor
            == if down {
                self.lines().saturating_sub(1)
            } else {
                0
            }
    }

    pub fn diff_page(&self) -> i64 {
        self.scroll.page()
    }

    /// 表示を動かす（選んでいる行はそのまま）。
    pub fn scroll(&mut self, delta: i64) -> bool {
        self.scroll.by(delta, self.lines())
    }

    pub fn pan(&mut self, delta: i32) -> bool {
        self.scroll.pan(delta)
    }

    /// 選んでいる行を動かす。見えなくなるなら、表示もついていく。
    pub fn move_cursor(&mut self, delta: i64) -> bool {
        self.cursor_to((i64::from(self.cursor) + delta).max(0) as u32)
    }

    pub fn cursor_to(&mut self, line: u32) -> bool {
        let line = line.min(self.lines().saturating_sub(1));
        let changed = line != self.cursor;
        self.cursor = line;
        self.scroll.reveal(line, self.lines());
        changed
    }

    /// 差分の `line` 行目（見えている中で）を選ぶ。
    pub fn click_diff(&mut self, line: u16) -> bool {
        let line = self.scroll.top + u32::from(line);
        line < self.lines() && self.cursor_to(line)
    }

    /// 次（`forward`）か前の hunk の見出しを選ぶ。
    pub fn jump_hunk(&mut self, forward: bool) -> bool {
        let Some(shown) = &self.shown else {
            return false;
        };
        let mut starts = shown.diff.hunk_rows.iter().map(|r| 1 + r);
        let target = if forward {
            starts.find(|&line| line > self.cursor)
        } else {
            starts.take_while(|&line| line < self.cursor).last()
        };
        target.is_some_and(|line| self.cursor_to(line))
    }

    /// 選んでいる範囲（文書の行。両端を含む）。
    fn range(&self) -> Option<(u32, u32)> {
        let mark = self.mark?;
        Some((mark.min(self.cursor), mark.max(self.cursor)))
    }

    pub fn has_mark(&self) -> bool {
        self.mark.is_some()
    }

    /// 範囲の始まりを、選んでいる行に置く。もう置いてあれば、やめる。
    pub fn toggle_mark(&mut self) -> bool {
        self.mark = match self.mark {
            Some(_) => None,
            None => Some(self.cursor),
        };
        true
    }

    pub fn clear_mark(&mut self) {
        self.mark = None;
    }

    /// 選んでいる行が指す変更: 追加・削除の行ならその行、hunk の見出しならその hunk の変更すべて。
    fn target(&self) -> Option<(&Shown, Target)> {
        let shown = self.shown.as_ref()?;
        let changed = |r: usize| matches!(shown.diff.kind(r).0, DiffRow::ADDED | DiffRow::REMOVED);
        // 範囲を選んでいれば、その中の変更すべて。
        if let Some((first, last)) = self.range() {
            let rows: Vec<u32> = (first.max(1) - 1..last.min(shown.diff.len()))
                .filter(|&r| changed(r as usize))
                .collect();
            return (!rows.is_empty()).then_some((shown, Target { rows, hunk: false }));
        }
        let row = self.cursor.checked_sub(1)? as usize;
        if row >= shown.diff.len() as usize {
            return None;
        }
        let rows: Vec<u32> = match shown.diff.kind(row).0 {
            DiffRow::HUNK => (row + 1..shown.diff.len() as usize)
                .take_while(|&r| !matches!(shown.diff.kind(r).0, DiffRow::HUNK | DiffRow::FILE))
                .filter(|&r| changed(r))
                .map(|r| r as u32)
                .collect(),
            _ if changed(row) => vec![row as u32],
            _ => return None,
        };
        let hunk = shown.diff.kind(row).0 == DiffRow::HUNK;
        (!rows.is_empty()).then_some((shown, Target { rows, hunk }))
    }

    /// 選んでいる行（か hunk）の stage を切り替える: stage していない行があれば stage、なければ unstage。
    pub fn toggle_rows(&self) -> std::result::Result<Option<Request>, String> {
        let Some((shown, target)) = self.target() else {
            return Ok(None);
        };
        if shown.staged_only {
            return Err(tr!("Lines of this diff can’t be staged one by one"));
        }
        let staged = |r: &u32| shown.diff.kind(*r as usize).1 & DiffRow::STAGED != 0;
        // stage していない変更だけの差分では、stage するだけ。
        let stage = !shown.combined || !target.rows.iter().all(staged);
        Ok(Some(Request::StageRows {
            diff: shown.id.clone(),
            rows: target.rows,
            stage,
        }))
    }

    /// 選んでいる行（か hunk）の、stage していない変更を捨てる中身。
    pub fn discard_rows(&self) -> std::result::Result<Option<(Target, Request)>, String> {
        let Some((shown, mut target)) = self.target() else {
            return Ok(None);
        };
        target
            .rows
            .retain(|r| shown.diff.kind(*r as usize).1 & DiffRow::STAGED == 0);
        if shown.staged_only || target.rows.is_empty() {
            return Err(tr!("Can’t discard staged changes (unstage them first)"));
        }
        let request = Request::DiscardRows {
            diff: shown.id.clone(),
            rows: target.rows.clone(),
        };
        Ok(Some((target, request)))
    }

    /// 選んでいるファイルのパス。
    pub fn selected_path(&self) -> Option<&str> {
        self.selected_entry().map(|e| e.path.as_str())
    }

    /// 選んでいるファイルが untracked なら、そのパス。
    pub fn untracked_path(&self) -> Option<&str> {
        self.selected_entry()
            .filter(|e| e.letter == '?')
            .map(|e| e.path.as_str())
    }

    /// 選んでいるファイルがコンフリクトしていれば、そのパス。
    pub fn conflicted_path(&self) -> Option<&str> {
        self.selected_entry()
            .filter(|e| e.letter == 'U')
            .map(|e| e.path.as_str())
    }

    /// 差分で選んでいる行を含むコンフリクト: パスと、ファイルの中で何番目か（0 始まり）。
    pub fn conflict_at_cursor(&self) -> Option<(&str, usize)> {
        let shown = self.shown.as_ref().filter(|s| s.conflict)?;
        let row = self.cursor.checked_sub(1)? as usize;
        let marker = |r: usize, start: &str| {
            shown.diff.kind(r).0 == DiffRow::MARKER && shown.diff.row_text(r).starts_with(start)
        };
        let rows = (shown.diff.len() as usize).min(row + 1);
        // 選んでいる行までの「<<<<<<<」の数。最後のものが「>>>>>>>」で閉じていれば、コンフリクトの外。
        let opened = (0..rows).filter(|&r| marker(r, "<<<<<<<")).count();
        let closed = (0..row).filter(|&r| marker(r, ">>>>>>>")).count();
        (opened > closed).then(|| (shown.path.as_str(), opened - 1))
    }

    /// 差分で選んでいる行を含むコンフリクトに、共通の祖先の側（`|||||||` の区切り。diff3 の形）があるか。
    pub fn conflict_has_base(&self) -> bool {
        let Some(shown) = self.shown.as_ref().filter(|s| s.conflict) else {
            return false;
        };
        let Some(row) = self.cursor.checked_sub(1).map(|r| r as usize) else {
            return false;
        };
        let marker = |r: usize, start: &str| {
            shown.diff.kind(r).0 == DiffRow::MARKER && shown.diff.row_text(r).starts_with(start)
        };
        let len = shown.diff.len() as usize;
        // このコンフリクトの始まりから終わりまでの間に、区切りがあるか。
        let Some(start) = (0..len.min(row + 1)).rev().find(|&r| marker(r, "<<<<<<<")) else {
            return false;
        };
        (start..len)
            .take_while(|&r| !marker(r, ">>>>>>>"))
            .any(|r| marker(r, "|||||||"))
    }

    /// 選んでいるファイルの変更を捨てる中身と、そのパス。
    pub fn discard_file(&self) -> Option<(String, Request)> {
        let entry = self.selected_entry()?;
        Some((
            entry.path.clone(),
            Request::Discard {
                paths: vec![entry.path.clone()],
            },
        ))
    }

    pub fn draw_diff(&mut self, area: Rect, buf: &mut Buffer, theme: &Theme, focused: bool) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        self.scroll.height = area.height;
        let selected = self.selected_entry().map(|e| e.path.as_str());
        // 読めなかった理由は、選んでいるファイルのものだけ。
        if let Some((_, _, error)) = self
            .failed
            .as_ref()
            .filter(|f| Some(f.0.as_str()) == selected && f.1 == self.serial)
        {
            buf.set_stringn(
                area.x,
                area.y,
                error,
                usize::from(area.width),
                Style::new().fg(Color::Red),
            );
            return;
        }
        let Some(shown) = &self.shown else {
            if selected.is_some() {
                buf.set_stringn(
                    area.x,
                    area.y,
                    tr!("Loading…"),
                    usize::from(area.width),
                    Style::new().fg(DIM),
                );
            }
            return;
        };
        self.scroll.clamp(1 + shown.diff.len());
        self.scroll.drawn();
        // 左の 1 列は、選んでいる行の印。
        let body = Rect::new(area.x + 1, area.y, area.width - 1, area.height);
        for y in 0..area.height {
            let line = self.scroll.top + u32::from(y);
            if line > shown.diff.len() {
                break;
            }
            let at = Rect::new(body.x, body.y + y, body.width, 1);
            if at.width > 0 {
                if line == 0 {
                    draw_title(at, buf, shown);
                } else {
                    draw_row_kept(
                        &mut self.kept,
                        area.height,
                        at,
                        buf,
                        &shown.diff,
                        (line - 1) as usize,
                        self.scroll.left,
                        theme,
                    );
                }
            }
            let in_range = self
                .range()
                .is_some_and(|(first, last)| first <= line && line <= last);
            if (line == self.cursor || in_range) && focused {
                buf[(area.x, area.y + y)].set_char('▌').set_fg(Color::Cyan);
            }
        }
    }
}

/// 構文の色を頼むときの名前（コミットの ID と重ならない）。
fn syntax_key(diff: &Value) -> String {
    format!("diff:{diff}")
}

/// 差分の 1 行目: パスと、増えた行・減った行。
fn draw_title(at: Rect, buf: &mut Buffer, shown: &Shown) {
    let counts = counts(shown.added, shown.removed, shown.binary);
    let end = buf
        .set_stringn(
            at.x,
            at.y,
            fit_path(&shown.path, usize::from(at.width)),
            usize::from(at.width),
            Style::new().add_modifier(Modifier::BOLD),
        )
        .0;
    let mut x = end + 1;
    for (text, style) in counts {
        if x >= at.right() {
            break;
        }
        x = buf
            .set_stringn(x, at.y, text, usize::from(at.right() - x), style)
            .0
            + 1;
    }
}
