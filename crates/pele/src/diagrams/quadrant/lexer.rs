//! quadrantChart の字句解析（pele の `src/diagrams/quadrant/lexer.ts`）。Mermaid の Jison の字句規則を、状態を
//! 持つ手書きの走査に置き換えたもの。位置は UTF-16 の符号単位。

use super::tokens::*;
use crate::diagrams::common::caseless::word_at;
use crate::js;
use crate::util::chars::{NONE, is_digit, is_letter, is_line_end, is_space, is_word};

pub struct Tokens {
    pub types: Vec<u8>,
    pub texts: Vec<String>,
    pub starts: Vec<usize>,
}

// 状態（TS の `const enum S`）。
const INITIAL: usize = 0;
const S_TITLE: usize = 1;
const S_ACC_TITLE: usize = 2;
const S_ACC_DESCR: usize = 3;
const S_ACC_DESCR_MULTILINE: usize = 4;
const S_STRING: usize = 5;
const S_MD_STRING: usize = 6;
const S_CLASS_NAME: usize = 7;
const S_POINT_START: usize = 8;
const S_POINT_X: usize = 9;
const S_POINT_Y: usize = 10;

/// 状態ごとに、入力の空の残りに対して出す字句（出すものだけ）。
const AT_END: [u8; 6] = [
    EOF,
    TITLE_VALUE,
    ACC_TITLE_VALUE,
    ACC_DESCR_VALUE,
    ACC_DESCR_MULTILINE_VALUE,
    STR,
];

fn single(c: u32) -> u8 {
    match c {
        58 => COLON,
        43 => PLUS,
        44 => COMMA,
        61 => EQUALS,
        42 => MULT,
        35 => BRKT,
        95 => UNDERSCORE,
        46 => DOT,
        38 => AMP,
        45 => MINUS,
        32 | 9 | 11 | 12 => SPACE,
        59 => SEMI,
        33 | 36 | 37 | 39 | 96 | 63 | 92 | 47 => PUNCTUATION,
        _ => 0,
    }
}

struct Lexer<'a> {
    src: &'a [u16],
    n: usize,
    p: usize,
    types: Vec<u8>,
    texts: Vec<String>,
    starts: Vec<usize>,
    stack: Vec<usize>,
    state: usize,
}

impl Lexer<'_> {
    fn at(&self, q: usize) -> u32 {
        self.src.get(q).map_or(NONE, |&c| c as u32)
    }

    fn emit(&mut self, type_: u8, s: usize, e: usize) {
        self.types.push(type_);
        self.texts.push(js::from16(&self.src[s..e]));
        self.starts.push(s);
        self.p = e;
    }

    fn push(&mut self, s: usize) {
        self.stack.push(self.state);
        self.state = s;
    }

    fn pop(&mut self) {
        if let Some(s) = self.stack.pop() {
            self.state = s;
        }
    }

    fn index_of(&self, c: u16, q: usize) -> Option<usize> {
        self.src
            .get(q..)
            .and_then(|s| s.iter().position(|&x| x == c))
            .map(|k| k + q)
    }

    fn line_end(&self, q: usize) -> usize {
        self.index_of(10, q).unwrap_or(self.n)
    }

    fn spaces(&self, mut q: usize) -> usize {
        while self.at(q) == 32 {
            q += 1;
        }
        q
    }

    fn run(&self, mut q: usize) -> usize {
        while q < self.n && is_space(self.at(q)) {
            q += 1;
        }
        q
    }

    /// Mermaid の `(1)|(0(.\d+)?)`。点は行の中のどの文字でもよい。
    fn coordinate(&self, q: usize) -> Option<usize> {
        let c = self.at(q);
        if c == 49 {
            return Some(q + 1);
        }
        if c != 48 {
            return None;
        }
        let mut e = q + 1;
        if e < self.n && !is_line_end(self.at(e)) && is_digit(self.at(e + 1)) {
            e += 2;
            while is_digit(self.at(e)) {
                e += 1;
            }
        }
        Some(e)
    }
}

pub fn tokenize(src: &[u16]) -> Tokens {
    let n = src.len();
    let mut l = Lexer {
        src,
        n,
        p: 0,
        types: Vec::new(),
        texts: Vec::new(),
        starts: Vec::new(),
        stack: Vec::new(),
        state: INITIAL,
    };
    // 種類ごとに最後に走査した並び。長い並びを、その文字ごとに走査し直さないように。
    let (mut space_from, mut space_to) = (0, 0);
    let (mut dash_from, mut dash_to) = (0, 0);
    let (mut ws_from, mut ws_to) = (0, 0);
    let (mut gap_from, mut gap_to) = (0, 0);

    'lexing: while l.p < n {
        let p = l.p;
        let c = l.at(p);
        let next = l.at(p + 1);

        match l.state {
            S_TITLE | S_ACC_TITLE | S_ACC_DESCR => {
                let e = l.line_end(p);
                l.emit(AT_END[l.state], p, e);
                l.pop();
                continue;
            }
            S_ACC_DESCR_MULTILINE => {
                if c == 125 {
                    l.p += 1;
                    l.pop();
                } else {
                    let e = l.index_of(125, p).unwrap_or(n);
                    l.emit(ACC_DESCR_MULTILINE_VALUE, p, e);
                }
                continue;
            }
            S_STRING => {
                if c == 34 {
                    l.p += 1;
                    l.pop();
                } else {
                    let e = l.index_of(34, p).unwrap_or(n);
                    l.emit(STR, p, e);
                }
                continue;
            }
            S_MD_STRING => {
                if c == 96 && next == 34 {
                    l.p += 2;
                    l.pop();
                    continue;
                }
                if c != 96 && c != 34 {
                    let mut e = p + 1;
                    while e < n && l.at(e) != 96 && l.at(e) != 34 {
                        e += 1;
                    }
                    l.emit(MD_STR, p, e);
                    continue;
                }
                break 'lexing;
            }
            S_CLASS_NAME => {
                if is_word(c) {
                    let mut e = p + 1;
                    while is_word(l.at(e)) {
                        e += 1;
                    }
                    l.emit(CLASS_NAME, p, e);
                    l.pop();
                    continue;
                }
                break 'lexing;
            }
            S_POINT_START => {
                if let Some(e) = l.coordinate(p) {
                    l.emit(POINT_X, p, e);
                    l.push(S_POINT_X);
                    continue;
                }
                let k = l.run(p);
                if l.at(k) == 93 {
                    l.p = l.spaces(k + 1);
                    l.pop();
                    continue;
                }
                break 'lexing;
            }
            S_POINT_X => {
                let k = l.run(p);
                if l.at(k) == 44 {
                    l.p = l.run(k + 1);
                    l.state = S_POINT_Y;
                    continue;
                }
                break 'lexing;
            }
            S_POINT_Y => {
                if let Some(e) = l.coordinate(p) {
                    l.emit(POINT_Y, p, e);
                    l.pop();
                    continue;
                }
                break 'lexing;
            }
            _ => {}
        }

        if c == 37 && next == 37 {
            l.p = l.line_end(p + 2);
            continue;
        }
        if c == 10 || c == 13 {
            let mut e = p + 1;
            while matches!(l.at(e), 10 | 13) {
                e += 1;
            }
            l.emit(NEWLINE, p, e);
            continue;
        }

        let lower = c | 32;
        if lower == 116 && word_at(src, p, "title") && !is_word(l.at(p + 5)) {
            l.emit(TITLE, p, p + 5);
            l.push(S_TITLE);
            continue;
        }
        if lower == 97 && word_at(src, p, "acc") {
            let descr = word_at(src, p + 3, "descr");
            if descr || word_at(src, p + 3, "title") {
                let k = l.run(p + 8);
                let d = l.at(k);
                if d == 58 {
                    let e = l.run(k + 1);
                    l.emit(if descr { ACC_DESCR } else { ACC_TITLE }, p, e);
                    l.push(if descr { S_ACC_DESCR } else { S_ACC_TITLE });
                    continue;
                }
                if d == 123 && descr {
                    l.p = l.run(k + 1);
                    l.push(S_ACC_DESCR_MULTILINE);
                    continue;
                }
            }
        }

        // 軸・象限・図の語と軸の矢印は、まわりの空白も取る。
        let mut j = p;
        if c == 32 {
            if p < space_from || p >= space_to {
                space_from = p;
                space_to = l.spaces(p);
            }
            j = space_to;
        }
        let d = l.at(j);
        let dl = d | 32;
        let mut type_ = 0;
        let mut end = 0;
        if dl == 120 && word_at(src, j, "x-axis") {
            type_ = X_AXIS;
            end = j + 6;
        } else if dl == 121 && word_at(src, j, "y-axis") {
            type_ = Y_AXIS;
            end = j + 6;
        } else if d == 45 {
            if j < dash_from || j >= dash_to {
                dash_from = j;
                dash_to = j;
                while l.at(dash_to) == 45 {
                    dash_to += 1;
                }
            }
            if dash_to - j >= 2 && l.at(dash_to) == 62 {
                type_ = DELIMITER;
                end = dash_to + 1;
            }
        } else if dl == 113 && word_at(src, j, "quadrant") {
            let q = l.at(j + 9);
            if l.at(j + 8) == 45 && (49..=52).contains(&q) {
                type_ = QUADRANT_1 + (q - 49) as u8;
                end = j + 10;
            } else if word_at(src, j + 8, "chart") {
                type_ = QUADRANT;
                end = j + 13;
            }
        }
        if type_ != 0 {
            let e = l.spaces(end);
            l.emit(type_, p, e);
            continue;
        }

        if lower == 99 && word_at(src, p, "classdef") && !is_word(l.at(p + 8)) {
            l.emit(CLASSDEF, p, p + 8);
            continue;
        }
        if c == 34 {
            if next == 96 {
                l.p += 2;
                l.push(S_MD_STRING);
            } else {
                l.p += 1;
                l.push(S_STRING);
            }
            continue;
        }
        if c == 58 && next == 58 && l.at(p + 2) == 58 {
            l.p += 3;
            l.push(S_CLASS_NAME);
            continue;
        }
        // 点の `: [`。前・間・後に、改行も含めてどんな空白があってもよい。
        if c == 58 || is_space(c) {
            let mut k = p;
            if c != 58 {
                if p < ws_from || p >= ws_to {
                    ws_from = p;
                    ws_to = l.run(p);
                }
                k = ws_to;
            }
            if l.at(k) == 58 {
                if k + 1 < gap_from || k + 1 >= gap_to {
                    gap_from = k + 1;
                    gap_to = l.run(k + 1);
                }
                if l.at(gap_to) == 91 {
                    let e = l.run(gap_to + 1);
                    l.emit(POINT_START, p, e);
                    l.push(S_POINT_START);
                    continue;
                }
            }
        }

        if is_letter(c) {
            let mut e = p + 1;
            while is_letter(l.at(e)) {
                e += 1;
            }
            l.emit(ALPHA, p, e);
            continue;
        }
        if c >= 128 {
            let mut e = p + 1;
            while l.at(e) >= 128 && l.at(e) != NONE {
                e += 1;
            }
            l.emit(UNICODE_TEXT, p, e);
            continue;
        }
        if is_digit(c) {
            let mut e = p + 1;
            while is_digit(l.at(e)) {
                e += 1;
            }
            l.emit(NUM, p, e);
            continue;
        }
        let s = single(c);
        if s != 0 {
            l.emit(s, p, p + 1);
            continue;
        }
        break;
    }

    let Lexer {
        mut types,
        mut texts,
        mut starts,
        state,
        p,
        ..
    } = l;
    if p < n {
        types.push(ERROR);
        texts.push(String::new());
        starts.push(p);
        return Tokens {
            types,
            texts,
            starts,
        };
    }
    // Jison は、今の状態の規則を空の残りに対してもう一度試す。
    if state < AT_END.len() {
        types.push(AT_END[state]);
        texts.push(String::new());
        starts.push(n);
    }
    types.push(END);
    texts.push(String::new());
    starts.push(n);
    Tokens {
        types,
        texts,
        starts,
    }
}
