//! ラベルの行分けと幅（pele の `src/text/label.ts`）。インラインの HTML と、Markdown のラベルでは強調の印を、
//! 飾りごとの span に分け、幅を超える行を折り返す。
//!
//! JS の正規表現（`RE_PIECE` など）は手で書いた走査に置き換えた。文字は `char` で数える（比べる文字はどれも BMP
//! なので、UTF-16 で数えるのと同じ所で分かれる）。幅は測り手が UTF-16 で数える。

use std::cell::RefCell;

use super::entities::decode_entities;
use super::icons::icon_markup;
use super::math::{math_active, typeset};
use super::measurer::{TextMeasurer, style as st};
use crate::js;
use crate::types::MathMarkup;
use crate::util::chars::{is_space_char, is_word_char};

#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub text: String,
    pub style: u32,
    pub icon: Option<String>,
    pub math: Option<MathMarkup>,
    pub width: f64,
}

impl Span {
    fn text(text: String, style: u32, width: f64) -> Self {
        Span {
            text,
            style,
            icon: None,
            math: None,
            width,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    pub lines: Vec<Vec<Span>>,
    pub widths: Vec<f64>,
    pub width: f64,
    pub height: f64,
    pub size: f64,
    pub line_height: f64,
    /// 数式が行の高さを増すときだけある。
    pub heights: Option<Vec<f64>>,
}

impl Label {
    pub fn empty() -> Self {
        Label {
            lines: Vec::new(),
            widths: Vec::new(),
            width: 0.0,
            height: 0.0,
            size: 0.0,
            line_height: 0.0,
            heights: None,
        }
    }
}

pub const LINE_HEIGHT: f64 = 1.5;
/// 折り返した 1 つのラベルの行は詰めて、ひとかたまりに読めるようにする。
const WRAPPED_LINE_HEIGHT: f64 = 1.3;

fn tag_style(tag: &str) -> u32 {
    match tag {
        "b" | "strong" => st::BOLD,
        "i" | "em" | "cite" | "dfn" | "var" => st::ITALIC,
        "code" | "kbd" | "samp" | "tt" => st::MONO,
        "sub" => st::SUB,
        "sup" => st::SUP,
        "u" | "ins" => st::UNDERLINE,
        _ => 0,
    }
}

const SHIFT: u32 = st::SUB | st::SUP;

const HTML_TAGS: &[&str] = &[
    "a",
    "abbr",
    "address",
    "article",
    "aside",
    "audio",
    "b",
    "bdi",
    "bdo",
    "big",
    "blockquote",
    "body",
    "br",
    "button",
    "caption",
    "center",
    "cite",
    "code",
    "col",
    "colgroup",
    "data",
    "dd",
    "del",
    "details",
    "dfn",
    "dialog",
    "div",
    "dl",
    "dt",
    "em",
    "embed",
    "fieldset",
    "figcaption",
    "figure",
    "font",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hr",
    "html",
    "i",
    "iframe",
    "img",
    "input",
    "ins",
    "kbd",
    "label",
    "legend",
    "li",
    "link",
    "main",
    "map",
    "mark",
    "meta",
    "meter",
    "nav",
    "noscript",
    "object",
    "ol",
    "optgroup",
    "option",
    "output",
    "p",
    "picture",
    "pre",
    "progress",
    "q",
    "rp",
    "rt",
    "ruby",
    "s",
    "samp",
    "script",
    "section",
    "select",
    "small",
    "source",
    "span",
    "strike",
    "strong",
    "style",
    "sub",
    "summary",
    "sup",
    "table",
    "tbody",
    "td",
    "template",
    "textarea",
    "tfoot",
    "th",
    "thead",
    "time",
    "title",
    "tr",
    "track",
    "tt",
    "u",
    "ul",
    "var",
    "video",
    "wbr",
];

/// 中国語・日本語の文字。どの 2 文字の間でも折り返せる。ハングルは入れない（韓国語は語の間に空白を置く）。
fn is_cjk(c: char) -> bool {
    matches!(c, '\u{2e80}'..='\u{9fff}' | '\u{f900}'..='\u{faff}' | '\u{ff00}'..='\u{ff60}')
}

const CLOSE: &str = "、。，．：；！？）］｝」』】〕〉》〙〗ー…‥・ゝゞぁぃぅぇぉっゃゅょゎァィゥェォッャュョヮ,.:;!?)]}";
const OPEN: &str = "（［｛「『【〔〈《〘〖([{";

fn is_close(c: char) -> bool {
    CLOSE.contains(c)
}

fn is_open(c: char) -> bool {
    OPEN.contains(c)
}

/// 折り返しの手がかり: ゼロ幅の空白と、折り返した所で `-` になるソフトハイフン。
fn is_hint(c: char) -> bool {
    c == '\u{200b}' || c == '\u{ad}'
}

/// `RE_PIECE` の 1 つの選択肢の空白の後ろ（`q`）から。
fn piece_body(s: &[char], q: usize) -> Option<usize> {
    let n = s.len();
    // [OPEN]*[CJK][CLOSE]*
    let mut k = 0;
    while q + k < n && is_open(s[q + k]) {
        k += 1;
    }
    for t in (0..=k).rev() {
        if q + t < n && is_cjk(s[q + t]) {
            let mut e = q + t + 1;
            while e < n && is_close(s[e]) {
                e += 1;
            }
            return Some(e);
        }
    }
    // [^\sCJK HINT]+[HINT]*[CLOSE]*
    let mut e = q;
    while e < n && !is_space_char(s[e]) && !is_cjk(s[e]) && !is_hint(s[e]) {
        e += 1;
    }
    if e > q {
        while e < n && is_hint(s[e]) {
            e += 1;
        }
        while e < n && is_close(s[e]) {
            e += 1;
        }
        return Some(e);
    }
    // [HINT]+
    while e < n && is_hint(s[e]) {
        e += 1;
    }
    (e > q).then_some(e)
}

/// `text.match(RE_PIECE)`: 空白をはさんだ、折り返せる切れ目までのかけら。
fn pieces(text: &str) -> Vec<String> {
    let s: Vec<char> = text.chars().collect();
    let n = s.len();
    let mut out = Vec::new();
    let mut p = 0;
    while p < n {
        let mut q = p;
        while q < n && is_space_char(s[q]) {
            q += 1;
        }
        // `\s*` は長い方から試す（U+3000 は空白でも CJK でもある）。
        let mut found = None;
        let mut qq = q;
        loop {
            if let Some(e) = piece_body(&s, qq) {
                let mut e = e;
                while e < n && is_space_char(s[e]) {
                    e += 1;
                }
                found = Some(e);
                break;
            }
            if qq == p {
                break;
            }
            qq -= 1;
        }
        let end = match found {
            Some(e) => e,
            None if q > p => q,
            None => p + 1,
        };
        if found.is_some() || q > p {
            out.push(s[p..end].iter().collect());
        }
        p = end;
    }
    out
}

/// 一意の置き字で TeX を印の読みから守り、タグを除いても式の位置がそろうようにする。
const MARK: u32 = 0xe000;
const MAX_MARKS: usize = 0x1900;

fn is_mark(c: char) -> bool {
    ('\u{e000}'..='\u{f8ff}').contains(&c)
}

#[derive(Clone)]
struct Marks {
    tex: Vec<String>,
    math: Vec<Option<MathMarkup>>,
}

thread_local! {
    static MARKS: RefCell<Option<Marks>> = const { RefCell::new(None) };
}

fn dedent(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let indent_of = |line: &str| line.chars().count() - js::trim_start(line).chars().count();
    let mut min = usize::MAX;
    for line in &lines {
        if js::trim(line).is_empty() {
            continue;
        }
        min = min.min(indent_of(line));
    }
    if min == usize::MAX || min == 0 {
        return text.to_string();
    }
    lines
        .iter()
        .map(|line| {
            let k = min.min(indent_of(line));
            line.chars().skip(k).collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn push_text(line: &mut Vec<Span>, text: &str, style: u32) {
    if text.is_empty() {
        return;
    }
    let marks = MARKS.with(|m| m.borrow().clone());
    if let Some(found) = marks.filter(|_| text.chars().any(is_mark)) {
        // `text.split(/([-])/)`
        let mut part = String::new();
        for c in text.chars() {
            if !is_mark(c) {
                part.push(c);
                continue;
            }
            if !part.is_empty() {
                push_plain(line, &part, style);
            }
            part.clear();
            let at = (c as u32 - MARK) as usize;
            match found.math.get(at).cloned().flatten() {
                Some(math) => {
                    let w = math.width();
                    line.push(Span {
                        text: String::new(),
                        style,
                        icon: None,
                        math: Some(math),
                        width: w,
                    });
                }
                None => {
                    let tex = found
                        .tex
                        .get(at)
                        .cloned()
                        .unwrap_or_else(|| "undefined".into());
                    push_plain(line, &format!("$${tex}$$"), style);
                }
            }
        }
        if !part.is_empty() {
            push_plain(line, &part, style);
        }
        return;
    }
    push_plain(line, text, style);
}

fn push_plain(line: &mut Vec<Span>, text: &str, style: u32) {
    let decoded = decode_entities(text);
    let last = line.last_mut();
    match last {
        Some(last) if last.icon.is_none() && last.math.is_none() && last.style == style => {
            last.text.push_str(&decoded)
        }
        _ => line.push(Span::text(decoded, style, 0.0)),
    }
}

/// `/(fa[bklrs]?):fa-([\w-]+)/` が `s[i..]` で合えば、終わりと、アイコンの名前。
fn icon_at(s: &[char], i: usize) -> Option<(usize, String)> {
    if s.get(i) != Some(&'f') || s.get(i + 1) != Some(&'a') {
        return None;
    }
    let mut p = i + 2;
    if s.get(p).is_some_and(|c| "bklrs".contains(*c)) {
        p += 1;
    }
    let prefix: String = s[i..p].iter().collect();
    if s.get(p..p + 4) != Some(&[':', 'f', 'a', '-']) {
        return None;
    }
    let start = p + 4;
    let mut e = start;
    while e < s.len() && (is_word_char(s[e]) || s[e] == '-') {
        e += 1;
    }
    if e == start {
        return None;
    }
    let name: String = s[start..e].iter().collect();
    Some((e, format!("{prefix}:fa-{name}")))
}

fn push_icons(line: &mut Vec<Span>, text: &str, style: u32) {
    if !text.contains(":fa-") {
        push_text(line, text, style);
        return;
    }
    let s: Vec<char> = text.chars().collect();
    let slice = |a: usize, b: usize| -> String { s[a..b].iter().collect() };
    let mut last = 0;
    let mut i = 0;
    while i < s.len() {
        let Some((end, icon)) = icon_at(&s, i) else {
            i += 1;
            continue;
        };
        push_text(line, &slice(last, i), style);
        last = end;
        if !icon_markup(&icon).is_empty() {
            line.push(Span {
                text: String::new(),
                style,
                icon: Some(icon),
                math: None,
                width: 0.0,
            });
        } else if s.get(last) == Some(&' ') {
            // ホストが持たないアイコンは、隙間も、文字と分けていた空白も残さない。
            last += 1;
        } else if last == s.len()
            && let Some(before) = line
                .last_mut()
                .filter(|b| b.icon.is_none() && b.math.is_none())
        {
            before.text = js::trim_end(&before.text).to_string();
        }
        i = end;
    }
    push_text(line, &slice(last, s.len()), style);
}

struct Open {
    style: u32,
    /// 下付き・上付きの積み重ね。いちばん内側のタグだけが文字をずらす。
    shifts: Vec<u32>,
}

/// `/<\/?([a-zA-Z][\w-]*)(?:\s[^<>]*)?\/?>/y` が `s[i..]` で合えば、終わりとタグの名前。
fn tag_at(s: &[char], i: usize) -> Option<(usize, String)> {
    let mut p = i + 1;
    if s.get(p) == Some(&'/') {
        p += 1;
    }
    if !s.get(p).is_some_and(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let start = p;
    p += 1;
    while p < s.len() && (is_word_char(s[p]) || s[p] == '-') {
        p += 1;
    }
    let name: String = s[start..p].iter().collect();
    if s.get(p).is_some_and(|&c| is_space_char(c)) {
        let mut q = p + 1;
        while q < s.len() && s[q] != '<' && s[q] != '>' {
            q += 1;
        }
        if s.get(q) == Some(&'>') {
            return Some((q + 1, name));
        }
        return None;
    }
    if s.get(p) == Some(&'/') {
        p += 1;
    }
    if s.get(p) == Some(&'>') {
        return Some((p + 1, name));
    }
    None
}

fn is_blank_str(c: Option<char>) -> bool {
    // `ch.trim() === ''`
    c.is_none_or(js::is_js_space)
}

/// インラインの HTML と、Markdown のラベルでは強調の印を、飾りごとの span に分ける。
fn inline(line: &mut Vec<Span>, source: &str, markdown: bool, base: u32, open: &mut Open) {
    let s: Vec<char> = source.chars().collect();
    let n = s.len();
    let mut style = open.style;
    let mut marked = 0u32;
    let mut text = String::new();
    let mut i = 0;
    let flush = |line: &mut Vec<Span>, text: &mut String, style: u32, shifts: &Vec<u32>| {
        push_icons(line, text, style | shifts.last().copied().unwrap_or(0));
        text.clear();
    };
    while i < n {
        let ch = s[i];
        if ch == '<' {
            if let Some((end, tag)) = tag_at(&s, i) {
                let tag = tag.to_lowercase();
                let closing = s.get(i + 1) == Some(&'/');
                let bit = tag_style(&tag);
                if bit & SHIFT != 0 {
                    flush(line, &mut text, style, &open.shifts);
                    let at = open.shifts.iter().rposition(|&b| b == bit);
                    if !closing {
                        open.shifts.push(bit);
                    } else if let Some(at) = at {
                        open.shifts.remove(at);
                    }
                } else if bit != 0 {
                    flush(line, &mut text, style, &open.shifts);
                    open.style = if closing {
                        (open.style & !bit) | (base & bit)
                    } else {
                        open.style | bit
                    };
                    style = if closing {
                        (style & !bit) | (base & bit)
                    } else {
                        style | bit
                    };
                }
                // ほかの HTML のタグは捨てて文字を残す。List<T> の型のようなタグに似たほかのものは文字。
                if bit != 0 || HTML_TAGS.contains(&tag.as_str()) {
                    i = end;
                    continue;
                }
            }
        } else if markdown && (ch == '*' || ch == '_') {
            let double = s.get(i + 1) == Some(&ch);
            let bit = if double { st::BOLD } else { st::ITALIC };
            let len = if double { 2 } else { 1 };
            let opening = marked & bit == 0;
            let prev = if i > 0 { Some(s[i - 1]) } else { Some(' ') };
            let next = if i + len < n {
                Some(s[i + len])
            } else {
                Some(' ')
            };
            let needle: Vec<char> = if double { vec![ch, ch] } else { vec![ch] };
            let found = (i + len..n).any(|k| s[k..].starts_with(&needle));
            let can_open = opening && !is_blank_str(next) && found;
            let can_close = !opening && !is_blank_str(prev);
            let wordy =
                ch == '_' && prev.is_some_and(is_word_char) && next.is_some_and(is_word_char);
            if (can_open || can_close) && !wordy {
                flush(line, &mut text, style, &open.shifts);
                marked = if can_open {
                    marked | bit
                } else {
                    marked & !bit
                };
                style = if can_open {
                    style | bit
                } else {
                    (style & !bit) | (base & bit)
                };
                i += len;
                continue;
            }
        }
        text.push(ch);
        i += 1;
    }
    flush(line, &mut text, style, &open.shifts);
}

fn measure_spans(line: &mut [Span], measurer: &dyn TextMeasurer, size: f64) -> f64 {
    let mut total = 0.0;
    for span in line.iter_mut() {
        span.width = if span.icon.is_some() {
            size * 1.25
        } else if let Some(m) = &span.math {
            m.width()
        } else {
            measurer.width(&span.text, size, span.style)
        };
        total += span.width;
    }
    total
}

fn ends_shy(s: &str) -> bool {
    s.ends_with('\u{ad}')
}

/// 最後の 1 文字（UTF-16 の 1 単位）を除く。
fn drop_last(s: &str) -> &str {
    match s.char_indices().next_back() {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

struct Wrap<'a> {
    current: Vec<Span>,
    width: f64,
    measurer: &'a dyn TextMeasurer,
    size: f64,
}

impl Wrap<'_> {
    fn commit(&mut self, hyphen: bool, out: &mut Vec<Vec<Span>>, widths: &mut Vec<f64>) {
        if let Some(last) = self
            .current
            .last_mut()
            .filter(|l| l.icon.is_none() && l.math.is_none())
        {
            let mut trimmed = js::trim_end(&last.text).to_string();
            if hyphen && ends_shy(&last.text) {
                trimmed = format!("{}-", drop_last(&trimmed));
            }
            if trimmed != last.text {
                let w = self.measurer.width(&trimmed, self.size, last.style);
                last.text = trimmed;
                self.width += w - last.width;
                last.width = w;
            }
        }
        out.push(std::mem::take(&mut self.current));
        widths.push(self.width);
        self.width = 0.0;
    }
}

fn wrap(
    line: &[Span],
    measurer: &dyn TextMeasurer,
    size: f64,
    max: f64,
    out: &mut Vec<Vec<Span>>,
    widths: &mut Vec<f64>,
) {
    let mut wr = Wrap {
        current: Vec::new(),
        width: 0.0,
        measurer,
        size,
    };
    let out_before = out.len();
    for (si, span) in line.iter().enumerate() {
        if span.icon.is_some() || span.math.is_some() {
            let w = match &span.math {
                Some(m) => m.width(),
                None => size * 1.25,
            };
            if wr.width > 0.0 && wr.width + w > max {
                wr.commit(false, out, widths);
            }
            let mut s = span.clone();
            s.width = w;
            wr.current.push(s);
            wr.width += w;
            continue;
        }
        let pieces = pieces(&span.text);
        // ソフトハイフンで終わるかけらは、その `-` が収まるか、語の残りが収まって（ハイフンを隠して）収まる。
        // 行は語に沿ってしか伸びないので、その語の後のハイフンも収まらず、各語の残りの幅でこの 1 つの確かめで済む。
        let soft = span.text.contains('\u{ad}');
        let n = if soft { pieces.len() } else { 0 };
        let mut own = vec![0.0f64; n];
        let mut rest = vec![0.0f64; n + 1];
        for q in (0..n).rev() {
            // 空白の前のソフトハイフンは、それ自身の切れ目ではない（空白がそう）。
            let shy = ends_shy(&pieces[q]);
            let end = js::trim_end(&pieces[q]);
            own[q] = measurer.width(if shy { drop_last(end) } else { end }, size, span.style);
            // この span の先の語の幅は分からない。
            rest[q] = if !shy {
                own[q]
            } else if q == n - 1 {
                if si == line.len() - 1 {
                    own[q]
                } else {
                    f64::INFINITY
                }
            } else {
                own[q] + rest[q + 1]
            };
        }
        for (p, piece) in pieces.iter().enumerate() {
            let mut part: &str = piece;
            let part_width = if soft {
                own[p]
            } else {
                measurer.width(js::trim_end(part), size, span.style)
            };
            let fits = if !soft || !ends_shy(part) {
                wr.width + part_width <= max
            } else {
                wr.width + measurer.width(&format!("{}-", drop_last(part)), size, span.style) <= max
                    || wr.width + rest[p] <= max
            };
            if wr.width > 0.0 && !fits {
                // 空白で折り返すと、前のソフトハイフンは隠れたまま。
                wr.commit(js::trim_start(part) == part, out, widths);
                part = js::trim_start(part);
            }
            match wr.current.last_mut() {
                Some(last)
                    if last.icon.is_none() && last.math.is_none() && last.style == span.style =>
                {
                    last.text.push_str(part);
                    // ゼロ幅のかけらのたびに測り直すと、2 乗の時間がかかる。
                    if part_width > 0.0 || js::trim_end(part) != part {
                        let before = last.width;
                        last.width = measurer.width(&last.text, size, last.style);
                        wr.width += last.width - before;
                    }
                }
                _ => {
                    let w = measurer.width(part, size, span.style);
                    wr.current.push(Span::text(part.to_string(), span.style, w));
                    wr.width += w;
                }
            }
        }
    }
    if !wr.current.is_empty() || out.len() == out_before && out.is_empty() {
        wr.commit(false, out, widths);
    }
}

pub fn layout_label(
    raw: Option<&str>,
    markdown: bool,
    measurer: &dyn TextMeasurer,
    size: f64,
    max_width: f64,
    base: u32,
) -> Label {
    let raw = match raw {
        Some(r) if !r.is_empty() => r,
        _ => return Label::empty(),
    };
    let line_height = js::round(size * LINE_HEIGHT);

    if math_active() && raw.contains("$$") && !raw.chars().any(is_mark) {
        let mut found = Marks {
            tex: Vec::new(),
            math: Vec::new(),
        };
        let marked = mark_math(raw, &mut found);
        if !found.tex.is_empty() {
            for tex in found.tex.clone() {
                found.math.push(typeset(&decode_entities(&tex), size));
            }
            return layout_markup(
                &marked,
                markdown,
                measurer,
                size,
                max_width,
                base,
                line_height,
                Some(found),
            );
        }
    }

    let markup = raw.contains(['<', '\\', '\n', '*', '_']) || has_icon_prefix(raw);
    if !markup && !raw.contains('&') && !raw.contains('ﬂ') {
        let text = js::trim(raw);
        let width = measurer.width(text, size, base);
        if width <= max_width
            || !(text.contains(' ') || text.chars().any(is_cjk) || text.chars().any(is_hint))
        {
            return Label {
                lines: vec![vec![Span::text(text.to_string(), base, width)]],
                widths: vec![width],
                width,
                height: line_height,
                size,
                line_height,
                heights: None,
            };
        }
        let mut lines = Vec::new();
        let mut widths = Vec::new();
        wrap(
            &[Span::text(text.to_string(), base, width)],
            measurer,
            size,
            max_width,
            &mut lines,
            &mut widths,
        );
        let mut widest = 0.0;
        for &w in &widths {
            if w > widest {
                widest = w;
            }
        }
        let snug = if lines.len() > 1 {
            js::round(size * WRAPPED_LINE_HEIGHT)
        } else {
            line_height
        };
        let height = lines.len() as f64 * snug;
        return Label {
            lines,
            widths,
            width: widest,
            height,
            size,
            line_height: snug,
            heights: None,
        };
    }
    layout_markup(
        raw,
        markdown,
        measurer,
        size,
        max_width,
        base,
        line_height,
        None,
    )
}

/// `/fa[bklrs]?:fa-/` を含むか。
fn has_icon_prefix(s: &str) -> bool {
    let c: Vec<char> = s.chars().collect();
    (0..c.len()).any(|i| {
        if c.get(i) != Some(&'f') || c.get(i + 1) != Some(&'a') {
            return false;
        }
        let mut p = i + 2;
        if c.get(p).is_some_and(|x| "bklrs".contains(*x))
            && c.get(p + 1..p + 5) == Some(&[':', 'f', 'a', '-'])
        {
            return true;
        }
        p = i + 2;
        c.get(p..p + 4) == Some(&[':', 'f', 'a', '-'])
    })
}

/// `raw.replace(/\$\$(.+?)\$\$/g, …)`: 式を置き字に。
fn mark_math(raw: &str, found: &mut Marks) -> String {
    let s: Vec<char> = raw.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < s.len() {
        if s[i] == '$' && s.get(i + 1) == Some(&'$') {
            // `.+?` は行の終わりの文字に合わない。
            let mut j = i + 2;
            let mut end = None;
            while j < s.len() && !matches!(s[j], '\n' | '\r' | '\u{2028}' | '\u{2029}') {
                if j > i + 2 && s[j] == '$' && s.get(j + 1) == Some(&'$') {
                    end = Some(j);
                    break;
                }
                j += 1;
            }
            if let Some(end) = end {
                let tex: String = s[i + 2..end].iter().collect();
                if found.tex.len() == MAX_MARKS {
                    out.extend(&s[i..end + 2]);
                } else {
                    found.tex.push(tex);
                    out.push(char::from_u32(MARK + found.tex.len() as u32 - 1).unwrap());
                }
                i = end + 2;
                continue;
            }
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn layout_markup(
    raw: &str,
    markdown: bool,
    measurer: &dyn TextMeasurer,
    size: f64,
    max_width: f64,
    base: u32,
    line_height: f64,
    found: Option<Marks>,
) -> Label {
    let has = found.is_some();
    let outer = MARKS.with(|m| m.replace(found));
    let label = layout_lines(raw, markdown, measurer, size, max_width, base, line_height);
    MARKS.with(|m| *m.borrow_mut() = outer);
    if has { with_heights(label) } else { label }
}

/// `source.split(/<br\s*\/?>|\\n|\n/gi)`
fn split_breaks(source: &str) -> Vec<String> {
    let s: Vec<char> = source.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut i = 0;
    while i < s.len() {
        if s[i] == '\n' {
            out.push(std::mem::take(&mut cur));
            i += 1;
            continue;
        }
        if s[i] == '\\' && s.get(i + 1) == Some(&'n') {
            out.push(std::mem::take(&mut cur));
            i += 2;
            continue;
        }
        if s[i] == '<'
            && s.get(i + 1).is_some_and(|c| *c == 'b' || *c == 'B')
            && s.get(i + 2).is_some_and(|c| *c == 'r' || *c == 'R')
        {
            let mut p = i + 3;
            while p < s.len() && is_space_char(s[p]) {
                p += 1;
            }
            if s.get(p) == Some(&'/') {
                p += 1;
            }
            if s.get(p) == Some(&'>') {
                out.push(std::mem::take(&mut cur));
                i = p + 1;
                continue;
            }
        }
        cur.push(s[i]);
        i += 1;
    }
    out.push(cur);
    out
}

fn layout_lines(
    raw: &str,
    markdown: bool,
    measurer: &dyn TextMeasurer,
    size: f64,
    max_width: f64,
    base: u32,
    line_height: f64,
) -> Label {
    let source = if markdown {
        let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
        js::trim(&dedent(&normalized)).to_string()
    } else {
        js::trim(raw).to_string()
    };
    let mut lines: Vec<Vec<Span>> = Vec::new();
    let mut widths: Vec<f64> = Vec::new();
    let mut width = 0.0;
    // HTML のタグは明示の改行をまたげる。Markdown の強調は行ごと。
    let mut open = Open {
        style: base,
        shifts: Vec::new(),
    };
    for part in split_breaks(&source) {
        let mut line: Vec<Span> = Vec::new();
        inline(&mut line, js::trim(&part), markdown, base, &mut open);
        let w = measure_spans(&mut line, measurer, size);
        if w > max_width {
            wrap(&line, measurer, size, max_width, &mut lines, &mut widths);
        } else {
            lines.push(line);
            widths.push(w);
        }
    }
    for &w in &widths {
        if w > width {
            width = w;
        }
    }
    let snug = if lines.len() > 1 {
        js::round(size * WRAPPED_LINE_HEIGHT)
    } else {
        line_height
    };
    let height = lines.len() as f64 * snug;
    Label {
        lines,
        widths,
        width,
        height,
        size,
        line_height: snug,
        heights: None,
    }
}

pub fn with_heights(label: Label) -> Label {
    let mut heights: Option<Vec<f64>> = None;
    for i in 0..label.lines.len() {
        let mut h = label.line_height;
        for span in &label.lines[i] {
            if let Some(m) = &span.math {
                h = js::max(h, (m.height() + label.size / 4.0).ceil());
            }
        }
        if h != label.line_height && heights.is_none() {
            heights = Some(vec![label.line_height; label.lines.len()]);
        }
        if let Some(hs) = heights.as_mut() {
            hs[i] = h;
        }
    }
    match heights {
        None => {
            if label.heights.is_none() {
                label
            } else {
                let height = label.lines.len() as f64 * label.line_height;
                Label {
                    heights: None,
                    height,
                    ..label
                }
            }
        }
        Some(hs) => {
            let mut height = 0.0;
            for &h in &hs {
                height += h;
            }
            Label {
                heights: Some(hs),
                height,
                ..label
            }
        }
    }
}

pub fn has_math(line: &[Span]) -> bool {
    line.iter().any(|s| s.math.is_some())
}

pub fn line_middle(label: &Label, i: usize) -> f64 {
    match &label.heights {
        None => (i as f64 + 0.5) * label.line_height,
        Some(h) => {
            let mut top = 0.0;
            for hh in &h[..i] {
                top += hh;
            }
            top + h[i] / 2.0
        }
    }
}

pub fn label_line(label: &Label, i: usize) -> Label {
    let width = label.widths[i];
    let height = match &label.heights {
        Some(h) => h[i],
        None => label.line_height,
    };
    Label {
        lines: vec![label.lines[i].clone()],
        widths: vec![width],
        width,
        height,
        size: label.size,
        line_height: label.line_height,
        heights: (height != label.line_height).then(|| vec![height]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_pieces() {
        assert_eq!(pieces("ab cd  ef "), vec!["ab ", "cd  ", "ef "]);
        assert_eq!(pieces("日本語。です"), vec!["日", "本", "語。", "で", "す"]);
        assert_eq!(pieces(" a"), vec![" a"]);
    }
}
