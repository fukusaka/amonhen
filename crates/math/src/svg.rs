//! RaTeX の描く物の並び（`DisplayList`）を SVG にする。字形は KaTeX のフォントの輪郭を `<defs>` の `<path>` に一度だけ
//! 書いて `<use>` で置き（OS のフォントは探さない）、KaTeX のフォントにない文字（`\text{日本語}` など）は `<text>` にして FE の書体で描かせる。座標は 1 em を
//! 1000 にした単位で書く（KaTeX のフォントの 1 em も 1000 なので、輪郭の数をそのまま使える）。
//!
//! 既定の色（`\color` で決めていない所）は書かず、根の `fill` の CSS の変数（`--math-fg`）を継がせる。FE は描くときに
//! この変数に文字の色を当てる（図と同じ `colorize` の道）。`\color{…}` で決めた色はその色のまま書く。

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex, OnceLock};

use ratex_font::FontId;
use ratex_types::color::Color;
use ratex_types::display_item::{DisplayItem, DisplayList};
use ratex_types::path_command::PathCommand;

use crate::ttf;

/// 1 em の単位の数。
const UNIT: f64 = 1000.0;
/// SVG の `width`・`height` に書く、1 em の画素（FE は JSON の大きさで描くので、目安）。
const PX_PER_EM: f64 = 16.0;
/// 線だけで描く形の線の太さ（em）。
const STROKE_EM: f64 = 0.04;

/// 既定の色を表す目印（`LayoutOptions::with_color` に渡し、出てきたらその色を書かずに根から継がせる）。`\color` で
/// 書ける色と重ならない半端な値にする。
pub(crate) const DEFAULT_MARK: Color = Color {
    r: 0.012_345,
    g: 0.023_456,
    b: 0.034_567,
    a: 0.987_654,
};

/// 根の `fill`（FE が `--math-fg` を当てる。当てなければ黒）。
pub const ROOT_FILL: &str = "var(--math-fg,#000000)";

/// 描いた SVG と、その外枠の大きさ（em）。
pub(crate) struct Written {
    pub svg: String,
    pub width: f64,
    pub height: f64,
    pub depth: f64,
}

/// `value` を小数 1 桁までで書く（整数なら小数点なし）。
pub(crate) fn num(out: &mut String, value: f64) {
    let tenths = (value * 10.0).round();
    if !tenths.is_finite() || tenths == 0.0 {
        out.push('0');
        return;
    }
    if tenths % 10.0 == 0.0 {
        let _ = write!(out, "{}", (tenths / 10.0) as i64);
    } else {
        let _ = write!(out, "{:.1}", tenths / 10.0);
    }
}

/// 変換の倍率（小数 4 桁まで、末尾の 0 は落とす）。
fn scale(out: &mut String, value: f64) {
    let start = out.len();
    let _ = write!(out, "{value:.4}");
    while out.ends_with('0') {
        out.pop();
    }
    if out.ends_with('.') {
        out.pop();
    }
    if out.len() == start || &out[start..] == "-0" {
        out.truncate(start);
        out.push('0');
    }
}

/// KaTeX のフォント（名前 → 読んだもの）。読めないものは None。
fn fonts() -> &'static HashMap<FontId, Option<ttf::Font>> {
    static FONTS: OnceLock<HashMap<FontId, Option<ttf::Font>>> = OnceLock::new();
    FONTS.get_or_init(|| {
        use FontId::*;
        [
            AmsRegular,
            CaligraphicRegular,
            FrakturRegular,
            FrakturBold,
            MainBold,
            MainBoldItalic,
            MainItalic,
            MainRegular,
            MathBoldItalic,
            MathItalic,
            SansSerifBold,
            SansSerifItalic,
            SansSerifRegular,
            ScriptRegular,
            Size1Regular,
            Size2Regular,
            Size3Regular,
            Size4Regular,
            TypewriterRegular,
        ]
        .into_iter()
        .map(|id| {
            let name = format!("KaTeX_{}.ttf", id.as_str());
            let font = match ratex_katex_fonts::ttf_bytes(&name) {
                Some(std::borrow::Cow::Borrowed(bytes)) => ttf::Font::parse(bytes),
                _ => None,
            };
            (id, font)
        })
        .collect()
    })
}

/// 字形の輪郭（フォントと字形の番号ごとに一度だけ作る）。
type Outlines = Mutex<HashMap<(FontId, u16), Option<Arc<ttf::Outline>>>>;

fn outline(font_id: FontId, font: &ttf::Font, glyph: u16) -> Option<Arc<ttf::Outline>> {
    static CACHE: OnceLock<Outlines> = OnceLock::new();
    let cache = CACHE.get_or_init(Outlines::default);
    if let Some(hit) = cache.lock().ok()?.get(&(font_id, glyph)) {
        return hit.clone();
    }
    let made = font.outline(glyph).map(Arc::new);
    if let Ok(mut map) = cache.lock() {
        map.insert((font_id, glyph), made.clone());
    }
    made
}

/// 外枠（SVG の単位）。
#[derive(Clone, Copy)]
struct Bounds {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

impl Bounds {
    fn add(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) {
        if [x0, y0, x1, y1].iter().all(|v| v.is_finite()) {
            self.x0 = self.x0.min(x0.min(x1));
            self.y0 = self.y0.min(y0.min(y1));
            self.x1 = self.x1.max(x0.max(x1));
            self.y1 = self.y1.max(y0.max(y1));
        }
    }
}

/// 色の属性（既定の色なら、`fill` は書かずに継がせ、`stroke` は根と同じ変数）。
fn paint(out: &mut String, attr: &str, color: &Color) {
    if *color == DEFAULT_MARK {
        if attr != "fill" {
            let _ = write!(out, " {attr}=\"{ROOT_FILL}\"");
        }
        return;
    }
    let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let _ = write!(
        out,
        " {attr}=\"#{:02x}{:02x}{:02x}\"",
        c(color.r),
        c(color.g),
        c(color.b)
    );
    let alpha = if color.a.is_finite() {
        color.a.clamp(0.0, 1.0)
    } else {
        1.0
    };
    if alpha < 1.0 {
        let _ = write!(out, " {attr}-opacity=\"{:.3}\"", alpha);
    }
}

fn escape(out: &mut String, ch: char) {
    match ch {
        '&' => out.push_str("&amp;"),
        '<' => out.push_str("&lt;"),
        '>' => out.push_str("&gt;"),
        '"' => out.push_str("&quot;"),
        c if c.is_control() => out.push('\u{fffd}'),
        c => out.push(c),
    }
}

/// `list` を SVG にする。描く物の数 `list.items` は呼ぶ側が先に絞っておく。
pub(crate) fn write(list: &DisplayList) -> Written {
    let fonts = fonts();
    let baseline = list.height * UNIT;
    let mut bounds = Bounds {
        x0: 0.0,
        y0: 0.0,
        x1: (list.width * UNIT).max(1.0),
        y1: ((list.height + list.depth) * UNIT).max(1.0),
    };
    let mut body = String::with_capacity(list.items.len() * 80);
    let mut defs = String::new();
    let mut ids: HashMap<(FontId, u16), usize> = HashMap::new();
    for item in &list.items {
        match item {
            DisplayItem::GlyphPath {
                x,
                y,
                scale: s,
                font,
                char_code,
                color,
            } => {
                let (gx, gy) = (x * UNIT, y * UNIT);
                let font_id = FontId::parse(font).unwrap_or(FontId::MainRegular);
                let found = fonts.get(&font_id).and_then(Option::as_ref).and_then(|f| {
                    let ch = ratex_font::katex_ttf_glyph_char(font_id, *char_code);
                    let glyph = f.glyph_id(ch)?;
                    Some((glyph, outline(font_id, f, glyph)?, f.units_per_em()))
                });
                match found {
                    Some((glyph, shape, upm)) => {
                        if shape.d.is_empty() {
                            continue;
                        }
                        let k = s * UNIT / upm;
                        let (bx0, by0, bx1, by1) = shape.bbox;
                        bounds.add(gx + k * bx0, gy - k * by1, gx + k * bx1, gy - k * by0);
                        // 同じ字形は <defs> に一度だけ書き、<use> で置く（行列で同じ字が何千も並んでも大きくならない）
                        let next = ids.len();
                        let id = *ids.entry((font_id, glyph)).or_insert_with(|| {
                            let _ = write!(defs, "<path id=\"g{next}\" d=\"{}\"/>", shape.d);
                            next
                        });
                        let _ = write!(body, "<use xlink:href=\"#g{id}\" transform=\"matrix(");
                        scale(&mut body, k);
                        body.push_str(" 0 0 ");
                        scale(&mut body, -k);
                        body.push(' ');
                        num(&mut body, gx);
                        body.push(' ');
                        num(&mut body, gy);
                        body.push_str(")\"");
                        paint(&mut body, "fill", color);
                        body.push_str("/>");
                    }
                    None => {
                        // KaTeX のフォントにない文字は FE の書体で（幅は RaTeX の見積もりのまま）。
                        let Some(ch) = char::from_u32(*char_code) else {
                            continue;
                        };
                        if ch.is_whitespace() {
                            continue;
                        }
                        let size = s * UNIT;
                        bounds.add(gx, gy - size * 0.9, gx + size, gy + size * 0.2);
                        body.push_str("<text x=\"");
                        num(&mut body, gx);
                        body.push_str("\" y=\"");
                        num(&mut body, gy);
                        body.push_str("\" font-size=\"");
                        num(&mut body, size);
                        body.push_str("\" font-family=\"sans-serif\"");
                        paint(&mut body, "fill", color);
                        body.push('>');
                        escape(&mut body, ch);
                        body.push_str("</text>");
                    }
                }
            }
            DisplayItem::Line {
                x,
                y,
                width,
                thickness,
                color,
                dashed,
            } => {
                let (lx, ly, w) = (x * UNIT, y * UNIT, width * UNIT);
                let t = (thickness * UNIT).max(0.5);
                bounds.add(lx, ly - t / 2.0, lx + w, ly + t / 2.0);
                if *dashed {
                    body.push_str("<line x1=\"");
                    num(&mut body, lx);
                    body.push_str("\" y1=\"");
                    num(&mut body, ly);
                    body.push_str("\" x2=\"");
                    num(&mut body, lx + w);
                    body.push_str("\" y2=\"");
                    num(&mut body, ly);
                    body.push_str("\" stroke-width=\"");
                    num(&mut body, t);
                    body.push_str("\" stroke-dasharray=\"");
                    num(&mut body, t * 3.0);
                    body.push('"');
                    paint(&mut body, "stroke", color);
                    body.push_str("/>");
                } else {
                    rect(&mut body, lx, ly - t / 2.0, w, t, color);
                }
            }
            DisplayItem::Rect {
                x,
                y,
                width,
                height,
                color,
            } => {
                let (rx, ry, w, h) = (x * UNIT, y * UNIT, width * UNIT, height * UNIT);
                bounds.add(rx, ry, rx + w, ry + h);
                rect(&mut body, rx, ry, w, h, color);
            }
            DisplayItem::Path {
                x,
                y,
                commands,
                fill,
                color,
            } => {
                let (ox, oy) = (x * UNIT, y * UNIT);
                if *fill {
                    // 輪郭ごとに別の <path> に（重なる輪郭で穴が開かないよう。ratex-svg と同じ）
                    let mut start = 0;
                    for i in 1..=commands.len() {
                        if i == commands.len() || matches!(commands[i], PathCommand::MoveTo { .. })
                        {
                            path(
                                &mut body,
                                &mut bounds,
                                ox,
                                oy,
                                &commands[start..i],
                                true,
                                color,
                            );
                            start = i;
                        }
                    }
                } else {
                    path(&mut body, &mut bounds, ox, oy, commands, false, color);
                }
            }
        }
    }
    let (w, h) = (bounds.x1 - bounds.x0, bounds.y1 - bounds.y0);
    let mut svg = String::with_capacity(defs.len() + body.len() + 240);
    svg.push_str(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"",
    );
    num(&mut svg, w / UNIT * PX_PER_EM);
    svg.push_str("\" height=\"");
    num(&mut svg, h / UNIT * PX_PER_EM);
    svg.push_str("\" viewBox=\"");
    num(&mut svg, bounds.x0);
    svg.push(' ');
    num(&mut svg, bounds.y0);
    svg.push(' ');
    num(&mut svg, w);
    svg.push(' ');
    num(&mut svg, h);
    let _ = write!(svg, "\" fill=\"{ROOT_FILL}\">");
    if !defs.is_empty() {
        svg.push_str("<defs>");
        svg.push_str(&defs);
        svg.push_str("</defs>");
    }
    svg.push_str(&body);
    svg.push_str("</svg>");
    Written {
        svg,
        width: w / UNIT,
        height: (baseline - bounds.y0) / UNIT,
        depth: (bounds.y1 - baseline) / UNIT,
    }
}

fn rect(out: &mut String, x: f64, y: f64, w: f64, h: f64, color: &Color) {
    out.push_str("<rect x=\"");
    num(out, x);
    out.push_str("\" y=\"");
    num(out, y);
    out.push_str("\" width=\"");
    num(out, w.max(0.0));
    out.push_str("\" height=\"");
    num(out, h.max(0.0));
    out.push('"');
    paint(out, "fill", color);
    out.push_str("/>");
}

fn path(
    out: &mut String,
    bounds: &mut Bounds,
    ox: f64,
    oy: f64,
    commands: &[PathCommand],
    fill: bool,
    color: &Color,
) {
    if commands.is_empty() {
        return;
    }
    let mut d = String::with_capacity(commands.len() * 16);
    let mut point = |d: &mut String, x: f64, y: f64| {
        let (px, py) = (ox + x * UNIT, oy + y * UNIT);
        bounds.add(px, py, px, py);
        num(d, px);
        d.push(' ');
        num(d, py);
    };
    for c in commands {
        if !d.is_empty() {
            d.push(' ');
        }
        match *c {
            PathCommand::MoveTo { x, y } => {
                d.push('M');
                point(&mut d, x, y);
            }
            PathCommand::LineTo { x, y } => {
                d.push('L');
                point(&mut d, x, y);
            }
            PathCommand::QuadTo { x1, y1, x, y } => {
                d.push('Q');
                point(&mut d, x1, y1);
                d.push(' ');
                point(&mut d, x, y);
            }
            PathCommand::CubicTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => {
                d.push('C');
                point(&mut d, x1, y1);
                d.push(' ');
                point(&mut d, x2, y2);
                d.push(' ');
                point(&mut d, x, y);
            }
            PathCommand::Close => d.push('Z'),
        }
    }
    out.push_str("<path d=\"");
    out.push_str(&d);
    out.push('"');
    if fill {
        paint(out, "fill", color);
    } else {
        out.push_str(" fill=\"none\" stroke-width=\"");
        num(out, STROKE_EM * UNIT);
        out.push_str("\" stroke-linecap=\"round\" stroke-linejoin=\"round\"");
        paint(out, "stroke", color);
    }
    out.push_str("/>");
}
