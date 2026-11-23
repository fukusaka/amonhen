//! sequence の字句（pele の `src/diagrams/sequence/lexer.ts`）。Mermaid の Jison の字句の規則と状態を手で写した
//! もの。位置は UTF-16 の符号単位。

use super::tokens::t;
use crate::js;
use crate::util::chars::{NONE, is_digit, is_space, is_word};

pub struct Tokens {
    pub types: Vec<u8>,
    pub texts: Vec<String>,
    pub starts: Vec<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum S {
    Initial,
    Id,
    Alias,
    Line,
    Config,
    AccTitle,
    AccDescr,
    AccDescrMultiline,
}

mod c {
    pub const ACTOR_START: u8 = 1;
    pub const ACTOR_REST: u8 = 2;
    pub const ID: u8 = 4;
    pub const ID_WIDE: u8 = 8;
    pub const LINE: u8 = 16;
    pub const MENU: u8 = 32;
}

/// Mermaid の字句の規則の文字の組（ASCII）。ほかの文字は空白かどうかで分ける。
fn class(ch: u32) -> u8 {
    let x = char::from_u32(ch).unwrap_or('\0');
    let ws = ch == 32 || (9..=13).contains(&ch);
    let mut bits = 0;
    if !"/\\+()<->:\n,;".contains(x) {
        bits |= c::ACTOR_START;
    }
    if !"+<->:\n,;".contains(x) {
        bits |= c::ACTOR_REST;
    }
    if !"<>:\n,;@".contains(x) {
        bits |= c::ID_WIDE | if ws { 0 } else { c::ID };
    }
    if !"#\n;".contains(x) {
        bits |= c::LINE;
    }
    if !"/\\+()<>:\n,;-".contains(x) && !ws {
        bits |= c::MENU;
    }
    bits
}

struct Lexer<'a> {
    src: &'a [u16],
    n: usize,
    out: Tokens,
    stack: Vec<S>,
    state: S,
    p: usize,
    next_nl: Option<usize>,
    classes: [u8; 128],
}

impl Lexer<'_> {
    fn at(&self, q: usize) -> u32 {
        self.src.get(q).map_or(NONE, |&c| c as u32)
    }

    fn emit(&mut self, type_: u8, s: usize, e: usize) {
        let text = js::from16(&self.src[s..e]);
        self.emit_text(type_, s, e, text);
    }

    fn emit_text(&mut self, type_: u8, s: usize, e: usize, text: String) {
        self.out.types.push(type_);
        self.out.texts.push(text);
        self.out.starts.push(s);
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

    fn index_of(&self, ch: u16, from: usize) -> usize {
        self.src
            .get(from..)
            .and_then(|s| s.iter().position(|&c| c == ch))
            .map_or(self.n, |k| from + k)
    }

    fn eol(&mut self, q: usize) -> usize {
        match self.next_nl {
            Some(nl) if nl >= q => nl,
            _ => {
                let nl = self.index_of(10, q);
                self.next_nl = Some(nl);
                nl
            }
        }
    }

    fn has(&self, q: usize, bit: u8) -> bool {
        let c = self.at(q);
        if c < 128 {
            return self.classes[c as usize] & bit != 0;
        }
        if bit == c::ID || bit == c::MENU {
            !is_space(c)
        } else {
            true
        }
    }

    /// 小文字の文字どおりの語に、大文字小文字を問わずに合うか。
    fn ci(&self, q: usize, word: &str) -> bool {
        word.bytes().enumerate().all(|(k, w)| {
            let c = self.at(q + k);
            if w.is_ascii_lowercase() {
                c | 32 == w as u32
            } else {
                c == w as u32
            }
        })
    }

    fn word(&self, lit: &str) -> bool {
        self.ci(self.p, lit) && !is_word(self.at(self.p + lit.len()))
    }

    fn skip_ws(&self, mut q: usize) -> usize {
        while q < self.n && is_space(self.src[q] as u32) {
            q += 1;
        }
        q
    }

    fn skip_same_line(&mut self) -> bool {
        let mut q = self.p;
        while q < self.n {
            let c = self.src[q] as u32;
            if c == 10 || !is_space(c) {
                break;
            }
            q += 1;
        }
        if q == self.p {
            return false;
        }
        self.p = q;
        true
    }

    fn line_run(&self, mut q: usize) -> usize {
        while q < self.n && self.has(q, c::LINE) {
            q += 1;
        }
        q
    }

    fn begin(&mut self, type_: u8, len: usize, next: S) {
        let p = self.p;
        self.emit(type_, p, p + len);
        self.push(next);
    }

    fn keyword(&mut self, lit: &str, type_: u8, next: Option<S>) -> bool {
        if !self.word(lit) {
            return false;
        }
        let p = self.p;
        self.emit(type_, p, p + lit.len());
        if let Some(next) = next {
            self.push(next);
        }
        true
    }

    /// 参加者のメニューの語は、空白と名前の始まりが続くときだけ数える。
    fn menu(&mut self, lit: &str, type_: u8) -> bool {
        if !self.ci(self.p, lit) {
            return false;
        }
        let mut q = self.p + lit.len();
        let mut ch = self.at(q);
        if ch != 32 && ch != 9 {
            return false;
        }
        while ch == 32 || ch == 9 {
            q += 1;
            ch = self.at(q);
        }
        if q >= self.n || !self.has(q, c::MENU) {
            return false;
        }
        let p = self.p;
        self.emit(type_, p, p + lit.len());
        true
    }

    fn arrow_ahead(&self, q: usize) -> bool {
        let ch = self.at(q);
        let d = self.at(q + 1);
        if ch == 45 {
            if matches!(d, 120 | 88 | 41 | 92 | 47 | 45) {
                return true;
            }
            let e = self.at(q + 2);
            return d == 124 && (e == 92 || e == 47);
        }
        if ch == 47 || ch == 92 {
            return (d == 124 || d == ch) && self.at(q + 2) == 45;
        }
        ch == 40 && d == 41
    }

    fn actor(&mut self) {
        let p = self.p;
        let mut q = p + 1;
        while q < self.n && self.has(q, c::ACTOR_START) {
            q += 1;
        }
        while q < self.n && !self.arrow_ahead(q) {
            let mut r = q;
            while self.at(r) == 45 {
                r += 1;
            }
            if r >= self.n || !self.has(r, c::ACTOR_REST) {
                break;
            }
            r += 1;
            while r < self.n && self.has(r, c::ACTOR_REST) {
                r += 1;
            }
            q = r;
        }
        let text = js::trim(&js::from16(&self.src[p..q])).to_string();
        self.emit_text(t::ACTOR, p, q, text);
    }

    fn number(&mut self) -> bool {
        let p = self.p;
        let mut q = p;
        while is_digit(self.at(q)) {
            q += 1;
        }
        let whole = q > p;
        if self.at(q) == 46 && is_digit(self.at(q + 1)) {
            let r = if is_digit(self.at(q + 2)) {
                q + 3
            } else {
                q + 2
            };
            let after = self.at(r);
            if after == 32 || after == 10 {
                self.emit(t::NUM, p, r);
                return true;
            }
        }
        let after = self.at(q);
        if whole && (after == 32 || after == 10) {
            self.emit(t::NUM, p, q);
            return true;
        }
        false
    }

    fn dash(&mut self) {
        let p = self.p;
        let d = self.at(p + 1);
        let e = self.at(p + 2);
        let (type_, len) = if d == 62 {
            if e == 62 {
                (t::SOLID_ARROW, 3)
            } else {
                (t::SOLID_OPEN_ARROW, 2)
            }
        } else if d == 120 || d == 88 {
            (t::SOLID_CROSS, 2)
        } else if d == 41 {
            (t::SOLID_POINT, 2)
        } else if d == 124 && e == 92 {
            (t::SOLID_ARROW_TOP, 3)
        } else if d == 124 && e == 47 {
            (t::SOLID_ARROW_BOTTOM, 3)
        } else if d == 92 && e == 92 {
            (t::STICK_ARROW_TOP, 3)
        } else if d == 47 && e == 47 {
            (t::STICK_ARROW_BOTTOM, 3)
        } else if d == 45 {
            let f = self.at(p + 3);
            if e == 62 {
                if f == 62 {
                    (t::DOTTED_ARROW, 4)
                } else {
                    (t::DOTTED_OPEN_ARROW, 3)
                }
            } else if e == 120 || e == 88 {
                (t::DOTTED_CROSS, 3)
            } else if e == 41 {
                (t::DOTTED_POINT, 3)
            } else if e == 124 && f == 92 {
                (t::SOLID_ARROW_TOP_DOTTED, 4)
            } else if e == 124 && f == 47 {
                (t::SOLID_ARROW_BOTTOM_DOTTED, 4)
            } else if e == 92 && f == 92 {
                (t::STICK_ARROW_TOP_DOTTED, 4)
            } else if e == 47 && f == 47 {
                (t::STICK_ARROW_BOTTOM_DOTTED, 4)
            } else {
                (t::MINUS, 1)
            }
        } else {
            (t::MINUS, 1)
        };
        self.emit(type_, p, p + len);
    }

    fn reverse(&mut self, ch: u32) -> bool {
        let p = self.p;
        let d = self.at(p + 1);
        if (d != 124 && d != ch) || self.at(p + 2) != 45 {
            return false;
        }
        let dotted = self.at(p + 3) == 45;
        let top = ch == 47;
        let type_ = if d == 124 {
            match (dotted, top) {
                (true, true) => t::SOLID_ARROW_TOP_REVERSE_DOTTED,
                (true, false) => t::SOLID_ARROW_BOTTOM_REVERSE_DOTTED,
                (false, true) => t::SOLID_ARROW_TOP_REVERSE,
                (false, false) => t::SOLID_ARROW_BOTTOM_REVERSE,
            }
        } else {
            match (dotted, top) {
                (true, true) => t::STICK_ARROW_TOP_REVERSE_DOTTED,
                (true, false) => t::STICK_ARROW_BOTTOM_REVERSE_DOTTED,
                (false, true) => t::STICK_ARROW_TOP_REVERSE,
                (false, false) => t::STICK_ARROW_BOTTOM_REVERSE,
            }
        };
        self.emit(type_, p, p + if dotted { 4 } else { 3 });
        true
    }

    fn title(&mut self) -> bool {
        if !self.ci(self.p, "title") {
            return false;
        }
        let p = self.p;
        let mut q = p + 5;
        let legacy = self.at(q) == 58;
        if legacy {
            q += 1;
        }
        let mut e = q;
        while e < self.n && self.src[e] != 10 && is_space(self.src[e] as u32) {
            e += 1;
        }
        if e == self.n || self.src[e] == 10 {
            self.p = e;
            return true;
        }
        let sep = self.at(q);
        if !is_space(sep) || sep == 13 || !self.has(q + 1, c::LINE) {
            return false;
        }
        let end = self.line_run(q + 1);
        self.emit(if legacy { t::LEGACY_TITLE } else { t::TITLE }, p, end);
        true
    }

    fn acc(&mut self) -> bool {
        let descr = self.ci(self.p, "accdescr");
        if !descr && !self.ci(self.p, "acctitle") {
            return false;
        }
        let q = self.skip_ws(self.p + 8);
        let ch = self.at(q);
        if ch == 58 {
            let p = self.p;
            let e = self.skip_ws(q + 1);
            self.emit(if descr { t::ACC_DESCR } else { t::ACC_TITLE }, p, e);
            self.push(if descr { S::AccDescr } else { S::AccTitle });
            return true;
        }
        if descr && ch == 123 {
            self.p = self.skip_ws(q + 1);
            self.push(S::AccDescrMultiline);
            return true;
        }
        false
    }

    fn keywords(&mut self, ch: u32) -> bool {
        match ch | 32 {
            97 => {
                self.keyword("actor", t::PARTICIPANT_ACTOR, Some(S::Id))
                    || self.keyword("alt", t::ALT, Some(S::Line))
                    || self.keyword("and", t::AND, Some(S::Line))
                    || self.keyword("activate", t::ACTIVATE, Some(S::Id))
                    || self.acc()
                    || self.keyword("autonumber", t::AUTONUMBER, None)
            }
            98 => {
                self.keyword("box", t::BOX, Some(S::Line))
                    || self.keyword("break", t::BREAK, Some(S::Line))
            }
            99 => {
                self.keyword("create", t::CREATE, None)
                    || self.keyword("critical", t::CRITICAL, Some(S::Line))
            }
            100 => {
                self.keyword("destroy", t::DESTROY, Some(S::Id))
                    || self.menu("details", t::DETAILS)
                    || self.keyword("deactivate", t::DEACTIVATE, Some(S::Id))
            }
            101 => {
                self.keyword("else", t::ELSE, Some(S::Line)) || self.keyword("end", t::END_KW, None)
            }
            108 => {
                self.keyword("loop", t::LOOP, Some(S::Line))
                    || self.keyword("left of", t::LEFT_OF, None)
                    || self.menu("links", t::LINKS)
                    || self.menu("link", t::LINK)
            }
            110 => self.keyword("note", t::NOTE, None),
            111 => {
                self.keyword("opt", t::OPT, Some(S::Line))
                    || self.keyword("option", t::OPTION, Some(S::Line))
                    || self.keyword("over", t::OVER, None)
                    || self.keyword("off", t::OFF, None)
            }
            112 => {
                self.keyword("participant", t::PARTICIPANT, Some(S::Id))
                    || self.keyword("par", t::PAR, Some(S::Line))
                    || self.keyword("par_over", t::PAR_OVER, Some(S::Line))
                    || self.menu("properties", t::PROPERTIES)
            }
            114 => {
                self.keyword("rect", t::RECT, Some(S::Line))
                    || self.keyword("right of", t::RIGHT_OF, None)
            }
            115 => self.keyword("sequencediagram", t::SD, None),
            116 => self.title(),
            _ => false,
        }
    }

    fn initial(&mut self) {
        let p = self.p;
        let ch = self.at(p);
        if ch == 10 {
            let mut q = p + 1;
            while self.at(q) == 10 {
                q += 1;
            }
            self.emit(t::NEWLINE, p, q);
            return;
        }
        if is_space(ch) {
            self.p = self.skip_ws(p + 1);
            return;
        }
        if ch == 35 || (ch == 37 && self.at(p + 1) != 123) {
            self.p = self.eol(p);
            return;
        }
        if (is_digit(ch) || ch == 46) && self.number() {
            return;
        }
        if ch < 128 && (97..=122).contains(&(ch | 32)) && self.keywords(ch) {
            return;
        }
        let starts = |s: &str| {
            self.src
                .get(p..)
                .is_some_and(|rest| rest.starts_with(&js::utf16(s)))
        };
        if ch == 44 {
            self.emit(t::COMMA, p, p + 1);
        } else if ch == 59 {
            self.emit(t::NEWLINE, p, p + 1);
        } else if self.has(p, c::ACTOR_START) {
            self.actor();
        } else if ch == 45 {
            self.dash();
        } else if ch == 60 && starts("<<->>") {
            self.emit(t::BIDIRECTIONAL_SOLID_ARROW, p, p + 5);
        } else if ch == 60 && starts("<<-->>") {
            self.emit(t::BIDIRECTIONAL_DOTTED_ARROW, p, p + 6);
        } else if (ch == 47 || ch == 92) && self.reverse(ch) {
        } else if ch == 58 {
            let e = self.line_run(p + 1);
            self.emit(t::TXT, p, e);
        } else if ch == 43 {
            self.emit(t::PLUS, p, p + 1);
        } else if ch == 40 && self.at(p + 1) == 41 {
            self.emit(t::CENTRAL, p, p + 2);
        } else {
            self.emit(t::INVALID, p, p + 1);
        }
    }

    /// 参加者の名前。別名、設定、文の終わりまで。
    fn id(&mut self) -> bool {
        if self.skip_same_line() {
            return true;
        }
        let p = self.p;
        let ch = self.at(p);
        if ch == 35 {
            self.p = self.eol(p);
            return true;
        }
        if ch == 64 && self.at(p + 1) == 123 {
            self.begin(t::CONFIG_START, 2, S::Config);
            return true;
        }
        let mut a = p;
        while a < self.n && self.has(a, c::ID) {
            a += 1;
        }
        if a > p {
            let w = self.skip_ws(a);
            if self.at(w) == 64 && self.at(w + 1) == 123 {
                self.emit(t::ACTOR, p, a);
                return true;
            }
            if w > a && self.ci(w, "as") && is_space(self.at(w + 2)) {
                self.emit(t::ACTOR, p, a);
                self.push(S::Alias);
                return true;
            }
        }
        let mut b = a;
        while b < self.n && self.has(b, c::ID_WIDE) {
            b += 1;
        }
        if b > p {
            let end = self.at(b);
            // ほかの区切りの前では名前の後にコメントがなければならないので、名前は最後のコメントで終わる。
            let mut cut = b;
            if b < self.n && end != 10 && end != 59 {
                cut -= 1;
                while cut > p && self.src[cut] != 35 {
                    cut -= 1;
                }
            }
            if cut > p {
                let text = js::trim(&js::from16(&self.src[p..cut])).to_string();
                self.emit_text(t::ACTOR, p, cut, text);
                self.pop();
                return true;
            }
        }
        if self.at(b) == 60 {
            let e = self.eol(p);
            self.emit(t::INVALID, p, e);
            self.pop();
            return true;
        }
        if ch != 10 {
            let e = self.eol(p);
            let text = js::trim(&js::from16(&self.src[p..e])).to_string();
            self.emit_text(t::INVALID, p, e, text);
            self.pop();
            return true;
        }
        false
    }

    fn step(&mut self) -> bool {
        match self.state {
            S::Initial => {
                self.initial();
                true
            }
            S::Id => self.id(),
            S::Alias => {
                if self.skip_same_line() {
                    return true;
                }
                if self.at(self.p) == 35 {
                    self.p = self.eol(self.p);
                    return true;
                }
                self.pop();
                self.pop();
                if self.word("as") {
                    self.begin(t::AS, 2, S::Line);
                } else {
                    let p = self.p;
                    self.emit(t::NEWLINE, p, p);
                }
                true
            }
            S::Line => {
                if self.skip_same_line() {
                    return true;
                }
                if self.at(self.p) == 35 {
                    self.p = self.eol(self.p);
                    return true;
                }
                let p = self.p;
                let e = self.line_run(p);
                self.emit(t::REST_OF_LINE, p, e);
                self.pop();
                true
            }
            S::Config => {
                let p = self.p;
                if self.at(p) != 125 {
                    let q = self.index_of(125, p);
                    self.emit(t::CONFIG_CONTENT, p, q);
                } else {
                    let w = self.skip_ws(p + 1);
                    let alias = w > p + 1 && self.ci(w, "as") && is_space(self.at(w + 2));
                    self.emit(t::CONFIG_END, p, p + 1);
                    self.pop();
                    if alias {
                        self.push(S::Alias);
                    } else {
                        self.pop();
                    }
                }
                true
            }
            S::AccTitle | S::AccDescr => {
                let p = self.p;
                let e = self.eol(p);
                self.emit(
                    if self.state == S::AccTitle {
                        t::ACC_TITLE_VALUE
                    } else {
                        t::ACC_DESCR_VALUE
                    },
                    p,
                    e,
                );
                self.pop();
                true
            }
            S::AccDescrMultiline => {
                let p = self.p;
                if self.at(p) == 125 {
                    self.p += 1;
                    self.pop();
                } else {
                    let q = self.index_of(125, p);
                    self.emit(t::ACC_DESCR_MULTILINE_VALUE, p, q);
                }
                true
            }
        }
    }
}

pub fn tokenize(src: &[u16]) -> Tokens {
    let mut classes = [0u8; 128];
    for (ch, slot) in classes.iter_mut().enumerate() {
        *slot = class(ch as u32);
    }
    let n = src.len();
    let mut lx = Lexer {
        src,
        n,
        out: Tokens {
            types: Vec::new(),
            texts: Vec::new(),
            starts: Vec::new(),
        },
        stack: Vec::new(),
        state: S::Initial,
        p: 0,
        next_nl: None,
        classes,
    };
    while lx.p < n {
        if !lx.step() {
            let p = lx.p;
            lx.out.types.push(t::ERROR);
            lx.out.texts.push(String::new());
            lx.out.starts.push(p);
            return lx.out;
        }
    }

    // Mermaid の字句解析は、終わりを知らせる前に、空の残りにもう一度規則を試す。
    match lx.state {
        S::Initial | S::Alias => lx.emit(t::NEWLINE, n, n),
        S::Line => lx.emit(t::REST_OF_LINE, n, n),
        S::AccTitle => lx.emit(t::ACC_TITLE_VALUE, n, n),
        S::AccDescr => lx.emit(t::ACC_DESCR_VALUE, n, n),
        S::AccDescrMultiline => lx.emit(t::ACC_DESCR_MULTILINE_VALUE, n, n),
        _ => {}
    }
    lx.out.types.push(t::END);
    lx.out.texts.push(String::new());
    lx.out.starts.push(n);
    lx.out
}
