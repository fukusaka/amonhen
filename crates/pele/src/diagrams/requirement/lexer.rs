//! requirement の字句解析（pele の `src/diagrams/requirement/lexer.ts`）。Mermaid の requirement の字句の並びを
//! 作る。規則は順に試し、最初に合ったものを取るので、どこかに向きの文を持つ行は 1 つの字句になる。JS と同じく
//! UTF-16 の符号単位で読む。

use super::tokens as t;
use crate::util::chars::{NONE, is_line_end, is_space, is_word};

pub struct Tokens {
    pub types: Vec<u8>,
    pub texts: Vec<String>,
    pub starts: Vec<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum S {
    Initial,
    String,
    Style,
    AccTitle,
    AccDescr,
    AccDescrMultiline,
}

const LONGEST_KEYWORD: usize = 22;

/// 引用のない文字列を終える文字: `:,{<>-=` と改行。
fn stop(c: u32) -> bool {
    matches!(
        c,
        0x3a | 0x2c | 0x0d | 0x0a | 0x7b | 0x3c | 0x3e | 0x2d | 0x3d
    )
}

fn lower(c: u32) -> u32 {
    if (65..=90).contains(&c) { c | 32 } else { c }
}

/// `word` が `at` から大文字小文字を問わずに合うか（`word` は小文字の ASCII）。
fn word_at(src: &[u16], at: usize, word: &str) -> bool {
    let w = word.as_bytes();
    src.len() >= at + w.len()
        && w.iter()
            .enumerate()
            .all(|(k, &b)| lower(src[at + k] as u32) == b as u32)
}

fn spaces(src: &[u16], mut q: usize) -> usize {
    while q < src.len() && is_space(src[q] as u32) {
        q += 1;
    }
    q
}

/// `/accTitle\s*:\s*/iy` など: `word` の後の空白、`mark`、空白。合えば終わり。
fn acc_at(src: &[u16], p: usize, word: &str, mark: u16) -> Option<usize> {
    if !word_at(src, p, word) {
        return None;
    }
    let q = spaces(src, p + word.len());
    if src.get(q) != Some(&mark) {
        return None;
    }
    Some(spaces(src, q + 1))
}

/// `/direction\s+(?:(TB)|(BT)|(RL)|LR)/gi` の合う所: (種類, 始まり, 終わり)。
fn directions(src: &[u16]) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    let n = src.len();
    let mut p = 0;
    while p + 9 <= n {
        if word_at(src, p, "direction") {
            let q = p + 9;
            let r = spaces(src, q);
            if r > q && r + 2 <= n {
                let a = lower(src[r] as u32);
                let b = lower(src[r + 1] as u32);
                let kind = match (a, b) {
                    (0x74, 0x62) => Some(0),
                    (0x62, 0x74) => Some(1),
                    (0x72, 0x6c) => Some(2),
                    (0x6c, 0x72) => Some(3),
                    _ => None,
                };
                if let Some(k) = kind {
                    out.push((k, p, r + 2));
                    p = r + 2;
                    continue;
                }
            }
        }
        p += 1;
    }
    out
}

struct Lexer<'a> {
    src: &'a [u16],
    n: usize,
    types: Vec<u8>,
    texts: Vec<String>,
    starts: Vec<usize>,
    stack: Vec<S>,
    state: S,
    p: usize,
    dir_starts: [Vec<usize>; 4],
    dir_ends: [Vec<usize>; 4],
    dir_seen: [usize; 4],
    any_dir: bool,
    line_end: isize,
}

impl<'a> Lexer<'a> {
    fn c(&self, i: usize) -> u32 {
        self.src.get(i).map_or(NONE, |&c| c as u32)
    }

    fn emit(&mut self, type_: u8, s: usize, e: usize) {
        self.types.push(type_);
        self.texts.push(String::from_utf16_lossy(&self.src[s..e]));
        self.starts.push(s);
        self.p = e;
    }

    fn push(&mut self, s: S) {
        self.stack.push(self.state);
        self.state = s;
    }

    fn pop(&mut self) {
        if let Some(s) = self.stack.pop() {
            self.state = s;
        }
    }

    fn index_of(&self, c: u16, from: usize) -> Option<usize> {
        if from >= self.n {
            return None;
        }
        self.src[from..]
            .iter()
            .position(|&x| x == c)
            .map(|k| k + from)
    }

    fn line_break(&self, q: usize) -> usize {
        self.index_of(b'\n' as u16, q).unwrap_or(self.n)
    }

    /// `.*direction\s+XX[^\n]*`: この行で始まる最後の向きの文が字句を決める。
    fn direction(&mut self) -> bool {
        if self.line_end < self.p as isize {
            let mut q = self.p;
            while q < self.n && !is_line_end(self.c(q)) {
                q += 1;
            }
            self.line_end = q as isize;
            for k in 0..4 {
                let list = &self.dir_starts[k];
                let mut j = self.dir_seen[k];
                while j < list.len() && list[j] as isize <= self.line_end {
                    j += 1;
                }
                self.dir_seen[k] = j;
            }
        }
        for k in 0..4 {
            let j = self.dir_seen[k];
            if j >= 1 && self.dir_starts[k][j - 1] >= self.p {
                let e = self.line_break(self.dir_ends[k][j - 1]);
                self.emit(t::direction_tb + k as u8, self.p, e);
                return true;
            }
        }
        false
    }

    fn initial(&mut self, c: u32) -> bool {
        let p = self.p;
        let n = self.n;
        let low = c | 32;
        if low == 116 && word_at(self.src, p + 1, "itle") && is_space(self.c(p + 5)) {
            let mut q = p + 6;
            while q < n {
                let d = self.c(q);
                if d == 35 || d == 10 || d == 59 {
                    break;
                }
                q += 1;
            }
            if q > p + 6 {
                self.emit(t::title, p, q);
                return true;
            }
        }
        if low == 97 && (self.c(p + 1) | 32) == 99 && (self.c(p + 2) | 32) == 99 {
            if let Some(e) = acc_at(self.src, p, "acctitle", b':' as u16) {
                self.push(S::AccTitle);
                self.emit(t::acc_title, p, e);
                return true;
            }
            if let Some(e) = acc_at(self.src, p, "accdescr", b':' as u16) {
                self.push(S::AccDescr);
                self.emit(t::acc_descr, p, e);
                return true;
            }
            if let Some(e) = acc_at(self.src, p, "accdescr", b'{' as u16) {
                self.push(S::AccDescrMultiline);
                self.p = e;
                return true;
            }
        }
        if self.any_dir && !is_line_end(c) && self.direction() {
            return true;
        }

        if c == 10 || (c == 13 && self.c(p + 1) == 10) {
            let mut q = p;
            while q < n {
                let d = self.c(q);
                if d == 10 {
                    q += 1;
                } else if d == 13 && self.c(q + 1) == 10 {
                    q += 2;
                } else {
                    break;
                }
            }
            self.emit(t::NEWLINE, p, q);
            return true;
        }
        if is_space(c) {
            self.p = spaces(self.src, p + 1);
            return true;
        }
        if c == 35 || c == 37 {
            self.p = self.line_break(p);
            return true;
        }

        if is_word(c) {
            let mut q = p + 1;
            while q < n && is_word(self.c(q)) {
                q += 1;
            }
            if q - p <= LONGEST_KEYWORD {
                let lowered: String = self.src[p..q]
                    .iter()
                    .map(|&u| (lower(u as u32) as u8) as char)
                    .collect();
                if let Some(keyword) = t::keyword(&lowered) {
                    if keyword == t::STYLE || keyword == t::CLASSDEF || keyword == t::CLASS {
                        self.push(S::Style);
                    }
                    self.emit(keyword, p, q);
                    return true;
                }
            }
            while q < n {
                let d = self.c(q);
                if d < 128 && stop(d) {
                    break;
                }
                q += 1;
            }
            self.types.push(t::unqString);
            self.texts
                .push(crate::js::trim(&String::from_utf16_lossy(&self.src[p..q])).to_string());
            self.starts.push(p);
            self.p = q;
            return true;
        }

        match c {
            123 => self.emit(t::STRUCT_START, p, p + 1),
            125 => self.emit(t::STRUCT_STOP, p, p + 1),
            58 => {
                if self.c(p + 1) == 58 && self.c(p + 2) == 58 {
                    self.emit(t::STYLE_SEPARATOR, p, p + 3);
                } else {
                    self.emit(t::COLONSEP, p, p + 1);
                }
            }
            60 => {
                if self.c(p + 1) != 45 {
                    return false;
                }
                self.emit(t::END_ARROW_L, p, p + 2);
            }
            45 => {
                if self.c(p + 1) == 62 {
                    self.emit(t::END_ARROW_R, p, p + 2);
                } else {
                    self.emit(t::LINE, p, p + 1);
                }
            }
            34 => {
                self.push(S::String);
                self.p += 1;
            }
            44 => self.emit(t::COMMA, p, p + 1),
            _ => return false,
        }
        true
    }

    fn style(&mut self, c: u32) -> bool {
        let p = self.p;
        if is_word(c) {
            let mut q = p + 1;
            while q < self.n && is_word(self.c(q)) {
                q += 1;
            }
            self.emit(t::ALPHA, p, q);
            return true;
        }
        match c {
            58 => self.emit(t::COLON, p, p + 1),
            59 => self.emit(t::SEMICOLON, p, p + 1),
            37 => self.emit(t::PERCENT, p, p + 1),
            45 => self.emit(t::MINUS, p, p + 1),
            35 => self.emit(t::BRKT, p, p + 1),
            32 => self.p += 1,
            34 => {
                self.push(S::String);
                self.p += 1;
            }
            10 => {
                self.pop();
                self.p += 1;
            }
            44 => self.emit(t::COMMA, p, p + 1),
            _ => return false,
        }
        true
    }
}

pub fn tokenize(src: &[u16]) -> Tokens {
    let n = src.len();
    let mut dir_starts: [Vec<usize>; 4] = Default::default();
    let mut dir_ends: [Vec<usize>; 4] = Default::default();
    let found = directions(src);
    for &(k, s, e) in &found {
        dir_starts[k].push(s);
        dir_ends[k].push(e);
    }
    let mut lx = Lexer {
        src,
        n,
        types: Vec::new(),
        texts: Vec::new(),
        starts: Vec::new(),
        stack: Vec::new(),
        state: S::Initial,
        p: 0,
        dir_starts,
        dir_ends,
        dir_seen: [0; 4],
        any_dir: !found.is_empty(),
        line_end: -1,
    };

    while lx.p < n {
        let c = lx.c(lx.p);
        let p = lx.p;
        let ok = match lx.state {
            S::Initial => lx.initial(c),
            S::Style => lx.style(c),
            S::String => {
                if c == 34 {
                    lx.pop();
                    lx.p += 1;
                } else {
                    let e = lx.index_of(b'"' as u16, p).unwrap_or(n);
                    lx.emit(t::qString, p, e);
                }
                true
            }
            S::AccTitle => {
                lx.pop();
                let e = lx.line_break(p);
                lx.emit(t::acc_title_value, p, e);
                true
            }
            S::AccDescr => {
                lx.pop();
                let e = lx.line_break(p);
                lx.emit(t::acc_descr_value, p, e);
                true
            }
            S::AccDescrMultiline => {
                if c == 125 {
                    lx.pop();
                    lx.p += 1;
                } else {
                    let e = lx.index_of(b'}' as u16, p).unwrap_or(n);
                    lx.emit(t::acc_descr_multiline_value, p, e);
                }
                true
            }
        };
        if !ok {
            lx.types.push(t::ERROR);
            lx.texts.push(String::new());
            lx.starts.push(lx.p);
            return Tokens {
                types: lx.types,
                texts: lx.texts,
                starts: lx.starts,
            };
        }
    }

    // 入力の終わりで、Jison は何にも合わなくてよい規則をもう 1 度だけ試す。
    let last = match lx.state {
        S::Initial => Some(t::EOF),
        S::String => Some(t::qString),
        S::AccTitle => Some(t::acc_title_value),
        S::AccDescr => Some(t::acc_descr_value),
        S::AccDescrMultiline => Some(t::acc_descr_multiline_value),
        S::Style => None,
    };
    if let Some(last) = last {
        lx.emit(last, n, n);
    }
    lx.emit(t::END, n, n);
    Tokens {
        types: lx.types,
        texts: lx.texts,
        starts: lx.starts,
    }
}
