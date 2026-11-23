//! block のマス目の配置（pele の `src/diagrams/block/layout.ts`）。

use crate::errors::{ErrorCode, PeleError, Result};
use crate::js;

#[derive(Clone, Debug, Default)]
pub struct Cell {
    /// 子（同じ並びの中の位置）。
    pub children: Vec<usize>,
    /// 入れ物のマス目の幅: 正の数か、子をすべて 1 行に並べる -1。
    pub columns: f64,
    pub span: f64,
    pub space: bool,
    /// 入れ物のマス目の上に題のために空ける高さ。
    pub head: f64,
    /// 中身に要る大きさ。葉は呼び手が決め、入れ物はここで求める。
    pub nw: f64,
    pub nh: f64,
    /// 置き場所: マス目の位置、それから画素での中心と大きさ。
    pub col: f64,
    pub row: f64,
    pub cols: f64,
    pub heights: Vec<f64>,
    /// 行・列ごとの後ろの余分な隙間（JS の穴は 0）。
    pub row_gaps: Vec<f64>,
    pub col_gaps: Vec<f64>,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

const MAX_SPAN: f64 = 1000.0;

pub fn cell(nw: f64, nh: f64, span: f64) -> Cell {
    Cell {
        columns: -1.0,
        span,
        nw,
        nh,
        ..Default::default()
    }
}

fn is_integer(x: f64) -> bool {
    x.is_finite() && x.trunc() == x
}

pub fn calculate_block_position(columns: f64, position: f64) -> Result<(f64, f64)> {
    if columns == 0.0 || !is_integer(columns) {
        return Err(PeleError::typed(
            "Columns must be an integer !== 0.",
            ErrorCode::Semantic,
            "block",
        ));
    }
    if position < 0.0 || !is_integer(position) {
        return Err(PeleError::typed(
            format!(
                "Position must be a non-negative integer.{}",
                js::fmt(position)
            ),
            ErrorCode::Semantic,
            "block",
        ));
    }
    if columns < 0.0 {
        return Ok((position, 0.0));
    }
    Ok((position % columns, (position / columns).floor()))
}

fn total(gaps: &[f64]) -> f64 {
    let mut sum = 0.0;
    for &gap in gaps {
        sum += gap;
    }
    sum
}

/// 子をそれぞれのマスに置き、入れ物の大きさを決める。列はどれも、いちばん広いブロックが要る幅。普通のブロックの
/// ある行は、入れ物の中でいちばん高いブロックの高さ（Mermaid と同じ）。入れ物だけの行は、それらが要る高さ。
fn measure(cells: &mut [Cell], b: usize, gap: f64, inset: f64) -> Result<()> {
    let columns = cells[b].columns;
    let mut heights: Vec<f64> = Vec::new();
    let mut position = 0.0;
    let mut cols = 0.0;
    let mut cw = 0.0;
    let mut tallest = 0.0;
    let children = std::mem::take(&mut cells[b].children);
    for &ci in &children {
        let (px, py) = calculate_block_position(columns, position)?;
        // Mermaid は行の残りより広いブロックをマス目の外にはみ出させる。ここでは最後の列で止める。
        let child = &mut cells[ci];
        let mut span = if child.span >= 1.0 {
            js::min(child.span, MAX_SPAN)
        } else {
            1.0
        };
        if columns > 0.0 && span > columns - px {
            span = columns - px;
        }
        child.span = span;
        child.col = px;
        child.row = py;
        position += span;
        if px + span > cols {
            cols = px + span;
        }
        let row = py as usize;
        while heights.len() <= row {
            heights.push(0.0);
        }
        if child.children.is_empty() {
            heights[row] = -1.0;
        } else if heights[row] >= 0.0 && child.nh > heights[row] {
            heights[row] = child.nh;
        }
        if child.space {
            continue;
        }
        let width = (child.nw - gap * (span - 1.0)) / span;
        if width > cw {
            cw = width;
        }
        if child.nh > tallest {
            tallest = child.nh;
        }
    }
    let bx = &mut cells[b];
    bx.children = children;
    let mut height = (heights.len() as f64 - 1.0) * gap + total(&bx.row_gaps);
    for h in heights.iter_mut() {
        if *h < 0.0 {
            *h = tallest;
        }
        height += *h;
    }
    bx.cols = cols;
    bx.heights = heights;
    bx.nw = js::max(
        bx.nw,
        cols * cw + (cols - 1.0) * gap + total(&bx.col_gaps) + 2.0 * inset,
    );
    bx.nh = height + 2.0 * inset + bx.head;
    Ok(())
}

/// 子にそれぞれのマスを与え、入れ物が要るより広い分を分ける。
fn arrange(cells: &mut [Cell], b: usize, gap: f64, inset: f64) {
    let bx = &cells[b];
    let cols = bx.cols;
    let heights = bx.heights.clone();
    let row_gaps = bx.row_gaps.clone();
    let col_gaps = bx.col_gaps.clone();
    let rows = heights.len() as f64;
    let cw = (bx.w - 2.0 * inset - (cols - 1.0) * gap - total(&col_gaps)) / cols;
    let room = bx.h - 2.0 * inset - bx.head - (rows - 1.0) * gap - total(&row_gaps);
    let mut offsets: Vec<f64> = Vec::new();
    if !col_gaps.is_empty() {
        let mut sum = 0.0;
        let mut col = 0.0;
        while col < cols {
            offsets.push(sum);
            sum += col_gaps.get(col as usize).copied().unwrap_or(0.0);
            col += 1.0;
        }
    }
    let mut needed = 0.0;
    for &h in &heights {
        needed += h;
    }
    let scale = if needed > 0.0 { room / needed } else { 0.0 };
    let share = if needed > 0.0 { 0.0 } else { room / rows };
    let left = bx.x - bx.w / 2.0 + inset;
    let mut top = bx.y - bx.h / 2.0 + inset + bx.head;
    let mut row = 0usize;
    let children = bx.children.clone();
    let at = |v: &[f64], i: f64| -> f64 {
        if i >= 0.0 {
            v.get(i as usize).copied().unwrap_or(0.0)
        } else {
            0.0
        }
    };
    for ci in children {
        let child = &mut cells[ci];
        while (row as f64) < child.row {
            top += heights[row] * scale + share + gap + at(&row_gaps, row as f64);
            row += 1;
        }
        let before = at(&offsets, child.col);
        child.w =
            cw * child.span + gap * (child.span - 1.0) + at(&offsets, child.col + child.span - 1.0)
                - before;
        child.h = heights[row] * scale + share;
        child.x = left + child.col * (cw + gap) + before + child.w / 2.0;
        child.y = top + child.h / 2.0;
    }
}

/// セルの木をその場で配置する。根には縁がないので、そのマス目は原点から始まる。`cells[0]` が根。
pub fn layout(cells: &mut [Cell], gap: f64) -> Result<(f64, f64)> {
    let mut order = vec![0usize];
    let mut i = 0;
    while i < order.len() {
        let kids = cells[order[i]].children.clone();
        order.extend(kids);
        i += 1;
    }
    for i in (0..order.len()).rev() {
        let b = order[i];
        if !cells[b].children.is_empty() {
            measure(cells, b, gap, if i == 0 { 0.0 } else { gap })?;
        }
    }
    let root = &mut cells[0];
    root.w = root.nw;
    root.h = root.nh;
    root.x = root.w / 2.0;
    root.y = root.h / 2.0;
    for (i, &b) in order.iter().enumerate() {
        if !cells[b].children.is_empty() {
            arrange(cells, b, gap, if i == 0 { 0.0 } else { gap });
        }
    }
    Ok((cells[0].w, cells[0].h))
}
