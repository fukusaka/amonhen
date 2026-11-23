//! state の字句解析（pele の `src/diagrams/state/lexer.ts`）。JS と同じく UTF-16 の符号単位で読む。

use super::tokens as t;
use crate::util::chars::{NONE, is_line_end, is_space, is_word};

pub struct Tokens {
    pub types: Vec<u8>,
    pub texts: Vec<String>,
    pub starts: Vec<usize>,
    /// 字句解析が、分からない文字ではなく文法そのものの知らせで止まったとき。
    pub error: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum S {
    Initial,
    Struct,
    State,
    StateString,
    StateId,
    Scale,
    AccTitle,
    AccDescr,
    AccDescrMultiline,
    Classdef,
    ClassdefId,
    Class,
    ClassStyle,
    Style,
    StyledefStyles,
    Note,
    NoteId,
    NoteText,
    FloatingNote,
    FloatingNoteId,
}

/// 小文字の ASCII の語に、大文字小文字を問わず合うか（文法の /i と同じ）。
fn word_at(src: &[u16], w: &str, q: usize) -> bool {
    for (i, a) in w.bytes().enumerate() {
        let a = a as u32;
        let c = src.get(q + i).map_or(NONE, |&c| c as u32);
        if c != a && !((97..=122).contains(&a) && c == a - 32) {
            return false;
        }
    }
    true
}

/// `/direction/gi`
fn is_direction(src: &[u16], k: usize) -> bool {
    word_at(src, "direction", k)
}

/// `/<<(?:fork|join|choice)>>|\[\[(?:fork|join|choice)\]\]/gi`
fn is_stereotype(src: &[u16], k: usize) -> bool {
    for (open, close) in [("<<", ">>"), ("[[", "]]")] {
        if !word_at(src, open, k) {
            continue;
        }
        for name in ["fork", "join", "choice"] {
            if word_at(src, name, k + 2) && word_at(src, close, k + 2 + name.len()) {
                return true;
            }
        }
    }
    false
}

fn is_line_end_at(src: &[u16], k: usize) -> bool {
    is_line_end(src[k] as u32)
}

fn is_percent(src: &[u16], k: usize) -> bool {
    src[k] == 37 && src.get(k + 1) == Some(&37)
}

fn is_brace(src: &[u16], k: usize) -> bool {
    src[k] == 123
}

/// 位置から後ろで型に合う最初の所を探し、答えを覚えて、字句ごとに訊いても全体で 1 度の走査で済ませる。
struct Finder {
    test: fn(&[u16], usize) -> bool,
    from: usize,
    /// まだ探していなければ None。
    next: Option<Option<usize>>,
}

impl Finder {
    fn new(test: fn(&[u16], usize) -> bool) -> Self {
        Finder {
            test,
            from: 0,
            next: None,
        }
    }

    fn find(&mut self, src: &[u16], q: usize) -> Option<usize> {
        let stale = match self.next {
            None => true,
            Some(next) => q < self.from || next.is_some_and(|n| q > n),
        };
        if stale {
            self.next = Some((q..src.len()).find(|&k| (self.test)(src, k)));
            self.from = q;
        }
        self.next.unwrap()
    }
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
    error: Option<String>,
    next_direction: Finder,
    next_stereotype: Finder,
    next_line_end: Finder,
    next_percent: Finder,
    next_brace: Finder,
    // 字句を読んでいる行の、種類ごとの最後の `direction <dir>` と、その語の終わり。
    dir_from: isize,
    dir_to: isize,
    dir_at: [isize; 4],
    dir_end: [usize; 4],
    // <<fork>>・<<join>>・<<choice>> と、その [[ ]] の書き方も同じく。
    stereo_from: isize,
    stereo_to: isize,
    stereo_at: [isize; 6],
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

    fn emit_text(&mut self, type_: u8, text: String, s: usize, e: usize) {
        self.types.push(type_);
        self.texts.push(text);
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

    fn word(&self, w: &str, q: usize) -> bool {
        word_at(self.src, w, q)
    }

    fn ws_end(&self, q: usize) -> usize {
        let mut j = q;
        while j < self.n && is_space(self.c(j)) {
            j += 1;
        }
        j
    }

    fn word_end(&self, q: usize) -> usize {
        let mut j = q;
        while j < self.n && is_word(self.c(j)) {
            j += 1;
        }
        j
    }

    fn index_of(&self, ch: u16, q: usize) -> Option<usize> {
        if q >= self.n {
            return None;
        }
        self.src[q..].iter().position(|&x| x == ch).map(|k| k + q)
    }

    fn eol(&self, q: usize) -> usize {
        self.index_of(10, q).unwrap_or(self.n)
    }

    fn line_end(&mut self, q: usize) -> usize {
        self.next_line_end.find(self.src, q).unwrap_or(self.n)
    }

    fn trimmed(&self, s: usize, e: usize) -> String {
        crate::js::trim(&String::from_utf16_lossy(&self.src[s..e])).to_string()
    }

    /// `.*direction\s+TB[^\n]*` と 3 つの兄弟の規則: q からその行の終わりまでで、文法の順で最初の種類。
    fn direction(&mut self, q: usize) -> isize {
        let qi = q as isize;
        if qi < self.dir_from || qi > self.dir_to {
            let first = self.next_direction.find(self.src, q);
            let end = match first {
                None => self.n,
                Some(_) => self.line_end(q),
            };
            self.dir_from = qi;
            self.dir_to = end as isize;
            self.dir_at = [-1; 4];
            let mut d = first;
            while let Some(k) = d.filter(|&k| k < end) {
                let w = self.ws_end(k + 9);
                if w != k + 9 {
                    let kind = ["tb", "bt", "rl", "lr"]
                        .iter()
                        .position(|code| self.word(code, w));
                    if let Some(kind) = kind {
                        self.dir_at[kind] = k as isize;
                        self.dir_end[kind] = w + 2;
                    }
                }
                d = self.next_direction.find(self.src, k + 1);
            }
        }
        (0..4)
            .find(|&kind| self.dir_at[kind] >= qi)
            .map_or(-1, |k| k as isize)
    }

    fn stereotype(&mut self, q: usize) -> isize {
        let qi = q as isize;
        if qi < self.stereo_from || qi > self.stereo_to {
            let first = self.next_stereotype.find(self.src, q);
            let end = match first {
                None => self.n,
                Some(_) => self.line_end(q),
            };
            self.stereo_from = qi;
            self.stereo_to = end as isize;
            self.stereo_at = [-1; 6];
            let mut d = first;
            while let Some(k) = d.filter(|&k| k < end) {
                let letter = self.c(k + 2) | 32;
                let slot = (if self.c(k) == 60 { 0 } else { 3 })
                    + if letter == 102 {
                        0
                    } else if letter == 106 {
                        1
                    } else {
                        2
                    };
                self.stereo_at[slot] = k as isize;
                d = self.next_stereotype.find(self.src, k + 1);
            }
        }
        (0..6)
            .find(|&kind| self.stereo_at[kind] >= qi)
            .map_or(-1, |k| k as isize)
    }

    /// 文法の processId(): id は行の中の `%%` で終わり、それで始まる文字は捨てる。
    fn emit_id(&mut self, s: usize, e: usize) -> bool {
        let at = self.next_percent.find(self.src, s);
        match at {
            Some(at) if at + 2 <= e => {
                if at == s {
                    self.p = e;
                    return false;
                }
                self.emit(t::ID, s, at);
                true
            }
            _ => {
                self.emit(t::ID, s, e);
                true
            }
        }
    }

    fn finish(mut self, type_: u8) -> Tokens {
        self.types.push(type_);
        self.texts.push(String::new());
        self.starts.push(self.p);
        Tokens {
            types: self.types,
            texts: self.texts,
            starts: self.starts,
            error: self.error,
        }
    }

    fn comment(&self, c: u32, p: usize) -> bool {
        c == 35 || (c == 37 && self.c(p + 1) == 37 && self.c(p + 2) != 123)
    }

    /// 1 つの字句。字句解析を終えるなら、その終わりの字句の種類。
    fn step(&mut self) -> Option<u8> {
        let p = self.p;
        let c = self.c(p);
        let n = self.n;
        match self.state {
            S::Initial | S::Struct => {
                let root = self.state == S::Initial;
                let lc = c | 32;
                if root {
                    if lc == 99 && self.word("click", p) && !is_word(self.c(p + 5)) {
                        self.emit(t::CLICK, p, p + 5);
                        return None;
                    }
                    if lc == 104 && self.word("href", p) && !is_word(self.c(p + 4)) {
                        self.emit(t::HREF, p, p + 4);
                        return None;
                    }
                    if c == 34
                        && let Some(close) = self.index_of(34, p + 1)
                    {
                        self.emit(t::STRING, p, close + 1);
                        return None;
                    }
                    if lc == 100 && self.word("default", p) && !is_word(self.c(p + 7)) {
                        self.emit(t::DEFAULT, p, p + 7);
                        return None;
                    }
                    let kind = self.direction(p);
                    if kind != -1 {
                        let e = self.eol(self.dir_end[kind as usize]);
                        self.emit(t::direction_tb + kind as u8, p, e);
                        return None;
                    }
                    if c == 10 {
                        let mut j = p + 1;
                        while self.c(j) == 10 {
                            j += 1;
                        }
                        self.emit(t::NL, p, j);
                        return None;
                    }
                    if is_space(c) {
                        self.p = self.ws_end(p);
                        return None;
                    }
                } else if c != 10 && is_space(c) {
                    let mut j = p + 1;
                    while j < n && self.c(j) != 10 && is_space(self.c(j)) {
                        j += 1;
                    }
                    self.p = j;
                    return None;
                }
                if self.comment(c, p) {
                    self.p = self.eol(p);
                    return None;
                }
                if lc == 115 {
                    if root && self.word("scale", p) {
                        let e = self.ws_end(p + 5);
                        if e > p + 5 {
                            self.emit(t::scale, p, e);
                            self.push(S::Scale);
                            return None;
                        }
                    }
                    if self.word("style", p) {
                        let e = self.ws_end(p + 5);
                        if e > p + 5 {
                            self.emit(t::style, p, e);
                            self.push(S::Style);
                            return None;
                        }
                    }
                    if self.word("state", p) {
                        let e = self.ws_end(p + 5);
                        if e > p + 5 {
                            self.p = e;
                            self.push(S::State);
                            return None;
                        }
                        if root && self.word("diagram", p + 5) {
                            let from = if self.word("-v2", p + 12) {
                                p + 15
                            } else {
                                p + 12
                            };
                            let e = self.ws_end(from);
                            if e > from {
                                self.emit(t::SD, p, e);
                                return None;
                            }
                        }
                    }
                } else if lc == 99 {
                    if self.word("class", p) {
                        let def = self.word("def", p + 5);
                        let from = if def { p + 8 } else { p + 5 };
                        let e = self.ws_end(from);
                        if e > from {
                            self.emit(if def { t::classDef } else { t::class }, p, e);
                            self.push(if def { S::Classdef } else { S::Class });
                            return None;
                        }
                    }
                } else if lc == 97 && root && (self.word("acctitle", p) || self.word("accdescr", p))
                {
                    let title = (self.c(p + 3) | 32) == 116;
                    let q = self.ws_end(p + 8);
                    let d = self.c(q);
                    if d == 58 {
                        let e = self.ws_end(q + 1);
                        self.emit(if title { t::acc_title } else { t::acc_descr }, p, e);
                        self.push(if title { S::AccTitle } else { S::AccDescr });
                        return None;
                    }
                    if d == 123 && !title {
                        self.p = self.ws_end(q + 1);
                        self.push(S::AccDescrMultiline);
                        return None;
                    }
                }
                if root {
                    if c == 123 {
                        self.emit(t::STRUCT_START, p, p + 1);
                        self.push(S::Struct);
                        return None;
                    }
                } else {
                    let kind = self.direction(p);
                    if kind != -1 {
                        let e = self.eol(self.dir_end[kind as usize]);
                        self.emit(t::direction_tb + kind as u8, p, e);
                        return None;
                    }
                    if c == 125 {
                        self.emit(t::STRUCT_STOP, p, p + 1);
                        self.pop();
                        return None;
                    }
                    if c == 10 {
                        self.p += 1;
                        return None;
                    }
                }
                if lc == 110 && self.word("note", p) {
                    let e = self.ws_end(p + 4);
                    if e > p + 4 {
                        self.emit(t::note, p, e);
                        self.push(S::Note);
                        return None;
                    }
                }
                if root
                    && lc == 104
                    && self.word("hide empty description", p)
                    && !is_word(self.c(p + 22))
                {
                    self.emit(t::HIDE_EMPTY, p, p + 22);
                    return None;
                }
                if c == 91 && self.c(p + 1) == 42 && self.c(p + 2) == 93 {
                    self.emit(t::EDGE_STATE, p, p + 3);
                    return None;
                }
                let mut j = p;
                while j < n {
                    let d = self.c(j);
                    if d == 58 || d == 45 || d == 123 || is_space(d) {
                        break;
                    }
                    j += 1;
                }
                if j > p {
                    self.emit_id(p, j);
                    return None;
                }
                if c == 58 {
                    j = p + 1;
                    while j < n {
                        let d = self.c(j);
                        if d == 10 || d == 59 {
                            break;
                        }
                        if d == 58 {
                            let after = self.c(j + 1);
                            if j + 1 >= n || after == 58 || after == 10 || after == 59 {
                                break;
                            }
                            j += 1;
                        }
                        j += 1;
                    }
                    if j > p + 1 {
                        let text = self.trimmed(p, j);
                        self.emit_text(t::DESCR, text, p, j);
                        return None;
                    }
                    if self.c(p + 1) == 58 && self.c(p + 2) == 58 {
                        self.emit(t::STYLE_SEPARATOR, p, p + 3);
                        return None;
                    }
                } else if c == 45 && self.c(p + 1) == 45 {
                    if self.c(p + 2) == 62 {
                        self.emit(t::ARROW, p, p + 3);
                        return None;
                    }
                    if !root {
                        self.emit(t::CONCURRENT, p, p + 2);
                        return None;
                    }
                }
                if !root {
                    return Some(t::ERROR);
                }
                self.emit(t::INVALID, p, p + 1);
                None
            }

            S::State => {
                if c != 10 && is_space(c) {
                    let mut j = p + 1;
                    while j < n && self.c(j) != 10 && is_space(self.c(j)) {
                        j += 1;
                    }
                    self.p = j;
                    return None;
                }
                if self.comment(c, p) {
                    self.p = self.eol(p);
                    return None;
                }
                let kind = self.stereotype(p);
                if kind != -1 {
                    let at = self.stereo_at[kind as usize] as usize;
                    let choice = kind == 2 || kind == 5;
                    let type_ = if choice {
                        t::CHOICE
                    } else if kind % 3 == 0 {
                        t::FORK
                    } else {
                        t::JOIN
                    };
                    let text = self.trimmed(p, at);
                    self.emit_text(type_, text, p, at + if choice { 10 } else { 8 });
                    self.pop();
                    return None;
                }
                if c == 34 {
                    self.p += 1;
                    self.push(S::StateString);
                    return None;
                }
                let q = if c == 10 { self.ws_end(p) } else { p };
                if self.word("as", q) {
                    let e = self.ws_end(q + 2);
                    if e > q + 2 {
                        self.emit(t::AS, p, e);
                        self.push(S::StateId);
                        return None;
                    }
                }
                if c == 10 {
                    self.p += 1;
                    self.pop();
                    return None;
                }
                if c == 123 {
                    self.emit(t::STRUCT_START, p, p + 1);
                    self.pop();
                    self.push(S::Struct);
                    return None;
                }
                if is_word(c) {
                    let first = self.word_end(p);
                    let second = self.ws_end(first);
                    if second > first && is_word(self.c(second)) {
                        let brace = self.next_brace.find(self.src, second + 1);
                        if let Some(brace) = brace
                            && brace < self.line_end(second)
                        {
                            let found = self.trimmed(p, brace + 1);
                            self.error = Some(format!(
                                "Error: State name must be a single word. Found: \"{found}\""
                            ));
                            return Some(t::ERROR);
                        }
                    }
                }
                let mut j = p + 1;
                while j < n {
                    let d = self.c(j);
                    if d == 123 || is_space(d) {
                        break;
                    }
                    j += 1;
                }
                self.emit(t::COMPOSIT_STATE, p, j);
                None
            }

            S::StateString => {
                if c == 34 {
                    self.p += 1;
                    self.pop();
                    return None;
                }
                let close = self.index_of(34, p).unwrap_or(n);
                self.emit(t::STATE_DESCR, p, close);
                None
            }

            S::StateId => {
                let mut j = p;
                while j < n {
                    let d = self.c(j);
                    if d == 10 || d == 123 {
                        break;
                    }
                    j += 1;
                }
                if self.emit_id(p, j) {
                    self.pop();
                }
                None
            }

            S::Scale => {
                if (48..=57).contains(&c) {
                    let mut j = p + 1;
                    while j < n && (48..=57).contains(&self.c(j)) {
                        j += 1;
                    }
                    self.emit(t::WIDTH, p, j);
                    return None;
                }
                let q = self.ws_end(p);
                if q > p && self.word("width", q) && !is_word(self.c(q + 5)) {
                    self.p = q + 5;
                    self.pop();
                    return None;
                }
                Some(t::ERROR)
            }

            S::AccTitle | S::AccDescr => {
                let e = self.eol(p);
                let type_ = if self.state == S::AccTitle {
                    t::acc_title_value
                } else {
                    t::acc_descr_value
                };
                self.emit(type_, p, e);
                self.pop();
                None
            }

            S::AccDescrMultiline => {
                if c == 125 {
                    self.p += 1;
                    self.pop();
                    return None;
                }
                let close = self.index_of(125, p).unwrap_or(n);
                self.emit(t::acc_descr_multiline_value, p, close);
                None
            }

            S::Classdef => {
                let is_default = self.word("default", p) && self.ws_end(p + 7) > p + 7;
                let j = if is_default { p + 7 } else { self.word_end(p) };
                let e = self.ws_end(j);
                if j == p || e == j {
                    return Some(t::ERROR);
                }
                self.emit(
                    if is_default {
                        t::DEFAULT_CLASSDEF_ID
                    } else {
                        t::CLASSDEF_ID
                    },
                    p,
                    e,
                );
                self.pop();
                self.push(S::ClassdefId);
                None
            }

            S::ClassdefId => {
                let e = self.eol(p);
                self.emit(t::CLASSDEF_STYLEOPTS, p, e);
                self.pop();
                None
            }

            S::Class => {
                let mut j = self.word_end(p);
                if j == p {
                    return Some(t::ERROR);
                }
                while self.c(j) == 44 {
                    let k = self.ws_end(j + 1);
                    let e = self.word_end(k);
                    if e == k {
                        break;
                    }
                    j = e;
                }
                self.emit(t::CLASSENTITY_IDS, p, j);
                self.pop();
                self.push(S::ClassStyle);
                None
            }

            S::ClassStyle => {
                let e = self.eol(p);
                self.emit(t::STYLECLASS, p, e);
                self.pop();
                None
            }

            S::Style => {
                let mut j = p;
                while j < n && (is_word(self.c(j)) || self.c(j) == 44) {
                    j += 1;
                }
                let e = self.ws_end(j);
                if j == p || e == j {
                    return Some(t::ERROR);
                }
                self.emit(t::STYLE_IDS, p, e);
                self.pop();
                self.push(S::StyledefStyles);
                None
            }

            S::StyledefStyles => {
                let e = self.eol(p);
                self.emit(t::STYLEDEF_STYLEOPTS, p, e);
                self.pop();
                None
            }

            S::Note => {
                if self.word("left of", p) && !is_word(self.c(p + 7)) {
                    self.emit(t::left_of, p, p + 7);
                    self.pop();
                    self.push(S::NoteId);
                    return None;
                }
                if self.word("right of", p) && !is_word(self.c(p + 8)) {
                    self.emit(t::right_of, p, p + 8);
                    self.pop();
                    self.push(S::NoteId);
                    return None;
                }
                if c == 34 {
                    self.p += 1;
                    self.pop();
                    self.push(S::FloatingNote);
                    return None;
                }
                Some(t::ERROR)
            }

            S::NoteId => {
                let q = self.ws_end(p);
                let mut j = q;
                while j < n {
                    let d = self.c(j);
                    if d == 58 || d == 45 || is_space(d) {
                        break;
                    }
                    j += 1;
                }
                if j == q {
                    return Some(t::ERROR);
                }
                if self.emit_id(p, j) {
                    self.pop();
                    self.push(S::NoteText);
                }
                None
            }

            S::NoteText => {
                let q = self.ws_end(p);
                if self.c(q) == 58 {
                    let mut j = q + 1;
                    while j < n {
                        let d = self.c(j);
                        if d == 58 || d == 10 || d == 59 {
                            break;
                        }
                        j += 1;
                    }
                    if j > q + 1 {
                        // Mermaid はここで " :" を待って 2 文字を捨てる。コロンの前に空白がなければ、ノートの最初の
                        // 文字も消える。
                        let text = self.trimmed(p + 2, j);
                        self.emit_text(t::NOTE_TEXT, text, p, j);
                        self.pop();
                        return None;
                    }
                }
                let mut k = self.index_of(10, p);
                let mut e = 0;
                while let Some(kk) = k {
                    e = self.ws_end(kk);
                    if self.word("end note", e) && !is_word(self.c(e + 8)) {
                        break;
                    }
                    k = self.index_of(10, e);
                }
                if k.is_none() {
                    return Some(t::ERROR);
                }
                let text = self.trimmed(p, e);
                self.emit_text(t::NOTE_TEXT, text, p, e + 8);
                self.pop();
                None
            }

            S::FloatingNote => {
                let q = self.ws_end(p);
                if self.word("as", q) {
                    let e = self.ws_end(q + 2);
                    self.emit(t::AS, p, e);
                    self.pop();
                    self.push(S::FloatingNoteId);
                    return None;
                }
                if c == 34 {
                    self.p += 1;
                    return None;
                }
                let close = self.index_of(34, p).unwrap_or(n);
                self.emit(t::NOTE_TEXT, p, close);
                None
            }

            S::FloatingNoteId => {
                let e = self.eol(p);
                if self.emit_id(p, e) {
                    self.pop();
                }
                None
            }
        }
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
        error: None,
        next_direction: Finder::new(is_direction),
        next_stereotype: Finder::new(is_stereotype),
        next_line_end: Finder::new(is_line_end_at),
        next_percent: Finder::new(is_percent),
        next_brace: Finder::new(is_brace),
        dir_from: -1,
        dir_to: -1,
        dir_at: [-1; 4],
        dir_end: [0; 4],
        stereo_from: -1,
        stereo_to: -1,
        stereo_at: [-1; 6],
    };
    loop {
        if lx.p >= lx.n {
            // Jison は今の状態の規則を、空の残りにもう 1 度試す。
            let n = lx.n;
            let type_ = match lx.state {
                S::Initial => Some(t::NL),
                S::AccTitle => Some(t::acc_title_value),
                S::AccDescr => Some(t::acc_descr_value),
                S::AccDescrMultiline => Some(t::acc_descr_multiline_value),
                S::ClassdefId => Some(t::CLASSDEF_STYLEOPTS),
                S::ClassStyle => Some(t::STYLECLASS),
                S::StyledefStyles => Some(t::STYLEDEF_STYLEOPTS),
                S::StateString => Some(t::STATE_DESCR),
                S::FloatingNote => Some(t::NOTE_TEXT),
                S::StateId | S::FloatingNoteId => Some(t::ID),
                _ => None,
            };
            if let Some(ty) = type_ {
                lx.emit(ty, n, n);
            }
            return lx.finish(t::END);
        }
        if let Some(end) = lx.step() {
            return lx.finish(end);
        }
    }
}
