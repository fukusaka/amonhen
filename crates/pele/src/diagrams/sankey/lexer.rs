//! sankey の字句解析（pele の `src/diagrams/sankey/lexer.ts`）。Mermaid の Jison の CSV の字句規則を手で写したもの。
//! 位置は UTF-16 の符号単位。

use crate::js;
use crate::util::chars::{NONE, is_word};

pub const END: u8 = 0;
/// Jison の字句ではない: 元の字句解析はここから同じ空の字句を限りなく返す。
pub const STUCK: u8 = 1;
pub const SANKEY: u8 = 2;
pub const NEWLINE: u8 = 3;
pub const COMMA: u8 = 4;
pub const DQUOTE: u8 = 5;
pub const NON_ESCAPED_TEXT: u8 = 6;
pub const ESCAPED_TEXT: u8 = 7;
pub const EOF: u8 = 8;

pub const TOKEN_NAMES: [&str; 9] = [
    "$end",
    "STUCK",
    "SANKEY",
    "NEWLINE",
    "COMMA",
    "DQUOTE",
    "NON_ESCAPED_TEXT",
    "ESCAPED_TEXT",
    "EOF",
];

pub struct Tokens {
    pub types: Vec<u8>,
    pub texts: Vec<String>,
    pub starts: Vec<usize>,
}

const INITIAL: u8 = 0;
const CSV: u8 = 1;
const ESCAPED: u8 = 2;

/// RFC 4180 の TEXTDATA を Unicode に広げたもの。制御文字と XML に書けない文字は除く。
fn is_text_data(c: u32) -> bool {
    ((32..=126).contains(&c) && c != 34 && c != 44)
        || (0xa0..=0xd7ff).contains(&c)
        || (0xe000..=0xfffd).contains(&c)
        || (0x10000..=0x10ffff).contains(&c)
}

/// `codePointAt`。
fn code_point_at(src: &[u16], q: usize) -> u32 {
    let c = src[q] as u32;
    if (0xd800..=0xdbff).contains(&c)
        && let Some(&d) = src.get(q + 1)
        && (0xdc00..=0xdfff).contains(&(d as u32))
    {
        return 0x10000 + ((c - 0xd800) << 10) + (d as u32 - 0xdc00);
    }
    c
}

pub fn tokenize(src: &[u16]) -> Tokens {
    let n = src.len();
    let mut types = Vec::new();
    let mut texts = Vec::new();
    let mut starts = Vec::new();
    let mut stack: Vec<u8> = Vec::new();
    let mut state = INITIAL;
    let mut p = 0;
    let at = |q: usize| src.get(q).map_or(NONE, |&c| c as u32);

    macro_rules! emit {
        ($t:expr, $s:expr, $e:expr) => {{
            let (s, e) = ($s, $e);
            types.push($t);
            texts.push(js::from16(&src[s..e]));
            starts.push(s);
            p = e;
        }};
    }
    let keyword = |p: usize, word: &str| -> bool {
        let end = (p + word.len()).min(n);
        String::from_utf16_lossy(&src[p..end]).to_lowercase() == word
            && !is_word(at(p + word.len()))
    };

    while p < n {
        let c = at(p);
        let start = p;
        let mut q = p;
        if state == ESCAPED {
            if c == 34 && at(p + 1) != 34 {
                if let Some(s) = stack.pop() {
                    state = s;
                }
                emit!(DQUOTE, p, p + 1);
                continue;
            }
            while q < n {
                let d = code_point_at(src, q);
                if is_text_data(d) || d == 44 || d == 13 || d == 10 {
                    q += if d > 0xffff { 2 } else { 1 };
                } else if d == 34 && at(q + 1) == 34 {
                    q += 2;
                } else {
                    break;
                }
            }
            emit!(ESCAPED_TEXT, p, q);
        } else {
            if state == INITIAL && (c | 32) == 115 {
                let length = if keyword(p, "sankey-beta") {
                    11
                } else if keyword(p, "sankey") {
                    6
                } else {
                    0
                };
                if length > 0 {
                    stack.push(state);
                    state = CSV;
                    emit!(SANKEY, p, p + length);
                    continue;
                }
            }
            if c == 10 || (c == 13 && at(p + 1) == 10) {
                emit!(NEWLINE, p, if c == 10 { p + 1 } else { p + 2 });
                continue;
            }
            if c == 44 {
                emit!(COMMA, p, p + 1);
                continue;
            }
            if c == 34 {
                stack.push(state);
                state = ESCAPED;
                emit!(DQUOTE, p, p + 1);
                continue;
            }
            while q < n {
                let d = code_point_at(src, q);
                if !is_text_data(d) {
                    break;
                }
                q += if d > 0xffff { 2 } else { 1 };
            }
            emit!(NON_ESCAPED_TEXT, p, q);
        }
        // CSV の字の外の文字は空の文字として合い、決して読み進まない。
        if q == start {
            types.push(STUCK);
            texts.push(String::new());
            starts.push(p);
            return Tokens {
                types,
                texts,
                starts,
            };
        }
    }

    types.push(if state == ESCAPED { ESCAPED_TEXT } else { EOF });
    types.push(END);
    texts.extend([String::new(), String::new()]);
    starts.extend([n, n]);
    Tokens {
        types,
        texts,
        starts,
    }
}
