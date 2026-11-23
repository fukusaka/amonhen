//! `json` のノードの中身（pele の `src/diagrams/usecase/json.ts`）。TS は `JSON.parse` を使い、その知らせの文を
//! そのまま見せるので、ここでは V8 の `JSON.parse` の受け付けるものと知らせ（文と位置）を写した読み手を持つ。

use std::collections::HashMap;

use crate::errors::{ErrorCode, PeleError};
use crate::js;
use crate::text::entities::written_codes;
use crate::util::chars::is_digit;

/// JSON の値。オブジェクトは JS と同じく、同じ鍵を書き直すと値だけが替わり、場所は最初のまま。
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(JsonObject),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct JsonObject {
    pub entries: Vec<(String, Json)>,
    index: HashMap<String, usize>,
}

impl JsonObject {
    fn set(&mut self, key: String, value: Json) {
        match self.index.get(&key) {
            Some(&i) => self.entries[i].1 = value,
            None => {
                self.index.insert(key.clone(), self.entries.len());
                self.entries.push((key, value));
            }
        }
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        self.index.get(key).map(|&i| &self.entries[i].1)
    }

    /// `Object.keys`: 配列の位置のような鍵を小さい順に、その後にほかの鍵を入れた順に。
    pub fn keys(&self) -> Vec<String> {
        let index_like = |k: &str| -> Option<u64> {
            if k.is_empty()
                || (k.len() > 1 && k.starts_with('0'))
                || !k.bytes().all(|b| b.is_ascii_digit())
            {
                return None;
            }
            k.parse::<u64>().ok().filter(|&v| v < 4294967295)
        };
        let mut numeric: Vec<(u64, &str)> = Vec::new();
        let mut other: Vec<&str> = Vec::new();
        for (k, _) in &self.entries {
            match index_like(k) {
                Some(v) => numeric.push((v, k)),
                None => other.push(k),
            }
        }
        numeric.sort_by_key(|&(v, _)| v);
        numeric
            .into_iter()
            .map(|(_, k)| k.to_string())
            .chain(other.into_iter().map(str::to_string))
            .collect()
    }
}

pub struct OrderedJson {
    pub value: Json,
    pub property_order: HashMap<String, Vec<String>>,
}

// ---- V8 の JSON.parse ----

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tok {
    Str,
    Number,
    LBrace,
    RBrace,
    LBrack,
    RBrack,
    True,
    False,
    Null,
    Colon,
    Comma,
    Whitespace,
    Illegal,
    Eos,
}

fn token_of(c: u16) -> Tok {
    match c {
        0x22 => Tok::Str,
        0x2d | 0x30..=0x39 => Tok::Number,
        0x7b => Tok::LBrace,
        0x7d => Tok::RBrace,
        0x5b => Tok::LBrack,
        0x5d => Tok::RBrack,
        0x74 => Tok::True,
        0x66 => Tok::False,
        0x6e => Tok::Null,
        0x3a => Tok::Colon,
        0x2c => Tok::Comma,
        0x20 | 0x09 | 0x0a | 0x0d => Tok::Whitespace,
        _ => Tok::Illegal,
    }
}

/// V8 の知らせの型。
#[derive(Clone, Copy)]
enum Msg {
    ExpectPropNameOrRBrace,
    ExpectCommaOrRBrack,
    ExpectCommaOrRBrace,
    ExpectColon,
    ExpectDoubleQuotedPropertyName,
    ExponentMissingNumber,
    UnterminatedFraction,
    NoNumberAfterMinus,
    BadControlCharacter,
    BadEscapedCharacter,
    BadUnicodeEscape,
    UnterminatedString,
    NonWhitespaceAfter,
}

impl Msg {
    fn text(self) -> &'static str {
        match self {
            Msg::ExpectPropNameOrRBrace => "Expected property name or '}'",
            Msg::ExpectCommaOrRBrack => "Expected ',' or ']' after array element",
            Msg::ExpectCommaOrRBrace => "Expected ',' or '}' after property value",
            Msg::ExpectColon => "Expected ':' after property name",
            Msg::ExpectDoubleQuotedPropertyName => "Expected double-quoted property name",
            Msg::ExponentMissingNumber => "Exponent part is missing a number",
            Msg::UnterminatedFraction => "Unterminated fractional number",
            Msg::NoNumberAfterMinus => "No number after minus sign",
            Msg::BadControlCharacter => "Bad control character in string literal",
            Msg::BadEscapedCharacter => "Bad escaped character",
            Msg::BadUnicodeEscape => "Bad Unicode escape",
            Msg::UnterminatedString => "Unterminated string",
            Msg::NonWhitespaceAfter => "Unexpected non-whitespace character after JSON",
        }
    }
}

const MAX_CONTEXT: usize = 10;
const MIN_LENGTH_FOR_CONTEXT: usize = 2 * MAX_CONTEXT + 1;

struct V8Parser<'a> {
    s: &'a [u16],
    at: usize,
}

type V8Result<T> = std::result::Result<T, String>;

impl V8Parser<'_> {
    fn peek_char(&self) -> Option<u16> {
        self.s.get(self.at).copied()
    }

    fn skip_ws(&mut self) {
        while let Some(c) = self.peek_char() {
            if c == 0x20 || c == 0x09 || c == 0x0a || c == 0x0d {
                self.at += 1;
            } else {
                break;
            }
        }
    }

    fn peek(&mut self) -> Tok {
        self.skip_ws();
        match self.peek_char() {
            Some(c) => token_of(c),
            None => Tok::Eos,
        }
    }

    /// V8 の `CalculateFileLocation`: \r\n は 1 つの改行。
    fn line_column(&self) -> (usize, usize) {
        let mut line = 1;
        let mut last_break = 0;
        let mut i = 0;
        let end = self.at;
        while i < end {
            if self.s[i] == 13 && i + 1 < end && self.s[i + 1] == 10 {
                i += 1;
            }
            if self.s[i] == 13 || self.s[i] == 10 {
                line += 1;
                last_break = i + 1;
            }
            i += 1;
        }
        (line, 1 + i - last_break)
    }

    fn report(&self, token: Tok, message: Option<Msg>) -> String {
        let pos = self.at;
        let (line, column) = self.line_column();
        let place = format!("at position {pos} (line {line} column {column})");
        let at_pos = format!("in JSON {place}");
        if let Some(m) = message {
            if let Msg::NonWhitespaceAfter = m {
                return format!("{} {place}", m.text());
            }
            return format!("{} {at_pos}", m.text());
        }
        match token {
            Tok::Eos => "Unexpected end of JSON input".to_string(),
            Tok::Number => format!("Unexpected number {at_pos}"),
            Tok::Str => format!("Unexpected string {at_pos}"),
            _ => {
                let ch = js::from16(&self.s[pos..pos + 1]);
                let len = self.s.len();
                if len < MIN_LENGTH_FOR_CONTEXT {
                    format!(
                        "Unexpected token '{ch}', \"{}\" is not valid JSON",
                        js::from16(self.s)
                    )
                } else if pos < MAX_CONTEXT {
                    format!(
                        "Unexpected token '{ch}', \"{}\"... is not valid JSON",
                        js::from16(&self.s[..pos + MAX_CONTEXT])
                    )
                } else if pos < len - MAX_CONTEXT {
                    format!(
                        "Unexpected token '{ch}', ...\"{}\"... is not valid JSON",
                        js::from16(&self.s[pos - MAX_CONTEXT..pos + MAX_CONTEXT])
                    )
                } else {
                    format!(
                        "Unexpected token '{ch}', ...\"{}\" is not valid JSON",
                        js::from16(&self.s[pos - MAX_CONTEXT..])
                    )
                }
            }
        }
    }

    fn fail<T>(&self, token: Tok, message: Option<Msg>) -> V8Result<T> {
        Err(self.report(token, message))
    }

    fn char_token(&self) -> Tok {
        match self.peek_char() {
            Some(c) => token_of(c),
            None => Tok::Eos,
        }
    }

    fn literal(&mut self, word: &str, value: Json) -> V8Result<Json> {
        let w: Vec<u16> = word.encode_utf16().collect();
        self.at += 1;
        for &c in &w[1..] {
            match self.peek_char() {
                Some(x) if x == c => self.at += 1,
                _ => return self.fail(self.char_token(), None),
            }
        }
        Ok(value)
    }

    fn string(&mut self) -> V8Result<String> {
        self.at += 1;
        let mut out: Vec<u16> = Vec::new();
        loop {
            let Some(c) = self.peek_char() else {
                return self.fail(Tok::Illegal, Some(Msg::UnterminatedString));
            };
            if c == 0x22 {
                self.at += 1;
                return Ok(js::from16(&out));
            }
            if c < 0x20 {
                return self.fail(Tok::Illegal, Some(Msg::BadControlCharacter));
            }
            if c != 0x5c {
                out.push(c);
                self.at += 1;
                continue;
            }
            self.at += 1;
            let Some(e) = self.peek_char() else {
                return self.fail(Tok::Eos, None);
            };
            let decoded = match e {
                0x22 | 0x5c | 0x2f => e,
                0x62 => 8,
                0x66 => 12,
                0x6e => 10,
                0x72 => 13,
                0x74 => 9,
                0x75 => {
                    let mut v: u16 = 0;
                    for _ in 0..4 {
                        self.at += 1;
                        let d = self
                            .peek_char()
                            .and_then(|h| char::from_u32(h as u32)?.to_digit(16));
                        match d {
                            Some(d) => v = v * 16 + d as u16,
                            None => return self.fail(Tok::Illegal, Some(Msg::BadUnicodeEscape)),
                        }
                    }
                    v
                }
                _ => return self.fail(Tok::Illegal, Some(Msg::BadEscapedCharacter)),
            };
            out.push(decoded);
            self.at += 1;
        }
    }

    fn digits(&mut self) {
        while self.peek_char().is_some_and(|c| is_digit(c as u32)) {
            self.at += 1;
        }
    }

    fn number(&mut self) -> V8Result<Json> {
        let start = self.at;
        if self.peek_char() == Some(0x2d) {
            self.at += 1;
        }
        match self.peek_char() {
            Some(0x30) => {
                self.at += 1;
                if self.peek_char().is_some_and(|c| is_digit(c as u32)) {
                    return self.fail(Tok::Number, None);
                }
            }
            Some(c) if is_digit(c as u32) => self.digits(),
            _ => return self.fail(Tok::Illegal, Some(Msg::NoNumberAfterMinus)),
        }
        if self.peek_char() == Some(0x2e) {
            self.at += 1;
            if !self.peek_char().is_some_and(|c| is_digit(c as u32)) {
                return self.fail(Tok::Illegal, Some(Msg::UnterminatedFraction));
            }
            self.digits();
        }
        if self.peek_char().is_some_and(|c| c | 32 == 0x65) {
            self.at += 1;
            if matches!(self.peek_char(), Some(0x2b | 0x2d)) {
                self.at += 1;
            }
            if !self.peek_char().is_some_and(|c| is_digit(c as u32)) {
                return self.fail(Tok::Illegal, Some(Msg::ExponentMissingNumber));
            }
            self.digits();
        }
        let text = js::from16(&self.s[start..self.at]);
        Ok(Json::Num(text.parse::<f64>().unwrap_or(f64::NAN)))
    }

    /// 入れ子は自前の積みで持ち、深さは呼び出しの積みでなくメモリで決まる。
    fn value(&mut self) -> V8Result<Json> {
        enum Frame {
            Arr(Vec<Json>),
            Obj(JsonObject, String),
        }
        let mut stack: Vec<Frame> = Vec::new();
        loop {
            // 値を 1 つ読む（入れ子を開くなら積んで次へ）。
            let token = self.peek();
            let mut done: Json = match token {
                Tok::Str => Json::Str(self.string()?),
                Tok::Number => self.number()?,
                Tok::True => self.literal("true", Json::Bool(true))?,
                Tok::False => self.literal("false", Json::Bool(false))?,
                Tok::Null => self.literal("null", Json::Null)?,
                Tok::LBrace => {
                    self.at += 1;
                    match self.peek() {
                        Tok::RBrace => {
                            self.at += 1;
                            Json::Obj(JsonObject::default())
                        }
                        Tok::Str => {
                            let key = self.string()?;
                            if self.peek() != Tok::Colon {
                                return self.fail(Tok::Illegal, Some(Msg::ExpectColon));
                            }
                            self.at += 1;
                            stack.push(Frame::Obj(JsonObject::default(), key));
                            continue;
                        }
                        _ => return self.fail(Tok::Illegal, Some(Msg::ExpectPropNameOrRBrace)),
                    }
                }
                Tok::LBrack => {
                    self.at += 1;
                    if self.peek() == Tok::RBrack {
                        self.at += 1;
                        Json::Arr(Vec::new())
                    } else {
                        stack.push(Frame::Arr(Vec::new()));
                        continue;
                    }
                }
                other => return self.fail(other, None),
            };
            // 読んだ値を親に入れ、閉じた入れ子は値として上へ。
            loop {
                match stack.pop() {
                    None => return Ok(done),
                    Some(Frame::Arr(mut items)) => {
                        items.push(done);
                        match self.peek() {
                            Tok::Comma => {
                                self.at += 1;
                                stack.push(Frame::Arr(items));
                                break;
                            }
                            Tok::RBrack => {
                                self.at += 1;
                                done = Json::Arr(items);
                            }
                            _ => return self.fail(Tok::Illegal, Some(Msg::ExpectCommaOrRBrack)),
                        }
                    }
                    Some(Frame::Obj(mut object, key)) => {
                        object.set(key, done);
                        match self.peek() {
                            Tok::Comma => {
                                self.at += 1;
                                if self.peek() != Tok::Str {
                                    return self.fail(
                                        Tok::Illegal,
                                        Some(Msg::ExpectDoubleQuotedPropertyName),
                                    );
                                }
                                let key = self.string()?;
                                if self.peek() != Tok::Colon {
                                    return self.fail(Tok::Illegal, Some(Msg::ExpectColon));
                                }
                                self.at += 1;
                                stack.push(Frame::Obj(object, key));
                                break;
                            }
                            Tok::RBrace => {
                                self.at += 1;
                                done = Json::Obj(object);
                            }
                            _ => return self.fail(Tok::Illegal, Some(Msg::ExpectCommaOrRBrace)),
                        }
                    }
                }
            }
        }
    }
}

/// `JSON.parse`: 値か、V8 の知らせの文。
pub fn json_parse(text: &[u16]) -> std::result::Result<Json, String> {
    let mut p = V8Parser { s: text, at: 0 };
    let value = p.value()?;
    if p.peek() != Tok::Eos {
        return p.fail(Tok::Illegal, Some(Msg::NonWhitespaceAfter));
    }
    Ok(value)
}

// ---- 鍵の順（pele の propertyOrder） ----

struct Frame {
    object: bool,
    pointer: String,
    keys_at: String,
    ranges: HashMap<String, (usize, usize)>,
    key: String,
    from: usize,
    index: usize,
}

#[derive(PartialEq, Eq)]
enum State {
    Value,
    Key,
    After,
}

/// `key.replaceAll('~', '~0').replaceAll('/', '~1')`
fn escape_pointer(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

/// JSON.parse が受け付けた文を歩き、鍵を書いた順を取り戻す（整数のような鍵の順を JSON.parse は失う）。
/// JSON.parse が断った文では、どこが違うかを見つける（`Err` は最初の JSON でない文字の位置）。
fn property_order(text: &[u16]) -> std::result::Result<HashMap<String, Vec<String>>, usize> {
    let mut order: HashMap<String, Vec<String>> = HashMap::new();
    let mut created: Vec<String> = Vec::new();
    let mut stack: Vec<Frame> = Vec::new();
    let n = text.len();
    let mut at = 0usize;
    let mut pointer = String::new();
    let mut state = State::Value;
    let ch = |i: usize| -> u32 { text.get(i).map_or(u32::MAX, |&c| c as u32) };

    let skip = |at: &mut usize| {
        while *at < n {
            let c = text[*at];
            if c != 32 && c != 9 && c != 10 && c != 13 {
                return;
            }
            *at += 1;
        }
    };

    let read_string = |at: &mut usize, decode: bool| -> std::result::Result<String, usize> {
        *at += 1;
        let mut value: Vec<u16> = Vec::new();
        while *at < n {
            let start = *at;
            let c = text[*at];
            *at += 1;
            if c == 34 {
                return Ok(js::from16(&value));
            }
            if c < 0x20 {
                return Err(start);
            }
            if c != 92 {
                if decode {
                    value.push(c);
                }
                continue;
            }
            let escape_at = *at;
            let escape = text.get(*at).copied();
            *at += 1;
            let decoded: u16 = match escape {
                Some(e @ (34 | 92 | 47)) => e,
                Some(98) => 8,
                Some(102) => 12,
                Some(110) => 10,
                Some(114) => 13,
                Some(116) => 9,
                Some(117) => {
                    let unit = &text[(*at).min(n)..(*at + 4).min(n)];
                    let ok = unit.len() == 4
                        && unit.iter().all(|&h| {
                            char::from_u32(h as u32).is_some_and(|c| c.is_ascii_hexdigit())
                        });
                    if !ok {
                        return Err(*at);
                    }
                    let v = unit.iter().fold(0u16, |v, &h| {
                        v * 16 + char::from_u32(h as u32).unwrap().to_digit(16).unwrap() as u16
                    });
                    *at += 4;
                    v
                }
                _ => return Err(escape_at),
            };
            if decode {
                value.push(decoded);
            }
        }
        Err(*at)
    };

    let literal = |at: &mut usize, word: &str| -> std::result::Result<(), usize> {
        for (k, b) in word.bytes().enumerate() {
            if ch(*at + k) != b as u32 {
                return Err(*at + k);
            }
        }
        *at += word.len();
        Ok(())
    };

    let number = |at: &mut usize| -> std::result::Result<(), usize> {
        if ch(*at) == 45 {
            *at += 1;
        }
        if ch(*at) == 48 {
            *at += 1;
        } else if is_digit(ch(*at)) {
            while is_digit(ch(*at)) {
                *at += 1;
            }
        } else {
            return Err(*at);
        }
        if ch(*at) == 46 {
            *at += 1;
            if !is_digit(ch(*at)) {
                return Err(*at);
            }
            while is_digit(ch(*at)) {
                *at += 1;
            }
        }
        if ch(*at) | 32 == 101 {
            *at += 1;
            if ch(*at) == 43 || ch(*at) == 45 {
                *at += 1;
            }
            if !is_digit(ch(*at)) {
                return Err(*at);
            }
            while is_digit(ch(*at)) {
                *at += 1;
            }
        }
        Ok(())
    };

    skip(&mut at);
    loop {
        match state {
            State::Value => {
                skip(&mut at);
                let c = ch(at);
                state = State::After;
                if c == 123 {
                    at += 1;
                    order.insert(pointer.clone(), Vec::new());
                    created.push(pointer.clone());
                    skip(&mut at);
                    if ch(at) == 125 {
                        at += 1;
                    } else {
                        stack.push(Frame {
                            object: true,
                            pointer: pointer.clone(),
                            keys_at: pointer.clone(),
                            ranges: HashMap::new(),
                            key: String::new(),
                            from: 0,
                            index: 0,
                        });
                        state = State::Key;
                    }
                } else if c == 91 {
                    at += 1;
                    skip(&mut at);
                    if ch(at) == 93 {
                        at += 1;
                    } else {
                        stack.push(Frame {
                            object: false,
                            pointer: pointer.clone(),
                            keys_at: String::new(),
                            ranges: HashMap::new(),
                            key: String::new(),
                            from: 0,
                            index: 0,
                        });
                        pointer.push_str("/0");
                        state = State::Value;
                    }
                } else if c == 34 {
                    read_string(&mut at, false)?;
                } else if c == 116 {
                    literal(&mut at, "true")?;
                } else if c == 102 {
                    literal(&mut at, "false")?;
                } else if c == 110 {
                    literal(&mut at, "null")?;
                } else if c == 45 || is_digit(c) {
                    number(&mut at)?;
                } else {
                    return Err(at);
                }
            }
            State::Key => {
                if ch(at) != 34 {
                    return Err(at);
                }
                let key = read_string(&mut at, true)?;
                let frame = stack.last_mut().unwrap();
                // 同じ鍵は順の最初の場所を保ち、入れ子の順は最後の値のものを取る。
                match frame.ranges.get(&key) {
                    None => {
                        if let Some(keys) = order.get_mut(&frame.keys_at) {
                            keys.push(key.clone());
                        }
                    }
                    Some(&(a, b)) => {
                        for c in &created[a..b] {
                            order.remove(c);
                        }
                    }
                }
                frame.key = key.clone();
                frame.from = created.len();
                skip(&mut at);
                if ch(at) != 58 {
                    return Err(at);
                }
                at += 1;
                pointer = format!("{}/{}", frame.pointer, escape_pointer(&key));
                state = State::Value;
            }
            State::After => {
                let Some(frame) = stack.last_mut() else {
                    break;
                };
                if frame.object {
                    frame
                        .ranges
                        .insert(frame.key.clone(), (frame.from, created.len()));
                } else {
                    frame.index += 1;
                }
                skip(&mut at);
                let c = ch(at);
                if c == if frame.object { 125 } else { 93 } {
                    at += 1;
                    stack.pop();
                } else if c != 44 {
                    return Err(at);
                } else {
                    at += 1;
                    skip(&mut at);
                    pointer = format!("{}/{}", frame.pointer, frame.index);
                    state = if frame.object {
                        State::Key
                    } else {
                        State::Value
                    };
                }
            }
        }
    }
    skip(&mut at);
    if at != n {
        return Err(at);
    }
    Ok(order)
}

fn location_at(text: &[u16], offset: f64, mut line: usize, mut column: usize) -> (usize, usize) {
    let end = js::min(js::max(offset, 0.0), text.len() as f64) as usize;
    for i in 0..end {
        let c = text[i];
        if c == 13 {
            line += 1;
            column = 1;
        } else if c == 10 {
            if i == 0 || text[i - 1] != 13 {
                line += 1;
                column = 1;
            }
        } else {
            column += 1;
        }
    }
    (line, column)
}

/// `\b<word> (\d+)\b` の数（最初に合うもの）。
fn number_after(message: &str, word: &str) -> Option<(usize, String)> {
    let b = message.as_bytes();
    let mut from = 0;
    while let Some(k) = message[from..].find(word) {
        let s = from + k;
        let before_ok = s == 0 || !crate::util::chars::is_word(b[s - 1] as u32);
        let mut e = s + word.len();
        if before_ok && e < b.len() && b[e] == b' ' {
            e += 1;
            let d = e;
            while e < b.len() && b[e].is_ascii_digit() {
                e += 1;
            }
            let after_ok = e == b.len() || !crate::util::chars::is_word(b[e] as u32);
            if e > d && after_ok {
                return Some((e, message[d..e].to_string()));
            }
        }
        from = s + 1;
    }
    None
}

/// エンジンによって JSON.parse の知らせの書き方は違う: 位置を言うものも、行と列を言うものもある。
fn location_from_message(
    message: &str,
    text: &[u16],
    line: usize,
    column: usize,
) -> Option<(usize, usize)> {
    if let Some((_, position)) = number_after(message, "position") {
        return Some(location_at(
            text,
            js::parse_int(&position, 10),
            line,
            column,
        ));
    }
    // `\bline (\d+) column (\d+)\b`
    let b = message.as_bytes();
    let mut from = 0;
    while let Some(k) = message[from..].find("line ") {
        let s = from + k;
        if s == 0 || !crate::util::chars::is_word(b[s - 1] as u32) {
            let mut e = s + 5;
            let d1 = e;
            while e < b.len() && b[e].is_ascii_digit() {
                e += 1;
            }
            if e > d1 && message[e..].starts_with(" column ") {
                let d2 = e + 8;
                let mut f = d2;
                while f < b.len() && b[f].is_ascii_digit() {
                    f += 1;
                }
                if f > d2 && (f == b.len() || !crate::util::chars::is_word(b[f] as u32)) {
                    let local_line = js::parse_int(&message[d1..e], 10) as usize;
                    let local_column = js::parse_int(&message[d2..f], 10) as usize;
                    return Some((
                        line + local_line - 1,
                        if local_line == 1 {
                            column + local_column - 1
                        } else {
                            local_column
                        },
                    ));
                }
            }
        }
        from = s + 1;
    }
    let lower = message.to_lowercase();
    if lower.contains("unexpected end") || lower.contains("end of json input") {
        return Some(location_at(text, text.len() as f64, line, column));
    }
    None
}

fn json_error(message: &str, line: usize, column: usize) -> PeleError {
    let mut e = PeleError::typed(
        format!("{message} (line {line}, column {column})"),
        ErrorCode::Syntax,
        "usecase",
    );
    e.line = line;
    e.column = column;
    e
}

/// `json` のノードの本体を読む。`origin` は、その文が図のどの行と列で始まるか。
pub fn parse_ordered_json(
    text: &str,
    origin: impl Fn() -> (usize, usize),
) -> std::result::Result<OrderedJson, PeleError> {
    let units: Vec<u16> = text.encode_utf16().collect();
    let parsed = match json_parse(&units) {
        Ok(v) => v,
        Err(first) => {
            let mut message = first;
            // 符号を戻した文で読み直し、知らせの文と位置を入力に合わせる。
            let shown_text = written_codes(text);
            let shown: Vec<u16> = shown_text.encode_utf16().collect();
            if shown_text != text
                && let Err(again) = json_parse(&shown)
            {
                message = again;
            }
            let (line, column) = origin();
            let location = match location_from_message(&message, &shown, line, column) {
                Some(l) => l,
                None => {
                    let offset = property_order(&shown).err().unwrap_or(0);
                    location_at(&shown, offset as f64, line, column)
                }
            };
            return Err(json_error(
                &format!("Invalid JSON: {message}"),
                location.0,
                location.1,
            ));
        }
    };
    if !matches!(parsed, Json::Obj(_)) {
        let (line, column) = origin();
        return Err(json_error(
            "JSON value must have an object root",
            line,
            column,
        ));
    }
    Ok(OrderedJson {
        value: parsed,
        property_order: property_order(&units).unwrap_or_default(),
    })
}
