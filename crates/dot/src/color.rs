//! 色（Graphviz の色の書き方）: X11・SVG の名前、`#rrggbb`・`#rrggbbaa`・`#rgb`、HSV（`h,s,v`・`h s v`、0〜1）、
//! `/scheme/name`（ColorBrewer の配色は `blues9` などの名前と 1 からの番号）、`transparent`・`none`。`a:b` の並びと
//! `a;0.3` の割合。持たない配色の色は読めない色として扱う（呼ぶ側が既定の色にして注記する）。

use crate::brewer::BREWER;
use crate::x11::X11;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const BLACK: Rgba = Rgba {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };

    fn from_u32(v: u32) -> Self {
        Rgba {
            r: (v >> 16) as u8,
            g: (v >> 8) as u8,
            b: v as u8,
            a: 255,
        }
    }

    /// `#rrggbb`（透明度は `opacity` で別に書く）。
    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// 不透明なら None、ほかは 0〜1。
    pub fn opacity(self) -> Option<f64> {
        (self.a != 255).then(|| (f64::from(self.a) / 255.0 * 1000.0).round() / 1000.0)
    }

    /// 明るさ（0〜1）。
    pub fn luminance(self) -> f64 {
        (0.299 * f64::from(self.r) + 0.587 * f64::from(self.g) + 0.114 * f64::from(self.b)) / 255.0
    }
}

/// 読んだ色。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    Rgba(Rgba),
    /// 描かない（`transparent`・`none`・`invis`、透明度 0）。
    None,
}

impl Color {
    /// この色の塗りの上で読める既定の文字の色（濃い塗りなら白、明るければ黒に近い色）。
    pub fn ink(self) -> Option<&'static str> {
        match self {
            Color::Rgba(rgba) if rgba.luminance() < 0.45 => Some("#ffffff"),
            Color::Rgba(_) => Some("#1f1f1f"),
            Color::None => None,
        }
    }
}

/// 名前の色の配色。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scheme {
    X11,
    Svg,
    /// ColorBrewer の配色（1 番からの色）。
    Brewer(&'static [u32]),
    /// 持たない配色。
    Other,
}

fn brewer(name: &str) -> Option<&'static [u32]> {
    BREWER
        .binary_search_by(|(n, _)| (*n).cmp(name))
        .ok()
        .map(|i| BREWER[i].1)
}

fn scheme_of(name: &str) -> Scheme {
    match name.trim().to_ascii_lowercase().as_str() {
        "" | "x11" => Scheme::X11,
        "svg" => Scheme::Svg,
        other => brewer(other).map_or(Scheme::Other, Scheme::Brewer),
    }
}

/// 知っている配色の名前か（X11・SVG・ColorBrewer）。
pub fn known_scheme(name: &str) -> bool {
    !matches!(scheme_of(name), Scheme::Other)
}

/// SVG（CSS）の色の名前。値は X11 と同じで、ここにある 4 つの名前だけが違う。
const SVG_NAMES: &[&str] = &[
    "aliceblue",
    "antiquewhite",
    "aqua",
    "aquamarine",
    "azure",
    "beige",
    "bisque",
    "black",
    "blanchedalmond",
    "blue",
    "blueviolet",
    "brown",
    "burlywood",
    "cadetblue",
    "chartreuse",
    "chocolate",
    "coral",
    "cornflowerblue",
    "cornsilk",
    "crimson",
    "cyan",
    "darkblue",
    "darkcyan",
    "darkgoldenrod",
    "darkgray",
    "darkgreen",
    "darkgrey",
    "darkkhaki",
    "darkmagenta",
    "darkolivegreen",
    "darkorange",
    "darkorchid",
    "darkred",
    "darksalmon",
    "darkseagreen",
    "darkslateblue",
    "darkslategray",
    "darkslategrey",
    "darkturquoise",
    "darkviolet",
    "deeppink",
    "deepskyblue",
    "dimgray",
    "dimgrey",
    "dodgerblue",
    "firebrick",
    "floralwhite",
    "forestgreen",
    "fuchsia",
    "gainsboro",
    "ghostwhite",
    "gold",
    "goldenrod",
    "gray",
    "grey",
    "green",
    "greenyellow",
    "honeydew",
    "hotpink",
    "indianred",
    "indigo",
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
    "lightgrey",
    "lightpink",
    "lightsalmon",
    "lightseagreen",
    "lightskyblue",
    "lightslategray",
    "lightslategrey",
    "lightsteelblue",
    "lightyellow",
    "lime",
    "limegreen",
    "linen",
    "magenta",
    "maroon",
    "mediumaquamarine",
    "mediumblue",
    "mediumorchid",
    "mediumpurple",
    "mediumseagreen",
    "mediumslateblue",
    "mediumspringgreen",
    "mediumturquoise",
    "mediumvioletred",
    "midnightblue",
    "mintcream",
    "mistyrose",
    "moccasin",
    "navajowhite",
    "navy",
    "oldlace",
    "olive",
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
    "purple",
    "rebeccapurple",
    "red",
    "rosybrown",
    "royalblue",
    "saddlebrown",
    "salmon",
    "sandybrown",
    "seagreen",
    "seashell",
    "sienna",
    "silver",
    "skyblue",
    "slateblue",
    "slategray",
    "slategrey",
    "snow",
    "springgreen",
    "steelblue",
    "tan",
    "teal",
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

fn x11(name: &str) -> Option<u32> {
    X11.binary_search_by(|(n, _)| (*n).cmp(name))
        .ok()
        .map(|i| X11[i].1)
}

fn svg(name: &str) -> Option<u32> {
    if !SVG_NAMES.contains(&name) {
        return None;
    }
    Some(match name {
        "gray" | "grey" => 0x808080,
        "green" => 0x008000,
        "maroon" => 0x800000,
        "purple" => 0x800080,
        _ => x11(name)?,
    })
}

fn hex_digit(c: u8) -> Option<u8> {
    (c as char).to_digit(16).map(|d| d as u8)
}

fn parse_hex(h: &str) -> Option<Rgba> {
    let b = h.as_bytes();
    let pair = |i: usize| Some(hex_digit(b[i])? * 16 + hex_digit(b[i + 1])?);
    match b.len() {
        3 | 4 => {
            let d = |i: usize| hex_digit(b[i]).map(|v| v * 17);
            Some(Rgba {
                r: d(0)?,
                g: d(1)?,
                b: d(2)?,
                a: if b.len() == 4 { d(3)? } else { 255 },
            })
        }
        6 | 8 => Some(Rgba {
            r: pair(0)?,
            g: pair(2)?,
            b: pair(4)?,
            a: if b.len() == 8 { pair(6)? } else { 255 },
        }),
        _ => None,
    }
}

fn hsv(h: f64, s: f64, v: f64) -> Rgba {
    let h = h.clamp(0.0, 1.0);
    let s = s.clamp(0.0, 1.0);
    let v = v.clamp(0.0, 1.0);
    let (r, g, b) = if s <= 0.0 {
        (v, v, v)
    } else {
        let h6 = if h >= 1.0 { 0.0 } else { h * 6.0 };
        let i = h6.floor();
        let f = h6 - i;
        let p = v * (1.0 - s);
        let q = v * (1.0 - s * f);
        let t = v * (1.0 - s * (1.0 - f));
        match i as i32 {
            0 => (v, t, p),
            1 => (q, v, p),
            2 => (p, v, t),
            3 => (p, q, v),
            4 => (t, p, v),
            _ => (v, p, q),
        }
    };
    let c = |x: f64| (x * 255.0).round() as u8;
    Rgba {
        r: c(r),
        g: c(g),
        b: c(b),
        a: 255,
    }
}

fn parse_hsv(s: &str) -> Option<Rgba> {
    let parts: Vec<&str> = s
        .split(|c: char| c == ',' || c.is_ascii_whitespace())
        .filter(|p| !p.is_empty())
        .collect();
    if !(3..=4).contains(&parts.len()) {
        return None;
    }
    let mut n = [0f64; 4];
    for (i, p) in parts.iter().enumerate() {
        let v: f64 = p.parse().ok()?;
        if !v.is_finite() {
            return None;
        }
        n[i] = v;
    }
    let mut c = hsv(n[0], n[1], n[2]);
    if parts.len() == 4 {
        c.a = (n[3].clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    Some(c)
}

/// 1 つの色。`scheme` は `colorscheme`。読めなければ None。
pub fn parse(text: &str, scheme: Option<&str>) -> Option<Color> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let lower = t.to_ascii_lowercase();
    if matches!(lower.as_str(), "transparent" | "none" | "invis") {
        return Some(Color::None);
    }
    let rgba = if let Some(h) = t.strip_prefix('#') {
        parse_hex(h)?
    } else if t.starts_with(|c: char| c.is_ascii_digit() || c == '.')
        && !(t.bytes().all(|c| c.is_ascii_digit())
            && scheme.is_some_and(|s| !matches!(scheme_of(s), Scheme::X11 | Scheme::Svg)))
    {
        parse_hsv(t)?
    } else {
        let (scheme, name) = match lower.strip_prefix('/') {
            Some(rest) => match rest.split_once('/') {
                Some((s, n)) => (scheme_of(s), n.to_owned()),
                None => (Scheme::X11, rest.to_owned()),
            },
            None => (scheme.map_or(Scheme::X11, scheme_of), lower.clone()),
        };
        let name: String = name.chars().filter(|c| !c.is_whitespace()).collect();
        let found = match scheme {
            Scheme::X11 => x11(&name),
            Scheme::Svg => svg(&name).or_else(|| x11(&name)),
            // 番号は配色の色、ほかは Graphviz と同じく X11 の名前
            Scheme::Brewer(colors) => match name.parse::<usize>() {
                Ok(i) => i.checked_sub(1).and_then(|i| colors.get(i)).copied(),
                Err(_) => x11(&name),
            },
            // 持たない配色の名前でも、X11 の名前なら Graphviz と同じくそれを使う
            Scheme::Other => {
                if name.chars().all(|c| c.is_ascii_digit()) {
                    None
                } else {
                    x11(&name)
                }
            }
        };
        Rgba::from_u32(found?)
    };
    if rgba.a == 0 {
        return Some(Color::None);
    }
    Some(Color::Rgba(rgba))
}

/// 色の並び（`a:b;0.3:c`）の 1 つ。
#[derive(Clone, Debug, PartialEq)]
pub struct Stop {
    /// 読めない色は None。
    pub color: Option<Color>,
    /// `;0.3` の割合。
    pub fraction: Option<f64>,
}

/// 色の並び。空の項目（`a::b`）は飛ばす。
pub fn parse_list(text: &str, scheme: Option<&str>) -> Vec<Stop> {
    let mut out = Vec::new();
    for part in text.split(':') {
        let (name, fraction) = match part.split_once(';') {
            Some((n, f)) => (
                n,
                f.trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|f| f.is_finite() && *f >= 0.0),
            ),
            None => (part, None),
        };
        if name.trim().is_empty() {
            continue;
        }
        out.push(Stop {
            color: parse(name, scheme),
            fraction,
        });
        if out.len() >= 64 {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(text: &str) -> Rgba {
        match parse(text, None) {
            Some(Color::Rgba(c)) => c,
            other => panic!("{text}: {other:?}"),
        }
    }

    #[test]
    fn reads_names_and_numbers() {
        assert_eq!(rgb("red").hex(), "#ff0000");
        assert_eq!(rgb("Light Gray").hex(), "#d3d3d3");
        assert_eq!(rgb("lightgoldenrod").hex(), "#eedd82");
        assert_eq!(rgb("gray50").hex(), "#7f7f7f");
        assert_eq!(rgb("grey100").hex(), "#ffffff");
        assert_eq!(rgb("#1a2B3c").hex(), "#1a2b3c");
        assert_eq!(rgb("#abc").hex(), "#aabbcc");
        let c = rgb("#ff000080");
        assert_eq!((c.hex(), c.opacity()), ("#ff0000".into(), Some(0.502)));
        assert_eq!(rgb("0.000 1.000 1.000").hex(), "#ff0000");
        assert_eq!(rgb(".333,1,1").hex(), "#01ff00");
        assert_eq!(rgb("0 0 0.5").hex(), "#808080");
        assert_eq!(rgb("/svg/green").hex(), "#008000");
        assert_eq!(rgb("/x11/green").hex(), "#00ff00");
        assert_eq!(
            parse("green", Some("svg")),
            Some(Color::Rgba(rgb("#008000")))
        );
        assert_eq!(parse("transparent", None), Some(Color::None));
        assert_eq!(parse("#ff000000", None), Some(Color::None));
        assert_eq!(parse("nosuchcolor", None), None);
        assert_eq!(parse("#12", None), None);
        assert_eq!(parse("1 2", None), None);
        // 持たない配色の番号の色は読めない
        assert_eq!(parse("3", Some("nosuch9")), None);
        assert_eq!(parse("/nosuch9/3", None), None);
        assert_eq!(parse("red", Some("nosuch9")), Some(Color::Rgba(rgb("red"))));
        assert_eq!(parse("red", Some("accent8")), Some(Color::Rgba(rgb("red"))));
    }

    #[test]
    fn reads_colorbrewer_schemes() {
        assert_eq!(
            parse("1", Some("blues9")),
            Some(Color::Rgba(rgb("#f7fbff")))
        );
        assert_eq!(rgb("/blues9/9").hex(), "#08306b");
        assert_eq!(rgb("/Set312/12").hex(), "#ffed6f");
        assert_eq!(rgb("/rdylgn11/6").hex(), "#ffffbf");
        assert_eq!(
            parse("8", Some("ACCENT8")),
            Some(Color::Rgba(rgb("#666666")))
        );
        // 範囲の外の番号は読めない
        assert_eq!(parse("0", Some("blues9")), None);
        assert_eq!(parse("10", Some("blues9")), None);
        assert!(known_scheme("paired12") && known_scheme("SVG") && !known_scheme("blues"));
        // 表は名前の順（二分探索のため）で、名前の数と色の数がそろう
        assert!(BREWER.windows(2).all(|w| w[0].0 < w[1].0));
        for (name, colors) in BREWER {
            assert!(name.ends_with(&colors.len().to_string()), "{name}");
        }
    }

    #[test]
    fn reads_lists() {
        let l = parse_list("red;0.3:blue::#00ff00", None);
        assert_eq!(l.len(), 3);
        assert_eq!(l[0].fraction, Some(0.3));
        assert_eq!(l[1].color, Some(Color::Rgba(rgb("blue"))));
        assert_eq!(l[2].fraction, None);
        assert_eq!(parse_list("x:", None)[0].color, None);
    }
}
