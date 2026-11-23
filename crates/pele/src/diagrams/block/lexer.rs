//! block の字句解析（pele の `src/diagrams/block/lexer.ts`）。JS と同じく UTF-16 の符号単位で読む。

use super::tokens as t;
use crate::js;
use crate::util::chars::{NONE, is_space, is_word};

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
    Node,
    BlockArrow,
    ArrowDir,
    LLabel,
    ClassDef,
    ClassDefId,
    Class,
    ClassStyle,
    StyleStmnt,
    StyleDefinition,
    AccTitle,
    AccDescr,
    AccDescrMultiline,
}

const DIRECTIONS: [&str; 6] = ["right", "left", "x", "y", "up", "down"];

/// ノードの名前を終える文字: ( ) [ { } < > - : = と空白。
fn id_stop(c: u32) -> bool {
    matches!(
        c,
        9 | 10 | 11 | 12 | 13 | 32 | 40 | 41 | 45 | 58 | 60 | 61 | 62 | 91 | 123 | 125
    )
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
}

impl Lexer<'_> {
    fn c(&self, i: usize) -> u32 {
        self.src.get(i).map_or(NONE, |&c| c as u32)
    }

    fn starts_with(&self, word: &str, at: usize) -> bool {
        word.bytes()
            .enumerate()
            .all(|(i, b)| self.c(at + i) == b as u32)
    }

    fn index_of(&self, c: u16, from: usize) -> Option<usize> {
        self.src
            .get(from..)?
            .iter()
            .position(|&x| x == c)
            .map(|k| k + from)
    }

    fn slice(&self, a: usize, b: usize) -> String {
        js::from16(&self.src[a.min(self.n)..b.min(self.n).max(a.min(self.n))])
    }

    fn put(&mut self, type_: u8, text: String, end: usize) -> bool {
        self.types.push(type_);
        self.texts.push(text);
        self.starts.push(self.p);
        self.p = end;
        true
    }

    fn emit(&mut self, type_: u8, end: usize) -> bool {
        let text = self.slice(self.p, end);
        self.put(type_, text, end)
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

    fn ws(&self, mut q: usize) -> usize {
        while q < self.n && is_space(self.c(q)) {
            q += 1;
        }
        q
    }

    fn digits(&self, mut q: usize) -> usize {
        while q < self.n {
            let c = self.c(q);
            if !(48..=57).contains(&c) {
                break;
            }
            q += 1;
        }
        q
    }

    fn word(&self, mut q: usize) -> usize {
        while q < self.n && is_word(self.c(q)) {
            q += 1;
        }
        q
    }

    fn keyword(&self, text: &str) -> bool {
        self.starts_with(text, self.p) && !is_word(self.c(self.p + text.len()))
    }

    fn quote(&mut self) -> bool {
        if self.c(self.p + 1) == 96 {
            self.push(S::MdString);
            self.p += 2;
        } else {
            self.push(S::String);
            self.p += 1;
        }
        true
    }

    /// q からの `.+-[xo>]?` の終わり。
    fn dotted(&self, mut q: usize) -> Option<usize> {
        if self.c(q) != 46 {
            return None;
        }
        q += 1;
        while self.c(q) == 46 {
            q += 1;
        }
        if self.c(q) != 45 {
            return None;
        }
        let c = self.c(q + 1);
        Some(if c == 120 || c == 111 || c == 62 {
            q + 2
        } else {
            q + 1
        })
    }

    /// q からの、後ろの空白を除いたリンクの終わり。
    fn link_end(&self, mut q: usize) -> Option<usize> {
        let c = self.c(q);
        if c == 120 || c == 111 || c == 60 {
            q += 1;
        }
        let line = self.c(q);
        if line == 46 {
            return self.dotted(q);
        }
        if line != 45 && line != 61 {
            return None;
        }
        let mut k = q + 1;
        while self.c(k) == line {
            k += 1;
        }
        if k - q == 1 {
            return if line == 45 { self.dotted(k) } else { None };
        }
        let head = self.c(k);
        if head == 120 || head == 111 || head == 62 {
            return Some(k + 1);
        }
        if k - q >= 3 { Some(k) } else { None }
    }

    fn link(&mut self) -> bool {
        if let Some(e) = self.link_end(self.p) {
            let end = self.ws(e);
            return self.emit(t::LINK, end);
        }
        let q = if self.c(self.p) == 60 {
            self.p + 1
        } else {
            self.p
        };
        let a = self.c(q);
        let b = self.c(q + 1);
        if (a == 45 && (b == 45 || b == 46)) || (a == 61 && b == 61) {
            self.push(S::LLabel);
            let end = self.ws(q + 2);
            return self.emit(t::START_LINK, end);
        }
        false
    }

    fn line_state(&mut self, type_: u8) -> bool {
        let e = self.index_of(10, self.p).unwrap_or(self.n);
        self.pop();
        self.emit(type_, e)
    }

    /// `\w+(,\s*\w+)*`
    fn id_list(&mut self, type_: u8, next: S) -> bool {
        let mut k = self.word(self.p);
        if k == self.p {
            return false;
        }
        while self.c(k) == 44 {
            let from = self.ws(k + 1);
            let to = self.word(from);
            if to == from {
                break;
            }
            k = to;
        }
        self.state = next;
        self.emit(type_, k)
    }

    fn initial(&mut self, c: u32) -> bool {
        let p = self.p;
        if is_space(c) {
            self.p = self.ws(p + 1);
            return true;
        }
        match c {
            98 => {
                if self.starts_with("block", p) {
                    if self.keyword("block-beta") {
                        return self.emit(t::KEY, p + 10);
                    }
                    if self.c(p + 5) == 58 {
                        return self.emit(t::ID_BLOCK, p + 6);
                    }
                    if !is_word(self.c(p + 5)) {
                        return self.emit(t::KEY, p + 5);
                    }
                }
            }
            99 => {
                if self.starts_with("columns", p) {
                    let q = self.ws(p + 7);
                    if q > p + 7 {
                        if self.starts_with("auto", q) && !is_word(self.c(q + 4)) {
                            return self.put(t::COLUMNS, "-1".into(), q + 4);
                        }
                        let e = self.digits(q);
                        if e > q {
                            let text = self.slice(q, e);
                            return self.put(t::COLUMNS, text, e);
                        }
                    }
                } else if self.starts_with("class", p) {
                    let def = self.starts_with("Def", p + 5);
                    let from = if def { p + 8 } else { p + 5 };
                    let q = self.ws(from);
                    if q > from {
                        self.push(if def { S::ClassDef } else { S::Class });
                        return self.emit(if def { t::CLASSDEF } else { t::CLASS }, q);
                    }
                }
            }
            34 => return self.quote(),
            115 => {
                if self.starts_with("space", p) {
                    if self.c(p + 5) == 58 {
                        let e = self.digits(p + 6);
                        if e > p + 6 {
                            let text = self.slice(p + 6, e);
                            return self.put(t::SPACE_BLOCK, text, e);
                        }
                    }
                    if !is_word(self.c(p + 5)) {
                        return self.put(t::SPACE_BLOCK, "1".into(), p + 5);
                    }
                } else if self.starts_with("style", p) {
                    let q = self.ws(p + 5);
                    if q > p + 5 {
                        self.push(S::StyleStmnt);
                        return self.emit(t::STYLE, q);
                    }
                }
            }
            100 => {
                if self.keyword("default") {
                    return self.emit(t::DEFAULT, p + 7);
                }
            }
            108 => {
                if self.keyword("linkStyle") {
                    return self.emit(t::LINKSTYLE, p + 9);
                }
            }
            105 => {
                if self.keyword("interpolate") {
                    return self.emit(t::INTERPOLATE, p + 11);
                }
            }
            97 => {
                if self.starts_with("accTitle", p) {
                    let q = self.ws(p + 8);
                    if self.c(q) == 58 {
                        self.push(S::AccTitle);
                        let end = self.ws(q + 1);
                        return self.emit(t::ACC_TITLE, end);
                    }
                } else if self.starts_with("accDescr", p) {
                    let q = self.ws(p + 8);
                    let d = self.c(q);
                    if d == 58 {
                        self.push(S::AccDescr);
                        let end = self.ws(q + 1);
                        return self.emit(t::ACC_DESCR, end);
                    }
                    if d == 123 {
                        self.push(S::AccDescrMultiline);
                        self.p = self.ws(q + 1);
                        return true;
                    }
                }
            }
            101 => {
                if self.keyword("end") {
                    let end = self.ws(p + 3);
                    return self.emit(t::END_KW, end);
                }
            }
            45 => {
                if self.c(p + 1) == 41 {
                    self.push(S::Node);
                    return self.emit(t::NODE_DSTART, p + 2);
                }
                return self.link();
            }
            61 => return self.link(),
            60 => {
                if self.c(p + 1) == 91 {
                    self.push(S::BlockArrow);
                    return self.emit(t::BLOCK_ARROW_START, p + 2);
                }
                return self.link();
            }
            40 => {
                let d = self.c(p + 1);
                let len = if d == 45 || d == 91 {
                    2
                } else if d == 40 {
                    if self.c(p + 2) == 40 { 3 } else { 2 }
                } else {
                    1
                };
                self.push(S::Node);
                return self.emit(t::NODE_DSTART, p + len);
            }
            41 | 123 => {
                self.push(S::Node);
                let end = if self.c(p + 1) == c { p + 2 } else { p + 1 };
                return self.emit(t::NODE_DSTART, end);
            }
            62 => {
                self.push(S::Node);
                return self.emit(t::NODE_DSTART, p + 1);
            }
            91 => {
                let d = self.c(p + 1);
                self.push(S::Node);
                let end = if d == 91 || d == 124 || d == 40 || d == 92 || d == 47 {
                    p + 2
                } else {
                    p + 1
                };
                return self.emit(t::NODE_DSTART, end);
            }
            58 => {
                let e = self.digits(p + 1);
                if e > p + 1 {
                    let text = self.slice(p + 1, e);
                    return self.put(t::SIZE, text, e);
                }
                return false;
            }
            _ => {}
        }

        let mut q = p;
        while q < self.n {
            let ch = self.c(q);
            if if ch < 128 { id_stop(ch) } else { is_space(ch) } {
                break;
            }
            q += 1;
        }
        q > p && self.emit(t::NODE_ID, q)
    }

    fn node_end(&mut self, c: u32) -> bool {
        let p = self.p;
        let d = self.c(p + 1);
        let len = match c {
            34 => return self.quote(),
            40 => {
                if d == 45 {
                    2
                } else if d == 40 {
                    if self.c(p + 2) == 40 { 3 } else { 2 }
                } else {
                    1
                }
            }
            41 => {
                if d == 41 {
                    if self.c(p + 2) == 41 { 3 } else { 2 }
                } else if d == 93 {
                    2
                } else {
                    1
                }
            }
            125 => {
                if d == 125 {
                    2
                } else {
                    1
                }
            }
            45 => {
                if d == 41 {
                    2
                } else {
                    0
                }
            }
            93 => {
                if d == 93 || d == 41 || d == 62 {
                    2
                } else {
                    1
                }
            }
            92 | 47 if d == 93 => 2,
            _ => 0,
        };
        if len == 0 {
            return false;
        }
        self.pop();
        self.emit(t::NODE_DEND, p + len)
    }

    fn arrow_dir(&mut self, c: u32) -> bool {
        let p = self.p;
        if c == 41 {
            self.pop();
            self.pop();
            let end = self.ws(p + 1);
            return self.put(t::BLOCK_ARROW_END, "]>".into(), end);
        }
        let from = self.ws(if c == 44 { p + 1 } else { p });
        for dir in DIRECTIONS {
            if self.starts_with(dir, from) {
                let e = self.ws(from + dir.len());
                let text = self.slice(if c == 44 { from } else { p }, e);
                return self.put(t::DIR, text, e);
            }
        }
        false
    }

    fn run(&mut self) {
        loop {
            if self.p >= self.n {
                let type_ = match self.state {
                    S::Initial => Some(t::EOF),
                    S::String => Some(t::STR),
                    S::ClassDefId => Some(t::CLASSDEF_STYLEOPTS),
                    S::ClassStyle => Some(t::STYLECLASS),
                    S::StyleDefinition => Some(t::STYLE_DEFINITION_DATA),
                    S::AccTitle => Some(t::ACC_TITLE_VALUE),
                    S::AccDescr => Some(t::ACC_DESCR_VALUE),
                    S::AccDescrMultiline => Some(t::ACC_DESCR_MULTILINE_VALUE),
                    _ => None,
                };
                self.p = self.n;
                if let Some(type_) = type_ {
                    self.put(type_, String::new(), self.n);
                }
                break;
            }

            let c = self.c(self.p);
            let p = self.p;
            let ok = match self.state {
                S::Initial => self.initial(c),
                S::String => {
                    if c == 34 {
                        self.pop();
                        self.p += 1;
                    } else {
                        let e = self.index_of(34, p).unwrap_or(self.n);
                        self.emit(t::STR, e);
                    }
                    true
                }
                S::MdString => {
                    if c == 96 {
                        let ok = self.c(p + 1) == 34;
                        if ok {
                            self.pop();
                            self.p += 2;
                        }
                        ok
                    } else if c == 34 {
                        false
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
                        true
                    }
                }
                S::Node => self.node_end(c),
                S::BlockArrow => {
                    if c == 34 {
                        self.quote();
                        true
                    } else if c == 93 && self.c(p + 1) == 62 {
                        let q = self.ws(p + 2);
                        let ok = self.c(q) == 40;
                        if ok {
                            self.push(S::ArrowDir);
                            self.p = q + 1;
                        }
                        ok
                    } else {
                        false
                    }
                }
                S::ArrowDir => self.arrow_dir(c),
                S::LLabel => {
                    if c == 34 {
                        if self.c(p + 1) == 96 {
                            self.quote();
                        } else {
                            self.emit(t::LINK_LABEL, p + 1);
                            self.push(S::String);
                        }
                        true
                    } else {
                        match self.link_end(self.ws(p)) {
                            Some(e) => {
                                self.pop();
                                let end = self.ws(e);
                                self.emit(t::LINK, end);
                                true
                            }
                            None => false,
                        }
                    }
                }
                S::ClassDef => {
                    let mut type_ = t::DEFAULT_CLASSDEF_ID;
                    let mut k = if self.starts_with("DEFAULT", p) {
                        p + 7
                    } else {
                        p
                    };
                    let mut q = self.ws(k);
                    if k == p || q == k {
                        type_ = t::CLASSDEF_ID;
                        k = self.word(p);
                        q = self.ws(k);
                    }
                    let ok = k > p && q > k;
                    if ok {
                        self.state = S::ClassDefId;
                        self.emit(type_, q);
                    }
                    ok
                }
                S::ClassDefId => self.line_state(t::CLASSDEF_STYLEOPTS),
                S::Class => self.id_list(t::CLASSENTITY_IDS, S::ClassStyle),
                S::ClassStyle => self.line_state(t::STYLECLASS),
                S::StyleStmnt => self.id_list(t::STYLE_ENTITY_IDS, S::StyleDefinition),
                S::StyleDefinition => self.line_state(t::STYLE_DEFINITION_DATA),
                S::AccTitle => self.line_state(t::ACC_TITLE_VALUE),
                S::AccDescr => self.line_state(t::ACC_DESCR_VALUE),
                S::AccDescrMultiline => {
                    if c == 125 {
                        self.pop();
                        self.p += 1;
                    } else {
                        let e = self.index_of(125, p).unwrap_or(self.n);
                        self.emit(t::ACC_DESCR_MULTILINE_VALUE, e);
                    }
                    true
                }
            };

            if !ok {
                self.types.push(t::ERROR);
                self.texts.push(String::new());
                self.starts.push(self.p);
                return;
            }
        }
        self.types.push(t::END);
        self.texts.push(String::new());
        self.starts.push(self.n);
    }
}

pub fn tokenize(src: &[u16]) -> Tokens {
    let mut lexer = Lexer {
        src,
        n: src.len(),
        types: Vec::new(),
        texts: Vec::new(),
        starts: Vec::new(),
        stack: Vec::new(),
        state: S::Initial,
        p: 0,
    };
    lexer.run();
    Tokens {
        types: lexer.types,
        texts: lexer.texts,
        starts: lexer.starts,
    }
}
