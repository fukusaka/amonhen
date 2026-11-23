//! テーマの色とスタイル（pele の `src/svg/theme.ts`）。

use crate::js;
use crate::svg::builder::{esc, invalid_xml};
use crate::util::chars::{is_space_char, is_word_char};

/// ルートの要素に 1 度だけ置く私的な別名。使うたびに短く、公開の `--pele-*` は既定の値を保つ。文字の色も置く:
/// アイコンは currentColor で描くので、置かなければページが図の入れ物に与える色になる。
pub const ROOT_STYLE: &str = concat!(
    "--_bg:var(--pele-bg,#fff);--_fg:var(--pele-fg,#1f1f1f);--_m:var(--pele-muted,#6e6e6e);",
    "--_l:var(--pele-line,#1f1f1f);--_s:var(--pele-surface,#f3f3f3);--_a:var(--pele-surface-alt,#e6e6e6);",
    "--_b:var(--pele-border,#8a8a8a);--_c:var(--pele-accent,#5b7bd5);color:var(--_fg)"
);

pub const FONT_MONO: &str = "var(--_fm)";
pub const RADIUS: &str = "var(--pele-radius,4px)";

/// 古いブラウザのため、rx の変数は CSS で。図のスタイルが上書きできるよう、既定を先に置く。
pub const ROUNDED: &str = " style=\"rx:var(--_r)\"";

/// `resolve_style` の形のスタイルが塗りを決めるか（`/[";]fill:/`）。
pub fn sets_fill(shape: &str) -> bool {
    shape.contains("\"fill:") || shape.contains(";fill:")
}

pub fn rounded(attrs: &str) -> String {
    match attrs.find(" style=\"") {
        None => format!("{attrs}{ROUNDED}"),
        Some(at) => format!("{}rx:var(--_r);{}", &attrs[..at + 8], &attrs[at + 8..]),
    }
}

/// 図が「重要」と印をつけたものの色。テーマが系列の色を好きな順に並べられるよう、系列の色ではない。
pub const CRITICAL: &str = "var(--pele-critical,#e45756)";

fn family_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-'
}

/// 書体の名前の並び（引用符つきでもなしでも）。置かれる宣言を終わらせうるものはない。
fn valid_families(list: &str) -> bool {
    let s: Vec<char> = list.chars().collect();
    let n = s.len();
    let mut i = 0;
    let ws = |i: &mut usize| {
        while *i < n && is_space_char(s[*i]) {
            *i += 1;
        }
    };
    ws(&mut i);
    loop {
        // FAMILY
        if i >= n {
            return false;
        }
        if s[i] == '"' || s[i] == '\'' {
            let q = s[i];
            i += 1;
            while i < n && s[i] != q {
                if "\"'\\\n<>{}();:@".contains(s[i]) {
                    return false;
                }
                i += 1;
            }
            if i >= n {
                return false;
            }
            i += 1;
        } else if family_name_char(s[i]) {
            i += 1;
            while i < n && (family_name_char(s[i]) || s[i] == ' ' || s[i] == '.') {
                i += 1;
            }
        } else {
            return false;
        }
        let save = i;
        ws(&mut i);
        if i >= n {
            return true;
        }
        if s[i] == ',' {
            i += 1;
            ws(&mut i);
            continue;
        }
        let _ = save;
        return false;
    }
}

fn families<'a>(list: Option<&'a str>, generic: &'a str) -> &'a str {
    match list {
        Some(l) if !l.is_empty() && valid_families(l) => l,
        _ => generic,
    }
}

/// ラベルを測った書体を描く書体にする（ページが別の書体を名指ししなければ）。文字が受け継ぐほかのものは
/// 測っていないので固定する。
pub fn font_style(size: f64, family: Option<&str>, mono: Option<&str>) -> String {
    format!(
        ";--_fm:var(--pele-font-mono,{});font:{}px sans-serif;letter-spacing:normal;word-spacing:normal;text-transform:none;direction:ltr;font-family:var(--pele-font,{})",
        families(mono, "monospace"),
        js::fmt(size),
        families(family, "sans-serif")
    )
}

const SHAPE_PROPS: &[&str] = &[
    "fill",
    "fill-opacity",
    "stroke",
    "stroke-width",
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-opacity",
    "opacity",
    "rx",
    "ry",
];

const TEXT_PROPS: &[&str] = &[
    "color",
    "font-size",
    "font-family",
    "font-weight",
    "font-style",
    "text-decoration",
    "letter-spacing",
    "word-spacing",
    "opacity",
];

/// `/url\s*\(|expression\s*\(|javascript:|[<>{};\\]|@import|image-set|\/\*/i`
fn unsafe_value(v: &str) -> bool {
    let l = v.to_lowercase();
    let fn_call = |word: &str| {
        let mut from = 0;
        while let Some(k) = l[from..].find(word) {
            let mut p = from + k + word.len();
            let rest = &l[p..];
            let skipped = rest.len() - rest.trim_start_matches(is_space_char).len();
            p += skipped;
            if l[p..].starts_with('(') {
                return true;
            }
            from += k + 1;
        }
        false
    };
    fn_call("url")
        || fn_call("expression")
        || l.contains("javascript:")
        || l.contains(['<', '>', '{', '}', ';', '\\'])
        || l.contains("@import")
        || l.contains("image-set")
        || l.contains("/*")
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResolvedStyle {
    pub shape: String,
    pub line: String,
    pub text: String,
    pub stroke: Option<String>,
    pub bold: bool,
    pub italic: bool,
    pub font_size: Option<f64>,
}

/// 入れた順を保つ小さな表（JS の `Map`）。
#[derive(Default)]
struct Ordered(Vec<(String, String)>);

impl Ordered {
    fn set(&mut self, k: &str, v: &str) {
        match self.0.iter_mut().find(|(kk, _)| kk == k) {
            Some(slot) => slot.1 = v.to_string(),
            None => self.0.push((k.to_string(), v.to_string())),
        }
    }

    fn get(&self, k: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(kk, _)| kk == k)
            .map(|(_, v)| v.as_str())
    }
}

/// `value.replace(/!important/gi, '')`
fn drop_important(v: &str) -> String {
    let mut out = String::new();
    let mut rest = v;
    loop {
        let lower = rest.to_ascii_lowercase();
        match lower.find("!important") {
            Some(k) => {
                out.push_str(&rest[..k]);
                rest = &rest[k + 10..];
            }
            None => {
                out.push_str(rest);
                return out;
            }
        }
    }
}

/// `/^[\d.]+(px)?$/`
fn plain_px(s: &str) -> bool {
    let body = s.strip_suffix("px").unwrap_or(s);
    !body.is_empty() && body.bytes().all(|c| c.is_ascii_digit() || c == b'.')
}

/// Mermaid のスタイルの宣言を、形に効くものと文字に効くものに分ける。
pub fn resolve_style(declarations: &[String]) -> ResolvedStyle {
    if declarations.is_empty() {
        return ResolvedStyle::default();
    }
    let mut shape = Ordered::default();
    let mut text = Ordered::default();
    for decl in declarations {
        // JS の位置は UTF-16 だが、`:` は ASCII なのでバイトで探してよい。
        let Some(at) = decl.find(':').filter(|&a| a > 0) else {
            continue;
        };
        let prop = js::trim(&decl[..at]).to_lowercase();
        let value = drop_important(&decl[at + 1..]);
        let value = js::trim(&value);
        if value.is_empty() || unsafe_value(value) || value.chars().any(invalid_xml) {
            continue;
        }
        if SHAPE_PROPS.contains(&prop.as_str()) {
            shape.set(&prop, value);
        }
        if TEXT_PROPS.contains(&prop.as_str()) {
            text.set(if prop == "color" { "fill" } else { &prop }, value);
        }
    }
    if shape.0.is_empty() && text.0.is_empty() {
        return ResolvedStyle::default();
    }
    let mut shape_css = String::new();
    let mut line_css = String::new();
    for (prop, value) in &shape.0 {
        shape_css.push_str(&format!("{prop}:{value};"));
        if prop != "fill" && prop != "fill-opacity" {
            line_css.push_str(&format!("{prop}:{value};"));
        }
    }
    let mut text_css = String::new();
    for (prop, value) in &text.0 {
        text_css.push_str(&format!("{prop}:{value};"));
    }
    let weight = text.get("font-weight");
    let size = text.get("font-size");
    let px = size.filter(|s| plain_px(s)).map(js::parse_float);
    let wrap = |css: &str| {
        if css.is_empty() {
            String::new()
        } else {
            format!(" style=\"{}\"", esc(css))
        }
    };
    ResolvedStyle {
        shape: wrap(&shape_css),
        line: wrap(&line_css),
        text: wrap(&text_css),
        stroke: shape.get("stroke").map(str::to_string),
        bold: matches!(weight, Some("bold" | "bolder"))
            || weight.is_some_and(|w| js::number(w) >= 600.0),
        italic: text.get("font-style") == Some("italic"),
        font_size: px.filter(|&p| p > 0.0 && p <= 200.0),
    }
}

/// `raw.replace(/[^\w-]+/g, ' ').trim()` を前に空白をつけて。
pub fn class_names(raw: &str) -> String {
    let mut cleaned = String::new();
    let mut gap = false;
    for c in raw.chars() {
        if is_word_char(c) || c == '-' {
            if gap {
                cleaned.push(' ');
                gap = false;
            }
            cleaned.push(c);
        } else {
            gap = true;
        }
    }
    if gap {
        cleaned.push(' ');
    }
    let cleaned = js::trim(&cleaned);
    if cleaned.is_empty() {
        String::new()
    } else {
        format!(" {cleaned}")
    }
}

const SERIES: [&str; 8] = [
    "#4c78a8", "#f58518", "#54a24b", "#e45756", "#72b7b2", "#eeca3b", "#b279a2", "#9d755d",
];

/// グラフの n 番目の系列の色。8 つの色を回す。
pub fn series_color(index: i64) -> String {
    let k = index.rem_euclid(8) as usize;
    format!("var(--pele-series-{},{})", k + 1, SERIES[k])
}
