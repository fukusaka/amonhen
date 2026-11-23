//! block を描く（pele の `src/diagrams/block/render.ts`）。

use std::collections::{HashMap, HashSet};
use std::f64::consts::{FRAC_1_SQRT_2, PI};

use super::arrows::{OVERHANG, arrow_axis, arrow_mask, arrow_size, draw_arrow};
use super::db::BlockDb;
use super::layout::{Cell, cell, layout};
use super::types::Block;
use crate::diagrams::common::fit_width::{tighten, title_room};
use crate::errors::Result;
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc_text, label_svg, num};
use crate::svg::edges::{
    ARROW, EdgePath, Fill, casings_svg, edge_casing, edge_label_svg, marker, marker_trim,
    over_fills,
};
use crate::svg::ink::{ink_fill, paints};
use crate::svg::root::{Accessible, svg_document};
use crate::svg::shapes::{draw_shape, shape_filled, shape_inset, shape_size};
use crate::svg::theme::{ResolvedStyle, class_names, resolve_style, rounded, sets_fill};
use crate::svg::title::title_label;
use crate::text::label::{Label, layout_label};
use crate::text::measurer::{TextMeasurer, default_measurer, style};
use crate::types::{RenderOptions, Rendered};
use crate::util::yaml::Value;

const GAP: f64 = 12.0;
const WRAP: f64 = 200.0;
const CORNER: f64 = 8.0;
const SHAPE_ATTRS: &str = " fill=\"var(--_s)\" stroke=\"var(--_b)\"";
const LINE_ATTRS: &str = " stroke=\"var(--_b)\"";
/// 辺をすべてのブロックと比べると 2 乗で増えるので、大きな図は比べずに引く。
const OBSTACLE_BUDGET: usize = 2_000_000;
const LABEL_GAP: f64 = 4.0;

fn shape_of(type_: &str) -> &'static str {
    match type_ {
        "square" => "rect",
        "round" => "rounded",
        "circle" => "circle",
        "doublecircle" => "dbl-circ",
        "diamond" => "diam",
        "hexagon" => "hex",
        "stadium" => "stadium",
        "subroutine" => "fr-rect",
        "cylinder" => "cyl",
        "rect_left_inv_arrow" => "odd",
        "lean_right" => "lean-r",
        "lean_left" => "lean-l",
        "trapezoid" => "trap-b",
        "inv_trapezoid" => "trap-t",
        _ => "rect",
    }
}

/// 大きなマスでも自分の大きさを保つ形。ほかはマスいっぱいに伸びる。
fn fixed(shape: &str) -> bool {
    matches!(shape, "circle" | "dbl-circ" | "diam")
}

/// 斜めや丸みが高さで決まるので、横にだけ伸びる形。
fn wide(shape: &str) -> bool {
    matches!(
        shape,
        "stadium" | "hex" | "odd" | "lean-r" | "lean-l" | "trap-b" | "trap-t"
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Leaf,
    Arrow,
    Composite,
    Space,
}

struct View {
    block: usize,
    kind: Kind,
    shape: &'static str,
    label: Label,
    style: ResolvedStyle,
    mask: u32,
    dx: f64,
    dy: f64,
    /// 描く大きさ。決まった形ではマスより小さい。
    bw: f64,
    bh: f64,
}

/// 外形の計算に要るもの。
#[derive(Clone, Copy)]
struct G {
    i: usize,
    x: f64,
    y: f64,
    bw: f64,
    bh: f64,
    kind: Kind,
    shape: &'static str,
    mask: u32,
}

/// ラベルが避ける箱。`of` はブロックならその位置（題は None）。
#[derive(Clone, Copy)]
struct Obs {
    of: Option<usize>,
    x: f64,
    y: f64,
    bw: f64,
    bh: f64,
}

struct Route {
    edge: usize,
    /// まっすぐなら 2 点、曲がるなら 3 点、自分への輪は空。
    points: Vec<f64>,
    loop_: usize,
    label: Label,
    lx: f64,
    ly: f64,
}

fn is_round(v: &G) -> bool {
    v.kind == Kind::Leaf && (v.shape == "circle" || v.shape == "dbl-circ")
}

/// x の縦の線が外形に当たる所。中心の下（sign 1）か上（sign -1）。
fn edge_y(v: &G, x: f64, sign: f64) -> f64 {
    let hw = v.bw / 2.0;
    let hh = v.bh / 2.0;
    let dx = (x - v.x).abs();
    let mut reach = hh;
    if is_round(v) {
        reach = js::max(hw * hw - dx * dx, 0.0).sqrt();
    } else if v.kind == Kind::Leaf && v.shape == "diam" {
        reach = hh * js::max(1.0 - dx / hw, 0.0);
    } else if v.kind == Kind::Leaf {
        reach = hh - shape_inset(v.shape, v.bw, v.bh, if sign > 0.0 { 2 } else { 0 }, 0.0);
    } else if v.kind == Kind::Arrow && arrow_axis(v.mask) == 'x' {
        reach = hh - OVERHANG;
    }
    v.y + sign * reach
}

/// y の横の線が外形に当たる所。中心の右（sign 1）か左（sign -1）。
fn edge_x(v: &G, y: f64, sign: f64) -> f64 {
    let hw = v.bw / 2.0;
    let hh = v.bh / 2.0;
    let dy = (y - v.y).abs();
    let mut reach = hw;
    if is_round(v) {
        reach = js::max(hh * hh - dy * dy, 0.0).sqrt();
    } else if v.kind == Kind::Leaf && v.shape == "diam" {
        reach = hw * js::max(1.0 - dy / hh, 0.0);
    } else if v.kind == Kind::Leaf {
        reach = hw - shape_inset(v.shape, v.bw, v.bh, if sign > 0.0 { 1 } else { 3 }, 0.0);
    } else if v.kind == Kind::Arrow && arrow_axis(v.mask) == 'y' {
        reach = hw - OVERHANG;
    }
    v.x + sign * reach
}

fn or1(x: f64) -> f64 {
    if js::truthy(x) { x } else { 1.0 }
}

/// 中心から (tx, ty) への線が外形を出る所。
fn border(v: &G, tx: f64, ty: f64, out: &mut Vec<f64>) {
    let dx = tx - v.x;
    let dy = ty - v.y;
    let hw = v.bw / 2.0;
    let hh = v.bh / 2.0;
    let mut k = if is_round(v) {
        hw / or1(js::hypot(dx, dy))
    } else if v.kind == Kind::Leaf && v.shape == "diam" {
        1.0 / or1(dx.abs() / hw + dy.abs() / hh)
    } else {
        js::min(
            if dx == 0.0 {
                f64::INFINITY
            } else {
                hw / dx.abs()
            },
            if dy == 0.0 {
                f64::INFINITY
            } else {
                hh / dy.abs()
            },
        )
    };
    if k == f64::INFINITY {
        k = 0.0;
    }
    out.push(v.x + dx * k);
    out.push(v.y + dy * k);
}

fn crosses_box(x1: f64, y1: f64, x2: f64, y2: f64, v: &G) -> bool {
    let l = v.x - v.bw / 2.0 + 1.0;
    let r = v.x + v.bw / 2.0 - 1.0;
    let t = v.y - v.bh / 2.0 + 1.0;
    let b = v.y + v.bh / 2.0 - 1.0;
    if (x1 < l && x2 < l) || (x1 > r && x2 > r) || (y1 < t && y2 < t) || (y1 > b && y2 > b) {
        return false;
    }
    if x1 == x2 || y1 == y2 {
        return true;
    }
    // 斜めの線で、囲む箱が重なる: 4 つの角が線の同じ側になければ当たる。
    let dx = x2 - x1;
    let dy = y2 - y1;
    let side = |x: f64, y: f64| js::sign(dx * (y - y1) - dy * (x - x1));
    let first = side(l, t);
    first == 0.0 || side(r, t) != first || side(r, b) != first || side(l, b) != first
}

fn hits(points: &[f64], s: usize, t: usize, obstacles: &[G]) -> usize {
    let mut count = 0;
    for v in obstacles {
        if v.i == s || v.i == t {
            continue;
        }
        let mut i = 0;
        while i + 3 < points.len() {
            if crosses_box(points[i], points[i + 1], points[i + 2], points[i + 3], v) {
                count += 1;
                break;
            }
            i += 2;
        }
    }
    count
}

/// 2 つのブロックが行か列を共にすればまっすぐな線、でなければ向き合う辺の間を 1 度曲がる線。
fn route_edge(s: &G, t: &G, shift: f64, obstacles: Option<&[G]>) -> Vec<f64> {
    let sl = s.x - s.bw / 2.0;
    let sr = s.x + s.bw / 2.0;
    let st = s.y - s.bh / 2.0;
    let sb = s.y + s.bh / 2.0;
    let tl = t.x - t.bw / 2.0;
    let tr = t.x + t.bw / 2.0;
    let tt = t.y - t.bh / 2.0;
    let tb = t.y + t.bh / 2.0;
    let x1 = js::max(sl, tl);
    let x2 = js::min(sr, tr);
    let y1 = js::max(st, tt);
    let y2 = js::min(sb, tb);
    let over_x = x2 - x1 > 6.0;
    let over_y = y2 - y1 > 6.0;

    if over_x && over_y {
        // 片方がもう片方の中にある: 内のものを外のもののいちばん近い辺へつなぐ。
        let inner_is_s = s.bw * s.bh <= t.bw * t.bh;
        let (inner, outer) = if inner_is_s { (s, t) } else { (t, s) };
        let up = inner.y - inner.bh / 2.0 - (outer.y - outer.bh / 2.0);
        let down = outer.y + outer.bh / 2.0 - (inner.y + inner.bh / 2.0);
        let left = inner.x - inner.bw / 2.0 - (outer.x - outer.bw / 2.0);
        let right = outer.x + outer.bw / 2.0 - (inner.x + inner.bw / 2.0);
        let least = js::min_of(&[up, down, left, right]);
        let points = if least == down {
            vec![
                inner.x,
                edge_y(inner, inner.x, 1.0),
                inner.x,
                outer.y + outer.bh / 2.0,
            ]
        } else if least == up {
            vec![
                inner.x,
                edge_y(inner, inner.x, -1.0),
                inner.x,
                outer.y - outer.bh / 2.0,
            ]
        } else if least == right {
            vec![
                edge_x(inner, inner.y, 1.0),
                inner.y,
                outer.x + outer.bw / 2.0,
                inner.y,
            ]
        } else {
            vec![
                edge_x(inner, inner.y, -1.0),
                inner.y,
                outer.x - outer.bw / 2.0,
                inner.y,
            ]
        };
        return if inner_is_s {
            points
        } else {
            vec![points[2], points[3], points[0], points[1]]
        };
    }
    if over_x {
        let margin = js::min(6.0, (x2 - x1) / 2.0);
        let x = js::min(js::max((x1 + x2) / 2.0 + shift, x1 + margin), x2 - margin);
        let down = if t.y > s.y { 1.0 } else { -1.0 };
        return vec![x, edge_y(s, x, down), x, edge_y(t, x, -down)];
    }
    if over_y {
        let margin = js::min(6.0, (y2 - y1) / 2.0);
        let y = js::min(js::max((y1 + y2) / 2.0 + shift, y1 + margin), y2 - margin);
        let right = if t.x > s.x { 1.0 } else { -1.0 };
        return vec![edge_x(s, y, right), y, edge_x(t, y, -right), y];
    }

    let right = if t.x > s.x { 1.0 } else { -1.0 };
    let down = if t.y > s.y { 1.0 } else { -1.0 };
    let across = vec![
        edge_x(s, s.y, right),
        s.y,
        t.x,
        s.y,
        t.x,
        edge_y(t, t.x, -down),
    ];
    let along = vec![
        s.x,
        edge_y(s, s.x, down),
        s.x,
        t.y,
        edge_x(t, t.y, -right),
        t.y,
    ];
    let wide = (t.x - s.x).abs() >= (t.y - s.y).abs();
    let (mut best, other) = if wide {
        (across, along)
    } else {
        (along, across)
    };
    let Some(obstacles) = obstacles else {
        return best;
    };
    let mut least = hits(&best, s.i, t.i, obstacles);
    if least == 0 {
        return best;
    }
    let count = hits(&other, s.i, t.i, obstacles);
    if count < least {
        best = other;
        least = count;
    }
    if least == 0 {
        return best;
    }
    let mut direct = Vec::new();
    border(s, t.x, t.y, &mut direct);
    border(t, s.x, s.y, &mut direct);
    if hits(&direct, s.i, t.i, obstacles) < least {
        direct
    } else {
        best
    }
}

/// 2 点か 3 点を通る経路。曲がり角を丸め、矢じりの場所を空ける。
fn trace(p: &[f64], mut start_trim: f64, mut end_trim: f64) -> EdgePath {
    let n = p.len();
    let sx = p[0];
    let sy = p[1];
    let ex = p[n - 2];
    let ey = p[n - 1];
    let mut ax = p[2] - sx;
    let mut ay = p[3] - sy;
    let first = js::hypot(ax, ay);
    ax /= or1(first);
    ay /= or1(first);
    let mut bx = ex - p[n - 4];
    let mut by = ey - p[n - 3];
    let last = js::hypot(bx, by);
    bx /= or1(last);
    by /= or1(last);
    if n == 4 && start_trim + end_trim > first {
        let scale = first / (start_trim + end_trim);
        start_trim *= scale;
        end_trim *= scale;
    }
    start_trim = js::min(start_trim, first);
    end_trim = js::min(end_trim, last);
    let mut d = format!(
        "M{},{}",
        num(sx + ax * start_trim),
        num(sy + ay * start_trim)
    );
    if n == 6 {
        let r = js::max(
            0.0,
            js::min_of(&[CORNER, first - start_trim, last - end_trim]),
        );
        d.push_str(&format!(
            "L{},{}Q{},{} {},{}",
            num(p[2] - ax * r),
            num(p[3] - ay * r),
            num(p[2]),
            num(p[3]),
            num(p[2] + bx * r),
            num(p[3] + by * r)
        ));
    }
    d.push_str(&format!(
        "L{},{}",
        num(ex - bx * end_trim),
        num(ey - by * end_trim)
    ));
    EdgePath {
        d,
        tokens: None,
        sx,
        sy,
        sdx: -ax,
        sdy: -ay,
        ex,
        ey,
        edx: bx,
        edy: by,
    }
}

/// 自分への輪は、ブロックの右上の角（丸や菱形では外形のそれに当たる点）を回る弧。弧の中心と半径、出る角度と
/// 戻る角度。
fn loop_arc(v: &G, k: usize) -> (f64, f64, f64, f64, f64) {
    let hw = v.bw / 2.0;
    let hh = v.bh / 2.0;
    let r = js::max(1.0, js::min_of(&[10.0 + 5.0 * (k as f64 - 1.0), hw, hh]));
    if is_round(v) {
        let px = v.x + hw * FRAC_1_SQRT_2;
        let py = v.y - hw * FRAC_1_SQRT_2;
        let half = 2.0 * js::min(1.0, r / (2.0 * hw)).asin();
        let angle = |at: f64| (v.y - hw * at.sin() - py).atan2(v.x + hw * at.cos() - px);
        return (px, py, r, angle(PI / 4.0 + half), angle(PI / 4.0 - half));
    }
    if v.kind == Kind::Leaf && v.shape == "diam" {
        let slope = hh.atan2(hw);
        return (v.x + hw / 2.0, v.y - hh / 2.0, r, slope + PI, slope);
    }
    (v.x + hw, v.y - hh, r, PI, PI / 2.0)
}

fn loop_path(v: &G, k: usize, start_trim: f64, end_trim: f64) -> EdgePath {
    let (px, py, r, a1, a2) = loop_arc(v, k);
    let from = a1 + start_trim / r;
    let to = a2 - end_trim / r;
    let mut sweep = (to - from) % (2.0 * PI);
    if sweep <= 0.0 {
        sweep += 2.0 * PI;
    }
    let x1 = px + r * from.cos();
    let y1 = py + r * from.sin();
    let x2 = px + r * to.cos();
    let y2 = py + r * to.sin();
    let sx = px + r * a1.cos();
    let sy = py + r * a1.sin();
    let ex = px + r * a2.cos();
    let ey = py + r * a2.sin();
    // 矢じりはそれが覆う弦に沿わせる。先の接線ではなく曲線に沿うように。
    let sl = js::hypot(sx - x1, sy - y1);
    let el = js::hypot(ex - x2, ey - y2);
    EdgePath {
        d: format!(
            "M{},{}A{},{} 0 {} 1 {},{}",
            num(x1),
            num(y1),
            num(r),
            num(r),
            if sweep > PI { 1 } else { 0 },
            num(x2),
            num(y2)
        ),
        tokens: None,
        sx,
        sy,
        sdx: if sl > 0.0 { (sx - x1) / sl } else { a1.sin() },
        sdy: if sl > 0.0 { (sy - y1) / sl } else { -a1.cos() },
        ex,
        ey,
        edx: if el > 0.0 { (ex - x2) / el } else { -a2.sin() },
        edy: if el > 0.0 { (ey - y2) / el } else { a2.cos() },
    }
}

const SPOTS: [f64; 5] = [0.5, 0.3, 0.7, 0.15, 0.85];

#[allow(clippy::too_many_arguments)]
fn covers(x: f64, y: f64, w: f64, h: f64, s: usize, t: usize, obstacles: Option<&[Obs]>) -> bool {
    let Some(obstacles) = obstacles else {
        return false;
    };
    obstacles.iter().any(|v| {
        v.of != Some(s)
            && v.of != Some(t)
            && (x - v.x).abs() < (w + v.bw) / 2.0
            && (y - v.y).abs() < (h + v.bh) / 2.0
    })
}

/// 脚の上で、その大きさのラベルがつなぐ 2 つのほかのブロックを覆わない所。
fn label_spot(
    p: &[f64],
    at: usize,
    w: f64,
    h: f64,
    s: usize,
    t: usize,
    obstacles: Option<&[Obs]>,
) -> f64 {
    if obstacles.is_none() {
        return 0.5;
    }
    for spot in SPOTS {
        let x = p[at] + (p[at + 2] - p[at]) * spot;
        let y = p[at + 1] + (p[at + 3] - p[at + 1]) * spot;
        if !covers(x, y, w, h, s, t, obstacles) {
            return spot;
        }
    }
    0.5
}

/// 重なるラベルを、共にする線に沿って積む（矢じりの間に収まり、ほかのブロックや題を覆わなければ）。
/// ラベルが収まるのに足りない長さを返す。
fn stack_labels(
    stack: &[usize],
    routes: &mut [Route],
    legs: &[Vec<f64>],
    s: usize,
    t: usize,
    obstacles: Option<&[Obs]>,
) -> f64 {
    let vertical = legs[stack[0]][0] == legs[stack[0]][2];
    let mut lo = f64::NEG_INFINITY;
    let mut hi = f64::INFINITY;
    let mut need = -LABEL_GAP;
    let mut crowded = false;
    for a in 0..stack.len() {
        let leg = &legs[stack[a]];
        if if vertical {
            leg[0] != leg[2]
        } else {
            leg[1] != leg[3]
        } {
            return 0.0;
        }
        lo = js::max(
            lo,
            if vertical {
                js::min(leg[1], leg[3])
            } else {
                js::min(leg[0], leg[2])
            },
        );
        hi = js::min(
            hi,
            if vertical {
                js::max(leg[1], leg[3])
            } else {
                js::max(leg[0], leg[2])
            },
        );
        let p = &routes[stack[a]];
        need += (if vertical {
            p.label.height
        } else {
            p.label.width + 8.0
        }) + LABEL_GAP;
        for &b in &stack[..a] {
            let q = &routes[b];
            if (p.lx - q.lx).abs() < (p.label.width + q.label.width) / 2.0 + 8.0
                && (p.ly - q.ly).abs() < (p.label.height + q.label.height) / 2.0
            {
                crowded = true;
            }
        }
    }
    if !crowded {
        return 0.0;
    }
    if need > hi - lo - 2.0 * ARROW {
        return need - (hi - lo - 2.0 * ARROW);
    }
    let mut along = Vec::new();
    let mut next = (lo + hi - need) / 2.0;
    for &i in stack {
        let r = &routes[i];
        let size = if vertical {
            r.label.height
        } else {
            r.label.width + 8.0
        };
        let at = next + size / 2.0;
        if covers(
            if vertical { r.lx } else { at },
            if vertical { at } else { r.ly },
            r.label.width + 8.0,
            r.label.height,
            s,
            t,
            obstacles,
        ) {
            return 0.0;
        }
        along.push(at);
        next += size + LABEL_GAP;
    }
    for (k, &i) in stack.iter().enumerate() {
        if vertical {
            routes[i].ly = along[k];
        } else {
            routes[i].lx = along[k];
        }
    }
    0.0
}

/// 2 つのブロックのいちばん近い共通の入れ物で、その間の隙間を広げる。`base` はラベルを測ったときの隙間を保つ。
fn widen(
    parents: &[Option<usize>],
    cells: &mut [Cell],
    s: usize,
    t: usize,
    vertical: bool,
    short: f64,
    base: &mut HashMap<(usize, bool), Vec<f64>>,
) -> bool {
    let mut chain = HashSet::new();
    let mut v = Some(s);
    while let Some(i) = v {
        chain.insert(i);
        v = parents[i];
    }
    let mut tb: Option<usize> = None;
    let mut bx: Option<usize> = Some(t);
    while let Some(b) = bx {
        if chain.contains(&b) {
            break;
        }
        tb = Some(b);
        bx = parents[b];
    }
    let (Some(bx), Some(tb)) = (bx, tb) else {
        return false;
    };
    if bx == s {
        return false;
    }
    let mut sb = s;
    while parents[sb] != Some(bx) {
        sb = parents[sb].unwrap();
    }
    let at = if vertical {
        if cells[sb].row == cells[tb].row {
            return false;
        }
        js::max(cells[sb].row, cells[tb].row) - 1.0
    } else {
        let (l, r) = if cells[sb].col < cells[tb].col {
            (sb, tb)
        } else {
            (tb, sb)
        };
        if cells[r].col < cells[l].col + cells[l].span {
            return false;
        }
        cells[r].col - 1.0
    } as usize;
    let gaps = if vertical {
        &mut cells[bx].row_gaps
    } else {
        &mut cells[bx].col_gaps
    };
    let was = base.entry((bx, vertical)).or_insert_with(|| gaps.clone());
    let from = was.get(at).copied().unwrap_or(0.0);
    if gaps.len() <= at {
        gaps.resize(at + 1, 0.0);
    }
    gaps[at] = js::max(gaps[at], from + short);
    true
}

pub fn render_block(db: &BlockDb, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    tighten(options, |tight| draw(db, config, options, tight))
}

struct Scene<'a> {
    db: &'a BlockDb,
    measurer: &'a dyn TextMeasurer,
    gap: f64,
    tight: bool,
    edge_size: f64,
    views: Vec<View>,
    cells: Vec<Cell>,
    parents: Vec<Option<usize>>,
    widths: Vec<f64>,
    obstacles: Vec<usize>,
    titles: Vec<Obs>,
    content: (f64, f64),
    ends: Vec<(usize, usize)>,
    drawn: Vec<usize>,
    pairs: HashMap<usize, usize>,
    labelled: HashSet<usize>,
    check: bool,
    routes: Vec<Route>,
    stacks: Vec<(usize, Vec<usize>)>,
    legs: Vec<Vec<f64>>,
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl Scene<'_> {
    fn g(&self, i: usize) -> G {
        let v = &self.views[i];
        let c = &self.cells[i];
        G {
            i,
            x: c.x,
            y: c.y,
            bw: v.bw,
            bh: v.bh,
            kind: v.kind,
            shape: v.shape,
            mask: v.mask,
        }
    }

    fn key(&self, s: usize, t: usize) -> usize {
        s.min(t) * self.views.len() + s.max(t)
    }

    fn place(&mut self) -> Result<()> {
        for (c, &w) in self.cells.iter_mut().zip(&self.widths) {
            c.nw = w;
        }
        self.content = layout(&mut self.cells, self.gap)?;
        self.obstacles.clear();
        self.titles.clear();
        for i in 0..self.views.len() {
            let c = &self.cells[i];
            let v = &mut self.views[i];
            if v.kind == Kind::Leaf && fixed(v.shape) {
                v.bw = c.nw;
                v.bh = c.nh;
            } else if v.kind == Kind::Arrow {
                v.bw = if c.span > 1.0 { c.w } else { c.nw };
                v.bh = c.nh;
            } else {
                v.bw = c.w;
                v.bh = if v.kind == Kind::Leaf && wide(v.shape) {
                    c.nh
                } else {
                    c.h
                };
            }
            if v.kind == Kind::Leaf && v.shape == "cyl" {
                v.dy = js::min(12.0, js::max(6.0, v.bw / 14.0)) / 2.0;
            }
            if v.kind == Kind::Leaf || v.kind == Kind::Arrow {
                self.obstacles.push(i);
            } else if c.head > 0.0 {
                self.titles.push(Obs {
                    of: None,
                    x: c.x,
                    y: c.y - c.h / 2.0 + (c.head + self.gap) / 2.0 + 2.0,
                    bw: v.label.width,
                    bh: v.label.height,
                });
            }
        }
        Ok(())
    }

    fn obstacle_geos(&self) -> Option<Vec<G>> {
        self.check
            .then(|| self.obstacles.iter().map(|&i| self.g(i)).collect())
    }

    fn obstacle_boxes(&self) -> Option<Vec<Obs>> {
        self.check.then(|| {
            self.obstacles
                .iter()
                .map(|&i| {
                    let g = self.g(i);
                    Obs {
                        of: Some(i),
                        x: g.x,
                        y: g.y,
                        bw: g.bw,
                        bh: g.bh,
                    }
                })
                .collect()
        })
    }

    fn route(&mut self) {
        let mut seen: HashMap<usize, usize> = HashMap::new();
        self.routes.clear();
        self.stacks.clear();
        let geos = self.obstacle_geos();
        let boxes = self.obstacle_boxes();
        for i in 0..self.drawn.len() {
            let (s, t) = self.ends[i];
            let key = self.key(s, t);
            let total = self.pairs.get(&key).copied().unwrap_or(1);
            let k = seen.get(&key).copied().unwrap_or(0);
            seen.insert(key, k + 1);
            let edge = &self.db.arena[self.drawn[i]];
            let label = layout_label(
                edge.label.as_deref(),
                false,
                self.measurer,
                self.edge_size,
                WRAP,
                0,
            );
            let points: Vec<f64>;
            let lx;
            let ly;
            if s == t {
                let (px, py, r, _, _) = loop_arc(&self.g(s), k + 1);
                points = Vec::new();
                lx = px + r + 6.0 + label.width / 2.0;
                ly = py - r / 2.0;
            } else {
                let step = if self.labelled.contains(&key) {
                    js::round(self.edge_size * 1.5) + 2.0
                } else {
                    14.0
                };
                points = route_edge(
                    &self.g(s),
                    &self.g(t),
                    (k as f64 - (total as f64 - 1.0) / 2.0) * step,
                    geos.as_deref(),
                );
                let n = points.len();
                // ラベルは長い方の脚に置く。
                let first_leg = (points[2] - points[0]).abs() + (points[3] - points[1]).abs();
                let last_leg =
                    (points[n - 2] - points[n - 4]).abs() + (points[n - 1] - points[n - 3]).abs();
                let at = if n == 6 && last_leg > first_leg { 2 } else { 0 };
                let spot = if label.width > 0.0 {
                    label_spot(
                        &points,
                        at,
                        label.width + 8.0,
                        label.height,
                        s,
                        t,
                        boxes.as_deref(),
                    )
                } else {
                    0.5
                };
                lx = points[at] + (points[at + 2] - points[at]) * spot;
                ly = points[at + 1] + (points[at + 3] - points[at + 1]) * spot;
                if label.width > 0.0 && total > 1 {
                    self.legs[i] = points[at..at + 4].to_vec();
                    match self.stacks.iter_mut().find(|(k, _)| *k == key) {
                        Some((_, stack)) => stack.push(i),
                        None => self.stacks.push((key, vec![i])),
                    }
                }
            }
            self.routes.push(Route {
                edge: self.drawn[i],
                points,
                loop_: if s == t { k + 1 } else { 0 },
                label,
                lx,
                ly,
            });
        }
    }

    fn settle(&mut self, grow: bool) -> bool {
        self.route();
        let blocked = self.obstacle_boxes().map(|mut b| {
            b.extend(self.titles.iter().copied());
            b
        });
        let mut base = HashMap::new();
        let mut grown = false;
        let stacks = std::mem::take(&mut self.stacks);
        for (_, stack) in &stacks {
            if stack.len() < 2 {
                continue;
            }
            let (s, t) = self.ends[stack[0]];
            let short = stack_labels(
                stack,
                &mut self.routes,
                &self.legs,
                s,
                t,
                blocked.as_deref(),
            );
            let vertical = self.legs[stack[0]][0] == self.legs[stack[0]][2];
            // 狭い所に収めるとき、列は狭いままにする。
            if grow
                && short > 0.0
                && (vertical || !self.tight)
                && widen(
                    &self.parents,
                    &mut self.cells,
                    s,
                    t,
                    vertical,
                    short.ceil(),
                    &mut base,
                )
            {
                grown = true;
            }
        }
        self.stacks = stacks;
        grown
    }

    fn overlap(&self) -> f64 {
        let mut area = 0.0;
        for i in 0..self.routes.len() {
            let p = &self.routes[i];
            for q in &self.routes[i + 1..] {
                let pw = (p.label.width + 8.0) / 2.0;
                let qw = (q.label.width + 8.0) / 2.0;
                let w = js::min(p.lx + pw, q.lx + qw) - js::max(p.lx - pw, q.lx - qw);
                let h = js::min(p.ly + p.label.height / 2.0, q.ly + q.label.height / 2.0)
                    - js::max(p.ly - p.label.height / 2.0, q.ly - q.label.height / 2.0);
                if p.label.width > 0.0 && q.label.width > 0.0 && w > 0.0 && h > 0.0 {
                    area += w * h;
                }
            }
        }
        area
    }

    fn grow(&mut self, x: f64, y: f64, w: f64, h: f64) {
        if x - w / 2.0 < self.min_x {
            self.min_x = x - w / 2.0;
        }
        if x + w / 2.0 > self.max_x {
            self.max_x = x + w / 2.0;
        }
        if y - h / 2.0 < self.min_y {
            self.min_y = y - h / 2.0;
        }
        if y + h / 2.0 > self.max_y {
            self.max_y = y + h / 2.0;
        }
    }

    /// 図の広がりを求め、その幅を返す。
    fn bound(&mut self) -> f64 {
        self.min_x = 0.0;
        self.min_y = 0.0;
        self.max_x = self.content.0;
        self.max_y = self.content.1;
        for i in 0..self.routes.len() {
            let r = &self.routes[i];
            let (loop_, lx, ly, lw, lh) = (r.loop_, r.lx, r.ly, r.label.width, r.label.height);
            if loop_ > 0 {
                let (px, py, r, _, _) = loop_arc(&self.g(self.ends[i].0), loop_);
                self.grow(px, py, 2.0 * r + 2.0, 2.0 * r + 2.0);
            }
            if lw > 0.0 {
                self.grow(lx, ly, lw + 8.0, lh);
            }
        }
        self.max_x - self.min_x
    }
}

fn style_of(db: &BlockDb, block: &Block) -> ResolvedStyle {
    let mut declarations: Vec<String> = Vec::new();
    // スタイルの並びはカンマだけで分けるので、1 つの項にいくつもの宣言がありうる。
    let mut add = |styles: &[String]| {
        for s in styles {
            for part in s.split(';') {
                declarations.push(part.to_string());
            }
        }
    };
    let default = ["default".to_string()];
    let names: &[String] = match &block.classes {
        Some(c) if !c.is_empty() => c,
        _ => &default,
    };
    for name in names {
        if let Some(def) = db.classes.get(name) {
            add(&def.styles);
        }
    }
    if let Some(s) = &block.styles {
        add(s);
    }
    resolve_style(&declarations)
}

fn draw(db: &BlockDb, config: &Config, options: &RenderOptions, tight: bool) -> Result<Rendered> {
    let size = options.font_size.unwrap_or(16.0);
    let edge_size = js::round(size * 0.875);
    let measurer: &dyn TextMeasurer = match &options.measurer {
        Some(m) => m.as_ref(),
        None => default_measurer(),
    };
    let pad = options.padding.unwrap_or(8.0);
    let icons = options.icons.as_ref();
    let configured = config
        .get("block")
        .and_then(Value::as_obj)
        .and_then(|b| b.get("padding"));
    let gap = js::min(
        match configured {
            Some(Value::Num(c)) if *c >= 0.0 => js::min(*c, 200.0),
            _ => GAP,
        },
        if tight { 6.0 } else { f64::INFINITY },
    );
    let wrap = if tight { 80.0 } else { WRAP };

    let view = |b: usize| -> (View, Cell) {
        let block = &db.arena[b];
        let type_ = block.type_.as_deref();
        let st = if type_ == Some("space") {
            resolve_style(&[])
        } else {
            style_of(db, block)
        };
        let text_style =
            (if st.bold { style::BOLD } else { 0 }) | (if st.italic { style::ITALIC } else { 0 });
        let font_size = st.font_size.unwrap_or(size);
        let mut kind = Kind::Leaf;
        let mut shape = "";
        let label;
        let mut nw = 0.0;
        let mut nh = 0.0;
        let mut head = 0.0;
        let mut mask = 0;
        let mut dx = 0.0;
        let mut dy = 0.0;
        if type_ == Some("space") {
            kind = Kind::Space;
            label = layout_label(None, false, measurer, font_size, wrap, 0);
        } else if type_ == Some("composite") {
            kind = Kind::Composite;
            label = layout_label(
                block.label.as_deref(),
                false,
                measurer,
                font_size,
                if tight { title_room(options) } else { 4000.0 },
                text_style,
            );
            head = if label.height > 0.0 {
                label.height + 4.0
            } else {
                0.0
            };
            nw = label.width + 2.0 * gap;
            nh = head + 2.0 * gap;
        } else if type_ == Some("block_arrow") {
            kind = Kind::Arrow;
            label = layout_label(
                block.label.as_deref(),
                false,
                measurer,
                font_size,
                wrap,
                text_style,
            );
            let blank = !label
                .lines
                .iter()
                .flatten()
                .any(|part| part.icon.is_some() || !js::trim(&part.text).is_empty());
            mask = arrow_mask(block.directions.as_deref());
            let s = if blank {
                arrow_size(mask, 0.0, 0.0)
            } else {
                arrow_size(mask, label.width, label.height)
            };
            nw = s.w;
            nh = s.h;
            dx = s.dx;
            dy = s.dy;
        } else {
            shape = shape_of(type_.unwrap_or(""));
            label = layout_label(
                block.label.as_deref(),
                false,
                measurer,
                font_size,
                wrap,
                text_style,
            );
            let s = shape_size(shape, label.width, label.height);
            nw = s.w;
            // 筒のふたは広がるほど深くなるので、いちばん深いふたで大きさを決める。
            nh = if shape == "cyl" {
                js::max(s.h, label.height + 40.0)
            } else {
                s.h
            };
            dy = s.dy;
        }
        let mut c = cell(
            nw,
            nh,
            if kind == Kind::Space {
                1.0
            } else {
                block.width_in_columns.unwrap_or(1.0)
            },
        );
        c.columns = if kind == Kind::Composite {
            block.columns.unwrap_or(-1.0)
        } else {
            -1.0
        };
        c.space = kind == Kind::Space;
        c.head = head;
        (
            View {
                block: b,
                kind,
                shape,
                label,
                style: st,
                mask,
                dx,
                dy,
                bw: 0.0,
                bh: 0.0,
            },
            c,
        )
    };

    let (root, mut root_cell) = view(0);
    root_cell.nw = 0.0;
    root_cell.nh = 0.0;
    let mut views = vec![root];
    let mut cells = vec![root_cell];
    let mut parents: Vec<Option<usize>> = vec![None];
    let mut by_id: HashMap<&str, usize> = HashMap::new();
    let mut i = 0;
    while i < views.len() {
        if views[i].kind == Kind::Composite {
            for &child in &db.arena[views[i].block].children {
                let (v, c) = view(child);
                let at = views.len();
                cells[i].children.push(at);
                parents.push(Some(i));
                let space = v.kind == Kind::Space;
                views.push(v);
                cells.push(c);
                if !space {
                    by_id.insert(db.arena[child].id.as_deref().unwrap_or(""), at);
                }
            }
        }
        i += 1;
    }

    // 配置は入れ物の広かった幅を残すので、毎回要る幅から始める。
    let widths = cells.iter().map(|c| c.nw).collect();
    let mut sc = Scene {
        db,
        measurer,
        gap,
        tight,
        edge_size,
        views,
        cells,
        parents,
        widths,
        obstacles: Vec::new(),
        titles: Vec::new(),
        content: (0.0, 0.0),
        ends: Vec::new(),
        drawn: Vec::new(),
        pairs: HashMap::new(),
        labelled: HashSet::new(),
        check: false,
        routes: Vec::new(),
        stacks: Vec::new(),
        legs: Vec::new(),
        min_x: 0.0,
        min_y: 0.0,
        max_x: 0.0,
        max_y: 0.0,
    };
    sc.place()?;

    // 同じ 2 つのブロックの間の辺は、重ねずに広げて描く。
    for &e in &db.edges {
        let edge = &db.arena[e];
        let s = by_id.get(edge.start.as_deref().unwrap_or(""));
        let t = by_id.get(edge.end.as_deref().unwrap_or(""));
        let (Some(&s), Some(&t)) = (s, t) else {
            continue;
        };
        let key = sc.key(s, t);
        *sc.pairs.entry(key).or_insert(0) += 1;
        if edge.label.as_deref().is_some_and(|l| !l.is_empty()) {
            sc.labelled.insert(key);
        }
        sc.ends.push((s, t));
        sc.drawn.push(e);
    }
    sc.legs = vec![Vec::new(); sc.drawn.len()];
    sc.check = sc.drawn.len() * sc.obstacles.len() <= OBSTACLE_BUDGET;

    if sc.settle(sc.drawn.len() * sc.drawn.len() <= OBSTACLE_BUDGET) {
        let before = sc.overlap();
        let narrow = sc.bound();
        // 行に引き伸ばされた入れ物は新しい隙間の一部を返すので、隙間は何度か要ることがある。
        let mut grown = true;
        let mut round = 0;
        while round < 3 && grown {
            sc.place()?;
            grown = sc.settle(round < 2);
            round += 1;
        }
        // 図が広がるのは、それでラベルの重なりが減り、maxWidth に収まるときだけ。
        let wide = sc.bound();
        let room = match options.max_width {
            Some(w) if w > 0.0 => w,
            _ => f64::INFINITY,
        };
        if sc.overlap() > before - 1.0 || (wide > narrow && (wide + 2.0 * pad).ceil() > room) {
            for c in &mut sc.cells {
                c.row_gaps = Vec::new();
                c.col_gaps = Vec::new();
            }
            sc.place()?;
            sc.settle(false);
        }
    }

    sc.bound();

    let title = title_label(
        db.title.as_deref(),
        measurer,
        size,
        if tight { title_room(options) } else { 4000.0 },
    );
    let title_height = if title.height > 0.0 {
        title.height + 12.0
    } else {
        0.0
    };
    let inner = js::max(sc.max_x - sc.min_x, title.width);
    let ox = pad - sc.min_x + (inner - (sc.max_x - sc.min_x)) / 2.0;
    let oy = pad + title_height - sc.min_y;
    for c in &mut sc.cells {
        c.x += ox;
        c.y += oy;
    }
    let width = (inner + 2.0 * pad).ceil();
    let height = (sc.max_y - sc.min_y + title_height + 2.0 * pad).ceil();

    let mut clusters = String::new();
    let mut nodes = String::new();
    let mut fills: Vec<Fill> = Vec::new();
    for i in 1..sc.views.len() {
        let v = &sc.views[i];
        let c = &sc.cells[i];
        if v.kind == Kind::Space {
            continue;
        }
        let block = &db.arena[v.block];
        let classes = class_names(&block.classes.as_deref().unwrap_or(&[]).join(" "));
        let id = esc_text(block.id.as_deref().unwrap_or(""));
        if v.kind == Kind::Composite {
            let top = c.y - c.h / 2.0;
            if paints(&v.style.shape) {
                fills.push(Fill {
                    box_: [c.x - c.w / 2.0, top, c.x + c.w / 2.0, top + c.h],
                    shape: v.style.shape.clone(),
                });
            }
            clusters.push_str(&format!(
                "<g class=\"pele-cluster{classes}\" data-id=\"{id}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_a)\"{} stroke=\"var(--_b)\"{}/>{}</g>",
                num(c.x - c.w / 2.0),
                num(top),
                num(c.w),
                num(c.h),
                if sets_fill(&v.style.shape) { "" } else { " fill-opacity=\"0.5\"" },
                rounded(&v.style.shape),
                label_svg(
                    &v.label,
                    c.x,
                    top + (c.head + gap) / 2.0 + 2.0,
                    &format!(
                        " class=\"pele-cluster-label\"{}{}",
                        ink_fill(&v.style.shape, &v.style.text, Some("var(--_m)")),
                        v.style.text
                    ),
                    icons
                )
            ));
            continue;
        }
        let arrow = v.kind == Kind::Arrow;
        let relabel;
        let mut label = &v.label;
        // 広いマスを使う。ノードの大きさも、それから求めた経路も変えずに。
        if c.span > 1.0
            && v.label.width > 0.0
            && v.label.lines.len() > 1
            && v.bw > c.nw
            && (arrow || !fixed(v.shape))
        {
            let text_style = (if v.style.bold { style::BOLD } else { 0 })
                | (if v.style.italic { style::ITALIC } else { 0 });
            let room = v.label.width + (v.bw - c.nw);
            relabel = layout_label(
                block.label.as_deref(),
                false,
                measurer,
                v.label.size,
                room,
                text_style,
            );
            label = &relabel;
        }
        let body = if arrow {
            draw_arrow(
                v.mask,
                v.bw,
                v.bh,
                &format!("{SHAPE_ATTRS}{}", v.style.shape),
            )
        } else {
            draw_shape(
                v.shape,
                v.bw,
                v.bh,
                &format!("{SHAPE_ATTRS}{}", v.style.shape),
                &format!("{LINE_ATTRS}{}", v.style.line),
            )
        };
        let ink = if arrow || shape_filled(v.shape) {
            ink_fill(&v.style.shape, &v.style.text, None)
        } else {
            String::new()
        };
        nodes.push_str(&format!(
            "<g class=\"pele-node pele-shape-{}{classes}\" data-id=\"{id}\" transform=\"translate({},{})\">{body}{}</g>",
            if arrow { "block-arrow" } else { v.shape },
            num(c.x),
            num(c.y),
            label_svg(
                label,
                v.dx,
                v.dy,
                &format!(" class=\"pele-label\"{ink}{}", v.style.text),
                icons
            )
        ));
    }

    let mut casings = String::new();
    let mut edges = String::new();
    let mut labels = String::new();
    for i in 0..sc.routes.len() {
        let edge = &db.arena[sc.routes[i].edge];
        let start_type = edge.arrow_type_start.as_deref().unwrap_or("");
        let end_type = edge.arrow_type_end.as_deref().unwrap_or("");
        let loop_ = sc.routes[i].loop_;
        let path = if loop_ > 0 {
            loop_path(
                &sc.g(sc.ends[i].0),
                loop_,
                marker_trim(start_type),
                marker_trim(end_type),
            )
        } else {
            let points = &mut sc.routes[i].points;
            for k in (0..points.len()).step_by(2) {
                points[k] += ox;
                points[k + 1] += oy;
            }
            trace(points, marker_trim(start_type), marker_trim(end_type))
        };
        let over = over_fills(
            &path.d,
            &[path.sx, path.sy, path.ex, path.ey],
            &fills,
            "",
            None,
        );
        let thick = edge.thickness.as_deref() == Some("thick");
        if over.as_deref() == Some("") {
            casings.push_str(&edge_casing(&path, start_type, end_type, "", thick));
        }
        let color = match over.as_deref() {
            Some(o) if !o.is_empty() => o,
            _ => "var(--_l)",
        };
        let id = esc_text(edge.id.as_deref().unwrap_or(""));
        edges.push_str(&format!(
            "<g class=\"pele-edge\" data-id=\"{id}\"{}><path d=\"{}\"{}{}/>{}{}</g>",
            match over.as_deref() {
                Some(o) if !o.is_empty() => format!(" stroke=\"{o}\""),
                _ => String::new(),
            },
            path.d,
            if thick { " stroke-width=\"2.5\"" } else { "" },
            if edge.pattern.as_deref() == Some("dotted") {
                " stroke-dasharray=\"3 4\""
            } else {
                ""
            },
            marker(
                start_type, path.sx, path.sy, path.sdx, path.sdy, color, "none"
            ),
            marker(
                end_type, path.ex, path.ey, path.edx, path.edy, color, "none"
            )
        ));
        let r = &sc.routes[i];
        if r.label.width > 0.0 {
            labels.push_str(&edge_label_svg(
                edge.id.as_deref().unwrap_or(""),
                &r.label,
                r.lx + ox,
                r.ly + oy,
                "",
                icons,
            ));
        }
    }

    let mut content = label_svg(
        &title,
        width / 2.0,
        pad + title.height / 2.0,
        " class=\"pele-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    if !clusters.is_empty() {
        content.push_str(&format!("<g class=\"pele-clusters\">{clusters}</g>"));
    }
    content.push_str(&casings_svg(&casings));
    if !edges.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-edges\" fill=\"none\" stroke=\"var(--_l)\" stroke-linecap=\"round\">{edges}</g>"
        ));
    }
    content.push_str(&format!("<g class=\"pele-nodes\">{nodes}</g>"));
    if !labels.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-edge-labels\" font-size=\"{}\">{labels}</g>",
            js::fmt(edge_size)
        ));
    }
    let svg = svg_document(
        "block",
        width,
        height,
        size,
        options,
        &Accessible::default(),
        &content,
    );
    Ok(Rendered {
        svg,
        width,
        height,
        links: Vec::new(),
    })
}
