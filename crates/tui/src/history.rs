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
use crate::rowcache::RowCache;

/// 手元に持つ行は、見えている範囲の前後にこの倍数の画面分（少なくとも `MIN_WINDOW` 行）。
const WINDOW_SCREENS: u32 = 4;
const MIN_WINDOW: u32 = 256;
/// 行のセルを、見えている行の何倍まで持っておくか（少し戻るスクロールでも写せるよう）。
const CACHE_SCREENS: usize = 2;
/// グラフに使う幅は、画面のこの分の 1 まで。
const GRAPH_SHARE: u16 = 3;
/// ホイール 1 目盛りで動く行数。
pub const WHEEL_ROWS: i64 = 3;

const AUTHOR_WIDTH: u16 = 16;
const DATE_WIDTH: u16 = 4;
/// これより狭ければ、作者と日付を出さない。
const NARROW: u16 = 60;

const SELECTED: Color = Color::Indexed(238);
/// ほかの所を操作している間の、選んでいる行。
const SELECTED_UNFOCUSED: Color = Color::Indexed(236);
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
    /// `edges` の線の、History のすべての線の中での番号（祖先の強調で、光らせる線を引くのに使う）。
    edge_ids: Vec<u32>,
    /// 折りたたんだ History で、行ごとの、取り込んだ側の隠したコミットの数（折りたたんでいなければ空）。
    folded: Vec<u32>,
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
    /// 最後に描いたときの `top`。
    drawn_top: u32,
    selected: u32,
    /// 行を出す所の高さ（最後に知らされた大きさ）。
    height: u32,
    incomplete: bool,
    /// 検索に合った行（上から順）。検索していなければ空。
    matches: Vec<u32>,
    /// 動いている検索の、途中の結果を入れた。
    partial: bool,
    /// 祖先を強調しているコミットの行と、その祖先（行ごとに 1 ビット）、線ごとの光らせ始める行（History の
    /// すべての線の並び）。
    lineage: Option<(u32, Vec<u64>, Vec<u32>)>,
    /// 祖先を頼んで、答えを待っている。
    lineage_loading: bool,
    /// 前のフレームで組み立てた行の、グラフより右のセル（ラベル・件名・作者・日付。選択と祖先の色は入れない）。
    /// 中身を決めるのは、幅、グラフの幅、今の時刻、`generation`。
    cache: RowCache<(u16, u16, i64, u64)>,
    /// 行の中身（ラベル・件名の色）が変わるたびに進める。
    generation: u64,
}

impl History {
    /// 開いた（か、読み直した）History と ref。
    pub fn opened(&mut self, refs: &Value, history: &Value) {
        self.id = history["history"].clone();
        self.total = history["rows"].as_u64().unwrap_or(0) as u32;
        self.head_row = history["headRow"].as_u64().map(|r| r as u32);
        self.window = None;
        // 行の番号が変わるので、検索の結果は捨てる（検索し直すのは呼ぶ側）。
        self.matches.clear();
        self.lineage = None;
        self.generation += 1;
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

    /// 前のフレームで組み立てた行を捨てる。
    pub fn forget_drawn(&mut self) {
        self.generation += 1;
    }

    pub fn total(&self) -> u32 {
        self.total
    }

    /// 検索に合った行を入れる。選んでいる行か、その下の最初の行（なければ一番上の行）を選ぶ。
    pub fn set_matches(&mut self, rows: Vec<u32>) {
        self.matches = rows;
        self.generation += 1;
        let at = self.matches.partition_point(|&r| r < self.selected);
        if let Some(&row) = self.matches.get(at).or(self.matches.first()) {
            self.select(row);
        }
    }

    /// 新しい検索を始めた（前の検索で合った行は、答えが届くまで残す）。
    pub fn begin_search(&mut self) {
        self.partial = false;
    }

    /// 動いている検索の、ここまでに合った行を入れる。最初に合った行が届いたときだけ、選ぶ行を動かす（後から
    /// 届く分で、見ている所を動かさない）。
    pub fn set_partial_matches(&mut self, rows: Vec<u32>) {
        if !std::mem::replace(&mut self.partial, true) {
            self.set_matches(rows);
        } else if rows != self.matches {
            self.matches = rows;
            self.generation += 1;
        }
    }

    pub fn clear_matches(&mut self) -> bool {
        let had = !self.matches.is_empty();
        self.matches.clear();
        self.generation += u64::from(had);
        had
    }

    /// 選んでいる行が何番目に合った行か（合った行でなければ `None`）と、合った行の数。
    pub fn match_position(&self) -> (Option<usize>, usize) {
        (
            self.matches
                .binary_search(&self.selected)
                .ok()
                .map(|i| i + 1),
            self.matches.len(),
        )
    }

    /// 次（`forward`）か前の合った行を選ぶ。端では反対の端へ回る。
    pub fn next_match(&mut self, forward: bool) -> bool {
        let row = if forward {
            let at = self.matches.partition_point(|&r| r <= self.selected);
            self.matches.get(at).or(self.matches.first())
        } else {
            let at = self.matches.partition_point(|&r| r < self.selected);
            at.checked_sub(1)
                .and_then(|i| self.matches.get(i))
                .or(self.matches.last())
        };
        row.copied().is_some_and(|row| self.select(row))
    }

    /// 選んでいるコミットの祖先がまだなら、頼む中身（答えを待っている間は、次を頼まない）。
    pub fn lineage_request(&mut self) -> Option<Request> {
        if self.lineage_loading
            || self.total == 0
            || self.id.is_null()
            || self.lineage.as_ref().is_some_and(|l| l.0 == self.selected)
        {
            return None;
        }
        self.lineage_loading = true;
        Some(Request::Lineage {
            history: self.id.clone(),
            row: self.selected,
        })
    }

    /// 祖先が届いた。待つ間に選択が動いていたら捨てる（`lineage_request` が次を頼む）。
    pub fn set_lineage(&mut self, row: u32, bits: Option<(Vec<u64>, Vec<u32>)>) {
        self.lineage_loading = false;
        self.lineage = match bits {
            Some((bits, edge_from)) if row == self.selected => Some((row, bits, edge_from)),
            // 取れなかった行は、頼み直さない。
            None if row == self.selected => Some((row, Vec::new(), Vec::new())),
            _ => self.lineage.take(),
        };
    }

    /// 窓口での History の番号。
    pub fn id(&self) -> &Value {
        &self.id
    }

    /// 選んでいるコミット（その行がまだ届いていなければ `None`）。
    pub fn selected_id(&self) -> Option<Oid> {
        let window = self.window.as_ref()?;
        Some(window.ids[window.index(self.selected)?])
    }

    /// 選んでいるコミットと、それが merge か、その件名（その行がまだ届いていなければ `None`）。
    pub fn selected_commit(&self) -> Option<(Oid, bool, &str)> {
        let window = self.window.as_ref()?;
        let i = window.index(self.selected)?;
        Some((
            window.ids[i],
            window.nodes[i].flags & Node::MERGE != 0,
            window.string(i, 0),
        ))
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
            edge_ids: reply.u32s("edgeIds"),
            folded: reply.u32s("folded"),
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

    /// 選んでいる行で、折りたたんで隠したコミットの数（折りたたんでいない、読んでいなければ 0）。
    pub fn selected_folded(&self) -> u32 {
        let Some(window) = &self.window else {
            return 0;
        };
        (window.index(self.selected))
            .and_then(|i| window.folded.get(i))
            .copied()
            .unwrap_or(0)
    }

    /// HEAD のコミットを選ぶ（HEAD が History になければ、何もしない）。
    pub fn go_to_head(&mut self) -> bool {
        self.head_row.is_some_and(|row| self.select(row))
    }

    /// 選んでいるコミットの親（1 つ目の親。なければいちばん近い親）を選ぶ。線は、両端の行のどちらかが読んである
    /// 範囲にあれば、窓に入っている。
    pub fn go_to_parent(&mut self) -> bool {
        let Some(window) = &self.window else {
            return false;
        };
        let selected = self.selected;
        let from = |first: bool| {
            (window.edges.iter())
                .filter(|e| e.child == selected && e.parent != selected)
                .filter(|e| !first || e.kind & Edge::FIRST_PARENT != 0)
                .map(|e| e.parent)
                .min()
        };
        from(true)
            .or_else(|| from(false))
            .is_some_and(|row| self.select(row))
    }

    /// 選んでいるコミットの子のうち、いちばん近いもの（すぐ上の子）を選ぶ。
    pub fn go_to_child(&mut self) -> bool {
        let Some(window) = &self.window else {
            return false;
        };
        let selected = self.selected;
        let child = (window.edges.iter())
            .filter(|e| e.parent == selected && e.child != selected)
            .map(|e| e.child)
            .max();
        child.is_some_and(|row| self.select(row))
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

    /// 最後に描いてから、縦に何行ずれたか（正なら、中身が上へ）。
    pub fn scrolled(&self) -> i32 {
        (i64::from(self.top) - i64::from(self.drawn_top)) as i32
    }

    pub fn draw(&mut self, area: Rect, buf: &mut Buffer, now: i64, focused: bool) {
        self.incomplete = false;
        self.drawn_top = self.top;
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
        let shown = |e: &&Edge| e.row_start < rows.end && rows.start <= e.row_end;
        let edges: Vec<Edge> = window.edges.iter().filter(shown).copied().collect();
        // 選んでいるコミットの祖先でない行は薄くする（祖先がまだ届いていなければ、薄くしない）。
        let lineage = self
            .lineage
            .as_ref()
            .filter(|l| l.0 == self.selected && !l.1.is_empty());
        // グラフは、祖先へ続く線と祖先の点だけを光らせる（線ごとの、光らせ始める行。線の番号が届いていれば）。
        let lit_from: Option<Vec<u32>> = lineage
            .filter(|_| window.edge_ids.len() == window.edges.len())
            .map(|l| {
                let from = |id: &u32| l.2.get(*id as usize).copied().unwrap_or(u32::MAX);
                (window.edges.iter().zip(&window.edge_ids))
                    .filter(|(e, _)| shown(e))
                    .map(|(_, id)| from(id))
                    .collect()
            });
        let lineage = lineage.map(|l| l.1.as_slice());
        let graph = Graph::new(&window.nodes[visible], &edges);
        let graph = match &lit_from {
            Some(lit_from) => graph.lit_from(lit_from),
            None => graph,
        };
        let graph_width = (graph.width() as u16).min(area.width / GRAPH_SHARE);
        let wide = area.width >= NARROW;
        // グラフより右は、前のフレームで組み立てた行なら写すだけ。
        let text_x = (area.x + graph_width + 1).min(area.right());
        let text_width = usize::from(area.right() - text_x);
        self.cache.prepare(
            (area.width, graph_width, now, self.generation),
            text_width,
            usize::from(area.height) * CACHE_SCREENS,
        );
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
            let ancestor = lineage.is_none_or(|bits| {
                bits.get((row / 64) as usize)
                    .is_some_and(|word| word & (1 << (row % 64)) != 0)
            });
            let line = graph.row_line_lit(row, Some(&node), ancestor);
            for (x, ((ch, color), lit)) in
                (area.x..area.x + graph_width).zip(line.cells().zip(line.lit()))
            {
                let cell = &mut buf[(x, y)];
                cell.set_char(ch);
                if let Some(color) = color {
                    cell.set_fg(Color::Indexed(palette(color)));
                }
                // 線ごとの印がなければ（古いエンジン）、祖先でない行のグラフをまとめて薄くする。
                let dim = match lit_from {
                    Some(_) => lineage.is_some() && !lit,
                    None => !ancestor,
                };
                if dim {
                    cell.set_style(Style::new().add_modifier(Modifier::DIM));
                }
            }
            let text = buf.index_of(text_x, y);
            let text = text..text + text_width;
            if !self.cache.copy_to(row, &mut buf.content[text.clone()]) {
                let at = Rect::new(text_x, y, text_width as u16, 1);
                self.text(window, i, row, at, wide, buf, now);
                self.cache.store(row, &buf.content[text]);
            }
            if !ancestor {
                buf.set_style(
                    Rect::new(text_x, y, text_width as u16, 1),
                    Style::new().add_modifier(Modifier::DIM),
                );
            }
            if row == self.selected {
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

    /// 行のグラフより右（`at`）: ラベル、件名、広ければ作者と日付。
    #[allow(clippy::too_many_arguments)]
    fn text(
        &self,
        window: &Window,
        i: usize,
        row: u32,
        at: Rect,
        wide: bool,
        buf: &mut Buffer,
        now: i64,
    ) {
        let y = at.y;
        // 右端から: 日付、作者。
        let date_x = at.right().saturating_sub(DATE_WIDTH);
        let author_x = date_x.saturating_sub(AUTHOR_WIDTH + 1);
        let text_end = if wide {
            author_x.saturating_sub(1)
        } else {
            at.right()
        };
        let mut x = at.x;
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
        // 折りたたんだマージは、取り込んだ側の隠したコミットの数を、件名の前に（アプリと同じ）。
        if let Some(&count) = window.folded.get(i).filter(|&&c| c > 0)
            && x + 2 < text_end
        {
            let folded = format!("+{count}");
            x =
                buf.set_stringn(
                    x,
                    y,
                    &folded,
                    usize::from(text_end - x),
                    Style::new().fg(DIM),
                )
                .0 + 1;
        }
        if x < text_end {
            let subject = fit(window.string(i, 0), usize::from(text_end - x));
            // 検索に合った行は、件名に色をつける。
            let style = match self.matches.binary_search(&row) {
                Ok(_) => Style::new().fg(Color::Yellow),
                Err(_) => Style::new(),
            };
            buf.set_string(x, y, subject, style);
        }
        if wide && author_x >= at.x {
            let dim = Style::new().fg(DIM);
            let author = fit(window.string(i, 1), usize::from(AUTHOR_WIDTH));
            buf.set_string(author_x, y, author, dim);
            let date = ago(now, window.times[i]);
            let pad = DATE_WIDTH.saturating_sub(date.width() as u16);
            buf.set_string(date_x + pad, y, date, dim);
        }
    }
}

/// `width` セルに収まるよう切り詰める（収まらなければ「…」で終える）。
pub(crate) fn fit(text: &str, width: usize) -> Cow<'_, str> {
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
pub(crate) fn ago(now: i64, time: i64) -> String {
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
