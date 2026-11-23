//! 箱の題の頭が色か（pele の `src/diagrams/sequence/colors.ts`）。Mermaid はブラウザに聞く。ここにブラウザはないので、
//! 色の名前と rgb()・hsl() の書き方を知っておく。calc() や var() のようなほかの関数は色とみなさない。

use crate::js;

const KEYWORDS: &str = "aliceblue antiquewhite aqua aquamarine azure beige bisque black blanchedalmond blue blueviolet brown burlywood cadetblue chartreuse chocolate coral cornflowerblue cornsilk crimson cyan darkblue darkcyan darkgoldenrod darkgray darkgreen darkgrey darkkhaki darkmagenta darkolivegreen darkorange darkorchid darkred darksalmon darkseagreen darkslateblue darkslategray darkslategrey darkturquoise darkviolet deeppink deepskyblue dimgray dimgrey dodgerblue firebrick floralwhite forestgreen fuchsia gainsboro ghostwhite gold goldenrod gray green greenyellow grey honeydew hotpink indianred indigo ivory khaki lavender lavenderblush lawngreen lemonchiffon lightblue lightcoral lightcyan lightgoldenrodyellow lightgray lightgreen lightgrey lightpink lightsalmon lightseagreen lightskyblue lightslategray lightslategrey lightsteelblue lightyellow lime limegreen linen magenta maroon mediumaquamarine mediumblue mediumorchid mediumpurple mediumseagreen mediumslateblue mediumspringgreen mediumturquoise mediumvioletred midnightblue mintcream mistyrose moccasin navajowhite navy oldlace olive olivedrab orange orangered orchid palegoldenrod palegreen paleturquoise palevioletred papayawhip peachpuff peru pink plum powderblue purple rebeccapurple red rosybrown royalblue saddlebrown salmon sandybrown seagreen seashell sienna silver skyblue slateblue slategray slategrey snow springgreen steelblue tan teal thistle tomato turquoise violet wheat white whitesmoke yellow yellowgreen transparent currentcolor inherit initial unset revert accentcolor accentcolortext activetext buttonborder buttonface buttontext canvas canvastext field fieldtext graytext highlight highlighttext linktext mark marktext selecteditem selecteditemtext visitedtext activeborder activecaption appworkspace background buttonhighlight buttonshadow captiontext inactiveborder inactivecaption inactivecaptiontext infobackground infotext menu menutext scrollbar threeddarkshadow threedface threedhighlight threedlightshadow threedshadow window windowframe windowtext";

#[derive(Clone, Copy, PartialEq, Eq)]
enum K {
    Bad,
    Number,
    Percent,
    Angle,
    None,
}

/// `/^[+-]?(?:\d+(?:\.\d+)?|\.\d+)(?:e[+-]?\d+)?(%|deg|grad|rad|turn)?$/i`。合えば単位（なければ ""）。
fn term(s: &str) -> Option<&str> {
    let b = s.as_bytes();
    let n = b.len();
    let digits = |mut i: usize| {
        while i < n && b[i].is_ascii_digit() {
            i += 1;
        }
        i
    };
    let mut i = 0;
    if i < n && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    if i < n && b[i].is_ascii_digit() {
        i = digits(i);
        if i + 1 < n && b[i] == b'.' && b[i + 1].is_ascii_digit() {
            i = digits(i + 1);
        }
    } else if i + 1 < n && b[i] == b'.' && b[i + 1].is_ascii_digit() {
        i = digits(i + 1);
    } else {
        return None;
    }
    if i < n && (b[i] | 32) == b'e' {
        let mut j = i + 1;
        if j < n && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        if j < n && b[j].is_ascii_digit() {
            i = digits(j);
        }
    }
    let unit = &s[i..];
    let lower = unit.to_ascii_lowercase();
    matches!(lower.as_str(), "" | "%" | "deg" | "grad" | "rad" | "turn").then_some(unit)
}

fn kind(t: &str) -> K {
    if t.to_lowercase() == "none" {
        return K::None;
    }
    match term(t) {
        None => K::Bad,
        Some("") => K::Number,
        Some("%") => K::Percent,
        Some(_) => K::Angle,
    }
}

fn is_alpha(t: &str, modern: bool) -> bool {
    let k = kind(t);
    k == K::Number || k == K::Percent || (modern && k == K::None)
}

fn is_function(color: &str) -> bool {
    let hsl = color.starts_with("hsl");
    let open = color.find('(').unwrap_or(0);
    let name = &color[..open];
    if !matches!(name, "rgb" | "rgba" | "hsl" | "hsla") {
        return false;
    }
    if js::len16(color) > 120 || color.find(')') != Some(color.len() - 1) {
        return false;
    }
    let inner = js::trim(&color[open + 1..color.len() - 1]);
    let mut alpha: Option<&str> = None;
    let legacy = inner.contains(',');
    let mut parts: Vec<&str>;
    if legacy {
        parts = inner.split(',').map(js::trim).collect();
        if parts.len() == 4 {
            alpha = parts.pop();
        }
    } else {
        let slash: Vec<&str> = inner.split('/').collect();
        if slash.len() > 2 {
            return false;
        }
        // `split(/\s+/)`
        let head = js::trim(slash[0]);
        parts = if head.is_empty() {
            vec![""]
        } else {
            head.split(js::is_js_space)
                .filter(|s| !s.is_empty())
                .collect()
        };
        if slash.len() == 2 {
            alpha = Some(js::trim(slash[1]));
        }
    }
    if parts.len() != 3 || alpha.is_some_and(|a| !is_alpha(a, !legacy)) {
        return false;
    }
    let kinds: Vec<K> = parts.iter().map(|p| kind(p)).collect();
    if legacy {
        if hsl {
            return (kinds[0] == K::Number || kinds[0] == K::Angle)
                && kinds[1] == K::Percent
                && kinds[2] == K::Percent;
        }
        return (kinds[0] == K::Number || kinds[0] == K::Percent)
            && kinds[1] == kinds[0]
            && kinds[2] == kinds[0];
    }
    kinds.iter().enumerate().all(|(i, &k)| {
        k == K::Number
            || k == K::None
            || if hsl && i == 0 {
                k == K::Angle
            } else {
                k == K::Percent
            }
    })
}

pub fn is_css_color(color: &str) -> bool {
    if color.contains('(') {
        return is_function(color);
    }
    let lower = color.to_lowercase();
    KEYWORDS.split(' ').any(|k| k == lower)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_colors() {
        assert!(is_css_color("Aqua"));
        assert!(is_css_color("rgb(1, 2, 3)"));
        assert!(is_css_color("rgba(1,2,3,0.5)"));
        assert!(is_css_color("hsl(120deg 50% 50% / .5)"));
        assert!(!is_css_color("rgb(1, 2%, 3)"));
        assert!(!is_css_color("calc(1)"));
        assert!(!is_css_color("nocolor"));
        assert!(is_css_color("rgb(1e2 none 3)"));
    }
}
