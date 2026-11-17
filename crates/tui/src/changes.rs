//! Working Copy: 変わったファイルの 1 つの一覧（ファイルごとに stage の印）と、選んだファイルの差分。
//!
//! 一覧は `status`、差分は `workingDiff`。差分は、できれば HEAD と作業ツリーを比べたもの（`combined`。行ごとに
//! stage 済みの印がつく）で、それが出せないファイルは、stage していない変更（なければ stage 済みの変更）を出す。

use amonhen_front::Reply;
use amonhen_model::tr;
use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Modifier, Style};
use serde_json::Value;

use crate::diff::{DIM, Diff, Scroll, Theme, counts, draw_row, fit_path};
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
}

#[derive(Default)]
pub struct Changes {
    entries: Vec<Entry>,
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
}

impl Changes {
    /// 一覧を読み直す（次の `request` で頼む）。
    pub fn refresh(&mut self) {
        self.stale = true;
    }

    pub fn count(&self) -> usize {
        self.entries.len()
    }

    fn selected_entry(&self) -> Option<&Entry> {
        self.entries.get(self.selected)
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
        let (path, unstaged) = (entry.path.clone(), entry.unstaged);
        self.loading = Some((path.clone(), self.serial));
        Some(Request::WorkingDiff {
            path,
            serial: self.serial,
            unstaged,
        })
    }

    /// `status` の答え。選んでいたファイルは、パスで引き継ぐ。
    pub fn status(&mut self, status: &Value) {
        let selected = self.selected_entry().map(|e| e.path.clone());
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
        self.selected = selected
            .and_then(|path| self.entries.iter().position(|e| e.path == path))
            .unwrap_or(self.selected)
            .min(self.entries.len().saturating_sub(1));
        self.clamp_top();
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
                }
                let id = reply.json["diff"].clone();
                self.shown = Some(Shown {
                    diff: Diff::new(&reply, reply.json["mode"] == "combined"),
                    added: reply.json["added"].as_u64().unwrap_or(0),
                    removed: reply.json["removed"].as_u64().unwrap_or(0),
                    binary: reply.json["binary"] == true,
                    syntax: syntax_key(&id),
                    path,
                    serial,
                });
                self.failed = None;
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
        let entry = self.selected_entry()?;
        Some(Request::Stage {
            paths: vec![entry.path.clone()],
            stage: entry.unstaged,
        })
    }

    // MARK: 一覧

    pub fn set_height(&mut self, height: u16) {
        self.height = height;
        self.scroll.height = height;
        self.clamp_top();
        self.scroll.clamp(self.lines());
    }

    fn clamp_top(&mut self) {
        let height = usize::from(self.height.max(1));
        let last = self.entries.len().saturating_sub(height);
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
        let index = index.min(self.entries.len().saturating_sub(1));
        let changed = index != self.selected;
        self.selected = index;
        self.clamp_top();
        changed
    }

    /// 一覧を動かす（選択はそのまま）。
    pub fn scroll_list(&mut self, delta: i64) -> bool {
        let last = self
            .entries
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
        index < self.entries.len() && self.select(index)
    }

    /// 選んでいる位置と、全体の数。
    pub fn list_position(&self) -> (u32, u32) {
        let count = self.entries.len() as u32;
        ((self.selected as u32 + 1).min(count), count)
    }

    pub fn draw_list(&self, area: Rect, buf: &mut Buffer, focused: bool) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        if self.entries.is_empty() {
            let text = if self.loaded {
                tr!("No changes")
            } else {
                tr!("Loading…")
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
        for (y, (index, entry)) in (area.y..area.bottom()).zip(
            self.entries
                .iter()
                .enumerate()
                .skip(self.top)
                .take(usize::from(area.height)),
        ) {
            // stage の印: すべて stage 済み、一部だけ、まだ。
            let (check, check_style) = match (entry.staged, entry.unstaged) {
                (true, false) => ("[x]", Style::new().fg(Color::Green)),
                (true, true) => ("[-]", Style::new().fg(Color::Yellow)),
                _ => ("[ ]", Style::new().fg(DIM)),
            };
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
            if area.width > BEFORE_PATH {
                let path = match &entry.orig_path {
                    Some(old) if old != &entry.path => format!("{old} → {}", entry.path),
                    _ => entry.path.clone(),
                };
                let room = usize::from(area.width - BEFORE_PATH);
                buf.set_stringn(
                    area.x + BEFORE_PATH,
                    y,
                    fit_path(&path, room),
                    room,
                    Style::new(),
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

    // MARK: 差分

    /// 差分の行数（1 行目はパス）。
    fn lines(&self) -> u32 {
        self.shown.as_ref().map_or(0, |s| 1 + s.diff.len())
    }

    pub fn diff_position(&self) -> (u32, u32) {
        self.scroll.position(self.lines())
    }

    pub fn diff_page(&self) -> i64 {
        self.scroll.page()
    }

    pub fn scroll(&mut self, delta: i64) -> bool {
        self.scroll.by(delta, self.lines())
    }

    pub fn scroll_to(&mut self, top: u32) -> bool {
        self.scroll.to(top, self.lines())
    }

    pub fn pan(&mut self, delta: i32) -> bool {
        self.scroll.pan(delta)
    }

    /// 次（`forward`）か前の hunk の見出しを一番上に。
    pub fn jump_hunk(&mut self, forward: bool) -> bool {
        let Some(shown) = &self.shown else {
            return false;
        };
        let mut starts = shown.diff.hunk_rows.iter().map(|r| 1 + r);
        let target = if forward {
            starts.find(|&line| line > self.scroll.top)
        } else {
            starts.take_while(|&line| line < self.scroll.top).last()
        };
        target.is_some_and(|line| self.scroll.to(line, self.lines()))
    }

    pub fn draw_diff(&mut self, area: Rect, buf: &mut Buffer, theme: &Theme) {
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
        for y in 0..area.height {
            let line = self.scroll.top + u32::from(y);
            if line > shown.diff.len() {
                break;
            }
            let at = Rect::new(area.x, area.y + y, area.width, 1);
            if line == 0 {
                draw_title(at, buf, shown);
            } else {
                draw_row(
                    at,
                    buf,
                    &shown.diff,
                    (line - 1) as usize,
                    self.scroll.left,
                    theme,
                );
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
