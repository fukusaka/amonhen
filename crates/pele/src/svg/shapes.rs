//! ノードの形（pele の `src/svg/shapes.ts`）: 大きさ、描き方、外形までの距離（辺の端を外形に寄せる）、辺が広がれる幅。
//! 名前の表は TS の `SHAPES` と同じ。知らない名前は rect として描く。

use crate::js;
use crate::layout::compound::CNode;
use crate::svg::builder::num;
use crate::svg::theme::rounded;
use crate::util::fdlibm;

#[derive(Clone, Copy, Debug)]
pub struct ShapeSize {
    pub w: f64,
    pub h: f64,
    /// ラベルを形の中心からずらす縦の量。
    pub dy: f64,
}

pub const PAD_X: f64 = 18.0;
pub const PAD_Y: f64 = 12.0;

/// まっすぐな辺の両端で、角のために空けておく長さ。
const CORNER: f64 = 12.0;

fn sz(w: f64, h: f64, dy: f64) -> ShapeSize {
    ShapeSize { w, h, dy }
}

fn flat(w: f64, h: f64, sideways: bool) -> f64 {
    (if sideways { h } else { w }) - 2.0 * CORNER
}

/// 上下は両端の `cut` を除いてまっすぐで、左右はまっすぐでない。
fn flat_across(cut: f64, w: f64, sideways: bool) -> f64 {
    if sideways {
        0.0
    } else {
        w - 2.0 * cut - 2.0 * CORNER
    }
}

/// 楕円の外形が箱から離れる量。`t` は長さ `half` の辺の真ん中からの距離。
fn bulge(t: f64, half: f64, depth: f64) -> f64 {
    let k = js::min(1.0, t.abs() / half);
    depth * (1.0 - (1.0 - k * k).sqrt())
}

fn round_inset(w: f64, h: f64, side: i32, t: f64) -> f64 {
    if side & 1 != 0 {
        bulge(t, h / 2.0, w / 2.0)
    } else {
        bulge(t, w / 2.0, h / 2.0)
    }
}

fn round_span(w: f64, h: f64, sideways: bool) -> f64 {
    (if sideways { h } else { w }) * 0.6
}

fn cap_inset(w: f64, side: i32, t: f64) -> f64 {
    if side & 1 != 0 {
        0.0
    } else {
        bulge(t, w / 2.0, js::min(12.0, js::max(6.0, w / 14.0)))
    }
}

fn tri_size(tw: f64, th: f64, flip: bool) -> ShapeSize {
    let w = js::max(1.6 * tw + 40.0, 80.0);
    let h = (th + 10.0) / (1.0 - (tw + 8.0) / w);
    let dy = h / 2.0 - 10.0 - th / 2.0;
    sz(w, h, if flip { -dy } else { dy })
}

fn path(d: &str, a: &str) -> String {
    format!("<path d=\"{d}\"{a}/>")
}

fn lean_inset(h: f64, side: i32) -> f64 {
    if side & 1 != 0 { h / 8.0 } else { 0.0 }
}

fn bx(tw: f64, th: f64) -> ShapeSize {
    sz(
        js::max(tw + 2.0 * PAD_X, 2.0 * PAD_X + 12.0),
        th + 2.0 * PAD_Y,
        0.0,
    )
}

fn poly(points: &[f64], a: &str) -> String {
    let mut d = String::new();
    let mut i = 0;
    while i < points.len() {
        if i > 0 {
            d.push(' ');
        }
        d.push_str(&num(points[i]));
        d.push(',');
        d.push_str(&num(points[i + 1]));
        i += 2;
    }
    format!("<polygon points=\"{d}\"{a}/>")
}

fn rect(w: f64, h: f64, a: &str, rx: &str) -> String {
    format!(
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{rx}{a}/>",
        num(-w / 2.0),
        num(-h / 2.0),
        num(w),
        num(h)
    )
}

fn brace(x: f64, h: f64, dir: f64, line: &str) -> String {
    let y = h / 2.0;
    let k = 6.0 * dir;
    format!(
        "<path d=\"M{},{}q{},0 {},6V-6q0,6 {},6q{},0 {},6V{}q0,6 {},6\" fill=\"none\"{line}/>",
        num(x),
        num(-y),
        num(k),
        num(k),
        num(k),
        num(-k),
        num(-k),
        num(y - 6.0),
        num(-k)
    )
}

fn wave(w: f64, h: f64, a: &str) -> String {
    let x = w / 2.0;
    let top = -h / 2.0;
    let base = h / 2.0 - 10.0;
    let q = w / 4.0;
    format!(
        "<path d=\"M{},{}H{}V{}q{},-12 {},0t{},0Z\"{a}/>",
        num(-x),
        num(top),
        num(x),
        num(base),
        num(-q),
        num(-2.0 * q),
        num(-2.0 * q)
    )
}

/// 名前の表にある形か。
fn known(shape: &str) -> bool {
    matches!(
        shape,
        "rect"
            | "rounded"
            | "stadium"
            | "fr-rect"
            | "cyl"
            | "circle"
            | "dbl-circ"
            | "ellipse"
            | "diam"
            | "hex"
            | "odd"
            | "lean-r"
            | "lean-l"
            | "trap-b"
            | "trap-t"
            | "text"
            | "sm-circ"
            | "f-circ"
            | "fr-circ"
            | "fork"
            | "notch-rect"
            | "lin-rect"
            | "div-rect"
            | "win-pane"
            | "tri"
            | "flip-tri"
            | "sl-rect"
            | "notch-pent"
            | "hourglass"
            | "delay"
            | "h-cyl"
            | "cross-circ"
            | "st-rect"
            | "doc"
            | "flag"
            | "datastore"
            | "folder"
            | "bucket"
            | "console"
            | "browser"
            | "person"
            | "bang"
            | "cloud"
            | "brace"
            | "brace-r"
            | "braces"
            | "bolt"
            | "lin-cyl"
            | "curv-trap"
            | "docs"
            | "bow-rect"
            | "tag-rect"
            | "tag-doc"
            | "lin-doc"
            | "collapsedGroup"
    )
}

pub fn shape_size(shape: &str, tw: f64, th: f64) -> ShapeSize {
    match shape {
        "rect" | "rounded" | "notch-rect" | "datastore" | "tag-rect" => bx(tw, th),
        "stadium" => {
            let h = th + 2.0 * PAD_Y;
            sz(tw + h * 0.5 + 2.0 * PAD_X - 8.0, h, 0.0)
        }
        "fr-rect" => sz(tw + 2.0 * PAD_X + 16.0, th + 2.0 * PAD_Y, 0.0),
        "cyl" => {
            let w = js::max(tw + 2.0 * PAD_X, 56.0);
            let ry = js::min(12.0, js::max(6.0, w / 14.0));
            sz(w, th + 2.0 * PAD_Y + 2.0 * ry, ry / 2.0)
        }
        "circle" => {
            let d = js::max((tw * tw + th * th).sqrt() + 16.0, 44.0);
            sz(d, d, 0.0)
        }
        "dbl-circ" => {
            let d = js::max((tw * tw + th * th).sqrt() + 26.0, 54.0);
            sz(d, d, 0.0)
        }
        "ellipse" => sz(tw * 1.3 + 2.0 * PAD_X, th * 1.2 + 2.0 * PAD_Y, 0.0),
        "diam" => {
            let a = tw + 16.0;
            let b = th + 8.0;
            sz(a + 1.6 * b, a / 1.6 + b, 0.0)
        }
        "hex" | "lean-r" | "lean-l" | "trap-b" | "trap-t" => {
            let h = th + 2.0 * PAD_Y;
            sz(tw + 2.0 * PAD_X + h / 2.0, h, 0.0)
        }
        "odd" | "delay" => {
            let h = th + 2.0 * PAD_Y;
            sz(tw + 2.0 * PAD_X + h / 4.0, h, 0.0)
        }
        "text" => sz(tw + 8.0, th + 8.0, 0.0),
        "sm-circ" | "f-circ" => sz(14.0, 14.0, 0.0),
        "fr-circ" => sz(18.0, 18.0, 0.0),
        "fork" => sz(72.0, 8.0, 0.0),
        "lin-rect" => sz(tw + 2.0 * PAD_X + 8.0, th + 2.0 * PAD_Y, 0.0),
        "div-rect" => sz(tw + 2.0 * PAD_X, th + 2.0 * PAD_Y + 8.0, 4.0),
        "win-pane" | "st-rect" => sz(tw + 2.0 * PAD_X + 8.0, th + 2.0 * PAD_Y + 8.0, 4.0),
        "tri" => tri_size(tw, th, false),
        "flip-tri" => tri_size(tw, th, true),
        "sl-rect" => sz(tw + 2.0 * PAD_X, th + 2.0 * PAD_Y + 10.0, 5.0),
        "notch-pent" => sz(tw + 2.0 * PAD_X, th + 2.0 * PAD_Y, 0.0),
        "hourglass" => sz(32.0, 40.0, 0.0),
        "h-cyl" => {
            let h = th + 2.0 * PAD_Y;
            let rx = js::min(12.0, h / 4.0);
            sz(tw + 2.0 * PAD_X + 2.0 * rx, h, 0.0)
        }
        "cross-circ" => sz(44.0, 44.0, 0.0),
        "doc" | "tag-doc" => sz(tw + 2.0 * PAD_X, th + 2.0 * PAD_Y + 10.0, -5.0),
        "flag" => sz(tw + 2.0 * PAD_X, th + 2.0 * PAD_Y + 16.0, 0.0),
        "folder" => sz(js::max(tw + 2.0 * PAD_X, 64.0), th + 2.0 * PAD_Y + 8.0, 4.0),
        "bucket" => sz(tw + 2.0 * PAD_X + 20.0, th + 2.0 * PAD_Y + 10.0, 3.0),
        "console" | "browser" => sz(
            js::max(tw + 2.0 * PAD_X, 72.0),
            th + 2.0 * PAD_Y + 12.0,
            6.0,
        ),
        "person" => sz(
            js::max(tw + 2.0 * PAD_X, 56.0),
            th + 2.0 * PAD_Y + 26.0,
            13.0,
        ),
        "bang" => sz(tw + 2.0 * PAD_X + 28.0, th + 2.0 * PAD_Y + 24.0, 0.0),
        "cloud" => sz(tw + 2.0 * PAD_X + 32.0, th + 2.0 * PAD_Y + 24.0, 0.0),
        "brace" | "brace-r" => sz(tw + 2.0 * PAD_X + 12.0, th + 2.0 * PAD_Y, 0.0),
        "braces" => sz(tw + 2.0 * PAD_X + 24.0, th + 2.0 * PAD_Y, 0.0),
        "bolt" => sz(36.0, 56.0, 0.0),
        "lin-cyl" => {
            let w = js::max(tw + 2.0 * PAD_X, 56.0);
            let ry = js::min(12.0, js::max(6.0, w / 14.0));
            sz(w, th + 2.0 * PAD_Y + 2.0 * ry + 8.0, ry / 2.0 + 4.0)
        }
        "curv-trap" => {
            let h = th + 2.0 * PAD_Y;
            sz(tw + 2.0 * PAD_X + h * 0.6, h, 0.0)
        }
        "docs" => sz(tw + 2.0 * PAD_X + 8.0, th + 2.0 * PAD_Y + 18.0, -1.0),
        "bow-rect" => {
            let h = th + 2.0 * PAD_Y;
            sz(tw + 2.0 * PAD_X + h * 0.4, h, 0.0)
        }
        "lin-doc" => sz(tw + 2.0 * PAD_X + 8.0, th + 2.0 * PAD_Y + 10.0, -5.0),
        "collapsedGroup" => sz(tw + 2.0 * PAD_X + 8.0, th + 2.0 * PAD_Y + 8.0, -2.0),
        _ => bx(tw, th),
    }
}

pub fn draw_shape(shape: &str, w: f64, h: f64, a: &str, line: &str) -> String {
    let shape = if known(shape) { shape } else { "rect" };
    match shape {
        "rect" => rect(w, h, &rounded(a), ""),
        "rounded" => rect(w, h, a, &format!(" rx=\"{}\"", num(js::min(12.0, h / 2.0)))),
        "stadium" => rect(w, h, a, &format!(" rx=\"{}\"", num(h / 2.0))),
        "fr-rect" => {
            let x = w / 2.0 - 8.0;
            rect(w, h, &rounded(a), "")
                + &format!(
                    "<path d=\"M{},{}V{}M{},{}V{}\" fill=\"none\"{line}/>",
                    num(-x),
                    num(-h / 2.0),
                    num(h / 2.0),
                    num(x),
                    num(-h / 2.0),
                    num(h / 2.0)
                )
        }
        "cyl" => {
            let rx = w / 2.0;
            let ry = js::min(12.0, js::max(6.0, w / 14.0));
            let top = -h / 2.0 + ry;
            let bottom = h / 2.0 - ry;
            format!(
                "<path d=\"M{},{}A{},{} 0 0 1 {},{}V{}A{},{} 0 0 1 {},{}Z\"{a}/><path d=\"M{},{}A{},{} 0 0 0 {},{}\" fill=\"none\"{line}/>",
                num(-rx),
                num(top),
                num(rx),
                num(ry),
                num(rx),
                num(top),
                num(bottom),
                num(rx),
                num(ry),
                num(-rx),
                num(bottom),
                num(-rx),
                num(top),
                num(rx),
                num(ry),
                num(rx),
                num(top)
            )
        }
        "circle" | "sm-circ" => format!("<circle r=\"{}\"{a}/>", num(w / 2.0)),
        "dbl-circ" => format!(
            "<circle r=\"{}\"{a}/><circle r=\"{}\" fill=\"none\"{line}/>",
            num(w / 2.0),
            num(w / 2.0 - 5.0)
        ),
        "ellipse" => format!(
            "<ellipse rx=\"{}\" ry=\"{}\"{a}/>",
            num(w / 2.0),
            num(h / 2.0)
        ),
        "diam" => poly(
            &[0.0, -h / 2.0, w / 2.0, 0.0, 0.0, h / 2.0, -w / 2.0, 0.0],
            a,
        ),
        "hex" => {
            let m = h / 4.0;
            poly(
                &[
                    -w / 2.0 + m,
                    -h / 2.0,
                    w / 2.0 - m,
                    -h / 2.0,
                    w / 2.0,
                    0.0,
                    w / 2.0 - m,
                    h / 2.0,
                    -w / 2.0 + m,
                    h / 2.0,
                    -w / 2.0,
                    0.0,
                ],
                a,
            )
        }
        "odd" => {
            let notch = h / 4.0;
            poly(
                &[
                    -w / 2.0,
                    -h / 2.0,
                    w / 2.0,
                    -h / 2.0,
                    w / 2.0,
                    h / 2.0,
                    -w / 2.0,
                    h / 2.0,
                    -w / 2.0 + notch,
                    0.0,
                ],
                a,
            )
        }
        "lean-r" => {
            let s = h / 4.0;
            poly(
                &[
                    -w / 2.0 + s,
                    -h / 2.0,
                    w / 2.0,
                    -h / 2.0,
                    w / 2.0 - s,
                    h / 2.0,
                    -w / 2.0,
                    h / 2.0,
                ],
                a,
            )
        }
        "lean-l" => {
            let s = h / 4.0;
            poly(
                &[
                    -w / 2.0,
                    -h / 2.0,
                    w / 2.0 - s,
                    -h / 2.0,
                    w / 2.0,
                    h / 2.0,
                    -w / 2.0 + s,
                    h / 2.0,
                ],
                a,
            )
        }
        "trap-b" => {
            let s = h / 4.0;
            poly(
                &[
                    -w / 2.0 + s,
                    -h / 2.0,
                    w / 2.0 - s,
                    -h / 2.0,
                    w / 2.0,
                    h / 2.0,
                    -w / 2.0,
                    h / 2.0,
                ],
                a,
            )
        }
        "trap-t" => {
            let s = h / 4.0;
            poly(
                &[
                    -w / 2.0,
                    -h / 2.0,
                    w / 2.0,
                    -h / 2.0,
                    w / 2.0 - s,
                    h / 2.0,
                    -w / 2.0 + s,
                    h / 2.0,
                ],
                a,
            )
        }
        "text" => rect(w, h, " fill=\"none\" stroke=\"none\"", ""),
        "f-circ" => format!("<circle r=\"{}\" fill=\"var(--_l)\"{line}/>", num(w / 2.0)),
        "fr-circ" => format!(
            "<circle r=\"{}\"{a}/><circle r=\"{}\" fill=\"var(--_l)\"{line}/>",
            num(w / 2.0),
            num(w / 2.0 - 4.0)
        ),
        "fork" => rect(w, h, &format!(" fill=\"var(--_l)\"{line}"), " rx=\"2\""),
        "notch-rect" => {
            let c = 10.0;
            poly(
                &[
                    -w / 2.0 + c,
                    -h / 2.0,
                    w / 2.0,
                    -h / 2.0,
                    w / 2.0,
                    h / 2.0,
                    -w / 2.0,
                    h / 2.0,
                    -w / 2.0,
                    -h / 2.0 + c,
                ],
                a,
            )
        }
        "lin-rect" => {
            rect(w, h, &rounded(a), "")
                + &format!(
                    "<path d=\"M{},{}V{}\" fill=\"none\"{line}/>",
                    num(-w / 2.0 + 8.0),
                    num(-h / 2.0),
                    num(h / 2.0)
                )
        }
        "div-rect" => {
            rect(w, h, &rounded(a), "")
                + &format!(
                    "<path d=\"M{},{}H{}\" fill=\"none\"{line}/>",
                    num(-w / 2.0),
                    num(-h / 2.0 + 8.0),
                    num(w / 2.0)
                )
        }
        "win-pane" => {
            rect(w, h, &rounded(a), "")
                + &format!(
                    "<path d=\"M{},{}V{}M{},{}H{}\" fill=\"none\"{line}/>",
                    num(-w / 2.0 + 8.0),
                    num(-h / 2.0),
                    num(h / 2.0),
                    num(-w / 2.0),
                    num(-h / 2.0 + 8.0),
                    num(w / 2.0)
                )
        }
        "tri" => poly(&[0.0, -h / 2.0, w / 2.0, h / 2.0, -w / 2.0, h / 2.0], a),
        "flip-tri" => poly(&[-w / 2.0, -h / 2.0, w / 2.0, -h / 2.0, 0.0, h / 2.0], a),
        "sl-rect" => poly(
            &[
                -w / 2.0,
                -h / 2.0 + 10.0,
                w / 2.0,
                -h / 2.0,
                w / 2.0,
                h / 2.0,
                -w / 2.0,
                h / 2.0,
            ],
            a,
        ),
        "notch-pent" => {
            let c = 10.0;
            poly(
                &[
                    -w / 2.0 + c,
                    -h / 2.0,
                    w / 2.0 - c,
                    -h / 2.0,
                    w / 2.0,
                    -h / 2.0 + c,
                    w / 2.0,
                    h / 2.0,
                    -w / 2.0,
                    h / 2.0,
                    -w / 2.0,
                    -h / 2.0 + c,
                ],
                a,
            )
        }
        "hourglass" => poly(
            &[
                -w / 2.0,
                -h / 2.0,
                w / 2.0,
                -h / 2.0,
                -w / 2.0,
                h / 2.0,
                w / 2.0,
                h / 2.0,
            ],
            a,
        ),
        "delay" => {
            let r = h / 2.0;
            format!(
                "<path d=\"M{},{}H{}A{},{} 0 0 1 {},{}H{}Z\"{a}/>",
                num(-w / 2.0),
                num(-r),
                num(w / 2.0 - r),
                num(r),
                num(r),
                num(w / 2.0 - r),
                num(r),
                num(-w / 2.0)
            )
        }
        "h-cyl" => {
            let ry = h / 2.0;
            let rx = js::min(12.0, h / 4.0);
            let l = -w / 2.0 + rx;
            let r = w / 2.0 - rx;
            format!(
                "<path d=\"M{},{}H{}A{},{} 0 0 1 {},{}H{}A{},{} 0 0 1 {},{}Z\"{a}/><path d=\"M{},{}A{},{} 0 0 0 {},{}\" fill=\"none\"{line}/>",
                num(l),
                num(-ry),
                num(r),
                num(rx),
                num(ry),
                num(r),
                num(ry),
                num(l),
                num(rx),
                num(ry),
                num(l),
                num(-ry),
                num(r),
                num(-ry),
                num(rx),
                num(ry),
                num(r),
                num(ry)
            )
        }
        "cross-circ" => {
            let r = w / 2.0;
            let k = r * std::f64::consts::FRAC_1_SQRT_2;
            format!(
                "<circle r=\"{}\"{a}/><path d=\"M{},{}L{},{}M{},{}L{},{}\" fill=\"none\"{line}/>",
                num(r),
                num(-k),
                num(-k),
                num(k),
                num(k),
                num(k),
                num(-k),
                num(-k),
                num(k)
            )
        }
        "st-rect" => {
            let bw = w - 8.0;
            let bh = h - 8.0;
            let mut out = String::new();
            for i in 0..3 {
                let x = -w / 2.0 + (2 - i) as f64 * 4.0;
                let y = -h / 2.0 + i as f64 * 4.0;
                out.push_str(&format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}/>",
                    num(x),
                    num(y),
                    num(bw),
                    num(bh),
                    rounded(a)
                ));
            }
            out
        }
        "doc" => wave(w, h, a),
        "flag" => {
            let x = w / 2.0;
            let y = h / 2.0 - 8.0;
            let q = w / 4.0;
            format!(
                "<path d=\"M{},{}q{},-10 {},0t{},0V{}q{},10 {},0t{},0Z\"{a}/>",
                num(-x),
                num(-y),
                num(q),
                num(2.0 * q),
                num(2.0 * q),
                num(y),
                num(-q),
                num(-2.0 * q),
                num(-2.0 * q)
            )
        }
        "datastore" => {
            rect(
                w,
                h,
                &a.replacen("stroke=\"var(--_b)\"", "stroke=\"none\"", 1),
                "",
            ) + &format!(
                "<path d=\"M{},{}H{}M{},{}H{}\" fill=\"none\"{line}/>",
                num(-w / 2.0),
                num(-h / 2.0),
                num(w / 2.0),
                num(-w / 2.0),
                num(h / 2.0),
                num(w / 2.0)
            )
        }
        "folder" => {
            let x = w / 2.0;
            let y = h / 2.0;
            let tab = js::min(w * 0.4, 48.0);
            path(
                &format!(
                    "M{},{}H{}L{},{}H{}V{}H{}Z",
                    num(-x),
                    num(-y),
                    num(-x + tab),
                    num(-x + tab + 8.0),
                    num(-y + 8.0),
                    num(x),
                    num(y),
                    num(-x)
                ),
                a,
            )
        }
        "bucket" => {
            let x = w / 2.0;
            let y = h / 2.0;
            let ry = 5.0;
            let taper = h / 6.0;
            path(
                &format!(
                    "M{},{}A{},{} 0 0 1 {},{}L{},{}A{},{} 0 0 1 {},{}Z",
                    num(-x),
                    num(-y + ry),
                    num(x),
                    num(ry),
                    num(x),
                    num(-y + ry),
                    num(x - taper),
                    num(y - ry),
                    num(x - taper),
                    num(ry),
                    num(-x + taper),
                    num(y - ry)
                ),
                a,
            ) + &format!(
                "<path d=\"M{},{}A{},{} 0 0 0 {},{}\" fill=\"none\"{line}/>",
                num(-x),
                num(-y + ry),
                num(x),
                num(ry),
                num(x),
                num(-y + ry)
            )
        }
        "console" => {
            rect(w, h, &rounded(a), "")
                + &format!(
                    "<path d=\"M{},{}H{}M{},{}l3,2.5l-3,2.5M{},{}h4\" fill=\"none\"{line}/>",
                    num(-w / 2.0),
                    num(-h / 2.0 + 12.0),
                    num(w / 2.0),
                    num(-w / 2.0 + 7.0),
                    num(-h / 2.0 + 4.0),
                    num(-w / 2.0 + 13.0),
                    num(-h / 2.0 + 9.0)
                )
        }
        "browser" => {
            let mut dots = String::new();
            for i in 0..3 {
                dots.push_str(&format!(
                    "<circle cx=\"{}\" cy=\"{}\" r=\"1.5\" fill=\"none\"{line}/>",
                    num(-w / 2.0 + 8.0 + i as f64 * 7.0),
                    num(-h / 2.0 + 6.0)
                ));
            }
            rect(w, h, &rounded(a), "")
                + &format!(
                    "<path d=\"M{},{}H{}\" fill=\"none\"{line}/>",
                    num(-w / 2.0),
                    num(-h / 2.0 + 12.0),
                    num(w / 2.0)
                )
                + &dots
        }
        "person" => {
            let top = -h / 2.0;
            format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"12\"{a}/><circle cy=\"{}\" r=\"13\"{a}/>",
                num(-w / 2.0),
                num(top + 26.0),
                num(w),
                num(h - 26.0),
                num(top + 14.0)
            )
        }
        "bang" => {
            let spikes = 14;
            let mut pts = Vec::new();
            for i in 0..spikes * 2 {
                let angle =
                    (std::f64::consts::PI * i as f64) / spikes as f64 - std::f64::consts::PI / 2.0;
                let k = if i % 2 == 0 { 1.0 } else { 0.82 };
                pts.push(fdlibm::cos(angle) * (w / 2.0) * k);
                pts.push(fdlibm::sin(angle) * (h / 2.0) * k);
            }
            poly(&pts, a)
        }
        "cloud" => {
            let x = w / 2.0;
            let y = h / 2.0;
            let r = y * 0.55;
            path(
                &format!(
                    "M{},{}A{},{} 0 0 1 {},{}A{},{} 0 0 1 {},{}A{},{} 0 0 1 {},{}A{},{} 0 0 1 {},{}Z",
                    num(-x + r),
                    num(y),
                    num(r),
                    num(r),
                    num(-x + r * 0.6),
                    num(-y * 0.1),
                    num(x * 0.42),
                    num(y * 0.7),
                    num(-x * 0.1),
                    num(-y * 0.72),
                    num(x * 0.4),
                    num(y * 0.62),
                    num(x * 0.62),
                    num(-y * 0.42),
                    num(r * 1.05),
                    num(r * 1.05),
                    num(x - r),
                    num(y)
                ),
                a,
            )
        }
        "brace" => {
            rect(w, h, " fill=\"none\" stroke=\"none\"", "")
                + &brace(-w / 2.0 + 12.0, h, -1.0, line)
        }
        "brace-r" => {
            rect(w, h, " fill=\"none\" stroke=\"none\"", "") + &brace(w / 2.0 - 12.0, h, 1.0, line)
        }
        "braces" => {
            rect(w, h, " fill=\"none\" stroke=\"none\"", "")
                + &brace(-w / 2.0 + 12.0, h, -1.0, line)
                + &brace(w / 2.0 - 12.0, h, 1.0, line)
        }
        "bolt" => poly(
            &[
                w * 0.15,
                -h / 2.0,
                -w / 2.0,
                h * 0.08,
                -w * 0.05,
                h * 0.08,
                -w * 0.15,
                h / 2.0,
                w / 2.0,
                -h * 0.08,
                w * 0.05,
                -h * 0.08,
            ],
            a,
        ),
        "lin-cyl" => {
            let rx = w / 2.0;
            let ry = js::min(12.0, js::max(6.0, w / 14.0));
            let top = -h / 2.0 + ry;
            let bottom = h / 2.0 - ry;
            let arc = |y: f64| {
                format!(
                    "M{},{}A{},{} 0 0 0 {},{}",
                    num(-rx),
                    num(y),
                    num(rx),
                    num(ry),
                    num(rx),
                    num(y)
                )
            };
            path(
                &format!(
                    "M{},{}A{},{} 0 0 1 {},{}V{}A{},{} 0 0 1 {},{}Z",
                    num(-rx),
                    num(top),
                    num(rx),
                    num(ry),
                    num(rx),
                    num(top),
                    num(bottom),
                    num(rx),
                    num(ry),
                    num(-rx),
                    num(bottom)
                ),
                a,
            ) + &format!(
                "<path d=\"{}{}\" fill=\"none\"{line}/>",
                arc(top),
                arc(top + 8.0)
            )
        }
        "curv-trap" => {
            let x = w / 2.0;
            let y = h / 2.0;
            let k = h * 0.3;
            path(
                &format!(
                    "M{},{}H{}A{},{} 0 0 1 {},{}H{}L{},0Z",
                    num(-x + k),
                    num(-y),
                    num(x - k),
                    num(k),
                    num(y),
                    num(x - k),
                    num(y),
                    num(-x + k),
                    num(-x)
                ),
                a,
            )
        }
        "docs" => {
            let mut out = String::new();
            for i in 0..3 {
                out.push_str(&format!(
                    "<g transform=\"translate({},{})\">{}</g>",
                    js::fmt(((1 - i) * 4) as f64),
                    js::fmt(((i - 1) * 4) as f64),
                    wave(w - 8.0, h - 8.0, a)
                ));
            }
            out
        }
        "bow-rect" => {
            let x = w / 2.0;
            let y = h / 2.0;
            let k = h * 0.2;
            path(
                &format!(
                    "M{},{}H{}A{},{} 0 0 0 {},{}H{}A{},{} 0 0 1 {},{}Z",
                    num(-x + k),
                    num(-y),
                    num(x),
                    num(k),
                    num(y),
                    num(x),
                    num(y),
                    num(-x + k),
                    num(k),
                    num(y),
                    num(-x + k),
                    num(-y)
                ),
                a,
            )
        }
        "tag-rect" => {
            rect(w, h, &rounded(a), "")
                + &format!(
                    "<path d=\"M{},{}L{},{}\" fill=\"none\"{line}/>",
                    num(w / 2.0 - 12.0),
                    num(h / 2.0),
                    num(w / 2.0),
                    num(h / 2.0 - 12.0)
                )
        }
        "tag-doc" => {
            wave(w, h, a)
                + &format!(
                    "<path d=\"M{},{}L{},{}\" fill=\"none\"{line}/>",
                    num(w / 2.0 - 12.0),
                    num(h / 2.0 - 14.0),
                    num(w / 2.0),
                    num(h / 2.0 - 26.0)
                )
        }
        "lin-doc" => {
            wave(w, h, a)
                + &format!(
                    "<path d=\"M{},{}V{}\" fill=\"none\"{line}/>",
                    num(-w / 2.0 + 8.0),
                    num(-h / 2.0),
                    num(h / 2.0 - 6.0)
                )
        }
        "collapsedGroup" => format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}/><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}/>",
            num(-w / 2.0 + 6.0),
            num(-h / 2.0 + 8.0),
            num(w - 12.0),
            num(h - 8.0),
            rounded(a),
            num(-w / 2.0),
            num(-h / 2.0),
            num(w),
            num(h - 4.0),
            rounded(a)
        ),
        _ => unreachable!(),
    }
}

/// 箱から外形までの距離。辺は 0 上、1 右、2 下、3 左。`t` はその辺の真ん中からの距離（右か下へ）。
pub fn shape_inset(shape: &str, w: f64, h: f64, side: i32, t: f64) -> f64 {
    match shape {
        "stadium" => {
            if side & 1 != 0 {
                bulge(t, h / 2.0, h / 2.0)
            } else {
                0.0
            }
        }
        "cyl" | "lin-cyl" => cap_inset(w, side, t),
        "circle" | "dbl-circ" | "ellipse" | "sm-circ" | "f-circ" | "fr-circ" | "cross-circ" => {
            round_inset(w, h, side, t)
        }
        "diam" => {
            if side & 1 != 0 {
                (t.abs() * w) / h
            } else {
                (t.abs() * h) / w
            }
        }
        "hex" => {
            if side & 1 != 0 {
                t.abs() / 2.0
            } else {
                0.0
            }
        }
        "odd" => {
            if side == 3 {
                h / 4.0
            } else {
                0.0
            }
        }
        "lean-r" | "lean-l" | "trap-b" | "trap-t" => lean_inset(h, side),
        "tri" | "flip-tri" | "bolt" => {
            if side & 1 != 0 {
                w / 4.0
            } else {
                0.0
            }
        }
        "sl-rect" => {
            if side == 0 {
                5.0
            } else {
                0.0
            }
        }
        "hourglass" => {
            if side & 1 != 0 {
                w / 2.0
            } else {
                0.0
            }
        }
        "doc" | "docs" | "tag-doc" | "lin-doc" => {
            if side == 2 {
                10.0
            } else {
                0.0
            }
        }
        "flag" => {
            if side & 1 != 0 {
                0.0
            } else {
                8.0
            }
        }
        "bucket" => {
            if side & 1 != 0 {
                h / 12.0
            } else {
                0.0
            }
        }
        "bang" => 3.0,
        "cloud" => {
            if side & 1 != 0 {
                2.0
            } else {
                1.0
            }
        }
        "bow-rect" if side == 1 => h * 0.2,
        _ => 0.0,
    }
}

/// 流れが下向き（`sideways` なら横向き）のとき、形の 1 辺のうち辺が広がれる幅。
pub fn shape_span(shape: &str, w: f64, h: f64, sideways: bool) -> f64 {
    let s = match shape {
        "rect" | "rounded" | "fr-rect" | "text" | "notch-rect" | "lin-rect" | "div-rect"
        | "win-pane" | "notch-pent" | "datastore" | "console" | "browser" | "tag-rect" => {
            flat(w, h, sideways)
        }
        "stadium" => {
            if sideways {
                h * 0.6
            } else {
                w - h
            }
        }
        "cyl" => {
            if sideways {
                h - 2.0 * CORNER - 12.0
            } else {
                w * 0.6
            }
        }
        "lin-cyl" => {
            if sideways {
                h - 2.0 * CORNER - 20.0
            } else {
                w * 0.6
            }
        }
        "circle" | "dbl-circ" | "ellipse" | "sm-circ" | "f-circ" | "fr-circ" | "cross-circ" => {
            round_span(w, h, sideways)
        }
        "diam" => (if sideways { h } else { w }) * 0.6,
        "hex" => {
            if sideways {
                h * 0.5
            } else {
                w - h / 2.0 - CORNER
            }
        }
        "odd" => flat_across(0.0, w, sideways),
        "lean-r" | "lean-l" | "trap-b" | "trap-t" => flat_across(h / 4.0, w, sideways),
        "fork" => (if sideways { h } else { w }) - 8.0,
        "st-rect" => (if sideways { h } else { w }) - 2.0 * CORNER - 8.0,
        _ => return 0.0,
    };
    js::max(0.0, s)
}

pub fn shape_has_label(shape: &str) -> bool {
    !matches!(
        shape,
        "sm-circ" | "f-circ" | "fr-circ" | "fork" | "hourglass" | "cross-circ" | "bolt"
    )
}

pub fn shape_filled(shape: &str) -> bool {
    !matches!(shape, "text" | "brace" | "brace-r" | "braces")
}

/// 経路の端がノードの配置の箱のどの辺にあるか: 0 上、1 右、2 下、3 左、どれでもなければ -1。
pub fn end_side(route: &[f64], at: usize, c: &CNode) -> i32 {
    let dx = route[at] - c.x;
    let dy = route[at + 1] - c.y;
    if (dy.abs() - c.h / 2.0).abs() < 0.5 {
        return if dy < 0.0 { 0 } else { 2 };
    }
    if (dx.abs() - c.w / 2.0).abs() < 0.5 {
        return if dx < 0.0 { 3 } else { 1 };
    }
    -1
}

/// 経路の端を、ノードの配置の箱から、その中に描く形の外形へ動かす。
pub fn inset_route(route: &mut [f64], at: usize, shape: &str, w: f64, h: f64, c: &CNode) {
    if c.is_group {
        return;
    }
    let side = end_side(route, at, c);
    if side == 0 || side == 2 {
        let amount = shape_inset(shape, w, h, side, route[at] - c.x) + (c.h - h) / 2.0;
        route[at + 1] += if side == 0 { amount } else { -amount };
    } else if side >= 0 {
        let amount = shape_inset(shape, w, h, side, route[at + 1] - c.y) + (c.w - w) / 2.0;
        route[at] += if side == 3 { amount } else { -amount };
    }
}
