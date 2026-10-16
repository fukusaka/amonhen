//! `amonhen log` のグラフを端末の文字で描く。
//!
//! 点と線はコアのレイアウト（アプリと同じ）をそのまま使う。曲線はどれも隣り合う 2 行をつなぐので、コミットの行の
//! 間に、曲線があるときだけ「つなぎの行」を挟む。レーンが多いリポジトリ（linux で 1 行を横切る線が数百本）でも
//! 幅に収まるよう、表示する行の範囲に点か曲線のあるレーンだけに列を取り、線が通るだけのレーンは隣り合うものを
//! 細い 1 列（┆）にまとめる（アプリの魚眼と同じ考え）。

use std::collections::HashMap;
use std::fmt::Write;

use amonhen_core::{HistorySnapshot, Repository};
use amonhen_layout::{Edge, Node};
use amonhen_model::{Oid, RefKind, Refs};

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

fn paint(out: &mut String, text: &str, color: Option<u8>, enabled: bool) {
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
        Self { dirs: vec![0; width], glyph: vec![None; width], color: vec![None; width] }
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

pub fn render(
    repo: &Repository,
    history: &HistorySnapshot,
    refs: &Refs,
    rows: &[u32],
    show_graph: bool,
    style: &Style,
) -> amonhen_model::Result<String> {
    let mut out = String::new();
    if rows.is_empty() {
        return Ok(out);
    }
    let ids: Vec<Oid> = rows.iter().map(|&r| history.graph.ids()[r as usize]).collect();
    let texts = repo.commit_texts(&ids)?;
    let mut labels: HashMap<Oid, Vec<(String, RefKind)>> = HashMap::new();
    for e in &refs.entries {
        if let (Some(target), RefKind::Local | RefKind::Remote | RefKind::Tag) = (e.target, e.kind) {
            labels.entry(target).or_default().push((e.name.clone(), e.kind));
        }
    }
    let nodes = history.layout.nodes();
    let graph = if show_graph { Some(Graph::new(history, rows)) } else { None };

    for (i, &row) in rows.iter().enumerate() {
        let mut line = String::new();
        let mut visible = 0;
        if let Some(g) = &graph {
            let l = g.commit_line(row, &nodes[row as usize]);
            line.push_str(&l.text(style.color));
            line.push(' ');
            visible += l.dirs.len() + 1;
        }
        let text = &texts[i];
        let id = ids[i].to_string();
        paint(&mut line, &id[..8], Some(179), style.color);
        line.push(' ');
        visible += 9;
        if let Some(names) = labels.get(&ids[i]) {
            let head = refs.head_branch.as_deref();
            let mut parts = Vec::new();
            for (name, kind) in names {
                let (color, shown) = match kind {
                    RefKind::Local if Some(name.as_str()) == head => (114, format!("HEAD → {name}")),
                    RefKind::Local => (114, name.clone()),
                    RefKind::Remote => (174, name.clone()),
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
        let hidden = history.folded.get(row as usize).copied().unwrap_or(0);
        let subject = String::from_utf8_lossy(text.subject());
        let mut tail = subject.into_owned();
        if hidden > 0 {
            tail.push_str(&format!("  ⊕ {hidden} 件を折りたたみ"));
        }
        let meta = format!(
            "  — {}, {}",
            String::from_utf8_lossy(text.author_name()),
            crate::term::local_time(text.author_time, false)
        );
        // 幅を超えるなら、件名を切り詰める（作者・日付は残す）。
        if let Some(width) = style.width {
            let room = width.saturating_sub(visible + meta.chars().count());
            if tail.chars().count() > room {
                tail = tail.chars().take(room.saturating_sub(1)).collect::<String>() + "…";
            }
        }
        line.push_str(&tail);
        paint(&mut line, &meta, Some(244), style.color);
        line.push('\n');
        out.push_str(&line);
        if let Some(g) = &graph
            && let Some(next) = rows.get(i + 1)
            && *next == row + 1
            && let Some(l) = g.connector_line(row)
        {
            out.push_str(l.text(style.color).trim_end());
            out.push('\n');
        }
    }
    Ok(out)
}

/// `rows` の行を JSON の配列に（`amonhen log --json`）。`lane`・`color` は FE が描くときのレーンと色の番号。
pub fn json_rows(repo: &Repository, history: &HistorySnapshot, refs: &Refs, rows: &[u32]) -> amonhen_model::Result<serde_json::Value> {
    let ids: Vec<Oid> = rows.iter().map(|&r| history.graph.ids()[r as usize]).collect();
    let texts = repo.commit_texts(&ids)?;
    let mut labels: HashMap<Oid, Vec<String>> = HashMap::new();
    for e in &refs.entries {
        if let (Some(target), RefKind::Local | RefKind::Remote | RefKind::Tag) = (e.target, e.kind) {
            labels.entry(target).or_default().push(e.name.clone());
        }
    }
    let nodes = history.layout.nodes();
    let all = history.graph.ids();
    Ok(serde_json::Value::Array(
        rows.iter()
            .zip(&ids)
            .zip(&texts)
            .map(|((&row, id), text)| {
                let node = &nodes[row as usize];
                serde_json::json!({
                    "row": row,
                    "id": id.to_string(),
                    "parents": history.graph.parents(row as usize).iter().map(|&p| all[p as usize].to_string()).collect::<Vec<_>>(),
                    "subject": String::from_utf8_lossy(text.subject()),
                    "author": String::from_utf8_lossy(text.author_name()),
                    "authorTime": text.author_time,
                    "refs": labels.get(id).cloned().unwrap_or_default(),
                    "lane": node.lane,
                    "color": node.color,
                    "folded": history.folded.get(row as usize).copied().unwrap_or(0),
                })
            })
            .collect(),
    ))
}

/// 表示する行の範囲の点と線と、レーンから列への対応。
struct Graph<'a> {
    edges: Vec<&'a Edge>,
    /// レーン → 半列の位置。
    column: HashMap<u16, usize>,
    /// 細い列にまとめたレーン。
    thin: std::collections::HashSet<u16>,
    width: usize,
}

impl<'a> Graph<'a> {
    fn new(history: &'a HistorySnapshot, rows: &[u32]) -> Self {
        let first = *rows.first().unwrap();
        let last = *rows.last().unwrap();
        let layout = &history.layout;
        let edges: Vec<&Edge> = layout.edges_in(first..last + 1).map(|i| &layout.edges()[i as usize]).collect();
        // 点か曲線の端があるレーンは広く、線が通るだけのレーンは細く。
        let mut wide = std::collections::BTreeSet::new();
        for &r in rows {
            wide.insert(layout.nodes()[r as usize].lane);
        }
        let mut through = std::collections::BTreeSet::new();
        for e in &edges {
            if e.lane_start != e.lane_end {
                wide.insert(e.lane_start);
                wide.insert(e.lane_end);
            } else {
                through.insert(e.lane_start);
            }
        }
        let max_lane = wide.iter().chain(through.iter()).copied().max().unwrap_or(0);
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
        Self { edges, column, thin, width: col * 2 }
    }

    fn x(&self, lane: u16) -> usize {
        self.column.get(&lane).copied().unwrap_or(0) * 2
    }

    fn color(e: &Edge) -> u8 {
        PALETTE[e.color as usize % PALETTE.len()]
    }

    fn commit_line(&self, row: u32, node: &Node) -> Line {
        let mut line = Line::new(self.width);
        for e in &self.edges {
            if e.row_start <= row && row <= e.row_end {
                // 曲線は、端の行ではそのレーンの縦線として出す（向きを変えるのはつなぎの行）。
                let lane = if e.lane_start == e.lane_end || e.row_start == row { e.lane_start } else { e.lane_end };
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
        for e in &self.edges {
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
