//! 図の `<svg>` の外枠（pele の `src/svg/root.ts`）。

use crate::svg::builder::{esc, esc_text};
use crate::svg::palette::palette_style;
use crate::svg::theme::{RADIUS, ROOT_STYLE, font_style};
use crate::types::RenderOptions;

/// 太字の太さ。その太さの文字がある図にだけ置く。
const WEIGHTS: [(&str, &str); 3] = [("tw", "title"), ("hw", "heading"), ("w", "bold")];

#[derive(Default)]
pub struct Accessible<'a> {
    pub acc_title: Option<&'a str>,
    pub acc_descr: Option<&'a str>,
}

/// `content` が使う `var(--_r|tw|hw|w)`。
fn used(content: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    let mut from = 0;
    while let Some(k) = content[from..].find("var(--_") {
        let at = from + k + 7;
        let rest = &content[at..];
        for name in ["r", "tw", "hw", "w"] {
            if rest.starts_with(name) && rest[name.len()..].starts_with(')') {
                if !out.contains(&name) {
                    out.push(name);
                }
                break;
            }
        }
        if out.len() == 4 {
            break;
        }
        from = at;
    }
    out
}

pub fn svg_document(
    type_: &str,
    width: f64,
    height: f64,
    font_size: f64,
    options: &RenderOptions,
    acc: &Accessible,
    content: &str,
) -> String {
    let prefix = esc(options.id_prefix.as_deref().unwrap_or("pele"));
    let mut head = String::new();
    let mut aria = String::new();
    if let Some(t) = acc.acc_title.filter(|t| !t.is_empty()) {
        head.push_str(&format!(
            "<title id=\"{prefix}-title\">{}</title>",
            esc_text(t)
        ));
        aria.push_str(&format!(" aria-labelledby=\"{prefix}-title\""));
    }
    if let Some(d) = acc.acc_descr.filter(|d| !d.is_empty()) {
        head.push_str(&format!(
            "<desc id=\"{prefix}-desc\">{}</desc>",
            esc_text(d)
        ));
        aria.push_str(&format!(" aria-describedby=\"{prefix}-desc\""));
    }
    let fit = if options.responsive == Some(false) {
        ""
    } else {
        "max-width:100%;height:auto;"
    };
    let used = used(content);
    let mut weights = if used.contains(&"r") {
        format!(";--_r:{RADIUS}")
    } else {
        String::new()
    };
    for (alias, name) in WEIGHTS {
        if used.contains(&alias) {
            weights.push_str(&format!(";--_{alias}:var(--pele-{name}-weight,bold)"));
        }
    }
    let w = crate::js::fmt(width);
    let h = crate::js::fmt(height);
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" class=\"pele pele-{type_}\" viewBox=\"0 0 {w} {h}\" width=\"{w}\" height=\"{h}\" style=\"{fit}{ROOT_STYLE}{weights}{}{}\" fill=\"var(--_fg)\" role=\"graphics-document document\" aria-roledescription=\"{type_}\"{aria}>{head}{content}</svg>",
        esc(&font_style(
            font_size,
            options.font_family.as_deref(),
            options.font_family_mono.as_deref()
        )),
        palette_style()
    )
}
