//! ラベルの文字: 行（`\n`・`\l`・`\r` の寄せ）と飾りの span、幅の見積もり、SVG の `<text>`。ふつうのラベルの
//! エスケープ（`\N`・`\G`・`\E`・`\T`・`\H`・`\L`）と、HTML のラベル（`<b>`・`<i>`・`<u>`・`<s>`・`<o>`・`<sub>`・
//! `<sup>`・`<br/>`・`<font>`）を読む。HTML の表（`<table>`）は `html` で組んで描く。

use amonhen_pele::svg::builder::{esc, num};
use amonhen_pele::text::entities::decode_entities;
use amonhen_pele::text::measurer::{default_measurer, style};

use crate::color::{Color, parse as parse_color};

/// 行の寄せ。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Just {
    Center,
    Left,
    Right,
}

/// 文字の飾り。
#[derive(Clone, Debug, PartialEq)]
pub struct Font {
    pub size: f64,
    pub bold: bool,
    pub italic: bool,
    pub mono: bool,
    pub underline: bool,
    pub strike: bool,
    pub overline: bool,
    pub sub: bool,
    pub sup: bool,
    /// 文が決めた色（なければテーマの文字の色）。
    pub color: Option<Color>,
}

impl Font {
    pub fn new(size: f64) -> Self {
        Font {
            size,
            bold: false,
            italic: false,
            mono: false,
            underline: false,
            strike: false,
            overline: false,
            sub: false,
            sup: false,
            color: None,
        }
    }

    /// `fontname` の名前から（太字・斜体・等幅だけを見る。書体そのものはアプリの書体）。
    pub fn with_name(mut self, name: &str) -> Self {
        let n = name.to_ascii_lowercase();
        if n.contains("bold") || n.contains("black") || n.contains("heavy") {
            self.bold = true;
        }
        if n.contains("italic") || n.contains("oblique") {
            self.italic = true;
        }
        if n.contains("courier")
            || n.contains("mono")
            || n.contains("consol")
            || n.contains("menlo")
        {
            self.mono = true;
        }
        self
    }

    fn bits(&self) -> u32 {
        (if self.bold { style::BOLD } else { 0 })
            | (if self.mono { style::MONO } else { 0 })
            | (if self.sub { style::SUB } else { 0 })
            | (if self.sup { style::SUP } else { 0 })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub text: String,
    pub font: Font,
    pub width: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub spans: Vec<Span>,
    pub just: Just,
    pub width: f64,
    pub height: f64,
    /// いちばん大きい文字（基線の位置を決める）。
    pub size: f64,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Text {
    pub lines: Vec<Line>,
    pub width: f64,
    pub height: f64,
    /// HTML の表を文字だけにした。
    pub table: bool,
    /// HTML の画像（`<img>`）を描けなかった（枠にした・省いた）。
    pub image: bool,
    /// HTML の表（あれば行の代わりにこれを描く。大きさは `width`・`height`）。
    pub html: Option<Box<crate::html::Table>>,
}

/// 行の高さ（文字の大きさの倍）。
const LINE: f64 = 1.2;

fn measure(text: &str, font: &Font) -> f64 {
    default_measurer().width(text, font.size, font.bits())
}

impl Text {
    pub fn is_empty(&self) -> bool {
        self.html.is_none()
            && self
                .lines
                .iter()
                .all(|l| l.spans.iter().all(|s| s.text.is_empty()))
    }

    fn finish(mut self, base: f64) -> Self {
        self.width = 0.0;
        self.height = 0.0;
        for line in &mut self.lines {
            line.width = line.spans.iter().map(|s| s.width).sum();
            line.size = line.spans.iter().map(|s| s.font.size).fold(0.0, f64::max);
            if line.size <= 0.0 {
                line.size = base;
            }
            line.height = line.size * LINE;
            self.width = self.width.max(line.width);
            self.height += line.height;
        }
        self
    }

    /// 1 色・1 書体の行から。
    pub fn plain(lines: Vec<(String, Just)>, font: &Font) -> Self {
        let lines = lines
            .into_iter()
            .map(|(t, just)| {
                let width = measure(&t, font);
                Line {
                    spans: if t.is_empty() {
                        vec![]
                    } else {
                        vec![Span {
                            text: t,
                            font: font.clone(),
                            width,
                        }]
                    },
                    just,
                    width: 0.0,
                    height: 0.0,
                    size: font.size,
                }
            })
            .collect();
        Text {
            lines,
            ..Default::default()
        }
        .finish(font.size)
    }
}

/// `\N` などの置き換えに使う名前。
#[derive(Clone, Copy, Debug, Default)]
pub struct Names<'a> {
    pub graph: &'a str,
    pub node: &'a str,
    pub edge: &'a str,
    pub tail: &'a str,
    pub head: &'a str,
    pub label: &'a str,
}

/// ふつうのラベルのエスケープを戻し、行に分ける。最後の行の終わりの `\l`・`\r`・`\n` は空の行を作らない。
pub fn escaped_lines(raw: &str, names: &Names) -> Vec<(String, Just)> {
    let mut lines: Vec<(String, Just)> = Vec::new();
    let mut cur = String::new();
    let mut chars = raw.chars().peekable();
    let mut ended = false;
    while let Some(c) = chars.next() {
        ended = false;
        match c {
            '\\' => match chars.next() {
                Some('n') => {
                    lines.push((std::mem::take(&mut cur), Just::Center));
                    ended = true;
                }
                Some('l') => {
                    lines.push((std::mem::take(&mut cur), Just::Left));
                    ended = true;
                }
                Some('r') => {
                    lines.push((std::mem::take(&mut cur), Just::Right));
                    ended = true;
                }
                Some('N') => cur.push_str(names.node),
                Some('G') => cur.push_str(names.graph),
                Some('E') => cur.push_str(names.edge),
                Some('T') => cur.push_str(names.tail),
                Some('H') => cur.push_str(names.head),
                Some('L') => cur.push_str(names.label),
                Some(other) => cur.push(other),
                None => cur.push('\\'),
            },
            '\n' => {
                lines.push((std::mem::take(&mut cur), Just::Center));
                ended = true;
            }
            '\r' => {}
            c => cur.push(c),
        }
    }
    if !ended || lines.is_empty() {
        lines.push((cur, Just::Center));
    }
    lines
        .into_iter()
        .map(|(t, j)| (decode_entities(&t), j))
        .collect()
}

/// ふつうのラベル。
pub fn plain_label(raw: &str, names: &Names, font: &Font) -> Text {
    Text::plain(escaped_lines(raw, names), font)
}

// ---- HTML のラベル ----

struct Html<'a> {
    s: &'a str,
    i: usize,
}

#[derive(Debug)]
pub(crate) enum Piece {
    Text(String),
    Open(String, Vec<(String, String)>),
    Close(String),
    /// `<br/>` など、閉じのいらないもの。
    Empty(String, Vec<(String, String)>),
}

impl<'a> Html<'a> {
    fn next(&mut self) -> Option<Piece> {
        let mut rest = &self.s[self.i..];
        // 注釈を飛ばす
        while let Some(body) = rest.strip_prefix("<!--") {
            let end = body.find("-->").map_or(rest.len(), |k| k + 7);
            self.i += end;
            rest = &self.s[self.i..];
        }
        if rest.is_empty() {
            return None;
        }
        if let Some(body) = rest.strip_prefix('<') {
            let (inner, advance) = match body.find('>') {
                Some(end) => (&body[..end], end + 2),
                None => (body, rest.len()),
            };
            self.i += advance;
            let inner = inner.trim();
            if let Some(name) = inner.strip_prefix('/') {
                return Some(Piece::Close(name.trim().to_ascii_lowercase()));
            }
            let (inner, empty) = match inner.strip_suffix('/') {
                Some(i) => (i, true),
                None => (inner, false),
            };
            let name_end = inner
                .find(|c: char| c.is_whitespace())
                .unwrap_or(inner.len());
            let name = inner[..name_end].to_ascii_lowercase();
            let attrs = html_attrs(&inner[name_end..]);
            let empty = empty || matches!(name.as_str(), "br" | "img" | "hr" | "vr");
            return Some(if empty {
                Piece::Empty(name, attrs)
            } else {
                Piece::Open(name, attrs)
            });
        }
        let end = rest.find('<').unwrap_or(rest.len());
        self.i += end;
        Some(Piece::Text(rest[..end].to_owned()))
    }
}

fn html_attrs(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let start = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'=' {
            i += 1;
        }
        if start == i {
            i += 1;
            continue;
        }
        let key = s[start..i].to_ascii_lowercase();
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                i += 1;
                let vs = i;
                while i < b.len() && b[i] != q {
                    i += 1;
                }
                value = s[vs..i.min(b.len())].to_owned();
                i += 1;
            } else {
                let vs = i;
                while i < b.len() && !b[i].is_ascii_whitespace() {
                    i += 1;
                }
                value = s[vs..i].to_owned();
            }
        }
        out.push((key, decode_entities(&value)));
    }
    out
}

/// HTML を切れ端に分ける（切れ端とその文の中の始め・終わり）。
pub(crate) fn pieces(src: &str, limit: usize) -> Vec<(Piece, usize, usize)> {
    let mut p = Html { s: src, i: 0 };
    let mut out = Vec::new();
    loop {
        let start = p.i;
        let Some(piece) = p.next() else { break };
        // 注釈を飛ばした分は始めに含めない
        let end = p.i;
        out.push((piece, start, end));
        if out.len() >= limit {
            break;
        }
    }
    out
}

/// 飾りの要素（`<b>`・`<font>` など）なら、その中の書体。
pub(crate) fn styled(
    font: &Font,
    name: &str,
    attrs: &[(String, String)],
    scheme: Option<&str>,
) -> Option<Font> {
    let mut font = font.clone();
    match name {
        "b" => font.bold = true,
        "i" => font.italic = true,
        "u" => font.underline = true,
        "s" => font.strike = true,
        "o" => font.overline = true,
        "sub" => font.sub = true,
        "sup" => font.sup = true,
        "font" => {
            if let Some(c) = attr(attrs, "color").and_then(|c| parse_color(c, scheme)) {
                font.color = Some(c);
            }
            if let Some(sz) = attr(attrs, "point-size")
                .and_then(|v| v.trim().parse::<f64>().ok())
                .filter(|v| v.is_finite() && *v > 0.0)
            {
                font.size = sz.min(400.0);
            }
            if let Some(face) = attr(attrs, "face") {
                font = font.with_name(face);
            }
        }
        _ => return None,
    }
    Some(font)
}

pub(crate) fn attr<'a>(attrs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

fn just_of(attrs: &[(String, String)]) -> Option<Just> {
    match attr(attrs, "align")?.to_ascii_lowercase().as_str() {
        "left" => Some(Just::Left),
        "right" => Some(Just::Right),
        "center" => Some(Just::Center),
        _ => None,
    }
}

/// HTML のラベルを読む。`base` はノードの書体、`scheme` は色の配色。表は中の文字を、行（`<tr>`）ごとに区切りを
/// 入れて並べる。
pub fn html_label(src: &str, base: &Font, scheme: Option<&str>) -> Text {
    html_label_with_images(src, base, scheme, &mut |_| None)
}

/// `html_label` の、表の `<IMG>` を `images`（`SRC` の値から画像）で読むもの。読めない画像は枠にして
/// `Text::image` を立てる。
pub fn html_label_with_images(
    src: &str,
    base: &Font,
    scheme: Option<&str>,
    images: &mut dyn FnMut(&str) -> Option<std::sync::Arc<crate::image::Picture>>,
) -> Text {
    if let Some(t) = crate::html::label_with_images(src, base, scheme, images) {
        return t;
    }
    inline_label(src, base, scheme)
}

/// 表を含まない HTML のラベル（表があれば中の文字だけを並べる）。
pub(crate) fn inline_label(src: &str, base: &Font, scheme: Option<&str>) -> Text {
    let mut p = Html { s: src, i: 0 };
    let mut fonts: Vec<Font> = vec![base.clone()];
    // 開いた要素の名前（閉じで飾りを戻すため。飾りを変えない要素は None）
    let mut open: Vec<(String, bool)> = Vec::new();
    let mut text = Text::default();
    let mut line = Line {
        spans: vec![],
        just: Just::Center,
        width: 0.0,
        height: 0.0,
        size: base.size,
    };
    // 表の中の深さと、行の中のセルの数
    let mut table = 0usize;
    let mut cells_in_row = 0usize;
    let mut table_just: Vec<Just> = Vec::new();
    let push_line = |text: &mut Text, line: &mut Line, just: Just| {
        let mut next = Line {
            spans: vec![],
            just,
            width: 0.0,
            height: 0.0,
            size: line.size,
        };
        std::mem::swap(line, &mut next);
        text.lines.push(next);
    };
    let mut pieces = 0usize;
    while let Some(piece) = p.next() {
        pieces += 1;
        if pieces > 100_000 {
            break;
        }
        match piece {
            Piece::Text(t) => {
                // HTML の空白はまとめる（改行も空白）
                let mut s = String::new();
                let mut space = false;
                for c in t.chars() {
                    if c.is_whitespace() {
                        space = true;
                    } else {
                        // 前の span が空白で終わっていれば重ねない
                        if space
                            && (!s.is_empty()
                                || line.spans.last().is_some_and(|l| !l.text.ends_with(' ')))
                        {
                            s.push(' ');
                        }
                        space = false;
                        s.push(c);
                    }
                }
                if space && !s.is_empty() {
                    s.push(' ');
                }
                // 要素の間の空白だけの切れ端も 1 つの空白（`</u> <font>` など）
                if s.is_empty()
                    && space
                    && line.spans.last().is_some_and(|l| !l.text.ends_with(' '))
                {
                    s.push(' ');
                }
                let s = decode_entities(&s);
                if s.is_empty() {
                    continue;
                }
                let font = fonts.last().cloned().unwrap_or_else(|| base.clone());
                let width = measure(&s, &font);
                line.spans.push(Span {
                    text: s,
                    font,
                    width,
                });
            }
            Piece::Empty(name, attrs) => match name.as_str() {
                "br" => {
                    let just = just_of(&attrs).unwrap_or(Just::Center);
                    line.just = just;
                    push_line(&mut text, &mut line, Just::Center);
                }
                "img" => text.image = true,
                _ => {}
            },
            Piece::Open(name, attrs) => {
                let current = fonts.last().cloned().unwrap_or_else(|| base.clone());
                let mut font = current.clone();
                let mut styled = true;
                if let Some(f) = self::styled(&current, &name, &attrs, scheme) {
                    font = f;
                } else {
                    match name.as_str() {
                        "table" => {
                            text.table = true;
                            if table == 0 && !line.spans.is_empty() {
                                push_line(&mut text, &mut line, Just::Center);
                            }
                            table += 1;
                            styled = false;
                        }
                        "tr" => {
                            cells_in_row = 0;
                            styled = false;
                        }
                        "td" => {
                            cells_in_row += 1;
                            if cells_in_row > 1 && !line.spans.is_empty() {
                                let font = fonts.last().cloned().unwrap_or_else(|| base.clone());
                                let width = measure("  ", &font);
                                line.spans.push(Span {
                                    text: "  ".into(),
                                    font,
                                    width,
                                });
                            }
                            table_just.push(just_of(&attrs).unwrap_or(Just::Center));
                            styled = false;
                        }
                        _ => styled = false,
                    }
                }
                if styled {
                    fonts.push(font);
                }
                open.push((name, styled));
            }
            Piece::Close(name) => {
                // 対応する開きまで戻る（閉じ忘れを許す）
                if let Some(at) = open.iter().rposition(|(n, _)| *n == name) {
                    for (n, styled) in open.drain(at..).rev() {
                        if styled && fonts.len() > 1 {
                            fonts.pop();
                        }
                        match n.as_str() {
                            "tr" if !line.spans.is_empty() => {
                                push_line(&mut text, &mut line, Just::Center)
                            }
                            "td" => {
                                if let Some(j) = table_just.pop()
                                    && cells_in_row == 1
                                {
                                    line.just = j;
                                }
                            }
                            "table" => {
                                table = table.saturating_sub(1);
                                if !line.spans.is_empty() {
                                    push_line(&mut text, &mut line, Just::Center);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    if !line.spans.is_empty() || text.lines.is_empty() {
        text.lines.push(line);
    }
    // 行の終わりの空白は描かない（幅にも入れない。`<td> a </td>` の後ろの空白など）
    for line in &mut text.lines {
        while let Some(last) = line.spans.last_mut() {
            let trimmed = last.text.trim_end_matches(' ');
            if trimmed.len() == last.text.len() {
                break;
            }
            if trimmed.is_empty() {
                line.spans.pop();
            } else {
                last.text.truncate(trimmed.len());
                last.width = measure(&last.text, &last.font);
                break;
            }
        }
    }
    // 空の行を除く（表の区切りで生まれたもの）。ただし `<br/>` の空行は残す
    if text.table {
        text.lines.retain(|l| !l.spans.is_empty());
        if text.lines.is_empty() {
            text.lines.push(Line {
                spans: vec![],
                just: Just::Center,
                width: 0.0,
                height: 0.0,
                size: base.size,
            });
        }
    }
    text.finish(base.size)
}

// ---- SVG ----

/// `fill` などの色の属性（文の色。テーマの色は呼ぶ側の既定）。
pub fn color_attr(name: &str, c: &Color) -> String {
    match c {
        Color::None => format!(" {name}=\"none\""),
        Color::Rgba(rgba) => {
            let mut s = format!(" {name}=\"{}\"", rgba.hex());
            if let Some(o) = rgba.opacity() {
                s.push_str(&format!(" {name}-opacity=\"{o}\""));
            }
            s
        }
    }
}

/// 空白を SVG で縮めてしまう文字（両端の空白・続く空白）か。そうなら `xml:space="preserve"` を付ける。span の
/// 並ぶ行はいつも付ける（librsvg は `<tspan>` の両端の空白を、間の空白でも縮めてしまう）。
fn needs_preserve(line: &Line, single: bool) -> bool {
    !single
        || line.spans.iter().any(|s| {
            let t = s.text.as_str();
            t.contains("  ") || t.starts_with(' ') || t.ends_with(' ')
        })
}

fn span_attrs(font: &Font, root: &Font, out: &mut String) {
    if (font.size - root.size).abs() > 0.01 || font.sub || font.sup {
        let size = if font.sub || font.sup {
            font.size / 1.2
        } else {
            font.size
        };
        out.push_str(&format!(" font-size=\"{}\"", num(size)));
    }
    if font.bold && !root.bold {
        out.push_str(" font-weight=\"bold\"");
    }
    if font.italic && !root.italic {
        out.push_str(" font-style=\"italic\"");
    }
    if font.mono && !root.mono {
        out.push_str(" font-family=\"var(--_fm)\"");
    }
    if font.color != root.color
        && let Some(c) = &font.color
    {
        out.push_str(&color_attr("fill", c));
    }
    let lines: Vec<&str> = [
        (font.underline, "underline"),
        (font.overline, "overline"),
        (font.strike, "line-through"),
    ]
    .iter()
    .filter(|(on, _)| *on)
    .map(|(_, name)| *name)
    .collect();
    if !lines.is_empty() {
        out.push_str(&format!(" text-decoration=\"{}\"", lines.join(" ")));
    }
}

/// 下線・上線・打ち消し線を、文字（左 `left`・幅 `w`・基線 `base`）に添える線で描く。色は文字と同じ。CoreSVG は
/// `text-decoration` を読まないので、その代わり（`data-coresvg-only` の組に入れ、librsvg には渡さない）。
fn decorations(
    out: &mut String,
    left: f64,
    w: f64,
    base: f64,
    font: &Font,
    root: &Font,
    ink: Option<&str>,
) {
    if !(font.underline || font.strike || font.overline) || w <= 0.0 {
        return;
    }
    let size = if font.sub || font.sup {
        font.size / 1.2
    } else {
        font.size
    };
    let mut d = String::new();
    for (on, dy) in [
        (font.underline, 0.15),
        (font.strike, -0.3),
        (font.overline, -0.85),
    ] {
        if on {
            d.push_str(&format!(
                "M{},{}h{}",
                num(left),
                num(base + dy * size),
                num(w)
            ));
        }
    }
    let stroke = match (&font.color, &root.color, ink) {
        (Some(c), _, _) | (None, Some(c), _) => color_attr("stroke", c),
        (None, None, Some(c)) => format!(" stroke=\"{c}\""),
        (None, None, None) => " stroke=\"var(--_fg)\"".to_owned(),
    };
    out.push_str(&format!(
        "<path d=\"{d}\" fill=\"none\"{stroke} stroke-width=\"{}\"/>",
        num((size / 14.0).max(0.5))
    ));
}

/// 文字を (cx, cy) を中心に書く。`box_w` は寄せの幅（ラベルより広い枠の中で左・右に寄せる）。`root` はまわりの
/// `<text>` の書体（色・大きさ）。`ink` は文が色を決めていないときの文字の色（下の塗りで読めるように。表の
/// `bgcolor` があれば表の中ではそちらが勝つ）。
///
/// 飾りの違う span が並ぶ行は、1 つの `<text>` に流れる `<tspan>`（x なし）で書き、飾りの線は `text-decoration`
/// にする。描き手の書体が測りより広くても、span の間の空白はつぶれない（librsvg）。CoreSVG は `<tspan>` の飾りも
/// `text-decoration` も読まないので、アプリは span ごとの `<text>` に分けて描く（core の `svgcss`）。そのために
/// 各 `<tspan>` に測った左端（両端の空白を除いた文字の左）を `data-x` で添え、飾りの線を `data-coresvg-only` の
/// `<g>` に入れて添える。
pub fn text_svg(
    text: &Text,
    cx: f64,
    cy: f64,
    box_w: f64,
    root: &Font,
    attrs: &str,
    ink: Option<&str>,
) -> String {
    let mut out = String::new();
    if let Some(table) = &text.html {
        crate::html::draw(
            table,
            cx - table.w / 2.0,
            cy - table.h / 2.0,
            root,
            attrs,
            ink,
            &mut out,
        );
        return out;
    }
    let ink_attr = match ink {
        Some(c) if root.color.is_none() => format!(" fill=\"{c}\""),
        _ => String::new(),
    };
    let mut top = cy - text.height / 2.0;
    let half = box_w.max(text.width) / 2.0;
    // CoreSVG だけが描く飾りの線
    let mut lines = String::new();
    for line in &text.lines {
        let base = top + line.height / 2.0 + line.size * 0.35;
        top += line.height;
        if line.spans.is_empty() {
            continue;
        }
        let single = line.spans.len() == 1 && !line.spans[0].font.sub && !line.spans[0].font.sup;
        let (x, anchor) = match line.just {
            Just::Center => (cx, "middle"),
            Just::Left => (cx - half, "start"),
            Just::Right => (cx + half, "end"),
        };
        // 測った行の左端
        let mut at = match line.just {
            Just::Center => cx - line.width / 2.0,
            Just::Left => cx - half,
            Just::Right => cx + half - line.width,
        };
        // 1 つの span の行は span の色を `<text>` に書くので、そのときは塗りの上の色を書かない
        let own_color = single && line.spans[0].font.color.is_some();
        out.push_str(&format!(
            "<text{attrs}{}{} x=\"{}\" y=\"{}\" text-anchor=\"{anchor}\"",
            if own_color { "" } else { ink_attr.as_str() },
            if needs_preserve(line, single) {
                " xml:space=\"preserve\""
            } else {
                ""
            },
            num(x),
            num(base)
        ));
        if single {
            let span = &line.spans[0];
            span_attrs(&span.font, root, &mut out);
            out.push('>');
            out.push_str(&esc(&span.text));
            out.push_str("</text>");
            let lead = span.text.len() - span.text.trim_start_matches(' ').len();
            let drawn = span.text.trim_matches(' ');
            decorations(
                &mut lines,
                at + measure(&" ".repeat(lead), &span.font),
                measure(drawn, &span.font),
                base,
                &span.font,
                root,
                ink,
            );
            continue;
        }
        out.push('>');
        let mut shifted = 0.0f64;
        for span in &line.spans {
            // span の文はそのまま（空白も）流す。`data-x` は両端の空白を除いた文字の左
            let drawn = span.text.trim_matches(' ');
            let lead = span.text.len() - span.text.trim_start_matches(' ').len();
            let left = at + measure(&" ".repeat(lead), &span.font);
            at += span.width;
            out.push_str(&format!("<tspan data-x=\"{}\"", num(left)));
            span_attrs(&span.font, root, &mut out);
            let want = if span.font.sub {
                span.font.size * 0.3
            } else if span.font.sup {
                -span.font.size * 0.4
            } else {
                0.0
            };
            if (want - shifted).abs() > 0.001 {
                out.push_str(&format!(" dy=\"{}\"", num(want - shifted)));
                shifted = want;
            }
            out.push('>');
            out.push_str(&esc(&span.text));
            out.push_str("</tspan>");
            decorations(
                &mut lines,
                left,
                measure(drawn, &span.font),
                base + want,
                &span.font,
                root,
                ink,
            );
        }
        out.push_str("</text>");
    }
    if !lines.is_empty() {
        out.push_str("<g data-coresvg-only=\"1\">");
        out.push_str(&lines);
        out.push_str("</g>");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_escaped_lines() {
        let names = Names {
            graph: "G",
            node: "n1",
            ..Default::default()
        };
        let lines = escaped_lines("a\\lb\\rc\\nd\\N \\G \\\\ \\x &amp;", &names);
        assert_eq!(
            lines,
            vec![
                ("a".into(), Just::Left),
                ("b".into(), Just::Right),
                ("c".into(), Just::Center),
                ("dn1 G \\ x &".into(), Just::Center),
            ]
        );
        // 終わりの `\l` は空の行を作らない
        assert_eq!(escaped_lines("x\\l", &names).len(), 1);
        assert_eq!(
            escaped_lines("", &names),
            vec![(String::new(), Just::Center)]
        );
        assert_eq!(escaped_lines("x\\n\\n", &names).len(), 2);
    }

    #[test]
    fn measures_plain_labels() {
        let font = Font::new(14.0);
        let t = plain_label("abc\\nde", &Names::default(), &font);
        assert_eq!(t.lines.len(), 2);
        assert!((t.height - 2.0 * 14.0 * 1.2).abs() < 1e-9);
        assert!(t.width > 0.0 && t.lines[0].width > t.lines[1].width);
    }

    #[test]
    fn reads_html_labels() {
        let font = Font::new(14.0);
        let t = html_label(
            "<b>bold</b> and <i>it</i><br align=\"left\"/><font color=\"red\" point-size=\"20\">big</font>x<sub>2</sub>",
            &font,
            None,
        );
        assert_eq!(t.lines.len(), 2);
        assert_eq!(t.lines[0].just, Just::Left);
        assert!(t.lines[0].spans[0].font.bold);
        assert_eq!(t.lines[0].spans[1].text, " and ");
        assert!(t.lines[0].spans[2].font.italic);
        let big = &t.lines[1].spans[0];
        assert_eq!(big.font.size, 20.0);
        assert!(matches!(big.font.color, Some(Color::Rgba(c)) if c.hex() == "#ff0000"));
        assert!(t.lines[1].spans[2].font.sub);
        assert!(!t.table);
        let svg = text_svg(&t, 0.0, 0.0, 0.0, &font, "", None);
        assert!(svg.contains("font-weight=\"bold\"") && svg.contains("fill=\"#ff0000\""));
        assert!(svg.contains("dy="), "{svg}");
    }

    #[test]
    fn flattens_html_tables_inline() {
        // 表の中の文字だけを並べる（表を組めないときの道）
        let font = Font::new(14.0);
        let t = inline_label(
            "<TABLE BORDER=\"0\"><TR><TD>a</TD><TD>b</TD></TR><TR><TD ALIGN=\"LEFT\">c &amp; d</TD></TR></TABLE>",
            &font,
            None,
        );
        assert!(t.table);
        let lines: Vec<String> = t
            .lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect())
            .collect();
        assert_eq!(lines, vec!["a  b", "c & d"]);
        assert_eq!(t.lines[1].just, Just::Left);
        // 壊れた HTML でも止まらない
        for src in [
            "<b>",
            "</i>x",
            "<font color=>",
            "<",
            "<td><tr></table>",
            "a<!-- c",
        ] {
            let _ = html_label(src, &font, None);
        }
    }

    #[test]
    fn draws_decorations_as_lines() {
        // 文字には text-decoration を書き、CoreSVG（読まない）のための線を `data-coresvg-only` の組に添える
        // （色は文字の色）
        let font = Font::new(14.0);
        let t = html_label(
            "plain <u>under</u> <font color=\"red\"><s>gone</s></font>",
            &font,
            None,
        );
        let svg = text_svg(&t, 0.0, 0.0, 0.0, &font, "", Some("#1f1f1f"));
        assert!(
            svg.contains("text-decoration=\"underline\">under</tspan>"),
            "{svg}"
        );
        assert!(
            svg.contains("text-decoration=\"line-through\">gone</tspan>"),
            "{svg}"
        );
        let (text, group) = svg.split_once("<g data-coresvg-only=\"1\">").unwrap();
        assert!(!text.contains("<path"), "{svg}");
        let paths: Vec<&str> = group.split("<path").skip(1).collect();
        assert_eq!(paths.len(), 2, "{svg}");
        assert!(paths[0].contains("stroke=\"#1f1f1f\""), "{svg}");
        assert!(paths[1].contains("stroke=\"#ff0000\""), "{svg}");
        // 下線は文字の幅だけ（span の両端の空白は含めない）
        let w = measure("under", &font);
        assert!(paths[0].contains(&format!("h{}", num(w))), "{svg}");
        let t = html_label("<o>top</o>", &font, None);
        let svg = text_svg(&t, 10.0, 0.0, 0.0, &font, "", None);
        assert!(
            svg.contains("text-decoration=\"overline\">top</text>"),
            "{svg}"
        );
        assert!(svg.contains("stroke=\"var(--_fg)\""), "{svg}");
        let w = measure("top", &font);
        assert!(svg.contains(&format!("M{},", num(10.0 - w / 2.0))), "{svg}");
    }

    #[test]
    fn mixed_lines_flow_with_hints() {
        // span の並ぶ行は 1 つの `<text>` に流れる `<tspan>`（x なし、空白も文の中）で書き、左端を `data-x` で添える
        let font = Font::new(14.0);
        let t = html_label("<b>bold</b> and <i>italic</i>", &font, None);
        let svg = text_svg(&t, 0.0, 0.0, 0.0, &font, "", None);
        assert_eq!(svg.matches("<text").count(), 1, "{svg}");
        assert!(svg.contains("text-anchor=\"middle\""), "{svg}");
        assert!(!svg.contains("<tspan x="), "{svg}");
        assert!(svg.contains(">bold</tspan><tspan data-x="), "{svg}");
        assert!(svg.contains("> and </tspan>"), "{svg}");
        let left = -t.lines[0].width / 2.0;
        assert!(
            svg.contains(&format!("<tspan data-x=\"{}\"", num(left))),
            "{svg}"
        );
        let and = left + measure("bold", &t.lines[0].spans[0].font) + measure(" ", &font);
        assert!(
            svg.contains(&format!("<tspan data-x=\"{}\">", num(and))),
            "{svg}"
        );
        assert!(svg.contains("xml:space=\"preserve\""), "{svg}");
    }

    #[test]
    fn keeps_runs_of_spaces() {
        let font = Font::new(14.0);
        let t = plain_label("a  b", &Names::default(), &font);
        assert!(text_svg(&t, 0.0, 0.0, 0.0, &font, "", None).contains("xml:space=\"preserve\""));
        let t = plain_label("a b", &Names::default(), &font);
        assert!(!text_svg(&t, 0.0, 0.0, 0.0, &font, "", None).contains("xml:space"));
    }
}
