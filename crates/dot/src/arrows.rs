//! 矢じり: Graphviz の矢じりの書き方（形の名前に `o`・`l`・`r` を前に付け、4 つまでつなぐ）を読み、長さと SVG を
//! 作る。知らない名前は `normal`（Graphviz と同じ）。古い名前（`ediamond`・`open`・`halfopen`・`empty`・
//! `invempty`・`invdot`・`invodot`）も読む。

use amonhen_pele::svg::builder::num;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    Normal,
    Inv,
    Dot,
    Tee,
    Box,
    Diamond,
    Crow,
    Vee,
    Curve,
    ICurve,
    /// 隙間（`none` を並びの中で使ったもの）。
    Gap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Half {
    Both,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Part {
    pub glyph: Glyph,
    pub open: bool,
    pub half: Half,
}

/// 矢じりの長さの基準（arrowsize 1）。
pub const LENGTH: f64 = 10.0;

const NAMES: &[(&str, Glyph)] = &[
    ("normal", Glyph::Normal),
    ("inv", Glyph::Inv),
    ("dot", Glyph::Dot),
    ("tee", Glyph::Tee),
    ("box", Glyph::Box),
    ("diamond", Glyph::Diamond),
    ("crow", Glyph::Crow),
    ("vee", Glyph::Vee),
    ("curve", Glyph::Curve),
    ("icurve", Glyph::ICurve),
    ("none", Glyph::Gap),
];

fn legacy(name: &str) -> Option<&'static str> {
    Some(match name {
        "ediamond" => "odiamond",
        "open" => "vee",
        "halfopen" => "lvee",
        "empty" => "onormal",
        "invempty" => "oinv",
        "invdot" => "invdot",
        "invodot" => "invodot",
        _ => return None,
    })
}

/// 矢じりの名前を読む。空の並びは矢じりなし。
pub fn parse(name: &str) -> Vec<Part> {
    let lower = name.trim().to_ascii_lowercase();
    let lower = legacy(&lower).map(str::to_owned).unwrap_or(lower);
    if lower == "none" || lower.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut rest = lower.as_str();
    while !rest.is_empty() && out.len() < 4 {
        let mut open = false;
        let mut half = Half::Both;
        loop {
            // 修飾は形の名前の前だけ（`o` で始まる名前はない）
            if let Some(r) = rest.strip_prefix('o') {
                open = true;
                rest = r;
            } else if let Some(r) = rest.strip_prefix('l') {
                half = Half::Left;
                rest = r;
            } else if let Some(r) = rest.strip_prefix('r') {
                half = Half::Right;
                rest = r;
            } else {
                break;
            }
        }
        match NAMES.iter().find(|(n, _)| rest.starts_with(n)) {
            Some((n, glyph)) => {
                out.push(Part {
                    glyph: *glyph,
                    open,
                    half,
                });
                rest = &rest[n.len()..];
            }
            None => {
                // 知らない名前は normal
                return vec![Part {
                    glyph: Glyph::Normal,
                    open: false,
                    half: Half::Both,
                }];
            }
        }
    }
    // 終わりの隙間は描かない
    while out.last().is_some_and(|p| p.glyph == Glyph::Gap) {
        out.pop();
    }
    out
}

fn factor(g: Glyph) -> f64 {
    match g {
        Glyph::Tee => 0.5,
        Glyph::Diamond => 1.2,
        Glyph::Dot => 0.8,
        Glyph::Gap => 0.5,
        _ => 1.0,
    }
}

/// 矢じり全体の長さ。
pub fn length(parts: &[Part], size: f64) -> f64 {
    parts.iter().map(|p| factor(p.glyph) * LENGTH * size).sum()
}

#[allow(clippy::too_many_arguments)]
/// 矢じりを描く。先は (x, y)、(ux, uy) は先へ向かう単位の向き。`color` は線と塗りの色（属性の値）、`pen` は線の
/// 太さ、`bg` は開いた形の塗り。
pub fn draw(
    parts: &[Part],
    x: f64,
    y: f64,
    ux: f64,
    uy: f64,
    size: f64,
    color: &str,
    pen: f64,
) -> String {
    let mut out = String::new();
    // 先の向きの左の法線（画面で）
    let (nx, ny) = (uy, -ux);
    let mut tip = (x, y);
    let pt = |b: f64, a: f64, tip: (f64, f64)| -> String {
        // 先から b だけ戻り、法線の向きに a
        format!(
            "{},{}",
            num(tip.0 - ux * b + nx * a),
            num(tip.1 - uy * b + ny * a)
        )
    };
    let stroke = format!(
        " stroke=\"{color}\"{}",
        if (pen - 1.0).abs() > 1e-9 {
            format!(" stroke-width=\"{}\"", num(pen))
        } else {
            String::new()
        }
    );
    for part in parts {
        let len = factor(part.glyph) * LENGTH * size;
        let w = 0.35 * LENGTH * size;
        let fill = if part.open {
            " fill=\"none\"".to_owned()
        } else {
            format!(" fill=\"{color}\"")
        };
        let (lw, rw) = match part.half {
            Half::Both => (w, -w),
            Half::Left => (w, 0.0),
            Half::Right => (0.0, -w),
        };
        match part.glyph {
            Glyph::Normal => out.push_str(&format!(
                "<polygon points=\"{} {} {}\"{fill}{stroke}/>",
                pt(0.0, 0.0, tip),
                pt(len, lw, tip),
                pt(len, rw, tip)
            )),
            Glyph::Inv => out.push_str(&format!(
                "<polygon points=\"{} {} {}\"{fill}{stroke}/>",
                pt(len, 0.0, tip),
                pt(0.0, lw, tip),
                pt(0.0, rw, tip)
            )),
            Glyph::Vee => out.push_str(&format!(
                "<polygon points=\"{} {} {} {}\"{fill}{stroke}/>",
                pt(0.0, 0.0, tip),
                pt(len, lw, tip),
                pt(len * 0.75, 0.0, tip),
                pt(len, rw, tip)
            )),
            Glyph::Crow => out.push_str(&format!(
                "<polygon points=\"{} {} {} {}\"{fill}{stroke}/>",
                pt(len, 0.0, tip),
                pt(0.0, lw, tip),
                pt(len * 0.25, 0.0, tip),
                pt(0.0, rw, tip)
            )),
            Glyph::Diamond => out.push_str(&format!(
                "<polygon points=\"{} {} {} {}\"{fill}{stroke}/>",
                pt(0.0, 0.0, tip),
                pt(len / 2.0, lw / 1.2, tip),
                pt(len, 0.0, tip),
                pt(len / 2.0, rw / 1.2, tip)
            )),
            Glyph::Box => {
                let s = len * 0.8;
                out.push_str(&format!(
                    "<polygon points=\"{} {} {} {}\"{fill}{stroke}/>",
                    pt(0.0, lw * 0.8, tip),
                    pt(s, lw * 0.8, tip),
                    pt(s, rw * 0.8, tip),
                    pt(0.0, rw * 0.8, tip)
                ));
                out.push_str(&format!(
                    "<path d=\"M{}L{}\" fill=\"none\"{stroke}/>",
                    pt(s, 0.0, tip),
                    pt(len, 0.0, tip)
                ));
            }
            Glyph::Tee => {
                out.push_str(&format!(
                    "<polygon points=\"{} {} {} {}\"{fill}{stroke}/>",
                    pt(len * 0.2, lw * 1.4, tip),
                    pt(len * 0.6, lw * 1.4, tip),
                    pt(len * 0.6, rw * 1.4, tip),
                    pt(len * 0.2, rw * 1.4, tip)
                ));
                out.push_str(&format!(
                    "<path d=\"M{}L{}M{}L{}\" fill=\"none\"{stroke}/>",
                    pt(0.0, 0.0, tip),
                    pt(len * 0.2, 0.0, tip),
                    pt(len * 0.6, 0.0, tip),
                    pt(len, 0.0, tip)
                ));
            }
            Glyph::Dot => {
                let r = len / 2.0;
                let (cx, cy) = (tip.0 - ux * r, tip.1 - uy * r);
                match part.half {
                    Half::Both => out.push_str(&format!(
                        "<circle cx=\"{}\" cy=\"{}\" r=\"{}\"{fill}{stroke}/>",
                        num(cx),
                        num(cy),
                        num(r)
                    )),
                    half => {
                        let side = if half == Half::Left { 1.0 } else { -1.0 };
                        out.push_str(&format!(
                            "<path d=\"M{}A{} {} 0 0 {} {}Z\"{fill}{stroke}/>",
                            pt(0.0, 0.0, tip),
                            num(r),
                            num(r),
                            if side > 0.0 { 0 } else { 1 },
                            pt(len, 0.0, tip)
                        ));
                    }
                }
            }
            Glyph::Curve | Glyph::ICurve => {
                // 線の先の弧（curve は先に向かって開く）
                let (a, b) = if part.glyph == Glyph::Curve {
                    (0.0, len)
                } else {
                    (len, 0.0)
                };
                out.push_str(&format!(
                    "<path d=\"M{}L{}M{}Q{} {}\" fill=\"none\"{stroke}/>",
                    pt(0.0, 0.0, tip),
                    pt(len, 0.0, tip),
                    pt(a, lw, tip),
                    pt(b, 0.0, tip),
                    pt(a, rw, tip)
                ));
            }
            Glyph::Gap => {
                out.push_str(&format!(
                    "<path d=\"M{}L{}\" fill=\"none\"{stroke}/>",
                    pt(0.0, 0.0, tip),
                    pt(len, 0.0, tip)
                ));
            }
        }
        tip = (tip.0 - ux * len, tip.1 - uy * len);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glyphs(name: &str) -> Vec<(Glyph, bool, Half)> {
        parse(name)
            .into_iter()
            .map(|p| (p.glyph, p.open, p.half))
            .collect()
    }

    #[test]
    fn reads_arrow_names() {
        assert_eq!(glyphs("normal"), [(Glyph::Normal, false, Half::Both)]);
        assert_eq!(glyphs("onormal"), [(Glyph::Normal, true, Half::Both)]);
        assert_eq!(
            glyphs("lteeoldiamond"),
            [
                (Glyph::Tee, false, Half::Left),
                (Glyph::Diamond, true, Half::Left)
            ]
        );
        assert_eq!(
            glyphs("invdot"),
            [
                (Glyph::Inv, false, Half::Both),
                (Glyph::Dot, false, Half::Both)
            ]
        );
        assert_eq!(glyphs("ediamond"), [(Glyph::Diamond, true, Half::Both)]);
        assert_eq!(glyphs("halfopen"), [(Glyph::Vee, false, Half::Left)]);
        assert_eq!(glyphs("empty"), [(Glyph::Normal, true, Half::Both)]);
        assert_eq!(glyphs("none"), []);
        assert_eq!(
            glyphs("nonenormal"),
            [
                (Glyph::Gap, false, Half::Both),
                (Glyph::Normal, false, Half::Both)
            ]
        );
        assert_eq!(glyphs("whatever"), [(Glyph::Normal, false, Half::Both)]);
        assert_eq!(glyphs("normalnormalnormalnormalnormal").len(), 4);
        assert_eq!(
            glyphs("rcrowicurve"),
            [
                (Glyph::Crow, false, Half::Right),
                (Glyph::ICurve, false, Half::Both)
            ]
        );
    }

    #[test]
    fn measures_and_draws() {
        let p = parse("dotnormal");
        assert!((length(&p, 1.0) - 18.0).abs() < 1e-9);
        assert!((length(&p, 2.0) - 36.0).abs() < 1e-9);
        let svg = draw(&p, 0.0, 0.0, 1.0, 0.0, 1.0, "red", 1.0);
        assert!(svg.contains("<circle") && svg.contains("<polygon"));
    }
}
