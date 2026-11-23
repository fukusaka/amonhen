//! 色の読み方（vgc の `svg/colors.dart` と `parser.dart` の `_parseColor`）。

use std::sync::OnceLock;

use regex::{Captures, Regex};

use super::dart::{self, Error, R};
use super::paint::Color;

/// `_parseColor`。currentColor はテーマの色（黒）。
pub fn parse_color(s: Option<&str>) -> R<Option<Color>> {
    let Some(s) = s.filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if s == "none" {
        return Ok(None);
    }
    let lower = s.to_lowercase();
    if lower == "currentcolor" {
        return Ok(Some(Color::OPAQUE_BLACK));
    }
    if s.starts_with('#') {
        let mut chars: Vec<char> = s.chars().collect();
        if chars.len() == 4 {
            let (r, g, b) = (chars[1], chars[2], chars[3]);
            chars = vec!['#', r, r, g, g, b, b];
        }
        if chars.len() == 7 || chars.len() == 9 {
            let rgb: String = chars[1..7].iter().collect();
            let color = dart::parse_int(&rgb, 16)?;
            let alpha = if chars.len() == 9 {
                let a: String = chars[7..9].iter().collect();
                dart::parse_int(&a, 16)?
            } else {
                255
            };
            return Ok(Some(Color(color | (alpha << 24))));
        }
    }
    if lower.starts_with("rgb") {
        return parse_rgb(s).map(Some);
    }
    if lower.starts_with("hsl") {
        return parse_hsl(s).map(Some);
    }
    Ok(named(s))
}

/// CSS の数（整数・小数・百分率）。
const DIGIT: &str = r"(-?(?:[0-9]*\.?[0-9]+|[0-9]+\.)%?)";
/// JavaScript の `\s`。
const WS: &str = r"[\t\n\x0B\x0C\r \x{a0}\x{1680}\x{2000}-\x{200a}\x{2028}\x{2029}\x{202f}\x{205f}\x{3000}\x{feff}]";

fn function_regex(name: &str, groups: [&str; 4]) -> Regex {
    let d = |g: &str| format!("(?P<{g}>{DIGIT})", g = g);
    let legacy = format!(
        "{a}{WS}*,{WS}*{b}{WS}*,{WS}*{c}(?:{WS}*,{WS}*{e})?",
        a = d(&format!("comma{}", groups[0])),
        b = d(&format!("comma{}", groups[1])),
        c = d(&format!("comma{}", groups[2])),
        e = d(&format!("comma{}", groups[3])),
    );
    let modern = format!(
        "{a}{WS}+{b}{WS}+{c}(?:{WS}*/{WS}*{e})?",
        a = d(&format!("space{}", groups[0])),
        b = d(&format!("space{}", groups[1])),
        c = d(&format!("space{}", groups[2])),
        e = d(&format!("space{}", groups[3])),
    );
    Regex::new(&format!(r"(?i){name}a?\({WS}*(?:{legacy}|{modern}){WS}*\)")).expect("color regex")
}

fn rgb_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| function_regex("rgb", ["Red", "Green", "Blue", "Alpha"]))
}

fn hsl_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| function_regex("hsl", ["Hue", "Saturation", "Lightness", "HslAlpha"]))
}

fn group<'a>(m: &Captures<'a>, name: &str) -> Option<&'a str> {
    m.name(&format!("comma{name}"))
        .or_else(|| m.name(&format!("space{name}")))
        .map(|g| g.as_str())
}

/// `parseRgbFunction`。
fn parse_rgb(s: &str) -> R<Color> {
    let Some(m) = rgb_regex().captures(s) else {
        return Err(Error::argument_value(
            s,
            "colorString",
            "Invalid CSS rgb/rgba color syntax",
        ));
    };
    let r = component(group(&m, "Red").unwrap_or(""), false)?;
    let g = component(group(&m, "Green").unwrap_or(""), false)?;
    let b = component(group(&m, "Blue").unwrap_or(""), false)?;
    let a = component(group(&m, "Alpha").unwrap_or("1"), true)?;
    Ok(Color::from_argb(a, r, g, b))
}

fn component(value: &str, is_alpha: bool) -> R<i64> {
    if let Some(num) = value.strip_suffix('%') {
        let percent = dart::parse_double(num)?;
        return dart::round(dart::clamp(percent, 0.0, 100.0) * 2.55);
    }
    let v = dart::parse_double(value)?;
    if is_alpha {
        return dart::round(dart::clamp(v, 0.0, 1.0) * 255.0);
    }
    dart::round(dart::clamp(v, 0.0, 255.0))
}

fn hsl_value(value: &str) -> R<f64> {
    dart::parse_double(value.strip_suffix('%').unwrap_or(value))
}

/// Dart の `%`（結果は 0 以上）。
fn modulo(a: f64, b: f64) -> f64 {
    let r = a % b;
    if r == 0.0 {
        0.0
    } else if r < 0.0 {
        r + b.abs()
    } else {
        r
    }
}

/// `parseHslFunction`。
fn parse_hsl(s: &str) -> R<Color> {
    let Some(m) = hsl_regex().captures(s) else {
        return Err(Error::argument_value(
            s,
            "colorString",
            "Invalid CSS hsl/hsla color syntax",
        ));
    };
    let hue = modulo(hsl_value(group(&m, "Hue").unwrap_or(""))? / 360.0, 1.0);
    let saturation = dart::clamp(
        hsl_value(group(&m, "Saturation").unwrap_or(""))?,
        0.0,
        100.0,
    ) / 100.0;
    let luminance =
        dart::clamp(hsl_value(group(&m, "Lightness").unwrap_or(""))?, 0.0, 100.0) / 100.0;
    let raw_alpha = group(&m, "HslAlpha").unwrap_or("1");
    let alpha = if raw_alpha.ends_with('%') {
        dart::round(dart::clamp(hsl_value(raw_alpha)?, 0.0, 100.0) / 100.0 * 255.0)?
    } else {
        dart::round(dart::clamp(dart::parse_double(raw_alpha)?, 0.0, 1.0) * 255.0)?
    };
    let (mut red, mut green, mut blue) = (0.0, 0.0, 0.0);
    if hue < 1.0 / 6.0 {
        red = 1.0;
        green = hue * 6.0;
    } else if hue < 2.0 / 6.0 {
        red = 2.0 - hue * 6.0;
        green = 1.0;
    } else if hue < 3.0 / 6.0 {
        green = 1.0;
        blue = hue * 6.0 - 2.0;
    } else if hue < 4.0 / 6.0 {
        green = 4.0 - hue * 6.0;
        blue = 1.0;
    } else if hue < 5.0 / 6.0 {
        red = hue * 6.0 - 4.0;
        blue = 1.0;
    } else {
        red = 1.0;
        blue = 6.0 - hue * 6.0;
    }
    Ok(Color::from_argb(
        alpha,
        channel(red, saturation, luminance)?,
        channel(green, saturation, luminance)?,
        channel(blue, saturation, luminance)?,
    ))
}

fn channel(c: f64, saturation: f64, luminance: f64) -> R<i64> {
    let mut c = c + (1.0 - saturation) * (0.5 - c);
    if luminance < 0.5 {
        c *= luminance * 2.0;
    } else {
        c = luminance * 2.0 * (1.0 - c) + 2.0 * c - 1.0;
    }
    dart::round(dart::clamp(c, 0.0, 1.0) * 255.0)
}

/// 名前の色（大文字と小文字を区別する）。
fn named(s: &str) -> Option<Color> {
    let (_, [a, r, g, b]) = NAMED.iter().find(|(n, _)| *n == s)?;
    Some(Color::from_argb(*a, *r, *g, *b))
}

const NAMED: [(&str, [i64; 4]); 148] = [
    ("black", [255, 0, 0, 0]),
    ("aliceblue", [255, 240, 248, 255]),
    ("antiquewhite", [255, 250, 235, 215]),
    ("aqua", [255, 0, 255, 255]),
    ("aquamarine", [255, 127, 255, 212]),
    ("azure", [255, 240, 255, 255]),
    ("beige", [255, 245, 245, 220]),
    ("bisque", [255, 255, 228, 196]),
    ("blanchedalmond", [255, 255, 235, 205]),
    ("blue", [255, 0, 0, 255]),
    ("blueviolet", [255, 138, 43, 226]),
    ("brown", [255, 165, 42, 42]),
    ("burlywood", [255, 222, 184, 135]),
    ("cadetblue", [255, 95, 158, 160]),
    ("chartreuse", [255, 127, 255, 0]),
    ("chocolate", [255, 210, 105, 30]),
    ("coral", [255, 255, 127, 80]),
    ("cornflowerblue", [255, 100, 149, 237]),
    ("cornsilk", [255, 255, 248, 220]),
    ("crimson", [255, 220, 20, 60]),
    ("cyan", [255, 0, 255, 255]),
    ("darkblue", [255, 0, 0, 139]),
    ("darkcyan", [255, 0, 139, 139]),
    ("darkgoldenrod", [255, 184, 134, 11]),
    ("darkgray", [255, 169, 169, 169]),
    ("darkgreen", [255, 0, 100, 0]),
    ("darkgrey", [255, 169, 169, 169]),
    ("darkkhaki", [255, 189, 183, 107]),
    ("darkmagenta", [255, 139, 0, 139]),
    ("darkolivegreen", [255, 85, 107, 47]),
    ("darkorange", [255, 255, 140, 0]),
    ("darkorchid", [255, 153, 50, 204]),
    ("darkred", [255, 139, 0, 0]),
    ("darksalmon", [255, 233, 150, 122]),
    ("darkseagreen", [255, 143, 188, 143]),
    ("darkslateblue", [255, 72, 61, 139]),
    ("darkslategray", [255, 47, 79, 79]),
    ("darkslategrey", [255, 47, 79, 79]),
    ("darkturquoise", [255, 0, 206, 209]),
    ("darkviolet", [255, 148, 0, 211]),
    ("deeppink", [255, 255, 20, 147]),
    ("deepskyblue", [255, 0, 191, 255]),
    ("dimgray", [255, 105, 105, 105]),
    ("dimgrey", [255, 105, 105, 105]),
    ("dodgerblue", [255, 30, 144, 255]),
    ("firebrick", [255, 178, 34, 34]),
    ("floralwhite", [255, 255, 250, 240]),
    ("forestgreen", [255, 34, 139, 34]),
    ("fuchsia", [255, 255, 0, 255]),
    ("gainsboro", [255, 220, 220, 220]),
    ("ghostwhite", [255, 248, 248, 255]),
    ("gold", [255, 255, 215, 0]),
    ("goldenrod", [255, 218, 165, 32]),
    ("gray", [255, 128, 128, 128]),
    ("grey", [255, 128, 128, 128]),
    ("green", [255, 0, 128, 0]),
    ("greenyellow", [255, 173, 255, 47]),
    ("honeydew", [255, 240, 255, 240]),
    ("hotpink", [255, 255, 105, 180]),
    ("indianred", [255, 205, 92, 92]),
    ("indigo", [255, 75, 0, 130]),
    ("ivory", [255, 255, 255, 240]),
    ("khaki", [255, 240, 230, 140]),
    ("lavender", [255, 230, 230, 250]),
    ("lavenderblush", [255, 255, 240, 245]),
    ("lawngreen", [255, 124, 252, 0]),
    ("lemonchiffon", [255, 255, 250, 205]),
    ("lightblue", [255, 173, 216, 230]),
    ("lightcoral", [255, 240, 128, 128]),
    ("lightcyan", [255, 224, 255, 255]),
    ("lightgoldenrodyellow", [255, 250, 250, 210]),
    ("lightgray", [255, 211, 211, 211]),
    ("lightgreen", [255, 144, 238, 144]),
    ("lightgrey", [255, 211, 211, 211]),
    ("lightpink", [255, 255, 182, 193]),
    ("lightsalmon", [255, 255, 160, 122]),
    ("lightseagreen", [255, 32, 178, 170]),
    ("lightskyblue", [255, 135, 206, 250]),
    ("lightslategray", [255, 119, 136, 153]),
    ("lightslategrey", [255, 119, 136, 153]),
    ("lightsteelblue", [255, 176, 196, 222]),
    ("lightyellow", [255, 255, 255, 224]),
    ("lime", [255, 0, 255, 0]),
    ("limegreen", [255, 50, 205, 50]),
    ("linen", [255, 250, 240, 230]),
    ("magenta", [255, 255, 0, 255]),
    ("maroon", [255, 128, 0, 0]),
    ("mediumaquamarine", [255, 102, 205, 170]),
    ("mediumblue", [255, 0, 0, 205]),
    ("mediumorchid", [255, 186, 85, 211]),
    ("mediumpurple", [255, 147, 112, 219]),
    ("mediumseagreen", [255, 60, 179, 113]),
    ("mediumslateblue", [255, 123, 104, 238]),
    ("mediumspringgreen", [255, 0, 250, 154]),
    ("mediumturquoise", [255, 72, 209, 204]),
    ("mediumvioletred", [255, 199, 21, 133]),
    ("midnightblue", [255, 25, 25, 112]),
    ("mintcream", [255, 245, 255, 250]),
    ("mistyrose", [255, 255, 228, 225]),
    ("moccasin", [255, 255, 228, 181]),
    ("navajowhite", [255, 255, 222, 173]),
    ("navy", [255, 0, 0, 128]),
    ("oldlace", [255, 253, 245, 230]),
    ("olive", [255, 128, 128, 0]),
    ("olivedrab", [255, 107, 142, 35]),
    ("orange", [255, 255, 165, 0]),
    ("orangered", [255, 255, 69, 0]),
    ("orchid", [255, 218, 112, 214]),
    ("palegoldenrod", [255, 238, 232, 170]),
    ("palegreen", [255, 152, 251, 152]),
    ("paleturquoise", [255, 175, 238, 238]),
    ("palevioletred", [255, 219, 112, 147]),
    ("papayawhip", [255, 255, 239, 213]),
    ("peachpuff", [255, 255, 218, 185]),
    ("peru", [255, 205, 133, 63]),
    ("pink", [255, 255, 192, 203]),
    ("plum", [255, 221, 160, 221]),
    ("powderblue", [255, 176, 224, 230]),
    ("purple", [255, 128, 0, 128]),
    ("red", [255, 255, 0, 0]),
    ("rosybrown", [255, 188, 143, 143]),
    ("royalblue", [255, 65, 105, 225]),
    ("saddlebrown", [255, 139, 69, 19]),
    ("salmon", [255, 250, 128, 114]),
    ("sandybrown", [255, 244, 164, 96]),
    ("seagreen", [255, 46, 139, 87]),
    ("seashell", [255, 255, 245, 238]),
    ("sienna", [255, 160, 82, 45]),
    ("silver", [255, 192, 192, 192]),
    ("skyblue", [255, 135, 206, 235]),
    ("slateblue", [255, 106, 90, 205]),
    ("slategray", [255, 112, 128, 144]),
    ("slategrey", [255, 112, 128, 144]),
    ("snow", [255, 255, 250, 250]),
    ("springgreen", [255, 0, 255, 127]),
    ("steelblue", [255, 70, 130, 180]),
    ("tan", [255, 210, 180, 140]),
    ("teal", [255, 0, 128, 128]),
    ("thistle", [255, 216, 191, 216]),
    ("tomato", [255, 255, 99, 71]),
    ("transparent", [0, 255, 255, 255]),
    ("turquoise", [255, 64, 224, 208]),
    ("violet", [255, 238, 130, 238]),
    ("wheat", [255, 245, 222, 179]),
    ("white", [255, 255, 255, 255]),
    ("whitesmoke", [255, 245, 245, 245]),
    ("yellow", [255, 255, 255, 0]),
    ("yellowgreen", [255, 154, 205, 50]),
];
