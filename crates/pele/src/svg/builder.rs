//! SVG の文字列を組む小物（pele の `src/svg/builder.ts`）: XML の逃がし、座標の数、ラベルの `<text>`。

use std::fmt::Write as _;

use crate::js;
use crate::text::entities::decode_entities;
use crate::text::label::Label;
use crate::text::measurer::style as st;
use crate::types::{IconResolver, MathMarkup};

/// XML に書けない文字（と DEL）。Rust の文字列に対のないサロゲートはない。
pub fn invalid_xml(c: char) -> bool {
    matches!(c, '\u{0}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' | '\u{7f}' | '\u{fffe}' | '\u{ffff}')
}

/// XML の文字として逃がし、XML に書けない文字と DEL を捨てる。
pub fn esc(text: &str) -> String {
    if !text
        .chars()
        .any(|c| matches!(c, '&' | '<' | '>' | '"') || invalid_xml(c))
    {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c if invalid_xml(c) => {}
            c => out.push(c),
        }
    }
    out
}

/// 文字としてだけ読まれるもの（ツールチップ、名前、`data-id`）を逃がす。符号は先に文字に戻す。書く前に確かめた
/// 値（スタイル、色、URL）は `esc` を通す（確かめたものがそのまま書かれるように）。
pub fn esc_text(text: &str) -> String {
    esc(&decode_entities(text))
}

/// 座標の小数 2 桁。`Math.round(value * 100)` を 100 で割った数を、JS と同じ文字にする。
pub fn num(value: f64) -> String {
    let mut s = String::new();
    write_num(&mut s, value);
    s
}

pub fn write_num(out: &mut String, value: f64) {
    let n = js::round(value * 100.0);
    if n == 0.0 {
        out.push('0');
        return;
    }
    let (neg, m) = if n > 0.0 && n < 1e11 {
        (false, n)
    } else if n < 0.0 && n > -1e11 {
        (true, -n)
    } else {
        js::write_num(out, n / 100.0);
        return;
    };
    let m = m as i64;
    let q = m / 100;
    let r = m - q * 100;
    if neg {
        out.push('-');
    }
    let _ = write!(out, "{q}");
    if r != 0 {
        if r % 10 == 0 {
            let _ = write!(out, ".{}", r / 10);
        } else {
            let _ = write!(out, ".{r:02}");
        }
    }
}

/// 測ったラベルを、(cx, cy) を中心にした SVG の文字として書く。
pub fn label_svg(
    label: &Label,
    cx: f64,
    cy: f64,
    attrs: &str,
    icons: Option<&IconResolver>,
) -> String {
    let lines = &label.lines;
    if lines.is_empty() {
        return String::new();
    }
    let lh = label.line_height;
    let heights = &label.heights;
    let shift = label.size * 0.35;
    let mut top = cy - label.height / 2.0;

    if lines.len() == 1 && lines[0].len() == 1 {
        let only = &lines[0][0];
        if only.icon.is_none() && only.math.is_none() && only.style == 0 {
            return format!(
                "<text{attrs} x=\"{}\" y=\"{}\" text-anchor=\"middle\">{}</text>",
                num(cx),
                num(top + lh / 2.0 + shift),
                esc(&only.text)
            );
        }
    }

    let shared = shared_style(label);
    let given = attrs;
    let mut math: Option<MathLook> = None;
    let attrs = text_style(attrs, shared);
    let mut body = String::new();
    let mut extra = String::new();
    for (i, line) in lines.iter().enumerate() {
        let h = match heights {
            Some(hs) => hs[i],
            None => lh,
        };
        let mid = top + h / 2.0;
        let y = mid + shift;
        let placed = line.iter().any(|s| s.icon.is_some() || s.math.is_some());
        if placed {
            let mut x = cx - label.widths[i] / 2.0;
            for span in line {
                if let Some(m) = &span.math {
                    let bx = format!(
                        "x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"",
                        num(x),
                        num(mid - m.height() / 2.0),
                        num(m.width()),
                        num(m.height())
                    );
                    let look = math.get_or_insert_with(|| math_look(given));
                    match &look.halo {
                        None => extra.push_str(&math_svg(m, &bx, label.size, &look.attrs)),
                        Some(halo) => {
                            let _ = write!(extra, "<rect {bx} rx=\"2\" fill=\"{halo}\"/>");
                        }
                    }
                } else if let Some(icon) = &span.icon {
                    let side = label.size * 1.1;
                    let inner = icons.and_then(|f| f(icon)).unwrap_or_default();
                    let _ = write!(
                        extra,
                        "<svg class=\"pele-icon\" data-icon=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\">{}</svg>",
                        esc_text(icon),
                        num(x),
                        num(mid - side / 2.0),
                        num(side),
                        num(side),
                        inner
                    );
                } else if !span.text.is_empty() {
                    let _ = write!(
                        body,
                        "<tspan x=\"{}\" y=\"{}\" text-anchor=\"start\"{}>{}</tspan>",
                        num(x),
                        num(y),
                        span_style(span.style & !shared),
                        esc(&span.text)
                    );
                }
                x += span.width;
            }
        } else {
            for (j, span) in line.iter().enumerate() {
                let pos = if j == 0 {
                    format!(" x=\"{}\" y=\"{}\"", num(cx), num(y))
                } else {
                    String::new()
                };
                let _ = write!(
                    body,
                    "<tspan{pos}{}>{}</tspan>",
                    span_style(span.style & !shared),
                    esc(&span.text)
                );
            }
            if line.is_empty() {
                let _ = write!(body, "<tspan x=\"{}\" y=\"{}\"> </tspan>", num(cx), num(y));
            }
        }
        top += h;
    }
    format!("<text{attrs} text-anchor=\"middle\" xml:space=\"preserve\">{body}</text>{extra}")
}

/// `attrs` の中の ` name="…"` の値。
fn attr_value<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let at = attrs.find(&key)? + key.len();
    let end = attrs[at..].find('"')? + at;
    Some(&attrs[at..end])
}

/// スタイルの `(?:^|;)\s*prop:([^;]*)` の値。
fn style_decl<'a>(style: &'a str, prop: &str) -> Option<&'a str> {
    let mut i = 0;
    let b = style.as_bytes();
    while i <= b.len() {
        if i == 0 || b[i - 1] == b';' {
            let rest = &style[i..];
            let t = rest.trim_start_matches(crate::util::chars::is_space_char);
            if let Some(v) = t.strip_prefix(prop).and_then(|v| v.strip_prefix(':')) {
                return Some(v.split(';').next().unwrap());
            }
        }
        i += 1;
    }
    None
}

/// 数式は色と不透明度だけを写し、測った大きさを保つ。題の縁取りでは、式の後ろの辺を隠す無地の当て布にする。
struct MathLook {
    attrs: String,
    halo: Option<String>,
}

fn math_look(attrs: &str) -> MathLook {
    let style = attr_value(attrs, "style").unwrap_or("");
    let fill = js::trim(
        style_decl(style, "fill")
            .or_else(|| attr_value(attrs, "fill"))
            .unwrap_or(""),
    )
    .to_string();
    if fill == "none" {
        return MathLook {
            attrs: String::new(),
            halo: Some(attr_value(attrs, "stroke").unwrap_or("none").to_string()),
        };
    }
    let opacity = style_decl(style, "opacity").map(|o| js::trim(o).to_string());
    let mut a = String::new();
    if !fill.is_empty() {
        let _ = write!(a, " color=\"{fill}\"");
    }
    if let Some(o) = opacity.filter(|o| !o.is_empty()) {
        let _ = write!(a, " opacity=\"{o}\"");
    }
    MathLook {
        attrs: a,
        halo: None,
    }
}

/// 組版の結果は信じる。
fn math_svg(math: &MathMarkup, bx: &str, size: f64, attrs: &str) -> String {
    match math {
        MathMarkup::Svg { svg, .. } => {
            format!("<svg class=\"pele-math\" {bx} overflow=\"visible\"{attrs}>{svg}</svg>")
        }
        MathMarkup::Html { html, .. } => format!(
            "<foreignObject class=\"pele-math\" {bx} overflow=\"visible\"{attrs}><div xmlns=\"http://www.w3.org/1999/xhtml\" style=\"display:flex;align-items:center;justify-content:center;width:100%;height:100%;font-size:{}px;line-height:normal;white-space:nowrap\">{html}</div></foreignObject>",
            num(size)
        ),
    }
}

const BOLD: &str = " font-weight=\"var(--_w)\"";
const ITALIC: &str = " font-style=\"italic\"";
const UNDERLINE: &str = " text-decoration=\"underline\"";
const MONO: &str = " font-family=\"var(--_fm)\"";
const SUB: &str = " baseline-shift=\"sub\" font-size=\"smaller\"";
const SUP: &str = " baseline-shift=\"super\" font-size=\"smaller\"";

pub fn span_style(style: u32) -> String {
    let mut s = String::new();
    if style & st::BOLD != 0 {
        s.push_str(BOLD);
    }
    if style & st::ITALIC != 0 {
        s.push_str(ITALIC);
    }
    if style & st::MONO != 0 {
        s.push_str(MONO);
    }
    if style & st::UNDERLINE != 0 {
        s.push_str(UNDERLINE);
    }
    if style & st::SUB != 0 {
        s.push_str(SUB);
    } else if style & st::SUP != 0 {
        s.push_str(SUP);
    }
    s
}

/// ラベルのどの span にもある強調。CSS の規則が届く `<text>` に 1 度だけ書く。
pub fn shared_style(label: &Label) -> u32 {
    let mut shared = st::BOLD | st::ITALIC | st::MONO;
    let mut any = false;
    for line in &label.lines {
        for span in line {
            if span.icon.is_some() || span.text.is_empty() {
                continue;
            }
            shared &= span.style;
            any = true;
        }
    }
    if any { shared } else { 0 }
}

/// その強調を `<text>` の属性に足す。属性が自分の太さなどを持つときは足さない。
pub fn text_style(attrs: &str, shared: u32) -> String {
    let mut s = attrs.to_string();
    if shared & st::BOLD != 0 && !attrs.contains(" font-weight=\"") {
        s.push_str(BOLD);
    }
    if shared & st::ITALIC != 0 && !attrs.contains(" font-style=\"") {
        s.push_str(ITALIC);
    }
    if shared & st::MONO != 0 && !attrs.contains(" font-family=\"") {
        s.push_str(MONO);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(num(0.0), "0");
        assert_eq!(num(1.005), "1");
        assert_eq!(num(-2.5), "-2.5");
        assert_eq!(num(5.12345), "5.12");
        assert_eq!(num(-0.004), "0");
        assert_eq!(num(12.30), "12.3");
        assert_eq!(num(0.07), "0.07");
        assert_eq!(num(-0.07), "-0.07");
    }
}
