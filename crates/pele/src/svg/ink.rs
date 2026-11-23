//! 塗りの上の文字の色（pele の `src/svg/ink.ts`）。塗りの色が分かるとき、#1f1f1f と #fff のうちよく見える方を選ぶ。

use crate::js;

/// #fff の方が #1f1f1f よりよく見える名前の色と、残り。
const DARK: &[&str] = &[
    "black",
    "blue",
    "blueviolet",
    "brown",
    "crimson",
    "darkblue",
    "darkcyan",
    "darkgreen",
    "darkmagenta",
    "darkolivegreen",
    "darkorchid",
    "darkred",
    "darkslateblue",
    "darkslategray",
    "darkviolet",
    "dimgray",
    "firebrick",
    "forestgreen",
    "green",
    "indigo",
    "maroon",
    "mediumblue",
    "mediumslateblue",
    "mediumvioletred",
    "midnightblue",
    "navy",
    "olive",
    "purple",
    "rebeccapurple",
    "royalblue",
    "saddlebrown",
    "seagreen",
    "sienna",
    "slateblue",
    "steelblue",
    "teal",
];
const LIGHT: &[&str] = &[
    "aliceblue",
    "antiquewhite",
    "aqua",
    "aquamarine",
    "azure",
    "beige",
    "bisque",
    "blanchedalmond",
    "burlywood",
    "cadetblue",
    "chartreuse",
    "chocolate",
    "coral",
    "cornflowerblue",
    "cornsilk",
    "cyan",
    "darkgoldenrod",
    "darkgray",
    "darkkhaki",
    "darkorange",
    "darksalmon",
    "darkseagreen",
    "darkturquoise",
    "deeppink",
    "deepskyblue",
    "dodgerblue",
    "floralwhite",
    "fuchsia",
    "gainsboro",
    "ghostwhite",
    "gold",
    "goldenrod",
    "gray",
    "greenyellow",
    "honeydew",
    "hotpink",
    "indianred",
    "ivory",
    "khaki",
    "lavender",
    "lavenderblush",
    "lawngreen",
    "lemonchiffon",
    "lightblue",
    "lightcoral",
    "lightcyan",
    "lightgoldenrodyellow",
    "lightgray",
    "lightgreen",
    "lightpink",
    "lightsalmon",
    "lightseagreen",
    "lightskyblue",
    "lightslategray",
    "lightsteelblue",
    "lightyellow",
    "lime",
    "limegreen",
    "linen",
    "magenta",
    "mediumaquamarine",
    "mediumorchid",
    "mediumpurple",
    "mediumseagreen",
    "mediumspringgreen",
    "mediumturquoise",
    "mintcream",
    "mistyrose",
    "moccasin",
    "navajowhite",
    "oldlace",
    "olivedrab",
    "orange",
    "orangered",
    "orchid",
    "palegoldenrod",
    "palegreen",
    "paleturquoise",
    "palevioletred",
    "papayawhip",
    "peachpuff",
    "peru",
    "pink",
    "plum",
    "powderblue",
    "red",
    "rosybrown",
    "salmon",
    "sandybrown",
    "seashell",
    "silver",
    "skyblue",
    "slategray",
    "snow",
    "springgreen",
    "tan",
    "thistle",
    "tomato",
    "turquoise",
    "violet",
    "wheat",
    "white",
    "whitesmoke",
    "yellow",
    "yellowgreen",
];

pub const ON_LIGHT: &str = "var(--pele-fg-on-light,#1f1f1f)";
pub const ON_DARK: &str = "var(--pele-fg-on-dark,#fff)";

/// `/[";]fill:([^;"]*)/` の値。
pub fn style_fill(s: &str) -> Option<&str> {
    decl_after(s, "fill:")
}

/// `[";]` の後ろの `prop` の値（`[^;"]*`）。
fn decl_after<'a>(s: &'a str, prop: &str) -> Option<&'a str> {
    let b = s.as_bytes();
    let mut from = 0;
    while let Some(k) = s[from..].find(prop) {
        let at = from + k;
        if at > 0 && (b[at - 1] == b'"' || b[at - 1] == b';') {
            let v = &s[at + prop.len()..];
            let end = v.find([';', '"']).unwrap_or(v.len());
            return Some(&v[..end]);
        }
        from = at + 1;
    }
    None
}

/// `/[";](?:fill-)?opacity:([^;"]*)/g` の値をすべて。
fn opacities(s: &str) -> Vec<&str> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'"' || b[i] == b';' {
            let rest = &s[i + 1..];
            let body = rest.strip_prefix("fill-").unwrap_or(rest);
            if let Some(v) = body.strip_prefix("opacity:") {
                let end = v.find([';', '"']).unwrap_or(v.len());
                out.push(&v[..end]);
                i = s.len() - v.len() + end;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn is_hex_color(v: &str) -> bool {
    let Some(d) = v.strip_prefix('#') else {
        return false;
    };
    matches!(d.len(), 3 | 4 | 6 | 8) && d.bytes().all(|c| c.is_ascii_hexdigit())
}

fn linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn luminance(r: f64, g: f64, b: f64) -> f64 {
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

fn clamp(n: f64) -> f64 {
    js::min(js::max(n, 0.0), 1.0)
}

/// #1f1f1f と #fff が同じだけ見える輝度。
fn even() -> f64 {
    (1.05 * (luminance(31.0 / 255.0, 31.0 / 255.0, 31.0 / 255.0) + 0.05)).sqrt() - 0.05
}

/// `/^[+-]?(?:\d+(?:\.\d+)?|\.\d+)(?:e[+-]?\d+)?(%|deg|grad|rad|turn)?$/i` の単位（合わなければ None、単位なしは Some("")）。
fn number_unit(raw: &str) -> Option<String> {
    let lower = raw.to_ascii_lowercase();
    let b = lower.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let digits = |i: &mut usize| {
        let s = *i;
        while *i < b.len() && b[*i].is_ascii_digit() {
            *i += 1;
        }
        *i > s
    };
    if digits(&mut i) {
        if i < b.len() && b[i] == b'.' {
            let save = i;
            i += 1;
            if !digits(&mut i) {
                i = save;
            }
        }
    } else if i < b.len() && b[i] == b'.' {
        i += 1;
        if !digits(&mut i) {
            return None;
        }
    } else {
        return None;
    }
    if i < b.len() && b[i] == b'e' {
        let save = i;
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        if !digits(&mut i) {
            i = save;
        }
    }
    let unit = &lower[i..];
    matches!(unit, "" | "%" | "deg" | "grad" | "rad" | "turn").then(|| unit.to_string())
}

fn amount(raw: &str, full: f64) -> f64 {
    match number_unit(raw) {
        Some(u) if u.is_empty() => clamp(js::parse_float(raw) / full),
        Some(u) if u == "%" => clamp(js::parse_float(raw) / 100.0),
        _ => f64::NAN,
    }
}

fn hue(raw: &str) -> f64 {
    match number_unit(raw) {
        Some(u) if u != "%" => {
            let n = js::parse_float(raw);
            match u.as_str() {
                "turn" => n * 360.0,
                "rad" => (n * 180.0) / std::f64::consts::PI,
                "grad" => n * 0.9,
                _ => n,
            }
        }
        _ => f64::NAN,
    }
}

/// カンマの書き方は `none` を取らず、rgb() はどれも % かどれも数、hsl() は彩度と明度が %。
fn args(body: &str, hsl: bool) -> Option<Vec<String>> {
    let legacy = body.contains(',');
    let (main, alpha) = if legacy {
        (body, None)
    } else {
        let parts: Vec<&str> = body.split('/').collect();
        if parts.len() > 2 {
            return None;
        }
        (parts[0], parts.get(1).copied())
    };
    let main = js::trim(main);
    let mut list: Vec<String> = if legacy {
        // `/\s*,\s*/`
        main.split(',')
            .map(|p| {
                p.trim_matches(crate::util::chars::is_space_char)
                    .to_string()
            })
            .collect()
    } else {
        // `/\s+/`
        let mut v = Vec::new();
        let mut cur = String::new();
        let mut in_space = false;
        for c in main.chars() {
            if crate::util::chars::is_space_char(c) {
                if !in_space {
                    v.push(std::mem::take(&mut cur));
                }
                in_space = true;
            } else {
                in_space = false;
                cur.push(c);
            }
        }
        v.push(cur);
        v
    };
    if legacy {
        // 先頭と末尾の空白は trim 済み。区切りの空白は split の後で取った（`\s*,\s*` と同じ）。
        if list.len() < 3 || list.len() > 4 {
            return None;
        }
    } else if list.len() != 3 {
        return None;
    }
    if let Some(a) = alpha {
        list.push(js::trim(a).to_string());
    }
    if !legacy {
        return Some(
            list.into_iter()
                .map(|c| if c == "none" { "0".into() } else { c })
                .collect(),
        );
    }
    let from = if hsl { 1 } else { 0 };
    let percent = list[from..3].iter().filter(|c| c.ends_with('%')).count();
    (percent == if hsl { 2 } else { 3 } || (!hsl && percent == 0)).then_some(list)
}

fn name_of(value: &str) -> String {
    value.replacen("grey", "gray", 1)
}

/// 16 進の色か CSS の名前の色。
pub fn plain_color(raw: &str) -> bool {
    let value = js::trim(raw).to_lowercase();
    let name = name_of(&value);
    is_hex_color(&value)
        || DARK.contains(&name.as_str())
        || LIGHT.contains(&name.as_str())
        || value == "transparent"
        || value == "currentcolor"
}

/// 輝度とアルファ。名前の色は 0 か 1 にして、見やすさの選び方を保つ。
fn color(raw: &str) -> Option<(f64, f64)> {
    let value = js::trim(raw).to_lowercase();
    let name = name_of(&value);
    if DARK.contains(&name.as_str()) {
        return Some((0.0, 1.0));
    }
    if LIGHT.contains(&name.as_str()) {
        return Some((1.0, 1.0));
    }
    if is_hex_color(&value) {
        let digits: String = if value.len() < 6 {
            value[1..].chars().flat_map(|c| [c, c]).collect()
        } else {
            value[1..].to_string()
        };
        let n = |i: usize| js::parse_int(&digits[2 * i..2 * i + 2], 16) / 255.0;
        return Some((
            luminance(n(0), n(1), n(2)),
            if digits.len() == 8 { n(3) } else { 1.0 },
        ));
    }
    // `/^(rgba?|hsla?)\(([^()]*)\)$/i`
    let open = value.find('(')?;
    let fname = &value[..open];
    if !matches!(fname, "rgb" | "rgba" | "hsl" | "hsla") || !value.ends_with(')') {
        return None;
    }
    let body = &value[open + 1..value.len() - 1];
    if body.contains(['(', ')']) {
        return None;
    }
    let hsl = fname.starts_with("hsl");
    let list = args(body, hsl)?;
    let alpha = if list.len() == 4 {
        amount(&list[3], 1.0)
    } else {
        1.0
    };
    let rgb: Vec<f64> = if !hsl {
        list[..3].iter().map(|c| amount(c, 255.0)).collect()
    } else {
        let h = (((hue(&list[0]) % 360.0) + 360.0) % 360.0) / 30.0;
        let s = amount(&list[1], 100.0);
        let l = amount(&list[2], 100.0);
        let a = s * js::min(l, 1.0 - l);
        [0.0, 8.0, 4.0]
            .iter()
            .map(|k| {
                let t = (k + h) % 12.0;
                l - a * js::max(-1.0, js::min(js::min(t - 3.0, 9.0 - t), 1.0))
            })
            .collect()
    };
    let lum = luminance(rgb[0], rgb[1], rgb[2]);
    if (lum + alpha).is_nan() {
        None
    } else {
        Some((lum, alpha))
    }
}

/// スタイルの fill-opacity と opacity が塗りをどれだけ不透明に残すか。どちらかが数でなければ NaN。
fn opacity(shape: &str) -> f64 {
    let mut product = 1.0;
    for v in opacities(shape) {
        product *= amount(js::trim(v), 1.0);
    }
    product
}

/// スタイルは `resolve_style` から。半分より薄い塗りは、分からない背景しだい。
pub fn ink(shape: &str, text: &str) -> Option<&'static str> {
    let fill = style_fill(shape)?;
    if style_fill(text).is_some() {
        return None;
    }
    let found = color(fill)?;
    // calc() や var() のような数でない不透明度は、まだ薄いかもしれない。
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    if !(found.1 * opacity(shape) >= 0.5) {
        return None;
    }
    Some(if found.0 > even() { ON_LIGHT } else { ON_DARK })
}

/// 分からない塗りの色と不透明度は不透明とみなす。
pub fn paints(shape: &str) -> bool {
    let Some(fill) = style_fill(shape).map(|f| js::trim(f).to_lowercase()) else {
        return false;
    };
    if fill.is_empty() || fill == "none" || fill == "transparent" {
        return false;
    }
    let scale = opacity(shape);
    color(&fill).map_or(1.0, |c| c.1) * (if scale.is_nan() { 1.0 } else { scale }) > 0.0
}

pub fn ink_fill(shape: &str, text: &str, fallback: Option<&str>) -> String {
    match ink(shape, text).or(fallback) {
        Some(c) => format!(" fill=\"{c}\""),
        None => String::new(),
    }
}
