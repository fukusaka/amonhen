//! 1 つのファイルの画面: Blame（行ごとに、最後に変えたコミット）か File History（そのファイルを変えたコミット）。
//! 同じ画面に、ブランチを前の版と比べた結果（range-diff の組）も出す。
//! 選んだ行のコミットの詳細と差分は、History と同じ枠に出す。

use amonhen_model::{Oid, tr};
use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Style};
use serde_json::Value;
use unicode_width::UnicodeWidthStr;

use crate::diff::DIM;
use crate::history::{ago, fit};
use crate::loader::Request;

const SELECTED: Color = Color::Indexed(238);
const SELECTED_UNFOCUSED: Color = Color::Indexed(236);
/// Blame の左の列: 短い ID、作者、どれだけ前か。
const ID_WIDTH: usize = 8;
const AUTHOR_WIDTH: usize = 12;
const DATE_WIDTH: u16 = 4;

struct Line {
    /// その行のコミット（まだコミットしていない行は `None`）。
    oid: Option<Oid>,
    author: String,
    time: i64,
    /// Blame は行の中身、File History は件名。
    text: String,
    /// Blame で、同じコミットが続く行の最初（左の列を出す）。
    first: bool,
    /// File History で、そのコミットの時点のパス（名前を変えていれば今と違う）。
    path: Option<String>,
    /// range-diff で、今の版にない（前の版にだけある）組。
    removed: bool,
    /// range-diff で、変わった組の下に出す差分の差分の行なら、その外側の記号（` `・`-`・`+`、見出しは `@`）。
    outer: Option<char>,
}

#[derive(Default)]
pub struct FileView {
    /// range-diff を出しているなら、その題（「main against main@{1}」）。
    range: Option<String>,
    path: String,
    blame: bool,
    /// Blame するコミット（16 進の ID。なければ作業ツリーの内容）。
    rev: Option<String>,
    lines: Vec<Line>,
    loaded: bool,
    /// 頼むことがある（開いた直後）。
    wanted: bool,
    selected: usize,
    top: usize,
    height: u16,
}

impl FileView {
    /// `path` の Blame（`blame`）か File History を開く。
    pub fn open(&mut self, path: &str, blame: bool, rev: Option<String>) {
        *self = Self {
            path: path.to_owned(),
            blame,
            rev,
            wanted: true,
            height: self.height,
            ..Self::default()
        };
    }

    /// 開いているファイル（range-diff を出しているなら None）。
    pub fn file_path(&self) -> Option<&str> {
        (self.range.is_none() && !self.path.is_empty()).then_some(self.path.as_str())
    }

    pub fn request(&mut self) -> Option<Request> {
        std::mem::take(&mut self.wanted).then(|| Request::File {
            path: self.path.clone(),
            blame: self.blame,
            rev: self.rev.clone(),
        })
    }

    /// 答え（`blame` か `fileHistory` の `info`）。今開いているもののでなければ捨てる。
    pub fn set(
        &mut self,
        path: &str,
        blame: bool,
        reply: std::result::Result<Value, String>,
    ) -> Option<String> {
        if path != self.path || blame != self.blame {
            return None;
        }
        self.loaded = true;
        let info = match reply {
            Ok(mut reply) => reply["info"].take(),
            Err(error) => return Some(error),
        };
        self.lines = if blame {
            let commits = info["commits"].as_array().cloned().unwrap_or_default();
            let mut previous = None;
            info["lines"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|line| {
                    let index = line["c"].as_u64().unwrap_or(0) as usize;
                    let commit = commits.get(index).unwrap_or(&Value::Null);
                    let first = previous.replace(index) != Some(index);
                    Line {
                        // まだコミットしていない行の ID は 0 が並ぶ。
                        oid: commit["oid"]
                            .as_str()
                            .filter(|o| o.bytes().any(|b| b != b'0'))
                            .and_then(Oid::from_hex),
                        author: commit["author"].as_str().unwrap_or("").to_owned(),
                        time: commit["time"].as_i64().unwrap_or(0),
                        text: line["t"].as_str().unwrap_or("").replace('\t', "    "),
                        first,
                        path: None,
                        removed: false,
                        outer: None,
                    }
                })
                .collect()
        } else {
            info["entries"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|e| Line {
                    oid: e["oid"].as_str().and_then(Oid::from_hex),
                    author: e["author"].as_str().unwrap_or("").to_owned(),
                    time: e["time"].as_i64().unwrap_or(0),
                    text: e["subject"].as_str().unwrap_or("").to_owned(),
                    first: true,
                    path: e["path"].as_str().map(str::to_owned),
                    removed: false,
                    outer: None,
                })
                .collect()
        };
        self.selected = 0;
        self.top = 0;
        None
    }

    /// ブランチを前の版と比べた結果（`rangeDiff` の `info.pairs`）を出す。行は git range-diff の 1 行と同じ形
    /// （前の版の番号と ID、`=` 同じ・`!` 変わった・`<` 消えた・`>` 足した、今の版の番号と ID、件名）。変わった組の
    /// 下には、git と同じく差分の差分を字下げして並べる（その行を選ぶと、組のコミットの詳細）。
    pub fn show_range_diff(&mut self, title: String, pairs: &Value) {
        let side = |p: &Value| -> (String, Option<Oid>) {
            match p["index"].as_u64() {
                Some(i) => {
                    let oid = p["oid"].as_str().and_then(Oid::from_hex);
                    let short = oid.map_or(String::new(), |o| o.to_string()[..ID_WIDTH].to_owned());
                    (format!("{i}: {short}"), oid)
                }
                None => ("-:  ".to_owned() + &"-".repeat(ID_WIDTH), None),
            }
        };
        let lines = (pairs.as_array().into_iter().flatten())
            .flat_map(|p| {
                let (old, old_oid) = side(&p["old"]);
                let (new, new_oid) = side(&p["new"]);
                let mark = match p["status"].as_str() {
                    Some("same") => '=',
                    Some("changed") => '!',
                    Some("removed") => '<',
                    _ => '>',
                };
                let subject = p["subject"].as_str().unwrap_or("");
                let oid = new_oid.or(old_oid);
                let pair = Line {
                    oid,
                    author: String::new(),
                    time: 0,
                    text: format!("{old} {mark} {new}  {subject}"),
                    first: true,
                    path: None,
                    removed: new_oid.is_none(),
                    outer: None,
                };
                let diff = (p["diff"].as_array().into_iter().flatten())
                    .filter_map(Value::as_str)
                    .map(move |line| Line {
                        oid,
                        author: String::new(),
                        time: 0,
                        text: format!("    {}", line.replace('\t', "    ")),
                        first: false,
                        path: None,
                        removed: false,
                        outer: line.chars().next(),
                    });
                std::iter::once(pair).chain(diff)
            })
            .collect();
        *self = Self {
            range: Some(title),
            lines,
            loaded: true,
            height: self.height,
            ..Self::default()
        };
    }

    /// 一番上の行に出す名前（「Blame: a.rs」）。
    pub fn title(&self) -> String {
        if let Some(range) = &self.range {
            return range.clone();
        }
        match (self.blame, &self.rev) {
            (true, Some(rev)) => tr!(
                "Blame at {commit}: {path}",
                commit = &rev[..rev.len().min(8)],
                path = self.path
            ),
            (true, None) => tr!("Blame: {path}", path = self.path),
            (false, _) => tr!("File History: {path}", path = self.path),
        }
    }

    /// File History で選んでいる版: コミットと、今のパスと、そのコミットの時点のパス。Blame では `None`。
    pub fn selected_version(&self) -> Option<(Oid, &str, &str)> {
        if self.blame || self.range.is_some() {
            return None;
        }
        let line = self.lines.get(self.selected)?;
        Some((
            line.oid?,
            &self.path,
            line.path.as_deref().unwrap_or(&self.path),
        ))
    }

    pub fn selected_id(&self) -> Option<Oid> {
        self.lines.get(self.selected)?.oid
    }

    pub fn set_height(&mut self, height: u16) {
        self.height = height;
        self.clamp_top();
    }

    fn clamp_top(&mut self) {
        let height = usize::from(self.height.max(1));
        if self.selected < self.top {
            self.top = self.selected;
        } else if self.selected >= self.top + height {
            self.top = self.selected + 1 - height;
        }
        self.top = self.top.min(self.lines.len().saturating_sub(height));
    }

    pub fn page(&self) -> i64 {
        i64::from(self.height.max(2)) - 1
    }

    pub fn move_selection(&mut self, delta: i64) -> bool {
        self.select((self.selected as i64 + delta).max(0) as usize)
    }

    pub fn select(&mut self, index: usize) -> bool {
        let index = index.min(self.lines.len().saturating_sub(1));
        let changed = index != self.selected;
        self.selected = index;
        self.clamp_top();
        changed
    }

    pub fn scroll(&mut self, delta: i64) -> bool {
        let last = self
            .lines
            .len()
            .saturating_sub(usize::from(self.height.max(1)));
        let top = (self.top as i64 + delta).clamp(0, last as i64) as usize;
        let changed = top != self.top;
        self.top = top;
        changed
    }

    pub fn click(&mut self, line: u16) -> bool {
        let index = self.top + usize::from(line);
        index < self.lines.len() && self.select(index)
    }

    pub fn position(&self) -> (u32, u32) {
        let count = self.lines.len() as u32;
        ((self.selected as u32 + 1).min(count), count)
    }

    pub fn draw(&self, area: Rect, buf: &mut Buffer, now: i64, focused: bool) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        if self.lines.is_empty() {
            let text = match self.loaded {
                true => tr!("Nothing to show"),
                false => tr!("Loading…"),
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
        let dim = Style::new().fg(DIM);
        let width = usize::from(area.width);
        for (y, (index, line)) in (area.y..area.bottom()).zip(
            self.lines
                .iter()
                .enumerate()
                .skip(self.top)
                .take(usize::from(area.height)),
        ) {
            if self.range.is_some() {
                // 前の版にだけある組は薄く。差分の差分は、外側の記号で色を分ける（`-` は前の版だけ、`+` は今の版
                // だけにある行）。
                let style = match (line.removed, line.outer) {
                    (true, _) => dim,
                    (_, Some('-')) => Style::new().fg(Color::Red),
                    (_, Some('+')) => Style::new().fg(Color::Green),
                    (_, Some('@')) => Style::new().fg(Color::Cyan),
                    (_, Some(_)) => dim,
                    (_, None) => Style::new(),
                };
                buf.set_stringn(area.x, y, fit(&line.text, width), width, style);
            } else if self.blame {
                // 左の列は、同じコミットが続く行の最初だけ。
                let meta = ID_WIDTH + 1 + AUTHOR_WIDTH + 1 + usize::from(DATE_WIDTH) + 1;
                if line.first {
                    let id = match line.oid {
                        Some(oid) => oid.to_string()[..ID_WIDTH].to_owned(),
                        None => tr!("not committed"),
                    };
                    let x = buf
                        .set_stringn(area.x, y, id, ID_WIDTH + 1 + AUTHOR_WIDTH, dim)
                        .0;
                    if line.oid.is_some() {
                        buf.set_stringn(x + 1, y, fit(&line.author, AUTHOR_WIDTH), width, dim);
                        let date = ago(now, line.time);
                        let date_x = area.x + (ID_WIDTH + 1 + AUTHOR_WIDTH + 1) as u16;
                        if date_x + DATE_WIDTH < area.right() {
                            let pad = DATE_WIDTH.saturating_sub(date.width() as u16);
                            buf.set_string(date_x + pad, y, date, dim);
                        }
                    }
                }
                if width > meta {
                    buf.set_stringn(
                        area.x + meta as u16,
                        y,
                        &line.text,
                        width - meta,
                        Style::new(),
                    );
                }
            } else {
                let date_x = area.right().saturating_sub(DATE_WIDTH);
                let author_x = date_x.saturating_sub(AUTHOR_WIDTH as u16 + 1);
                if author_x > area.x + 2 {
                    let room = usize::from(author_x - area.x - 1);
                    buf.set_stringn(area.x, y, fit(&line.text, room), room, Style::new());
                    buf.set_stringn(
                        author_x,
                        y,
                        fit(&line.author, AUTHOR_WIDTH),
                        AUTHOR_WIDTH,
                        dim,
                    );
                    let date = ago(now, line.time);
                    let pad = DATE_WIDTH.saturating_sub(date.width() as u16);
                    buf.set_string(date_x + pad, y, date, dim);
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
