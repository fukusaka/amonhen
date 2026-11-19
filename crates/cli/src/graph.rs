//! `amonhen log` のグラフを端末の文字で描く。
//!
//! 点と線をセルにする所（レーンから列への割り当て・罫線の文字）は `amonhen_projection::terminal` で、TUI と同じ。
//! ここでは、コミットの行の間に、曲線があるときだけ「つなぎの行」を挟む形で、ANSI の文字列にする。

use std::collections::HashMap;
use std::fmt::Write;

use amonhen_front::Reply;
use amonhen_layout::{Edge, Node};
use amonhen_model::{Oid, tr};
use amonhen_projection::terminal::{Graph, Line, palette};
use serde_json::Value;

pub struct Style {
    pub color: bool,
    /// 端末の幅（tty でなければ None で、切り詰めない）。
    pub width: Option<usize>,
}

pub fn paint(out: &mut String, text: &str, color: Option<u8>, enabled: bool) {
    match (color, enabled) {
        (Some(c), true) => {
            let _ = write!(out, "\x1b[38;5;{c}m{text}\x1b[0m");
        }
        _ => out.push_str(text),
    }
}

/// 格子の 1 行を文字列に。
fn line_text(line: &Line, color: bool) -> String {
    let mut out = String::new();
    for (ch, c) in line.cells() {
        paint(&mut out, &ch.to_string(), c.map(palette), color);
    }
    out
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
        Some(Graph::new(&rows.nodes, &rows.edges))
    } else {
        None
    };

    for (i, &row) in rows.numbers.iter().enumerate() {
        let mut line = String::new();
        let mut visible = 0;
        if let Some(g) = &graph {
            let l = g.commit_line(row, &rows.nodes[i]);
            line.push_str(&line_text(&l, style.color));
            line.push(' ');
            visible += l.len() + 1;
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
            out.push_str(line_text(&l, style.color).trim_end());
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
