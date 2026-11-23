//! ishikawa の字句（pele の `src/diagrams/ishikawa/lexer.ts`）。

use crate::util::chars::{NONE, is_space, is_word};

pub mod t {
    pub const END: u8 = 0;
    pub const EOF: u8 = 1;
    pub const SPACELINE: u8 = 2;
    pub const NL: u8 = 3;
    pub const ISHIKAWA: u8 = 4;
    pub const SPACELIST: u8 = 5;
    pub const TEXT: u8 = 6;
}

pub const TOKEN_NAMES: [&str; 7] = [
    "$end",
    "EOF",
    "SPACELINE",
    "NL",
    "ISHIKAWA",
    "SPACELIST",
    "TEXT",
];

pub struct Tokens {
    pub types: Vec<u8>,
    pub starts: Vec<usize>,
    pub ends: Vec<usize>,
}

fn at(src: &[u16], p: usize) -> u32 {
    src.get(p).map_or(NONE, |&c| c as u32)
}

fn keyword(src: &[u16], p: usize) -> usize {
    let word = b"ishikawa-beta";
    let mut k = 0;
    while k < 13 {
        let c = at(src, p + k);
        let w = word[k] as u32;
        if c != w && (c.wrapping_add(32) != w || !(65..=90).contains(&c)) {
            break;
        }
        k += 1;
    }
    // `ishikawa-beta` か、それでなければ `ishikawa`。どちらも語の境で終わる。
    if k == 13 && !is_word(at(src, p + 13)) {
        return 13;
    }
    if k >= 8 && !is_word(at(src, p + 8)) {
        8
    } else {
        0
    }
}

/// Mermaid の ishikawa の Jison の字句解析と同じ字句を作る（最初に合う規則が勝つ）: コメントまでか、中の最後の
/// 改行までの空白は空行、文字の前のただ 1 つの改行は新しい行、ほかの空白は字下げ、行の残りは文字。
pub fn tokenize(src: &[u16]) -> Tokens {
    let mut types = Vec::new();
    let mut starts = Vec::new();
    let mut ends = Vec::new();
    let n = src.len();
    let mut i = 0;
    while i < n {
        let mut j = i;
        let mut last_break: Option<usize> = None;
        while j < n && is_space(src[j] as u32) {
            if src[j] == 10 {
                last_break = Some(j);
            }
            j += 1;
        }
        let (type_, end) = if at(src, j) == 37 && at(src, j + 1) == 37 {
            let mut end = j + 2;
            while end < n && !matches!(src[end], 10 | 13 | 0x2028 | 0x2029) {
                end += 1;
            }
            (t::SPACELINE, end)
        } else if j == i {
            let length = keyword(src, i);
            if length > 0 {
                (t::ISHIKAWA, i + length)
            } else {
                let end = src[i..].iter().position(|&c| c == 10).map_or(n, |k| i + k);
                (t::TEXT, end)
            }
        } else if last_break.is_some_and(|b| b > i) {
            (t::SPACELINE, last_break.unwrap() + 1)
        } else if last_break == Some(i) {
            (t::NL, i + 1)
        } else {
            (t::SPACELIST, j)
        };
        types.push(type_);
        starts.push(i);
        ends.push(end);
        i = end;
    }
    types.extend([t::EOF, t::END]);
    starts.extend([n, n]);
    ends.extend([n, n]);
    Tokens {
        types,
        starts,
        ends,
    }
}
