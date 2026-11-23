//! 小さな YAML の読み手（pele の `src/util/yaml.ts`）。Mermaid が js-yaml に JSON スキーマで渡すもの（ブロックと
//! フローの集まり、引用符つき・なしのスカラー、ブロックスカラー）を読む。JSON（`%%{init: …}%%`）もここで読む。
//!
//! 文字は `char` で数える。位置が出力に届かず、比べるのは ASCII の文字だけなので、UTF-16 で数えるのと結果は同じ。

use crate::js;

/// YAML の値。オブジェクトは入れた順を保つ（JS の `Object.keys` は整数の名前を先に並べるが、出力には届かない）。
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Value>),
    Obj(Map),
}

/// 名前つきの値の並び（JS のオブジェクト）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Map(pub Vec<(String, Value)>);

impl Map {
    pub fn new() -> Self {
        Map(Vec::new())
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.0.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn set(&mut self, key: &str, value: Value) {
        match self.get_mut(key) {
            Some(slot) => *slot = value,
            None => self.0.push((key.to_string(), value)),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &(String, Value)> {
        self.0.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Value {
    /// JS の真偽。
    pub fn truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Num(n) => js::truthy(*n),
            Value::Str(s) => !s.is_empty(),
            Value::Arr(_) | Value::Obj(_) => true,
        }
    }

    /// `String(value)`。
    pub fn to_js_string(&self) -> String {
        match self {
            Value::Null => "null".into(),
            Value::Bool(b) => b.to_string(),
            Value::Num(n) => js::fmt(*n),
            Value::Str(s) => s.clone(),
            Value::Arr(a) => a
                .iter()
                .map(|v| match v {
                    Value::Null => String::new(),
                    v => v.to_js_string(),
                })
                .collect::<Vec<_>>()
                .join(","),
            Value::Obj(_) => "[object Object]".into(),
        }
    }

    /// `Number(value)`。
    pub fn to_number(&self) -> f64 {
        match self {
            Value::Null => 0.0,
            Value::Bool(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            Value::Num(n) => *n,
            Value::Str(s) => js::number(s),
            Value::Arr(a) => match a.len() {
                0 => 0.0,
                1 => js::number(&a[0].to_js_string()),
                _ => f64::NAN,
            },
            Value::Obj(_) => f64::NAN,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_obj(&self) -> Option<&Map> {
        match self {
            Value::Obj(m) => Some(m),
            _ => None,
        }
    }
}

/// 読めなかった、という知らせ（`Error` の message）。
#[derive(Debug)]
pub enum YamlError {
    /// 引用符つきのスカラーかフローの集まりが、読んだところで終わっている。
    Incomplete(String),
    Error(String),
}

impl YamlError {
    pub fn message(&self) -> &str {
        match self {
            YamlError::Incomplete(m) | YamlError::Error(m) => m,
        }
    }
}

type R<T> = std::result::Result<T, YamlError>;

fn err<T>(m: &str) -> R<T> {
    Err(YamlError::Error(m.to_string()))
}

const MAX_DEPTH: usize = 64;

type Chars = Vec<char>;

fn s_of(c: &[char]) -> String {
    c.iter().collect()
}

fn trim_chars(c: &[char]) -> &[char] {
    let mut a = 0;
    let mut b = c.len();
    while a < b && js::is_js_space(c[a]) {
        a += 1;
    }
    while b > a && js::is_js_space(c[b - 1]) {
        b -= 1;
    }
    &c[a..b]
}

fn trim_start_len(c: &[char]) -> usize {
    let mut a = 0;
    while a < c.len() && js::is_js_space(c[a]) {
        a += 1;
    }
    c.len() - a
}

fn index_of(s: &[char], c: char, from: usize) -> Option<usize> {
    if from >= s.len() {
        return None;
    }
    s[from..].iter().position(|&x| x == c).map(|i| i + from)
}

struct Reader {
    lines: Vec<Chars>,
    i: usize,
}

impl Reader {
    fn new(src: &str) -> Self {
        let src = if src.contains('\r') {
            src.replace("\r\n", "\n").replace('\r', "\n")
        } else {
            src.to_string()
        };
        Reader {
            lines: src.split('\n').map(|l| l.chars().collect()).collect(),
            i: 0,
        }
    }

    fn skip_blank(&mut self) {
        while self.i < self.lines.len() {
            let t = trim_chars(&self.lines[self.i]);
            if !t.is_empty() && t[0] != '#' && t != ['-', '-', '-'] {
                break;
            }
            self.i += 1;
        }
    }

    fn indent(&self) -> R<usize> {
        let line = &self.lines[self.i];
        let mut k = 0;
        while k < line.len() && line[k] == ' ' {
            k += 1;
        }
        if line.get(k) == Some(&'\t') {
            return err("YAML: tabs are not allowed for indentation");
        }
        Ok(k)
    }
}

fn is_blank(ch: Option<&char>) -> bool {
    matches!(ch, Some(' ' | '\t' | '\n' | '\r'))
}

/// `/[\s#:,[\]{}&*!|>'"%@`]/`
fn key_start(c: char) -> bool {
    js::is_js_space(c) || "#:,[]{}&*!|>'\"%@`".contains(c)
}

struct KeySplit {
    key: Chars,
    length: usize,
}

/// 行の頭の `key: rest` を分ける。
fn split_key(line: &[char]) -> Option<KeySplit> {
    let first = line.first().copied();
    let end;
    let mut colon;
    if first == Some('"') || first == Some('\'') {
        let first = first.unwrap();
        let mut i = 1;
        loop {
            if i >= line.len() {
                return None;
            }
            let c = line[i];
            if first == '"' && c == '\\' {
                i += 2;
            } else if c == first {
                if first == '"' || line.get(i + 1) != Some(&'\'') {
                    break;
                }
                i += 2;
            } else {
                i += 1;
            }
        }
        end = i + 1;
        colon = end;
        while is_blank(line.get(colon)) {
            colon += 1;
        }
        if line.get(colon) != Some(&':') {
            return None;
        }
    } else {
        let first = first?;
        if key_start(first) {
            return None;
        }
        if first == '-'
            && (line.len() < 2 || is_blank(line.get(1)) || line[1] == ':' || line[1] == '#')
        {
            return None;
        }
        colon = 1;
        while colon < line.len() && line[colon] != ':' && line[colon] != '#' {
            colon += 1;
        }
        if line.get(colon) != Some(&':') {
            return None;
        }
        let mut e = colon;
        while e > 0 && is_blank(line.get(e - 1)) {
            e -= 1;
        }
        end = e;
    }
    let mut length = colon + 1;
    if length < line.len() && !is_blank(line.get(length)) {
        return None;
    }
    while is_blank(line.get(length)) {
        length += 1;
    }
    Some(KeySplit {
        key: line[..end].to_vec(),
        length,
    })
}

fn all(s: &str, f: impl Fn(char) -> bool) -> bool {
    !s.is_empty() && s.chars().all(f)
}

fn strip_sign(s: &str) -> &str {
    s.strip_prefix(['-', '+']).unwrap_or(s)
}

/// `/^[-+]?(?:0|[1-9][0-9]*)$/`
fn re_int(s: &str) -> bool {
    let b = strip_sign(s);
    b == "0" || (all(b, |c| c.is_ascii_digit()) && !b.starts_with('0'))
}

/// `/^[-+]?(?:[0-9][0-9]*(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][-+]?[0-9]+)?$/`
fn re_float(s: &str) -> bool {
    let b = strip_sign(s).as_bytes();
    let mut i = 0;
    if i < b.len() && b[i].is_ascii_digit() {
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i < b.len() && b[i] == b'.' {
            i += 1;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
        }
    } else if i < b.len() && b[i] == b'.' {
        i += 1;
        let s = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == s {
            return false;
        }
    } else {
        return false;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
            i += 1;
        }
        let s = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == s {
            return false;
        }
    }
    i == b.len()
}

fn re_radix(s: &str, prefix: &str, ok: impl Fn(char) -> bool) -> bool {
    strip_sign(s)
        .strip_prefix(prefix)
        .is_some_and(|b| all(b, ok))
}

fn scalar(raw: &str) -> Value {
    let s = js::trim(raw);
    match s {
        "" | "~" | "null" | "Null" | "NULL" => return Value::Null,
        "true" | "True" | "TRUE" => return Value::Bool(true),
        "false" | "False" | "FALSE" => return Value::Bool(false),
        _ => {}
    }
    let c = s.chars().next().unwrap();
    if c != '-' && c != '+' && c != '.' && !c.is_ascii_digit() {
        return Value::Str(s.to_string());
    }
    if re_int(s) || re_float(s) {
        return Value::Num(js::number(s));
    }
    if re_radix(s, "0x", |c| c.is_ascii_hexdigit())
        || re_radix(s, "0o", |c| ('0'..='7').contains(&c))
        || re_radix(s, "0b", |c| c == '0' || c == '1')
    {
        let neg = s.starts_with('-');
        let v = js::number(if neg || s.starts_with('+') {
            &s[1..]
        } else {
            s
        });
        return Value::Num(if neg { -v } else { v });
    }
    if matches!(strip_sign(s), ".inf" | ".Inf" | ".INF") {
        return Value::Num(if s.starts_with('-') {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        });
    }
    if matches!(s, ".nan" | ".NaN" | ".NAN") {
        return Value::Num(f64::NAN);
    }
    Value::Str(s.to_string())
}

fn escape(e: char) -> Option<&'static str> {
    Some(match e {
        '0' => "\0",
        'a' => "\x07",
        'b' => "\x08",
        't' => "\t",
        'n' => "\n",
        'v' => "\x0b",
        'f' => "\x0c",
        'r' => "\r",
        'e' => "\x1b",
        ' ' => " ",
        '"' => "\"",
        '/' => "/",
        '\\' => "\\",
        'N' => "\u{85}",
        '_' => "\u{a0}",
        'L' => "\u{2028}",
        'P' => "\u{2029}",
        _ => return None,
    })
}

/// スカラーの改行をたたむ。改行のまわりの空白は消え、1 つの改行は空白に、続く改行はそのまま残る。
fn fold(text: &str) -> String {
    if !text.contains('\n') {
        return text.to_string();
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let last = lines.len() - 1;
    let mut joined = String::new();
    for (i, line) in lines.iter().enumerate() {
        let b = line.as_bytes();
        let mut from = 0;
        let mut to = b.len();
        if i > 0 {
            while from < to && (b[from] == b' ' || b[from] == b'\t') {
                from += 1;
            }
        }
        if i < last {
            while to > from && (b[to - 1] == b' ' || b[to - 1] == b'\t') {
                to -= 1;
            }
        }
        if i > 0 {
            joined.push('\n');
        }
        joined.push_str(&line[from..to]);
    }
    let mut out = String::new();
    let mut run = 0;
    for c in joined.chars() {
        if c == '\n' {
            run += 1;
            continue;
        }
        flush_breaks(&mut out, run);
        run = 0;
        out.push(c);
    }
    flush_breaks(&mut out, run);
    out
}

fn flush_breaks(out: &mut String, run: usize) {
    if run == 1 {
        out.push(' ');
    } else if run > 1 {
        for _ in 0..run - 1 {
            out.push('\n');
        }
    }
}

fn unquote_double(body: &str) -> R<String> {
    if !body.contains('\\') {
        return Ok(fold(body));
    }
    // `\\\n[ \t]*` を消す。
    let mut cut = String::new();
    let b: Vec<char> = body.chars().collect();
    let mut i = 0;
    while i < b.len() {
        if b[i] == '\\' && b.get(i + 1) == Some(&'\n') {
            i += 2;
            while i < b.len() && (b[i] == ' ' || b[i] == '\t') {
                i += 1;
            }
            continue;
        }
        cut.push(b[i]);
        i += 1;
    }
    let s: Vec<char> = fold(&cut).chars().collect();
    // JS は UTF-16 で数えるので、\x などの桁数は UTF-16 で取る。ここでは桁は ASCII なので同じ。
    let mut out = String::new();
    let mut i = 0;
    while i < s.len() {
        let ch = s[i];
        if ch != '\\' {
            out.push(ch);
            i += 1;
            continue;
        }
        i += 1;
        let e = s.get(i).copied();
        match e {
            Some(e @ ('x' | 'u' | 'U')) => {
                let len = if e == 'x' {
                    2
                } else if e == 'u' {
                    4
                } else {
                    8
                };
                let digits: String = s[(i + 1).min(s.len())..(i + 1 + len).min(s.len())]
                    .iter()
                    .collect();
                let v = js::parse_int(&digits, 16);
                // String.fromCodePoint は範囲外で RangeError を投げる。
                match (v.is_finite() && v >= 0.0 && v <= 0x10ffff as f64).then_some(v as u32) {
                    Some(cp) => match char::from_u32(cp) {
                        Some(c) => out.push(c),
                        None => out.push('\u{fffd}'),
                    },
                    None => return err(&format!("Invalid code point {}", js::fmt(v))),
                }
                i += len;
            }
            Some(e) if escape(e).is_some() => out.push_str(escape(e).unwrap()),
            _ => return err("YAML: unknown escape sequence"),
        }
        i += 1;
    }
    Ok(out)
}

fn skip(s: &[char], mut pos: usize, commas: bool) -> usize {
    while pos < s.len() {
        let c = s[pos];
        if c == ' ' || c == '\n' || c == '\t' || c == '\r' || (commas && c == ',') {
            pos += 1;
        } else if c == '#' && (pos == 0 || js::is_js_space(s[pos - 1])) {
            pos = index_of(s, '\n', pos).unwrap_or(s.len());
        } else {
            break;
        }
    }
    pos
}

fn entry_start(ch: char, separated: bool) -> R<()> {
    if !separated {
        return err("YAML: missed comma between flow collection entries");
    }
    if ch == ',' {
        return err("YAML: expected the node content, but found ','");
    }
    Ok(())
}

fn incomplete<T>(m: &str) -> R<T> {
    Err(YamlError::Incomplete(m.to_string()))
}

/// `s[pos]` から始まるフローの値を読み、値とその後ろの位置を返す。
fn flow(s: &[char], pos: usize, stops: &str, depth: usize) -> R<(Value, usize)> {
    if depth > MAX_DEPTH {
        return err("YAML: nesting is too deep");
    }
    let mut pos = skip(s, pos, false);
    let ch = s.get(pos).copied();
    if ch == Some('{') {
        let mut out = Map::new();
        pos += 1;
        let mut separated = true;
        loop {
            pos = skip(s, pos, false);
            if pos >= s.len() {
                return incomplete("YAML: unexpected end of the stream within a flow collection");
            }
            if s[pos] == '}' {
                return Ok((Value::Obj(out), pos + 1));
            }
            entry_start(s[pos], separated)?;
            let (key, p) = flow(s, pos, ":,}", depth + 1)?;
            pos = skip(s, p, false);
            let mut value = Value::Null;
            if s.get(pos) == Some(&':') {
                let (v, p) = flow(s, pos + 1, ",}", depth + 1)?;
                value = v;
                pos = p;
            }
            out.set(&key.to_js_string(), value);
            pos = skip(s, pos, false);
            separated = s.get(pos) == Some(&',');
            if separated {
                pos += 1;
            }
        }
    }
    if ch == Some('[') {
        let mut out = Vec::new();
        pos += 1;
        let mut separated = true;
        loop {
            pos = skip(s, pos, false);
            if pos >= s.len() {
                return incomplete("YAML: unexpected end of the stream within a flow collection");
            }
            if s[pos] == ']' {
                return Ok((Value::Arr(out), pos + 1));
            }
            entry_start(s[pos], separated)?;
            let (value, p) = flow(s, pos, ",]", depth + 1)?;
            out.push(value);
            pos = skip(s, p, false);
            separated = s.get(pos) == Some(&',');
            if separated {
                pos += 1;
            }
        }
    }
    if ch == Some('"') {
        let mut end = pos + 1;
        while end < s.len() && s[end] != '"' {
            end += if s[end] == '\\' { 2 } else { 1 };
        }
        if end >= s.len() {
            return incomplete("YAML: unexpected end of the stream within a double quoted scalar");
        }
        return Ok((
            Value::Str(unquote_double(&s_of(&s[pos + 1..end]))?),
            end + 1,
        ));
    }
    if ch == Some('\'') {
        let mut end = pos + 1;
        while end < s.len() {
            if s[end] == '\'' {
                if s.get(end + 1) != Some(&'\'') {
                    break;
                }
                end += 1;
            }
            end += 1;
        }
        if end >= s.len() {
            return incomplete("YAML: unexpected end of the stream within a single quoted scalar");
        }
        return Ok((
            Value::Str(fold(&s_of(&s[pos + 1..end])).replace("''", "'")),
            end + 1,
        ));
    }
    let mut end = pos;
    while end < s.len() {
        let c = s[end];
        if stops.contains(c)
            && (c != ':'
                || end + 1 >= s.len()
                || js::is_js_space(s[end + 1])
                || matches!(s[end + 1], ',' | '[' | ']' | '{' | '}'))
        {
            break;
        }
        if c == '#' && end > pos && js::is_js_space(s[end - 1]) {
            let nl = index_of(s, '\n', end);
            return Ok((
                scalar(&fold(js::trim(&s_of(&s[pos..end])))),
                nl.unwrap_or(s.len()),
            ));
        }
        end += 1;
    }
    Ok((
        scalar(&fold(js::trim(&s_of(&s[pos.min(s.len())..end])))),
        end,
    ))
}

fn strip_comment(s: &[char]) -> &[char] {
    let mut quote: Option<char> = None;
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if let Some(q) = quote {
            if c == '\\' && q == '"' {
                i += 1;
            } else if c == q {
                quote = None;
            }
        } else if c == '"' || c == '\'' {
            quote = Some(c);
        } else if c == '#' && (i == 0 || s[i - 1] == ' ' || s[i - 1] == '\t') {
            return &s[..i];
        }
        i += 1;
    }
    s
}

fn block_scalar(r: &mut Reader, header: &[char], parent_indent: isize) -> String {
    let folded = header[0] == '>';
    let chomp = if header.contains(&'-') {
        '-'
    } else if header.contains(&'+') {
        '+'
    } else {
        ' '
    };
    let explicit = header.iter().find(|c| ('1'..='9').contains(*c));
    let mut indent: isize = match explicit {
        Some(c) => parent_indent + c.to_digit(10).unwrap() as isize,
        None => -1,
    };
    let mut out: Vec<Chars> = Vec::new();
    while r.i < r.lines.len() {
        let line = r.lines[r.i].clone();
        if trim_chars(&line).is_empty() {
            out.push(Vec::new());
            r.i += 1;
            continue;
        }
        let k = (line.len() - trim_start_len(&line)) as isize;
        if indent == -1 {
            if k <= parent_indent {
                break;
            }
            indent = k;
        }
        if k < indent {
            break;
        }
        out.push(line[indent as usize..].to_vec());
        r.i += 1;
    }
    let mut trailing = 0;
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
        trailing += 1;
    }
    let text = if folded {
        let mut text = String::new();
        for i in 0..out.len() {
            let cur = &out[i];
            if i > 0 {
                let prev = &out[i - 1];
                if cur.is_empty() {
                    text.push('\n');
                } else if !prev.is_empty() {
                    let lead = |l: &Chars| matches!(l.first(), Some(' ' | '\t'));
                    text.push(if lead(prev) || lead(cur) { '\n' } else { ' ' });
                }
            }
            text.push_str(&s_of(cur));
        }
        text
    } else {
        out.iter().map(|l| s_of(l)).collect::<Vec<_>>().join("\n")
    };
    if out.is_empty() {
        return if chomp == '+' {
            "\n".repeat(trailing)
        } else {
            String::new()
        };
    }
    match chomp {
        '-' => text,
        '+' => text + &"\n".repeat(trailing + 1),
        _ => text + "\n",
    }
}

/// `/^[|>][-+0-9]*\s*(?:#.*)?$/`
fn block_header(rest: &[char]) -> bool {
    let mut i = 1;
    while i < rest.len() && (rest[i] == '-' || rest[i] == '+' || rest[i].is_ascii_digit()) {
        i += 1;
    }
    while i < rest.len() && js::is_js_space(rest[i]) {
        i += 1;
    }
    if i == rest.len() {
        return true;
    }
    if rest[i] != '#' {
        return false;
    }
    // `.` は行の終わりの文字に合わない。
    !rest[i..]
        .iter()
        .any(|&c| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
}

fn inline_value(r: &mut Reader, rest: &[char], indent: isize) -> R<Value> {
    let first = rest.first().copied();
    if (first == Some('|') || first == Some('>')) && block_header(rest) {
        return Ok(Value::Str(block_scalar(r, rest, indent)));
    }
    if matches!(first, Some('{' | '[' | '"' | '\'')) {
        // 値は後の行に続くことがある。閉じるまで、だんだん多くの行を足して読む。
        let start = r.i;
        let mut text: Chars = rest.to_vec();
        let mut taken = 0;
        let mut batch = 1;
        loop {
            match flow(&text, 0, "", 0) {
                Ok((value, end)) => {
                    let mut used = 0;
                    let mut k = index_of(&text, '\n', 0);
                    while let Some(kk) = k {
                        if kk >= end {
                            break;
                        }
                        used += 1;
                        k = index_of(&text, '\n', kk + 1);
                    }
                    let line_end = index_of(&text, '\n', end).unwrap_or(text.len());
                    if !trim_chars(strip_comment(&text[end.min(line_end)..line_end])).is_empty() {
                        return err("YAML: unexpected content after a value");
                    }
                    r.i = start + used;
                    return Ok(value);
                }
                Err(YamlError::Incomplete(m)) => {
                    if start + taken >= r.lines.len() {
                        return Err(YamlError::Error(m));
                    }
                    let to = (start + taken + batch).min(r.lines.len());
                    let more = &r.lines[start + taken..to];
                    for l in more {
                        text.push('\n');
                        text.extend_from_slice(l);
                    }
                    if more.is_empty() {
                        text.push('\n');
                    }
                    taken += more.len();
                    batch *= 2;
                }
                Err(e) => return Err(e),
            }
        }
    }
    let mut text = s_of(trim_chars(strip_comment(rest)));
    while r.i < r.lines.len() {
        let line = r.lines[r.i].clone();
        if trim_chars(&line).is_empty() {
            let mut j = r.i;
            while j < r.lines.len() && trim_chars(&r.lines[j]).is_empty() {
                j += 1;
            }
            if j >= r.lines.len()
                || (r.lines[j].len() - trim_start_len(&r.lines[j])) as isize <= indent
            {
                break;
            }
            text.push_str(&"\n".repeat(j - r.i));
            r.i = j;
            continue;
        }
        if ((line.len() - trim_start_len(&line)) as isize) <= indent {
            break;
        }
        let more = s_of(trim_chars(strip_comment(&line)));
        if !text.ends_with('\n') {
            text.push(' ');
        }
        text.push_str(&more);
        r.i += 1;
    }
    Ok(scalar(&text))
}

fn is_seq(line: &[char]) -> bool {
    line == ['-'] || line.starts_with(&['-', ' '])
}

fn block(r: &mut Reader, min_indent: usize, depth: usize) -> R<Value> {
    if depth > MAX_DEPTH {
        return err("YAML: nesting is too deep");
    }
    r.skip_blank();
    if r.i >= r.lines.len() {
        return Ok(Value::Null);
    }
    let indent = r.indent()?;
    if indent < min_indent {
        return Ok(Value::Null);
    }
    let line: Chars = r.lines[r.i][indent..].to_vec();

    if is_seq(&line) {
        let mut out = Vec::new();
        while r.i < r.lines.len() {
            r.skip_blank();
            if r.i >= r.lines.len() || r.indent()? != indent {
                break;
            }
            let cur: Chars = r.lines[r.i][indent..].to_vec();
            if !is_seq(&cur) {
                break;
            }
            let rest = &cur[1..];
            let pad = rest.len() - trim_start_len(rest) + 1;
            let body = trim_chars(strip_comment(rest));
            if body.is_empty() {
                r.i += 1;
                out.push(block(r, indent + 1, depth + 1)?);
            } else if split_key(body).is_some() {
                let mut l: Chars = vec![' '; indent + pad];
                l.extend_from_slice(&rest[rest.len() - trim_start_len(rest)..]);
                r.lines[r.i] = l;
                out.push(block(r, indent + pad, depth + 1)?);
            } else {
                r.i += 1;
                out.push(inline_value(r, trim_chars(rest), indent as isize)?);
            }
        }
        return Ok(Value::Arr(out));
    }

    if split_key(&line).is_none() {
        r.i += 1;
        return inline_value(r, trim_chars(&line), indent as isize - 1);
    }

    let mut out = Map::new();
    while r.i < r.lines.len() {
        r.skip_blank();
        if r.i >= r.lines.len() {
            break;
        }
        let k = r.indent()?;
        if k < indent {
            break;
        }
        if k > indent {
            return err("YAML: bad indentation of a mapping entry");
        }
        let cur: Chars = r.lines[r.i][indent..].to_vec();
        let Some(km) = split_key(&cur) else {
            return err("YAML: can not read a block mapping entry");
        };
        let key = flow(&km.key, 0, "", 0)?.0.to_js_string();
        let rest: Chars = cur[km.length.min(cur.len())..].to_vec();
        r.i += 1;
        if trim_chars(strip_comment(&rest)).is_empty() {
            r.skip_blank();
            if r.i < r.lines.len() {
                let next = r.indent()?;
                let next_line = &r.lines[r.i][next..];
                let seq = is_seq(next_line);
                let v = if next > indent || (seq && next == indent) {
                    block(r, if seq { indent } else { indent + 1 }, depth + 1)?
                } else {
                    Value::Null
                };
                out.set(&key, v);
            } else {
                out.set(&key, Value::Null);
            }
        } else {
            let v = inline_value(r, trim_chars(&rest), indent as isize)?;
            out.set(&key, v);
        }
    }
    Ok(Value::Obj(out))
}

/// `parseYaml`。読めなければ js-yaml の言い方の知らせ。
pub fn parse_yaml(src: &str) -> std::result::Result<Value, String> {
    let trimmed: Chars = js::trim(src).chars().collect();
    if matches!(trimmed.first(), Some('{' | '[')) {
        match flow(&trimmed, 0, "", 0) {
            Ok((value, end)) => {
                if skip(&trimmed, end, false) == trimmed.len() {
                    return Ok(value);
                }
            }
            Err(e) => return Err(e.message().to_string()),
        }
    }
    let mut r = Reader::new(src);
    let value = block(&mut r, 0, 0).map_err(|e| e.message().to_string())?;
    r.skip_blank();
    if r.i < r.lines.len() && trim_chars(&r.lines[r.i]) != ['.', '.', '.'] {
        return Err("YAML: end of the stream or a document separator is expected".into());
    }
    Ok(value)
}

// --- JSON（`JSON.parse`） ---

/// `JSON.parse`。読めなければ None（pele は読めない init を無視する）。
pub fn parse_json(src: &str) -> Option<Value> {
    let s: Vec<char> = src.chars().collect();
    let mut p = 0;
    let v = json_value(&s, &mut p, 0)?;
    json_ws(&s, &mut p);
    (p == s.len()).then_some(v)
}

fn json_ws(s: &[char], p: &mut usize) {
    while *p < s.len() && matches!(s[*p], ' ' | '\t' | '\n' | '\r') {
        *p += 1;
    }
}

fn json_value(s: &[char], p: &mut usize, depth: usize) -> Option<Value> {
    if depth > 512 {
        return None;
    }
    json_ws(s, p);
    let c = *s.get(*p)?;
    match c {
        '{' => {
            *p += 1;
            let mut m = Map::new();
            json_ws(s, p);
            if s.get(*p) == Some(&'}') {
                *p += 1;
                return Some(Value::Obj(m));
            }
            loop {
                json_ws(s, p);
                let Value::Str(k) = json_string(s, p)? else {
                    return None;
                };
                json_ws(s, p);
                if s.get(*p) != Some(&':') {
                    return None;
                }
                *p += 1;
                let v = json_value(s, p, depth + 1)?;
                m.set(&k, v);
                json_ws(s, p);
                match s.get(*p) {
                    Some(',') => *p += 1,
                    Some('}') => {
                        *p += 1;
                        return Some(Value::Obj(m));
                    }
                    _ => return None,
                }
            }
        }
        '[' => {
            *p += 1;
            let mut a = Vec::new();
            json_ws(s, p);
            if s.get(*p) == Some(&']') {
                *p += 1;
                return Some(Value::Arr(a));
            }
            loop {
                a.push(json_value(s, p, depth + 1)?);
                json_ws(s, p);
                match s.get(*p) {
                    Some(',') => *p += 1,
                    Some(']') => {
                        *p += 1;
                        return Some(Value::Arr(a));
                    }
                    _ => return None,
                }
            }
        }
        '"' => json_string(s, p),
        't' | 'f' | 'n' => {
            for (word, v) in [
                ("true", Value::Bool(true)),
                ("false", Value::Bool(false)),
                ("null", Value::Null),
            ] {
                let w: Vec<char> = word.chars().collect();
                if s[*p..].starts_with(&w) {
                    *p += w.len();
                    return Some(v);
                }
            }
            None
        }
        _ => {
            let start = *p;
            if s.get(*p) == Some(&'-') {
                *p += 1;
            }
            match s.get(*p) {
                Some('0') => *p += 1,
                Some(c) if c.is_ascii_digit() => {
                    while s.get(*p).is_some_and(|c| c.is_ascii_digit()) {
                        *p += 1;
                    }
                }
                _ => return None,
            }
            if s.get(*p) == Some(&'.') {
                *p += 1;
                if !s.get(*p).is_some_and(|c| c.is_ascii_digit()) {
                    return None;
                }
                while s.get(*p).is_some_and(|c| c.is_ascii_digit()) {
                    *p += 1;
                }
            }
            if matches!(s.get(*p), Some('e' | 'E')) {
                *p += 1;
                if matches!(s.get(*p), Some('+' | '-')) {
                    *p += 1;
                }
                if !s.get(*p).is_some_and(|c| c.is_ascii_digit()) {
                    return None;
                }
                while s.get(*p).is_some_and(|c| c.is_ascii_digit()) {
                    *p += 1;
                }
            }
            Some(Value::Num(js::number(&s_of(&s[start..*p]))))
        }
    }
}

fn json_string(s: &[char], p: &mut usize) -> Option<Value> {
    if s.get(*p) != Some(&'"') {
        return None;
    }
    *p += 1;
    let mut units: Vec<u16> = Vec::new();
    loop {
        let c = *s.get(*p)?;
        *p += 1;
        match c {
            '"' => return Some(Value::Str(js::from16(&units))),
            '\\' => {
                let e = *s.get(*p)?;
                *p += 1;
                let ch = match e {
                    '"' => '"',
                    '\\' => '\\',
                    '/' => '/',
                    'b' => '\x08',
                    'f' => '\x0c',
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    'u' => {
                        let hex: String = s.get(*p..*p + 4)?.iter().collect();
                        if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                            return None;
                        }
                        *p += 4;
                        units.push(u16::from_str_radix(&hex, 16).ok()?);
                        continue;
                    }
                    _ => return None,
                };
                let mut b = [0u16; 2];
                units.extend_from_slice(ch.encode_utf16(&mut b));
            }
            c if (c as u32) < 0x20 => return None,
            c => {
                let mut b = [0u16; 2];
                units.extend_from_slice(c.encode_utf16(&mut b));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_front_matter() {
        let v = parse_yaml("title: Hello\nconfig:\n  flowchart:\n    curve: linear\n  n: 12\n")
            .unwrap();
        let m = v.as_obj().unwrap();
        assert_eq!(m.get("title"), Some(&Value::Str("Hello".into())));
        let c = m.get("config").unwrap().as_obj().unwrap();
        assert_eq!(c.get("n"), Some(&Value::Num(12.0)));
        let v = parse_yaml("{ shape: rect, label: \"a b\" }").unwrap();
        assert_eq!(
            v.as_obj().unwrap().get("label"),
            Some(&Value::Str("a b".into()))
        );
    }
}
