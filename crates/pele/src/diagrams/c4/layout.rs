//! C4 の配置（pele の `src/diagrams/c4/layout.ts`）。

use std::collections::HashMap;

use super::db::{C4Db, ElementRef};
use super::route::{Box, HEAD, LABEL_PAD, LANE, NARROW};
use super::types::C4Element;
use crate::js;
use crate::svg::edges::ARROW;
use crate::text::label::{Label, layout_label};
use crate::text::measurer::{TextMeasurer, style};

const SHAPE_W: f64 = 216.0;
const SHAPE_MIN_H: f64 = 64.0;
const PAD_X: f64 = 14.0;
const PAD_Y: f64 = 12.0;
/// 2 つの列の間を横切る線のラベルの場所。どちらの矢じりからも離れ、ラベルの場所より少し広い語のために矢じり
/// 1 つ分のゆとりを持つ。
const GAP_X: f64 = NARROW + LABEL_PAD + 3.0 * ARROW;
const GAP_Y: f64 = 88.0;
/// 関係が境界の内側に沿って通る場所。
pub const GROUP_PAD: f64 = LANE + 8.0;
const GROUP_GAP: f64 = 48.0;
/// 境界の見出しが始まる所（左上の角から）。
pub const HEAD_LEFT: f64 = 16.0;
pub const HEAD_TOP: f64 = 10.0;

/// Mermaid が $shape・$sprite・$tags で受け付ける形のキーワードを、共通の形の名前に。
fn form_named(name: &str) -> Option<&'static str> {
    Some(match name {
        "person" => "person",
        "box" | "rounded" | "component" => "rect",
        "cylinder" | "database" | "db" => "cyl",
        "queue" | "pipe" => "h-cyl",
        _ => return None,
    })
}

pub struct Node {
    pub b: Box,
    pub shape: usize,
    /// 種類、名前、技術、説明。
    pub labels: Vec<Label>,
    pub text_h: f64,
}

pub struct Group {
    pub b: Box,
    pub boundary: usize,
    /// 中の境界（`Layout::groups` の位置）。
    pub kids: Vec<usize>,
    pub nodes: Vec<Node>,
    /// 名前、種類、説明。
    pub labels: Vec<Label>,
    pub head: f64,
    pub head_w: f64,
    pub head_x: f64,
}

pub struct Layout {
    /// 描く境界（`groups` の位置）。親が子より先。最初は名のない上の段。
    pub order: Vec<usize>,
    /// 境界ごと（`C4Db::boundaries` と同じ並び）。
    pub groups: Vec<Group>,
    pub boxes: HashMap<ElementRef, Box>,
}

fn form_of(shape: &C4Element) -> &'static str {
    let explicit = form_named(&shape.shape.as_deref().unwrap_or("").to_lowercase())
        .or_else(|| form_named(&shape.sprite.as_deref().unwrap_or("").to_lowercase()));
    if let Some(f) = explicit {
        return f;
    }
    if let Some(tags) = shape.tags.as_deref().filter(|t| !t.is_empty()) {
        for tag in tags.split(',') {
            if let Some(f) = form_named(&js::trim(tag).to_lowercase()) {
                return f;
            }
        }
    }
    let kind = &shape.kind;
    if kind.ends_with("person") {
        "person"
    } else if kind.ends_with("_db") {
        "cyl"
    } else if kind.ends_with("_queue") {
        "h-cyl"
    } else {
        "rect"
    }
}

fn measure(
    shape: &C4Element,
    index: usize,
    id: usize,
    measurer: &dyn TextMeasurer,
    size: f64,
    small: f64,
    width: f64,
) -> Node {
    let form = form_of(shape);
    let cap_w = if form == "h-cyl" { 36.0 } else { 0.0 };
    let cap_h = if form == "person" {
        HEAD
    } else if form == "cyl" {
        24.0
    } else {
        0.0
    };
    let inner = width - 2.0 * PAD_X - cap_w;
    let kind = format!("«{}»", shape.kind.replace('_', " "));
    let techn = shape
        .techn
        .as_deref()
        .filter(|t| !t.is_empty())
        .map(|t| format!("[{t}]"));
    let labels = vec![
        layout_label(Some(&kind), false, measurer, small - 1.0, inner, 0),
        layout_label(
            Some(&shape.label),
            false,
            measurer,
            size,
            inner,
            style::BOLD,
        ),
        layout_label(techn.as_deref(), false, measurer, small, inner, 0),
        layout_label(shape.descr.as_deref(), false, measurer, small, inner, 0),
    ];
    let mut tw: f64 = 0.0;
    let mut text_h = if labels[3].height > 0.0 { 4.0 } else { 0.0 };
    for label in &labels {
        tw = js::max(tw, label.width);
        text_h += label.height;
    }
    Node {
        b: Box {
            id,
            x: 0.0,
            y: 0.0,
            w: js::max(width, tw + 2.0 * PAD_X + cap_w),
            h: js::max(SHAPE_MIN_H, text_h + 2.0 * PAD_Y + cap_h),
            form,
        },
        shape: index,
        labels,
        text_h,
    }
}

/// 箱を (0, 0) から `per_row` ずつの行に左から右へ置き、行の取る幅と高さを返す。形は行の高さに伸ばし、境界は
/// 自分の高さのまま。
fn rows(boxes: &mut [&mut Box], per_row: f64, gap_x: f64, gap_y: f64, stretch: bool) -> (f64, f64) {
    let len = boxes.len();
    let mut width: f64 = 0.0;
    let mut y = 0.0;
    let mut from = 0.0f64;
    while from < len as f64 {
        let to = js::min(len as f64, from + per_row) as usize;
        let start = from as usize;
        let mut x = 0.0;
        let mut row_h: f64 = 0.0;
        for b in &boxes[start..to] {
            row_h = js::max(row_h, b.h);
        }
        for b in &mut boxes[start..to] {
            b.x = x;
            b.y = y;
            if stretch {
                b.h = row_h;
            }
            x += b.w + gap_x;
        }
        width = js::max(width, x - gap_x);
        y += row_h + gap_y;
        from += per_row;
    }
    (width, if len > 0 { y - gap_y } else { 0.0 })
}

/// Mermaid の格子: 各境界で、形を c4ShapeInRow ずつの行に、その下に中の境界を c4BoundaryInRow ずつの行に置き、
/// それぞれを自分の中身のまわりの大きさにする。
pub fn layout_c4(
    db: &C4Db,
    measurer: &dyn TextMeasurer,
    size: f64,
    small: f64,
    room: Option<f64>,
    gap: f64,
) -> Layout {
    let group_pad = if room.is_none() { GROUP_PAD } else { 12.0 };
    let mut groups: Vec<Group> = Vec::new();
    for (i, boundary) in db.boundaries.iter().enumerate() {
        let label_room = match room {
            None => 4000.0,
            Some(r) => js::max(80.0, r - 2.0 * HEAD_LEFT - 2.0 * GROUP_PAD * 2.0),
        };
        let labels = if boundary.parent.is_some() {
            let type_ = boundary
                .type_
                .as_deref()
                .filter(|t| !t.is_empty())
                .map(|t| format!("[{t}]"));
            vec![
                layout_label(
                    Some(&boundary.label),
                    false,
                    measurer,
                    size,
                    label_room,
                    style::BOLD,
                ),
                layout_label(type_.as_deref(), false, measurer, small, label_room, 0),
                layout_label(
                    boundary.descr.as_deref(),
                    false,
                    measurer,
                    small,
                    label_room,
                    0,
                ),
            ]
        } else {
            Vec::new()
        };
        let mut head = 0.0;
        let mut head_w: f64 = 0.0;
        for label in &labels {
            head += label.height;
            head_w = js::max(head_w, label.width);
        }
        groups.push(Group {
            b: Box {
                id: i,
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
                form: "rect",
            },
            boundary: i,
            kids: Vec::new(),
            nodes: Vec::new(),
            labels,
            head: if head > 0.0 {
                head + HEAD_TOP + 6.0 + LANE
            } else {
                group_pad
            },
            head_w,
            head_x: HEAD_LEFT,
        });
    }
    let root = 0;
    for (i, boundary) in db.boundaries.iter().enumerate() {
        if let Some(parent) = boundary.parent {
            groups[parent].kids.push(i);
        }
    }
    // 自分の中に入った境界は、Mermaid と同じく上の段から届かない。
    let mut order: Vec<usize> = vec![root];
    let mut depths: HashMap<usize, f64> = HashMap::new();
    depths.insert(root, 0.0);
    let mut i = 0;
    while i < order.len() {
        let g = order[i];
        let d = depths[&g];
        for &kid in &groups[g].kids {
            depths.insert(kid, d + 1.0);
            order.push(kid);
        }
        i += 1;
    }
    let nb = db.boundaries.len();
    for (s, shape) in db.shapes.iter().enumerate() {
        if let Some(parent) = shape.parent {
            let depth = depths.get(&parent).copied().unwrap_or(0.0);
            let node_width = match room {
                None => SHAPE_W,
                Some(r) => js::max(100.0, js::min(SHAPE_W, r - 2.0 * depth * group_pad - 32.0)),
            };
            let node = measure(shape, s, nb + s, measurer, size, small, node_width);
            groups[parent].nodes.push(node);
        }
    }

    let per_row = js::max(1.0, db.shape_in_row);
    let groups_per_row = js::max(1.0, db.boundary_in_row);
    let mut kids_top: HashMap<usize, f64> = HashMap::new();
    let count = |widths: &[f64], limit: f64, gap: f64| -> f64 {
        let Some(room) = room else {
            return limit;
        };
        let mut widest: f64 = 1.0;
        for &w in widths {
            widest = js::max(widest, w);
        }
        js::max(
            1.0,
            js::min(
                limit,
                ((room + gap - 2.0 * group_pad) / (widest + gap)).floor(),
            ),
        )
    };
    for &g in order.iter().rev() {
        let gap_x = if room.is_none() { GAP_X } else { gap };
        let node_widths: Vec<f64> = groups[g].nodes.iter().map(|n| n.b.w).collect();
        let n_per = count(&node_widths, per_row, gap_x);
        let (grid_w, grid_h) = {
            let mut list: Vec<&mut Box> = groups[g].nodes.iter_mut().map(|n| &mut n.b).collect();
            rows(&mut list, n_per, gap_x, GAP_Y, true)
        };
        let kids = groups[g].kids.clone();
        let mut kid_boxes: Vec<Box> = kids.iter().map(|&k| groups[k].b.clone()).collect();
        let kid_widths: Vec<f64> = kid_boxes.iter().map(|b| b.w).collect();
        let k_per = count(&kid_widths, groups_per_row, GROUP_GAP);
        let (kids_w, kids_h) = {
            let mut list: Vec<&mut Box> = kid_boxes.iter_mut().collect();
            rows(&mut list, k_per, GROUP_GAP, GROUP_GAP, false)
        };
        for (k, b) in kids.iter().zip(kid_boxes) {
            groups[*k].b = b;
        }
        let between = if grid_h > 0.0 && kids_h > 0.0 {
            GROUP_GAP
        } else {
            0.0
        };
        let frame = if g == root { 0.0 } else { group_pad };
        kids_top.insert(g, grid_h + between);
        let group = &mut groups[g];
        group.b.w = js::max_of(&[grid_w, kids_w, group.head_w + 2.0 * HEAD_LEFT - 2.0 * frame])
            + 2.0 * frame;
        let framed_head = if frame == 0.0 { frame } else { group.head };
        group.b.h = framed_head + grid_h + between + kids_h + frame;
    }

    let mut boxes: HashMap<ElementRef, Box> = HashMap::new();
    for &g in &order {
        let left = groups[g].b.x + if g == root { 0.0 } else { group_pad };
        let top = groups[g].b.y + if g == root { 0.0 } else { groups[g].head };
        for node in &mut groups[g].nodes {
            node.b.x += left;
            node.b.y += top;
            boxes.insert(ElementRef::Shape(node.shape), node.b.clone());
        }
        let below = kids_top[&g];
        for k in 0..groups[g].kids.len() {
            let kid = groups[g].kids[k];
            groups[kid].b.x += left;
            groups[kid].b.y += top + below;
        }
        if g != root {
            boxes.insert(
                ElementRef::Boundary(groups[g].boundary),
                groups[g].b.clone(),
            );
        }
    }
    Layout {
        order,
        groups,
        boxes,
    }
}
