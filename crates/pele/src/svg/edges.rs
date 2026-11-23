//! 辺の線と矢じり（pele の `src/svg/edges.ts`）。矢じりの種類は Mermaid のもの（arrow_point・arrow_circle・
//! arrow_cross）で、ほかは何も描かない。

use std::collections::HashMap;

use crate::js;
use crate::layout::compound::CEdge;
use crate::layout::layered::ease;
use crate::svg::builder::{esc_text, label_svg, num};
use crate::svg::ink::ink;
use crate::text::label::Label;
use crate::types::IconResolver;

pub const ARROW: f64 = 8.0;

#[derive(Clone, Debug)]
pub struct EdgePath {
    pub d: String,
    pub tokens: Option<Vec<f64>>,
    pub sx: f64,
    pub sy: f64,
    pub sdx: f64,
    pub sdy: f64,
    pub ex: f64,
    pub ey: f64,
    pub edx: f64,
    pub edy: f64,
}

/// 段のついた経路で、角を丸める最大。
const CORNER: f64 = 12.0;
/// 広い曲がり角の取っ手が 2 つの脚に沿って伸びる割合。円より少し長く、曲がりがふっくらする。
const HANDLE: f64 = 0.62;
/// 辺の端の矢じりの手前で、場所があればまっすぐ走る長さ。
pub const LEAD: f64 = 6.0;

/// 各ノードの各辺で、そこに着く辺が曲がり始める距離。その辺のどの辺にも、そこでいちばん大きい矢じりが要るだけ。
/// どれも隣より先に曲がって交わらないように。
#[derive(Default)]
pub struct SideReach {
    reach: HashMap<i64, f64>,
}

impl SideReach {
    pub fn add(&mut self, node: usize, side: i32, type_: &str) {
        let trim = marker_trim(type_);
        let key = node as i64 * 4 + side as i64;
        if side >= 0 && trim > 0.0 && trim + LEAD > self.reach.get(&key).copied().unwrap_or(0.0) {
            self.reach.insert(key, trim + LEAD);
        }
    }

    pub fn of(&self, node: usize, side: i32) -> f64 {
        if side >= 0 {
            self.reach
                .get(&(node as i64 * 4 + side as i64))
                .copied()
                .unwrap_or(0.0)
        } else {
            0.0
        }
    }
}

pub fn side_reach() -> SideReach {
    SideReach::default()
}

fn near(ax: f64, ay: f64, bx: f64, by: f64) -> bool {
    (ax - bx).abs() < 0.01 && (ay - by).abs() < 0.01
}

/// 経路の点 i（`xs`・`ys` は今のもの）。
struct Pts {
    xs: Vec<f64>,
    ys: Vec<f64>,
    axes: Vec<f64>,
    linear: bool,
}

impl Pts {
    fn aligned(&self, i: usize) -> bool {
        (self.xs[i] - self.xs[i - 1]).abs() < 0.01 || (self.ys[i] - self.ys[i - 1]).abs() < 0.01
    }

    fn straight(&self, i: usize) -> bool {
        self.linear || self.aligned(i)
    }

    /// 点 i から点 j へ向かう向き。S でつなぐときは流れに沿う。
    fn unit(&self, i: usize, j: usize, axis: f64) -> (f64, f64) {
        if !self.straight(i.max(j)) {
            let or1 = |v: f64| if js::truthy(v) { v } else { 1.0 };
            return if axis == 0.0 {
                (0.0, or1(js::sign(self.ys[j] - self.ys[i])))
            } else {
                (or1(js::sign(self.xs[j] - self.xs[i])), 0.0)
            };
        }
        let dx = self.xs[j] - self.xs[i];
        let dy = self.ys[j] - self.ys[i];
        let h = js::hypot(dx, dy);
        let len = if js::truthy(h) { h } else { 1.0 };
        (dx / len, dy / len)
    }

    fn length(&self, i: usize) -> f64 {
        (self.xs[i] - self.xs[i - 1]).abs() + (self.ys[i] - self.ys[i - 1]).abs()
    }

    /// 経路が点 i で角を曲がるか（一方へ走っていたのが他方へ）。
    fn turns(&self, i: usize, last: usize) -> bool {
        !self.linear
            && i > 0
            && i < last
            && self.aligned(i)
            && self.aligned(i + 1)
            && self.length(i) >= 0.01
            && self.length(i + 1) >= 0.01
            && (((self.xs[i] - self.xs[i - 1]).abs() < 0.01)
                != ((self.xs[i + 1] - self.xs[i]).abs() < 0.01))
    }

    /// 矢じりに曲がって入る辺は、場所があれば少しまっすぐ走り、矢じりが曲がりに乗らないようにする。
    fn along(&self, i: usize, ux: f64, uy: f64) -> f64 {
        (self.xs[i] - self.xs[i - 1]).abs() * ux.abs()
            + (self.ys[i] - self.ys[i - 1]).abs() * uy.abs()
    }
}

/// 走りの終わりから曲がるまでのうち、隣の大きい矢じりの埋め合わせはできるだけ保ち、残りは残りの 4 分の 1 まで。
/// ノードの 1 辺の端が、みな同じだけ離れて曲がるように。
fn lead(reach: f64, trim: f64, run: f64) -> f64 {
    let room = js::min(js::max(0.0, reach - LEAD - trim), run / 4.0);
    room + js::min(reach - trim - room, (run - room) / 4.0)
}

/// num() の書いた値を読んだときの値。
fn written(v: f64) -> f64 {
    let n = js::round(v * 100.0);
    if n == 0.0 { 0.0 } else { n / 100.0 }
}

struct PathOut {
    d: String,
    tokens: Vec<f64>,
}

impl PathOut {
    fn tok(&mut self, v: f64) {
        self.tokens.push(written(v));
        crate::svg::builder::write_num(&mut self.d, v);
    }
}

/// x・y・axis の 3 つ組の経路から線を作り、両端を矢じりの分だけ縮める。並んでいない 2 点の間は、流れに沿って出て
/// 着く 1 つの S（段の経路では段）。経路自身が角を曲がる所は、段の経路では少し丸め、まっすぐでないほかの経路では
/// できるだけ広く曲がる: ノードで終わる脚は全部、次の曲がりと分け合う脚は半分を使う。`start_reach`・`end_reach` は
/// 同じ辺のほかの辺が要るだけ、ノードから離れて曲がり始める距離。既定は自分の矢じりをよけるだけ。
pub fn route_path(
    route: &[f64],
    curve: Option<&str>,
    start_trim: f64,
    end_trim: f64,
    start_reach: Option<f64>,
    end_reach: Option<f64>,
) -> EdgePath {
    let start_reach = start_reach.unwrap_or(if start_trim > 0.0 {
        start_trim + LEAD
    } else {
        0.0
    });
    let end_reach = end_reach.unwrap_or(if end_trim > 0.0 { end_trim + LEAD } else { 0.0 });
    let linear = curve == Some("linear");
    let stepped = matches!(curve, Some("step" | "stepBefore" | "stepAfter"));
    let mut p = Pts {
        xs: Vec::new(),
        ys: Vec::new(),
        axes: Vec::new(),
        linear,
    };
    let mut i = 0;
    while i < route.len() {
        let x = route[i];
        let y = route[i + 1];
        let axis = route[i + 2];
        // 段の経路は、なめらかな経路が曲がる所で直角に曲がる。
        if stepped && !p.xs.is_empty() {
            let last = p.xs.len() - 1;
            if (x - p.xs[last]).abs() >= 0.01 && (y - p.ys[last]).abs() >= 0.01 {
                let px = p.xs[last];
                let py = p.ys[last];
                let my = match curve {
                    Some("stepBefore") => py,
                    Some("stepAfter") => y,
                    _ => (py + y) / 2.0,
                };
                let mx = match curve {
                    Some("stepBefore") => px,
                    Some("stepAfter") => x,
                    _ => (px + x) / 2.0,
                };
                let turns = if axis == 0.0 {
                    [px, my, x, my]
                } else {
                    [mx, py, mx, y]
                };
                for k in [0, 2] {
                    if near(turns[k], turns[k + 1], px, py) || near(turns[k], turns[k + 1], x, y) {
                        continue;
                    }
                    p.xs.push(turns[k]);
                    p.ys.push(turns[k + 1]);
                    p.axes.push(axis);
                }
            }
        }
        p.xs.push(x);
        p.ys.push(y);
        p.axes.push(axis);
        i += 3;
    }
    let mut last = p.xs.len() - 1;
    let (sdx, sdy) = p.unit(0, 1, p.axes[1]);
    let (edx, edy) = p.unit(last - 1, last, p.axes[last]);
    let sx = p.xs[0];
    let sy = p.ys[0];
    let ex = p.xs[last];
    let ey = p.ys[last];
    p.xs[0] += sdx * start_trim;
    p.ys[0] += sdy * start_trim;
    p.xs[last] -= edx * end_trim;
    p.ys[last] -= edy * end_trim;
    let arrives =
        if end_reach > end_trim && (!p.straight(last) || (last >= 1 && p.turns(last - 1, last))) {
            lead(end_reach, end_trim, p.along(last, edx, edy))
        } else {
            0.0
        };
    let leaves = if start_reach > start_trim && (!p.straight(1) || p.turns(1, last)) {
        lead(start_reach, start_trim, p.along(1, sdx, sdy))
    } else {
        0.0
    };
    if arrives >= 1.0 {
        let nx = p.xs[last] - edx * arrives;
        let ny = p.ys[last] - edy * arrives;
        let na = p.axes[last];
        p.xs.insert(last, nx);
        p.ys.insert(last, ny);
        p.axes.insert(last, na);
        last += 1;
    }
    if leaves >= 1.0 {
        let nx = p.xs[0] + sdx * leaves;
        let ny = p.ys[0] + sdy * leaves;
        let na = p.axes[1];
        p.xs.insert(1, nx);
        p.ys.insert(1, ny);
        p.axes.insert(1, na);
        last += 1;
    }

    // 塗りを確かめるとき d を読み直さないよう、数を取っておく。
    let mut out = PathOut {
        d: String::new(),
        tokens: Vec::new(),
    };
    out.tokens.extend([77.0, 2.0]);
    out.d.push('M');
    out.tok(p.xs[0]);
    out.d.push(',');
    out.tok(p.ys[0]);
    let mut at_x = p.xs[0];
    let mut at_y = p.ys[0];
    let line_to = |out: &mut PathOut, at_x: &mut f64, at_y: &mut f64, x: f64, y: f64| {
        if near(x, y, *at_x, *at_y) {
            return;
        }
        out.tokens.extend([76.0, 2.0]);
        out.d.push('L');
        out.tok(x);
        out.d.push(',');
        out.tok(y);
        *at_x = x;
        *at_y = y;
    };
    for i in 1..=last {
        let x = p.xs[i];
        let y = p.ys[i];
        if !p.straight(i) {
            let down = p.axes[i] == 0.0;
            let along = if down { y - at_y } else { x - at_x };
            let k = along.abs() * ease(if down { x - at_x } else { y - at_y }, along);
            let ux = if down { 0.0 } else { js::sign(along) * k };
            let uy = if down { js::sign(along) * k } else { 0.0 };
            out.tokens.extend([67.0, 6.0]);
            out.d.push('C');
            out.tok(at_x + ux);
            out.d.push(',');
            out.tok(at_y + uy);
            out.d.push(' ');
            out.tok(x - ux);
            out.d.push(',');
            out.tok(y - uy);
            out.d.push(' ');
            out.tok(x);
            out.d.push(',');
            out.tok(y);
            at_x = x;
            at_y = y;
            continue;
        }
        if !p.turns(i, last) {
            // 1 つのまっすぐな走りの途中の点は、線に何も足さない。
            if i < last
                && p.aligned(i)
                && p.aligned(i + 1)
                && (x - p.xs[i - 1]) * (p.xs[i + 1] - x) + (y - p.ys[i - 1]) * (p.ys[i + 1] - y)
                    > 0.0
            {
                continue;
            }
            line_to(&mut out, &mut at_x, &mut at_y, x, y);
            continue;
        }
        // 曲がりが前の脚と後ろの脚のどれだけを使うか: ノードで終わる脚は全部（矢じりの前のまっすぐな走りの手前まで）、
        // 分け合う脚は半分。
        let mut before = if p.turns(i - 1, last) {
            p.length(i) / 2.0
        } else {
            p.length(i)
        };
        let mut after = if p.turns(i + 1, last) {
            p.length(i + 1) / 2.0
        } else {
            p.length(i + 1)
        };
        if stepped {
            let m = js::min(js::min(CORNER, before), after);
            before = m;
            after = m;
        }
        let (ux, uy) = p.unit(i - 1, i, p.axes[i]);
        let (vx, vy) = p.unit(i, i + 1, p.axes[i + 1]);
        line_to(
            &mut out,
            &mut at_x,
            &mut at_y,
            x - ux * before,
            y - uy * before,
        );
        at_x = x + vx * after;
        at_y = y + vy * after;
        if stepped {
            out.tokens.extend([81.0, 4.0]);
            out.d.push('Q');
            out.tok(x);
            out.d.push(',');
            out.tok(y);
            out.d.push(' ');
            out.tok(at_x);
            out.d.push(',');
            out.tok(at_y);
        } else {
            out.tokens.extend([67.0, 6.0]);
            out.d.push('C');
            out.tok(x - ux * before * (1.0 - HANDLE));
            out.d.push(',');
            out.tok(y - uy * before * (1.0 - HANDLE));
            out.d.push(' ');
            out.tok(x + vx * after * (1.0 - HANDLE));
            out.d.push(',');
            out.tok(y + vy * after * (1.0 - HANDLE));
            out.d.push(' ');
            out.tok(at_x);
            out.d.push(',');
            out.tok(at_y);
        }
    }
    EdgePath {
        d: out.d,
        tokens: Some(out.tokens),
        sx,
        sy,
        sdx: -sdx,
        sdy: -sdy,
        ex,
        ey,
        edx,
        edy,
    }
}

pub fn marker_trim(type_: &str) -> f64 {
    match type_ {
        "arrow_point" => ARROW - 1.0,
        "arrow_circle" => ARROW,
        "arrow_cross" => 4.0,
        _ => 0.0,
    }
}

/// 先が (x, y) で (dx, dy) を向く矢じりを描く。
pub fn marker(type_: &str, x: f64, y: f64, dx: f64, dy: f64, color: &str, stroke: &str) -> String {
    let nx = -dy;
    let ny = dx;
    match type_ {
        "arrow_point" => {
            let bx = x - dx * ARROW;
            let by = y - dy * ARROW;
            let half = ARROW * 0.42;
            format!(
                "<path class=\"pele-marker\" d=\"M{},{}L{},{}L{},{}Z\" fill=\"{color}\" stroke=\"{stroke}\"/>",
                num(x),
                num(y),
                num(bx + nx * half),
                num(by + ny * half),
                num(bx - nx * half),
                num(by - ny * half)
            )
        }
        "arrow_circle" => format!(
            "<circle class=\"pele-marker\" cx=\"{}\" cy=\"{}\" r=\"3.5\" fill=\"{color}\" stroke=\"{stroke}\"/>",
            num(x - dx * 4.0),
            num(y - dy * 4.0)
        ),
        "arrow_cross" => {
            let cx = x - dx * 4.0;
            let cy = y - dy * 4.0;
            let a = 3.5;
            let ux = (dx + nx) * a;
            let uy = (dy + ny) * a;
            let vx = (dx - nx) * a;
            let vy = (dy - ny) * a;
            format!(
                "<path class=\"pele-marker\" d=\"M{},{}L{},{}M{},{}L{},{}\"/>",
                num(cx - ux),
                num(cy - uy),
                num(cx + ux),
                num(cy + uy),
                num(cx - vx),
                num(cy - vy),
                num(cx + vx),
                num(cy + vy)
            )
        }
        _ => String::new(),
    }
}

/// 曲線と弧を点に。NaN が部分の線を分ける。
fn path_points(d: &str) -> Vec<f64> {
    sample(&tokenize(d))
}

fn tens(i: usize) -> f64 {
    10f64.powi(i as i32)
}

/// 命令ごとに、文字の番号、引数の数、引数を並べる。
fn tokenize(d: &str) -> Vec<f64> {
    let b = d.as_bytes();
    let at_ = |k: usize| b.get(k).copied().map_or(u32::MAX, |c| c as u32);
    let mut out: Vec<f64> = Vec::new();
    let mut at: isize = -1;
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as u32;
        if (65..=90).contains(&c) && c != 69 {
            at = out.len() as isize + 1;
            out.push(c as f64);
            out.push(0.0);
            i += 1;
        } else if c == 45 || c == 46 || (48..=57).contains(&c) {
            // ちょうどの整数は桁から作り、Number() と同じ丸めにする。ほかは Number()。
            let mut j = if c == 45 { i + 1 } else { i };
            let mut m = 0.0f64;
            let mut places: i32 = -1;
            let mut k = at_(j);
            while (48..=57).contains(&k) || (k == 46 && places < 0) {
                if k == 46 {
                    places = 0;
                } else {
                    m = m * 10.0 + (k - 48) as f64;
                    if places >= 0 {
                        places += 1;
                    }
                }
                j += 1;
                k = at_(j);
            }

            let v = if (at_(j) | 32) == 101 || m > 9007199254740991.0 || places > 22 {
                let mut k = at_(j);
                while k == 46
                    || k == 101
                    || k == 69
                    || (48..=57).contains(&k)
                    || ((k == 45 || k == 43) && j > 0 && (at_(j - 1) | 32) == 101)
                {
                    j += 1;
                    k = at_(j);
                }
                js::number(&d[i..j])
            } else {
                let mut vv = if places > 0 {
                    m / tens(places as usize)
                } else {
                    m
                };
                if c == 45 {
                    vv = -vv;
                }
                vv
            };
            if at >= 0 {
                out.push(v);
                out[at as usize] += 1.0;
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

#[allow(clippy::needless_range_loop)]
fn sample(tokens: &[f64]) -> Vec<f64> {
    let mut out = Vec::new();
    let mut x = 0.0;
    let mut y = 0.0;
    let mut i = 0;
    while i < tokens.len() {
        let cmd = tokens[i] as u32;
        let count = tokens[i + 1] as usize;
        let s = i + 2;
        let e = s + count;
        i = e;
        if cmd == 72 || cmd == 86 {
            for k in s..e {
                if cmd == 72 {
                    x = tokens[k];
                } else {
                    y = tokens[k];
                }
                out.push(x);
                out.push(y);
            }
        } else if cmd == 67 || cmd == 81 {
            let cubic = cmd == 67;
            let step = if cubic { 6 } else { 4 };
            let mut k = s;
            while k + step <= e {
                let x1 = tokens[k];
                let y1 = tokens[k + 1];
                let x2 = tokens[k + 2];
                let y2 = tokens[k + 3];
                let x3 = if cubic { tokens[k + 4] } else { x2 };
                let y3 = if cubic { tokens[k + 5] } else { y2 };
                let mut t = 0.125;
                while t <= 1.0 {
                    // 丸めを変えないよう、de Casteljau の計算の順を保つ。
                    let mut ax = x + (x1 - x) * t;
                    let mut ay = y + (y1 - y) * t;
                    let mut bx = x1 + (x2 - x1) * t;
                    let mut by = y1 + (y2 - y1) * t;
                    if cubic {
                        let cx = x2 + (x3 - x2) * t;
                        let cy = y2 + (y3 - y2) * t;
                        ax += (bx - ax) * t;
                        ay += (by - ay) * t;
                        bx += (cx - bx) * t;
                        by += (cy - by) * t;
                    }
                    out.push(ax + (bx - ax) * t);
                    out.push(ay + (by - ay) * t);
                    t += 0.125;
                }
                x = x3;
                y = y3;
                k += step;
            }
        } else if cmd == 65 {
            let mut k = s;
            while k + 7 <= e {
                arc(
                    &mut out,
                    x,
                    y,
                    tokens[k],
                    tokens[k + 1],
                    tokens[k + 3],
                    tokens[k + 4],
                    tokens[k + 5],
                    tokens[k + 6],
                );
                x = tokens[k + 5];
                y = tokens[k + 6];
                k += 7;
            }
        } else {
            let mut k = s;
            while k + 2 <= e {
                if cmd == 77 && k == s && !out.is_empty() {
                    out.push(f64::NAN);
                    out.push(f64::NAN);
                }
                x = tokens[k];
                y = tokens[k + 1];
                out.push(x);
                out.push(y);
                k += 2;
            }
        }
    }
    out
}

/// (x1, y1) からの回していない弧を、SVG の端点から中心への変換の後に点にする。
#[allow(clippy::too_many_arguments)]
fn arc(
    out: &mut Vec<f64>,
    x1: f64,
    y1: f64,
    mut rx: f64,
    mut ry: f64,
    large: f64,
    sweep: f64,
    x2: f64,
    y2: f64,
) {
    let hx = (x1 - x2) / 2.0;
    let hy = (y1 - y2) / 2.0;
    if rx == 0.0 || ry == 0.0 || (hx == 0.0 && hy == 0.0) {
        out.push(x2);
        out.push(y2);
        return;
    }
    let scale = js::max(1.0, ((hx * hx) / (rx * rx) + (hy * hy) / (ry * ry)).sqrt());
    rx *= scale;
    ry *= scale;
    let k = (if large == sweep { -1.0 } else { 1.0 })
        * js::max(
            0.0,
            (rx * rx * ry * ry - rx * rx * hy * hy - ry * ry * hx * hx)
                / (rx * rx * hy * hy + ry * ry * hx * hx),
        )
        .sqrt();
    let cx = (k * rx * hy) / ry + (x1 + x2) / 2.0;
    let cy = (-k * ry * hx) / rx + (y1 + y2) / 2.0;
    let from = ((y1 - cy) / ry).atan2((x1 - cx) / rx);
    let mut turn = ((y2 - cy) / ry).atan2((x2 - cx) / rx) - from;
    if js::truthy(sweep) && turn < 0.0 {
        turn += 2.0 * std::f64::consts::PI;
    } else if !js::truthy(sweep) && turn > 0.0 {
        turn -= 2.0 * std::f64::consts::PI;
    }
    for i in 1..=8 {
        let a = from + (turn * i as f64) / 8.0;
        out.push(cx + rx * a.cos());
        out.push(cy + ry * a.sin());
    }
}

/// 線分が箱と重なるかを Liang-Barsky の切り取りで。
#[allow(clippy::too_many_arguments)]
fn clips(x: f64, y: f64, dx: f64, dy: f64, l: f64, t: f64, r: f64, b: f64) -> bool {
    let mut lo = 0.0;
    let mut hi = 1.0;
    for side in 0..4 {
        let p = match side {
            0 => -dx,
            1 => dx,
            2 => -dy,
            _ => dy,
        };
        let q = match side {
            0 => x - l,
            1 => r - x,
            2 => y - t,
            _ => b - y,
        };
        if p == 0.0 {
            if q < 0.0 {
                hi = -1.0;
            }
        } else if p < 0.0 {
            lo = js::max(lo, q / p);
        } else {
            hi = js::min(hi, q / p);
        }
    }
    lo < hi
}

fn enters(points: &[f64], l: f64, t: f64, r: f64, b: f64) -> bool {
    let mut k = 0;
    while k + 2 < points.len() {
        let x = points[k];
        let y = points[k + 1];
        let x2 = points[k + 2];
        let y2 = points[k + 3];
        k += 2;
        if (x < l && x2 < l) || (x > r && x2 > r) || (y < t && y2 < t) || (y > b && y2 > b) {
            continue;
        }
        if clips(x, y, x2 - x, y2 - y, l, t, r, b) {
            return true;
        }
    }
    false
}

/// 塗りの箱は [left, top, right, bottom]。
#[derive(Clone, Debug)]
pub struct Fill {
    pub box_: [f64; 4],
    pub shape: String,
}

/// `fill_under` の答え。
pub enum Under<'a> {
    /// 塗りの外。
    Off,
    /// 塗りの縁を横切る。
    Crosses,
    /// 辺と矢じりの先を丸ごと持ついちばん内側の塗り。
    In(&'a Fill),
}

/// 辺と矢じりの先を丸ごと持ついちばん内側の塗り、縁を横切るなら `Crosses`、塗りの外なら `Off`。
pub fn fill_under<'a>(
    d: &str,
    tips: &[f64],
    fills: &'a [Fill],
    tokens: Option<&[f64]>,
) -> Under<'a> {
    if fills.is_empty() {
        return Under::Off;
    }
    let points = match tokens {
        Some(t) => sample(t),
        None => path_points(d),
    };
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut k = 0;
    while k < points.len() + tips.len() {
        let x = if k < points.len() {
            points[k]
        } else {
            tips[k - points.len()]
        };
        let y = if k < points.len() {
            points[k + 1]
        } else {
            tips[k + 1 - points.len()]
        };
        if x < min_x {
            min_x = x;
        }
        if x > max_x {
            max_x = x;
        }
        if y < min_y {
            min_y = y;
        }
        if y > max_y {
            max_y = y;
        }
        k += 2;
    }
    let mut holder: Option<&Fill> = None;
    let mut least = f64::INFINITY;
    for fill in fills {
        let [l, t, r, b] = fill.box_;
        if max_x < l + 0.5 || min_x > r - 0.5 || max_y < t + 0.5 || min_y > b - 0.5 {
            continue;
        }
        let mut touched = enters(&points, l + 0.5, t + 0.5, r - 0.5, b - 0.5);
        let mut k = 0;
        while k < tips.len() && !touched {
            touched = tips[k] > l + 0.5
                && tips[k] < r - 0.5
                && tips[k + 1] > t + 0.5
                && tips[k + 1] < b - 0.5;
            k += 2;
        }
        if !touched {
            continue;
        }
        if min_x < l || max_x > r || min_y < t || max_y > b {
            return Under::Crosses;
        }
        let area = (r - l) * (b - t);
        if area < least {
            least = area;
            holder = Some(fill);
        }
    }
    match holder {
        Some(f) => Under::In(f),
        None => Under::Off,
    }
}

/// 塗りの上の辺の色。分からないとき、線のスタイルが stroke を決めるときは、下敷き（casing）の ''。
pub fn edge_ink(fill: &Fill, line: &str) -> String {
    if line.contains("\"stroke:") || line.contains(";stroke:") {
        return String::new();
    }
    ink(&fill.shape, "").unwrap_or("").to_string()
}

/// 中にページの色を持つ矢じりを、塗りの色に。
pub fn hollow_on(markers: &str, fill: &Fill) -> String {
    match crate::svg::ink::style_fill(&fill.shape).map(js::trim) {
        Some(color) if !color.is_empty() => {
            markers.replace(" fill=\"var(--_bg)\"", &format!(" fill=\"{color}\""))
        }
        _ => markers.to_string(),
    }
}

/// 辺を丸ごと持ついちばん内側の塗りの色、縁を横切るときは下敷きの ''、塗りの外なら None。
pub fn over_fills(
    d: &str,
    tips: &[f64],
    fills: &[Fill],
    line: &str,
    tokens: Option<&[f64]>,
) -> Option<String> {
    match fill_under(d, tips, fills, tokens) {
        Under::In(f) => Some(edge_ink(f, line)),
        Under::Crosses => Some(String::new()),
        Under::Off => None,
    }
}

const CASE: f64 = 4.0;

/// スタイルの `[";]prop:([^;"]+);` の値。
fn decl<'a>(line: &'a str, prop: &str) -> Option<&'a str> {
    if !line.contains(prop) {
        return None;
    }
    let key = format!("{prop}:");
    let b = line.as_bytes();
    let mut from = 0;
    while let Some(k) = line[from..].find(&key) {
        let at = from + k;
        if at > 0 && (b[at - 1] == b'"' || b[at - 1] == b';') {
            let v = &line[at + key.len()..];
            let end = v.find([';', '"']).unwrap_or(v.len());
            if end > 0 && v[end..].starts_with(';') {
                return Some(js::trim(&v[..end]));
            }
        }
        from = at + 1;
    }
    None
}

fn plain_width(w: &str) -> bool {
    let body = w.strip_suffix("px").unwrap_or(w);
    !body.is_empty() && body.bytes().all(|c| c.is_ascii_digit() || c == b'.')
}

/// 辺の下の、ページの色の太い線（辺と同じだけ薄く）。矢じりはクラスを落とし、矢じりのスタイルが届かないように。
pub fn casing(d: &str, markers: &str, line: &str, thick: bool) -> String {
    let stroke = decl(line, "stroke");
    if stroke == Some("none") || stroke == Some("transparent") {
        return String::new();
    }
    let width = if thick {
        Some("2.5")
    } else {
        decl(line, "stroke-width")
    };
    let mut attrs = String::new();
    let mut style = String::new();
    match width {
        Some(w) if plain_width(w) => {
            if js::parse_float(w) != 1.0 {
                attrs = format!(" stroke-width=\"{}\"", num(js::parse_float(w) + CASE));
            }
        }
        Some(w) => style.push_str(&format!("stroke-width:calc({w} + {}px);", js::fmt(CASE))),
        None => {}
    }
    for prop in ["opacity", "stroke-opacity"] {
        if let Some(v) = decl(line, prop) {
            style.push_str(&format!("{prop}:{v};"));
        }
    }
    let body = format!(
        "<path d=\"{d}\"/>{}",
        markers.replace(" class=\"pele-marker\"", "")
    );
    if !attrs.is_empty() || !style.is_empty() {
        let s = if style.is_empty() {
            String::new()
        } else {
            format!(" style=\"{style}\"")
        };
        format!("<g{attrs}{s}>{body}</g>")
    } else {
        body
    }
}

pub fn edge_casing(
    path: &EdgePath,
    start_type: &str,
    end_type: &str,
    line: &str,
    thick: bool,
) -> String {
    casing(
        &path.d,
        &(marker(
            start_type,
            path.sx,
            path.sy,
            path.sdx,
            path.sdy,
            "var(--_bg)",
            "var(--_bg)",
        ) + &marker(
            end_type,
            path.ex,
            path.ey,
            path.edx,
            path.edy,
            "var(--_bg)",
            "var(--_bg)",
        )),
        line,
        thick,
    )
}

pub fn casings_svg(casings: &str) -> String {
    if casings.is_empty() {
        return String::new();
    }
    format!(
        "<g class=\"pele-casings\" fill=\"none\" stroke=\"var(--_bg)\" stroke-width=\"{}\" stroke-linecap=\"round\" stroke-linejoin=\"round\" pointer-events=\"none\">{casings}</g>",
        js::fmt(1.0 + CASE)
    )
}

/// 背景の色の当て布の上の辺のラベル。線が文字を通らないように。
pub fn edge_label_svg(
    id: &str,
    label: &Label,
    x: f64,
    y: f64,
    attrs: &str,
    icons: Option<&IconResolver>,
) -> String {
    let w = label.width + 8.0;
    let h = label.height;
    format!(
        "<g class=\"pele-edge-label\" data-id=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"3\" fill=\"var(--_bg)\"/>{}</g>",
        esc_text(id),
        num(x - w / 2.0),
        num(y - h / 2.0),
        num(w),
        num(h),
        label_svg(label, x, y, attrs, icons)
    )
}

/// 描いた大きさ w×h のノードの横の自分への輪: 流れを横切る辺から出て戻る。`k` はそのノードにすでにある輪の数で、
/// 輪ごとに少し遠くまで伸びる。辺のラベルも輪の先に置く。
#[allow(clippy::too_many_arguments)]
pub fn loop_path(
    cx: f64,
    cy: f64,
    w: f64,
    h: f64,
    k: f64,
    sideways: bool,
    base: f64,
    trim: f64,
    e: &mut CEdge,
) -> EdgePath {
    let half = (if sideways { w } else { h }) / 2.0;
    let spread = js::min(half - 4.0, 8.0 + k * 6.0);
    let reach = base + k * 8.0;
    let hyp = js::hypot(reach, spread);
    let len = if js::truthy(hyp) { hyp } else { 1.0 };
    // 手元の座標: `out` はノードから離れる向き、`along` はその辺に沿う。
    let at = |out: f64, along: f64| -> String {
        if sideways {
            format!("{},{}", num(cx + along), num(cy + h / 2.0 + out))
        } else {
            format!("{},{}", num(cx + w / 2.0 + out), num(cy + along))
        }
    };
    let tx = (reach / len) * trim;
    let ty = (spread / len) * trim;
    if sideways {
        e.label_x = cx;
        e.label_y = cy + h / 2.0 + reach + 4.0 + e.label_h / 2.0;
    } else {
        e.label_x = cx + w / 2.0 + reach + 4.0 + e.label_w / 2.0;
        e.label_y = cy;
    }
    EdgePath {
        d: format!(
            "M{}C{} {} {}",
            at(0.0, -spread),
            at(reach, -spread * 2.0),
            at(reach, spread * 2.0),
            at(tx, spread + ty)
        ),
        tokens: None,
        sx: if sideways { cx - spread } else { cx + w / 2.0 },
        sy: if sideways { cy + h / 2.0 } else { cy - spread },
        sdx: if sideways { 0.0 } else { -1.0 },
        sdy: if sideways { -1.0 } else { 0.0 },
        ex: if sideways { cx + spread } else { cx + w / 2.0 },
        ey: if sideways { cy + h / 2.0 } else { cy + spread },
        edx: if sideways {
            -spread / len
        } else {
            -reach / len
        },
        edy: if sideways {
            -reach / len
        } else {
            -spread / len
        },
    }
}
