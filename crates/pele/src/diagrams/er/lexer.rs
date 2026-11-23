//! er の字句解析（pele の `src/diagrams/er/lexer.ts`）。JS と同じく UTF-16 の符号単位で読む。粘着の正規表現は、
//! 同じ所に合う手書きの走査に置き換えた。

use super::tokens as t;
use crate::util::chars::{NONE, is_digit, is_line_end, is_space, is_word};

pub struct Tokens {
    pub types: Vec<u8>,
    pub texts: Vec<String>,
    pub starts: Vec<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum S {
    Initial,
    Block,
    BlockBq,
    AccTitle,
    AccDescr,
    AccDescrMultiline,
    Style,
}

const TEXT: u8 = 1;
const STYLE_TEXT: u8 = 2;
const ATTR_START: u8 = 4;
const ATTR_REST: u8 = 8;

fn class(c: u32) -> u8 {
    if c >= 128 {
        return 0;
    }
    let mut k = 0;
    let word = is_word(c);
    if word {
        k |= TEXT | STYLE_TEXT | ATTR_REST;
        if !is_digit(c) {
            k |= ATTR_START;
        }
    }
    match c {
        45 | 42 => k |= TEXT | STYLE_TEXT | ATTR_REST,
        46 => k |= TEXT | ATTR_REST,
        _ => {}
    }
    if c == 42 {
        k |= ATTR_START;
    }
    if matches!(c, 91 | 93 | 40 | 41 | 44) {
        k |= ATTR_REST;
    }
    k
}

/// Mermaid の属性の型は大文字小文字を問わないので、マイクロ記号も `À-￿` の範囲に入る。
fn is_attr(c: u32, kind: u8) -> bool {
    if c < 128 {
        class(c) & kind != 0
    } else {
        c >= 0xc0 || c == 0xb5
    }
}

/// `c >= 128 || CLASS[c] & kind`（終わりの先は当たらない）。
fn wide_or(c: u32, kind: u8) -> bool {
    c != NONE && (c >= 128 || class(c) & kind != 0)
}

const DIRECTIONS: [&str; 4] = ["tb", "bt", "rl", "lr"];

struct Lexer<'a> {
    src: &'a [u16],
    n: usize,
    types: Vec<u8>,
    texts: Vec<String>,
    starts: Vec<usize>,
    state: S,
    p: usize,
    dir_next: isize,
    dir_failed: usize,
    line_end: isize,
    tilde_next: isize,
    tilde_line: isize,
    tilde_last: isize,
    ws_next: isize,
}

impl Lexer<'_> {
    fn c(&self, i: usize) -> u32 {
        self.src.get(i).map_or(NONE, |&c| c as u32)
    }

    fn emit(&mut self, type_: u8, s: usize, e: usize) {
        self.types.push(type_);
        self.texts.push(String::from_utf16_lossy(&self.src[s..e]));
        self.starts.push(s);
        self.p = e;
    }

    /// q に小文字の語が、大文字小文字を問わず合うか。
    fn kw_at(&self, word: &str, q: usize) -> bool {
        for (i, k) in word.bytes().enumerate() {
            let k = k as u32;
            let c = self.c(q + i);
            if c != k && (!(97..=122).contains(&k) || (c | 32) != k) {
                return false;
            }
        }
        true
    }

    fn kw(&self, word: &str) -> bool {
        self.kw_at(word, self.p)
    }

    fn kwb(&self, word: &str) -> bool {
        self.kw(word) && !is_word(self.c(self.p + word.len()))
    }

    fn lower(&self, q: usize) -> u32 {
        let c = self.c(q);
        if (65..=90).contains(&c) { c + 32 } else { c }
    }

    fn next_line_end(&self, q: usize) -> usize {
        let mut k = q;
        while k < self.n && !is_line_end(self.c(k)) {
            k += 1;
        }
        k
    }

    fn digits(&self, mut q: usize) -> usize {
        while is_digit(self.c(q)) {
            q += 1;
        }
        q
    }

    fn spaces(&self, mut q: usize) -> usize {
        while q < self.n && is_space(self.c(q)) {
            q += 1;
        }
        q
    }

    fn index_of(&self, ch: u16, q: usize) -> Option<usize> {
        if q >= self.n {
            return None;
        }
        self.src[q..].iter().position(|&x| x == ch).map(|k| k + q)
    }

    /// `/accTitle\s*:\s*/iy` と `/accDescr\s*:\s*/iy`・`/accDescr\s*\{\s*/iy`。
    fn acc_at(&self, word: &str, open: u32) -> Option<usize> {
        if !self.kw(word) {
            return None;
        }
        let q = self.spaces(self.p + 8);
        (self.c(q) == open).then(|| self.spaces(q + 1))
    }

    /// `/.*direction\s+TB[^\n]*/iy` とその兄弟: 行の終わりの文字までで、いちばん後ろの `direction` から。
    fn direction_at(&self, q: usize, i: usize) -> Option<usize> {
        let stop = self.next_line_end(q);
        let mut k = stop;
        while k > q {
            k -= 1;
            if k + 9 <= stop && self.kw_at("direction", k) {
                let s = k + 9;
                let e = self.spaces(s);
                if e > s && self.kw_at(DIRECTIONS[i], e) {
                    let mut end = e + 2;
                    while end < self.n && self.c(end) != 10 {
                        end += 1;
                    }
                    return Some(end);
                }
            }
        }
        None
    }

    /// 向きの文は、字句を読む所がその行のどこにあっても、行を丸ごと取る。
    fn direction(&mut self) -> bool {
        let p = self.p;
        if self.dir_next == -1 {
            return false;
        }
        if self.dir_next < p as isize {
            self.dir_next = (p..self.n)
                .find(|&k| self.kw_at("direction", k))
                .map_or(-1, |k| k as isize);
            if self.dir_next == -1 {
                return false;
            }
        }
        if p < self.dir_failed {
            return false;
        }
        if self.line_end < p as isize {
            self.line_end = self.next_line_end(p) as isize;
        }
        if self.dir_next >= self.line_end {
            return false;
        }
        for i in 0..4 {
            if let Some(e) = self.direction_at(p, i) {
                self.emit(t::direction_tb + i as u8, p, e);
                return true;
            }
        }
        self.dir_failed = self.line_end as usize;
        false
    }

    fn quoted(&mut self) {
        let p = self.p;
        let Some(e) = self.index_of(34, p + 1) else {
            self.emit(t::CHAR, p, p + 1);
            return;
        };
        let name =
            e > p + 1 && (p + 1..e).all(|q| !matches!(self.c(q), 37 | 92 | 13 | 10 | 11 | 8));
        self.emit(if name { t::ENTITY_NAME } else { t::WORD }, p, e + 1);
    }

    fn number(&mut self, c: u32) {
        let p = self.p;
        let d = self.c(p + 1);
        if d == 43 && (c == 49 || c == 48) {
            self.emit(
                if c == 49 {
                    t::ONE_OR_MORE
                } else {
                    t::ZERO_OR_MORE
                },
                p,
                p + 2,
            );
            return;
        }
        let q = self.digits(p + 1);
        if self.c(q) == 46 && is_digit(self.c(q + 1)) {
            let e = self.digits(q + 2);
            self.emit(t::DECIMAL_NUM, p, e);
            return;
        }
        if c == 49 {
            if is_space(d) {
                let mut r = p + 2;
                while is_space(self.c(r)) {
                    r += 1;
                }
                let f = self.c(r);
                if is_word(f) || f == 34 || f == 39 {
                    self.emit(t::ONLY_ONE, p, p + 1);
                    return;
                }
            } else if d == 45 || d == 46 {
                let f = self.c(p + 2);
                if f == 45 || f == 46 {
                    self.emit(t::ONLY_ONE, p, p + 1);
                    return;
                }
            }
            if !is_word(d) {
                self.emit(t::ENTITY_ONE, p, p + 1);
                return;
            }
        }
        self.emit(t::NUM, p, q);
    }

    fn keyword(&mut self, lc: u32) -> bool {
        let p = self.p;
        let d = self.c(p + 1);
        match lc {
            101 => {
                if self.kwb("erdiagram") {
                    self.emit(t::ER_DIAGRAM, p, p + 9);
                } else if self.kwb("end") {
                    let e = self.spaces(p + 3);
                    self.emit(t::END_KW, p, e);
                } else {
                    return false;
                }
                true
            }
            115 => {
                if self.kwb("style") {
                    self.state = S::Style;
                    self.emit(t::STYLE, p, p + 5);
                } else if self.kwb("subgraph") {
                    self.emit(t::SUBGRAPH, p, p + 8);
                } else {
                    return false;
                }
                true
            }
            99 => {
                if self.kwb("classdef") {
                    self.state = S::Style;
                    self.emit(t::CLASSDEF, p, p + 8);
                } else if self.kwb("class") {
                    self.emit(t::CLASS, p, p + 5);
                } else {
                    return false;
                }
                true
            }
            111 => {
                if self.kwb("one or zero") {
                    self.emit(t::ZERO_OR_ONE, p, p + 11);
                } else if self.kwb("one or more") || self.kwb("one or many") {
                    self.emit(t::ONE_OR_MORE, p, p + 11);
                } else if self.kwb("one") {
                    self.emit(t::ONLY_ONE, p, p + 3);
                } else if self.kwb("only one") {
                    self.emit(t::ONLY_ONE, p, p + 8);
                } else if d == 124 {
                    self.emit(t::ZERO_OR_ONE, p, p + 2);
                } else if d == 123 {
                    self.emit(t::ZERO_OR_MORE, p, p + 2);
                } else if self.kwb("optionally to") {
                    self.emit(t::NON_IDENTIFYING, p, p + 13);
                } else {
                    return false;
                }
                true
            }
            122 => {
                if self.kwb("zero or one") {
                    self.emit(t::ZERO_OR_ONE, p, p + 11);
                } else if self.kwb("zero or more") || self.kwb("zero or many") {
                    self.emit(t::ZERO_OR_MORE, p, p + 12);
                } else {
                    return false;
                }
                true
            }
            109 => {
                if self.kw("many(0)") {
                    self.emit(t::ZERO_OR_MORE, p, p + 7);
                } else if self.kw("many(1)") {
                    self.emit(t::ONE_OR_MORE, p, p + 7);
                } else if self.kwb("many") {
                    self.emit(t::ZERO_OR_MORE, p, p + 4);
                } else {
                    return false;
                }
                true
            }
            116 => {
                if !self.kwb("to") {
                    return false;
                }
                self.emit(t::IDENTIFYING, p, p + 2);
                true
            }
            117 => {
                if d != 46 && d != 45 && d != 124 {
                    return false;
                }
                self.emit(t::MD_PARENT, p, p + 1);
                true
            }
            124 => {
                if self.lower(p + 1) == 111 && !is_word(self.c(p + 2)) {
                    self.emit(t::ZERO_OR_ONE, p, p + 2);
                } else if d == 124 {
                    self.emit(t::ONLY_ONE, p, p + 2);
                } else if d == 123 {
                    self.emit(t::ONE_OR_MORE, p, p + 2);
                } else {
                    return false;
                }
                true
            }
            125 => {
                if self.lower(p + 1) == 111 && !is_word(self.c(p + 2)) {
                    self.emit(t::ZERO_OR_MORE, p, p + 2);
                } else if d == 124 {
                    self.emit(t::ONE_OR_MORE, p, p + 2);
                } else {
                    return false;
                }
                true
            }
            46 => {
                if d != 46 && d != 45 {
                    return false;
                }
                self.emit(t::NON_IDENTIFYING, p, p + 2);
                true
            }
            45 => {
                if d == 45 {
                    self.emit(t::IDENTIFYING, p, p + 2);
                } else if d == 46 {
                    self.emit(t::NON_IDENTIFYING, p, p + 2);
                } else {
                    return false;
                }
                true
            }
            _ => false,
        }
    }

    fn initial(&mut self, c: u32) {
        let p = self.p;
        if (c == 97 || c == 65) && self.kw("acc") {
            if let Some(e) = self.acc_at("acctitle", 58) {
                self.state = S::AccTitle;
                self.emit(t::acc_title, p, e);
                return;
            }
            if let Some(e) = self.acc_at("accdescr", 58) {
                self.state = S::AccDescr;
                self.emit(t::acc_descr, p, e);
                return;
            }
            if let Some(e) = self.acc_at("accdescr", 123) {
                self.state = S::AccDescrMultiline;
                self.p = e;
                return;
            }
        }
        if self.direction() {
            return;
        }
        match c {
            32 | 9 | 13 => {
                let mut e = p + 1;
                while matches!(self.c(e), 32 | 9 | 13) {
                    e += 1;
                }
                self.p = e;
                return;
            }
            10 => {
                let mut e = p + 1;
                while self.c(e) == 10 {
                    e += 1;
                }
                self.emit(t::NEWLINE, p, e);
                return;
            }
            34 => return self.quoted(),
            123 => {
                self.state = S::Block;
                return self.emit(t::BLOCK_START, p, p + 1);
            }
            35 => return self.emit(t::BRKT, p, p + 1),
            44 => return self.emit(t::COMMA, p, p + 1),
            58 => {
                if self.c(p + 1) == 58 && self.c(p + 2) == 58 {
                    self.emit(t::STYLE_SEPARATOR, p, p + 3);
                } else {
                    self.emit(t::COLON, p, p + 1);
                }
                return;
            }
            91 => return self.emit(t::SQS, p, p + 1),
            93 => return self.emit(t::SQE, p, p + 1),
            _ => {}
        }
        if is_digit(c) {
            return self.number(c);
        }
        if self.keyword(if (65..=90).contains(&c) { c + 32 } else { c }) {
            return;
        }
        if wide_or(c, TEXT) {
            let mut e = p + 1;
            while wide_or(self.c(e), TEXT) {
                e += 1;
            }
            return self.emit(t::UNICODE_TEXT, p, e);
        }
        self.emit(t::CHAR, p, p + 1);
    }

    /// Mermaid の総称の型の規則: `~` を含む空白でない文字の並びを、行の最後の `~` まで。
    fn generic(&mut self) -> bool {
        let p = self.p as isize;
        if self.tilde_next == -1 {
            return false;
        }
        if self.tilde_next < p {
            self.tilde_next = self.index_of(126, self.p).map_or(-1, |k| k as isize);
            if self.tilde_next == -1 {
                return false;
            }
        }
        if self.ws_next < p {
            let mut q = self.p;
            while q < self.n && !is_space(self.c(q)) {
                q += 1;
            }
            self.ws_next = q as isize;
        }
        if self.tilde_next >= self.ws_next {
            return false;
        }
        if self.tilde_line < p {
            self.tilde_line = self.next_line_end(self.p) as isize;
            // `lastIndexOf('~', tildeLine - 1)`
            let from = (self.tilde_line - 1).max(0) as usize;
            self.tilde_last = (0..=from.min(self.n.saturating_sub(1)))
                .rev()
                .find(|&k| self.n > 0 && self.src[k] == 126)
                .map_or(-1, |k| k as isize);
        }
        if self.tilde_last <= self.tilde_next {
            return false;
        }
        let mut e = self.tilde_last as usize + 1;
        while e < self.n && !is_space(self.c(e)) {
            e += 1;
        }
        self.emit(t::ATTRIBUTE_WORD, self.p, e);
        true
    }

    fn block(&mut self, c: u32) {
        let p = self.p;
        if is_space(c) {
            self.p = self.spaces(p + 1);
            return;
        }
        let lc = c | 32;
        if (lc == 112 || lc == 102 || lc == 117)
            && (self.c(p + 1) | 32) == 107
            && !is_word(self.c(p + 2))
        {
            return self.emit(t::ATTRIBUTE_KEY, p, p + 2);
        }
        if self.generic() {
            return;
        }
        if is_attr(c, ATTR_START) {
            let mut e = p + 1;
            while e < self.n && is_attr(self.c(e), ATTR_REST) {
                e += 1;
            }
            return self.emit(t::ATTRIBUTE_WORD, p, e);
        }
        if c == 96 {
            self.state = S::BlockBq;
            self.p += 1;
            return;
        }
        if c == 34 {
            if let Some(e) = self.index_of(34, p + 1) {
                return self.emit(t::COMMENT, p, e + 1);
            }
        } else if c == 125 {
            self.state = S::Initial;
            return self.emit(t::BLOCK_STOP, p, p + 1);
        }
        self.emit(t::CHAR, p, p + 1);
    }
}

pub fn tokenize(src: &[u16]) -> Tokens {
    let mut lx = Lexer {
        src,
        n: src.len(),
        types: Vec::new(),
        texts: Vec::new(),
        starts: Vec::new(),
        state: S::Initial,
        p: 0,
        dir_next: -2,
        dir_failed: 0,
        line_end: -1,
        tilde_next: -2,
        tilde_line: -1,
        tilde_last: -1,
        ws_next: -1,
    };
    while lx.p < lx.n {
        let p = lx.p;
        let c = lx.c(p);
        match lx.state {
            S::Initial => lx.initial(c),
            S::Block => lx.block(c),
            S::BlockBq => {
                if c == 96 {
                    lx.state = S::Block;
                    lx.p += 1;
                } else {
                    let e = lx.index_of(96, p).unwrap_or(lx.n);
                    lx.emit(t::ATTRIBUTE_WORD, p, e);
                }
            }
            S::AccTitle | S::AccDescr => {
                let e = lx.index_of(10, p).unwrap_or(lx.n);
                let type_ = if lx.state == S::AccTitle {
                    t::acc_title_value
                } else {
                    t::acc_descr_value
                };
                lx.emit(type_, p, e);
                lx.state = S::Initial;
            }
            S::AccDescrMultiline => {
                if c == 125 {
                    lx.state = S::Initial;
                    lx.p += 1;
                } else {
                    let e = lx.index_of(125, p).unwrap_or(lx.n);
                    lx.emit(t::acc_descr_multiline_value, p, e);
                }
            }
            S::Style => {
                if c == 10 {
                    let mut e = p + 1;
                    while lx.c(e) == 10 {
                        e += 1;
                    }
                    lx.state = S::Initial;
                    lx.emit(t::NEWLINE, p, e);
                } else if is_space(c) {
                    lx.p = lx.spaces(p + 1);
                } else if c == 58 {
                    lx.emit(t::COLON, p, p + 1);
                } else if c == 44 {
                    lx.emit(t::COMMA, p, p + 1);
                } else if c == 35 {
                    lx.emit(t::BRKT, p, p + 1);
                } else if c == 59 {
                    lx.emit(t::SEMI, p, p + 1);
                } else if wide_or(c, STYLE_TEXT) {
                    let mut e = p + 1;
                    while wide_or(lx.c(e), STYLE_TEXT) {
                        e += 1;
                    }
                    lx.emit(t::STYLE_TEXT, p, e);
                } else {
                    lx.types.push(t::ERROR);
                    lx.texts.push(String::new());
                    lx.starts.push(p);
                    return Tokens {
                        types: lx.types,
                        texts: lx.texts,
                        starts: lx.starts,
                    };
                }
            }
        }
    }

    // 入力の終わりで Jison の INITIAL は EOF を、題と説明の状態は空の値を出し、属性の塊と style の状態は何も出さない。
    let n = lx.n;
    let last = match lx.state {
        S::Initial => Some(t::EOF),
        S::AccTitle => Some(t::acc_title_value),
        S::AccDescr => Some(t::acc_descr_value),
        S::AccDescrMultiline => Some(t::acc_descr_multiline_value),
        _ => None,
    };
    if let Some(ty) = last {
        lx.types.push(ty);
        lx.texts.push(String::new());
        lx.starts.push(n);
    }
    lx.types.push(t::END);
    lx.texts.push(String::new());
    lx.starts.push(n);
    Tokens {
        types: lx.types,
        texts: lx.texts,
        starts: lx.starts,
    }
}
