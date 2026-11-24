//! osage と patchwork: クラスタの入れ子を箱として敷く。
//!
//! - osage: 各クラスタの中（内のクラスタの箱と直に入るノード）を配列に詰める（`packmode=array` の行・列、
//!   `_c`・`_u`・`t/b/l/r` の印と数）。内から外へ箱を作る。
//! - patchwork: クラスタの木を squarified treemap（Bruls, Huizing, van Wijk, "Squarified Treemaps", 2000）で
//!   敷き詰める。ノードの面積は `area`（既定 1。1 は 40pt 四方で、Graphviz の絵の大きさに合わせた）。ノードは升いっぱいの箱になる。

use std::collections::BTreeSet;

use super::common::P;
use super::{Group, Item};
use crate::Note;
use crate::graph::Attrs;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Unit {
    Node(usize),
    Cluster(usize),
}

/// クラスタ `level` の直の中身（中にノードのあるクラスタと、直に入るノード）。
fn units(items: &[Item], groups: &[Group], level: Option<usize>, live: &[bool]) -> Vec<Unit> {
    let mut out: Vec<Unit> = (0..groups.len())
        .filter(|&c| groups[c].parent == level && live[c] && Some(c) != level)
        .map(Unit::Cluster)
        .collect();
    out.extend(
        items
            .iter()
            .enumerate()
            .filter(|(_, it)| it.cluster.filter(|&c| c < groups.len()) == level)
            .map(|(v, _)| Unit::Node(v)),
    );
    out
}

/// 中にノードのあるクラスタ（内のクラスタを通しても）。
fn live_groups(items: &[Item], groups: &[Group]) -> Vec<bool> {
    let mut live = vec![false; groups.len()];
    for it in items {
        let mut c = it.cluster.filter(|&c| c < groups.len());
        let mut steps = 0;
        while let Some(x) = c {
            if live[x] || steps > groups.len() {
                break;
            }
            live[x] = true;
            c = groups[x].parent;
            steps += 1;
        }
    }
    live
}

/// `packmode` の配列の決まり。
struct Array {
    column_major: bool,
    user_order: bool,
    count: Option<usize>,
    /// 升の中の寄せ（-1 左・上、0 中、1 右・下）。
    align_x: i8,
    align_y: i8,
}

fn array_mode(g: &Attrs, notes: &mut BTreeSet<Note>) -> Array {
    let mut a = Array {
        column_major: false,
        user_order: false,
        count: None,
        align_x: 0,
        align_y: 0,
    };
    let Some(mode) = g.get("packmode").map(|s| s.trim().to_ascii_lowercase()) else {
        return a;
    };
    if mode.is_empty() {
        return a;
    }
    let Some(rest) = mode.strip_prefix("array") else {
        // node・clust・graph の詰め方は配列で
        notes.insert(Note::LayoutOptions);
        return a;
    };
    let rest = rest.trim_start_matches('_');
    let digits: String = rest.chars().filter(char::is_ascii_digit).collect();
    a.count = digits
        .parse::<usize>()
        .ok()
        .filter(|&n| n > 0)
        .map(|n| n.min(1000));
    for ch in rest.chars() {
        match ch {
            'c' => a.column_major = true,
            'u' => a.user_order = true,
            't' => a.align_y = -1,
            'b' => a.align_y = 1,
            'l' => a.align_x = -1,
            'r' => a.align_x = 1,
            _ => {}
        }
    }
    a
}

pub fn osage(
    items: &[Item],
    groups: &[Group],
    g: &Attrs,
    notes: &mut BTreeSet<Note>,
) -> (Vec<P>, Vec<Option<[f64; 4]>>) {
    let mode = array_mode(g, notes);
    let gap = g
        .num("pack")
        .filter(|v| *v >= 0.0)
        .unwrap_or(8.0)
        .min(500.0);
    let live = live_groups(items, groups);
    let mut pos = vec![(0.0, 0.0); items.len()];
    let mut boxes: Vec<Option<[f64; 4]>> = vec![None; groups.len()];
    // 内から外へ: 深いクラスタから大きさを決め、最後に外から位置を足す
    let mut order: Vec<usize> = (0..groups.len()).filter(|&c| live[c]).collect();
    order.sort_by_key(|&c| std::cmp::Reverse(super::common::depth(groups, c)));
    let mut size: Vec<P> = vec![(0.0, 0.0); groups.len()];
    // 中身の相対の位置（箱の中心が原点）
    let mut rel: Vec<Vec<(Unit, P)>> = vec![Vec::new(); groups.len()];
    let unit_size = |u: Unit, size: &[P]| match u {
        Unit::Node(v) => (items[v].w, items[v].h),
        Unit::Cluster(c) => size[c],
    };
    for &c in &order {
        let us = units(items, groups, Some(c), &live);
        let sizes: Vec<P> = us.iter().map(|&u| unit_size(u, &size)).collect();
        let sortv: Vec<Option<f64>> = us.iter().map(|&u| sortv_of(u, items, groups)).collect();
        let (placed, w, h) = arrange(&sizes, &sortv, &mode, gap);
        let gr = &groups[c];
        let m = gr.margin;
        let (lw, lh) = gr.label;
        let bw = w.max(lw) + 2.0 * m;
        let bh = h + 2.0 * m + lh;
        size[c] = (bw, bh);
        // 中身の中心は箱の中心から題の分だけ下
        rel[c] = us
            .iter()
            .zip(placed)
            .map(|(&u, p)| (u, (p.0 - w / 2.0, p.1 - h / 2.0 + lh / 2.0)))
            .collect();
    }
    let top = units(items, groups, None, &live);
    let sizes: Vec<P> = top.iter().map(|&u| unit_size(u, &size)).collect();
    let sortv: Vec<Option<f64>> = top.iter().map(|&u| sortv_of(u, items, groups)).collect();
    let (placed, _, _) = arrange(&sizes, &sortv, &mode, gap);
    let mut stack: Vec<(Unit, P)> = top.into_iter().zip(placed).collect();
    while let Some((u, p)) = stack.pop() {
        match u {
            Unit::Node(v) => pos[v] = p,
            Unit::Cluster(c) => {
                let (w, h) = size[c];
                boxes[c] = Some([p.0 - w / 2.0, p.1 - h / 2.0, p.0 + w / 2.0, p.1 + h / 2.0]);
                for &(cu, cp) in &rel[c] {
                    stack.push((cu, (p.0 + cp.0, p.1 + cp.1)));
                }
            }
        }
    }
    (pos, boxes)
}

fn sortv_of(u: Unit, items: &[Item], groups: &[Group]) -> Option<f64> {
    match u {
        Unit::Node(v) => items[v].sortv,
        Unit::Cluster(c) => groups[c].sortv,
    }
}

/// 箱を配列に並べる。中心の位置（左上が原点）と、全体の幅・高さ。
fn arrange(sizes: &[P], sortv: &[Option<f64>], mode: &Array, gap: f64) -> (Vec<P>, f64, f64) {
    let n = sizes.len();
    if n == 0 {
        return (vec![], 0.0, 0.0);
    }
    let mut order: Vec<usize> = (0..n).collect();
    if mode.user_order {
        order.sort_by(|&a, &b| {
            sortv[a]
                .unwrap_or(f64::MAX)
                .total_cmp(&sortv[b].unwrap_or(f64::MAX))
                .then(a.cmp(&b))
        });
    }
    let side = (n as f64).sqrt().ceil() as usize;
    let (cols, rows) = if mode.column_major {
        let rows = mode.count.unwrap_or(side).clamp(1, n);
        (n.div_ceil(rows), rows)
    } else {
        let cols = mode.count.unwrap_or(side).clamp(1, n);
        (cols, n.div_ceil(cols))
    };
    let cell = |k: usize| -> (usize, usize) {
        if mode.column_major {
            (k / rows, k % rows)
        } else {
            (k % cols, k / cols)
        }
    };
    let mut col_w = vec![0.0f64; cols];
    let mut row_h = vec![0.0f64; rows];
    for (k, &i) in order.iter().enumerate() {
        let (c, r) = cell(k);
        col_w[c] = col_w[c].max(sizes[i].0);
        row_h[r] = row_h[r].max(sizes[i].1);
    }
    let mut col_x = vec![0.0; cols];
    for c in 1..cols {
        col_x[c] = col_x[c - 1] + col_w[c - 1] + gap;
    }
    let mut row_y = vec![0.0; rows];
    for r in 1..rows {
        row_y[r] = row_y[r - 1] + row_h[r - 1] + gap;
    }
    let mut out = vec![(0.0, 0.0); n];
    for (k, &i) in order.iter().enumerate() {
        let (c, r) = cell(k);
        let (w, h) = sizes[i];
        let fx = match mode.align_x {
            -1 => w / 2.0,
            1 => col_w[c] - w / 2.0,
            _ => col_w[c] / 2.0,
        };
        let fy = match mode.align_y {
            -1 => h / 2.0,
            1 => row_h[r] - h / 2.0,
            _ => row_h[r] / 2.0,
        };
        out[i] = (col_x[c] + fx, row_y[r] + fy);
    }
    let w = col_x[cols - 1] + col_w[cols - 1];
    let h = row_y[rows - 1] + row_h[rows - 1];
    (out, w, h)
}

// ---- patchwork ----

/// ノードの中心・クラスタの箱・ノードの升の大きさ。
type Patches = (Vec<P>, Vec<Option<[f64; 4]>>, Vec<(f64, f64)>);

/// 敷き詰め。ノードの中心・クラスタの箱・ノードの升の大きさ。
pub fn patchwork(items: &[Item], groups: &[Group]) -> Patches {
    let n = items.len();
    let live = live_groups(items, groups);
    // 面積（内から）
    let mut area = vec![0.0f64; groups.len()];
    let node_area: Vec<f64> = items
        .iter()
        .map(|it| {
            if it.area.is_finite() && it.area > 0.0 {
                it.area
            } else {
                1.0
            }
        })
        .collect();
    for (v, it) in items.iter().enumerate() {
        let mut c = it.cluster.filter(|&c| c < groups.len());
        let mut steps = 0;
        while let Some(x) = c {
            area[x] += node_area[v];
            c = groups[x].parent;
            steps += 1;
            if steps > groups.len() {
                break;
            }
        }
    }
    let mut pos = vec![(0.0, 0.0); n];
    let mut sizes = vec![(0.0, 0.0); n];
    let mut boxes: Vec<Option<[f64; 4]>> = vec![None; groups.len()];
    let total: f64 = node_area.iter().sum();
    let side = total.sqrt();
    let mut stack: Vec<(Option<usize>, [f64; 4])> = vec![(None, [0.0, 0.0, side, side])];
    while let Some((level, rect)) = stack.pop() {
        let us = units(items, groups, level, &live);
        let mut weighted: Vec<(Unit, f64)> = us
            .into_iter()
            .map(|u| {
                let a = match u {
                    Unit::Node(v) => node_area[v],
                    Unit::Cluster(c) => area[c],
                };
                (u, a)
            })
            .filter(|(_, a)| *a > 0.0)
            .collect();
        weighted.sort_by(|a, b| b.1.total_cmp(&a.1));
        let rects = squarify(&weighted.iter().map(|w| w.1).collect::<Vec<_>>(), rect);
        for ((u, _), r) in weighted.into_iter().zip(rects) {
            match u {
                Unit::Node(v) => {
                    pos[v] = ((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0);
                    sizes[v] = (r[2] - r[0], r[3] - r[1]);
                }
                Unit::Cluster(c) => {
                    boxes[c] = Some(r);
                    stack.push((Some(c), r));
                }
            }
        }
    }
    (pos, boxes, sizes)
}

/// 面積の大きい順の `areas` を `rect` に敷く（並べる行の最も悪い縦横比が悪くなるまで行に足す）。
fn squarify(areas: &[f64], rect: [f64; 4]) -> Vec<[f64; 4]> {
    let mut out = Vec::with_capacity(areas.len());
    let total: f64 = areas.iter().sum();
    let (mut x0, mut y0, x1, y1) = (rect[0], rect[1], rect[2], rect[3]);
    let full = (x1 - x0) * (y1 - y0);
    if total <= 0.0 || full <= 0.0 {
        return areas.iter().map(|_| rect).collect();
    }
    let scale = full / total;
    let a: Vec<f64> = areas.iter().map(|v| v * scale).collect();
    let worst = |row: &[f64], w: f64| -> f64 {
        let s: f64 = row.iter().sum();
        let (mx, mn) = row
            .iter()
            .fold((f64::MIN, f64::MAX), |(a, b), &v| (a.max(v), b.min(v)));
        let (w2, s2) = (w * w, s * s);
        (w2 * mx / s2).max(s2 / (w2 * mn))
    };
    let mut i = 0;
    while i < a.len() {
        let (w, h) = (x1 - x0, y1 - y0);
        let short = w.min(h).max(1e-9);
        let mut j = i + 1;
        while j < a.len() && worst(&a[i..=j], short) <= worst(&a[i..j], short) {
            j += 1;
        }
        let row = &a[i..j];
        let s: f64 = row.iter().sum();
        let thick = s / short;
        let mut at = 0.0;
        for &v in row {
            let len = v / thick;
            if w >= h {
                // 縦の列を左に
                out.push([x0, y0 + at, x0 + thick, y0 + at + len]);
            } else {
                out.push([x0 + at, y0, x0 + at + len, y0 + thick]);
            }
            at += len;
        }
        if w >= h {
            x0 += thick;
        } else {
            y0 += thick;
        }
        i = j;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(cluster: Option<usize>, area: f64) -> Item {
        Item {
            w: 54.0,
            h: 36.0,
            cluster,
            pos: None,
            pinned: false,
            area,
            root: false,
            sortv: None,
        }
    }

    fn group(parent: Option<usize>) -> Group {
        Group {
            parent,
            margin: 8.0,
            label: (0.0, 0.0),
            sortv: None,
        }
    }

    #[test]
    fn treemap_keeps_areas() {
        let items = vec![
            item(Some(0), 6.0),
            item(Some(0), 2.0),
            item(None, 4.0),
            item(Some(1), 4.0),
        ];
        let groups = vec![group(None), group(Some(0))];
        let (pos, boxes, sizes) = patchwork(&items, &groups);
        for (v, it) in items.iter().enumerate() {
            let a = sizes[v].0 * sizes[v].1;
            assert!((a - it.area).abs() < 1e-6, "{v}: {a}");
            let b = boxes[0].unwrap();
            if it.cluster.is_some() {
                assert!(pos[v].0 > b[0] && pos[v].0 < b[2]);
            }
        }
        let b = boxes[0].unwrap();
        assert!(((b[2] - b[0]) * (b[3] - b[1]) - 12.0).abs() < 1e-6);
    }

    #[test]
    fn osage_packs_clusters_in_an_array() {
        let items = vec![item(Some(0), 1.0), item(Some(0), 1.0), item(None, 1.0)];
        let groups = vec![group(None)];
        let (pos, boxes) = osage(&items, &groups, &Attrs::default(), &mut BTreeSet::new());
        let b = boxes[0].unwrap();
        for p in &pos[..2] {
            assert!(p.0 - 27.0 >= b[0] && p.0 + 27.0 <= b[2]);
        }
        assert!(pos[2].0 - 27.0 >= b[2] || pos[2].1 - 18.0 >= b[3]);
    }
}
