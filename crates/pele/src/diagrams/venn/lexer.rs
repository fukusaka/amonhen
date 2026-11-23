//! venn の字句解析（pele の `src/diagrams/venn/lexer.ts`）。Mermaid の venn の Jison の字句解析と同じく、字句を
//! 1 つずつ読む: どの規則も大文字小文字を区別せず、最初に合った規則を取り、キーワードは語の境で終わる。

use crate::js;
use crate::util::chars::{NONE, is_digit, is_hex, is_space, is_word};

pub mod t {
    pub const END: i32 = 0;
    pub const ERROR: i32 = 1;
    pub const EOF: i32 = 2;
    pub const NEWLINE: i32 = 3;
    pub const VENN: i32 = 4;
    pub const TITLE: i32 = 5;
    pub const SET: i32 = 6;
    pub const UNION: i32 = 7;
    pub const TEXT: i32 = 8;
    pub const INDENT_TEXT: i32 = 9;
    pub const STYLE: i32 = 10;
    pub const BRACKET_LABEL: i32 = 11;
    pub const NUMERIC: i32 = 12;
    pub const HEXCOLOR: i32 = 13;
    pub const RGBCOLOR: i32 = 14;
    pub const RGBACOLOR: i32 = 15;
    pub const IDENTIFIER: i32 = 16;
    pub const STRING: i32 = 17;
    pub const COMMA: i32 = 18;
    pub const COLON: i32 = 19;
}

pub const TOKEN_NAMES: [&str; 20] = [
    "$end",
    "INVALID",
    "EOF",
    "NEWLINE",
    "VENN",
    "TITLE",
    "SET",
    "UNION",
    "TEXT",
    "INDENT_TEXT",
    "STYLE",
    "BRACKET_LABEL",
    "NUMERIC",
    "HEXCOLOR",
    "RGBCOLOR",
    "RGBACOLOR",
    "IDENTIFIER",
    "STRING",
    "COMMA",
    "COLON",
];

/// 字句解析が話しかける形の部分: 字下げした `text` の行は、set・union の後とそれ以外とで違う字句で、どちらが前に
/// あったかは読み手が知っている。
pub trait IndentHost {
    fn indent_mode(&self) -> bool;
    fn set_indent_mode(&mut self, enabled: bool);
}

const KEYWORDS: [(&str, i32); 5] = [
    ("venn-beta", t::VENN),
    ("set", t::SET),
    ("union", t::UNION),
    ("text", t::TEXT),
    ("style", t::STYLE),
];

pub struct VennLexer {
    src: Vec<u16>,
    /// `next` が最後に返した字句の文字と位置。
    pub text: String,
    pub start: usize,
    at: usize,
    line_start: bool,
    ended: bool,
}

impl VennLexer {
    pub fn new(src: &str) -> Self {
        VennLexer {
            src: js::utf16(src),
            text: String::new(),
            start: 0,
            at: 0,
            line_start: false,
            ended: false,
        }
    }

    pub fn src(&self) -> &[u16] {
        &self.src
    }

    fn c(&self, i: usize) -> u32 {
        self.src.get(i).map_or(NONE, |&c| c as u32)
    }

    fn slice(&self, from: usize, to: usize) -> String {
        let n = self.src.len();
        js::from16(&self.src[from.min(n)..to.min(n).max(from.min(n))])
    }

    fn word(&self, at: usize, word: &str) -> bool {
        for (k, w) in word.bytes().enumerate() {
            let c = self.c(at + k);
            let w = w as u32;
            if c != w && (c.wrapping_add(32) != w || !(65..=90).contains(&c)) {
                return false;
            }
        }
        true
    }

    fn keyword(&self, at: usize, word: &str) -> bool {
        self.word(at, word) && !is_word(self.c(at + word.len()))
    }

    /// 括弧が `at` で開く `rgb(1, 2, 3)`・`rgba(1, 2, 3, 0.5)` の終わり。
    fn color(&self, at: usize, parts: usize) -> Option<usize> {
        let mut i = at + 1;
        for part in 0..parts {
            while is_space(self.c(i)) {
                i += 1;
            }
            let from = i;
            while is_digit(self.c(i)) || self.c(i) == 46 {
                i += 1;
            }
            if i == from {
                return None;
            }
            while is_space(self.c(i)) {
                i += 1;
            }
            if self.c(i) != (if part == parts - 1 { 41 } else { 44 }) {
                return None;
            }
            i += 1;
        }
        Some(i)
    }

    fn after_line_feed(&self, at: usize) -> bool {
        let mut k = at as isize - 1;
        let c = |k: isize| {
            if k < 0 { NONE } else { self.c(k as usize) }
        };
        while c(k) == 32 || c(k) == 9 {
            k -= 1;
        }
        c(k) == 10
    }

    fn line_end(&self, from: usize) -> usize {
        (from.min(self.src.len())..self.src.len())
            .find(|&k| self.src[k] == 10)
            .unwrap_or(self.src.len())
    }

    pub fn next(&mut self, host: &mut dyn IndentHost) -> i32 {
        let n = self.src.len();
        loop {
            let mut i = self.at;
            self.start = i;
            self.text = String::new();
            if i >= n {
                if self.ended {
                    return t::END;
                }
                self.ended = true;
                return t::EOF;
            }
            let c = self.c(i);
            if c == 37 && self.c(i + 1) == 37 && self.c(i + 2) != 123 {
                self.at = self.line_end(i + 2);
                continue;
            }
            if self.line_start
                && self.word(i, "title")
                && self.after_line_feed(i)
                && js::trim(&self.slice(i + 5, self.line_end(i + 5))).is_empty()
            {
                // 中身のない題はコメントのように飛ばし、字下げした文字を終わらせない。
                let mut j = self.line_end(i + 5);
                while self.c(j) == 10 || self.c(j) == 13 {
                    j += 1;
                }
                self.at = j;
                continue;
            }
            if self.line_start {
                if c == 32 || c == 9 {
                    let mut j = i + 1;
                    while self.c(j) == 32 || self.c(j) == 9 {
                        j += 1;
                    }
                    self.at = j;
                    if self.keyword(j, "text") && host.indent_mode() {
                        // `text` の語そのものは捨てる。字下げがそれを表す。
                        self.at = j + 4;
                        self.line_start = false;
                        self.text = self.slice(i, j);
                        return t::INDENT_TEXT;
                    }
                    continue;
                }
                if c != 10 && c != 13 {
                    host.set_indent_mode(false);
                    self.line_start = false;
                }
            }
            if c == 10 || c == 13 {
                i += 1;
                while self.c(i) == 10 || self.c(i) == 13 {
                    i += 1;
                }
                self.line_start = true;
                return self.token(t::NEWLINE, i);
            }
            if c == 37 && self.c(i + 1) == 37 {
                self.at = self.line_end(i);
                continue;
            }
            if c == 32 || c == 9 {
                i += 1;
                while self.c(i) == 32 || self.c(i) == 9 {
                    i += 1;
                }
                self.at = i;
                continue;
            }

            if ((c | 32) >= 97 && (c | 32) <= 122) || c == 95 {
                let sep = self.c(i + 5);
                if self.word(i, "title") && is_space(sep) && sep != 10 && sep != 13 {
                    let mut end = i + 6;
                    while end < n {
                        let ch = self.c(end);
                        if ch == 35 || ch == 10 || ch == 59 {
                            break;
                        }
                        end += 1;
                    }
                    if end > i + 6 {
                        return self.token(t::TITLE, end);
                    }
                }
                for (word, type_) in KEYWORDS {
                    // `set-theory` のように、ハイフンは名前を続ける。
                    if self.keyword(i, word) && self.c(i + word.len()) != 45 {
                        return self.token(type_, i + word.len());
                    }
                }
                if self.word(i, "rgb") {
                    let alpha = (self.c(i + 3) | 32) == 97;
                    let open = i + if alpha { 4 } else { 3 };
                    let end = if self.c(open) == 40 {
                        self.color(open, if alpha { 4 } else { 3 })
                    } else {
                        None
                    };
                    if let Some(end) = end {
                        return self.token(if alpha { t::RGBACOLOR } else { t::RGBCOLOR }, end);
                    }
                }
                i += 1;
                while is_word(self.c(i)) || self.c(i) == 45 {
                    i += 1;
                }
                return self.token(t::IDENTIFIER, i);
            }
            if c == 91 {
                if self.c(i + 1) == 34 {
                    let quote = (i + 2..n).find(|&k| self.src[k] == 34);
                    let Some(quote) = quote.filter(|&q| self.c(q + 1) == 93) else {
                        return t::ERROR;
                    };
                    self.at = quote + 2;
                    self.text = self.slice(i + 2, quote);
                    return t::BRACKET_LABEL;
                }
                let mut end = i + 1;
                while end < n && self.c(end) != 93 && self.c(end) != 34 {
                    end += 1;
                }
                if end == i + 1 || self.c(end) != 93 {
                    return t::ERROR;
                }
                self.at = end + 1;
                self.text = js::trim(&self.slice(i + 1, end)).to_string();
                return t::BRACKET_LABEL;
            }
            if is_digit(c) || c == 46 || c == 43 || c == 45 {
                if c == 43 || c == 45 {
                    i += 1;
                }
                let from = i;
                while is_digit(self.c(i)) {
                    i += 1;
                }
                if i > from {
                    if self.c(i) == 46 && is_digit(self.c(i + 1)) {
                        i += 2;
                        while is_digit(self.c(i)) {
                            i += 1;
                        }
                    }
                } else if self.c(i) == 46 && is_digit(self.c(i + 1)) {
                    i += 2;
                    while is_digit(self.c(i)) {
                        i += 1;
                    }
                } else {
                    return t::ERROR;
                }
                return self.token(t::NUMERIC, i);
            }
            if c == 35 {
                let mut end = i + 1;
                while end < i + 9 && is_hex(self.c(end)) {
                    end += 1;
                }
                return if end >= i + 4 {
                    self.token(t::HEXCOLOR, end)
                } else {
                    t::ERROR
                };
            }
            if c == 34 {
                return match (i + 1..n).find(|&k| self.src[k] == 34) {
                    Some(quote) => self.token(t::STRING, quote + 1),
                    None => t::ERROR,
                };
            }
            if c == 44 {
                return self.token(t::COMMA, i + 1);
            }
            if c == 58 {
                return self.token(t::COLON, i + 1);
            }
            return t::ERROR;
        }
    }

    fn token(&mut self, type_: i32, end: usize) -> i32 {
        self.text = self.slice(self.at, end);
        self.at = end;
        type_
    }
}
