//! `amonhen log` のグラフを端末の文字で描く。
//!
//! 点と線はコアのレイアウト（アプリと同じ）をそのまま使う。曲線はどれも隣り合う 2 行をつなぐので、コミットの行の
//! 間に、曲線があるときだけ「つなぎの行」を挟む。レーンが多いリポジトリ（linux で 1 行を横切る線が数百本）でも
//! 幅に収まるよう、表示する行の範囲に点か曲線のあるレーンだけに列を取り、線が通るだけのレーンは隣り合うものを
//! 細い 1 列（┆）にまとめる（アプリの魚眼と同じ考え）。

use std::collections::HashMap;
use std::fmt::Write;

use amonhen_front::Reply;
use amonhen_layout::{Edge, Node};
use amonhen_model::{Oid, tr};
use serde_json::Value;

pub struct Style {
    pub color: bool,
    /// 端末の幅（tty でなければ None で、切り詰めない）。
    pub width: Option<usize>,
}

const UP: u8 = 1;
const DOWN: u8 = 2;
const LEFT: u8 = 4;
const RIGHT: u8 = 8;

/// 線の向きの組から罫線の文字。
fn line_char(dirs: u8) -> char {
    match dirs {
        0 => ' ',
        d if d == UP | DOWN || d == UP || d == DOWN => '│',
        d if d == LEFT | RIGHT || d == LEFT || d == RIGHT => '─',
        d if d == UP | RIGHT => '╰',
        d if d == UP | LEFT => '╯',
        d if d == DOWN | RIGHT => '╭',
        d if d == DOWN | LEFT => '╮',
        d if d == UP | DOWN | RIGHT => '├',
        d if d == UP | DOWN | LEFT => '┤',
        d if d == LEFT | RIGHT | DOWN => '┬',
        d if d == LEFT | RIGHT | UP => '┴',
        _ => '┼',
    }
}

/// 線の色（アプリの色の番号を、端末の 256 色に寄せる）。
const PALETTE: [u8; 8] = [75, 114, 179, 176, 80, 209, 141, 150];

pub fn paint(out: &mut String, text: &str, color: Option<u8>, enabled: bool) {
    match (color, enabled) {
        (Some(c), true) => {
            let _ = write!(out, "\x1b[38;5;{c}m{text}\x1b[0m");
        }
        _ => out.push_str(text),
    }
}

/// 1 行分の格子（半列単位: 偶数がレーン、奇数がその間）。
struct Line {
    dirs: Vec<u8>,
    glyph: Vec<Option<char>>,
    color: Vec<Option<u8>>,
}

impl Line {
    fn new(width: usize) -> Self {
        Self {
            dirs: vec![0; width],
            glyph: vec![None; width],
            color: vec![None; width],
        }
    }

    fn add(&mut self, x: usize, dirs: u8, color: u8) {
        if x < self.dirs.len() {
            self.dirs[x] |= dirs;
            self.color[x].get_or_insert(color);
        }
    }

    fn text(&self, color: bool) -> String {
        let mut out = String::new();
        for x in 0..self.dirs.len() {
            let ch = self.glyph[x].unwrap_or_else(|| line_char(self.dirs[x]));
            paint(&mut out, &ch.to_string(), self.color[x], color && ch != ' ');
        }
        out
    }
}

/// 窓口（`historyRows`）から受け取った行。
pub struct Rows {
    pub numbers: Vec<u32>,
    pub nodes: Vec<Node>,
    pub ids: Vec<Oid>,
    pub times: Vec<i64>,
    pub subjects: Vec<String>,
    pub authors: Vec<String>,
    /// mailmap で置き換えて記録と違う行の、記録された作者（`{name, email}`。頼んだときだけ）。
    pub recorded: Vec<Option<Value>>,
    pub folded: Vec<u32>,
    pub parents: Vec<Vec<u32>>,
    pub edges: Vec<Edge>,
}

impl Rows {
    pub fn from(reply: &Reply) -> Self {
        let spans = reply.u32s("spans");
        let text = reply.section("text");
        let string = |i: usize| {
            let (off, len) = (spans[i] as usize, spans[i + 1] as usize);
            String::from_utf8_lossy(&text[off..off + len]).into_owned()
        };
        let numbers = reply.u32s("rowNumbers");
        let mut recorded = vec![None; numbers.len()];
        for r in reply.json["recorded"].as_array().into_iter().flatten() {
            if let Some(slot) = r["index"]
                .as_u64()
                .and_then(|i| recorded.get_mut(i as usize))
            {
                *slot = Some(serde_json::json!({ "name": r["name"], "email": r["email"] }));
            }
        }
        let starts = reply.u32s("parentStarts");
        let all_parents = reply.u32s("parents");
        Self {
            subjects: (0..numbers.len()).map(|r| string(r * 4)).collect(),
            authors: (0..numbers.len()).map(|r| string(r * 4 + 2)).collect(),
            recorded,
            parents: starts
                .windows(2)
                .map(|w| all_parents[w[0] as usize..w[1] as usize].to_vec())
                .collect(),
            nodes: reply.items("nodes"),
            ids: reply.items("ids"),
            times: reply.items("times"),
            folded: reply.u32s("folded"),
            edges: reply.items("edges"),
            numbers,
        }
    }
}

/// ブランチ・タグの名前をコミットごとに（窓口の `refs` の答えから）。
fn labels(refs: &Value) -> HashMap<String, Vec<(String, &'static str)>> {
    let mut labels: HashMap<String, Vec<(String, &'static str)>> = HashMap::new();
    for e in refs["entries"].as_array().into_iter().flatten() {
        let kind = match e["kind"].as_str() {
            Some("branch") => "branch",
            Some("remote") => "remote",
            Some("tag") => "tag",
            _ => continue,
        };
        if let (Some(target), Some(name)) = (e["target"].as_str(), e["name"].as_str()) {
            labels
                .entry(target.to_owned())
                .or_default()
                .push((name.to_owned(), kind));
        }
    }
    labels
}

pub fn render(rows: &Rows, refs: &Value, show_graph: bool, style: &Style) -> String {
    let mut out = String::new();
    if rows.numbers.is_empty() {
        return out;
    }
    let labels = labels(refs);
    let head = refs["headBranch"].as_str();
    let graph = if show_graph {
        Some(Graph::new(rows))
    } else {
        None
    };

    for (i, &row) in rows.numbers.iter().enumerate() {
        let mut line = String::new();
        let mut visible = 0;
        if let Some(g) = &graph {
            let l = g.commit_line(row, &rows.nodes[i]);
            line.push_str(&l.text(style.color));
            line.push(' ');
            visible += l.dirs.len() + 1;
        }
        let id = rows.ids[i].to_string();
        paint(&mut line, &id[..8], Some(179), style.color);
        line.push(' ');
        visible += 9;
        if let Some(names) = labels.get(&id) {
            let mut parts = Vec::new();
            for (name, kind) in names {
                let (color, shown) = match *kind {
                    "branch" if Some(name.as_str()) == head => (114, format!("HEAD → {name}")),
                    "branch" => (114, name.clone()),
                    "remote" => (174, name.clone()),
                    _ => (179, format!("tag: {name}")),
                };
                parts.push((color, shown));
            }
            line.push('(');
            for (k, (color, shown)) in parts.iter().enumerate() {
                if k > 0 {
                    line.push_str(", ");
                }
                paint(&mut line, shown, Some(*color), style.color);
                visible += shown.chars().count() + 2;
            }
            line.push_str(") ");
            visible += 1;
        }
        let hidden = rows.folded.get(i).copied().unwrap_or(0);
        let mut tail = rows.subjects[i].clone();
        if hidden > 0 {
            tail.push_str(&tr!("  ⊕ {count} folded", count = hidden));
        }
        let meta = format!(
            "  — {}, {}",
            rows.authors[i],
            crate::term::local_time(rows.times[i], false)
        );
        // 幅を超えるなら、件名を切り詰める（作者・日付は残す）。
        if let Some(width) = style.width {
            let room = width.saturating_sub(visible + meta.chars().count());
            if tail.chars().count() > room {
                tail = tail
                    .chars()
                    .take(room.saturating_sub(1))
                    .collect::<String>()
                    + "…";
            }
        }
        line.push_str(&tail);
        paint(&mut line, &meta, Some(244), style.color);
        line.push('\n');
        out.push_str(&line);
        if let Some(g) = &graph
            && let Some(next) = rows.numbers.get(i + 1)
            && *next == row + 1
            && let Some(l) = g.connector_line(row)
        {
            out.push_str(l.text(style.color).trim_end());
            out.push('\n');
        }
    }
    out
}

/// `rows` の行を JSON の配列に（`amonhen log --json`）。`lane`・`color` は FE が描くときのレーンと色の番号。
pub fn json_rows(rows: &Rows, refs: &Value) -> Value {
    let labels = labels(refs);
    Value::Array(
        rows.numbers
            .iter()
            .enumerate()
            .map(|(i, &row)| {
                let id = rows.ids[i].to_string();
                let mut item = serde_json::json!({
                    "row": row,
                    "id": id,
                    "parentRows": rows.parents[i],
                    "subject": rows.subjects[i],
                    "author": rows.authors[i],
                    "authorTime": rows.times[i],
                    "refs": labels.get(&id).map(|l| l.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>()).unwrap_or_default(),
                    "lane": rows.nodes[i].lane,
                    "color": rows.nodes[i].color,
                    "folded": rows.folded.get(i).copied().unwrap_or(0),
                });
                if let Some(recorded) = &rows.recorded[i] {
                    item["recordedAuthor"] = recorded.clone();
                }
                item
            })
            .collect(),
    )
}

/// 表示する行の範囲の点と線と、レーンから列への対応。
struct Graph<'a> {
    edges: &'a [Edge],
    /// レーン → 半列の位置。
    column: HashMap<u16, usize>,
    /// 細い列にまとめたレーン。
    thin: std::collections::HashSet<u16>,
    width: usize,
}

impl<'a> Graph<'a> {
    fn new(rows: &'a Rows) -> Self {
        let edges = &rows.edges[..];
        // 点か曲線の端があるレーンは広く、線が通るだけのレーンは細く。
        let mut wide = std::collections::BTreeSet::new();
        for n in &rows.nodes {
            wide.insert(n.lane);
        }
        let mut through = std::collections::BTreeSet::new();
        for e in edges {
            if e.lane_start != e.lane_end {
                wide.insert(e.lane_start);
                wide.insert(e.lane_end);
            } else {
                through.insert(e.lane_start);
            }
        }
        let max_lane = wide
            .iter()
            .chain(through.iter())
            .copied()
            .max()
            .unwrap_or(0);
        let mut column = HashMap::new();
        let mut thin = std::collections::HashSet::new();
        let mut col = 0;
        let mut in_thin_run = false;
        for lane in 0..=max_lane {
            if wide.contains(&lane) {
                column.insert(lane, col);
                col += 1;
                in_thin_run = false;
            } else if through.contains(&lane) {
                if !in_thin_run {
                    col += 1;
                    in_thin_run = true;
                }
                column.insert(lane, col - 1);
                thin.insert(lane);
            }
        }
        Self {
            edges,
            column,
            thin,
            width: col * 2,
        }
    }

    fn x(&self, lane: u16) -> usize {
        self.column.get(&lane).copied().unwrap_or(0) * 2
    }

    fn color(e: &Edge) -> u8 {
        PALETTE[e.color as usize % PALETTE.len()]
    }

    fn commit_line(&self, row: u32, node: &Node) -> Line {
        let mut line = Line::new(self.width);
        for e in self.edges {
            if e.row_start <= row && row <= e.row_end {
                // 曲線は、端の行ではそのレーンの縦線として出す（向きを変えるのはつなぎの行）。
                let lane = if e.lane_start == e.lane_end || e.row_start == row {
                    e.lane_start
                } else {
                    e.lane_end
                };
                line.add(self.x(lane), UP | DOWN, Self::color(e));
                if self.thin.contains(&lane) {
                    line.glyph[self.x(lane)] = Some('┆');
                }
            }
        }
        let x = self.x(node.lane);
        line.glyph[x] = Some(if node.flags & Node::HEAD != 0 {
            '◉'
        } else if node.flags & Node::MERGE != 0 {
            '◆'
        } else {
            '●'
        });
        line.color[x] = Some(PALETTE[node.color as usize % PALETTE.len()]);
        line
    }

    /// `row` と次の行の間（曲線がなければ None）。
    fn connector_line(&self, row: u32) -> Option<Line> {
        let mut line = Line::new(self.width);
        let mut curved = false;
        for e in self.edges {
            let c = Self::color(e);
            if e.lane_start != e.lane_end && e.row_start == row {
                curved = true;
                let (a, b) = (self.x(e.lane_start), self.x(e.lane_end));
                if b > a {
                    line.add(a, UP | RIGHT, c);
                    (a + 1..b).for_each(|x| line.add(x, LEFT | RIGHT, c));
                    line.add(b, LEFT | DOWN, c);
                } else {
                    line.add(a, UP | LEFT, c);
                    (b + 1..a).for_each(|x| line.add(x, LEFT | RIGHT, c));
                    line.add(b, RIGHT | DOWN, c);
                }
            } else if e.lane_start == e.lane_end && e.row_start <= row && row < e.row_end {
                let x = self.x(e.lane_start);
                line.add(x, UP | DOWN, c);
                if self.thin.contains(&e.lane_start) {
                    line.glyph[x] = Some('┆');
                }
            }
        }
        curved.then_some(line)
    }
}
