//! treemap の配置（pele の `src/diagrams/treemap/layout.ts`）。

use super::model::TreemapNode;
use crate::js;

#[derive(Clone, Debug)]
pub struct Box {
    pub node: usize,
    pub parent: Option<usize>,
    /// 箱の属する枝の、上の段のノードの中の位置。色を選ぶ。
    pub series: usize,
    /// 葉はその値、節はその下の葉の和。
    pub value: f64,
    pub children: Vec<usize>,
    /// 小さな節は見出しの場所を子に譲る。
    pub headed: bool,
    pub header: f64,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Clone, Copy)]
pub struct Insets {
    pub gap: f64,
    /// 節が見出しに取る場所と、ほかの 3 辺で子のまわりに取る場所。
    pub header: f64,
    pub short: f64,
    pub side: f64,
}

fn new_box(nodes: &[TreemapNode], node: usize, parent: Option<usize>, series: usize) -> Box {
    let n = &nodes[node];
    let value = match (n.children.is_none(), n.value) {
        (true, Some(v)) if v.is_finite() && v > 0.0 => v,
        _ => 0.0,
    };
    Box {
        node,
        parent,
        series,
        value,
        children: Vec::new(),
        headed: false,
        header: 0.0,
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
    }
}

fn clamp(v: f64, low: f64, high: f64) -> f64 {
    if v >= low { js::min(v, high) } else { low }
}

/// 箱を矩形に 1 つの正方化した treemap として並べる: 行は短い辺に沿って足し、次の箱で箱が正方形から遠のくなら
/// 行を止める。
#[allow(clippy::too_many_arguments)]
fn squarify(
    all: &mut [Box],
    boxes: &[usize],
    total: f64,
    mut x: f64,
    mut y: f64,
    mut w: f64,
    mut h: f64,
    gap: f64,
) {
    let mut rest = total;
    let mut from = 0;
    while from < boxes.len() {
        let scale = (w * h) / rest;
        let side = js::min(w, h);
        let mut sum = all[boxes[from]].value * scale;
        let largest = sum;
        let mut worst = js::max(
            (side * side * largest) / (sum * sum),
            (sum * sum) / (side * side * largest),
        );
        let mut to = from + 1;
        while to < boxes.len() {
            let smallest = all[boxes[to]].value * scale;
            let next = sum + smallest;
            let ratio = js::max(
                (side * side * largest) / (next * next),
                (next * next) / (side * side * smallest),
            );
            if ratio > worst {
                break;
            }
            worst = ratio;
            sum = next;
            to += 1;
        }

        let thickness = sum / side;
        let mut offset = 0.0;
        for i in from..to {
            let b = &mut all[boxes[i]];
            let length = if i == to - 1 {
                side - offset
            } else {
                (b.value * scale) / thickness
            };
            let across = w >= h;
            let bx = if across { x } else { x + offset };
            let by = if across { y + offset } else { y };
            let bw = if across { thickness } else { length };
            let bh = if across { length } else { thickness };
            // 箱を消してしまうなら隙間を取らない。
            let gx = if js::round(bx + bw - gap / 2.0) > js::round(bx + gap / 2.0) {
                gap / 2.0
            } else {
                0.0
            };
            let gy = if js::round(by + bh - gap / 2.0) > js::round(by + gap / 2.0) {
                gap / 2.0
            } else {
                0.0
            };
            b.x = js::round(bx + gx);
            b.y = js::round(by + gy);
            b.w = js::round(bx + bw - gx) - b.x;
            b.h = js::round(by + bh - gy) - b.y;
            offset += length;
            rest -= b.value;
        }
        if w >= h {
            x += thickness;
            w -= thickness;
        } else {
            y += thickness;
            h -= thickness;
        }
        from = to;
    }
}

/// 値が正の箱（大きさ 0 の箱も）を、親を子より先に返す。
pub fn layout_treemap(
    nodes: &[TreemapNode],
    roots: &[usize],
    width: f64,
    height: f64,
    insets: &Insets,
) -> (Vec<Box>, Vec<usize>) {
    let mut all: Vec<Box> = roots
        .iter()
        .enumerate()
        .map(|(index, &node)| new_box(nodes, node, None, index))
        .collect();
    let top: Vec<usize> = (0..all.len()).collect();
    let mut i = 0;
    while i < all.len() {
        let series = all[i].series;
        let kids = nodes[all[i].node].children.clone().unwrap_or_default();
        for node in kids {
            let child = new_box(nodes, node, Some(i), series);
            let k = all.len();
            all.push(child);
            all[i].children.push(k);
        }
        i += 1;
    }
    for i in (0..all.len()).rev() {
        if let Some(p) = all[i].parent {
            all[p].value += all[i].value;
        }
    }

    let Insets {
        gap,
        header,
        short,
        side,
    } = *insets;
    let mut placed: Vec<usize> = Vec::new();
    let place = |all: &mut Vec<Box>,
                 placed: &mut Vec<usize>,
                 children: &[usize],
                 x: f64,
                 y: f64,
                 w: f64,
                 h: f64| {
        let mut shown: Vec<usize> = children
            .iter()
            .copied()
            .filter(|&b| all[b].value > 0.0)
            .collect();
        shown.sort_by(|&a, &b| js::order(all[b].value - all[a].value));
        let mut total = 0.0;
        for &b in &shown {
            total += all[b].value;
        }
        if shown.is_empty() || !total.is_finite() {
            return;
        }
        let w = js::max(0.0, w);
        let h = js::max(0.0, h);
        if w > 0.0 && h > 0.0 {
            squarify(
                all,
                &shown,
                total,
                x - gap / 2.0,
                y - gap / 2.0,
                w + gap,
                h + gap,
                gap,
            );
        }
        for &k in &shown {
            let b = &mut all[k];
            let x0 = clamp(b.x, x, x + w);
            let y0 = clamp(b.y, y, y + h);
            b.w = clamp(b.x + b.w, x0, x + w) - x0;
            b.h = clamp(b.y + b.h, y0, y + h) - y0;
            b.x = x0;
            b.y = y0;
            placed.push(k);
        }
    };

    place(&mut all, &mut placed, &top, 0.0, 0.0, width, height);
    let mut i = 0;
    while i < placed.len() {
        let k = placed[i];
        i += 1;
        if all[k].children.is_empty() {
            continue;
        }
        let b = &all[k];
        let top = if b.h - header - side >= 12.0 {
            header
        } else {
            short
        };
        let headed = b.w > js::max(24.0, 2.0 * side) && b.h - top - side >= 8.0;
        let (bx, by, bw, bh) = (b.x, b.y, b.w, b.h);
        let children = b.children.clone();
        all[k].headed = headed;
        all[k].header = if headed { top } else { 0.0 };
        if headed {
            place(
                &mut all,
                &mut placed,
                &children,
                bx + side,
                by + top,
                bw - 2.0 * side,
                bh - top - side,
            );
        } else {
            place(&mut all, &mut placed, &children, bx, by, bw, bh);
        }
    }
    (all, placed)
}
