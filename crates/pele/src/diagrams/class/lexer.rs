//! class の字句解析（pele の `src/diagrams/class/lexer.ts`）。JS と同じく UTF-16 の符号単位で読む。

use super::tokens as t;
use crate::util::chars::{NONE, is_line_end, is_space, is_word};
use crate::util::unicode::is_unicode_letter;

pub struct Tokens {
    pub types: Vec<u8>,
    pub texts: Vec<String>,
    pub starts: Vec<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum S {
    Initial,
    String,
    Bqstring,
    Generic,
    CallbackName,
    CallbackArgs,
    AccTitle,
    AccDescr,
    AccDescrMultiline,
    Cls,
    ClassBody,
    Namespace,
    NamespaceBody,
}

/// 入力の終わりでそれぞれの状態が出す字句（読み手が終わりそのものを見る前に）。
fn at_end(s: S) -> u8 {
    match s {
        S::Initial => t::EOF,
        S::String => t::STR,
        S::Bqstring => t::EOF,
        S::Generic => t::GENERICTYPE,
        S::CallbackName => t::CALLBACK_NAME,
        S::CallbackArgs => t::CALLBACK_ARGS,
        S::AccTitle => t::acc_title_value,
        S::AccDescr => t::acc_descr_value,
        S::AccDescrMultiline => t::acc_descr_multiline_value,
        S::Cls => t::EOF,
        S::ClassBody => t::EOF_IN_STRUCT,
        S::Namespace => t::EOF,
        S::NamespaceBody => t::EOF_IN_STRUCT,
    }
}

const DIRECTIONS: [&str; 4] = ["TB", "BT", "RL", "LR"];

struct Lexer<'a> {
    src: &'a [u16],
    n: usize,
    types: Vec<u8>,
    texts: Vec<String>,
    starts: Vec<usize>,
    stack: Vec<S>,
    state: S,
    p: usize,
    dir_next: Option<usize>,
    line_end: isize,
    dir_failed: usize,
}

impl Lexer<'_> {
    fn c(&self, i: usize) -> u32 {
        self.src.get(i).map_or(NONE, |&c| c as u32)
    }

    fn starts_with(&self, word: &str, at: usize) -> bool {
        let w: Vec<u16> = word.encode_utf16().collect();
        self.src.len() >= at + w.len() && self.src[at..at + w.len()] == w[..]
    }

    fn index_of(&self, word: &str, from: usize) -> Option<usize> {
        let w: Vec<u16> = word.encode_utf16().collect();
        if self.src.len() < w.len() {
            return None;
        }
        (from..=self.src.len() - w.len()).find(|&k| self.src[k..k + w.len()] == w[..])
    }

    fn until(&self, ch: u16) -> usize {
        let from = self.p;
        if from >= self.n {
            return self.n;
        }
        self.src[from..]
            .iter()
            .position(|&x| x == ch)
            .map_or(self.n, |k| k + from)
    }

    fn emit(&mut self, type_: u8, s: usize, e: usize) -> bool {
        self.types.push(type_);
        self.texts.push(String::from_utf16_lossy(&self.src[s..e]));
        self.starts.push(s);
        self.p = e;
        true
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

    fn kw(&self, word: &str) -> bool {
        self.starts_with(word, self.p) && !is_word(self.c(self.p + word.len()))
    }

    fn skip_ws(&self, mut q: usize) -> usize {
        while q < self.n && is_space(self.c(q)) {
            q += 1;
        }
        q
    }

    /// 空白の並びは最後の改行までが NEWLINE。残りは飛ばす。
    fn space(&mut self, leaves: bool) {
        let mut q = self.p;
        let mut k: Option<usize> = None;
        while q < self.n && is_space(self.c(q)) {
            if self.c(q) == 10 {
                k = Some(q);
            }
            q += 1;
        }
        let Some(k) = k else {
            self.p = q;
            return;
        };
        if leaves {
            self.pop();
        }
        self.emit(t::NEWLINE, self.p, k + 1);
    }

    /// どの状態でも、総称の型や逆引用符の名前の中でも分かる語。
    fn keyword(&mut self, c: u32) -> bool {
        let p = self.p;
        let found = match c {
            99 => [
                ("cssClass", t::CSSCLASS),
                ("callback", t::CALLBACK),
                ("click", t::CLICK),
            ]
            .into_iter()
            .find(|(w, _)| self.kw(w)),
            108 => [("link", t::LINK)].into_iter().find(|(w, _)| self.kw(w)),
            110 => [("note for", t::NOTE_FOR), ("note", t::NOTE)]
                .into_iter()
                .find(|(w, _)| self.kw(w)),
            104 => [("href", t::HREF)].into_iter().find(|(w, _)| self.kw(w)),
            60 if self.c(p + 1) == 60 => Some(("<<", t::ANNOTATION_START)),
            62 if self.c(p + 1) == 62 => Some((">>", t::ANNOTATION_END)),
            _ => None,
        };
        match found {
            Some((w, type_)) => self.emit(type_, p, p + w.len()),
            None => false,
        }
    }

    fn common(&mut self, c: u32) -> bool {
        if self.keyword(c) {
            return true;
        }
        let p = self.p;
        let d = self.c(p + 1);
        match c {
            34 => {
                self.push(S::String);
                self.p += 1;
                return true;
            }
            126 => {
                self.push(S::Generic);
                self.p += 1;
                return true;
            }
            96 => {
                self.push(S::Bqstring);
                self.p += 1;
                return true;
            }
            95 => {
                if self.kw("_self") {
                    return self.emit(t::LINK_TARGET, p, p + 5);
                }
                if self.kw("_blank") {
                    return self.emit(t::LINK_TARGET, p, p + 6);
                }
                if self.kw("_parent") {
                    return self.emit(t::LINK_TARGET, p, p + 7);
                }
                if self.kw("_top") {
                    return self.emit(t::LINK_TARGET, p, p + 4);
                }
            }
            60 => {
                return if d == 124 {
                    self.emit(t::EXTENSION, p, p + 2)
                } else {
                    self.emit(t::DEPENDENCY, p, p + 1)
                };
            }
            124 => return d == 62 && self.emit(t::EXTENSION, p, p + 2),
            62 => return self.emit(t::DEPENDENCY, p, p + 1),
            42 => return self.emit(t::COMPOSITION, p, p + 1),
            111 if !is_word(d) => {
                return self.emit(t::AGGREGATION, p, p + 1);
            }
            40 => return d == 41 && self.emit(t::LOLLIPOP, p, p + 2),
            45 => {
                return if d == 45 {
                    self.emit(t::LINE, p, p + 2)
                } else {
                    self.emit(t::MINUS, p, p + 1)
                };
            }
            46 => {
                return if d == 46 {
                    self.emit(t::DOTTED_LINE, p, p + 2)
                } else {
                    self.emit(t::DOT, p, p + 1)
                };
            }
            58 => {
                let mut q = p + 1;
                while q < self.n {
                    let ch = self.c(q);
                    if ch == 58 || ch == 10 || ch == 59 {
                        break;
                    }
                    q += 1;
                }
                if q > p + 1 {
                    return self.emit(t::LABEL, p, q);
                }
                return d == 58 && self.c(p + 2) == 58 && self.emit(t::STYLE_SEPARATOR, p, p + 3);
            }
            43 => return self.emit(t::PLUS, p, p + 1),
            37 => return self.emit(t::PCT, p, p + 1),
            61 => return self.emit(t::EQUALS, p, p + 1),
            91 => return self.emit(t::SQS, p, p + 1),
            93 => return self.emit(t::SQE, p, p + 1),
            33 | 35 | 36 | 38 | 39 | 44 | 63 | 92 | 47 => {
                return self.emit(t::PUNCTUATION, p, p + 1);
            }
            _ => {}
        }
        if is_word(c) {
            let mut q = p + 1;
            while is_word(self.c(q)) {
                q += 1;
            }
            return self.emit(t::ALPHA, p, q);
        }
        is_unicode_letter(c) && self.emit(t::UNICODE_TEXT, p, p + 1)
    }

    /// `.*direction\s+XX[^\n]*`: 行の終わりの文字までで、いちばん後ろの `direction` から。
    fn direction_at(&self, q: usize, i: usize) -> Option<usize> {
        let mut stop = q;
        while stop < self.n && !is_line_end(self.c(stop)) {
            stop += 1;
        }
        let dir: Vec<u16> = DIRECTIONS[i].encode_utf16().collect();
        let mut k = stop;
        while k > q {
            k -= 1;
            if k + 9 <= stop && self.starts_with("direction", k) {
                let s = k + 9;
                let e = self.skip_ws(s);
                if e > s && self.src.len() >= e + 2 && self.src[e..e + 2] == dir[..] {
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

    fn direction(&mut self) -> bool {
        let p = self.p;
        if self.dir_next.is_none() || p < self.dir_failed {
            return false;
        }
        if self.dir_next.is_some_and(|d| d < p) {
            self.dir_next = self.index_of("direction", p);
        }
        let Some(dir_next) = self.dir_next else {
            return false;
        };
        if self.line_end < p as isize {
            let mut q = p;
            while q < self.n && !is_line_end(self.c(q)) {
                q += 1;
            }
            self.line_end = q as isize;
        }
        if dir_next as isize >= self.line_end {
            return false;
        }
        for i in 0..4 {
            if let Some(e) = self.direction_at(p, i) {
                return self.emit(t::direction_tb + i as u8, p, e);
            }
        }
        // この行の残りのどこにも、向きの文は始まらない。
        self.dir_failed = self.line_end as usize;
        false
    }

    fn initial(&mut self, c: u32) -> bool {
        if self.direction() {
            return true;
        }
        if is_space(c) {
            self.space(false);
            return true;
        }
        let p = self.p;
        match c {
            37 if self.c(p + 1) == 37 => {
                let mut e = self.until(10);
                while e < self.n && (self.c(e) == 10 || self.c(e) == 13) {
                    e += 1;
                }
                self.p = e;
                return true;
            }
            97 => {
                let title = self.starts_with("accTitle", p);
                if title || self.starts_with("accDescr", p) {
                    let q = self.skip_ws(p + 8);
                    let d = self.c(q);
                    if d == 58 {
                        let e = self.skip_ws(q + 1);
                        self.emit(if title { t::acc_title } else { t::acc_descr }, p, e);
                        self.push(if title { S::AccTitle } else { S::AccDescr });
                        return true;
                    }
                    if d == 123 && !title {
                        self.p = self.skip_ws(q + 1);
                        self.push(S::AccDescrMultiline);
                        return true;
                    }
                }
            }
            99 => {
                if self.kw("classDiagram-v2") {
                    return self.emit(t::CLASS_DIAGRAM, p, p + 15);
                }
                if self.kw("classDiagram") {
                    return self.emit(t::CLASS_DIAGRAM, p, p + 12);
                }
                if self.starts_with("call", p) && is_space(self.c(p + 4)) {
                    self.p = self.skip_ws(p + 4);
                    self.push(S::CallbackName);
                    return true;
                }
                if self.kw("classDef") {
                    return self.emit(t::CLASSDEF, p, p + 8);
                }
                if self.kw("class") {
                    self.push(S::Cls);
                    return self.emit(t::CLASS, p, p + 5);
                }
            }
            91 if self.starts_with("*]", p + 1) => {
                return self.emit(t::EDGE_STATE, p, p + 3);
            }
            115 if self.kw("style") => {
                return self.emit(t::STYLE, p, p + 5);
            }
            110 if self.kw("namespace") => {
                self.push(S::Namespace);
                return self.emit(t::NAMESPACE, p, p + 9);
            }
            44 => return self.emit(t::COMMA, p, p + 1),
            35 => return self.emit(t::BRKT, p, p + 1),
            _ => {}
        }
        self.common(c) || (c == 58 && self.emit(t::COLON, p, p + 1))
    }

    fn step(&mut self, c: u32) -> bool {
        let p = self.p;
        match self.state {
            S::Initial => return self.initial(c),
            S::Cls => {
                if is_space(c) {
                    self.space(true);
                } else if c == 125 {
                    self.pop();
                    self.pop();
                    self.emit(t::STRUCT_STOP, p, p + 1);
                } else if c == 123 {
                    self.push(S::ClassBody);
                    self.emit(t::STRUCT_START, p, p + 1);
                } else {
                    return self.common(c);
                }
            }
            S::ClassBody => {
                if c == 34 {
                    self.push(S::String);
                    self.p += 1;
                } else if c == 125 {
                    self.pop();
                    self.emit(t::STRUCT_STOP, p, p + 1);
                } else if c == 91 && self.starts_with("*]", p + 1) {
                    self.emit(t::EDGE_STATE, p, p + 3);
                } else if c == 123 {
                    self.emit(t::OPEN_IN_STRUCT, p, p + 1);
                } else if c == 10 {
                    self.p += 1;
                } else {
                    let mut q = p + 1;
                    while q < self.n {
                        let ch = self.c(q);
                        if ch == 123 || ch == 125 || ch == 10 {
                            break;
                        }
                        q += 1;
                    }
                    self.emit(t::MEMBER, p, q);
                }
            }
            S::Namespace => {
                if is_space(c) {
                    self.space(true);
                } else if c == 123 {
                    self.push(S::NamespaceBody);
                    self.emit(t::STRUCT_START, p, p + 1);
                } else if c == 125 {
                    // 括弧は、名前空間を開いた状態でもう一度読む。
                    self.pop();
                } else if c == 110 && self.kw("namespace") {
                    self.push(S::Namespace);
                    self.emit(t::NAMESPACE, p, p + 9);
                } else {
                    return self.common(c);
                }
            }
            S::NamespaceBody => {
                if is_space(c) {
                    self.space(false);
                } else if c == 125 {
                    self.pop();
                    self.emit(t::STRUCT_STOP, p, p + 1);
                } else if c == 110 && self.kw("namespace") {
                    self.push(S::Namespace);
                    self.emit(t::NAMESPACE, p, p + 9);
                } else if c == 91 && self.starts_with("*]", p + 1) {
                    self.emit(t::EDGE_STATE, p, p + 3);
                } else if c == 99 && self.kw("class") {
                    self.push(S::Cls);
                    self.emit(t::CLASS, p, p + 5);
                } else {
                    return self.common(c);
                }
            }
            S::String => {
                if c == 34 {
                    self.pop();
                    self.p += 1;
                } else {
                    let e = self.until(34);
                    self.emit(t::STR, p, e);
                }
            }
            S::Generic => {
                if c == 34 {
                    self.push(S::String);
                    self.p += 1;
                } else if self.keyword(c) {
                } else if c == 126 {
                    self.pop();
                    self.p += 1;
                } else {
                    let e = self.until(126);
                    self.emit(t::GENERICTYPE, p, e);
                }
            }
            S::Bqstring => {
                if c == 34 {
                    self.push(S::String);
                    self.p += 1;
                } else if self.keyword(c) {
                } else if c == 126 {
                    self.push(S::Generic);
                    self.p += 1;
                } else if c == 96 {
                    self.pop();
                    self.p += 1;
                } else {
                    let e = self.until(96);
                    self.emit(t::BQUOTE_STR, p, e);
                }
            }
            S::CallbackName => {
                if c == 40 {
                    let q = self.skip_ws(p + 1);
                    self.pop();
                    if self.c(q) == 41 {
                        self.p = q + 1;
                    } else {
                        self.push(S::CallbackArgs);
                        self.p += 1;
                    }
                } else {
                    let e = self.until(40);
                    self.emit(t::CALLBACK_NAME, p, e);
                }
            }
            S::CallbackArgs => {
                if c == 41 {
                    self.pop();
                    self.p += 1;
                } else {
                    let e = self.until(41);
                    self.emit(t::CALLBACK_ARGS, p, e);
                }
            }
            S::AccTitle | S::AccDescr => {
                let e = self.until(10);
                let type_ = if self.state == S::AccTitle {
                    t::acc_title_value
                } else {
                    t::acc_descr_value
                };
                self.emit(type_, p, e);
                self.pop();
            }
            S::AccDescrMultiline => {
                if c == 125 {
                    self.pop();
                    self.p += 1;
                } else {
                    let e = self.until(125);
                    self.emit(t::acc_descr_multiline_value, p, e);
                }
            }
        }
        true
    }
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
        dir_next: None,
        line_end: -1,
        dir_failed: 0,
    };
    lx.dir_next = lx.index_of("direction", 0);
    loop {
        if lx.p >= lx.n {
            lx.types.push(at_end(lx.state));
            lx.texts.push(String::new());
            lx.starts.push(lx.n);
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
