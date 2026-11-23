//! block の矢印のブロック（pele の `src/diagrams/block/arrows.ts`）。

use crate::js;
use crate::svg::builder::num;

pub const RIGHT: u32 = 1;
pub const LEFT: u32 = 2;
pub const UP: u32 = 4;
pub const DOWN: u32 = 8;

/// 矢じりが軸の両側へはみ出す長さ。
pub const OVERHANG: f64 = 7.0;
const MAX_DEPTH: f64 = 20.0;
const NUB: f64 = 12.0;

pub struct ArrowSize {
    pub w: f64,
    pub h: f64,
    /// ラベルを真ん中から、矢じりと逆へずらす量。
    pub dx: f64,
    pub dy: f64,
}

/// Mermaid は向きをそのまま比べるので、`( up )` はそこに矢じりを描かない。ここでは空白を除く。
pub fn arrow_mask(directions: Option<&[String]>) -> u32 {
    let mut mask = 0;
    for direction in directions.unwrap_or(&[]) {
        match js::trim(direction) {
            "right" => mask |= RIGHT,
            "left" => mask |= LEFT,
            "up" => mask |= UP,
            "down" => mask |= DOWN,
            "x" => mask |= RIGHT | LEFT,
            "y" => mask |= UP | DOWN,
            _ => {}
        }
    }
    mask
}

fn bit(mask: u32, flag: u32) -> f64 {
    if mask & flag != 0 { 1.0 } else { 0.0 }
}

/// 1 つの軸に沿う矢印は、軸とそれより広い矢じり。両方の軸を指すものは、名指した辺ごとに小さな突起のある箱。
pub fn arrow_axis(mask: u32) -> char {
    if mask != 0 && mask & (UP | DOWN) == 0 {
        return 'x';
    }
    if mask != 0 && mask & (RIGHT | LEFT) == 0 {
        return 'y';
    }
    ' '
}

pub fn arrow_size(mask: u32, tw: f64, th: f64) -> ArrowSize {
    let axis = arrow_axis(mask);
    let across = bit(mask, LEFT) - bit(mask, RIGHT);
    let along = bit(mask, UP) - bit(mask, DOWN);
    if axis == 'x' {
        let h = js::max(th + 8.0, 14.0) + 2.0 * OVERHANG;
        let depth = js::min(h / 2.0, MAX_DEPTH);
        return ArrowSize {
            w: js::max(tw + 12.0, 16.0) + depth * if mask == (RIGHT | LEFT) { 2.0 } else { 1.0 },
            h,
            dx: (across * depth) / 2.0,
            dy: 0.0,
        };
    }
    if axis == 'y' {
        let w = js::max(tw + 16.0, 14.0) + 2.0 * OVERHANG;
        let depth = js::min(w / 2.0, MAX_DEPTH);
        return ArrowSize {
            w,
            h: js::max(th + 8.0, 16.0) + depth * if mask == (UP | DOWN) { 2.0 } else { 1.0 },
            dx: 0.0,
            dy: (along * depth) / 2.0,
        };
    }
    ArrowSize {
        w: js::max(tw + 16.0, 24.0) + NUB * (bit(mask, LEFT) + bit(mask, RIGHT)),
        h: js::max(th + 8.0, 24.0) + NUB * (bit(mask, UP) + bit(mask, DOWN)),
        dx: (across * NUB) / 2.0,
        dy: (along * NUB) / 2.0,
    }
}

pub fn draw_arrow(mask: u32, w: f64, h: f64, attrs: &str) -> String {
    let axis = arrow_axis(mask);
    let x = w / 2.0;
    let y = h / 2.0;
    let mut p: Vec<f64>;
    let has = |flag: u32| mask & flag != 0;
    if axis == 'x' {
        let depth = js::min(y, MAX_DEPTH);
        let s = y - OVERHANG;
        let x0 = if has(LEFT) { -x + depth } else { -x };
        let x1 = if has(RIGHT) { x - depth } else { x };
        p = vec![x0, -s, x1, -s];
        if has(RIGHT) {
            p.extend([x1, -y, x, 0.0, x1, y]);
        }
        p.extend([x1, s, x0, s]);
        if has(LEFT) {
            p.extend([x0, y, -x, 0.0, x0, -y]);
        }
    } else if axis == 'y' {
        let depth = js::min(x, MAX_DEPTH);
        let s = x - OVERHANG;
        let y0 = if has(UP) { -y + depth } else { -y };
        let y1 = if has(DOWN) { y - depth } else { y };
        p = vec![s, y0, s, y1];
        if has(DOWN) {
            p.extend([x, y1, 0.0, y, -x, y1]);
        }
        p.extend([-s, y1, -s, y0]);
        if has(UP) {
            p.extend([-x, y0, 0.0, -y, x, y0]);
        }
    } else {
        let x0 = if has(LEFT) { -x + NUB } else { -x };
        let x1 = if has(RIGHT) { x - NUB } else { x };
        let y0 = if has(UP) { -y + NUB } else { -y };
        let y1 = if has(DOWN) { y - NUB } else { y };
        let cx = (x0 + x1) / 2.0;
        let cy = (y0 + y1) / 2.0;
        let kx = js::min(10.0, (x1 - x0) / 2.0);
        let ky = js::min(10.0, (y1 - y0) / 2.0);
        p = vec![x0, y0];
        if has(UP) {
            p.extend([cx - kx, y0, cx, -y, cx + kx, y0]);
        }
        p.extend([x1, y0]);
        if has(RIGHT) {
            p.extend([x1, cy - ky, x, cy, x1, cy + ky]);
        }
        p.extend([x1, y1]);
        if has(DOWN) {
            p.extend([cx + kx, y1, cx, y, cx - kx, y1]);
        }
        p.extend([x0, y1]);
        if has(LEFT) {
            p.extend([x0, cy + ky, -x, cy, x0, cy - ky]);
        }
    }
    let mut points = String::new();
    for (i, xy) in p.chunks(2).enumerate() {
        if i > 0 {
            points.push(' ');
        }
        points.push_str(&num(xy[0]));
        points.push(',');
        points.push_str(&num(xy[1]));
    }
    format!("<polygon points=\"{points}\" stroke-linejoin=\"round\"{attrs}/>")
}
