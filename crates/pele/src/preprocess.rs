//! 図の文字列の前処理（pele の `src/preprocess.ts`）: 改行をそろえ、front matter と `%%{init}%%` の設定を読み、
//! コメントの行を消し、`#name;` の符号を隠す。正規表現は手で書いた走査に置き換えた（どれも JS の正規表現と同じ所に
//! 合う）。`steps` は位置の写しの記録で、知らせを元の文字列の位置で出すときだけ取る（UTF-16 の位置）。

use crate::errors::{Edit, ErrorCode, PeleError, Result};
use crate::js;
use crate::util::chars::is_space_char;
use crate::util::yaml::{Map, Value, parse_json, parse_yaml};

/// 図の設定（Mermaid の config）。
pub type Config = Map;

pub struct Preprocessed {
    pub text: String,
    pub title: Option<String>,
    pub config: Config,
    /// コメントの行を消す前の文字列と、その前に取り除いた front matter の行数。
    pub with_comments: String,
    pub line_offset: usize,
}

/// バイトの位置を UTF-16 の位置に。前に求めた位置から数え進めるので、前から順に聞けば全体で文字列の長さで済む
/// （位置ごとに頭から数えると、書き換えの多い文字列で 2 乗になる）。
struct Pos16<'a> {
    s: &'a str,
    byte: usize,
    unit: usize,
}

impl<'a> Pos16<'a> {
    fn new(s: &'a str) -> Self {
        Pos16 {
            s,
            byte: 0,
            unit: 0,
        }
    }

    fn at(&mut self, byte: usize) -> usize {
        if byte < self.byte {
            self.byte = 0;
            self.unit = 0;
        }
        self.unit += js::len16(&self.s[self.byte..byte]);
        self.byte = byte;
        self.unit
    }
}

/// 範囲（バイト）を UTF-16 の位置の `Edit` に。
fn edits16(s: &str, ranges: &[(usize, usize)], length: impl Fn(usize) -> usize) -> Vec<Edit> {
    let mut pos = Pos16::new(s);
    ranges
        .iter()
        .enumerate()
        .map(|(i, &(a, b))| (pos.at(a), pos.at(b), length(i)))
        .collect()
}

fn remove_ranges(text: &str, ranges: &[(usize, usize)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for &(a, b) in ranges {
        out.push_str(&text[last..a]);
        last = b;
    }
    out.push_str(&text[last..]);
    out
}

/// 行の頭（`^` を m で）か。
fn line_start(b: &[u8], i: usize) -> bool {
    if i == 0 {
        return true;
    }
    let c = b[i - 1];
    if c == b'\n' || c == b'\r' {
        return true;
    }
    // U+2028・U+2029 は E2 80 A8/A9。`i` は文字の境とは限らないので、バイトで見る。
    i >= 3 && b[i - 3] == 0xe2 && b[i - 2] == 0x80 && matches!(b[i - 1], 0xa8 | 0xa9)
}

/// `/^\s*%%(?!{)[^\n]+\n?/gm` の当たる範囲。
fn comment_ranges(text: &str) -> Vec<(usize, usize)> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if !line_start(b, i) {
            i += 1;
            continue;
        }
        // \s* は改行もまたぐ。
        let mut j = i;
        for (k, c) in text[i..].char_indices() {
            if !is_space_char(c) {
                j = i + k;
                break;
            }
            j = i + k + c.len_utf8();
        }
        if text[j..].starts_with("%%") && j + 2 < b.len() && b[j + 2] != b'{' && b[j + 2] != b'\n' {
            let mut e = j + 3;
            while e < b.len() && b[e] != b'\n' {
                e += 1;
            }
            if e < b.len() {
                e += 1;
            }
            out.push((i, e));
            i = e;
            continue;
        }
        i += 1;
    }
    out
}

pub fn cleanup_comments(text: &str, steps: Option<&mut Vec<Vec<Edit>>>) -> String {
    let mut text = text.to_string();
    let mut steps = steps;
    if text.contains("%%") {
        let ranges = comment_ranges(&text);
        if let Some(s) = steps.as_deref_mut() {
            s.push(edits16(&text, &ranges, |_| 0));
        }
        text = remove_ranges(&text, &ranges);
    }
    let trimmed = js::trim_start(&text).to_string();
    if let Some(s) = steps {
        s.push(vec![(0, js::len16(&text) - js::len16(&trimmed), 0)]);
    }
    trimmed
}

pub fn normalize_line_breaks(text: &str) -> String {
    if text.contains('\r') {
        text.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        text.to_string()
    }
}

fn is_line_break(c: Option<char>) -> bool {
    c == Some('\n') || c == Some('\r')
}

fn is_space(c: Option<char>) -> bool {
    c.is_some_and(is_space_char)
}

struct FrontMatter {
    indent: String,
    body: String,
    /// 取り除く長さ（`text` の char の数）。
    length: usize,
}

/// 頭の `---` で囲んだ YAML を、Mermaid の front matter の正規表現と同じに探す。
fn match_front_matter(text: &[char]) -> Option<FrontMatter> {
    let at = |i: usize| text.get(i).copied();
    let starts_with = |s: &str, from: usize| {
        let p: Vec<char> = s.chars().collect();
        text.len() >= from + p.len() && text[from..from + p.len()] == p[..]
    };
    let mut cursor = 0;
    while cursor < text.len() && is_space(at(cursor)) && !is_line_break(at(cursor)) {
        cursor += 1;
    }
    let indent: String = text[..cursor].iter().collect();
    if !starts_with("---", cursor) {
        return None;
    }
    let mut opening_ends = Vec::new();
    let mut i = cursor + 3;
    while i < text.len() && is_space(at(i)) {
        if is_line_break(at(i)) {
            opening_ends.push(i);
        }
        i += 1;
    }
    if opening_ends.is_empty() {
        return None;
    }
    let fence = format!("{indent}---");
    let fence_len = fence.chars().count();
    let closing_end = |i: usize| -> Option<usize> {
        if !is_line_break(at(i)) || !starts_with(&fence, i + 1) {
            return None;
        }
        let mut last = None;
        let mut j = i + 1 + fence_len;
        while j < text.len() && is_space(at(j)) {
            if is_line_break(at(j)) {
                last = Some(j);
            }
            j += 1;
        }
        last
    };
    let needle: Vec<char> = format!("\n{fence}").chars().collect();
    let found_at =
        |i: usize| text.len() >= i + needle.len() && text[i..i + needle.len()] == needle[..];
    let last_index_of = |from: usize| -> Option<usize> {
        let mut k = from.min(text.len().saturating_sub(needle.len()));
        if text.len() < needle.len() {
            return None;
        }
        loop {
            if found_at(k) {
                return Some(k);
            }
            if k == 0 {
                return None;
            }
            k -= 1;
        }
    };
    let index_of = |from: usize| -> Option<usize> { (from..text.len()).find(|&k| found_at(k)) };
    let mut last_fence = None;
    let mut i = last_index_of(text.len());
    while let Some(k) = i {
        if k <= opening_ends[0] {
            break;
        }
        if closing_end(k).is_some() {
            last_fence = Some(k);
            break;
        }
        if k == 0 {
            break;
        }
        i = last_index_of(k - 1);
    }
    let last_fence = last_fence?;
    let mut opening_end = opening_ends[0];
    for &c in &opening_ends {
        if c < last_fence {
            opening_end = c;
        }
    }
    let body_start = opening_end + 1;
    let first_at = if body_start > 0 { body_start - 1 } else { 0 };
    let mut i = index_of(first_at);
    while let Some(k) = i {
        if k > last_fence {
            break;
        }
        if k >= body_start
            && let Some(end) = closing_end(k)
        {
            return Some(FrontMatter {
                indent,
                body: text[body_start..k].iter().collect(),
                length: end + 1,
            });
        }
        i = index_of(k + 1);
    }
    None
}

pub fn is_record(value: &Value) -> bool {
    matches!(value, Value::Obj(_))
}

const UNSAFE_KEYS: [&str; 3] = ["__proto__", "constructor", "prototype"];

pub fn merge(target: &mut Config, source: &Config) {
    for (key, value) in source.iter() {
        if UNSAFE_KEYS.contains(&key.as_str()) {
            continue;
        }
        match (value, target.get_mut(key)) {
            (Value::Obj(v), Some(Value::Obj(current))) => merge(current, v),
            (Value::Obj(v), _) => {
                let mut fresh = Config::new();
                merge(&mut fresh, v);
                target.set(key, Value::Obj(fresh));
            }
            _ => target.set(key, value.clone()),
        }
    }
}

/// `/%{2}{\s*(?:(\w+)\s*:|(\w+))\s*(?:(\w+)|((?:(?!}%{2}).|\r?\n)*))?\s*(?:}%{2})?/gi` の 1 つの当たり。
/// 範囲（char の位置）と、種類（1 か 2 の組）と、4 の組を返す。
fn directive_at(s: &[char], at: usize) -> Option<(usize, String, Option<String>)> {
    if !(s.get(at) == Some(&'%') && s.get(at + 1) == Some(&'%') && s.get(at + 2) == Some(&'{')) {
        return None;
    }
    let word = |c: Option<&char>| c.is_some_and(|&c| crate::util::chars::is_word_char(c));
    let space = |c: Option<&char>| c.is_some_and(|&c| is_space_char(c));
    let mut p = at + 3;
    while space(s.get(p)) {
        p += 1;
    }
    let ws = p;
    while word(s.get(p)) {
        p += 1;
    }
    if p == ws {
        return None;
    }
    let kind: String = s[ws..p].iter().collect();
    let mut q = p;
    while space(s.get(q)) {
        q += 1;
    }
    if s.get(q) == Some(&':') {
        p = q + 1;
    }
    while space(s.get(p)) {
        p += 1;
    }
    let mut body = None;
    if word(s.get(p)) {
        while word(s.get(p)) {
            p += 1;
        }
    } else {
        let start = p;
        loop {
            let c = s.get(p).copied();
            match c {
                Some('}') if s.get(p + 1) == Some(&'%') && s.get(p + 2) == Some(&'%') => break,
                Some('\r') if s.get(p + 1) == Some(&'\n') => p += 2,
                Some('\n') => p += 1,
                Some(c) if !matches!(c, '\r' | '\u{2028}' | '\u{2029}') => p += 1,
                _ => break,
            }
        }
        body = Some(s[start..p].iter().collect());
    }
    while space(s.get(p)) {
        p += 1;
    }
    if s.get(p) == Some(&'}') && s.get(p + 1) == Some(&'%') && s.get(p + 2) == Some(&'%') {
        p += 3;
    }
    Some((p, kind, body))
}

fn directive_ranges(s: &[char]) -> Vec<(usize, usize, String, Option<String>)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        if let Some((end, kind, body)) = directive_at(s, i) {
            out.push((i, end, kind, body));
            i = end;
        } else {
            i += 1;
        }
    }
    out
}

fn char_ranges_to_bytes(text: &str, ranges: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let idx: Vec<usize> = text
        .char_indices()
        .map(|(b, _)| b)
        .chain(std::iter::once(text.len()))
        .collect();
    ranges.iter().map(|&(a, b)| (idx[a], idx[b])).collect()
}

fn directives(text: &str, config: &mut Config, steps: Option<&mut Vec<Vec<Edit>>>) -> String {
    if !text.contains("%%{") {
        return text.to_string();
    }
    let normalized: Vec<char> = js::trim(text).replace('\'', "\"").chars().collect();
    for (_, _, kind, body) in directive_ranges(&normalized) {
        // 種類は名前の頭の語（init など）。
        if kind == "wrap" {
            config.set("wrap", Value::Bool(true));
        } else if (kind == "init" || kind == "initialize")
            && let Some(body) = body.filter(|b| !b.is_empty())
        {
            // init の中身が JSON でなければ、Mermaid は無視する。
            if let Some(Value::Obj(args)) = parse_json(js::trim(&body)) {
                merge(config, &args);
            }
        }
    }
    let chars: Vec<char> = text.chars().collect();
    let ranges: Vec<(usize, usize)> = directive_ranges(&chars)
        .into_iter()
        .map(|(a, b, _, _)| (a, b))
        .collect();
    let bytes = char_ranges_to_bytes(text, &ranges);
    if let Some(s) = steps {
        s.push(edits16(text, &bytes, |_| 0));
    }
    remove_ranges(text, &bytes)
}

/// `<(\w+)([^>]*)>` の属性の `="…"` を `='…'` に。
fn quote_attrs(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
            j += 1;
        }
        if j == i + 1 {
            i += 1;
            continue;
        }
        let Some(close) = text[j..].find('>').map(|k| k + j) else {
            i += 1;
            continue;
        };
        out.push_str(&text[last..j]);
        let attrs = &text[j..close];
        let ab = attrs.as_bytes();
        let mut k = 0;
        let mut alast = 0;
        while k + 1 < ab.len() {
            if ab[k] == b'='
                && ab[k + 1] == b'"'
                && let Some(e) = attrs[k + 2..].find('"')
            {
                let e = e + k + 2;
                out.push_str(&attrs[alast..k]);
                out.push_str("='");
                out.push_str(&attrs[k + 2..e]);
                out.push('\'');
                alast = e + 1;
                k = e + 1;
                continue;
            }
            k += 1;
        }
        out.push_str(&attrs[alast..]);
        out.push('>');
        last = close + 1;
        i = close + 1;
    }
    out.push_str(&text[last..]);
    out
}

/// `steps` は正規化した文字列から `with_comments` への写し。
pub fn preprocess(source: &str, mut steps: Option<&mut Vec<Vec<Edit>>>) -> Result<Preprocessed> {
    let mut text = normalize_line_breaks(source);
    // タグには閉じ括弧が要るので、最後の `>` の後ろは当たらない。
    if text.contains('<')
        && let Some(last_close) = text.rfind('>').filter(|&c| c > 0)
    {
        text = quote_attrs(&text[..last_close + 1]) + &text[last_close + 1..];
    }

    let mut config = Config::new();
    let mut title = None;
    let mut line_offset = 0;

    let front = if text.contains("---") {
        let chars: Vec<char> = text.chars().collect();
        match_front_matter(&chars)
    } else {
        None
    };
    if let Some(front) = front {
        let body = if !front.indent.is_empty() {
            front
                .body
                .split('\n')
                .map(|line| line.strip_prefix(front.indent.as_str()).unwrap_or(line))
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            front.body.clone()
        };
        let parsed = parse_yaml(&body).map_err(|m| {
            PeleError::new(
                format!("Front matter is not valid YAML. {m}"),
                ErrorCode::Syntax,
            )
        })?;
        if let Value::Obj(parsed) = &parsed {
            if let Some(t) = parsed.get("title").filter(|t| t.truthy()) {
                title = Some(t.to_js_string());
            }
            if let Some(Value::Obj(c)) = parsed.get("config") {
                merge(&mut config, c);
            }
            if let Some(mode) = parsed.get("displayMode").filter(|m| m.truthy()) {
                let mode = mode.to_js_string();
                if !matches!(config.get("gantt"), Some(Value::Obj(_))) {
                    config.set("gantt", Value::Obj(Map::new()));
                }
                if let Some(Value::Obj(g)) = config.get_mut("gantt") {
                    g.set("displayMode", Value::Str(mode));
                }
            }
        }
        let byte_len = text
            .char_indices()
            .nth(front.length)
            .map(|(b, _)| b)
            .unwrap_or(text.len());
        line_offset = text[..byte_len].matches('\n').count();
        if let Some(s) = steps.as_deref_mut() {
            s.push(vec![(0, js::len16(&text[..byte_len]), 0)]);
        }
        text = text[byte_len..].to_string();
    }

    let text = directives(&text, &mut config, steps);
    Ok(Preprocessed {
        text: cleanup_comments(&text, None),
        title,
        config,
        with_comments: text,
        line_offset,
    })
}

const LINE_BREAKS: [char; 4] = ['\n', '\r', '\u{2028}', '\u{2029}'];

/// Mermaid の `text.replace(/keyword.*:\S*#.*;/g, 最後の文字を落とす)` と同じこと: キーワードの後にコロン、色の
/// ような `#`、さらに後にセミコロンがある行では、最後のセミコロンを落とす。
fn drop_color_semicolons(text: &str, keyword: &str, steps: Option<&mut Vec<Vec<Edit>>>) -> String {
    let mut edits: Vec<(usize, usize)> = Vec::new();
    let Some(mut from) = text.find(keyword) else {
        if let Some(s) = steps {
            s.push(Vec::new());
        }
        return text.to_string();
    };
    let b = text.as_bytes();
    let mut out = String::new();
    let mut last = 0;
    loop {
        let line_end = text[from..]
            .find(LINE_BREAKS)
            .map(|k| k + from)
            .unwrap_or(text.len());
        let mut after_colon = false;
        let mut hash = None;
        for (j, c) in text[from + keyword.len()..line_end].char_indices() {
            let j = j + from + keyword.len();
            if c == ':' {
                after_colon = true;
            } else if c == '#' {
                if after_colon {
                    hash = Some(j);
                    break;
                }
            } else if is_space_char(c) {
                after_colon = false;
            }
        }
        if let Some(hash) = hash
            && let Some(semi) = text[..line_end].rfind(';').filter(|&s| s > hash)
        {
            edits.push((semi, semi + 1));
            out.push_str(&text[last..semi]);
            last = semi + 1;
        }
        if line_end >= text.len() {
            break;
        }
        // 行の終わりの文字の次から。
        let next = line_end + text[line_end..].chars().next().map_or(1, char::len_utf8);
        let _ = b;
        match text[next.min(text.len())..].find(keyword) {
            Some(k) => from = k + next,
            None => break,
        }
    }
    if let Some(s) = steps {
        s.push(edits16(text, &edits, |_| 0));
    }
    if last == 0 {
        text.to_string()
    } else {
        out + &text[last..]
    }
}

/// Mermaid は `#name;` の符号を置き字の陰に隠し、`#` と `;` が読みを通り抜けるようにする。
pub fn encode_entities(text: &str, mut steps: Option<&mut Vec<Vec<Edit>>>) -> String {
    if !text.contains('#') {
        return text.to_string();
    }
    let text = drop_color_semicolons(text, "style", steps.as_deref_mut());
    let text = drop_color_semicolons(&text, "classDef", steps.as_deref_mut());
    // `/#\w+;/g`
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut edits: Vec<Edit> = Vec::new();
    let mut pos = Pos16::new(&text);
    let mut last = 0;
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'#' {
            let mut j = i + 1;
            while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                j += 1;
            }
            if j > i + 1 && j < b.len() && b[j] == b';' {
                let inner = &text[i + 1..j];
                let numeric = inner.bytes().all(|c| c.is_ascii_digit());
                let code = format!("{}{}¶ß", if numeric { "ﬂ°°" } else { "ﬂ°" }, inner);
                if steps.is_some() {
                    edits.push((pos.at(i), pos.at(j + 1), js::len16(&code)));
                }
                out.push_str(&text[last..i]);
                out.push_str(&code);
                last = j + 1;
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out.push_str(&text[last..]);
    if let Some(s) = steps {
        s.push(edits);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_matter_and_comments() {
        let p = preprocess("---\ntitle: T\n---\nflowchart LR\n%% note\nA-->B", None).unwrap();
        assert_eq!(p.title.as_deref(), Some("T"));
        assert_eq!(p.text, "flowchart LR\nA-->B");
        assert_eq!(p.line_offset, 3);
        let p = preprocess(
            "%%{init: {'flowchart': {'curve': 'linear'}}}%%\ngraph TD\nA",
            None,
        )
        .unwrap();
        assert!(p.config.get("flowchart").is_some());
        assert_eq!(p.text, "graph TD\nA");
        assert_eq!(
            encode_entities("A[#quot; #35;]", None),
            "A[ﬂ°quot¶ß ﬂ°°35¶ß]"
        );
    }

    #[test]
    fn comments_after_multibyte_text() {
        assert_eq!(
            cleanup_comments("日本\n%% c\n語\u{2028}%% d\nx", None),
            "日本\n語\u{2028}x"
        );
    }
}
