//! Mermaid が Langium の文法で定める図の字句解析（pele の `src/diagrams/common/tokens.ts`）。字句の種類は順に試し、
//! 最初に合ったものを取る（Chevrotain と同じ）。
//!
//! JS は粘着（`y`）の正規表現で字句を表すが、ここには正規表現がないので、同じ所に合う手書きの関数で表す
//! （`Pattern::Regex`）。関数は UTF-16 の符号単位の `src` と位置を受け、合えば終わりの位置を返す。図の種類を移す
//! ときは、TS の正規表現をそのまま関数に書き直す（後戻りで答えが変わらないかを確かめること）。共通の字句
//! （`ACC_DESCR` など）と `keyword`・`title_before` はここにある。
//!
//! 字句の表は `Lexicon::new` で 1 度だけ用意し（TS の `prepare` と `WeakMap`）、`tokenize` に渡す。表は
//! `LazyLock` の static に置ける。
//!
//! ```ignore
//! static LEX: LazyLock<Lexicon> = LazyLock::new(|| Lexicon::new(vec![
//!     keyword("showData"),
//!     TokenType::literal(":", ":"),
//!     TokenType::new("NUMBER", Pattern::scanner(number_scanner)).first("-0123456789"),
//!     ACC_DESCR.clone(),
//!     NEWLINE.clone(),
//!     WHITESPACE.clone(),
//! ]));
//! let mut r = Reader::new(tokenize(src, &LEX, "pie")?, &LEX, "pie");
//! ```

use std::sync::{Arc, LazyLock};

use crate::errors::{PeleError, Result, syntax_error};
use crate::js;
use crate::util::chars::{NONE, is_space};

/// 粘着の正規表現か、TS の `match`。`previous` は最後の見える字句の種類（初めは -1）。合えば終わりの位置。
/// 合っても長さが 0 なら合わなかったものとして扱う（TS と同じ）。
pub type Match = Arc<dyn Fn(&[u16], usize, i32) -> Option<usize> + Send + Sync>;
/// TS の `Scanner`。文字列ごとに作り直すので、走査したものを覚えておける。
pub type Scanner = Box<dyn FnMut(&[u16], usize, i32) -> Option<usize>>;
pub type MakeScanner = Arc<dyn Fn() -> Scanner + Send + Sync>;

#[derive(Clone)]
pub enum Pattern {
    /// 文字どおりの文字列。
    Literal(Vec<u16>),
    /// 粘着の正規表現の代わり。合わなかったときだけ `opener` を見る。
    Regex(Match),
    /// TS の `match`: 文字列を何度も走査しうる正規表現や、前の字句によって変わる正規表現の代わり。`opener` は見ない。
    Match(Match),
    /// TS の `scanner`。
    Scanner(MakeScanner),
}

impl Pattern {
    pub fn regex(f: impl Fn(&[u16], usize) -> Option<usize> + Send + Sync + 'static) -> Self {
        Pattern::Regex(Arc::new(move |src, at, _| f(src, at)))
    }

    pub fn matcher(
        f: impl Fn(&[u16], usize, i32) -> Option<usize> + Send + Sync + 'static,
    ) -> Self {
        Pattern::Match(Arc::new(f))
    }

    pub fn scanner(make: impl Fn() -> Scanner + Send + Sync + 'static) -> Self {
        Pattern::Scanner(Arc::new(make))
    }
}

#[derive(Clone)]
pub struct TokenType {
    pub name: String,
    pub pattern: Pattern,
    pub hidden: bool,
    /// 閉じが要る形の始まり。これが合って全体が合わなければ、残りの文字に閉じはないので、この種類はもう試さない。
    pub opener: Option<Match>,
    /// 正規表現が合いうる最初の ASCII の文字すべて（分かっているとき）。その文字と ASCII の外の文字でだけ試す。
    pub first: Option<String>,
    /// Chevrotain の LONGER_ALT。この種類が合ったら、同じ所でこれらを順に試し、より長く合った最初のものを取る。
    /// TS は字句の種類そのものを並べる。ここでは名前で書き、表の中でその名前を持つ最初の種類を指す。
    pub longer: Vec<String>,
}

impl TokenType {
    pub fn new(name: impl Into<String>, pattern: Pattern) -> Self {
        TokenType {
            name: name.into(),
            pattern,
            hidden: false,
            opener: None,
            first: None,
            longer: Vec::new(),
        }
    }

    /// `{ name, pattern: 'text' }`。
    pub fn literal(name: impl Into<String>, text: &str) -> Self {
        Self::new(name, Pattern::Literal(js::utf16(text)))
    }

    pub fn hidden(mut self) -> Self {
        self.hidden = true;
        self
    }

    pub fn first(mut self, first: &str) -> Self {
        self.first = Some(first.to_string());
        self
    }

    pub fn opener(
        mut self,
        f: impl Fn(&[u16], usize) -> Option<usize> + Send + Sync + 'static,
    ) -> Self {
        self.opener = Some(Arc::new(move |src, at, _| f(src, at)));
        self
    }

    pub fn longer(mut self, names: &[&str]) -> Self {
        self.longer = names.iter().map(|s| s.to_string()).collect();
        self
    }

    /// `{ ...type, name }`。
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// `{ ...type, pattern }`（`match`・`scanner` を足すときも）。
    pub fn with_pattern(mut self, pattern: Pattern) -> Self {
        self.pattern = pattern;
        self
    }
}

/// 字句の種類の表と、そこから用意したもの（TS の `Table`）。
pub struct Lexicon {
    pub types: Vec<TokenType>,
    /// ASCII の文字ごとに、その文字で始まりうる種類（試す順）。
    starting: Vec<Vec<usize>>,
    every: Vec<usize>,
    longer: Vec<Vec<usize>>,
}

impl Lexicon {
    pub fn new(types: Vec<TokenType>) -> Self {
        let every: Vec<usize> = (0..types.len()).collect();
        let starting = (0..128u16)
            .map(|c| {
                every
                    .iter()
                    .copied()
                    .filter(|&k| match (&types[k].pattern, &types[k].first) {
                        (Pattern::Literal(text), _) => text.first() == Some(&c),
                        (_, None) => true,
                        (_, Some(first)) => first.encode_utf16().any(|f| f == c),
                    })
                    .collect()
            })
            .collect();
        let longer = types
            .iter()
            .map(|t| {
                t.longer
                    .iter()
                    .map(|name| {
                        types
                            .iter()
                            .position(|u| &u.name == name)
                            .unwrap_or_else(|| panic!("no token type named {name}"))
                    })
                    .collect()
            })
            .collect();
        Lexicon {
            types,
            starting,
            every,
            longer,
        }
    }

    /// 名前の種類の番号。
    pub fn index(&self, name: &str) -> Option<usize> {
        self.types.iter().position(|t| t.name == name)
    }
}

/// 字句の列。最後に種類 -1 の終わりがある。位置は UTF-16 の符号単位。
pub struct Tokens {
    pub kinds: Vec<i32>,
    pub starts: Vec<usize>,
    pub ends: Vec<usize>,
    pub src: Vec<u16>,
}

pub fn tokenize(src: &str, lexicon: &Lexicon, diagram: &str) -> Result<Tokens> {
    let src = js::utf16(src);
    let types = &lexicon.types;
    let mut kinds: Vec<i32> = Vec::new();
    let mut starts: Vec<usize> = Vec::new();
    let mut ends: Vec<usize> = Vec::new();
    let mut dead = vec![false; types.len()];
    let mut scanners: Vec<Option<Scanner>> = (0..types.len()).map(|_| None).collect();
    let n = src.len();

    let mut p = 0;
    'scan: while p < n {
        let c = src[p] as usize;
        let list = if c < 128 {
            &lexicon.starting[c]
        } else {
            &lexicon.every
        };
        for &k in list {
            let previous = kinds.last().copied().unwrap_or(-1);
            let mut try_match = |k: usize| -> Option<usize> {
                if dead[k] {
                    return None;
                }
                let type_ = &types[k];
                match &type_.pattern {
                    Pattern::Match(f) => f(&src, p, previous),
                    Pattern::Scanner(make) => {
                        scanners[k].get_or_insert_with(|| make())(&src, p, previous)
                    }
                    Pattern::Literal(text) => src[p..].starts_with(text).then_some(p + text.len()),
                    Pattern::Regex(f) => {
                        let end = f(&src, p, previous);
                        if end.is_none()
                            && let Some(opener) = &type_.opener
                            && opener(&src, p, previous).is_some()
                        {
                            dead[k] = true;
                        }
                        end
                    }
                }
            };
            let Some(mut end) = try_match(k).filter(|&e| e > p) else {
                continue;
            };
            let mut kind = k;
            for &alt in &lexicon.longer[k] {
                if let Some(alt_end) = try_match(alt)
                    && alt_end > end
                {
                    kind = alt;
                    end = alt_end;
                    break;
                }
            }
            if !types[kind].hidden {
                kinds.push(kind as i32);
                starts.push(p);
                ends.push(end);
            }
            p = end;
            continue 'scan;
        }
        return Err(syntax_error(diagram, &src, p, "", true));
    }
    kinds.push(-1);
    starts.push(n);
    ends.push(n);
    Ok(Tokens {
        kinds,
        starts,
        ends,
        src,
    })
}

fn at(src: &[u16], p: usize) -> u32 {
    src.get(p).map_or(NONE, |&c| c as u32)
}

/// `[\t ]*`
fn blanks(src: &[u16], mut p: usize) -> usize {
    while matches!(at(src, p), 9 | 32) {
        p += 1;
    }
    p
}

/// ASCII の `word` が `p` にあるか（大文字小文字を区別する）。
fn has(src: &[u16], p: usize, word: &str) -> bool {
    src.get(p..)
        .is_some_and(|s| s.len() >= word.len() && word.bytes().zip(s).all(|(w, &c)| w as u16 == c))
}

/// `[^\n\r]*` の終わり。
fn line_stop(src: &[u16], mut p: usize) -> usize {
    while p < src.len() && src[p] != 10 && src[p] != 13 {
        p += 1;
    }
    p
}

/// `[^\n\r]*?(?=%%)|[^\n\r]*`: 行の中の最初の `%%` の前まで、なければ行の終わりまで。
fn to_comment(src: &[u16], from: usize) -> usize {
    let stop = line_stop(src, from);
    (from..stop)
        .find(|&k| src[k] == 37 && at(src, k + 1) == 37)
        .unwrap_or(stop)
}

/// `(?:\r?\n|(?!\S))`
fn line_tail(src: &[u16], p: usize) -> Option<usize> {
    let c = at(src, p);
    if c == 13 && at(src, p + 1) == 10 {
        Some(p + 2)
    } else if c == 10 {
        Some(p + 1)
    } else if c == NONE || is_space(c) {
        Some(p)
    } else {
        None
    }
}

/// `---[\t ]*\r?\n`
fn yaml_head(src: &[u16], p: usize) -> Option<usize> {
    if !has(src, p, "---") {
        return None;
    }
    let q = blanks(src, p + 3);
    if at(src, q) == 13 && at(src, q + 1) == 10 {
        Some(q + 2)
    } else if at(src, q) == 10 {
        Some(q + 1)
    } else {
        None
    }
}

/// `---(?:\r?\n|(?!\S))`
fn yaml_foot(src: &[u16], p: usize) -> Option<usize> {
    if has(src, p, "---") {
        line_tail(src, p + 3)
    } else {
        None
    }
}

/// `---[\t ]*\r?\n(?:[\S\s]*?\r?\n)?---(?:\r?\n|(?!\S))`
fn yaml(src: &[u16], p: usize) -> Option<usize> {
    let h = yaml_head(src, p)?;
    // 中身があれば、その最後の改行の後に閉じがある。改行ごとに早い順に試す。
    for j in h..src.len() {
        if src[j] == 10
            && let Some(end) = yaml_foot(src, j + 1)
        {
            return Some(end);
        }
    }
    yaml_foot(src, h)
}

/// `[\t ]*%%{`
fn directive_head(src: &[u16], p: usize) -> Option<usize> {
    let q = blanks(src, p);
    has(src, q, "%%{").then_some(q + 3)
}

/// `[\t ]*%%{[\S\s]*?}%%(?:\r?\n|(?!\S))`
fn directive(src: &[u16], p: usize) -> Option<usize> {
    let h = directive_head(src, p)?;
    (h..src.len()).find_map(|k| {
        if has(src, k, "}%%") {
            line_tail(src, k + 3)
        } else {
            None
        }
    })
}

/// `[\t ]*accDescr(?:[\t ]*:([^\n\r]*?(?=%%)|[^\n\r]*)|\s*{([^}]*)})`
fn acc_descr_re(src: &[u16], p: usize) -> Option<usize> {
    let q = blanks(src, p);
    if !has(src, q, "accDescr") {
        return None;
    }
    let q = q + 8;
    let r = blanks(src, q);
    if at(src, r) == 58 {
        return Some(to_comment(src, r + 1));
    }
    let mut r = q;
    while is_space(at(src, r)) {
        r += 1;
    }
    if at(src, r) != 123 {
        return None;
    }
    (r + 1..src.len()).find(|&k| src[k] == 125).map(|k| k + 1)
}

/// `[\t ]*accTitle[\t ]*:(?:[^\n\r]*?(?=%%)|[^\n\r]*)`
fn acc_title_re(src: &[u16], p: usize) -> Option<usize> {
    let q = blanks(src, p);
    if !has(src, q, "accTitle") {
        return None;
    }
    let r = blanks(src, q + 8);
    (at(src, r) == 58).then(|| to_comment(src, r + 1))
}

/// `[\t ]*title(?:[\t ][^\n\r]*?(?=%%)|[\t ][^\n\r]*|…)`。`empty` は空の 3 つ目の選択肢が合うか。
fn title_re(src: &[u16], p: usize, empty: &dyn Fn(&[u16], usize) -> bool) -> Option<usize> {
    let q = blanks(src, p);
    if !has(src, q, "title") {
        return None;
    }
    let q = q + 5;
    if matches!(at(src, q), 9 | 32) {
        Some(to_comment(src, q + 1))
    } else if empty(src, q) {
        Some(q)
    } else {
        None
    }
}

/// `"([^"\\]|\\.)*"|'([^'\\]|\\.)*'`
fn string_re(src: &[u16], p: usize) -> Option<usize> {
    let quote = at(src, p);
    if quote != 34 && quote != 39 {
        return None;
    }
    let mut i = p + 1;
    loop {
        let c = at(src, i);
        if c == NONE {
            return None;
        }
        if c == quote {
            return Some(i + 1);
        }
        if c == 92 {
            // `.` は行の終わりに合わない。
            let next = at(src, i + 1);
            if next == NONE || matches!(next, 10 | 13 | 0x2028 | 0x2029) {
                return None;
            }
            i += 2;
        } else {
            i += 1;
        }
    }
}

pub static ACC_DESCR: LazyLock<TokenType> =
    LazyLock::new(|| TokenType::new("ACC_DESCR", Pattern::regex(acc_descr_re)).first("\t a"));
pub static ACC_TITLE: LazyLock<TokenType> =
    LazyLock::new(|| TokenType::new("ACC_TITLE", Pattern::regex(acc_title_re)).first("\t a"));
pub static TITLE: LazyLock<TokenType> = LazyLock::new(|| {
    TokenType::new(
        "TITLE",
        Pattern::regex(|src, p| title_re(src, p, &|_, _| true)),
    )
    .first("\t t")
});

/// `title` が長い名前を分けないよう、`rest` は名前の続き（TS の `rest` の正規表現が `at` に合うか）を答える。
/// 続きがあれば、語のすぐ後で終わる title にはしない。
pub fn title_before(rest: impl Fn(&[u16], usize) -> bool + Send + Sync + 'static) -> TokenType {
    TITLE.clone().with_pattern(Pattern::regex(move |src, p| {
        title_re(src, p, &|s, q| !rest(s, q))
    }))
}

pub static STRING: LazyLock<TokenType> =
    LazyLock::new(|| TokenType::new("STRING", Pattern::regex(string_re)).first("\"'"));
pub static NEWLINE: LazyLock<TokenType> = LazyLock::new(|| {
    TokenType::new(
        "NEWLINE",
        Pattern::regex(|src, p| {
            if at(src, p) == 13 && at(src, p + 1) == 10 {
                Some(p + 2)
            } else if at(src, p) == 10 {
                Some(p + 1)
            } else {
                None
            }
        }),
    )
    .first("\r\n")
});
pub static WHITESPACE: LazyLock<TokenType> = LazyLock::new(|| {
    TokenType::new("WHITESPACE", Pattern::regex(|src, p| Some(blanks(src, p))))
        .hidden()
        .first("\t ")
});
pub static YAML: LazyLock<TokenType> = LazyLock::new(|| {
    TokenType::new("YAML", Pattern::regex(yaml))
        .hidden()
        .opener(yaml_head)
        .first("-")
});
pub static DIRECTIVE: LazyLock<TokenType> = LazyLock::new(|| {
    TokenType::new("DIRECTIVE", Pattern::regex(directive))
        .hidden()
        .opener(directive_head)
        .first("\t %")
});
pub static SINGLE_LINE_COMMENT: LazyLock<TokenType> = LazyLock::new(|| {
    TokenType::new(
        "SINGLE_LINE_COMMENT",
        Pattern::regex(|src, p| {
            let q = blanks(src, p);
            has(src, q, "%%").then(|| line_stop(src, q + 2))
        }),
    )
    .first("\t %")
    .hidden()
});

/// Mermaid は語を、空白・コメント・入力の終わりでだけ終える。
pub fn keyword(word: &str) -> TokenType {
    let text = js::utf16(word);
    let first = word.chars().next().map(String::from).unwrap_or_default();
    TokenType::new(
        word,
        Pattern::regex(move |src, p| {
            if !src.get(p..).is_some_and(|s| s.starts_with(&text)) {
                return None;
            }
            let e = p + text.len();
            let c = at(src, e);
            ((c == 37 && at(src, e + 1) == 37) || c == NONE || is_space(c)).then_some(e)
        }),
    )
    .first(&first)
}

/// `[\t ]{2,}` を空白 1 つに。
fn squeeze(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut run = String::new();
    for c in text.chars() {
        if c == '\t' || c == ' ' {
            run.push(c);
            continue;
        }
        out.push_str(if run.chars().count() >= 2 { " " } else { &run });
        run.clear();
        out.push(c);
    }
    out.push_str(if run.chars().count() >= 2 { " " } else { &run });
    out
}

fn single(text: &str) -> String {
    squeeze(js::trim(text))
}

/// `[^\n\r]*` の終わり（バイトの位置）。
fn line_stop_str(text: &str, from: usize) -> usize {
    text[from..]
        .find(['\n', '\r'])
        .map_or(text.len(), |k| from + k)
}

fn blanks_str(text: &str, mut p: usize) -> usize {
    while matches!(text.as_bytes().get(p), Some(b'\t' | b' ')) {
        p += 1;
    }
    p
}

/// `title([\t ][^\n\r]*|)` の 1 つ目の組。
pub fn title_value(text: &str) -> String {
    let at = text.find("title").expect("title");
    let q = at + 5;
    let group = if matches!(text.as_bytes().get(q), Some(b'\t' | b' ')) {
        &text[q..line_stop_str(text, q)]
    } else {
        ""
    };
    single(group)
}

/// `accTitle[\t ]*:([^\n\r]*)` の 1 つ目の組。
pub fn acc_title_value(text: &str) -> String {
    for (at, _) in text.match_indices("accTitle") {
        let r = blanks_str(text, at + 8);
        if text.as_bytes().get(r) == Some(&b':') {
            return single(&text[r + 1..line_stop_str(text, r + 1)]);
        }
    }
    panic!("accTitle")
}

fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// `.replace(/^\s*/gm, '')`
fn strip_line_starts(chars: &[char]) -> Vec<char> {
    let n = chars.len();
    let mut out = Vec::with_capacity(n);
    let mut p = 0;
    loop {
        // 次の行の頭。
        let mut q = p;
        while q <= n && !(q == 0 || is_line_terminator(chars[q - 1])) {
            q += 1;
        }
        if q > n {
            out.extend_from_slice(&chars[p..]);
            return out;
        }
        out.extend_from_slice(&chars[p..q]);
        let mut e = q;
        while e < n && is_space(chars[e] as u32) {
            e += 1;
        }
        if e == q {
            if q < n {
                out.push(chars[q]);
            }
            p = q + 1;
            if p > n {
                return out;
            }
        } else {
            p = e;
        }
    }
}

/// `.replace(/\s+$/gm, '')`
fn strip_line_ends(chars: &[char]) -> Vec<char> {
    let n = chars.len();
    let mut out = Vec::with_capacity(n);
    let mut p = 0;
    while p < n {
        if is_space(chars[p] as u32) {
            let mut r = p;
            while r < n && is_space(chars[r] as u32) {
                r += 1;
            }
            if let Some(e) = (p + 1..=r)
                .rev()
                .find(|&e| e == n || is_line_terminator(chars[e]))
            {
                p = e;
                continue;
            }
        }
        out.push(chars[p]);
        p += 1;
    }
    out
}

/// `.replace(/[\n\r]{2,}/gm, '\n')`
fn squeeze_breaks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut run = 0;
    let mut held = String::new();
    for c in text.chars() {
        if c == '\n' || c == '\r' {
            run += 1;
            held.push(c);
            continue;
        }
        out.push_str(if run >= 2 { "\n" } else { &held });
        run = 0;
        held.clear();
        out.push(c);
    }
    out.push_str(if run >= 2 { "\n" } else { &held });
    out
}

/// `accDescr(?:[\t ]*:([^\n\r]*)|\s*{([^}]*)})` の組。
pub fn acc_descr_value(text: &str) -> String {
    for (at, _) in text.match_indices("accDescr") {
        let q = at + 8;
        let r = blanks_str(text, q);
        if text.as_bytes().get(r) == Some(&b':') {
            return single(&text[r + 1..line_stop_str(text, r + 1)]);
        }
        let rest = &text[q..];
        let Some(open) = rest.find(|c: char| !is_space(c as u32)) else {
            continue;
        };
        if !rest[open..].starts_with('{') {
            continue;
        }
        let body = &rest[open + 1..];
        let Some(close) = body.find('}') else {
            continue;
        };
        let chars: Vec<char> = body[..close].chars().collect();
        let stripped: String = strip_line_ends(&strip_line_starts(&chars))
            .into_iter()
            .collect();
        return squeeze_breaks(&squeeze(&stripped));
    }
    panic!("accDescr")
}

/// 引用符の文字列の中身。`\\` の後の文字は、決まった逃がしのほかはその文字。
pub fn string_value(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    if !text.contains('\\') {
        return chars[1.min(n)..n.saturating_sub(1).max(1.min(n))]
            .iter()
            .collect();
    }
    let mut out = String::with_capacity(text.len());
    let mut i = 1;
    while i + 1 < n {
        let c = chars[i];
        if c == '\\' {
            i += 1;
            out.push(match chars[i] {
                'b' => '\u{8}',
                'f' => '\u{c}',
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                'v' => '\u{b}',
                '0' => '\0',
                c => c,
            });
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

/// 手で書いた読み手のために字句を読む。失敗は Chevrotain の言い方で知らせる。
pub struct Reader<'a> {
    pub i: usize,
    pub tokens: Tokens,
    pub lexicon: &'a Lexicon,
    pub diagram: &'a str,
}

impl<'a> Reader<'a> {
    pub fn new(tokens: Tokens, lexicon: &'a Lexicon, diagram: &'a str) -> Self {
        Reader {
            i: 0,
            tokens,
            lexicon,
            diagram,
        }
    }

    /// 今の字句の種類。終わりは -1。
    pub fn kind(&self) -> i32 {
        self.kind_at(self.i)
    }

    pub fn kind_at(&self, i: usize) -> i32 {
        self.tokens.kinds.get(i).copied().unwrap_or(-1)
    }

    pub fn text_at(&self, index: usize) -> String {
        js::from16(&self.tokens.src[self.tokens.starts[index]..self.tokens.ends[index]])
    }

    pub fn text(&self) -> String {
        self.text_at(self.i)
    }

    pub fn take(&mut self) -> String {
        let text = self.text();
        self.i += 1;
        text
    }

    pub fn accept(&mut self, kind: i32) -> bool {
        if self.kind() != kind {
            return false;
        }
        self.i += 1;
        true
    }

    pub fn expect(&mut self, kind: i32) -> Result<String> {
        if self.kind() != kind {
            return Err(self.fail(&format!(
                "token of type '{}'",
                self.lexicon.types[kind as usize].name
            )));
        }
        Ok(self.take())
    }

    /// 失敗の知らせ。TS は投げるので、呼ぶ側は `return Err(r.fail(..))` とする。
    pub fn fail(&self, expected: &str) -> PeleError {
        let end = self.kind() == -1;
        let found = if end {
            "end of input".to_string()
        } else {
            format!("`{}`", self.text())
        };
        let Tokens {
            src, starts, ends, ..
        } = &self.tokens;
        let mut at = starts[self.i];
        if end {
            // 入力の終わりの失敗は、最後の字句の後（後ろの改行と隠れたコメントの前）に置く。
            let mut k = self.i;
            while k > 0 && js::trim(&js::from16(&src[starts[k - 1]..ends[k - 1]])).is_empty() {
                k -= 1;
            }
            if k > 0 {
                at = ends[k - 1];
            }
        }
        syntax_error(
            self.diagram,
            src,
            at,
            &format!("Expecting {expected} but found {found}."),
            false,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(t: &TokenType, src: &str, p: usize) -> Option<usize> {
        let src = js::utf16(src);
        match &t.pattern {
            Pattern::Regex(f) | Pattern::Match(f) => f(&src, p, -1),
            Pattern::Literal(text) => src[p..].starts_with(text).then_some(p + text.len()),
            Pattern::Scanner(make) => make()(&src, p, -1),
        }
    }

    #[test]
    fn matches_common_tokens() {
        assert_eq!(run(&ACC_DESCR, "  accDescr: a %% c\n", 0), Some(14));
        assert_eq!(run(&ACC_DESCR, "accDescr : a\nb", 0), Some(12));
        assert_eq!(run(&ACC_DESCR, "accDescr\n {a\n}x", 0), Some(14));
        assert_eq!(run(&ACC_DESCR, "accDescr {a", 0), None);
        assert_eq!(run(&ACC_TITLE, "accTitle:x%%y", 0), Some(10));
        assert_eq!(run(&ACC_TITLE, "accTitle x", 0), None);
        assert_eq!(run(&TITLE, "title", 0), Some(5));
        assert_eq!(run(&TITLE, "titles", 0), Some(5));
        assert_eq!(run(&TITLE, " title a b\r\n", 0), Some(10));
        assert_eq!(run(&TITLE, "title %%x", 0), Some(6));
        let tb = title_before(|src, p| at(src, p) == 's' as u32);
        assert_eq!(run(&tb, "titles", 0), None);
        assert_eq!(run(&tb, "title:", 0), Some(5));
        assert_eq!(run(&STRING, r#""a\"b" x"#, 0), Some(6));
        assert_eq!(run(&STRING, "'a\nb'", 0), Some(5));
        assert_eq!(run(&STRING, "\"a\\\nb\"", 0), None);
        assert_eq!(run(&STRING, "\"ab", 0), None);
        assert_eq!(run(&NEWLINE, "\r\n", 0), Some(2));
        assert_eq!(run(&NEWLINE, "\r", 0), None);
        assert_eq!(run(&YAML, "---\na: 1\n---\nx", 0), Some(13));
        assert_eq!(run(&YAML, "--- \r\n---", 0), Some(9));
        assert_eq!(run(&YAML, "---\n---x\n--- y", 0), Some(12));
        assert_eq!(run(&YAML, "---\na", 0), None);
        assert_eq!(run(&DIRECTIVE, " %%{init: {}}%%\nx", 0), Some(16));
        assert_eq!(run(&DIRECTIVE, "%%{a}%%b}%% c", 0), Some(11));
        assert_eq!(run(&SINGLE_LINE_COMMENT, " %% c\nx", 0), Some(5));
        let pie = keyword("pie");
        assert_eq!(run(&pie, "pie showData", 0), Some(3));
        assert_eq!(run(&pie, "pie%%x", 0), Some(3));
        assert_eq!(run(&pie, "pies", 0), None);
        assert_eq!(run(&pie, "pie", 0), Some(3));
    }

    #[test]
    fn reads_values() {
        assert_eq!(title_value("title  A   b  "), "A b");
        assert_eq!(title_value("title"), "");
        assert_eq!(acc_title_value("  accTitle :  The  t "), "The t");
        assert_eq!(acc_descr_value("accDescr: one  two"), "one two");
        assert_eq!(
            acc_descr_value("accDescr {\n   line one  \n\n\n   line  two\n  }"),
            "line one\nline two"
        );
        assert_eq!(string_value(r#""a\nb\"c\q""#), "a\nb\"cq");
        assert_eq!(string_value("'plain'"), "plain");
    }

    fn lexicon() -> Lexicon {
        Lexicon::new(vec![
            keyword("pie"),
            TokenType::literal(":", ":"),
            TokenType::new(
                "ID",
                Pattern::regex(|src, p| {
                    let mut e = p;
                    while (e < src.len())
                        && (src[e] as u8 as char).is_ascii_alphabetic()
                        && src[e] < 128
                    {
                        e += 1;
                    }
                    Some(e)
                }),
            )
            .first("abcdefghijklmnopqrstuvwxyz"),
            NEWLINE.clone(),
            WHITESPACE.clone(),
            YAML.clone(),
            SINGLE_LINE_COMMENT.clone(),
        ])
    }

    #[test]
    fn tokenizes_and_reads() {
        let lex = lexicon();
        let tokens = tokenize("pie a:b %% c\n", &lex, "pie").unwrap();
        assert_eq!(tokens.kinds, vec![0, 2, 1, 2, 3, -1]);
        assert_eq!(tokens.starts, vec![0, 4, 5, 6, 12, 13]);
        let mut r = Reader::new(tokens, &lex, "pie");
        assert_eq!(r.expect(0).unwrap(), "pie");
        assert_eq!(r.take(), "a");
        assert!(r.accept(1));
        let err = r.expect(1).unwrap_err();
        assert_eq!(
            err.message,
            "Parse error on line 1:\npie a:b %% c\n------^\nExpecting token of type ':' but found `b`."
        );
        r.i = 5;
        let err = r.fail("a section");
        assert_eq!(err.column, 8);

        let err = tokenize("pie ?", &lex, "pie").err().unwrap();
        assert_eq!(
            err.message,
            "Lexical error on line 1. Unrecognized text.\npie ?\n----^"
        );
    }
}
