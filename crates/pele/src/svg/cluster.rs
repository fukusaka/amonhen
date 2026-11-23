//! まとまり（cluster）の題の置き場所（pele の `src/svg/cluster.ts`）。

use std::collections::HashMap;

use crate::js;
use crate::layout::compound::CNode;
use crate::svg::builder::{label_svg, num};
use crate::text::label::Label;
use crate::types::IconResolver;

/// 経路の点の x を、丸めた y ごとに: まとまりの上の縁を横切る辺。
pub type Crossings = HashMap<i64, Vec<f64>>;

fn key(y: f64) -> i64 {
    js::round(y) as i64
}

pub fn mark_crossings(crossings: &mut Crossings, route: &[f64]) {
    let mut k = 0;
    while k + 1 < route.len() {
        crossings
            .entry(key(route[k + 1]))
            .or_default()
            .push(route[k]);
        k += 3;
    }
}

/// まとまりの題を上の縁のどこに置くか。そこに入る辺が題を横切らないように。
pub fn cluster_title_x(c: &CNode, width: f64, crossings: &Crossings, pad: f64) -> f64 {
    if width == 0.0 {
        return c.x;
    }
    let top = c.y - c.h / 2.0;
    let left = c.x - c.w / 2.0;
    let half = width / 2.0;
    let candidates = [left + pad - 6.0 + half, c.x, left + c.w - pad + 6.0 - half];
    let Some(xs) = crossings.get(&key(top)) else {
        return candidates[0];
    };
    let mut best = candidates[0];
    let mut best_gap = -1.0;
    for x in candidates {
        let mut gap = f64::INFINITY;
        for &cx in xs {
            if cx > left && cx < left + c.w {
                gap = js::min(gap, (cx - x).abs() - half);
            }
        }
        if gap >= 6.0 {
            return x;
        }
        if gap > best_gap + 0.5 {
            best_gap = gap;
            best = x;
        }
    }
    best
}

/// 上から入る辺が、`x` に置いた題を通るか。
pub fn title_crossed(c: &CNode, width: f64, x: f64, crossings: &Crossings) -> bool {
    if width <= 0.0 {
        return false;
    }
    let Some(xs) = crossings.get(&key(c.y - c.h / 2.0)) else {
        return false;
    };
    let left = c.x - c.w / 2.0;
    xs.iter()
        .any(|&cx| cx > left && cx < left + c.w && (cx - x).abs() < width / 2.0 + 4.0)
}

/// 辺が通る題を辺の上に描く: 後ろのまとまりの色の縁取りの上に文字を置き、線が文字の手前で止まって見えるように。
/// `nodes` と `index` で題がまとまりの何段目かを数える（段ごとに背景が濃くなる）。
#[allow(clippy::too_many_arguments)]
pub fn struck_title(
    label: &Label,
    x: f64,
    y: f64,
    attrs: &str,
    id: &str,
    nodes: &[CNode],
    index: usize,
    icons: Option<&IconResolver>,
) -> String {
    let mut depth = 1;
    let mut p = nodes[index].parent;
    while p >= 0 && depth < 8 {
        depth += 1;
        p = nodes[p as usize].parent;
    }
    // まとまりはどれも、後ろのものの上に半分の塗り。
    let fill = format!(
        "color-mix(in srgb,var(--_a) {}%,var(--_bg))",
        num(100.0 * (1.0 - 0.5f64.powi(depth)))
    );
    format!(
        "<g class=\"pele-cluster-title\" data-id=\"{id}\">{}{}</g>",
        label_svg(
            label,
            x,
            y,
            &format!(
                " fill=\"none\" stroke=\"{fill}\" stroke-width=\"5\" stroke-linejoin=\"round\""
            ),
            icons
        ),
        label_svg(label, x, y, attrs, icons)
    )
}
