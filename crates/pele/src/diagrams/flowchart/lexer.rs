//! flowchart の字句解析（pele の `src/diagrams/flowchart/lexer.ts`）。JS と同じく UTF-16 の符号単位で読む。
//! 粘着（`y`）の正規表現は、同じ所に合う手書きの走査に置き換えた。

use super::tokens as t;
use crate::util::chars::{NONE, is_space, is_word};
use crate::util::unicode::is_unicode_letter;

/// どの文字とも、終わりの先（`NONE`）とも違う値（TS の -1）。
const NO_CHAR: u32 = u32::MAX - 1;

pub struct Tokens {
    pub types: Vec<u8>,
    pub texts: Vec<String>,
    pub starts: Vec<usize>,
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
    ThickEdgeText,
    DottedEdgeText,
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

/// 粘着の正規表現の代わり。`q` から合えば終わりの位置。
#[derive(Clone, Copy)]
enum Re {
    AccTitle,
    AccDescr,
    AccDescrMl,
    Call,
    Click,
    NoDir,
    Dir,
    DirSym,
    DirV,
    Direction(usize),
    Link,
    StartLink,
    ThickLink,
    ThickStart,
    DottedLink,
    DottedStart,
    InvisLink,
    Newline,
    Space,
    EmptyCall,
}

const DIRECTIONS: [&str; 5] = ["TB", "BT", "RL", "LR", "TD"];

struct Lexer<'a> {
    src: &'a [u16],
    n: usize,
    types: Vec<u8>,
    texts: Vec<String>,
    starts: Vec<usize>,
    stack: Vec<S>,
    state: S,
    p: usize,
    first_graph: bool,
    dir_next: Option<usize>,
    at_next: Option<usize>,
    line_end: isize,
    dir_failed: usize,
    run_end: usize,
    run_at: isize,
    ws_from: isize,
    ws_to: isize,
}

impl<'a> Lexer<'a> {
    fn c(&self, i: usize) -> u32 {
        self.src.get(i).map_or(NONE, |&c| c as u32)
    }

    fn starts_with(&self, word: &str, at: usize) -> bool {
        let w: Vec<u16> = word.encode_utf16().collect();
        self.src.len() >= at + w.len() && self.src[at..at + w.len()] == w[..]
    }

    fn index_of(&self, word: &str, from: usize) -> Option<usize> {
        let w: Vec<u16> = word.encode_utf16().collect();
        if w.is_empty() || self.src.len() < w.len() {
            return None;
        }
        (from..=self.src.len() - w.len()).find(|&k| self.src[k..k + w.len()] == w[..])
    }

    fn index_of_unit(&self, c: u16, from: usize) -> Option<usize> {
        if from >= self.n {
            return None;
        }
        self.src[from..]
            .iter()
            .position(|&x| x == c)
            .map(|k| k + from)
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

    fn spaces(&self, mut q: usize) -> usize {
        while q < self.n && is_space(self.c(q)) {
            q += 1;
        }
        q
    }

    /// `[xo<]?`
    fn opt_xo_lt(&self, q: usize) -> usize {
        if matches!(self.c(q), 120 | 111 | 60) {
            q + 1
        } else {
            q
        }
    }

    fn run_of(&self, q: usize, ch: u32) -> usize {
        let mut e = q;
        while self.c(e) == ch {
            e += 1;
        }
        e
    }

    fn at(&self, re: Re, q: usize) -> Option<usize> {
        let c = |i: usize| self.c(i);
        match re {
            Re::AccTitle | Re::AccDescr => {
                let word = if matches!(re, Re::AccTitle) {
                    "accTitle"
                } else {
                    "accDescr"
                };
                if !self.starts_with(word, q) {
                    return None;
                }
                let e = self.spaces(q + 8);
                if c(e) != 58 {
                    return None;
                }
                Some(self.spaces(e + 1))
            }
            Re::AccDescrMl => {
                if !self.starts_with("accDescr", q) {
                    return None;
                }
                let e = self.spaces(q + 8);
                if c(e) != 123 {
                    return None;
                }
                Some(self.spaces(e + 1))
            }
            Re::Call | Re::Click => {
                let word = if matches!(re, Re::Call) {
                    "call"
                } else {
                    "click"
                };
                if !self.starts_with(word, q) {
                    return None;
                }
                let s = q + word.len();
                let e = self.spaces(s);
                (e > s).then_some(e)
            }
            Re::NoDir => {
                // `(?:\r?\n)*\s*\n`: 空白の並びのうち、最後の \n の後ろまで。
                let e = self.spaces(q);
                (q..e).rev().find(|&k| c(k) == 10).map(|k| k + 1)
            }
            Re::Dir => {
                let s = self.spaces(q);
                let a = c(s);
                let b = c(s + 1);
                let pair = [a, b];
                let ok =
                    [[76, 82], [82, 76], [84, 66], [66, 84], [84, 68], [66, 82]].contains(&pair);
                (ok && !is_word(c(s + 2))).then_some(s + 2)
            }
            Re::DirSym => {
                let s = self.spaces(q);
                matches!(c(s), 60 | 62 | 94).then_some(s + 1)
            }
            Re::DirV => {
                let s = self.spaces(q);
                (c(s) == 118 && !is_word(c(s + 1))).then_some(s + 1)
            }
            Re::Direction(i) => {
                // `.*direction\s+XX[^\n]*`: 行の終わりの文字までで、いちばん後ろの `direction` から。
                let mut stop = q;
                while stop < self.n && !matches!(c(stop), 10 | 13 | 0x2028 | 0x2029) {
                    stop += 1;
                }
                let dir: Vec<u16> = DIRECTIONS[i].encode_utf16().collect();
                let mut k = stop;
                while k > q {
                    k -= 1;
                    if k + 9 <= stop && self.starts_with("direction", k) {
                        let s = k + 9;
                        let e = self.spaces(s);
                        if e > s && self.src.len() >= e + 2 && self.src[e..e + 2] == dir[..] {
                            let mut end = e + 2;
                            while end < self.n && c(end) != 10 {
                                end += 1;
                            }
                            return Some(end);
                        }
                    }
                }
                None
            }
            Re::Link | Re::ThickLink => {
                // `\s*[xo<]?--+[-xo>]\s*`（`=` も同じ）
                let ch = if matches!(re, Re::Link) { 45 } else { 61 };
                let s = self.opt_xo_lt(self.spaces(q));
                let e = self.run_of(s, ch);
                let k = e - s;
                let end = if k >= 2 && matches!(c(e), 120 | 111 | 62) {
                    e + 1
                } else if k >= 3 {
                    e
                } else {
                    return None;
                };
                Some(self.spaces(end))
            }
            Re::StartLink | Re::ThickStart => {
                let ch = if matches!(re, Re::StartLink) { 45 } else { 61 };
                let s = self.opt_xo_lt(self.spaces(q));
                (c(s) == ch && c(s + 1) == ch).then(|| self.spaces(s + 2))
            }
            Re::DottedLink => {
                // `\s*[xo<]?-?\.+-[xo>]?\s*`
                let mut s = self.opt_xo_lt(self.spaces(q));
                if c(s) == 45 {
                    s += 1;
                }
                let e = self.run_of(s, 46);
                if e == s || c(e) != 45 {
                    return None;
                }
                let mut end = e + 1;
                if matches!(c(end), 120 | 111 | 62) {
                    end += 1;
                }
                Some(self.spaces(end))
            }
            Re::DottedStart => {
                let s = self.opt_xo_lt(self.spaces(q));
                (c(s) == 45 && c(s + 1) == 46).then(|| self.spaces(s + 2))
            }
            Re::InvisLink => {
                let s = self.spaces(q);
                let e = self.run_of(s, 126);
                (e - s >= 3).then(|| self.spaces(e))
            }
            Re::Newline => {
                let mut e = q;
                loop {
                    if c(e) == 10 {
                        e += 1;
                    } else if c(e) == 13 && c(e + 1) == 10 {
                        e += 2;
                    } else {
                        break;
                    }
                }
                (e > q).then_some(e)
            }
            Re::Space => {
                let mut e = q;
                while e < self.n && is_space(c(e)) && c(e) != 10 && c(e) != 13 {
                    e += 1;
                }
                (e > q).then_some(e)
            }
            Re::EmptyCall => {
                if c(q) != 40 {
                    return None;
                }
                let e = self.spaces(q + 1);
                (c(e) == 41).then_some(e + 1)
            }
        }
    }

    fn kw(&self, word: &str) -> bool {
        self.starts_with(word, self.p) && !is_word(self.c(self.p + word.len()))
    }

    fn skip_ws_cached(&mut self, q: usize) -> usize {
        let mut j = q;
        if q as isize >= self.ws_from && q as isize <= self.ws_to {
            j = self.ws_to as usize;
        } else if is_space(self.c(q)) {
            j = self.spaces(q);
            self.ws_from = q as isize;
            self.ws_to = j as isize;
        }
        j
    }

    /// q から空白の並びの後の最初の文字（x・o・< を 1 つ飛ばして）。
    fn link_char(&mut self, q: usize) -> u32 {
        let j = self.skip_ws_cached(q);
        let d = self.c(j);
        if d == 120 || d == 111 || d == 60 {
            self.c(j + 1)
        } else {
            d
        }
    }

    fn invis_ahead(&mut self, q: usize) -> bool {
        let j = self.skip_ws_cached(q);
        self.c(j) == 126
    }

    fn quote(&mut self) {
        if self.c(self.p + 1) == 96 {
            self.push(S::MdString);
            self.p += 2;
        } else {
            self.push(S::String);
            self.p += 1;
        }
    }

    fn invis_link(&mut self) -> bool {
        if !self.invis_ahead(self.p) {
            return false;
        }
        match self.at(Re::InvisLink, self.p) {
            Some(e) => {
                self.emit(t::LINK, self.p, e);
                true
            }
            None => false,
        }
    }

    fn opener(&mut self, c: u32) -> bool {
        let p = self.p;
        let d = self.c(p + 1);
        if c == 40 {
            if d == 45 {
                self.push(S::EllipseText);
                self.emit(t::ELLIPSE_START, p, p + 2);
            } else if d == 91 {
                self.push(S::Text);
                self.emit(t::STADIUMSTART, p, p + 2);
            } else if d == 40 && self.c(p + 2) == 40 {
                self.push(S::Text);
                self.emit(t::DOUBLECIRCLESTART, p, p + 3);
            } else {
                self.push(S::Text);
                self.emit(t::PS, p, p + 1);
            }
            return true;
        }
        if c == 91 {
            if d == 91 {
                self.push(S::Text);
                self.emit(t::SUBROUTINESTART, p, p + 2);
            } else if d == 40 {
                self.push(S::Text);
                self.emit(t::CYLINDERSTART, p, p + 2);
            } else if d == 47 {
                self.push(S::TrapText);
                self.emit(t::TRAPSTART, p, p + 2);
            } else if d == 92 {
                self.push(S::TrapText);
                self.emit(t::INVTRAPSTART, p, p + 2);
            } else {
                self.push(S::Text);
                self.emit(t::SQS, p, p + 1);
            }
            return true;
        }
        if c == 124 {
            self.push(S::Text);
            self.emit(t::PIPE, p, p + 1);
            return true;
        }
        if c == 123 {
            self.push(S::Text);
            self.emit(t::DIAMOND_START, p, p + 1);
            return true;
        }
        false
    }

    fn graph(&mut self, len: usize) {
        if self.first_graph {
            self.first_graph = false;
            self.push(S::Dir);
        }
        self.emit(t::GRAPH, self.p, self.p + len);
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
        self.emit(t::NODE_STRING, self.p, q);
        true
    }

    fn try_emit(&mut self, re: Re, type_: u8, push: Option<S>) -> bool {
        match self.at(re, self.p) {
            Some(e) => {
                if let Some(s) = push {
                    self.push(s);
                }
                self.emit(type_, self.p, e);
                true
            }
            None => false,
        }
    }

    fn link(&mut self, c: u32) -> bool {
        let d = self.link_char(self.p);
        if d == 45 {
            if self.try_emit(Re::Link, t::LINK, None)
                || self.try_emit(Re::StartLink, t::START_LINK, Some(S::EdgeText))
                || self.try_emit(Re::DottedLink, t::LINK, None)
                || self.try_emit(Re::DottedStart, t::START_LINK, Some(S::DottedEdgeText))
            {
                return true;
            }
        } else if d == 61 {
            if self.try_emit(Re::ThickLink, t::LINK, None)
                || self.try_emit(Re::ThickStart, t::START_LINK, Some(S::ThickEdgeText))
            {
                return true;
            }
        } else if d == 46 && self.try_emit(Re::DottedLink, t::LINK, None) {
            return true;
        }
        c != 45 && c != 61 && c != 46 && self.invis_link()
    }

    fn initial(&mut self, c: u32) -> bool {
        let p = self.p;
        match c {
            97 => {
                if self.starts_with("acc", p) {
                    if self.try_emit(Re::AccTitle, t::acc_title, Some(S::AccTitle)) {
                        return true;
                    }
                    if self.try_emit(Re::AccDescr, t::acc_descr, Some(S::AccDescr)) {
                        return true;
                    }
                    if let Some(e) = self.at(Re::AccDescrMl, p) {
                        self.push(S::AccDescrMultiline);
                        self.p = e;
                        return true;
                    }
                }
            }
            64 => {
                if self.c(p + 1) == 123 {
                    self.push(S::ShapeData);
                    self.types.push(t::SHAPE_DATA);
                    self.texts.push(String::new());
                    self.starts.push(p);
                    self.p += 2;
                    return true;
                }
            }
            99 => {
                if let Some(e) = self.at(Re::Call, p) {
                    self.push(S::Callbackname);
                    self.p = e;
                    return true;
                }
                if self.kw("classDef") {
                    self.emit(t::CLASSDEF, p, p + 8);
                    return true;
                }
                if self.kw("class") {
                    self.emit(t::CLASS, p, p + 5);
                    return true;
                }
                if let Some(e) = self.at(Re::Click, p) {
                    self.push(S::Click);
                    self.p = e;
                    return true;
                }
            }
            34 => {
                self.quote();
                return true;
            }
            115 => {
                if self.kw("style") {
                    self.emit(t::STYLE, p, p + 5);
                    return true;
                }
                if self.kw("swimlane-beta") {
                    self.graph(13);
                    return true;
                }
                if self.kw("subgraph") {
                    self.emit(t::subgraph, p, p + 8);
                    return true;
                }
            }
            100 => {
                if self.kw("default") {
                    self.emit(t::DEFAULT, p, p + 7);
                    return true;
                }
            }
            108 => {
                if self.kw("linkStyle") {
                    self.emit(t::LINKSTYLE, p, p + 9);
                    return true;
                }
            }
            105 => {
                if self.kw("interpolate") {
                    self.emit(t::INTERPOLATE, p, p + 11);
                    return true;
                }
            }
            104 => {
                if self.starts_with("href", p) && is_space(self.c(p + 4)) {
                    self.emit(t::HREF, p, p + 5);
                    return true;
                }
            }
            102 => {
                if self.kw("flowchart-elk") {
                    self.graph(13);
                    return true;
                }
                if self.kw("flowchart") {
                    self.graph(9);
                    return true;
                }
            }
            103 => {
                if self.kw("graph") {
                    self.graph(5);
                    return true;
                }
            }
            101 => {
                if self.kw("end") {
                    let e = self.spaces(p + 3);
                    self.emit(t::end, p, e);
                    return true;
                }
            }
            95 => {
                for (w, len) in [("_self", 5), ("_blank", 6), ("_parent", 7), ("_top", 4)] {
                    if self.kw(w) {
                        self.emit(t::LINK_TARGET, p, p + len);
                        return true;
                    }
                }
            }
            _ => {}
        }

        if self.dir_next.is_some() && p >= self.dir_failed {
            if self.dir_next.is_some_and(|d| d < p) {
                self.dir_next = self.index_of("direction", p);
            }
            if let Some(dir_next) = self.dir_next {
                if self.line_end < p as isize {
                    let mut e = p;
                    while e < self.n && !matches!(self.c(e), 10 | 13 | 0x2028 | 0x2029) {
                        e += 1;
                    }
                    self.line_end = e as isize;
                }
                if (dir_next as isize) < self.line_end {
                    for i in 0..5 {
                        if let Some(e) = self.at(Re::Direction(i), p) {
                            self.emit(t::direction_tb + i as u8, p, e);
                            return true;
                        }
                    }
                    // この行の残りのどこにも、向きの文は始まらない。
                    self.dir_failed = self.line_end as usize;
                }
            }
        }

        // リンクの id は、空白でない文字の並びの、`{` か `"` が後に続かない最後の `@` まで。
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
                self.run_at = -1;
                if let Some(at_next) = self.at_next.filter(|&a| a < q) {
                    let mut k = q.min(self.n - 1) as isize - 1;
                    while k >= at_next as isize {
                        let ku = k as usize;
                        if self.c(ku) == 64 {
                            let after = self.c(ku + 1);
                            if after != 123 && after != 34 {
                                self.run_at = k;
                                break;
                            }
                        }
                        k -= 1;
                    }
                }
            }
            if self.run_at > p as isize {
                let e = self.run_at as usize + 1;
                self.emit(t::LINK_ID, p, e);
                return true;
            }
        }

        if (48..=57).contains(&c) {
            let mut q = p + 1;
            while q < self.n && (48..=57).contains(&self.c(q)) {
                q += 1;
            }
            self.emit(t::NUM, p, q);
            return true;
        }

        match c {
            35 => {
                self.emit(t::BRKT, p, p + 1);
                return true;
            }
            58 => {
                if self.c(p + 1) == 58 && self.c(p + 2) == 58 {
                    self.emit(t::STYLE_SEPARATOR, p, p + 3);
                } else {
                    self.emit(t::COLON, p, p + 1);
                }
                return true;
            }
            38 => {
                self.emit(t::AMP, p, p + 1);
                return true;
            }
            59 => {
                self.emit(t::SEMI, p, p + 1);
                return true;
            }
            44 => {
                self.emit(t::COMMA, p, p + 1);
                return true;
            }
            42 => {
                self.emit(t::MULT, p, p + 1);
                return true;
            }
            120 | 111 => return self.link(c) || self.node_string(),
            60 => {
                if self.link(c) {
                    return true;
                }
                self.emit(t::TAGSTART, p, p + 1);
                return true;
            }
            45 => {
                if self.link(c) || self.node_string() {
                    return true;
                }
                self.emit(t::MINUS, p, p + 1);
                return true;
            }
            61 => return self.link(c),
            46 => return self.link(c) || self.node_string(),
            126 => return self.invis_link(),
            40 => return self.opener(c),
            91 => {
                if self.c(p + 1) == 124 {
                    self.emit(t::VERTEX_WITH_PROPS_START, p, p + 2);
                    return true;
                }
                return self.opener(c);
            }
            124 | 123 => return self.opener(c),
            62 => {
                self.push(S::Text);
                self.emit(t::TAGEND, p, p + 1);
                return true;
            }
            94 => {
                self.emit(t::UP, p, p + 1);
                return true;
            }
            92 => {
                if self.c(p + 1) == 124 {
                    self.emit(t::SEP, p, p + 2);
                    return true;
                }
                return self.node_string();
            }
            118 => {
                if !is_word(self.c(p + 1)) {
                    self.emit(t::DOWN, p, p + 1);
                    return true;
                }
                return self.node_string();
            }
            _ => {}
        }

        if is_space(c) {
            if self.link(c) {
                return true;
            }
            if self.try_emit(Re::Newline, t::NEWLINE, None) {
                return true;
            }
            if self.try_emit(Re::Space, t::SPACE, None) {
                return true;
            }
            return false;
        }

        if c < 128 {
            return self.node_string();
        }
        if is_unicode_letter(c) {
            self.emit(t::UNICODE_TEXT, p, p + 1);
            return true;
        }
        false
    }

    fn edge_text(&mut self, c: u32, link_re: Re, a: u32, b: u32) -> bool {
        if c == 34 {
            self.quote();
            return true;
        }
        let d = self.link_char(self.p);
        if (d == a || d == b)
            && let Some(e) = self.at(link_re, self.p)
        {
            self.pop();
            self.emit(t::LINK, self.p, e);
            return true;
        }
        if c == a && (a != 45 || self.c(self.p + 1) == 45) {
            return false;
        }
        let mut q = self.p + 1;
        while q < self.n {
            let ch = self.c(q);
            if ch == 34 {
                break;
            }
            if ch == a {
                if a != 45 || self.c(q + 1) == 45 {
                    break;
                }
            } else if ch == 120 || ch == 111 || ch == 60 || ch == b || is_space(ch) {
                let dd = self.link_char(q);
                if (dd == a || dd == b) && self.at(link_re, q).is_some() {
                    break;
                }
            }
            q += 1;
        }
        self.emit(t::EDGE_TEXT, self.p, q);
        true
    }

    fn step(&mut self, c: u32) -> bool {
        let p = self.p;
        let mut ok = true;
        match self.state {
            S::Initial => ok = self.initial(c),
            S::String => {
                if c == 34 {
                    if self.c(p + 1) == 96 {
                        self.push(S::MdString);
                        self.p += 2;
                    } else {
                        self.pop();
                        self.p += 1;
                    }
                } else {
                    let e = self.index_of_unit(34, p).unwrap_or(self.n);
                    self.emit(t::STR, p, e);
                }
            }
            S::MdString => {
                if c == 96 {
                    if self.c(p + 1) == 34 {
                        self.pop();
                        self.p += 2;
                    } else {
                        ok = false;
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
                    self.emit(t::MD_STR, p, q);
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
                self.emit(type_, p, e);
            }
            S::AccDescrMultiline => {
                if c == 125 {
                    self.pop();
                    self.p += 1;
                } else {
                    let e = self.index_of_unit(125, p).unwrap_or(self.n);
                    self.emit(t::acc_descr_multiline_value, p, e);
                }
            }
            S::Dir => {
                if c == 34 {
                    self.quote();
                } else if let Some(e) = self.at(Re::NoDir, p) {
                    self.pop();
                    self.emit(t::NODIR, p, e);
                } else if let Some(e) = self
                    .at(Re::Dir, p)
                    .or_else(|| self.at(Re::DirSym, p))
                    .or_else(|| self.at(Re::DirV, p))
                {
                    self.pop();
                    self.emit(t::DIR, p, e);
                } else {
                    ok = self.invis_link() || self.opener(c);
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
                            self.emit(t::STADIUMEND, p, p + 2);
                        } else if d == 93 {
                            self.emit(t::SUBROUTINEEND, p, p + 2);
                        } else {
                            self.emit(t::SQE, p, p + 1);
                        }
                    }
                    41 => {
                        self.pop();
                        if d == 93 {
                            self.emit(t::CYLINDEREND, p, p + 2);
                        } else if d == 41 && self.c(p + 2) == 41 {
                            self.emit(t::DOUBLECIRCLEEND, p, p + 3);
                        } else {
                            self.emit(t::PE, p, p + 1);
                        }
                    }
                    124 => {
                        self.pop();
                        self.emit(t::PIPE, p, p + 1);
                    }
                    125 => {
                        self.pop();
                        self.emit(t::DIAMOND_STOP, p, p + 1);
                    }
                    _ => {
                        if (c == 126 || is_space(c)) && self.invis_link() {
                            return true;
                        }
                        let mut q = p + 1;
                        while q < self.n {
                            let ch = self.c(q);
                            if matches!(ch, 91 | 93 | 40 | 41 | 123 | 125 | 124 | 34) {
                                break;
                            }
                            q += 1;
                        }
                        self.emit(t::TEXT, p, q);
                    }
                }
            }
            S::EllipseText => {
                let d = self.c(p + 1);
                if c == 34 {
                    self.quote();
                } else if (c == 126 || is_space(c)) && self.invis_link() {
                    return true;
                } else if (c == 45 || c == 47 || c == 41) && d == 41 {
                    self.pop();
                    self.emit(t::ELLIPSE_END, p, p + 2);
                } else if c == 40 || c == 91 || c == 123 {
                    self.opener(c);
                } else if c == 41 || c == 93 || c == 125 {
                    ok = false;
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
                        if (ch == 126 || is_space(ch))
                            && self.invis_ahead(q)
                            && self.at(Re::InvisLink, q).is_some()
                        {
                            break;
                        }
                        q += 1;
                    }
                    self.emit(t::TEXT, p, q);
                }
            }
            S::TrapText => {
                let d = self.c(p + 1);
                if c == 34 {
                    self.quote();
                    return true;
                }
                if (c == 126 || is_space(c)) && self.invis_link() {
                    return true;
                }
                match c {
                    40 => {
                        if d == 45 || d == 91 || (d == 40 && self.c(p + 2) == 40) {
                            self.opener(c);
                        } else if d == 93 {
                            self.pop();
                            self.emit(t::TRAPEND, p, p + 2);
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
                            self.emit(t::TRAPEND, p, p + 2);
                        } else {
                            self.emit(t::TEXT, p, p + 1);
                        }
                    }
                    47 => {
                        if d == 93 {
                            self.pop();
                            self.emit(t::INVTRAPEND, p, p + 2);
                        } else {
                            self.emit(t::TEXT, p, p + 1);
                        }
                    }
                    93 | 41 => {
                        if d == 93 {
                            self.pop();
                            self.emit(t::TRAPEND, p, p + 2);
                        } else {
                            ok = false;
                        }
                    }
                    125 => ok = false,
                    _ => {
                        if (c == 63 || c == 61) && d == 93 {
                            self.pop();
                            self.emit(t::TRAPEND, p, p + 2);
                            return true;
                        }
                        let mut q = p + 1;
                        while q < self.n {
                            let ch = self.c(q);
                            if matches!(ch, 92 | 91 | 93 | 40 | 41 | 123 | 125 | 47) {
                                break;
                            }
                            q += 1;
                        }
                        self.emit(t::TEXT, p, q);
                    }
                }
            }
            S::EdgeText => ok = self.edge_text(c, Re::Link, 45, NO_CHAR),
            S::ThickEdgeText => ok = self.edge_text(c, Re::ThickLink, 61, NO_CHAR),
            S::DottedEdgeText => ok = self.edge_text(c, Re::DottedLink, 46, 45),
            S::Click => {
                if c == 34 {
                    self.quote();
                } else if is_space(c) {
                    self.pop();
                    self.p += 1;
                } else {
                    let mut q = p + 1;
                    while q < self.n && !is_space(self.c(q)) {
                        q += 1;
                    }
                    self.emit(t::CLICK, p, q);
                }
            }
            S::Callbackname => {
                if c == 40 {
                    let e = self.at(Re::EmptyCall, p);
                    self.pop();
                    match e {
                        Some(e) => self.p = e,
                        None => {
                            self.push(S::Callbackargs);
                            self.p += 1;
                        }
                    }
                } else {
                    let e = self.index_of_unit(40, p).unwrap_or(self.n);
                    self.emit(t::CALLBACKNAME, p, e);
                }
            }
            S::Callbackargs => {
                if c == 41 {
                    self.pop();
                    self.p += 1;
                } else {
                    let e = self.index_of_unit(41, p).unwrap_or(self.n);
                    self.emit(t::CALLBACKARGS, p, e);
                }
            }
            S::ShapeData => {
                if c == 34 {
                    self.push(S::ShapeDataStr);
                    self.emit(t::SHAPE_DATA, p, p + 1);
                } else if c == 125 {
                    self.pop();
                    self.p += 1;
                } else if c == 94 {
                    ok = false;
                } else {
                    let mut q = p + 1;
                    while q < self.n {
                        let ch = self.c(q);
                        if ch == 125 || ch == 94 || ch == 34 {
                            break;
                        }
                        q += 1;
                    }
                    self.emit(t::SHAPE_DATA, p, q);
                }
            }
            S::ShapeDataStr => {
                if c == 34 {
                    self.pop();
                    self.emit(t::SHAPE_DATA, p, p + 1);
                } else {
                    let e = self.index_of_unit(34, p).unwrap_or(self.n);
                    self.types.push(t::SHAPE_DATA);
                    self.texts
                        .push(br_lines(&String::from_utf16_lossy(&self.src[p..e])));
                    self.starts.push(p);
                    self.p = e;
                }
            }
        }
        ok
    }
}

/// `text.replace(/\n\s*/g, '<br/>')`
fn br_lines(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\n' {
            while chars
                .peek()
                .is_some_and(|c| crate::util::chars::is_space_char(*c))
            {
                chars.next();
            }
            out.push_str("<br/>");
        } else {
            out.push(c);
        }
    }
    out
}

pub fn tokenize(src: &[u16]) -> Tokens {
    let mut lx = Lexer {
        src,
        n: src.len(),
        types: Vec::new(),
        texts: Vec::new(),
        starts: Vec::new(),
        stack: Vec::new(),
        state: S::Initial,
        p: 0,
        first_graph: true,
        dir_next: None,
        at_next: None,
        line_end: -1,
        dir_failed: 0,
        run_end: 0,
        run_at: -1,
        ws_from: -1,
        ws_to: -1,
    };
    lx.dir_next = lx.index_of("direction", 0);
    lx.at_next = lx.index_of_unit(64, 0);
    loop {
        if lx.p >= lx.n {
            let type_ = match lx.state {
                S::Initial => Some(t::EOF),
                S::AccTitle => Some(t::acc_title_value),
                S::AccDescr => Some(t::acc_descr_value),
                S::AccDescrMultiline => Some(t::acc_descr_multiline_value),
                S::Click => Some(t::CLICK),
                S::Callbackname => Some(t::CALLBACKNAME),
                S::Callbackargs => Some(t::CALLBACKARGS),
                _ => None,
            };
            if let Some(ty) = type_ {
                lx.emit(ty, lx.n, lx.n);
            }
            break;
        }
        let c = lx.c(lx.p);
        if !lx.step(c) {
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
    lx.types.push(t::END);
    lx.texts.push(String::new());
    lx.starts.push(lx.n);
    Tokens {
        types: lx.types,
        texts: lx.texts,
        starts: lx.starts,
    }
}
