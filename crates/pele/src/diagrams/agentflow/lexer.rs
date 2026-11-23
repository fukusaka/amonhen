//! agentflow の字句解析（pele の `src/diagrams/agentflow/lexer.ts`）。JS と同じく UTF-16 の符号単位で読む。
//! 粘着（`y`）の正規表現は、同じ所に合う手書きの走査に置き換えた。

use super::tokens as t;
use crate::js;
use crate::util::chars::{NONE, is_space, is_word};
use crate::util::unicode::is_unicode_letter;

pub struct Tokens {
    pub types: Vec<u8>,
    pub texts: Vec<String>,
    pub starts: Vec<usize>,
    /// 字句ごとに 4 つ、Jison の数え方で: 始まりの行と列、終わりの行と列。行は 1 から、列は 0 から。
    pub locs: Vec<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum S {
    Initial,
    String,
    MdString,
    AccTitle,
    AccDescr,
    AccDescrMultiline,
    Dir,
    Text,
    EllipseText,
    TrapText,
    EdgeText,
    Click,
    Callbackname,
    Callbackargs,
    ShapeData,
    ShapeDataStr,
}

fn node_char(c: u32) -> bool {
    c < 128 && {
        let ch = c as u8;
        ch.is_ascii_alphanumeric() || b"!\"#$%&'*+.`?\\_/".contains(&ch)
    }
}

const DIRECTIONS: [&str; 5] = ["TB", "BT", "RL", "LR", "TD"];

/// どの位置とも違う値（TS の -1）。
const NO: usize = usize::MAX;

struct Lexer<'a> {
    src: &'a [u16],
    n: usize,
    types: Vec<u8>,
    texts: Vec<String>,
    starts: Vec<usize>,
    locs: Vec<usize>,
    stack: Vec<S>,
    state: S,
    p: usize,
    line: usize,
    col: usize,
    first_graph: bool,
    dir_next: Option<usize>,
    at_next: Option<usize>,
    line_end: Option<usize>,
    dir_failed: usize,
    run_end: usize,
    run_at: Option<usize>,
    ws_from: usize,
    ws_to: usize,
    blank_from: usize,
    blank_to: usize,
    no_link: usize,
    no_arrow: usize,
}

impl<'a> Lexer<'a> {
    fn c(&self, i: usize) -> u32 {
        self.src.get(i).map_or(NONE, |&c| c as u32)
    }

    fn starts_with(&self, word: &str, at: usize) -> bool {
        word.bytes()
            .enumerate()
            .all(|(i, b)| self.c(at + i) == b as u32)
    }

    fn index_of(&self, word: &str, from: usize) -> Option<usize> {
        let w: Vec<u16> = word.encode_utf16().collect();
        if self.n < w.len() || from > self.n - w.len() {
            return None;
        }
        (from..=self.n - w.len()).find(|&k| self.src[k..k + w.len()] == w[..])
    }

    fn index_of_unit(&self, c: u16, from: usize) -> Option<usize> {
        self.src
            .get(from..)?
            .iter()
            .position(|&x| x == c)
            .map(|k| k + from)
    }

    fn slice(&self, a: usize, b: usize) -> String {
        js::from16(&self.src[a.min(self.n)..b.min(self.n).max(a.min(self.n))])
    }

    /// 1 つの一致の上を進む。Jison は \r\n・\r・\n をそれぞれ 1 つの改行と数え、改行の後の列は最初の U+2028 か
    /// U+2029 で止まる。
    fn advance(&mut self, e: usize) {
        let mut brk: Option<usize> = None;
        let mut k = self.p;
        while k < e {
            let ch = self.c(k);
            if ch == 10 {
                self.line += 1;
                brk = Some(k);
            } else if ch == 13 {
                if k + 1 < e && self.c(k + 1) == 10 {
                    k += 1;
                }
                self.line += 1;
                brk = Some(k);
            }
            k += 1;
        }
        match brk {
            None => self.col += e - self.p,
            Some(brk) => {
                let mut k = brk + 1;
                while k < e {
                    let ch = self.c(k);
                    if ch == 0x2028 || ch == 0x2029 {
                        break;
                    }
                    k += 1;
                }
                self.col = k - brk - 1;
            }
        }
        self.p = e;
    }

    fn open(&mut self, type_: u8, text: String) {
        self.types.push(type_);
        self.texts.push(text);
        self.starts.push(self.p);
        self.locs.push(self.line);
        self.locs.push(self.col);
    }

    fn close(&mut self) {
        self.locs.push(self.line);
        self.locs.push(self.col);
    }

    fn emit(&mut self, type_: u8, e: usize) {
        let text = self.slice(self.p, e);
        self.open(type_, text);
        self.advance(e);
        self.close();
    }

    /// 改行を含みえない字句。
    fn emit_flat(&mut self, type_: u8, e: usize) {
        let text = self.slice(self.p, e);
        self.open(type_, text);
        self.col += e - self.p;
        self.p = e;
        self.close();
    }

    /// Jison が 1 文字ずつ合わせる並び。\r\n は 2 つの改行と数える。
    fn emit_chars(&mut self, type_: u8, e: usize) {
        let text = self.slice(self.p, e);
        self.open(type_, text);
        for k in self.p..e {
            let ch = self.c(k);
            if ch == 10 || ch == 13 {
                self.line += 1;
                self.col = 0;
            } else {
                self.col += 1;
            }
        }
        self.p = e;
        self.close();
    }

    fn emit_text(&mut self, type_: u8, text: String, e: usize) {
        self.open(type_, text);
        self.advance(e);
        self.close();
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

    fn spaces(&self, mut q: usize) -> usize {
        while q < self.n && is_space(self.c(q)) {
            q += 1;
        }
        q
    }

    fn kw(&self, word: &str) -> bool {
        self.starts_with(word, self.p) && !is_word(self.c(self.p + word.len()))
    }

    /// `word\s*:\s*` などの `word\s*X\s*`。
    fn word_then(&self, word: &str, q: usize, mark: u32) -> Option<usize> {
        if !self.starts_with(word, q) {
            return None;
        }
        let e = self.spaces(q + word.len());
        (self.c(e) == mark).then(|| self.spaces(e + 1))
    }

    /// `word\s+`
    fn word_spaces(&self, word: &str, q: usize) -> Option<usize> {
        if !self.starts_with(word, q) {
            return None;
        }
        let s = q + word.len();
        let e = self.spaces(s);
        (e > s).then_some(e)
    }

    /// `(?:\r?\n)*\s*\n`: 空白の並びのうち、最後の \n の後ろまで。
    fn no_dir(&self, q: usize) -> Option<usize> {
        let e = self.spaces(q);
        (q..e).rev().find(|&k| self.c(k) == 10).map(|k| k + 1)
    }

    /// `\s*(?:LR|RL|TB|BT|TD|BR)\b`
    fn dir(&self, q: usize) -> Option<usize> {
        let s = self.spaces(q);
        let pair = [self.c(s), self.c(s + 1)];
        let ok = [[76, 82], [82, 76], [84, 66], [66, 84], [84, 68], [66, 82]].contains(&pair);
        (ok && !is_word(self.c(s + 2))).then_some(s + 2)
    }

    /// `\s*[<>^]`
    fn dir_sym(&self, q: usize) -> Option<usize> {
        let s = self.spaces(q);
        matches!(self.c(s), 60 | 62 | 94).then_some(s + 1)
    }

    /// `\s*v\b`
    fn dir_v(&self, q: usize) -> Option<usize> {
        let s = self.spaces(q);
        (self.c(s) == 118 && !is_word(self.c(s + 1))).then_some(s + 1)
    }

    /// `.*direction\s+XX[^\n]*`: 行の終わりの文字までで、いちばん後ろの `direction` から。
    fn direction(&self, i: usize, q: usize) -> Option<usize> {
        let mut stop = q;
        while stop < self.n && !matches!(self.c(stop), 10 | 13 | 0x2028 | 0x2029) {
            stop += 1;
        }
        let dir: Vec<u16> = DIRECTIONS[i].encode_utf16().collect();
        let mut k = stop;
        while k > q {
            k -= 1;
            if k + 9 <= stop && self.starts_with("direction", k) {
                let s = k + 9;
                let e = self.spaces(s);
                if e > s && self.n >= e + 2 && self.src[e..e + 2] == dir[..] {
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

    /// `\(\s*\)`
    fn empty_call(&self, q: usize) -> Option<usize> {
        if self.c(q) != 40 {
            return None;
        }
        let e = self.spaces(q + 1);
        (self.c(e) == 41).then_some(e + 1)
    }

    /// q からの空白の並びの終わり。最後の並びを覚え、その中から聞き直しても手間がかからない。
    fn ws_end(&mut self, q: usize) -> usize {
        if self.ws_from != NO && q >= self.ws_from && q <= self.ws_to {
            return self.ws_to;
        }
        let j = self.spaces(q);
        self.ws_from = q;
        self.ws_to = j;
        j
    }

    fn blank_end(&mut self, q: usize) -> usize {
        if self.blank_from != NO && q >= self.blank_from && q <= self.blank_to {
            return self.blank_to;
        }
        let mut j = q;
        while j < self.n {
            let ch = self.c(j);
            if ch != 32 && ch != 9 {
                break;
            }
            j += 1;
        }
        self.blank_from = q;
        self.blank_to = j;
        j
    }

    /// q からの `\s*--+[x>]\s*` の終わり。
    fn arrow_end(&mut self, q: usize) -> Option<usize> {
        let j = self.ws_end(q);
        if j == self.no_arrow || self.c(j) != 45 || self.c(j + 1) != 45 {
            return None;
        }
        let mut k = j + 2;
        while self.c(k) == 45 {
            k += 1;
        }
        let tc = self.c(k);
        if tc == 120 || tc == 62 {
            return Some(self.ws_end(k + 1));
        }
        self.no_arrow = j;
        None
    }

    fn link(&mut self) -> bool {
        let j = self.ws_end(self.p);
        if j == self.no_link || self.c(j) != 45 {
            return false;
        }
        let d = self.c(j + 1);
        let mut k = j + 2;
        if d == 45 {
            while self.c(k) == 45 {
                k += 1;
            }
            let tc = self.c(k);
            if tc == 120 || tc == 62 {
                let e = self.ws_end(k + 1);
                self.emit(t::LINK, e);
            } else {
                self.push(S::EdgeText);
                let e = self.ws_end(j + 2);
                self.emit(t::START_LINK, e);
            }
            return true;
        }
        if d == 46 {
            while self.c(k) == 46 {
                k += 1;
            }
            if self.c(k) == 45 {
                let e = self.ws_end(k + 1);
                self.emit(t::LINK, e);
                return true;
            }
        }
        self.no_link = j;
        false
    }

    /// `{` の続かない `%%` から行の終わりまで。
    fn comment_end(&self, q: usize) -> Option<usize> {
        if self.c(q) != 37 || self.c(q + 1) != 37 || self.c(q + 2) == 123 {
            return None;
        }
        Some(self.index_of_unit(10, q + 2).unwrap_or(self.n))
    }

    fn quote(&mut self) {
        if self.c(self.p + 1) == 96 {
            self.push(S::MdString);
            self.advance(self.p + 2);
        } else {
            self.push(S::String);
            self.advance(self.p + 1);
        }
    }

    fn opener(&mut self, c: u32) -> bool {
        let p = self.p;
        let d = self.c(p + 1);
        if c == 40 {
            if d == 45 {
                self.push(S::EllipseText);
                self.emit_flat(t::ELLIPSE_START, p + 2);
            } else if d == 91 {
                self.push(S::Text);
                self.emit_flat(t::STADIUMSTART, p + 2);
            } else if d == 40 && self.c(p + 2) == 40 {
                self.push(S::Text);
                self.emit_flat(t::DOUBLECIRCLESTART, p + 3);
            } else {
                self.push(S::Text);
                self.emit_flat(t::PS, p + 1);
            }
            return true;
        }
        if c == 91 {
            if d == 91 {
                self.push(S::Text);
                self.emit_flat(t::SUBROUTINESTART, p + 2);
            } else if d == 40 {
                self.push(S::Text);
                self.emit_flat(t::CYLINDERSTART, p + 2);
            } else if d == 47 {
                self.push(S::TrapText);
                self.emit_flat(t::TRAPSTART, p + 2);
            } else if d == 92 {
                self.push(S::TrapText);
                self.emit_flat(t::INVTRAPSTART, p + 2);
            } else {
                self.push(S::Text);
                self.emit_flat(t::SQS, p + 1);
            }
            return true;
        }
        if c == 124 {
            self.push(S::Text);
            self.emit_flat(t::PIPE, p + 1);
            return true;
        }
        if c == 123 {
            self.push(S::Text);
            self.emit_flat(t::DIAMOND_START, p + 1);
            return true;
        }
        false
    }

    fn node_string(&mut self) -> bool {
        let mut q = self.p;
        while q < self.n {
            let ch = self.c(q);
            if node_char(ch) {
                q += 1;
            } else if ch == 45 && q + 1 < self.n {
                let d = self.c(q + 1);
                if d == 62 || d == 45 || d == 46 {
                    break;
                }
                q += 1;
            } else {
                break;
            }
        }
        if q == self.p {
            return false;
        }
        self.emit_flat(t::NODE_STRING, q);
        true
    }

    fn blanks(&self, from: usize) -> usize {
        let mut e = from;
        while e < self.n {
            let ch = self.c(e);
            if ch != 32 && ch != 9 {
                break;
            }
            e += 1;
        }
        e
    }

    fn initial(&mut self, c: u32) -> bool {
        let p = self.p;
        match c {
            97 => {
                if self.starts_with("acc", p) {
                    if let Some(e) = self.word_then("accTitle", p, 58) {
                        self.push(S::AccTitle);
                        self.emit(t::acc_title, e);
                        return true;
                    }
                    if let Some(e) = self.word_then("accDescr", p, 58) {
                        self.push(S::AccDescr);
                        self.emit(t::acc_descr, e);
                        return true;
                    }
                    if let Some(e) = self.word_then("accDescr", p, 123) {
                        self.push(S::AccDescrMultiline);
                        self.advance(e);
                        return true;
                    }
                }
                if self.kw("agentflow-beta") {
                    if self.first_graph {
                        self.first_graph = false;
                        self.push(S::Dir);
                    }
                    self.emit_flat(t::GRAPH, p + 14);
                    return true;
                }
            }
            64 => {
                if self.c(p + 1) == 123 {
                    self.push(S::ShapeData);
                    self.emit_text(t::SHAPE_DATA, String::new(), p + 2);
                    return true;
                }
            }
            99 => {
                if let Some(e) = self.word_spaces("call", p) {
                    self.push(S::Callbackname);
                    self.advance(e);
                    return true;
                }
                if self.kw("classDef") {
                    self.emit_flat(t::CLASSDEF, p + 8);
                    return true;
                }
                if self.kw("class") {
                    self.emit_flat(t::CLASS, p + 5);
                    return true;
                }
                if let Some(e) = self.word_spaces("click", p) {
                    self.push(S::Click);
                    self.advance(e);
                    return true;
                }
                if self.kw("connector") {
                    self.emit_flat(t::connector, p + 9);
                    return true;
                }
            }
            34 => {
                self.quote();
                return true;
            }
            115 => {
                if self.kw("style") {
                    self.emit_flat(t::STYLE, p + 5);
                    return true;
                }
            }
            100 => {
                if self.kw("default") {
                    self.emit_flat(t::DEFAULT, p + 7);
                    return true;
                }
            }
            108 => {
                if self.kw("linkStyle") {
                    self.emit_flat(t::LINKSTYLE, p + 9);
                    return true;
                }
            }
            105 => {
                if self.kw("interpolate") {
                    self.emit_flat(t::INTERPOLATE, p + 11);
                    return true;
                }
            }
            104 => {
                if self.starts_with("href", p) && is_space(self.c(p + 4)) {
                    self.emit(t::HREF, p + 5);
                    return true;
                }
            }
            102 => {
                if self.kw("flow") {
                    self.emit_flat(t::flow, p + 4);
                    return true;
                }
            }
            103 => {
                if self.kw("global") {
                    let e = self.blanks(p + 6);
                    self.emit_flat(t::global, e);
                    return true;
                }
            }
            101 => {
                if self.kw("end") {
                    let e = self.blanks(p + 3);
                    self.emit_flat(t::end, e);
                    return true;
                }
            }
            95 => {
                for word in ["_self", "_blank", "_parent", "_top"] {
                    if self.kw(word) {
                        self.emit_flat(t::LINK_TARGET, p + word.len());
                        return true;
                    }
                }
            }
            _ => {}
        }

        if let Some(mut dn) = self.dir_next
            && p >= self.dir_failed
        {
            if dn < p {
                self.dir_next = self.index_of("direction", p);
            }
            if let Some(d) = self.dir_next {
                dn = d;
                if self.line_end.is_none_or(|le| le < p) {
                    let mut le = p;
                    while le < self.n && !matches!(self.c(le), 10 | 13 | 0x2028 | 0x2029) {
                        le += 1;
                    }
                    self.line_end = Some(le);
                }
                let line_end = self.line_end.unwrap();
                if dn < line_end {
                    for i in 0..5 {
                        if let Some(e) = self.direction(i, p) {
                            self.emit(t::direction_tb + i as u8, e);
                            return true;
                        }
                    }
                    // この行の残りにも方向の文は始まらない。
                    self.dir_failed = line_end;
                }
            }
        }

        // リンクの名前は、`{` か `"` の続かない最後の `@` までの、空白でない文字の並び。
        if self.at_next.is_some() && c != 34 && !is_space(c) {
            if p >= self.run_end {
                if self.at_next.is_some_and(|a| a < p) {
                    self.at_next = self.index_of_unit(64, p);
                }
                let mut q = p;
                while q < self.n {
                    let ch = self.c(q);
                    if ch == 34 || is_space(ch) {
                        break;
                    }
                    q += 1;
                }
                self.run_end = q;
                self.run_at = None;
                if let Some(at) = self.at_next.filter(|&a| a < q) {
                    let mut k = q.min(self.n - 1);
                    while k > at {
                        k -= 1;
                        if self.c(k) != 64 {
                            continue;
                        }
                        let after = self.c(k + 1);
                        if after != 123 && after != 34 {
                            self.run_at = Some(k);
                            break;
                        }
                    }
                }
            }
            if let Some(ra) = self.run_at.filter(|&r| r > p) {
                self.emit_flat(t::LINK_ID, ra + 1);
                return true;
            }
        }

        if (48..=57).contains(&c) {
            let mut q = p + 1;
            while q < self.n {
                let ch = self.c(q);
                if !(48..=57).contains(&ch) {
                    break;
                }
                q += 1;
            }
            self.emit_flat(t::NUM, q);
            return true;
        }

        match c {
            35 => {
                self.emit_flat(t::BRKT, p + 1);
                return true;
            }
            58 => {
                if self.c(p + 1) == 58 && self.c(p + 2) == 58 {
                    self.emit_flat(t::STYLE_SEPARATOR, p + 3);
                } else {
                    self.emit_flat(t::COLON, p + 1);
                }
                return true;
            }
            38 => {
                self.emit_flat(t::AMP, p + 1);
                return true;
            }
            59 => {
                self.emit_flat(t::SEMI, p + 1);
                return true;
            }
            44 => {
                self.emit_flat(t::COMMA, p + 1);
                return true;
            }
            42 => {
                self.emit_flat(t::MULT, p + 1);
                return true;
            }
            60 => {
                self.emit_flat(t::TAGSTART, p + 1);
                return true;
            }
            45 => {
                if self.link() || self.node_string() {
                    return true;
                }
                self.emit_flat(t::MINUS, p + 1);
                return true;
            }
            40 => return self.opener(c),
            91 => {
                if self.c(p + 1) == 124 {
                    self.emit_flat(t::VERTEX_WITH_PROPS_START, p + 2);
                    return true;
                }
                return self.opener(c);
            }
            124 | 123 => return self.opener(c),
            62 => {
                self.push(S::Text);
                self.emit_flat(t::TAGEND, p + 1);
                return true;
            }
            94 => {
                self.emit_flat(t::UP, p + 1);
                return true;
            }
            92 => {
                if self.c(p + 1) == 124 {
                    self.emit_flat(t::SEP, p + 2);
                    return true;
                }
                return self.node_string();
            }
            118 => {
                if !is_word(self.c(p + 1)) {
                    self.emit_flat(t::DOWN, p + 1);
                    return true;
                }
                return self.node_string();
            }
            37 => {
                if let Some(e) = self.comment_end(p) {
                    self.emit(t::COMMENT, e);
                    return true;
                }
                return self.node_string();
            }
            _ => {}
        }

        if is_space(c) {
            if self.link() {
                return true;
            }
            if c == 32 || c == 9 {
                let b = self.blank_end(p);
                if let Some(e) = self.comment_end(b) {
                    self.emit(t::COMMENT, e);
                    return true;
                }
            }
            let mut e = p;
            loop {
                let ch = self.c(e);
                if ch == 10 {
                    e += 1;
                } else if ch == 13 && self.c(e + 1) == 10 {
                    e += 2;
                } else {
                    break;
                }
            }
            if e > p {
                self.emit(t::NEWLINE, e);
            } else {
                self.emit(t::SPACE, p + 1);
            }
            return true;
        }

        if c < 128 {
            return self.node_string();
        }
        if is_unicode_letter(c) {
            self.emit_flat(t::UNICODE_TEXT, p + 1);
            return true;
        }
        false
    }

    fn step(&mut self, c: u32) -> bool {
        let p = self.p;
        match self.state {
            S::Initial => return self.initial(c),
            S::String => {
                if c == 34 {
                    if self.c(p + 1) == 96 {
                        self.push(S::MdString);
                        self.advance(p + 2);
                    } else {
                        self.pop();
                        self.advance(p + 1);
                    }
                } else {
                    let e = self.index_of_unit(34, p).unwrap_or(self.n);
                    self.emit(t::STR, e);
                }
            }
            S::MdString => {
                if c == 96 {
                    if self.c(p + 1) == 34 {
                        self.pop();
                        self.advance(p + 2);
                    } else {
                        return false;
                    }
                } else if c == 34 {
                    self.quote();
                } else {
                    let mut q = p + 1;
                    while q < self.n {
                        let ch = self.c(q);
                        if ch == 96 || ch == 34 {
                            break;
                        }
                        q += 1;
                    }
                    self.emit(t::MD_STR, q);
                }
            }
            S::AccTitle | S::AccDescr => {
                let e = self.index_of_unit(10, p).unwrap_or(self.n);
                let type_ = if self.state == S::AccTitle {
                    t::acc_title_value
                } else {
                    t::acc_descr_value
                };
                self.pop();
                self.emit(type_, e);
            }
            S::AccDescrMultiline => {
                if c == 125 {
                    self.pop();
                    self.advance(p + 1);
                } else {
                    let e = self.index_of_unit(125, p).unwrap_or(self.n);
                    self.emit(t::acc_descr_multiline_value, e);
                }
            }
            S::Dir => {
                if c == 34 {
                    self.quote();
                } else if let Some(e) = self.no_dir(p) {
                    self.pop();
                    self.emit(t::NODIR, e);
                } else if let Some(e) = self
                    .dir(p)
                    .or_else(|| self.dir_sym(p))
                    .or_else(|| self.dir_v(p))
                {
                    self.pop();
                    self.emit(t::DIR, e);
                } else {
                    return self.opener(c);
                }
            }
            S::Text => {
                let d = self.c(p + 1);
                match c {
                    34 => self.quote(),
                    40 | 91 | 123 => {
                        self.opener(c);
                    }
                    93 => {
                        self.pop();
                        if d == 41 {
                            self.emit_flat(t::STADIUMEND, p + 2);
                        } else if d == 93 {
                            self.emit_flat(t::SUBROUTINEEND, p + 2);
                        } else {
                            self.emit_flat(t::SQE, p + 1);
                        }
                    }
                    41 => {
                        self.pop();
                        if d == 93 {
                            self.emit_flat(t::CYLINDEREND, p + 2);
                        } else if d == 41 && self.c(p + 2) == 41 {
                            self.emit_flat(t::DOUBLECIRCLEEND, p + 3);
                        } else {
                            self.emit_flat(t::PE, p + 1);
                        }
                    }
                    124 => {
                        self.pop();
                        self.emit_flat(t::PIPE, p + 1);
                    }
                    125 => {
                        self.pop();
                        self.emit_flat(t::DIAMOND_STOP, p + 1);
                    }
                    37 => {
                        // Jison は Mermaid の文字の型から 1 つだけの `%` を落とすので、`%` で始まれるのはコメントだけ。
                        match self.comment_end(p) {
                            Some(e) => self.advance(e),
                            None => return false,
                        }
                    }
                    _ => {
                        let mut q = p + 1;
                        while q < self.n {
                            let ch = self.c(q);
                            if matches!(ch, 91 | 93 | 40 | 41 | 123 | 125 | 124 | 34 | 37) {
                                break;
                            }
                            q += 1;
                        }
                        self.emit(t::TEXT, q);
                    }
                }
            }
            S::EllipseText => {
                let d = self.c(p + 1);
                if c == 34 {
                    self.quote();
                } else if let Some(e) = (c == 37).then(|| self.comment_end(p)).flatten() {
                    self.advance(e);
                } else if (c == 45 || c == 47 || c == 41) && d == 41 {
                    self.pop();
                    self.emit_flat(t::ELLIPSE_END, p + 2);
                } else if c == 40 || c == 91 || c == 123 {
                    self.opener(c);
                } else if c == 41 || c == 93 || c == 125 {
                    return false;
                } else {
                    let mut q = p + 1;
                    while q < self.n {
                        let ch = self.c(q);
                        if matches!(ch, 34 | 40 | 41 | 91 | 93 | 123 | 125) {
                            break;
                        }
                        if (ch == 45 || ch == 47) && self.c(q + 1) == 41 {
                            break;
                        }
                        if ch == 37 && self.c(q + 1) == 37 && self.c(q + 2) != 123 {
                            break;
                        }
                        q += 1;
                    }
                    self.emit_chars(t::TEXT, q);
                }
            }
            S::TrapText => {
                let d = self.c(p + 1);
                if c == 34 {
                    self.quote();
                    return true;
                }
                match c {
                    40 => {
                        if d == 45 || d == 91 || (d == 40 && self.c(p + 2) == 40) {
                            self.opener(c);
                        } else if d == 93 {
                            self.pop();
                            self.emit_flat(t::TRAPEND, p + 2);
                        } else {
                            self.opener(c);
                        }
                    }
                    91 | 123 => {
                        self.opener(c);
                    }
                    92 => {
                        if d == 93 {
                            self.pop();
                            self.emit_flat(t::TRAPEND, p + 2);
                        } else {
                            self.emit_flat(t::TEXT, p + 1);
                        }
                    }
                    47 => {
                        if d == 93 {
                            self.pop();
                            self.emit_flat(t::INVTRAPEND, p + 2);
                        } else {
                            self.emit_flat(t::TEXT, p + 1);
                        }
                    }
                    93 | 41 => {
                        if d == 93 {
                            self.pop();
                            self.emit_flat(t::TRAPEND, p + 2);
                        } else {
                            return false;
                        }
                    }
                    125 => return false,
                    37 => match self.comment_end(p) {
                        Some(e) => self.advance(e),
                        None => return false,
                    },
                    _ => {
                        if (c == 63 || c == 61) && d == 93 {
                            self.pop();
                            self.emit_flat(t::TRAPEND, p + 2);
                            return true;
                        }
                        let mut q = p + 1;
                        while q < self.n {
                            let ch = self.c(q);
                            if matches!(ch, 92 | 91 | 93 | 40 | 41 | 123 | 125 | 47 | 37) {
                                break;
                            }
                            q += 1;
                        }
                        self.emit(t::TEXT, q);
                    }
                }
            }
            S::EdgeText => {
                if c == 34 {
                    self.quote();
                    return true;
                }
                if let Some(e) = self.arrow_end(p) {
                    self.pop();
                    self.emit(t::LINK, e);
                    return true;
                }
                if c == 45 && self.c(p + 1) == 45 {
                    return false;
                }
                let mut q = p + 1;
                while q < self.n {
                    let ch = self.c(q);
                    if ch == 34 {
                        break;
                    }
                    if ch == 45 {
                        if self.c(q + 1) == 45 {
                            break;
                        }
                    } else if is_space(ch) && self.arrow_end(q).is_some() {
                        break;
                    }
                    q += 1;
                }
                self.emit_chars(t::EDGE_TEXT, q);
            }
            S::Click => {
                if c == 34 {
                    self.quote();
                } else if is_space(c) {
                    self.pop();
                    self.advance(p + 1);
                } else {
                    let mut q = p + 1;
                    while q < self.n && !is_space(self.c(q)) {
                        q += 1;
                    }
                    self.emit_flat(t::CLICK, q);
                }
            }
            S::Callbackname => {
                if c == 40 {
                    let e = self.empty_call(p);
                    self.pop();
                    match e {
                        Some(e) => self.advance(e),
                        None => {
                            self.push(S::Callbackargs);
                            self.advance(p + 1);
                        }
                    }
                } else {
                    let e = self.index_of_unit(40, p).unwrap_or(self.n);
                    self.emit(t::CALLBACKNAME, e);
                }
            }
            S::Callbackargs => {
                if c == 41 {
                    self.pop();
                    self.advance(p + 1);
                } else {
                    let e = self.index_of_unit(41, p).unwrap_or(self.n);
                    self.emit(t::CALLBACKARGS, e);
                }
            }
            S::ShapeData => {
                if c == 34 {
                    self.push(S::ShapeDataStr);
                    self.emit_flat(t::SHAPE_DATA, p + 1);
                } else if c == 125 {
                    self.pop();
                    self.advance(p + 1);
                } else if c == 94 {
                    return false;
                } else {
                    let mut q = p + 1;
                    while q < self.n {
                        let ch = self.c(q);
                        if ch == 125 || ch == 94 || ch == 34 {
                            break;
                        }
                        q += 1;
                    }
                    self.emit(t::SHAPE_DATA, q);
                }
            }
            S::ShapeDataStr => {
                if c == 34 {
                    self.pop();
                    self.emit_flat(t::SHAPE_DATA, p + 1);
                } else {
                    let e = self.index_of_unit(34, p).unwrap_or(self.n);
                    let text = br_lines(&self.src[p..e]);
                    self.emit_text(t::SHAPE_DATA, text, e);
                }
            }
        }
        true
    }

    fn run(&mut self) {
        loop {
            if self.p >= self.n {
                let type_ = match self.state {
                    S::Initial => Some(t::EOF),
                    S::AccTitle => Some(t::acc_title_value),
                    S::AccDescr => Some(t::acc_descr_value),
                    S::AccDescrMultiline => Some(t::acc_descr_multiline_value),
                    S::Click => Some(t::CLICK),
                    S::Callbackname => Some(t::CALLBACKNAME),
                    S::Callbackargs => Some(t::CALLBACKARGS),
                    _ => None,
                };
                if let Some(type_) = type_ {
                    self.emit_flat(type_, self.n);
                }
                break;
            }
            let c = self.c(self.p);
            if !self.step(c) {
                self.types.push(t::ERROR);
                self.texts.push(String::new());
                self.starts.push(self.p);
                self.locs.extend([self.line, self.col, self.line, self.col]);
                return;
            }
        }
        self.types.push(t::END);
        self.texts.push(String::new());
        self.starts.push(self.n);
        self.locs.extend([self.line, self.col, self.line, self.col]);
    }
}

/// `text.replace(/\n\s*/g, '<br/>')`
fn br_lines(s: &[u16]) -> String {
    let mut out: Vec<u16> = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == 10 {
            out.extend("<br/>".encode_utf16());
            i += 1;
            while i < s.len() && is_space(s[i] as u32) {
                i += 1;
            }
        } else {
            out.push(s[i]);
            i += 1;
        }
    }
    js::from16(&out)
}

pub fn tokenize(src: &[u16]) -> Tokens {
    let mut lexer = Lexer {
        src,
        n: src.len(),
        types: Vec::new(),
        texts: Vec::new(),
        starts: Vec::new(),
        locs: Vec::new(),
        stack: Vec::new(),
        state: S::Initial,
        p: 0,
        line: 1,
        col: 0,
        first_graph: true,
        dir_next: None,
        at_next: None,
        line_end: None,
        dir_failed: 0,
        run_end: 0,
        run_at: None,
        ws_from: NO,
        ws_to: NO,
        blank_from: NO,
        blank_to: NO,
        no_link: NO,
        no_arrow: NO,
    };
    lexer.dir_next = lexer.index_of("direction", 0);
    lexer.at_next = lexer.index_of_unit(64, 0);
    lexer.run();
    Tokens {
        types: lexer.types,
        texts: lexer.texts,
        starts: lexer.starts,
        locs: lexer.locs,
    }
}
