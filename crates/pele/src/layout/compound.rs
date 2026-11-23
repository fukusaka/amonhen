//! 入れ子のグループを持つグラフの配置。pele の `src/layout/compound.ts` を移したもの。グループはそれぞれ自分の
//! 向きで配置してから、親の中に 1 つの箱として置く。グループの境を越える辺は、もう一方の端に向いた辺のポートを通る。

#![allow(
    clippy::too_many_arguments,
    clippy::needless_range_loop,
    clippy::type_complexity
)]

use std::collections::HashMap;

use super::layered::{
    Groups, Kind, LEdge, LNode, LayeredOptions, LayeredResult, ease, layered, ledge, lnode, orient,
    rank, sort_by,
};
use crate::js;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    TB,
    BT,
    LR,
    RL,
}

/// 図の名前の向き。ほかは上から下。
pub fn direction(dir: &str) -> Dir {
    match dir {
        "BT" => Dir::BT,
        "LR" => Dir::LR,
        "RL" => Dir::RL,
        _ => Dir::TB,
    }
}

#[derive(Clone, Debug)]
pub struct CNode {
    pub w: f64,
    pub h: f64,
    pub parent: i64,
    pub is_group: bool,
    pub dir: Option<Dir>,
    pub pad_x: f64,
    pub pad_top: f64,
    pub pad_bottom: f64,
    pub min_w: f64,
    /// 宣言の順。グループはその中の最小を取る。
    pub seq: f64,
    /// 辺が 1 辺に広がってよい長さ（流れを横切る向き）。0 ならすべて真ん中。
    pub span: f64,
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug)]
pub struct CEdge {
    pub src: usize,
    pub dst: usize,
    pub minlen: f64,
    pub label_w: f64,
    pub label_h: f64,
    /// x, y, 軸 の 3 つ組。軸はその点に縦に入る所で 0、横で 1。
    pub route: Vec<f64>,
    pub label_x: f64,
    pub label_y: f64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CompoundResult {
    pub width: f64,
    pub height: f64,
}

const TOP: u8 = 0;
const RIGHT: u8 = 1;
const BOTTOM: u8 = 2;
const LEFT: u8 = 3;

#[derive(Clone, Debug)]
struct PortEdge {
    e: usize,
    inner: usize,
    out: bool,
    side: u8,
    /// 前の配置での、グループの外の流れを横切る位置。分からなければ NaN。
    hint: f64,
    px: f64,
    py: f64,
    route: Vec<f64>,
}

#[derive(Clone, Copy, Debug)]
struct LevelEdge {
    e: usize,
    a: usize,
    b: usize,
}

#[derive(Clone, Debug)]
struct Level {
    id: i64,
    dir: Dir,
    items: Vec<usize>,
    edges: Vec<LevelEdge>,
    ports: Vec<PortEdge>,
    /// 辺 * 2 + (入るなら 1) から `ports` の添字。
    port_of: HashMap<usize, usize>,
    routes: HashMap<usize, Vec<f64>>,
    x: f64,
    y: f64,
}

const MAX_PORTS: i64 = 40000;

// 何度もの配置は小さなグラフだけ。
const MAX_ORDERED_PORTS: i64 = 400;
const MAX_ORDERED_EDGES: usize = 500;
// グループを独り立ちで並べるのを試す前に許す交差。
const FLAT_SLACK: f64 = 1.5;
const FLAT_SPARE: f64 = 4.0;

struct Snapshot {
    nodes: Vec<f64>,
    routes: Vec<Vec<f64>>,
    labels: Vec<f64>,
    levels: Vec<f64>,
    total: CompoundResult,
}

fn snapshot(nodes: &[CNode], edges: &[CEdge], levels: &[Level], total: CompoundResult) -> Snapshot {
    let mut at: Vec<f64> = Vec::new();
    for c in nodes {
        at.extend_from_slice(&[c.x, c.y, c.w, c.h]);
    }
    let mut labels: Vec<f64> = Vec::new();
    for e in edges {
        labels.push(e.label_x);
        labels.push(e.label_y);
    }
    let mut places: Vec<f64> = Vec::new();
    for level in levels {
        places.push(level.x);
        places.push(level.y);
        for port in &level.ports {
            places.push(port.px);
            places.push(port.py);
        }
    }
    Snapshot {
        nodes: at,
        routes: edges.iter().map(|e| e.route.clone()).collect(),
        labels,
        levels: places,
        total,
    }
}

fn restore(
    s: &Snapshot,
    nodes: &mut [CNode],
    edges: &mut [CEdge],
    levels: &mut [Level],
) -> CompoundResult {
    for (i, c) in nodes.iter_mut().enumerate() {
        c.x = s.nodes[4 * i];
        c.y = s.nodes[4 * i + 1];
        c.w = s.nodes[4 * i + 2];
        c.h = s.nodes[4 * i + 3];
    }
    for (i, e) in edges.iter_mut().enumerate() {
        e.route = s.routes[i].clone();
        e.label_x = s.labels[2 * i];
        e.label_y = s.labels[2 * i + 1];
    }
    let mut k = 0;
    for level in levels.iter_mut() {
        level.x = s.levels[k];
        level.y = s.levels[k + 1];
        k += 2;
        for port in level.ports.iter_mut() {
            port.px = s.levels[k];
            port.py = s.levels[k + 1];
            k += 2;
        }
    }
    s.total
}

// 共有のノードからこの内の交差は数えない。
const FAN: f64 = 24.0;
// 一直線とみなす幅。
const IN_LINE: f64 = 0.5;
const CURVE_PIECES: usize = 3;
const CORNER_PIECES: usize = 2;
// 段、S の曲線、まっすぐな切れ端と丸めた角を収める。
const STEP_PIECES: usize = 3;
// 描画の角の取っ手に合わせる。
const HANDLE: f64 = 0.62;
const MAX_INSERT: usize = 128;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Style {
    Curved,
    Stepped,
    Linear,
}

pub fn style_of(opt: &LayeredOptions) -> Style {
    if opt.tracks {
        Style::Stepped
    } else if opt.straight {
        Style::Linear
    } else {
        Style::Curved
    }
}

struct Segs {
    ax: Vec<f64>,
    ay: Vec<f64>,
    bx: Vec<f64>,
    by: Vec<f64>,
    low: Vec<f64>,
    high: Vec<f64>,
    owner: Vec<usize>,
    // 端の印: 最初の切れ端は 1、最後は 2。
    ends: Vec<u8>,
    count: usize,
    top: f64,
    bottom: f64,
    from_x: f64,
    from_y: f64,
    to_x: f64,
    to_y: f64,
}

impl Segs {
    fn add(&mut self, ei: usize, part: u8, mut x0: f64, mut y0: f64, x1: f64, y1: f64) {
        let on = x0 == self.to_x
            && y0 == self.to_y
            && ((y1 == y0 && self.from_y == y0) || (x1 == x0 && self.from_x == x0));
        if on && self.count > 0 {
            let p = self.count - 1;
            if self.owner[p] == ei
                && self.ends[p] == part
                && (x0 - self.from_x) * (x1 - x0) + (y0 - self.from_y) * (y1 - y0) > 0.0
            {
                self.count -= 1;
                x0 = self.from_x;
                y0 = self.from_y;
            }
        }
        self.from_x = x0;
        self.from_y = y0;
        self.to_x = x1;
        self.to_y = y1;
        let c = self.count;
        self.ends[c] = part;
        self.ax[c] = js::min(x0, x1);
        self.bx[c] = js::max(x0, x1);
        self.ay[c] = if x0 <= x1 { y0 } else { y1 };
        self.by[c] = if x0 <= x1 { y1 } else { y0 };
        self.low[c] = js::min(y0, y1);
        self.high[c] = js::max(y0, y1);
        if self.low[c] < self.top {
            self.top = self.low[c];
        }
        if self.high[c] > self.bottom {
            self.bottom = self.high[c];
        }
        self.owner[c] = ei;
        self.count += 1;
    }
}

pub fn route_crossings(nodes: &[CNode], edges: &[CEdge], style: Style) -> f64 {
    let mut size = 0usize;
    for e in edges {
        if e.src != e.dst && e.route.len() > 3 {
            size += (e.route.len() / 3 - 1) * STEP_PIECES;
        }
    }
    let mut sg = Segs {
        ax: vec![0.0; size],
        ay: vec![0.0; size],
        bx: vec![0.0; size],
        by: vec![0.0; size],
        low: vec![0.0; size],
        high: vec![0.0; size],
        owner: vec![0; size],
        ends: vec![0; size],
        count: 0,
        top: f64::INFINITY,
        bottom: f64::NEG_INFINITY,
        from_x: f64::NAN,
        from_y: f64::NAN,
        to_x: f64::NAN,
        to_y: f64::NAN,
    };
    for ei in 0..edges.len() {
        let e = &edges[ei];
        if e.src == e.dst {
            continue;
        }
        let r = &e.route;
        let rl = r.len() as i64;
        let aligned = |k: i64| -> bool {
            let k = k as usize;
            (r[k] - r[k - 3]).abs() < 0.01 || (r[k + 1] - r[k - 2]).abs() < 0.01
        };
        let length = |k: i64| -> f64 {
            let k = k as usize;
            (r[k] - r[k - 3]).abs() + (r[k + 1] - r[k - 2]).abs()
        };
        let turns = |k: i64| -> bool {
            style == Style::Curved
                && k > 0
                && k < rl - 3
                && aligned(k)
                && aligned(k + 3)
                && length(k) >= 0.01
                && length(k + 3) >= 0.01
                && (((r[k as usize] - r[k as usize - 3]).abs() < 0.01)
                    != ((r[k as usize + 3] - r[k as usize]).abs() < 0.01))
        };
        let mut at_x = r.first().copied().unwrap_or(f64::NAN);
        let mut at_y = r.get(1).copied().unwrap_or(f64::NAN);
        let mut k: i64 = 3;
        while k < rl {
            let ku = k as usize;
            let part: u8 = (if k == 3 { 1 } else { 0 }) | (if k == rl - 3 { 2 } else { 0 });
            let x0 = at_x;
            let y0 = at_y;
            let x1 = r[ku];
            let y1 = r[ku + 1];
            at_x = x1;
            at_y = y1;
            if aligned(k) || style == Style::Linear {
                if !turns(k) {
                    sg.add(ei, part, x0, y0, x1, y1);
                    k += 3;
                    continue;
                }
                // 隣の角は脚を分け合うので、それぞれ半分。
                let before = if turns(k - 3) {
                    length(k) / 2.0
                } else {
                    length(k)
                };
                let after = if turns(k + 3) {
                    length(k + 3) / 2.0
                } else {
                    length(k + 3)
                };
                let ux = (x1 - r[ku - 3]) / length(k);
                let uy = (y1 - r[ku - 2]) / length(k);
                let vx = (r[ku + 3] - x1) / length(k + 3);
                let vy = (r[ku + 4] - y1) / length(k + 3);
                let sx = x1 - ux * before;
                let sy = y1 - uy * before;
                sg.add(ei, part, x0, y0, sx, sy);
                let c1x = x1 - ux * before * (1.0 - HANDLE);
                let c1y = y1 - uy * before * (1.0 - HANDLE);
                let c2x = x1 + vx * after * (1.0 - HANDLE);
                let c2y = y1 + vy * after * (1.0 - HANDLE);
                at_x = x1 + vx * after;
                at_y = y1 + vy * after;
                let turn = part | (if k + 3 == rl - 3 { 2 } else { 0 });
                let mut px = sx;
                let mut py = sy;
                for i in 1..=CORNER_PIECES {
                    let t = i as f64 / CORNER_PIECES as f64;
                    let u = 1.0 - t;
                    let x = u * u * u * sx
                        + 3.0 * u * u * t * c1x
                        + 3.0 * u * t * t * c2x
                        + t * t * t * at_x;
                    let y = u * u * u * sy
                        + 3.0 * u * u * t * c1y
                        + 3.0 * u * t * t * c2y
                        + t * t * t * at_y;
                    sg.add(ei, turn, px, py, x, y);
                    px = x;
                    py = y;
                }
                k += 3;
                continue;
            }
            if style == Style::Stepped {
                let down = r[ku + 2] == 0.0;
                let mx = if down { x0 } else { (x0 + x1) / 2.0 };
                let my = if down { (y0 + y1) / 2.0 } else { y0 };
                sg.add(ei, part, x0, y0, mx, my);
                sg.add(
                    ei,
                    part,
                    mx,
                    my,
                    if down { x1 } else { mx },
                    if down { my } else { y1 },
                );
                sg.add(
                    ei,
                    part,
                    if down { x1 } else { mx },
                    if down { my } else { y1 },
                    x1,
                    y1,
                );
                k += 3;
                continue;
            }
            // 描く S の曲線を直線の切れ端で近似する。
            let down = r[ku + 2] == 0.0;
            let c = if down {
                ease(x1 - x0, y1 - y0)
            } else {
                ease(y1 - y0, x1 - x0)
            };
            let mut px = x0;
            let mut py = y0;
            for i in 1..=CURVE_PIECES {
                let t = i as f64 / CURVE_PIECES as f64;
                let u = 1.0 - t;
                let across = t * t * (3.0 - 2.0 * t);
                let along = 3.0 * c * t * u * u + 3.0 * (1.0 - c) * t * t * u + t * t * t;
                let x = x0 + (x1 - x0) * (if down { across } else { along });
                let y = y0 + (y1 - y0) * (if down { along } else { across });
                sg.add(ei, part, px, py, x, y);
                px = x;
                py = y;
            }
            k += 3;
        }
    }
    let count = sg.count;
    let (top, bottom) = (sg.top, sg.bottom);
    // 組は両方の切れ端が分け合う最初の帯でだけ数える。
    let bands = if bottom > top {
        js::round((count as f64).sqrt()) as usize
    } else {
        1
    };
    let scale = if bottom > top {
        bands as f64 / (bottom - top)
    } else {
        0.0
    };
    let band_of =
        |y: f64| -> usize { js::min(bands as f64 - 1.0, ((y - top) * scale).floor()) as usize };
    let mut first = vec![0usize; count];
    let mut band_start = vec![0usize; bands + 1];
    for p in 0..count {
        first[p] = band_of(sg.low[p]);
        let last = band_of(sg.high[p]);
        for b in first[p]..=last {
            band_start[b + 1] += 1;
        }
    }
    for b in 0..bands {
        band_start[b + 1] += band_start[b];
    }
    let mut fill = band_start[..bands].to_vec();
    let mut in_band = vec![0usize; band_start[bands]];
    for p in 0..count {
        let last = band_of(sg.high[p]);
        for b in first[p]..=last {
            in_band[fill[b]] = p;
            fill[b] += 1;
        }
    }
    // 共有の端の並びに合流する辺は交差と数えない。
    let mut runs = vec![0f64; edges.len() * 8];
    let in_line = |r: &[f64], a: usize, b: usize, c: usize| -> bool {
        ((r[a] - r[b]).abs() < IN_LINE && (r[a] - r[c]).abs() < IN_LINE)
            || ((r[a + 1] - r[b + 1]).abs() < IN_LINE && (r[a + 1] - r[c + 1]).abs() < IN_LINE)
    };
    for ei in 0..edges.len() {
        let r = &edges[ei].route;
        if r.len() < 6 {
            continue;
        }
        let mut k = 3usize;
        while k + 3 < r.len() && in_line(r, 0, k, k + 3) {
            k += 3;
        }
        let last = r.len() - 3;
        let mut j = last as i64 - 3;
        while j >= 3 && in_line(r, last, j as usize, j as usize - 3) {
            j -= 3;
        }
        let j = j as usize;
        runs[ei * 8..ei * 8 + 8].copy_from_slice(&[
            r[0],
            r[1],
            r[k],
            r[k + 1],
            r[j],
            r[j + 1],
            r[last],
            r[last + 1],
        ]);
    }
    let on_run = |ei: usize, end: u8, x: f64, y: f64| -> bool {
        let o = ei * 8 + if end == 1 { 0 } else { 4 };
        let x0 = runs[o];
        let y0 = runs[o + 1];
        let dx = runs[o + 2] - x0;
        let dy = runs[o + 3] - y0;
        let length = dx * dx + dy * dy;
        let t = if length > 0.0 {
            js::max(0.0, js::min(1.0, ((x - x0) * dx + (y - y0) * dy) / length))
        } else {
            0.0
        };
        js::hypot(x - x0 - t * dx, y - y0 - t * dy) < IN_LINE
    };
    let (ax, ay, bx, by, low, high, owner, ends) = (
        &sg.ax, &sg.ay, &sg.bx, &sg.by, &sg.low, &sg.high, &sg.owner, &sg.ends,
    );
    let mut total = 0f64;
    for band in 0..bands {
        let band_end = band_start[band + 1];
        if band_end - band_start[band] > MAX_INSERT {
            sort_by(&mut in_band[band_start[band]..band_end], |p, q| {
                let d = ax[p] - ax[q];
                if js::truthy(d) {
                    d
                } else {
                    p as f64 - q as f64
                }
            });
        } else {
            for i in (band_start[band] + 1)..band_end {
                let p = in_band[i];
                let mut j = i;
                while j > band_start[band] && ax[in_band[j - 1]] > ax[p] {
                    in_band[j] = in_band[j - 1];
                    j -= 1;
                }
                in_band[j] = p;
            }
        }
        for s in band_start[band]..band_end {
            let p = in_band[s];
            let dx = bx[p] - ax[p];
            let dy = by[p] - ay[p];
            let here = first[p] == band;
            for t in (s + 1)..band_end {
                let q = in_band[t];
                if ax[q] > bx[p] {
                    break;
                }
                if high[q] < low[p]
                    || low[q] > high[p]
                    || owner[p] == owner[q]
                    || (!here && first[q] != band)
                {
                    continue;
                }
                let d1 = dx * (ay[q] - ay[p]) - dy * (ax[q] - ax[p]);
                let d2 = dx * (by[q] - ay[p]) - dy * (bx[q] - ax[p]);
                // 線の上の点は片側とみなし、切れ端の境での交差を 1 度だけ数える。
                if (d1 > 0.0) == (d2 > 0.0) {
                    continue;
                }
                let ex = bx[q] - ax[q];
                let ey = by[q] - ay[q];
                let d3 = ex * (ay[p] - ay[q]) - ey * (ax[p] - ax[q]);
                let d4 = ex * (by[p] - ay[q]) - ey * (bx[p] - ax[q]);
                if (d3 > 0.0) == (d4 > 0.0) {
                    continue;
                }
                let ep = &edges[owner[p]];
                let eq = &edges[owner[q]];
                let at = d3 / (d3 - d4);
                let x = ax[p] + at * dx;
                let y = ay[p] + at * dy;
                // 共有のノードでの扇の入り・出は除く。
                let fan = |v: usize, pe: u8, qe: u8| -> bool {
                    ((ends[p] & pe) != 0 && (ends[q] & qe) != 0)
                        || ((x - nodes[v].x).abs() < nodes[v].w / 2.0 + FAN
                            && (y - nodes[v].y).abs() < nodes[v].h / 2.0 + FAN)
                        || (on_run(owner[p], pe, x, y) && on_run(owner[q], qe, x, y))
                };
                if ep.src == eq.src && fan(ep.src, 1, 1) {
                    continue;
                }
                if ep.src == eq.dst && fan(ep.src, 1, 2) {
                    continue;
                }
                if ep.dst == eq.src && fan(ep.dst, 2, 1) {
                    continue;
                }
                if ep.dst == eq.dst && fan(ep.dst, 2, 2) {
                    continue;
                }
                total += 1.0;
            }
        }
    }
    total
}

/// Mermaid に合わせる: 辺が越えるグループは親の向きを受け継ぐ。
pub fn inherit_crossed_directions(nodes: &mut [CNode], ends: impl IntoIterator<Item = (i64, i64)>) {
    let ancestors = |nodes: &[CNode], i: usize| -> Vec<i64> {
        let mut set: Vec<i64> = Vec::new();
        let mut p = nodes[i].parent;
        while p >= 0 && !set.contains(&p) {
            set.push(p);
            p = nodes[p as usize].parent;
        }
        set
    };
    for (src, dst) in ends {
        if src < 0 || dst < 0 || src == dst {
            continue;
        }
        let a = ancestors(nodes, src as usize);
        let b = ancestors(nodes, dst as usize);
        for &g in &a {
            if !b.contains(&g) {
                nodes[g as usize].dir = None;
            }
        }
        for &g in &b {
            if !a.contains(&g) {
                nodes[g as usize].dir = None;
            }
        }
    }
}

pub fn shift_layout(nodes: &mut [CNode], edges: &mut [CEdge], ox: f64, oy: f64) {
    for c in nodes.iter_mut() {
        c.x += ox;
        c.y += oy;
    }
    for e in edges.iter_mut() {
        let mut k = 0;
        while k < e.route.len() {
            e.route[k] += ox;
            e.route[k + 1] += oy;
            k += 3;
        }
        e.label_x += ox;
        e.label_y += oy;
    }
}

pub fn cnode(w: f64, h: f64, parent: i64) -> CNode {
    CNode {
        w,
        h,
        parent,
        is_group: false,
        dir: None,
        pad_x: 0.0,
        pad_top: 0.0,
        pad_bottom: 0.0,
        min_w: 0.0,
        seq: 0.0,
        span: 0.0,
        x: 0.0,
        y: 0.0,
    }
}

fn start_side(dir: Dir) -> u8 {
    match dir {
        Dir::TB => TOP,
        Dir::BT => BOTTOM,
        Dir::LR => LEFT,
        Dir::RL => RIGHT,
    }
}

fn opposite(side: u8) -> u8 {
    (side + 2) & 3
}

fn endpoint_below(item: usize, e: &CEdge, out: bool) -> bool {
    (if out { e.src } else { e.dst }) != item
}

fn vertical(dir: Dir) -> bool {
    dir == Dir::TB || dir == Dir::BT
}

// 平らな配置から、グループの外の行き先でポートを並べる手がかり。
struct FarHint {
    ledge_of: Vec<i64>,
    ledges: Vec<LEdge>,
    last: Vec<i32>,
}

struct Cached {
    given: Vec<f64>,
    lnodes: Vec<LNode>,
    ledges: Vec<LEdge>,
    size: LayeredResult,
}

struct State<'a> {
    opt: &'a LayeredOptions,
    levels: Vec<Level>,
    level_of: HashMap<i64, usize>,
    local: Vec<usize>,
    key: Vec<f64>,
    keyed: bool,
    far_hint: Option<FarHint>,
    ordered: bool,
    laid: HashMap<i64, Vec<Cached>>,
    total: CompoundResult,
}

impl State<'_> {
    fn level(&self, id: i64) -> &Level {
        &self.levels[self.level_of[&id]]
    }

    fn port(&self, item: usize, slot: usize) -> &PortEdge {
        let level = self.level(item as i64);
        &level.ports[level.port_of[&slot]]
    }

    fn port_mut(&mut self, item: usize, slot: usize) -> &mut PortEdge {
        let li = self.level_of[&(item as i64)];
        let level = &mut self.levels[li];
        let pi = level.port_of[&slot];
        &mut level.ports[pi]
    }

    // 外へ出るポートはグループの最後のランクで並べ、同じなら次のランクで決める。
    fn far_hint(&self, edges: &[CEdge], level: &Level, port: &PortEdge) -> f64 {
        let Some(fh) = &self.far_hint else {
            return f64::NAN;
        };
        let le = fh.ledge_of[port.e];
        if le < 0 || level.id < 0 || !port.out || port.side != opposite(start_side(level.dir)) {
            return f64::NAN;
        }
        let points = &fh.ledges[le as usize].points;
        let mut inside = self.key[edges[port.e].src];
        let mut outside = 0.0;
        let last = fh.last[level.id as usize] as f64;
        let mut k = 0;
        while k < points.len() {
            if points[k] == last {
                inside = points[k + 1];
            }
            if points[k] == last + 1.0 {
                outside = points[k + 1];
            }
            k += 2;
        }
        inside + outside * 1e-6
    }

    fn build(
        &mut self,
        li: usize,
        sized: bool,
        nodes: &[CNode],
        edges: &[CEdge],
    ) -> (Vec<LNode>, Vec<LEdge>, Vec<i64>) {
        let level = &self.levels[li];
        let vertical = vertical(level.dir);
        let start = start_side(level.dir);
        let mut lnodes: Vec<LNode> = Vec::new();
        let mut ledges: Vec<LEdge> = Vec::new();
        for (i, &item) in level.items.iter().enumerate() {
            self.local[item] = i;
            let node = &nodes[item];
            let mut ln = if sized {
                lnode(
                    if vertical { node.w } else { node.h },
                    if vertical { node.h } else { node.w },
                    Kind::Node,
                )
            } else {
                lnode(0.0, 0.0, Kind::Node)
            };
            ln.span = if node.is_group {
                js::min(node.span, ln.w - 2.0 * node.pad_x)
            } else {
                node.span
            };
            ln.key = if self.keyed { self.key[item] } else { f64::NAN };
            lnodes.push(ln);
        }
        let held = |item: usize, ei: usize, out: bool| -> bool {
            nodes[item].is_group && endpoint_below(item, &edges[ei], out)
        };
        let offset = |item: usize, ei: usize, out: bool| -> f64 {
            if !sized || !nodes[item].is_group || !endpoint_below(item, &edges[ei], out) {
                return 0.0;
            }
            let port = self.port(item, ei * 2 + if out { 0 } else { 1 });
            if vertical {
                port.px - nodes[item].w / 2.0
            } else {
                port.py - nodes[item].h / 2.0
            }
        };
        for le in &level.edges {
            let e = &edges[le.e];
            let mut edge = ledge(self.local[le.a], self.local[le.b], e.minlen, 0.0, 0.0);
            if sized {
                edge.label_w = if vertical { e.label_w } else { e.label_h };
                edge.label_h = if vertical { e.label_h } else { e.label_w };
                edge.tail_dx = offset(le.a, le.e, true);
                edge.head_dx = offset(le.b, le.e, false);
                edge.tail_fixed = held(le.a, le.e, true);
                edge.head_fixed = held(le.b, le.e, false);
            }
            ledges.push(edge);
        }
        let mut dummies = vec![-1i64; level.ports.len()];
        for (i, port) in level.ports.iter().enumerate() {
            if port.side == start || port.side == opposite(start) {
                let d = lnodes.len();
                let mut ln = lnode(
                    0.0,
                    0.0,
                    if port.side == start {
                        Kind::StartPort
                    } else {
                        Kind::EndPort
                    },
                );
                ln.key = if self.keyed {
                    self.key[if port.out {
                        edges[port.e].dst
                    } else {
                        edges[port.e].src
                    }]
                } else {
                    f64::NAN
                };
                ln.hint = if port.hint.is_nan() && self.keyed && self.far_hint.is_some() {
                    self.far_hint(edges, level, port)
                } else {
                    port.hint
                };
                lnodes.push(ln);
                dummies[i] = d as i64;
                let mut edge = if port.out {
                    ledge(self.local[port.inner], d, 1.0, 0.0, 0.0)
                } else {
                    ledge(d, self.local[port.inner], 1.0, 0.0, 0.0)
                };
                if port.out {
                    edge.tail_dx = offset(port.inner, port.e, true);
                    edge.tail_fixed = held(port.inner, port.e, true);
                } else {
                    edge.head_dx = offset(port.inner, port.e, false);
                    edge.head_fixed = held(port.inner, port.e, false);
                }
                ledges.push(edge);
            } else {
                lnodes[self.local[port.inner]].pin = if port.side == TOP || port.side == LEFT {
                    -1.0
                } else {
                    1.0
                };
            }
        }
        (lnodes, ledges, dummies)
    }

    // 変わらないレベルは配置を使い回す。
    fn lay_out(
        &mut self,
        id: i64,
        mut lnodes: Vec<LNode>,
        mut ledges: Vec<LEdge>,
    ) -> (Vec<LNode>, Vec<LEdge>, LayeredResult) {
        if !self.ordered {
            let size = layered(&mut lnodes, &mut ledges, self.opt);
            return (lnodes, ledges, size);
        }
        let mut given: Vec<f64> = Vec::new();
        for ln in &lnodes {
            given.extend_from_slice(&[
                ln.w,
                ln.h,
                ln.kind as u8 as f64,
                ln.pin,
                ln.hint,
                ln.key,
                ln.span,
            ]);
        }
        for le in &ledges {
            given.extend_from_slice(&[
                le.tail as f64,
                le.head as f64,
                le.minlen,
                le.label_w,
                le.label_h,
                le.tail_dx,
                le.head_dx,
                if le.tail_fixed { 1.0 } else { 0.0 },
                if le.head_fixed { 1.0 } else { 0.0 },
            ]);
        }
        let past = self.laid.entry(id).or_default();
        for p in past.iter() {
            // Object.is: NaN は NaN と同じ、0 と -0 は別。
            let same = p.given.len() == given.len()
                && p.given
                    .iter()
                    .zip(&given)
                    .all(|(a, b)| a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan()));
            if same {
                return (p.lnodes.clone(), p.ledges.clone(), p.size);
            }
        }
        let size = layered(&mut lnodes, &mut ledges, self.opt);
        past.push(Cached {
            given,
            lnodes: lnodes.clone(),
            ledges: ledges.clone(),
            size,
        });
        (lnodes, ledges, size)
    }

    fn place(&mut self, nodes: &mut [CNode], edges: &mut [CEdge]) {
        // 親を先にランク付けし、辺がグループのどの辺を越えるかを決める。
        for li in 0..self.levels.len() {
            let mut sets: Vec<(usize, usize, u8)> = Vec::new();
            for port in &self.levels[li].ports {
                if nodes[port.inner].is_group
                    && endpoint_below(port.inner, &edges[port.e], port.out)
                {
                    sets.push((
                        port.inner,
                        port.e * 2 + if port.out { 0 } else { 1 },
                        port.side,
                    ));
                }
            }
            for (inner, slot, side) in sets {
                self.port_mut(inner, slot).side = side;
            }
            if self.levels[li].edges.is_empty() {
                continue;
            }
            let mut needed = false;
            for le in &self.levels[li].edges {
                let e = &edges[le.e];
                if (nodes[le.a].is_group && e.src != le.a)
                    || (nodes[le.b].is_group && e.dst != le.b)
                {
                    needed = true;
                }
            }
            if !needed {
                continue;
            }
            let (mut lnodes, mut ledges, _) = self.build(li, false, nodes, edges);
            orient(&lnodes, &mut ledges);
            rank(&mut lnodes, &ledges, 1);
            let start = start_side(self.levels[li].dir);
            let level_edges = self.levels[li].edges.clone();
            for (i, le) in level_edges.iter().enumerate() {
                let e = &edges[le.e];
                let a_above = !ledges[i].reversed;
                if nodes[le.a].is_group && e.src != le.a {
                    self.port_mut(le.a, le.e * 2).side =
                        if a_above { opposite(start) } else { start };
                }
                if nodes[le.b].is_group && e.dst != le.b {
                    self.port_mut(le.b, le.e * 2 + 1).side =
                        if a_above { start } else { opposite(start) };
                }
            }
        }

        // 子を先に配置して大きさを知る。
        for q in (0..self.levels.len()).rev() {
            let (id, dir) = (self.levels[q].id, self.levels[q].dir);
            let group = if id >= 0 { Some(id as usize) } else { None };
            let vertical = vertical(dir);
            let flip = dir == Dir::BT || dir == Dir::RL;
            let (blnodes, bledges, dummies) = self.build(q, true, nodes, edges);
            let (lnodes, ledges, size) = self.lay_out(id, blnodes, bledges);
            let content_w = if vertical { size.width } else { size.height };
            let content_h = if vertical { size.height } else { size.width };
            let pad_x = group.map_or(0.0, |g| nodes[g].pad_x);
            let pad_top = group.map_or(0.0, |g| nodes[g].pad_top);
            let box_w = js::max(
                content_w + 2.0 * pad_x,
                group.map_or(0.0, |g| nodes[g].min_w),
            );
            let box_h = content_h + pad_top + group.map_or(0.0, |g| nodes[g].pad_bottom);
            let off_x = (box_w - content_w) / 2.0;
            let off_y = pad_top;
            let tx = |lx: f64, ly: f64| -> f64 {
                off_x
                    + (if vertical {
                        lx
                    } else if flip {
                        size.height - ly
                    } else {
                        ly
                    })
            };
            let ty = |lx: f64, ly: f64| -> f64 {
                off_y
                    + (if vertical {
                        if flip { size.height - ly } else { ly }
                    } else {
                        lx
                    })
            };
            let axis = if vertical { 0.0 } else { 1.0 };
            let convert = |points: &[f64]| -> Vec<f64> {
                let mut out: Vec<f64> = Vec::new();
                let mut i = 0;
                while i < points.len() {
                    out.push(tx(points[i], points[i + 1]));
                    out.push(ty(points[i], points[i + 1]));
                    out.push(axis);
                    i += 2;
                }
                out
            };

            let items = self.levels[q].items.clone();
            for (i, &item) in items.iter().enumerate() {
                nodes[item].x = tx(lnodes[i].x, lnodes[i].y);
                nodes[item].y = ty(lnodes[i].x, lnodes[i].y);
            }
            let level_edges = self.levels[q].edges.clone();
            let mut label_sets: Vec<(usize, f64, f64)> = Vec::new();
            for (i, le) in level_edges.iter().enumerate() {
                let edge = &ledges[i];
                let route = convert(&edge.points);
                self.levels[q].routes.insert(le.e, route);
                let e = &edges[le.e];
                if e.label_w > 0.0 {
                    label_sets.push((
                        le.e,
                        tx(edge.label_x, edge.label_y),
                        ty(edge.label_x, edge.label_y),
                    ));
                }
            }
            let mut k = level_edges.len();
            let nports = self.levels[q].ports.len();
            for i in 0..nports {
                let port = self.levels[q].ports[i].clone();
                let item = &nodes[port.inner];
                if dummies[i] >= 0 {
                    let mut route = convert(&ledges[k].points);
                    k += 1;
                    let at = if port.out { route.len() - 3 } else { 0 };
                    let mut bx = route[at];
                    let mut by = route[at + 1];
                    if port.side == TOP {
                        by = 0.0;
                    } else if port.side == BOTTOM {
                        by = box_h;
                    } else if port.side == LEFT {
                        bx = 0.0;
                    } else {
                        bx = box_w;
                    }
                    if port.out {
                        route.extend_from_slice(&[bx, by, axis]);
                    } else {
                        route.splice(0..0, [bx, by, axis]);
                    }
                    let p = &mut self.levels[q].ports[i];
                    p.px = bx;
                    p.py = by;
                    p.route = route;
                    continue;
                }
                let mut ix = item.x;
                let mut iy = item.y;
                if item.is_group && endpoint_below(port.inner, &edges[port.e], port.out) {
                    let inner = self.port(port.inner, port.e * 2 + if port.out { 0 } else { 1 });
                    ix += inner.px - item.w / 2.0;
                    iy += inner.py - item.h / 2.0;
                } else if port.side == TOP {
                    iy -= item.h / 2.0;
                } else if port.side == BOTTOM {
                    iy += item.h / 2.0;
                } else if port.side == LEFT {
                    ix -= item.w / 2.0;
                } else {
                    ix += item.w / 2.0;
                }
                let mut bx = ix;
                let mut by = iy;
                if port.side == TOP {
                    by = 0.0;
                } else if port.side == BOTTOM {
                    by = box_h;
                } else if port.side == LEFT {
                    bx = 0.0;
                } else {
                    bx = box_w;
                }
                let a = if port.side == TOP || port.side == BOTTOM {
                    0.0
                } else {
                    1.0
                };
                let p = &mut self.levels[q].ports[i];
                p.px = bx;
                p.py = by;
                p.route = if port.out {
                    vec![ix, iy, a, bx, by, a]
                } else {
                    vec![bx, by, a, ix, iy, a]
                };
            }
            for (ei, x, y) in label_sets {
                edges[ei].label_x = x;
                edges[ei].label_y = y;
            }

            if let Some(g) = group {
                nodes[g].w = box_w;
                nodes[g].h = box_h;
            } else {
                self.total = CompoundResult {
                    width: box_w,
                    height: box_h,
                };
            }
        }
    }
}

fn place(st: &mut State, nodes: &mut [CNode], edges: &mut [CEdge]) {
    st.place(nodes, edges);

    for li in 0..st.levels.len() {
        let ox = st.levels[li].x;
        let oy = st.levels[li].y;
        let items = st.levels[li].items.clone();
        for item in items {
            let node = &mut nodes[item];
            node.x += ox;
            node.y += oy;
            if node.is_group {
                let (cx, cy) = (node.x - node.w / 2.0, node.y - node.h / 2.0);
                let ci = st.level_of[&(item as i64)];
                st.levels[ci].x = cx;
                st.levels[ci].y = cy;
            }
        }
    }

    let append = |route: &mut Vec<f64>, part: &[f64], ox: f64, oy: f64| {
        let mut i = 0;
        while i < part.len() {
            let x = part[i] + ox;
            let y = part[i + 1] + oy;
            let len = route.len();
            if len >= 3 && (route[len - 3] - x).abs() < 0.01 && (route[len - 2] - y).abs() < 0.01 {
                i += 3;
                continue;
            }
            route.extend_from_slice(&[x, y, part[i + 2]]);
            i += 3;
        }
    };

    for li in 0..st.levels.len() {
        let level = &st.levels[li];
        for le in &level.edges {
            let e = &edges[le.e];
            let mut route: Vec<f64> = Vec::new();
            let mut outward: Vec<usize> = Vec::new();
            let mut item = e.src;
            while nodes[item].parent != level.id {
                outward.push(st.level_of[&nodes[item].parent]);
                item = nodes[item].parent as usize;
            }
            for &lv in &outward {
                let l = &st.levels[lv];
                append(&mut route, &l.ports[l.port_of[&(le.e * 2)]].route, l.x, l.y);
            }
            append(&mut route, &level.routes[&le.e], level.x, level.y);
            let mut inward: Vec<usize> = Vec::new();
            let mut item = e.dst;
            while nodes[item].parent != level.id {
                inward.push(st.level_of[&nodes[item].parent]);
                item = nodes[item].parent as usize;
            }
            for &lv in inward.iter().rev() {
                let l = &st.levels[lv];
                append(
                    &mut route,
                    &l.ports[l.port_of[&(le.e * 2 + 1)]].route,
                    l.x,
                    l.y,
                );
            }
            let e = &mut edges[le.e];
            e.route = route;
            if e.label_w > 0.0 {
                e.label_x += level.x;
                e.label_y += level.y;
            }
        }
    }
}

/// 入れ子のグループを持つグラフを配置する。グループはそれぞれ自分の向きで配置し、親の中に 1 つの箱として置く。
/// グループの境を越える辺は、もう一方の端に向いた辺のポートを通る。
pub fn compound_layout(
    nodes: &mut [CNode],
    edges: &mut [CEdge],
    root_dir: Dir,
    opt: &LayeredOptions,
) -> CompoundResult {
    let n = nodes.len();
    let mut levels: Vec<Level> = Vec::new();
    let mut level_of: HashMap<i64, usize> = HashMap::new();
    let make_level = |levels: &mut Vec<Level>,
                      level_of: &mut HashMap<i64, usize>,
                      id: i64,
                      dir: Dir,
                      items: Vec<usize>| {
        level_of.insert(id, levels.len());
        levels.push(Level {
            id,
            dir,
            items,
            edges: vec![],
            ports: vec![],
            port_of: HashMap::new(),
            routes: HashMap::new(),
            x: 0.0,
            y: 0.0,
        });
    };

    let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut root_items: Vec<usize> = Vec::new();
    for i in 0..n {
        let p = nodes[i].parent;
        if p >= 0 && (p as usize) < n && p as usize != i && nodes[p as usize].is_group {
            children[p as usize].push(i);
        } else {
            nodes[i].parent = -1;
            root_items.push(i);
        }
    }
    make_level(&mut levels, &mut level_of, -1, root_dir, root_items);
    let mut depth = vec![0i32; n];
    let mut placed = vec![0u8; n];
    let descend = |levels: &mut Vec<Level>,
                   level_of: &mut HashMap<i64, usize>,
                   depth: &mut Vec<i32>,
                   placed: &mut Vec<u8>,
                   children: &Vec<Vec<usize>>,
                   nodes: &[CNode],
                   from: usize| {
        let mut q = from;
        while q < levels.len() {
            let (id, dir) = (levels[q].id, levels[q].dir);
            let items = levels[q].items.clone();
            for item in items {
                placed[item] = 1;
                depth[item] = if id < 0 { 0 } else { depth[id as usize] + 1 };
                if nodes[item].is_group {
                    make_level(
                        levels,
                        level_of,
                        item as i64,
                        nodes[item].dir.unwrap_or(dir),
                        children[item].clone(),
                    );
                }
            }
            q += 1;
        }
    };
    descend(
        &mut levels,
        &mut level_of,
        &mut depth,
        &mut placed,
        &children,
        nodes,
        0,
    );
    // 互いを含むグループは根から届かない。外へ出す。
    for i in 0..n {
        if placed[i] != 0 {
            continue;
        }
        let former = nodes[i].parent as usize;
        children[former].retain(|&c| c != i);
        nodes[i].parent = -1;
        levels[0].items.push(i);
        placed[i] = 1;
        depth[i] = 0;
        if nodes[i].is_group {
            let from = levels.len();
            make_level(
                &mut levels,
                &mut level_of,
                i as i64,
                nodes[i].dir.unwrap_or(root_dir),
                children[i].clone(),
            );
            descend(
                &mut levels,
                &mut level_of,
                &mut depth,
                &mut placed,
                &children,
                nodes,
                from,
            );
        }
    }

    for q in (1..levels.len()).rev() {
        let mut seq = f64::INFINITY;
        for &item in &levels[q].items {
            if nodes[item].seq < seq {
                seq = nodes[item].seq;
            }
        }
        if seq != f64::INFINITY {
            nodes[levels[q].id as usize].seq = seq;
        }
    }
    for level in levels.iter_mut() {
        sort_by(&mut level.items, |a, b| {
            let d = nodes[a].seq - nodes[b].seq;
            if js::truthy(d) {
                d
            } else {
                a as f64 - b as f64
            }
        });
    }

    // 辺が越えるグループの境はポート 1 つ。予算を超えたら、辺は両端を持つグループの間に、共有のレベルで引く。
    let mut ports: i64 = 0;
    for e in edges.iter_mut() {
        let mut a = e.src;
        let mut b = e.dst;
        let mut crossed: i64 = 0;
        while depth[a] > depth[b] {
            a = nodes[a].parent as usize;
            crossed += 1;
        }
        while depth[b] > depth[a] {
            b = nodes[b].parent as usize;
            crossed += 1;
        }
        while a != b && nodes[a].parent != nodes[b].parent {
            a = nodes[a].parent as usize;
            b = nodes[b].parent as usize;
            crossed += 2;
        }
        if a == b {
            continue;
        }
        if ports + crossed > MAX_PORTS {
            e.src = a;
            e.dst = b;
        } else {
            ports += crossed;
        }
    }

    let mut loops: Vec<usize> = Vec::new();
    let mut stray: Vec<usize> = Vec::new();
    for ei in 0..edges.len() {
        let e = &mut edges[ei];
        e.route = Vec::new();
        if e.src == e.dst {
            loops.push(ei);
            continue;
        }
        let mut a = e.src;
        let mut b = e.dst;
        let mut a_path: Vec<usize> = Vec::new();
        let mut b_path: Vec<usize> = Vec::new();
        while depth[a] > depth[b] {
            a_path.push(a);
            a = nodes[a].parent as usize;
        }
        while depth[b] > depth[a] {
            b_path.push(b);
            b = nodes[b].parent as usize;
        }
        if a == b {
            stray.push(ei);
            continue;
        }
        while nodes[a].parent != nodes[b].parent {
            a_path.push(a);
            b_path.push(b);
            a = nodes[a].parent as usize;
            b = nodes[b].parent as usize;
        }
        let li = level_of[&nodes[a].parent];
        levels[li].edges.push(LevelEdge { e: ei, a, b });
        for (path, out) in [(&a_path, true), (&b_path, false)] {
            for &inner in path {
                let li = level_of[&nodes[inner].parent];
                let level = &mut levels[li];
                let port = PortEdge {
                    e: ei,
                    inner,
                    out,
                    side: TOP,
                    hint: f64::NAN,
                    px: 0.0,
                    py: 0.0,
                    route: vec![],
                };
                level
                    .port_of
                    .insert(ei * 2 + if out { 0 } else { 1 }, level.ports.len());
                level.ports.push(port);
            }
        }
    }

    let ordered = ports > 1 && ports <= MAX_ORDERED_PORTS && edges.len() <= MAX_ORDERED_EDGES;

    // 平らな配置で、グループをまたいでノードとポートの順をそろえる。
    let mut key = vec![f64::NAN; n];
    let mut flat_crossings = 0f64;
    let mut far_hint: Option<FarHint> = None;
    let mut flat = levels.len() > 1 && ordered;
    for level in &levels {
        if level.dir != root_dir {
            flat = false;
        }
    }
    if flat {
        let vertical = vertical(root_dir);
        let mut leaf = vec![-1i64; n];
        let mut group = vec![-1i32; n];
        let mut lnodes: Vec<LNode> = Vec::new();
        let mut of: Vec<i32> = Vec::new();
        let mut parent: Vec<i32> = Vec::new();
        for i in 0..n {
            if !nodes[i].is_group {
                continue;
            }
            group[i] = parent.len() as i32;
            parent.push(-1);
        }
        for i in 0..n {
            let p = nodes[i].parent;
            if nodes[i].is_group {
                parent[group[i] as usize] = if p >= 0 { group[p as usize] } else { -1 };
            } else {
                leaf[i] = lnodes.len() as i64;
                lnodes.push(lnode(
                    if vertical { nodes[i].w } else { nodes[i].h },
                    if vertical { nodes[i].h } else { nodes[i].w },
                    Kind::Node,
                ));
                of.push(if p >= 0 { group[p as usize] } else { -1 });
            }
        }
        let mut ledges: Vec<LEdge> = Vec::new();
        let mut ledge_of = vec![-1i64; edges.len()];
        for (ei, e) in edges.iter().enumerate() {
            if e.src == e.dst || leaf[e.src] < 0 || leaf[e.dst] < 0 {
                continue;
            }
            ledge_of[ei] = ledges.len() as i64;
            ledges.push(ledge(
                leaf[e.src] as usize,
                leaf[e.dst] as usize,
                e.minlen,
                if vertical { e.label_w } else { e.label_h },
                if vertical { e.label_h } else { e.label_w },
            ));
        }
        let mut flat_opt = opt.clone();
        flat_opt.groups = Some(Groups { of, parent });
        flat_opt.order_only = true;
        let flat_layout = layered(&mut lnodes, &mut ledges, &flat_opt);
        flat_crossings = js::min(
            flat_layout.crossings.unwrap_or(0.0),
            flat_layout.declared.unwrap_or(0.0),
        );
        let mut sum = vec![0f64; n];
        let mut count = vec![0i32; n];
        // 各グループが占める最後のランク。
        let mut last = vec![-1i32; n];
        for i in 0..n {
            if leaf[i] < 0 {
                continue;
            }
            let ln = &lnodes[leaf[i] as usize];
            key[i] = ln.x;
            let mut p = nodes[i].parent;
            while p >= 0 {
                let pu = p as usize;
                sum[pu] += key[i];
                count[pu] += 1;
                last[pu] = last[pu].max(ln.rank);
                p = nodes[pu].parent;
            }
        }
        for i in 0..n {
            if count[i] > 0 {
                key[i] = sum[i] / count[i] as f64;
            }
        }
        far_hint = Some(FarHint {
            ledge_of,
            ledges,
            last,
        });
    }

    let mut st = State {
        opt,
        levels,
        level_of,
        local: vec![0usize; n],
        key,
        keyed: false,
        far_hint,
        ordered,
        laid: HashMap::new(),
        total: CompoundResult::default(),
    };

    // 平らな配置はグループの箱を見ないので、独り立ちで並べるほうが交差が少ないことがある。
    let style = style_of(opt);
    st.keyed = flat;
    place(&mut st, nodes, edges);
    let mut crossings = -1f64;
    if flat {
        crossings = route_crossings(nodes, edges, style);
        if crossings > flat_crossings * FLAT_SLACK + FLAT_SPARE {
            let first = snapshot(nodes, edges, &st.levels, st.total);
            st.keyed = false;
            place(&mut st, nodes, edges);
            let own = route_crossings(nodes, edges, style);
            if own >= crossings {
                st.keyed = true;
                st.total = restore(&first, nodes, edges, &mut st.levels);
            } else {
                crossings = own;
            }
        }
    }

    // グループの外の道筋でポートの順を詰める。独り立ちの順が勝ったら省く（もう 1 度の配置に見合うほど
    // 良くなることはまれ）。
    if st.keyed == flat && ordered {
        let mut again = false;
        for li in 0..st.levels.len() {
            let level = &mut st.levels[li];
            if level.id < 0 {
                continue;
            }
            let bx_node = &nodes[level.id as usize];
            let vertical = vertical(level.dir);
            let end = opposite(start_side(level.dir));
            let mut seen: Vec<(f64, f64)> = Vec::new();
            let (lx, ly) = (level.x, level.y);
            for port in level.ports.iter_mut() {
                if port.side != end {
                    continue;
                }
                // ポートから出るまっすぐな部分を飛ばし、辺の曲がる所を探す。
                let route = &edges[port.e].route;
                let bx = lx + port.px;
                let by = ly + port.py;
                let mut at: i64 = -1;
                let mut k = 0usize;
                while k < route.len() && (at < 0 || port.out) {
                    if (route[k] - bx).abs() < 0.01 && (route[k + 1] - by).abs() < 0.01 {
                        at = k as i64;
                    }
                    k += 3;
                }
                if at < 0 {
                    continue;
                }
                let a: i64 = if vertical { 0 } else { 1 };
                let mut next: i64 = if port.out { at + 3 } else { at - 3 };
                while next >= 0
                    && (next as usize) < route.len()
                    && (route[(next + a) as usize] - route[(at + a) as usize]).abs() < IN_LINE
                {
                    next += if port.out { 3 } else { -3 };
                }
                if next < 0 || next as usize >= route.len() {
                    continue;
                }
                port.hint = route[(next + a) as usize]
                    - (if vertical {
                        bx_node.x - bx_node.w / 2.0
                    } else {
                        bx_node.y - bx_node.h / 2.0
                    });
                seen.push((if vertical { port.px } else { port.py }, port.hint));
            }
            sort_by(&mut seen, |p, q| p.0 - q.0);
            for i in 1..seen.len() {
                if seen[i - 1].1 > seen[i].1 {
                    again = true;
                }
            }
        }
        if again {
            let before = if crossings >= 0.0 {
                crossings
            } else {
                route_crossings(nodes, edges, style)
            };
            if before > 0.0 {
                let first = snapshot(nodes, edges, &st.levels, st.total);
                place(&mut st, nodes, edges);
                if route_crossings(nodes, edges, style) >= before {
                    st.total = restore(&first, nodes, edges, &mut st.levels);
                }
            }
        }
    }

    for &ei in &stray {
        let (ax, ay, bx, by) = {
            let e = &edges[ei];
            (
                nodes[e.src].x,
                nodes[e.src].y,
                nodes[e.dst].x,
                nodes[e.dst].y,
            )
        };
        let e = &mut edges[ei];
        e.route = vec![ax, ay, 0.0, bx, by, 0.0];
        e.label_x = (ax + bx) / 2.0;
        e.label_y = (ay + by) / 2.0;
    }
    for &ei in &loops {
        let src = edges[ei].src;
        let node = &nodes[src];
        let e = &mut edges[ei];
        e.label_x = node.x + node.w / 2.0 + 28.0 + e.label_w / 2.0;
        e.label_y = node.y;
    }

    st.total
}
