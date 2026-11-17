//! History の画面: グラフ・ref の札・件名・作者・日付。
//!
//! 行は窓口から範囲ごとに取り、見えている範囲の前後を手元に持つ（`Window`）。スクロールの 1 歩は手元の行だけで
//! 描き、端が近づいたら次の範囲を頼む。1 コミットは必ず 1 行なので、行と画面の位置は引き算で求まる。

use std::borrow::Cow;
use std::collections::HashMap;

use amonhen_front::Reply;
use amonhen_layout::{Edge, Node};
use amonhen_model::Oid;
use amonhen_projection::terminal::{Graph, palette};
use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Modifier, Style};
use serde_json::Value;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::loader::Request;

/// 手元に持つ行は、見えている範囲の前後にこの倍数の画面分（少なくとも `MIN_WINDOW` 行）。
const WINDOW_SCREENS: u32 = 4;
const MIN_WINDOW: u32 = 256;
/// グラフに使う幅は、画面のこの分の 1 まで。
const GRAPH_SHARE: u16 = 3;
/// ホイール 1 目盛りで動く行数。
pub const WHEEL_ROWS: i64 = 3;

const AUTHOR_WIDTH: u16 = 16;
const DATE_WIDTH: u16 = 4;
/// これより狭ければ、作者と日付を出さない。
const NARROW: u16 = 60;

const SELECTED: Color = Color::Indexed(238);
const DIM: Color = Color::Indexed(244);

#[derive(Clone, Copy)]
enum LabelKind {
    Head,
    Branch,
    Remote,
    Tag,
}

struct Label {
    name: String,
    kind: LabelKind,
}

/// 手元に持っている行（`start` から続けて）。
struct Window {
    start: u32,
    nodes: Vec<Node>,
    ids: Vec<Oid>,
    times: Vec<i64>,
    /// 行ごとに、件名と作者の `text` での位置と長さ（u32 × 4）。
    spans: Vec<u32>,
    text: Vec<u8>,
    /// この範囲に重なる線。
    edges: Vec<Edge>,
}

impl Window {
    fn len(&self) -> u32 {
        self.nodes.len() as u32
    }

    fn end(&self) -> u32 {
        self.start + self.len()
    }

    fn index(&self, row: u32) -> Option<usize> {
        (self.start <= row && row < self.end()).then(|| (row - self.start) as usize)
    }

    fn string(&self, index: usize, field: usize) -> &str {
        let at = index * 4 + field * 2;
        let (start, len) = (self.spans[at] as usize, self.spans[at + 1] as usize);
        std::str::from_utf8(&self.text[start..start + len]).unwrap_or("")
    }
}

#[derive(Default)]
pub struct History {
    id: Value,
    total: u32,
    /// HEAD のコミットの行。
    head_row: Option<u32>,
    labels: HashMap<Oid, Vec<Label>>,
    window: Option<Window>,
    loading: bool,
    /// 一番上に見えている行。
    top: u32,
    selected: u32,
    /// 行を出す所の高さ（最後に知らされた大きさ）。
    height: u32,
    incomplete: bool,
}

impl History {
    /// 開いた（か、読み直した）History と ref。
    pub fn opened(&mut self, refs: &Value, history: &Value) {
        self.id = history["history"].clone();
        self.total = history["rows"].as_u64().unwrap_or(0) as u32;
        self.head_row = history["headRow"].as_u64().map(|r| r as u32);
        self.window = None;
        self.labels.clear();
        let head = refs["headBranch"].as_str();
        for e in refs["entries"].as_array().into_iter().flatten() {
            let (Some(name), Some(target)) = (e["name"].as_str(), e["target"].as_str()) else {
                continue;
            };
            let kind = match e["kind"].as_str() {
                Some("branch") if Some(name) == head => LabelKind::Head,
                Some("branch") => LabelKind::Branch,
                Some("remote") => LabelKind::Remote,
                Some("tag") => LabelKind::Tag,
                _ => continue,
            };
            if let Some(oid) = Oid::from_hex(target) {
                self.labels.entry(oid).or_default().push(Label {
                    name: name.to_owned(),
                    kind,
                });
            }
        }
        // 今のブランチを先頭に。
        for labels in self.labels.values_mut() {
            labels.sort_by_key(|l| !matches!(l.kind, LabelKind::Head));
        }
        self.selected = self.selected.min(self.last());
        self.clamp_top();
    }

    pub fn total(&self) -> u32 {
        self.total
    }

    pub fn selected(&self) -> u32 {
        self.selected
    }

    /// 最後に描いたフレームに、まだ届いていない行があった。
    pub fn incomplete(&self) -> bool {
        self.incomplete
    }

    /// `down` の向きに、もうスクロールできない。
    pub fn at_end(&self, down: bool) -> bool {
        if down {
            self.top >= self.max_top()
        } else {
            self.top == 0
        }
    }

    fn last(&self) -> u32 {
        self.total.saturating_sub(1)
    }

    fn max_top(&self) -> u32 {
        self.total.saturating_sub(self.height)
    }

    fn clamp_top(&mut self) {
        self.top = self.top.min(self.max_top());
    }

    pub fn set_height(&mut self, height: u16) {
        self.height = u32::from(height);
        self.clamp_top();
    }

    /// 次に頼む行の範囲（もう頼んでいるか、足りていれば None）。見えている範囲の前後 1 画面が手元になければ、
    /// 前後 `WINDOW_SCREENS` 画面を取り直す。
    pub fn request(&mut self) -> Option<Request> {
        if self.loading || self.total == 0 || self.height == 0 {
            return None;
        }
        let need =
            self.top.saturating_sub(self.height)..(self.top + self.height * 2).min(self.total);
        if self
            .window
            .as_ref()
            .is_some_and(|w| w.start <= need.start && need.end <= w.end())
        {
            return None;
        }
        let around = (self.height * WINDOW_SCREENS).max(MIN_WINDOW);
        let start = self.top.saturating_sub(around);
        let end = (self.top + self.height + around).min(self.total);
        self.loading = true;
        Some(Request::Rows {
            history: self.id.clone(),
            start,
            count: end - start,
        })
    }

    pub fn loaded(&mut self, start: u32, reply: &Reply) {
        self.loading = false;
        self.window = Some(Window {
            start,
            nodes: reply.items("nodes"),
            ids: reply.items("ids"),
            times: reply.items("times"),
            spans: reply.u32s("spans"),
            text: reply.section("text").to_vec(),
            edges: reply.items("edges"),
        });
    }

    pub fn failed(&mut self) {
        self.loading = false;
    }

    /// 選択を `delta` 行動かし、見える所に入れる。
    pub fn move_selection(&mut self, delta: i64) -> bool {
        let to = (i64::from(self.selected) + delta).clamp(0, i64::from(self.last())) as u32;
        self.select(to)
    }

    pub fn page(&self) -> i64 {
        i64::from(self.height.saturating_sub(1).max(1))
    }

    pub fn select(&mut self, row: u32) -> bool {
        let row = row.min(self.last());
        let before = (self.selected, self.top);
        self.selected = row;
        if row < self.top {
            self.top = row;
        } else if row >= self.top + self.height {
            self.top = row + 1 - self.height;
        }
        self.clamp_top();
        before != (self.selected, self.top)
    }

    /// 選択はそのままで、見える範囲を `delta` 行動かす。
    pub fn scroll(&mut self, delta: i64) -> bool {
        let to = (i64::from(self.top) + delta).clamp(0, i64::from(self.max_top())) as u32;
        let changed = to != self.top;
        self.top = to;
        changed
    }

    /// 上から `line` 行目に見えている行を選ぶ。
    pub fn click(&mut self, line: u16) -> bool {
        let row = self.top + u32::from(line);
        row < self.total && self.select(row)
    }

    pub fn draw(&mut self, area: Rect, buf: &mut Buffer, now: i64) {
        self.incomplete = false;
        let Some(window) = &self.window else {
            self.incomplete = self.total > 0;
            return;
        };
        let rows = self.top..(self.top + u32::from(area.height)).min(self.total);
        if rows.is_empty() {
            return;
        }
        // 見えている行の点と、そこに重なる線だけで列を決める。
        let visible =
            window.index(rows.start).unwrap_or(0)..window.index(rows.end - 1).map_or(0, |i| i + 1);
        let edges: Vec<Edge> = window
            .edges
            .iter()
            .filter(|e| e.row_start < rows.end && rows.start <= e.row_end)
            .copied()
            .collect();
        let graph = Graph::new(&window.nodes[visible], &edges);
        let graph_width = (graph.width() as u16).min(area.width / GRAPH_SHARE);
        let wide = area.width >= NARROW;
        // 右端から: 日付、作者。
        let date_x = area.right().saturating_sub(DATE_WIDTH);
        let author_x = date_x.saturating_sub(AUTHOR_WIDTH + 1);
        let text_end = if wide {
            author_x.saturating_sub(1)
        } else {
            area.right()
        };

        for row in rows {
            let y = area.y + (row - self.top) as u16;
            let Some(i) = window.index(row) else {
                self.incomplete = true;
                continue;
            };
            // HEAD の印は、レイアウトではなく History の答えが持つ。
            let mut node = window.nodes[i];
            if Some(row) == self.head_row {
                node.flags |= Node::HEAD;
            }
            for (x, (ch, color)) in
                (area.x..area.x + graph_width).zip(graph.row_line(row, Some(&node)).cells())
            {
                let cell = &mut buf[(x, y)];
                cell.set_char(ch);
                if let Some(color) = color {
                    cell.set_fg(Color::Indexed(palette(color)));
                }
            }
            let mut x = area.x + graph_width + 1;
            for label in self.labels.get(&window.ids[i]).into_iter().flatten() {
                if x + 2 >= text_end {
                    break;
                }
                let style = match label.kind {
                    LabelKind::Head => Style::new()
                        .fg(Color::Indexed(114))
                        .add_modifier(Modifier::BOLD),
                    LabelKind::Branch => Style::new().fg(Color::Indexed(114)),
                    LabelKind::Remote => Style::new().fg(Color::Indexed(174)),
                    LabelKind::Tag => Style::new().fg(Color::Indexed(179)),
                };
                let name = fit(&label.name, usize::from(text_end - x - 1));
                x = buf
                    .set_stringn(x, y, format!("[{name}]"), usize::from(text_end - x), style)
                    .0
                    + 1;
            }
            if x < text_end {
                let subject = fit(window.string(i, 0), usize::from(text_end - x));
                buf.set_string(x, y, subject, Style::new());
            }
            if wide {
                let dim = Style::new().fg(DIM);
                let author = fit(window.string(i, 1), usize::from(AUTHOR_WIDTH));
                buf.set_string(author_x, y, author, dim);
                let date = ago(now, window.times[i]);
                let pad = DATE_WIDTH.saturating_sub(date.width() as u16);
                buf.set_string(date_x + pad, y, date, dim);
            }
            if row == self.selected {
                buf.set_style(
                    Rect::new(area.x, y, area.width, 1),
                    Style::new().bg(SELECTED),
                );
            }
        }
    }
}

/// `width` セルに収まるよう切り詰める（収まらなければ「…」で終える）。
fn fit(text: &str, width: usize) -> Cow<'_, str> {
    if text.width() <= width {
        return Cow::Borrowed(text);
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let w = ch.width().unwrap_or(0);
        if used + w + 1 > width {
            break;
        }
        used += w;
        out.push(ch);
    }
    if width > 0 {
        out.push('…');
    }
    Cow::Owned(out)
}

/// `time` が `now` からどれだけ前かを、4 文字までで（「5m」「3h」「2d」「6w」「11mo」「3y」）。
fn ago(now: i64, time: i64) -> String {
    let secs = (now - time).max(0);
    let (minute, hour, day) = (60, 3600, 86400);
    match secs {
        s if s < minute => "now".to_owned(),
        s if s < hour => format!("{}m", s / minute),
        s if s < day => format!("{}h", s / hour),
        s if s < day * 14 => format!("{}d", s / day),
        s if s < day * 60 => format!("{}w", s / (day * 7)),
        s if s < day * 365 => format!("{}mo", s / (day * 30)),
        s => format!("{}y", s / (day * 365)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 全角の文字も、セルの幅で切り詰める
    #[test]
    fn fit_truncates_by_cell_width() {
        assert_eq!(fit("abc", 3), "abc");
        assert_eq!(fit("abcd", 3), "ab…");
        assert_eq!(fit("日本語の件名", 7), "日本語…");
        assert_eq!(fit("abc", 0), "");
    }

    /// 日付は 4 文字までの「どれだけ前か」
    #[test]
    fn ago_stays_within_four_cells() {
        let day = 86400;
        assert_eq!(ago(100, 90), "now");
        assert_eq!(ago(day, 0), "1d");
        assert_eq!(ago(day * 20, 0), "2w");
        assert_eq!(ago(day * 200, 0), "6mo");
        assert_eq!(ago(day * 364, 0), "12mo");
        assert_eq!(ago(day * 800, 0), "2y");
    }
}
