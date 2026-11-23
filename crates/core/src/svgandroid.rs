//! VectorDrawable（Android Studio の取り込み `Svg2Vector` が SVG から作るもの。`svg2vector` で作る）を、CoreSVG・
//! librsvg で描ける SVG に戻す（History の「すべてのファイル」の Mobile）。

use std::fmt::Write as _;

use amonhen_model::{Result, tr};
// ---- VectorDrawable を SVG に ----

/// VectorDrawable の XML を、CoreSVG・librsvg で描ける SVG に戻す。
pub fn vector_drawable_to_svg(xml: &str) -> Result<String> {
    vector_drawable_to_svg_with_notes(xml).map(|(svg, _)| svg)
}

/// `vector_drawable_to_svg` と、SVG では同じに描けずに近づけた・捨てたことの説明（訳した文）。
pub fn vector_drawable_to_svg_with_notes(xml: &str) -> Result<(String, Vec<String>)> {
    let nodes = parse_xml(xml);
    let root = nodes
        .iter()
        .find(|n| n.local() == "vector")
        .ok_or_else(|| tr!("Not a VectorDrawable: the root isn’t <vector>"))?;
    let num = |key: &str| root.attr(key).and_then(|v| parse_dimension(&v));
    let (Some(width), Some(height), Some(vw), Some(vh)) = (
        num("width"),
        num("height"),
        num("viewportWidth"),
        num("viewportHeight"),
    ) else {
        return Err(tr!(
            "The <vector> needs a positive width, height, viewportWidth and viewportHeight"
        )
        .into());
    };
    if width <= 0.0 || height <= 0.0 || vw <= 0.0 || vh <= 0.0 {
        return Err(tr!(
            "The <vector> needs a positive width, height, viewportWidth and viewportHeight"
        )
        .into());
    }
    let mut w = Writer::default();
    if let Some(tint) = root.attr("tint") {
        match parse_color(&tint) {
            Some(color) => {
                let mode = root.attr("tintMode").unwrap_or_else(|| "src_in".into());
                if mode == "src_in" {
                    w.tint = Some(color);
                } else {
                    w.note(tr!("Only tintMode src_in is applied"));
                }
            }
            None => w.unresolved(&tint),
        }
    }
    let alpha = root
        .attr("alpha")
        .and_then(|a| a.trim().parse::<f64>().ok())
        .unwrap_or(1.0)
        .clamp(0.0, 1.0);
    w.body.push_str("<g");
    if alpha < 1.0 {
        let _ = write!(w.body, " opacity=\"{}\"", fmt(alpha));
    }
    w.body.push('>');
    w.children(root);
    w.body.push_str("</g>");

    let mut svg = String::new();
    let _ = write!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">",
        fmt(width),
        fmt(height),
        fmt(vw),
        fmt(vh)
    );
    if !w.defs.is_empty() {
        let _ = write!(svg, "<defs>{}</defs>", w.defs);
    }
    svg.push_str(&w.body);
    svg.push_str("</svg>");
    Ok((svg, w.notes))
}

/// `24dp`・`24px`・`24` の数。
fn parse_dimension(value: &str) -> Option<f64> {
    let v = value.trim();
    let end = v
        .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')))
        .unwrap_or(v.len());
    // 単位の `e`（`em` など）を数に含めない
    let mut number = &v[..end];
    while number.ends_with(['e', 'E']) {
        number = &number[..number.len() - 1];
    }
    number.parse().ok()
}

/// Android の色（`#RGB`・`#ARGB`・`#RRGGBB`・`#AARRGGBB`、アルファが先）。
fn parse_color(value: &str) -> Option<Color> {
    let hex = value.trim().strip_prefix('#')?;
    let digits: Vec<u8> = hex
        .chars()
        .map(|c| c.to_digit(16).map(|d| d as u8))
        .collect::<Option<_>>()?;
    let pair = |hi: u8, lo: u8| hi * 16 + lo;
    let (a, r, g, b) = match digits.as_slice() {
        [r, g, b] => (255, r * 17, g * 17, b * 17),
        [a, r, g, b] => (a * 17, r * 17, g * 17, b * 17),
        [r1, r2, g1, g2, b1, b2] => (255, pair(*r1, *r2), pair(*g1, *g2), pair(*b1, *b2)),
        [a1, a2, r1, r2, g1, g2, b1, b2] => (
            pair(*a1, *a2),
            pair(*r1, *r2),
            pair(*g1, *g2),
            pair(*b1, *b2),
        ),
        _ => return None,
    };
    Some(Color {
        rgb: [r, g, b],
        alpha: f64::from(a) / 255.0,
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Color {
    rgb: [u8; 3],
    alpha: f64,
}

impl Color {
    fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.rgb[0], self.rgb[1], self.rgb[2])
    }

    fn mix(self, other: Color, t: f64) -> Color {
        let lerp = |a: u8, b: u8| (f64::from(a) + (f64::from(b) - f64::from(a)) * t).round() as u8;
        Color {
            rgb: [
                lerp(self.rgb[0], other.rgb[0]),
                lerp(self.rgb[1], other.rgb[1]),
                lerp(self.rgb[2], other.rgb[2]),
            ],
            alpha: self.alpha + (other.alpha - self.alpha) * t,
        }
    }
}

#[derive(Clone, Debug)]
enum GradientKind {
    Linear { x1: f64, y1: f64, x2: f64, y2: f64 },
    Radial { cx: f64, cy: f64, r: f64 },
    Sweep { cx: f64, cy: f64 },
}

#[derive(Clone, Debug)]
struct Gradient {
    kind: GradientKind,
    stops: Vec<(f64, Color)>,
    spread: &'static str,
}

impl Gradient {
    /// `t`（0〜1）の色。
    fn sample(&self, t: f64) -> Color {
        let Some(first) = self.stops.first() else {
            return Color {
                rgb: [0, 0, 0],
                alpha: 0.0,
            };
        };
        if t <= first.0 {
            return first.1;
        }
        for pair in self.stops.windows(2) {
            let ((o0, c0), (o1, c1)) = (pair[0], pair[1]);
            if t <= o1 {
                let span = o1 - o0;
                return if span <= 0.0 {
                    c1
                } else {
                    c0.mix(c1, (t - o0) / span)
                };
            }
        }
        self.stops.last().map_or(first.1, |s| s.1)
    }
}

#[derive(Clone, Debug)]
enum Paint {
    None,
    Color(Color),
    Gradient(Gradient),
}

#[derive(Default)]
struct Writer {
    defs: String,
    body: String,
    notes: Vec<String>,
    next_id: usize,
    tint: Option<Color>,
}

impl Writer {
    fn note(&mut self, note: String) {
        if !self.notes.contains(&note) {
            self.notes.push(note);
        }
    }

    fn unresolved(&mut self, value: &str) {
        self.note(tr!(
            "Resource references can’t be resolved: {value}",
            value = value.trim()
        ));
    }

    fn id(&mut self, prefix: &str) -> String {
        self.next_id += 1;
        format!("{prefix}{}", self.next_id)
    }

    /// `<vector>` と `<group>` の子。`<clip-path>` はそのあとの兄弟を切り取る。
    fn children(&mut self, parent: &XmlNode) {
        let mut open = 0;
        for child in &parent.children {
            match child.local() {
                "group" => self.group(child),
                "path" => self.path(child),
                "clip-path" => {
                    let id = self.id("clip");
                    let d = child.attr("pathData").unwrap_or_default();
                    let _ = write!(
                        self.defs,
                        "<clipPath id=\"{id}\"><path d=\"{}\"/></clipPath>",
                        escape(&d)
                    );
                    let _ = write!(self.body, "<g clip-path=\"url(#{id})\">");
                    open += 1;
                }
                _ => {}
            }
        }
        for _ in 0..open {
            self.body.push_str("</g>");
        }
    }

    fn group(&mut self, group: &XmlNode) {
        let num = |key: &str, default: f64| {
            group
                .attr(key)
                .and_then(|v| v.trim().parse::<f64>().ok())
                .unwrap_or(default)
        };
        let (px, py) = (num("pivotX", 0.0), num("pivotY", 0.0));
        let (sx, sy) = (num("scaleX", 1.0), num("scaleY", 1.0));
        let rotation = num("rotation", 0.0);
        let (tx, ty) = (num("translateX", 0.0), num("translateY", 0.0));
        // Android: 原点を pivot に移し、拡大、回転、translate＋pivot へ戻す
        let mut transform = Vec::new();
        if tx + px != 0.0 || ty + py != 0.0 {
            transform.push(format!("translate({} {})", fmt(tx + px), fmt(ty + py)));
        }
        if rotation != 0.0 {
            transform.push(format!("rotate({})", fmt(rotation)));
        }
        if sx != 1.0 || sy != 1.0 {
            transform.push(format!("scale({} {})", fmt(sx), fmt(sy)));
        }
        if px != 0.0 || py != 0.0 {
            transform.push(format!("translate({} {})", fmt(-px), fmt(-py)));
        }
        self.body.push_str("<g");
        if !transform.is_empty() {
            let _ = write!(self.body, " transform=\"{}\"", transform.join(" "));
        }
        self.body.push('>');
        self.children(group);
        self.body.push_str("</g>");
    }

    fn paint(&mut self, node: &XmlNode, key: &str) -> Paint {
        let gradient = node
            .children
            .iter()
            .filter(|c| c.local() == "attr")
            .filter(|c| {
                c.attr_exact("name")
                    .is_some_and(|n| n.rsplit(':').next() == Some(key))
            })
            .find_map(|c| c.children.iter().find(|g| g.local() == "gradient"));
        if let Some(gradient) = gradient {
            return match self.gradient(gradient) {
                Some(g) => Paint::Gradient(g),
                None => Paint::None,
            };
        }
        match node.attr(key) {
            None => Paint::None,
            Some(value) => match parse_color(&value) {
                Some(color) => Paint::Color(color),
                None => {
                    self.unresolved(&value);
                    Paint::None
                }
            },
        }
    }

    fn gradient(&mut self, g: &XmlNode) -> Option<Gradient> {
        let num = |key: &str| {
            g.attr(key)
                .and_then(|v| v.trim().parse::<f64>().ok())
                .unwrap_or(0.0)
        };
        let kind = match g.attr("type").as_deref().map(str::trim) {
            Some("radial") => GradientKind::Radial {
                cx: num("centerX"),
                cy: num("centerY"),
                r: num("gradientRadius"),
            },
            Some("sweep") => GradientKind::Sweep {
                cx: num("centerX"),
                cy: num("centerY"),
            },
            _ => GradientKind::Linear {
                x1: num("startX"),
                y1: num("startY"),
                x2: num("endX"),
                y2: num("endY"),
            },
        };
        let mut stops = Vec::new();
        let items: Vec<&XmlNode> = g.children.iter().filter(|c| c.local() == "item").collect();
        if items.is_empty() {
            for (key, offset) in [("startColor", 0.0), ("centerColor", 0.5), ("endColor", 1.0)] {
                if let Some(value) = g.attr(key) {
                    match parse_color(&value) {
                        Some(color) => stops.push((offset, color)),
                        None => self.unresolved(&value),
                    }
                }
            }
        } else {
            for item in items {
                let offset = item
                    .attr("offset")
                    .and_then(|v| v.trim().parse::<f64>().ok())
                    .unwrap_or(0.0)
                    .clamp(0.0, 1.0);
                if let Some(value) = item.attr("color") {
                    match parse_color(&value) {
                        Some(color) => stops.push((offset, color)),
                        None => self.unresolved(&value),
                    }
                }
            }
        }
        if stops.is_empty() {
            return None;
        }
        // Android は並べ直さない。前より小さい offset は前に合わせる（SVG と同じ）
        let mut last = 0.0f64;
        for stop in &mut stops {
            stop.0 = stop.0.max(last);
            last = stop.0;
        }
        if let Some(tint) = self.tint {
            for stop in &mut stops {
                stop.1 = Color {
                    rgb: tint.rgb,
                    alpha: tint.alpha * stop.1.alpha,
                };
            }
        }
        let spread = match g.attr("tileMode").as_deref().map(str::trim) {
            Some("repeat") => "repeat",
            Some("mirror") => "reflect",
            _ => "pad",
        };
        Some(Gradient {
            kind,
            stops,
            spread,
        })
    }

    /// `fill`・`stroke` の属性。グラデーションは defs に置く。
    fn paint_attrs(&mut self, paint: &Paint, prefix: &str, alpha: f64) -> String {
        match paint {
            Paint::None => format!(" {prefix}=\"none\""),
            Paint::Color(color) => {
                let color = match self.tint {
                    Some(tint) => Color {
                        rgb: tint.rgb,
                        alpha: tint.alpha * color.alpha,
                    },
                    None => *color,
                };
                let mut out = format!(" {prefix}=\"{}\"", color.hex());
                let opacity = color.alpha * alpha;
                if opacity < 1.0 {
                    let _ = write!(out, " {prefix}-opacity=\"{}\"", fmt(opacity));
                }
                out
            }
            Paint::Gradient(g) => {
                let id = self.id("grad");
                let mut stops = String::new();
                for (offset, color) in &g.stops {
                    let _ = write!(
                        stops,
                        "<stop offset=\"{}\" stop-color=\"{}\"",
                        fmt(*offset),
                        color.hex()
                    );
                    if color.alpha < 1.0 {
                        let _ = write!(stops, " stop-opacity=\"{}\"", fmt(color.alpha));
                    }
                    stops.push_str("/>");
                }
                match g.kind {
                    GradientKind::Linear { x1, y1, x2, y2 } => {
                        let _ = write!(
                            self.defs,
                            "<linearGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" spreadMethod=\"{}\">{stops}</linearGradient>",
                            fmt(x1),
                            fmt(y1),
                            fmt(x2),
                            fmt(y2),
                            g.spread
                        );
                    }
                    GradientKind::Radial { cx, cy, r } => {
                        let _ = write!(
                            self.defs,
                            "<radialGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" cx=\"{}\" cy=\"{}\" r=\"{}\" spreadMethod=\"{}\">{stops}</radialGradient>",
                            fmt(cx),
                            fmt(cy),
                            fmt(r),
                            g.spread
                        );
                    }
                    GradientKind::Sweep { .. } => {
                        // 線のスイープは扇で描けないので、真ん中の色で
                        self.note(tr!("A sweep gradient on a stroke is drawn in one color"));
                        return self.paint_attrs(&Paint::Color(g.sample(0.5)), prefix, alpha);
                    }
                }
                let mut out = format!(" {prefix}=\"url(#{id})\"");
                if alpha < 1.0 {
                    let _ = write!(out, " {prefix}-opacity=\"{}\"", fmt(alpha));
                }
                out
            }
        }
    }

    fn path(&mut self, path: &XmlNode) {
        let d = path.attr("pathData").unwrap_or_default();
        let num = |key: &str, default: f64| {
            path.attr(key)
                .and_then(|v| v.trim().parse::<f64>().ok())
                .unwrap_or(default)
        };
        let fill = self.paint(path, "fillColor");
        let stroke = self.paint(path, "strokeColor");
        let fill_alpha = num("fillAlpha", 1.0).clamp(0.0, 1.0);
        let stroke_alpha = num("strokeAlpha", 1.0).clamp(0.0, 1.0);
        let even_odd = path.attr("fillType").is_some_and(|t| t.trim() == "evenOdd");
        if num("trimPathStart", 0.0) != 0.0
            || num("trimPathEnd", 1.0) != 1.0
            || num("trimPathOffset", 0.0) != 0.0
        {
            self.note(tr!("Trimmed paths are drawn untrimmed"));
        }
        let mut stroke_width = num("strokeWidth", 0.0);
        if !matches!(stroke, Paint::None) && stroke_width <= 0.0 {
            // Android は幅 0 を 1 ピクセルの線（hairline）で描く。ここでは 1dp に
            self.note(tr!("Hairline strokes (strokeWidth 0) are drawn 1 dp wide"));
            stroke_width = 1.0;
        }
        let d = escape(&d);
        let rule = if even_odd { "evenodd" } else { "nonzero" };

        // スイープの塗りは、形で切り取った扇の集まりで描く
        let fill = if let Paint::Gradient(
            g @ Gradient {
                kind: GradientKind::Sweep { .. },
                ..
            },
        ) = &fill
        {
            self.sweep_fill(&d, rule, g, fill_alpha);
            Paint::None
        } else {
            fill
        };
        if matches!(fill, Paint::None) && matches!(stroke, Paint::None) {
            return;
        }
        let mut el = format!("<path d=\"{d}\"");
        el.push_str(&self.paint_attrs(&fill, "fill", fill_alpha));
        if even_odd {
            el.push_str(" fill-rule=\"evenodd\"");
        }
        if !matches!(stroke, Paint::None) {
            el.push_str(&self.paint_attrs(&stroke, "stroke", stroke_alpha));
            let _ = write!(el, " stroke-width=\"{}\"", fmt(stroke_width));
            let cap = match path.attr("strokeLineCap").as_deref().map(str::trim) {
                Some("round") => "round",
                Some("square") => "square",
                _ => "butt",
            };
            let join = match path.attr("strokeLineJoin").as_deref().map(str::trim) {
                Some("round") => "round",
                Some("bevel") => "bevel",
                _ => "miter",
            };
            let _ = write!(
                el,
                " stroke-linecap=\"{cap}\" stroke-linejoin=\"{join}\" stroke-miterlimit=\"{}\"",
                fmt(num("strokeMiterLimit", 4.0).max(1.0))
            );
        }
        el.push_str("/>");
        self.body.push_str(&el);
    }

    fn sweep_fill(&mut self, d: &str, rule: &str, g: &Gradient, alpha: f64) {
        let GradientKind::Sweep { cx, cy } = g.kind else {
            return;
        };
        self.note(tr!("Sweep gradients are approximated with wedges"));
        let id = self.id("clip");
        let _ = write!(
            self.defs,
            "<clipPath id=\"{id}\"><path d=\"{d}\" clip-rule=\"{rule}\"/></clipPath>"
        );
        // 形をすべて覆う半径: 中心の距離＋パスの数の絶対値の和（どの点も原点からそれより近い）
        let reach: f64 = d
            .split(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | 'e' | 'E')))
            .filter_map(|n| n.parse::<f64>().ok())
            .map(f64::abs)
            .sum();
        let radius = (cx.hypot(cy) + reach + 1.0).min(1e6);
        let _ = write!(self.body, "<g clip-path=\"url(#{id})\"");
        if alpha < 1.0 {
            let _ = write!(self.body, " opacity=\"{}\"", fmt(alpha));
        }
        self.body.push('>');
        const WEDGES: usize = 72;
        let step = std::f64::consts::TAU / WEDGES as f64;
        for i in 0..WEDGES {
            let a0 = step * i as f64;
            // 継ぎ目が透けないよう、次の扇に少し重ねる
            let a1 = a0 + step * 1.1;
            let color = g.sample((i as f64 + 0.5) / WEDGES as f64);
            let _ = write!(
                self.body,
                "<path d=\"M{} {}L{} {}A{r} {r} 0 0 1 {} {}Z\" fill=\"{}\"",
                fmt(cx),
                fmt(cy),
                fmt(cx + radius * a0.cos()),
                fmt(cy + radius * a0.sin()),
                fmt(cx + radius * a1.cos()),
                fmt(cy + radius * a1.sin()),
                color.hex(),
                r = fmt(radius),
            );
            if color.alpha < 1.0 {
                let _ = write!(self.body, " fill-opacity=\"{}\"", fmt(color.alpha));
            }
            self.body.push_str("/>");
        }
        self.body.push_str("</g>");
    }
}

/// 数を短く（小数 4 桁まで、余分な 0 を落とす）。
fn fmt(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".to_owned()
    } else {
        s.to_owned()
    }
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

// ---- 小さな XML の読み手 ----

#[derive(Debug, Default)]
struct XmlNode {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<XmlNode>,
}

impl XmlNode {
    fn local(&self) -> &str {
        self.name.rsplit(':').next().unwrap_or(&self.name)
    }

    /// `android:key`（接頭辞は問わない）か、接頭辞のない `key`。
    fn attr(&self, key: &str) -> Option<String> {
        self.attrs
            .iter()
            .find(|(k, _)| k.rsplit(':').next() == Some(key))
            .map(|(_, v)| v.clone())
    }

    fn attr_exact(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// いちばん上の要素の並び（ふつうは 1 つ）。
fn parse_xml(text: &str) -> Vec<XmlNode> {
    let bytes = text.as_bytes();
    // stack[0] は仮の親
    let mut stack: Vec<XmlNode> = vec![XmlNode::default()];
    let mut i = 0;
    let skip_to = |from: usize, end: &str| {
        text[from..]
            .find(end)
            .map_or(text.len(), |p| from + p + end.len())
    };
    let close = |stack: &mut Vec<XmlNode>| {
        if stack.len() > 1 {
            let node = stack.pop().unwrap_or_default();
            if let Some(parent) = stack.last_mut() {
                parent.children.push(node);
            }
        }
    };
    while let Some(p) = text[i..].find('<') {
        i += p;
        let rest = &text[i..];
        if rest.starts_with("<!--") {
            i = skip_to(i + 4, "-->");
        } else if rest.starts_with("<![CDATA[") {
            i = skip_to(i + 9, "]]>");
        } else if rest.starts_with("<?") {
            i = skip_to(i + 2, "?>");
        } else if rest.starts_with("<!") {
            i = skip_to(i + 2, ">");
        } else if rest.starts_with("</") {
            close(&mut stack);
            i = skip_to(i + 2, ">");
        } else {
            i += 1;
            let start = i;
            while i < bytes.len()
                && !bytes[i].is_ascii_whitespace()
                && bytes[i] != b'>'
                && bytes[i] != b'/'
            {
                i += 1;
            }
            let name = text[start..i].to_owned();
            let mut attrs = Vec::new();
            let mut closed = false;
            loop {
                while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i >= bytes.len() {
                    break;
                }
                match bytes[i] {
                    b'>' => {
                        i += 1;
                        break;
                    }
                    b'/' => {
                        closed = true;
                        i += 1;
                        continue;
                    }
                    _ => {}
                }
                let k0 = i;
                while i < bytes.len()
                    && !bytes[i].is_ascii_whitespace()
                    && !matches!(bytes[i], b'=' | b'>' | b'/')
                {
                    i += 1;
                }
                let key = text[k0..i].to_owned();
                while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                let mut value = String::new();
                if i < bytes.len() && bytes[i] == b'=' {
                    i += 1;
                    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
                        let quote = bytes[i] as char;
                        let v0 = i + 1;
                        let end = text[v0..].find(quote).map_or(text.len(), |p| v0 + p);
                        value = unescape(&text[v0..end]);
                        i = (end + 1).min(text.len());
                    }
                }
                if key.is_empty() {
                    i += 1;
                } else {
                    attrs.push((key, value));
                }
            }
            if name.is_empty() {
                continue;
            }
            stack.push(XmlNode {
                name,
                attrs,
                children: Vec::new(),
            });
            if closed {
                close(&mut stack);
            }
        }
    }
    while stack.len() > 1 {
        close(&mut stack);
    }
    stack.pop().map(|root| root.children).unwrap_or_default()
}

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        rest = &rest[p..];
        let Some(end) = rest.find(';') else {
            break;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = r###"<vector xmlns:android="http://schemas.android.com/apk/res/android" xmlns:aapt="http://schemas.android.com/aapt" android:width="24dp" android:height="48dp" android:viewportWidth="12" android:viewportHeight="24""###;

    fn svg(body: &str) -> (String, Vec<String>) {
        vector_drawable_to_svg_with_notes(&format!("{HEAD}>{body}</vector>")).unwrap()
    }

    #[test]
    fn reads_the_size_and_viewport() {
        let (out, notes) = svg("");
        assert!(out.starts_with(
            r###"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="48" viewBox="0 0 12 24">"###
        ));
        assert!(notes.is_empty());
        assert!(parse_xml(&out).first().is_some_and(|n| n.name == "svg"));
    }

    #[test]
    fn refuses_what_isnt_a_vector() {
        assert!(vector_drawable_to_svg("<shape/>").is_err());
        assert!(
            vector_drawable_to_svg(
                r###"<vector android:width="24dp" android:height="24dp" android:viewportWidth="0" android:viewportHeight="24"/>"###
            )
            .is_err()
        );
    }

    #[test]
    fn reads_android_colors_alpha_first() {
        let c = |s| parse_color(s).map(|c| (c.hex(), (c.alpha * 255.0).round() as u8));
        assert_eq!(c("#f00"), Some(("#ff0000".into(), 255)));
        assert_eq!(c("#8f00"), Some(("#ff0000".into(), 136)));
        assert_eq!(c("#00ff00"), Some(("#00ff00".into(), 255)));
        assert_eq!(c("#800000ff"), Some(("#0000ff".into(), 128)));
        assert_eq!(c("@color/red"), None);
        assert_eq!(c("#12345"), None);
    }

    #[test]
    fn draws_paths_with_their_paint() {
        let (out, _) = svg(
            r##"<path android:pathData="M0,0h10v10z" android:fillColor="#80FF0000" android:fillAlpha="0.5" android:fillType="evenOdd"
                android:strokeColor="#00f" android:strokeWidth="2" android:strokeLineCap="round" android:strokeLineJoin="bevel" android:strokeMiterLimit="8"/>"##,
        );
        assert!(out.contains(r###"<path d="M0,0h10v10z" fill="#ff0000" fill-opacity="0.251" fill-rule="evenodd" stroke="#0000ff" stroke-width="2" stroke-linecap="round" stroke-linejoin="bevel" stroke-miterlimit="8"/>"###), "{out}");
    }

    #[test]
    fn skips_paths_without_paint_and_draws_hairlines_1dp() {
        let (out, notes) = svg(
            r##"<path android:pathData="M0,0h10"/><path android:pathData="M0,1h10" android:strokeColor="#000"/>"##,
        );
        assert!(!out.contains("M0,0h10"));
        assert!(out.contains(r###"d="M0,1h10" fill="none" stroke="#000000" stroke-width="1""###));
        assert_eq!(
            notes,
            vec![tr!("Hairline strokes (strokeWidth 0) are drawn 1 dp wide")]
        );
    }

    #[test]
    fn turns_group_attributes_into_a_transform() {
        let (out, _) = svg(
            r##"<group android:rotation="90" android:pivotX="6" android:pivotY="12" android:scaleX="2" android:translateX="1">
                <path android:pathData="M0,0h1" android:strokeColor="#000" android:strokeWidth="1"/></group>
                <group><path android:pathData="M0,0h2" android:fillColor="#000"/></group>"##,
        );
        assert!(
            out.contains(
                r###"<g transform="translate(7 12) rotate(90) scale(2 1) translate(-6 -12)">"###
            ),
            "{out}"
        );
        assert!(out.contains(r###"<g><path d="M0,0h2" fill="#000000"/></g>"###));
    }

    #[test]
    fn clip_paths_cut_the_siblings_after_them() {
        let (out, _) = svg(
            r##"<group><path android:pathData="M1" android:fillColor="#000"/>
                <clip-path android:pathData="M0,0h5v5z"/>
                <path android:pathData="M2" android:fillColor="#000"/>
                <clip-path android:pathData="M1,1h5v5z"/>
                <path android:pathData="M3" android:fillColor="#000"/></group>
                <path android:pathData="M4" android:fillColor="#000"/>"##,
        );
        assert!(out.contains(r###"<clipPath id="clip1"><path d="M0,0h5v5z"/></clipPath><clipPath id="clip2"><path d="M1,1h5v5z"/></clipPath>"###), "{out}");
        assert!(out.contains(r###"<g><path d="M1" fill="#000000"/><g clip-path="url(#clip1)"><path d="M2" fill="#000000"/><g clip-path="url(#clip2)"><path d="M3" fill="#000000"/></g></g></g><path d="M4""###), "{out}");
    }

    #[test]
    fn turns_aapt_gradients_into_svg_gradients() {
        let (out, notes) = svg(
            r##"<path android:pathData="M0,0h10v10z" android:fillAlpha="0.5">
                <aapt:attr name="android:fillColor"><gradient android:type="linear" android:startX="0" android:startY="1" android:endX="10" android:endY="1" android:tileMode="mirror">
                  <item android:offset="0" android:color="#FFFF0000"/><item android:offset="1" android:color="#7F0000FF"/></gradient></aapt:attr>
                <aapt:attr name="android:strokeColor"><gradient android:type="radial" android:centerX="5" android:centerY="5" android:gradientRadius="4"
                  android:startColor="#fff" android:centerColor="#888" android:endColor="#000"/></aapt:attr></path>"##,
        );
        assert_eq!(
            notes,
            vec![tr!("Hairline strokes (strokeWidth 0) are drawn 1 dp wide")]
        );
        assert!(out.contains(r##"<linearGradient id="grad1" gradientUnits="userSpaceOnUse" x1="0" y1="1" x2="10" y2="1" spreadMethod="reflect"><stop offset="0" stop-color="#ff0000"/><stop offset="1" stop-color="#0000ff" stop-opacity="0.498"/></linearGradient>"##), "{out}");
        assert!(out.contains(r##"<radialGradient id="grad2" gradientUnits="userSpaceOnUse" cx="5" cy="5" r="4" spreadMethod="pad"><stop offset="0" stop-color="#ffffff"/><stop offset="0.5" stop-color="#888888"/><stop offset="1" stop-color="#000000"/></radialGradient>"##), "{out}");
        assert!(out.contains(r###"fill="url(#grad1)" fill-opacity="0.5""###));
        // 幅のない線は hairline
        assert!(out.contains(r###"stroke="url(#grad2)" stroke-width="1""###));
    }

    #[test]
    fn approximates_sweep_gradients_with_wedges() {
        let (out, notes) = svg(
            r##"<path android:pathData="M0,0h10v10z"><aapt:attr name="android:fillColor">
                <gradient android:type="sweep" android:centerX="5" android:centerY="5" android:startColor="#f00" android:endColor="#00f"/></aapt:attr></path>"##,
        );
        assert_eq!(
            notes,
            vec![tr!("Sweep gradients are approximated with wedges")]
        );
        assert!(out.contains(
            r###"<clipPath id="clip1"><path d="M0,0h10v10z" clip-rule="nonzero"/></clipPath>"###
        ));
        assert_eq!(out.matches("<path d=\"M5 5L").count(), 72);
        // 始まりは赤寄り、終わりは青寄り
        let first = out.find("<path d=\"M5 5L").unwrap();
        assert!(out[first..].contains("fill=\"#fd0002\""), "{out}");
    }

    #[test]
    fn applies_tint_and_alpha_and_notes_the_rest() {
        let xml = format!(
            r##"{HEAD} android:tint="#8000ff00" android:alpha="0.5"><path android:pathData="M0,0h1" android:fillColor="#f00" android:trimPathEnd="0.5"/><path android:pathData="M1" android:fillColor="@color/x"/></vector>"##
        );
        let (out, notes) = vector_drawable_to_svg_with_notes(&xml).unwrap();
        assert!(
            out.contains(
                r###"<g opacity="0.5"><path d="M0,0h1" fill="#00ff00" fill-opacity="0.502"/>"###
            ),
            "{out}"
        );
        assert!(!out.contains("M1\""));
        assert_eq!(
            notes,
            vec![
                tr!("Trimmed paths are drawn untrimmed"),
                tr!(
                    "Resource references can’t be resolved: {value}",
                    value = "@color/x"
                ),
            ]
        );
    }

    #[test]
    fn escapes_and_unescapes() {
        assert_eq!(unescape("a&amp;b&#x41;&#66;&lt;&bogus;"), "a&bAB<&bogus;");
        assert_eq!(escape("a<\"&"), "a&lt;&quot;&amp;");
        assert_eq!(parse_dimension("24dp"), Some(24.0));
        assert_eq!(parse_dimension("1.5e2px"), Some(150.0));
        assert_eq!(parse_dimension("3em"), Some(3.0));
        assert_eq!(fmt(-0.00001), "0");
        assert_eq!(fmt(2.5), "2.5");
    }
}
