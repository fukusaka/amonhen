//! モバイル向けの描画部品が SVG をどう崩すかの近似（History の「すべてのファイル」）。本物の取り込みがないとき、
//! その部品が描けない・捨てる機能を SVG の文から取り除き、ふつうの SVG の描画（macOS は CoreSVG、Linux は
//! librsvg）で描ける形にする。規則は `svglint` と同じ資料から。
//!
//! 書き換えは元の文の上で、要素と属性の範囲を切り貼りして行う（変えないところはバイト単位でそのまま）。

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use amonhen_model::tr;

use crate::svglint::{FLUTTER_FILTERS, RN_FILTERS, Target};

/// 近似の絵の説明（1 行）。
pub fn description(target: Target) -> String {
    tr!(
        "Approximation: the SVG rewritten the way {renderer} degrades it, drawn by this app",
        renderer = target.name()
    )
}

/// `svg` を、`target` が崩すように書き換える。変えるところがなければ入力をそのまま返す。
pub fn approximate(svg: &[u8], target: Target) -> Vec<u8> {
    let text = String::from_utf8_lossy(svg);
    let mut sim = Sim::new(&text, target);
    sim.run();
    let mut out = sim.apply();
    if matches!(target, Target::Android | Target::Ios) {
        out = replace_p3(&out);
    }
    if out == text {
        svg.to_vec()
    } else {
        out.into_bytes()
    }
}

struct Attr {
    name: String,
    /// 前の空白から値の終わりの引用符まで。
    span: Range<usize>,
    /// 引用符の内側。
    value: Range<usize>,
}

struct El {
    name: String,
    /// `<` の位置。
    start: usize,
    /// 要素名の終わり（属性を足す位置）。
    name_end: usize,
    /// 開始タグの終わり（`>` の次）。
    tag_end: usize,
    /// 終了タグの始まり（空要素なら `tag_end`）。
    close_start: usize,
    /// 要素の終わり（終了タグの `>` の次）。
    end: usize,
    attrs: Vec<Attr>,
    parent: Option<usize>,
    children: Vec<usize>,
}

impl El {
    fn local(&self) -> &str {
        self.name.rsplit(':').next().unwrap_or(&self.name)
    }

    fn attr<'t>(&self, text: &'t str, key: &str) -> Option<&'t str> {
        self.attrs
            .iter()
            .find(|a| a.name == key)
            .map(|a| &text[a.value.clone()])
    }

    /// 属性か、`style` の中の同じ名前の宣言。
    fn prop(&self, text: &str, key: &str) -> Option<String> {
        if let Some(v) = self.attr(text, key) {
            return Some(v.to_owned());
        }
        self.attr(text, "style")?.split(';').find_map(|decl| {
            let (k, v) = decl.split_once(':')?;
            (k.trim() == key).then(|| v.trim().to_owned())
        })
    }

    fn href_id(&self, text: &str) -> Option<String> {
        let href = self
            .attr(text, "href")
            .or_else(|| self.attr(text, "xlink:href"))?;
        href.trim().strip_prefix('#').map(str::to_owned)
    }
}

/// 要素を範囲つきで順に読む（コメント・CDATA・処理命令・DOCTYPE は飛ばす）。壊れた XML でも読めたところまで。
fn parse(text: &str) -> Vec<El> {
    let bytes = text.as_bytes();
    let len = text.len();
    let mut out: Vec<El> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut i = 0;
    let skip_to =
        |from: usize, end: &str| text[from..].find(end).map_or(len, |p| from + p + end.len());
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
            let n0 = i + 2;
            let mut j = n0;
            while j < len && !bytes[j].is_ascii_whitespace() && bytes[j] != b'>' {
                j += 1;
            }
            let name = &text[n0..j];
            let end = skip_to(j, ">");
            if let Some(pos) = stack.iter().rposition(|&e| out[e].name == name) {
                // 閉じ忘れた内側の要素は、ここで終わったことにする。
                for &e in &stack[pos + 1..] {
                    out[e].close_start = i;
                    out[e].end = i;
                }
                let e = stack[pos];
                out[e].close_start = i;
                out[e].end = end;
                stack.truncate(pos);
            }
            i = end;
        } else {
            let start = i;
            i += 1;
            let n0 = i;
            while i < len && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' && bytes[i] != b'/'
            {
                i += 1;
            }
            let name = text[n0..i].to_owned();
            let name_end = i;
            let mut attrs = Vec::new();
            let mut closed = false;
            loop {
                let ws0 = i;
                while i < len && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i >= len {
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
                while i < len
                    && !bytes[i].is_ascii_whitespace()
                    && !matches!(bytes[i], b'=' | b'>' | b'/')
                {
                    i += 1;
                }
                let key = text[k0..i].to_owned();
                while i < len && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                let mut value = i..i;
                if i < len && bytes[i] == b'=' {
                    i += 1;
                    while i < len && bytes[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    if i < len && (bytes[i] == b'"' || bytes[i] == b'\'') {
                        let quote = bytes[i] as char;
                        let v0 = i + 1;
                        let v1 = text[v0..].find(quote).map_or(len, |p| v0 + p);
                        value = v0..v1;
                        i = (v1 + 1).min(len);
                    }
                }
                if key.is_empty() {
                    i += 1;
                } else {
                    attrs.push(Attr {
                        name: key,
                        span: ws0..i,
                        value,
                    });
                }
            }
            if name.is_empty() {
                continue;
            }
            let parent = stack.last().copied();
            let index = out.len();
            if let Some(p) = parent {
                out[p].children.push(index);
            }
            out.push(El {
                name,
                start,
                name_end,
                tag_end: i,
                close_start: i,
                end: i,
                attrs,
                parent,
                children: Vec::new(),
            });
            if !closed {
                stack.push(index);
                // スタイルとスクリプトの中身は要素として読まない。
                let local = out[index].local();
                if matches!(local, "style" | "script") {
                    let close = format!("</{}", out[index].name);
                    i = text[i..].find(&close).map_or(len, |p| i + p);
                }
            }
        }
    }
    for &e in &stack {
        out[e].close_start = len;
        out[e].end = len;
    }
    out
}

/// アニメーションの要素（どの部品も描かない）。
const ANIMATION: [&str; 6] = [
    "animate",
    "animateMotion",
    "animateTransform",
    "animateColor",
    "set",
    "mpath",
];
/// 図形（クリップパス・マスクの中で意味があるもの）。
const SHAPES: [&str; 7] = [
    "path", "rect", "circle", "ellipse", "line", "polyline", "polygon",
];
/// それ自体は描かれない要素（グループの不透明度を下ろさない）。
const NON_RENDERING: [&str; 14] = [
    "defs",
    "clipPath",
    "mask",
    "pattern",
    "marker",
    "symbol",
    "linearGradient",
    "radialGradient",
    "filter",
    "style",
    "script",
    "title",
    "desc",
    "metadata",
];
const MARKERS: [&str; 4] = ["marker", "marker-start", "marker-mid", "marker-end"];

fn is_primitive(name: &str) -> bool {
    name.len() > 2 && name.starts_with("fe") && name.as_bytes()[2].is_ascii_uppercase()
}

/// `url(#id)` の id。
fn url_id(value: &str) -> Option<String> {
    let rest = value.trim().strip_prefix("url(")?;
    let inner = &rest[..rest.find(')')?];
    let inner = inner.trim().trim_matches(|c| c == '"' || c == '\'');
    inner.strip_prefix('#').map(str::to_owned)
}

fn parse_opacity(value: &str) -> Option<f64> {
    let v = value.trim();
    match v.strip_suffix('%') {
        Some(p) => p.trim().parse::<f64>().ok().map(|p| p / 100.0),
        None => v.parse().ok(),
    }
}

fn format_number(v: f64) -> String {
    format!("{}", (v * 1e6).round() / 1e6)
}

fn is_white(fill: &str) -> bool {
    let v: String = fill
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    matches!(
        v.as_str(),
        "white" | "#fff" | "#ffff" | "#ffffff" | "#ffffffff" | "rgb(255,255,255)"
    )
}

/// `color(display-p3 …)` を黒にする（Android は読めず、iOS は黒で描く）。
fn replace_p3(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while let Some(p) = s[i..].find("color(") {
        let at = i + p;
        let word = at > 0 && {
            let c = s.as_bytes()[at - 1];
            c.is_ascii_alphanumeric() || c == b'-' || c == b'_'
        };
        let p3 = s[at + 6..].trim_start().starts_with("display-p3");
        if !word
            && p3
            && let Some(close) = s[at..].find(')')
        {
            out.push_str(&s[i..at]);
            out.push_str("black");
            i = at + close + 1;
            continue;
        }
        out.push_str(&s[i..at + 6]);
        i = at + 6;
    }
    out.push_str(&s[i..]);
    out
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Keep,
    /// 要素を中身ごと消す。
    Remove,
    /// タグだけ消して中身を残す。
    Unwrap,
    /// 要素は残して中身を消す。
    Empty,
}

enum PropEdit {
    Keep,
    Remove,
    Set(String),
    Rename(&'static str, String),
}

struct Sim<'a> {
    text: &'a str,
    els: Vec<El>,
    target: Target,
    root: Option<usize>,
    patterns: HashSet<String>,
    symbols: HashSet<String>,
    dropped_filters: HashSet<usize>,
    dropped_filter_ids: HashSet<String>,
    /// マスクの id → 代わりのクリップパスの id（白一色の図形がなければ None）。
    masks: HashMap<String, Option<String>>,
    /// 要素 → 不透明度の書き換え（None は消す）。
    opacity: HashMap<usize, Option<String>>,
    edits: Vec<(usize, usize, String)>,
}

impl<'a> Sim<'a> {
    fn new(text: &'a str, target: Target) -> Self {
        let els = parse(text);
        let root = els.iter().position(|e| e.local() == "svg");
        Self {
            text,
            els,
            target,
            root,
            patterns: HashSet::new(),
            symbols: HashSet::new(),
            dropped_filters: HashSet::new(),
            dropped_filter_ids: HashSet::new(),
            masks: HashMap::new(),
            opacity: HashMap::new(),
            edits: Vec::new(),
        }
    }

    /// `i` の子孫（文書の順で `i` の直後に続く）。
    fn descendants(&self, i: usize) -> impl Iterator<Item = usize> + '_ {
        let end = self.els[i].end;
        (i + 1..self.els.len()).take_while(move |&j| self.els[j].start < end)
    }

    fn prepare(&mut self) {
        let text = self.text;
        for i in 0..self.els.len() {
            let e = &self.els[i];
            let id = e.attr(text, "id").map(str::to_owned);
            match (self.target, e.local()) {
                (Target::Android, "pattern") => self.patterns.extend(id),
                (Target::Android, "symbol") => self.symbols.extend(id),
                (Target::Android, "mask") => {
                    let Some(id) = id else { continue };
                    let white: String = e
                        .children
                        .iter()
                        .map(|&c| &self.els[c])
                        .filter(|c| {
                            let opaque = ["opacity", "fill-opacity"].iter().all(|k| {
                                c.prop(text, k)
                                    .is_none_or(|v| parse_opacity(&v).is_some_and(|v| v >= 1.0))
                            });
                            SHAPES.contains(&c.local())
                                && c.prop(text, "fill").is_some_and(|f| is_white(&f))
                                && opaque
                        })
                        .map(|c| &text[c.start..c.end])
                        .collect();
                    if white.is_empty() {
                        self.masks.insert(id, None);
                    } else {
                        let clip = format!("{id}-clip");
                        let insert = format!("<clipPath id=\"{clip}\">{white}</clipPath>");
                        self.edits.push((e.end, e.end, insert));
                        self.masks.insert(id, Some(clip));
                    }
                }
                (Target::Flutter, "filter") => {
                    let bad = self.descendants(i).any(|d| {
                        let n = self.els[d].local();
                        is_primitive(n) && !FLUTTER_FILTERS.contains(&n)
                    });
                    if bad {
                        self.dropped_filters.insert(i);
                        self.dropped_filter_ids.extend(id);
                    }
                }
                (Target::ReactNative, "filter") => {
                    let primitives: Vec<&str> = e
                        .children
                        .iter()
                        .map(|&c| self.els[c].local())
                        .filter(|n| is_primitive(n))
                        .collect();
                    if !primitives.is_empty() && !primitives.iter().any(|n| RN_FILTERS.contains(n))
                    {
                        self.dropped_filters.insert(i);
                        self.dropped_filter_ids.extend(id);
                    }
                }
                _ => {}
            }
        }
        if self.target == Target::Android {
            self.push_down_group_opacity();
        }
    }

    /// Android はグループの不透明度を図形ごとにかける。子が 2 つ以上のグループは、不透明度を子へ下ろす。
    fn push_down_group_opacity(&mut self) {
        let text = self.text;
        let mut pushed = vec![1.0_f64; self.els.len()];
        for i in 0..self.els.len() {
            let e = &self.els[i];
            if NON_RENDERING.contains(&e.local()) {
                continue;
            }
            let raw = e.prop(text, "opacity");
            let own = match raw.as_deref().map(parse_opacity) {
                Some(None) => continue,
                Some(Some(v)) => v,
                None => 1.0,
            };
            let effective = own * pushed[i];
            let children: Vec<usize> = e
                .children
                .iter()
                .copied()
                .filter(|&c| !NON_RENDERING.contains(&self.els[c].local()))
                .collect();
            if e.local() == "g" && children.len() > 1 && (effective - 1.0).abs() > 1e-9 {
                for c in children {
                    pushed[c] = effective;
                }
                if raw.is_some() {
                    self.opacity.insert(i, None);
                }
            } else if (pushed[i] - 1.0).abs() > 1e-9 {
                self.opacity.insert(i, Some(format_number(effective)));
            }
        }
    }

    fn action(&self, i: usize) -> Action {
        let text = self.text;
        let e = &self.els[i];
        let name = e.local();
        let nested_svg = name == "svg" && Some(i) != self.root;
        let not_drawn = matches!(name, "foreignObject" | "script" | "switch")
            || ANIMATION.contains(&name)
            || nested_svg;
        match self.target {
            Target::Ios => Action::Keep,
            // Svg2Vector は switch の子を読む。
            Target::Android if name == "switch" => Action::Unwrap,
            Target::Android => {
                let symbol_use =
                    name == "use" && e.href_id(text).is_some_and(|id| self.symbols.contains(&id));
                if not_drawn
                    || symbol_use
                    || matches!(
                        name,
                        "filter" | "pattern" | "marker" | "image" | "symbol" | "textPath"
                    )
                {
                    Action::Remove
                } else {
                    Action::Keep
                }
            }
            Target::Flutter => {
                let image = name == "image" && {
                    let href = e
                        .attr(text, "href")
                        .or_else(|| e.attr(text, "xlink:href"))
                        .unwrap_or_default()
                        .trim_start();
                    !["png", "jpeg", "jpg", "webp", "gif", "bmp"]
                        .iter()
                        .any(|t| href.starts_with(&format!("data:image/{t}")))
                };
                let in_clip = e.parent.is_some_and(|p| self.els[p].local() == "clipPath")
                    && !SHAPES.contains(&name)
                    && !matches!(name, "use" | "text" | "title" | "desc");
                if not_drawn
                    || image
                    || in_clip
                    || self.dropped_filters.contains(&i)
                    || matches!(name, "style" | "marker" | "textPath")
                {
                    Action::Remove
                } else {
                    Action::Keep
                }
            }
            Target::ReactNative => {
                let primitive = is_primitive(name)
                    && e.parent.is_some_and(|p| self.els[p].local() == "filter")
                    && !RN_FILTERS.contains(&name);
                if name == "foreignObject" {
                    Action::Empty
                } else if primitive
                    || self.dropped_filters.contains(&i)
                    || (not_drawn && !nested_svg)
                    || matches!(name, "style" | "a")
                {
                    Action::Remove
                } else {
                    Action::Keep
                }
            }
        }
    }

    fn prop(&self, el: Option<usize>, name: &str, value: &str) -> PropEdit {
        let e = el.map(|i| &self.els[i]);
        let focal = matches!(name, "fx" | "fy") && e.is_some_and(|e| e.local() == "radialGradient");
        let dropped_filter = name == "filter"
            && url_id(value).is_some_and(|id| self.dropped_filter_ids.contains(&id));
        match self.target {
            Target::Ios => PropEdit::Keep,
            Target::Android => match name {
                "filter" | "stroke-dasharray" | "stroke-dashoffset" | "mix-blend-mode" => {
                    PropEdit::Remove
                }
                _ if focal || MARKERS.contains(&name) => PropEdit::Remove,
                "fill" | "stroke"
                    if url_id(value).is_some_and(|id| self.patterns.contains(&id)) =>
                {
                    PropEdit::Set("none".into())
                }
                "mask" => {
                    let clip = url_id(value).and_then(|id| self.masks.get(&id).cloned().flatten());
                    let clipped = e.is_some_and(|e| e.prop(self.text, "clip-path").is_some());
                    match clip {
                        Some(clip) if !clipped => {
                            PropEdit::Rename("clip-path", format!("url(#{clip})"))
                        }
                        _ => PropEdit::Remove,
                    }
                }
                "opacity" => match el.and_then(|i| self.opacity.get(&i)) {
                    Some(None) => PropEdit::Remove,
                    Some(Some(v)) => PropEdit::Set(v.clone()),
                    None => PropEdit::Keep,
                },
                _ => PropEdit::Keep,
            },
            Target::Flutter if MARKERS.contains(&name) || dropped_filter => PropEdit::Remove,
            Target::ReactNative if focal || dropped_filter => PropEdit::Remove,
            Target::Flutter | Target::ReactNative => PropEdit::Keep,
        }
    }

    /// `k: v; …` の並びを書き換える。変わらなければ None。
    fn rewrite_decls(&self, decls: &str, el: Option<usize>) -> Option<String> {
        let mut out = Vec::new();
        let mut changed = false;
        for piece in decls.split(';') {
            let Some((k, v)) = piece.split_once(':') else {
                out.push(piece.to_owned());
                continue;
            };
            match self.prop(el, k.trim(), v.trim()) {
                PropEdit::Keep => out.push(piece.to_owned()),
                PropEdit::Set(new) if new == v.trim() => out.push(piece.to_owned()),
                PropEdit::Remove => changed = true,
                PropEdit::Set(new) => {
                    out.push(format!("{k}:{new}"));
                    changed = true;
                }
                PropEdit::Rename(name, new) => {
                    out.push(format!("{name}:{new}"));
                    changed = true;
                }
            }
        }
        changed.then(|| out.join(";").trim().to_owned())
    }

    fn rewrite_attrs(&mut self, i: usize) {
        let text = self.text;
        let mut edits = Vec::new();
        let mut has_opacity = false;
        for a in &self.els[i].attrs {
            let value = &text[a.value.clone()];
            has_opacity |= a.name == "opacity";
            if a.name == "style" {
                has_opacity |= value.split(';').any(|d| {
                    d.split_once(':')
                        .is_some_and(|(k, _)| k.trim() == "opacity")
                });
                match self.rewrite_decls(value, Some(i)) {
                    Some(new) if new.trim().trim_matches(';').trim().is_empty() => {
                        edits.push((a.span.start, a.span.end, String::new()));
                    }
                    Some(new) => edits.push((a.value.start, a.value.end, new)),
                    None => {}
                }
                continue;
            }
            match self.prop(Some(i), &a.name, value) {
                PropEdit::Keep => {}
                PropEdit::Set(new) if new == value => {}
                PropEdit::Remove => edits.push((a.span.start, a.span.end, String::new())),
                PropEdit::Set(new) => edits.push((a.value.start, a.value.end, new)),
                PropEdit::Rename(name, new) => {
                    edits.push((a.span.start, a.span.end, format!(" {name}=\"{new}\"")));
                }
            }
        }
        if !has_opacity && let Some(Some(v)) = self.opacity.get(&i) {
            let at = self.els[i].name_end;
            edits.push((at, at, format!(" opacity=\"{v}\"")));
        }
        self.edits.extend(edits);
    }

    /// Android は `.class` だけのセレクタの規則を読む。ほかの規則と @ の規則は捨てる。
    fn rewrite_android_css(&self, css: &str) -> Option<String> {
        let (pre, inner, post) = match css.find("<![CDATA[") {
            Some(s) => {
                let a = s + 9;
                let b = css[a..].find("]]>").map_or(css.len(), |p| a + p);
                (&css[..a], &css[a..b], &css[b..])
            }
            None => ("", css, ""),
        };
        let mut plain = String::with_capacity(inner.len());
        let mut rest = inner;
        while let Some(p) = rest.find("/*") {
            plain.push_str(&rest[..p]);
            rest = rest[p + 2..]
                .find("*/")
                .map_or("", |q| &rest[p + 2 + q + 2..]);
        }
        plain.push_str(rest);
        let mut changed = plain != inner;
        let s = plain.as_str();
        let mut kept = Vec::new();
        let mut i = 0;
        loop {
            i += s[i..].len() - s[i..].trim_start().len();
            if i >= s.len() {
                break;
            }
            changed |= s[i..].starts_with('@');
            if s[i..].starts_with('@') {
                let semi = s[i..].find(';').map(|p| i + p);
                let brace = s[i..].find('{').map(|p| i + p);
                i = match (semi, brace) {
                    (Some(a), b) if b.is_none_or(|b| a < b) => a + 1,
                    (_, Some(b)) => {
                        let mut depth = 0;
                        let mut j = b;
                        for (k, c) in s[b..].char_indices() {
                            match c {
                                '{' => depth += 1,
                                '}' => depth -= 1,
                                _ => {}
                            }
                            j = b + k + c.len_utf8();
                            if depth == 0 {
                                break;
                            }
                        }
                        j
                    }
                    _ => s.len(),
                };
                continue;
            }
            let (Some(open), Some(close)) = (
                s[i..].find('{').map(|p| i + p),
                s[i..].find('}').map(|p| i + p),
            ) else {
                changed = true;
                break;
            };
            if close < open {
                changed = true;
                i = close + 1;
                continue;
            }
            let selectors = &s[i..open];
            let plain_class = selectors.split(',').all(|sel| {
                sel.trim().strip_prefix('.').is_some_and(|c| {
                    !c.is_empty()
                        && c.chars()
                            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
                })
            });
            if plain_class {
                let body = &s[open + 1..close];
                match self.rewrite_decls(body, None) {
                    Some(new) => {
                        kept.push(format!("{}{{{new}}}", selectors.trim()));
                        changed = true;
                    }
                    None => kept.push(s[i..=close].to_owned()),
                }
            } else {
                changed = true;
            }
            i = close + 1;
        }
        changed.then(|| format!("{pre}{}{post}", kept.join("\n")))
    }

    fn run(&mut self) {
        self.prepare();
        let mut dead = vec![false; self.els.len()];
        for i in 0..self.els.len() {
            if let Some(p) = self.els[i].parent
                && dead[p]
            {
                dead[i] = true;
                continue;
            }
            let (start, tag_end, close_start, end) = {
                let e = &self.els[i];
                (e.start, e.tag_end, e.close_start, e.end)
            };
            match self.action(i) {
                Action::Remove => {
                    dead[i] = true;
                    self.edits.push((start, end, String::new()));
                }
                Action::Unwrap => {
                    self.edits.push((start, tag_end, String::new()));
                    self.edits.push((close_start, end, String::new()));
                }
                Action::Empty => {
                    dead[i] = true;
                    self.edits.push((tag_end, close_start, String::new()));
                    self.rewrite_attrs(i);
                }
                Action::Keep => {
                    self.rewrite_attrs(i);
                    if self.target == Target::Android
                        && self.els[i].local() == "style"
                        && let Some(css) =
                            self.rewrite_android_css(&self.text[tag_end..close_start])
                    {
                        self.edits.push((tag_end, close_start, css));
                    }
                }
            }
        }
    }

    fn apply(mut self) -> String {
        self.edits.sort_by_key(|(s, e, _)| (*s, *e));
        let mut out = String::with_capacity(self.text.len());
        let mut pos = 0;
        for (s, e, t) in &self.edits {
            if *s < pos {
                continue;
            }
            out.push_str(&self.text[pos..*s]);
            out.push_str(t);
            pos = *e;
        }
        out.push_str(&self.text[pos..]);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sim(svg: &str, target: Target) -> String {
        String::from_utf8(approximate(svg.as_bytes(), target)).unwrap()
    }

    const ICON: &str = r##"<?xml version="1.0"?>
<!-- icon -->
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
  <path d="M0 0h24v24H0z" fill="none"/>
  <g opacity="1"><circle cx="12" cy="12" r="5" fill="#2a7"/></g>
</svg>
"##;

    #[test]
    fn a_plain_icon_is_unchanged_everywhere() {
        for t in [
            Target::Android,
            Target::Ios,
            Target::Flutter,
            Target::ReactNative,
        ] {
            assert_eq!(approximate(ICON.as_bytes(), t), ICON.as_bytes(), "{t:?}");
        }
    }

    #[test]
    fn android_drops_what_svg2vector_doesnt_import() {
        let svg = r##"<svg viewBox="0 0 10 10" xmlns:xlink="http://www.w3.org/1999/xlink">
<style>.a{fill:red;stroke-dasharray:2} rect{fill:blue} @media print{.a{fill:none}}</style>
<defs><filter id="f"><feGaussianBlur/></filter><pattern id="p"><rect/></pattern>
<marker id="k"/><symbol id="s"><rect/></symbol>
<mask id="m"><rect width="10" height="10" fill="#fff"/><circle r="2" fill="black"/></mask>
<mask id="n"><rect fill="gray"/></mask>
<radialGradient id="r" cx="5" fx="2" fy="3"/></defs>
<rect class="a" filter="url(#f)" fill="url(#p)" stroke="url( '#p' )" marker-end="url(#k)"
  style="stroke-dasharray: 4 2; fill-opacity: .5; mix-blend-mode: multiply" mask="url(#m)"/>
<circle mask="url(#n)" stroke-dashoffset="1"/>
<use xlink:href="#s"/><use href="#other"/>
<image href="a.png"/><foreignObject><div/></foreignObject><script>if (a<b) {}</script>
<switch><path id="kept"/></switch>
<text><textPath href="#x">t</textPath></text><animate/><svg/>
<g opacity="0.5"><path id="one"/><path id="two" opacity="0.5"/></g>
<g opacity=".5"><path id="only"/></g>
<path fill="color(display-p3 1 0 0)"/>
</svg>"##;
        let out = sim(svg, Target::Android);
        for gone in [
            "<filter",
            "filter=",
            "<pattern",
            "url(#p)",
            "<marker",
            "marker-end",
            "<symbol",
            "#s",
            "<image",
            "foreignObject",
            "<script",
            "<switch",
            "</switch>",
            "textPath",
            "<animate",
            "<svg/>",
            "dasharray",
            "dashoffset",
            "mix-blend-mode",
            "fx=",
            "fy=",
            "rect{fill:blue}",
            "@media",
            "mask=",
            "display-p3",
            r#"<g opacity="0.5">"#,
        ] {
            assert!(!out.contains(gone), "{gone} in {out}");
        }
        for kept in [
            r#"fill="none" stroke="none""#,
            r#"style="fill-opacity: .5""#,
            r#"clip-path="url(#m-clip)""#,
            r##"<clipPath id="m-clip"><rect width="10" height="10" fill="#fff"/></clipPath>"##,
            "<circle/>",
            r##"<use href="#other"/>"##,
            r#"<path id="kept"/>"#,
            r#"<radialGradient id="r" cx="5"/>"#,
            ".a{fill:red}",
            r#"<g><path opacity="0.5" id="one"/><path id="two" opacity="0.25"/></g>"#,
            r#"<g opacity=".5"><path id="only"/></g>"#,
            r#"fill="black""#,
        ] {
            assert!(out.contains(kept), "{kept} not in {out}");
        }
    }

    #[test]
    fn ios_only_blackens_display_p3() {
        let svg = r#"<svg viewBox="0 0 1 1"><filter id="f"/><rect style="fill: color(display-p3 0 1 0)" filter="url(#f)"/></svg>"#;
        assert_eq!(
            sim(svg, Target::Ios),
            r#"<svg viewBox="0 0 1 1"><filter id="f"/><rect style="fill: black" filter="url(#f)"/></svg>"#
        );
    }

    #[test]
    fn flutter_drops_what_vector_graphics_compiler_skips() {
        let svg = r##"<svg viewBox="0 0 10 10">
<style>rect{fill:red}</style>
<filter id="ok"><feGaussianBlur/><feOffset/></filter>
<filter id="bad"><feGaussianBlur/><feTurbulence/></filter>
<clipPath id="c"><rect/><g><rect/></g><text>t</text></clipPath>
<rect filter="url(#bad)" style="filter:url(#bad);fill:red" marker-start="url(#k)"/>
<circle filter="url(#ok)"/>
<image href="data:image/png;base64,AAAA"/><image href="pic.svg"/>
<marker id="k"/><text><textPath>t</textPath></text><switch><rect/></switch><svg/>
</svg>"##;
        let out = sim(svg, Target::Flutter);
        for gone in [
            "<style",
            r#"id="bad""#,
            "url(#bad)",
            "marker",
            "pic.svg",
            "<g>",
            "textPath",
            "switch",
            "<svg/>",
        ] {
            assert!(!out.contains(gone), "{gone} in {out}");
        }
        for kept in [
            r#"<filter id="ok">"#,
            r#"filter="url(#ok)""#,
            "data:image/png",
            r#"<clipPath id="c"><rect/><text>t</text></clipPath>"#,
            r#"style="fill:red""#,
        ] {
            assert!(out.contains(kept), "{kept} not in {out}");
        }
    }

    #[test]
    fn react_native_drops_what_it_doesnt_draw_natively() {
        let svg = r##"<svg viewBox="0 0 10 10">
<style>rect{fill:red}</style>
<filter id="mixed"><feGaussianBlur/><feTurbulence/></filter>
<filter id="none"><feTurbulence/><feComponentTransfer><feFuncA/></feComponentTransfer></filter>
<radialGradient id="r" fx="1"/>
<rect filter="url(#mixed)"/><circle filter="url(#none)"/>
<a href="https://example.com"><rect id="linked"/></a>
<foreignObject width="5"><div>hi</div></foreignObject>
<switch><rect/></switch><script>x</script><set/>
</svg>"##;
        let out = sim(svg, Target::ReactNative);
        for gone in [
            "<style",
            "feTurbulence",
            r#"id="none""#,
            "url(#none)",
            "fx=",
            "<a ",
            "linked",
            "<div>",
            "switch",
            "script",
            "<set",
        ] {
            assert!(!out.contains(gone), "{gone} in {out}");
        }
        for kept in [
            r#"<filter id="mixed"><feGaussianBlur/></filter>"#,
            r#"filter="url(#mixed)""#,
            r#"<foreignObject width="5"></foreignObject>"#,
            r#"<radialGradient id="r"/>"#,
        ] {
            assert!(out.contains(kept), "{kept} not in {out}");
        }
    }

    #[test]
    fn a_mask_without_white_shapes_is_dropped_on_android() {
        let svg = r#"<svg viewBox="0 0 1 1"><mask id="m"><rect fill="white" opacity="0.5"/></mask><rect style="mask:url(#m)"/></svg>"#;
        let out = sim(svg, Target::Android);
        assert!(
            !out.contains("clipPath") && !out.contains("style="),
            "{out}"
        );
    }
}
