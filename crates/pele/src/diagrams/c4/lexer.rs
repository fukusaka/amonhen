//! C4 の字句解析（pele の `src/diagrams/c4/lexer.ts`）。Jison が c4Diagram.jison から作る字句解析に合わせる:
//! 規則の順、開始条件、それぞれで入力が尽きたときに返すもの。JS と同じく UTF-16 の符号単位で読む。字句の文字は
//! `src[starts[i]..ends[i]]` で、読み手が求めるまで写さない。

use super::tokens as t;
use crate::util::chars::{NONE, is_line_end, is_space, is_word};

pub struct Tokens {
    pub types: Vec<u8>,
    pub starts: Vec<usize>,
    pub ends: Vec<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum S {
    Initial,
    Struct,
    Attribute,
    String,
    StringKv,
    StringKvKey,
    StringKvValue,
    Acc,
    AccDescrMultiline,
}

fn keyword(word: &str) -> Option<u8> {
    Some(match word {
        "C4Context" => t::C4_CONTEXT,
        "C4Container" => t::C4_CONTAINER,
        "C4Component" => t::C4_COMPONENT,
        "C4Dynamic" => t::C4_DYNAMIC,
        "C4Deployment" => t::C4_DEPLOYMENT,
        "Person" => t::PERSON,
        "Person_Ext" => t::PERSON_EXT,
        "System" => t::SYSTEM,
        "SystemDb" => t::SYSTEM_DB,
        "SystemQueue" => t::SYSTEM_QUEUE,
        "System_Ext" => t::SYSTEM_EXT,
        "SystemDb_Ext" => t::SYSTEM_EXT_DB,
        "SystemQueue_Ext" => t::SYSTEM_EXT_QUEUE,
        "Container" => t::CONTAINER,
        "ContainerDb" => t::CONTAINER_DB,
        "ContainerQueue" => t::CONTAINER_QUEUE,
        "Container_Ext" => t::CONTAINER_EXT,
        "ContainerDb_Ext" => t::CONTAINER_EXT_DB,
        "ContainerQueue_Ext" => t::CONTAINER_EXT_QUEUE,
        "Component" => t::COMPONENT,
        "ComponentDb" => t::COMPONENT_DB,
        "ComponentQueue" => t::COMPONENT_QUEUE,
        "Component_Ext" => t::COMPONENT_EXT,
        "ComponentDb_Ext" => t::COMPONENT_EXT_DB,
        "ComponentQueue_Ext" => t::COMPONENT_EXT_QUEUE,
        "Enterprise_Boundary" => t::ENTERPRISE_BOUNDARY,
        "System_Boundary" => t::SYSTEM_BOUNDARY,
        "Boundary" => t::BOUNDARY,
        "Container_Boundary" => t::CONTAINER_BOUNDARY,
        "Deployment_Node" => t::NODE,
        "Node" => t::NODE,
        "Node_L" => t::NODE_L,
        "Node_R" => t::NODE_R,
        "Rel" => t::REL,
        "BiRel" => t::BIREL,
        "Rel_Up" => t::REL_U,
        "Rel_U" => t::REL_U,
        "Rel_Down" => t::REL_D,
        "Rel_D" => t::REL_D,
        "Rel_Left" => t::REL_L,
        "Rel_L" => t::REL_L,
        "Rel_Right" => t::REL_R,
        "Rel_R" => t::REL_R,
        "Rel_Back" => t::REL_B,
        "RelIndex" => t::REL_INDEX,
        "UpdateElementStyle" => t::UPDATE_EL_STYLE,
        "UpdateRelStyle" => t::UPDATE_REL_STYLE,
        "UpdateLayoutConfig" => t::UPDATE_LAYOUT_CONFIG,
        _ => return None,
    })
}

const DIRECTIONS: [&str; 4] = ["TB", "BT", "RL", "LR"];

struct Lexer<'a> {
    src: &'a [u16],
    n: usize,
    types: Vec<u8>,
    starts: Vec<usize>,
    ends: Vec<usize>,
    p: usize,
}

impl Lexer<'_> {
    fn c(&self, i: usize) -> u32 {
        self.src.get(i).map_or(NONE, |&c| c as u32)
    }

    fn emit(&mut self, type_: u8, s: usize, e: usize) {
        self.types.push(type_);
        self.starts.push(s);
        self.ends.push(e);
        self.p = e;
    }

    fn skip_ws(&self, mut q: usize) -> usize {
        while q < self.n && is_space(self.c(q)) {
            q += 1;
        }
        q
    }

    fn starts_with(&self, word: &str, at: usize) -> bool {
        let w = word.as_bytes();
        self.n >= at + w.len()
            && w.iter()
                .enumerate()
                .all(|(k, &b)| self.src[at + k] == b as u16)
    }

    fn index_of(&self, word: &str, from: usize) -> Option<usize> {
        let w = word.as_bytes();
        if self.n < w.len() {
            return None;
        }
        (from..=self.n - w.len()).find(|&k| self.starts_with(word, k))
    }

    fn up_to(&self, ch: u16, from: usize) -> usize {
        if from >= self.n {
            return self.n;
        }
        self.src[from..]
            .iter()
            .position(|&x| x == ch)
            .map_or(self.n, |k| k + from)
    }

    /// `word\s[^#\n;]+`
    fn statement(&mut self, word: &str, type_: u8) -> bool {
        let mut q = self.p + word.len();
        if !self.starts_with(word, self.p) || !is_space(self.c(q)) {
            return false;
        }
        q += 1;
        let from = q;
        while q < self.n {
            let c = self.c(q);
            if c == 35 || c == 10 || c == 59 {
                break;
            }
            q += 1;
        }
        if q == from {
            return false;
        }
        self.emit(type_, self.p, q);
        true
    }

    /// `/.*direction\s+XX[^\n]*/y` を p から: 合えば終わり。`.*` は欲張りなので、行の中の最後の合う所を取る。
    fn direction_at(&self, which: &str) -> Option<usize> {
        let p = self.p;
        let mut dot_end = p;
        while dot_end < self.n && !is_line_end(self.c(dot_end)) {
            dot_end += 1;
        }
        let w: Vec<u16> = which.encode_utf16().collect();
        let mut k = dot_end;
        loop {
            if self.starts_with("direction", k) {
                let q = k + 9;
                let r = self.skip_ws(q);
                if r > q && r + 2 <= self.n && self.src[r..r + 2] == w[..] {
                    return Some(self.up_to(b'\n' as u16, r + 2));
                }
            }
            if k == p {
                return None;
            }
            k -= 1;
        }
    }

    /// `/title[^\S\r\n]*(?=[\r\n]|$)\s*/y`
    fn bare_title(&self) -> Option<usize> {
        if !self.starts_with("title", self.p) {
            return None;
        }
        let mut q = self.p + 5;
        while q < self.n {
            let c = self.c(q);
            if !is_space(c) || c == 13 || c == 10 {
                break;
            }
            q += 1;
        }
        if q < self.n && self.c(q) != 13 && self.c(q) != 10 {
            return None;
        }
        Some(self.skip_ws(q))
    }
}

pub fn tokenize(src: &[u16]) -> Tokens {
    let n = src.len();
    let mut lx = Lexer {
        src,
        n,
        types: Vec::new(),
        starts: Vec::new(),
        ends: Vec::new(),
        p: 0,
    };
    let mut state = S::Initial;
    let mut acc_value = t::acc_title_value;
    let mut done = false;
    // 次の `direction` の位置。-2 は探す前。
    let mut dir_next: isize = -2;
    let mut line_end: isize = -1;
    let mut dir_failed = 0usize;

    loop {
        // Jison は入力の終わりを、知らせる 1 つ前の呼び出しで知るので、何にも合わない規則が最後に 1 度だけ回る。
        if done {
            lx.emit(t::END, n, n);
            break;
        }
        let p = lx.p;
        if p >= n {
            done = true;
        }
        let c = lx.c(p);

        match state {
            S::Initial => {
                if p >= n {
                    lx.emit(t::EOF, n, n);
                    continue;
                }
                if dir_next != -1 && p >= dir_failed {
                    if dir_next < p as isize {
                        dir_next = lx.index_of("direction", p).map_or(-1, |k| k as isize);
                    }
                    if dir_next != -1 {
                        if line_end < p as isize {
                            let mut q = p;
                            while q < n && !is_line_end(lx.c(q)) {
                                q += 1;
                            }
                            line_end = q as isize;
                        }
                        if dir_next < line_end {
                            let mut matched = false;
                            for (i, which) in DIRECTIONS.iter().enumerate() {
                                if let Some(e) = lx.direction_at(which) {
                                    lx.emit(t::direction_tb + i as u8, p, e);
                                    matched = true;
                                    break;
                                }
                            }
                            if matched {
                                continue;
                            }
                            // この行の残りのどこにも、向きの文は始まらない。
                            dir_failed = line_end as usize;
                        }
                    }
                }
                if c == 116 {
                    if let Some(e) = lx.bare_title() {
                        lx.p = e;
                        continue;
                    }
                    if lx.statement("title", t::title) {
                        continue;
                    }
                }
                if c == 97 {
                    if lx.statement("accDescription", t::accDescription) {
                        continue;
                    }
                    let title = lx.starts_with("accTitle", p);
                    if title || lx.starts_with("accDescr", p) {
                        let q = lx.skip_ws(p + 8);
                        let d = lx.c(q);
                        if d == 58 {
                            let e = lx.skip_ws(q + 1);
                            lx.emit(if title { t::acc_title } else { t::acc_descr }, p, e);
                            acc_value = if title {
                                t::acc_title_value
                            } else {
                                t::acc_descr_value
                            };
                            state = S::Acc;
                            continue;
                        }
                        if d == 123 && !title {
                            lx.p = lx.skip_ws(q + 1);
                            state = S::AccDescrMultiline;
                            continue;
                        }
                    }
                }
                if c == 37 && lx.c(p + 1) == 37 {
                    // コメントはその後の改行も取るので、NEWLINE は続かない。
                    let mut q = lx.up_to(b'\n' as u16, p + 2);
                    while q < n && (lx.c(q) == 10 || lx.c(q) == 13) {
                        q += 1;
                    }
                    lx.p = q;
                    continue;
                }
                if is_space(c) {
                    let mut q = p;
                    let mut last: Option<usize> = None;
                    while q < n {
                        let d = lx.c(q);
                        if !is_space(d) {
                            break;
                        }
                        if d == 10 {
                            last = Some(q);
                        }
                        q += 1;
                    }
                    match last {
                        Some(l) => lx.emit(t::NEWLINE, p, l + 1),
                        None => lx.p = q,
                    }
                    continue;
                }
                if c == 123 {
                    lx.emit(t::LBRACE, p, p + 1);
                    continue;
                }
                if c == 125 {
                    lx.emit(t::RBRACE, p, p + 1);
                    continue;
                }
                if is_word(c) {
                    let mut q = p + 1;
                    while q < n && is_word(lx.c(q)) {
                        q += 1;
                    }
                    let type_ = if q - p <= 19 {
                        let word: String = src[p..q].iter().map(|&u| u as u8 as char).collect();
                        keyword(&word)
                    } else {
                        None
                    };
                    if let Some(type_) = type_ {
                        if type_ > t::C4_DEPLOYMENT {
                            state = S::Struct;
                        }
                        lx.emit(type_, p, q);
                        continue;
                    }
                }
                lx.emit(t::ERROR, p, p);
                break;
            }
            S::Struct => {
                if p >= n {
                    lx.emit(t::EOF_IN_STRUCT, n, n);
                } else if c == 40 {
                    let mut q = p + 1;
                    while lx.c(q) == 32 {
                        q += 1;
                    }
                    if lx.c(q) == 44 {
                        lx.emit(t::ATTRIBUTE_EMPTY, p, q + 1);
                    } else {
                        lx.p += 1;
                    }
                    state = S::Attribute;
                } else if c == 41 {
                    lx.p += 1;
                    state = S::Initial;
                } else {
                    lx.emit(t::ERROR, p, p);
                    break;
                }
            }
            S::Attribute => {
                if p >= n {
                    continue;
                }
                if c == 41 {
                    lx.p += 1;
                    state = S::Initial;
                } else if c == 44 {
                    if lx.c(p + 1) == 44 {
                        lx.emit(t::ATTRIBUTE_EMPTY, p, p + 2);
                    } else {
                        lx.p += 1;
                    }
                } else {
                    let mut q = p;
                    while lx.c(q) == 32 {
                        q += 1;
                    }
                    let d = lx.c(q);
                    if d == 34 {
                        if lx.c(q + 1) == 34 {
                            lx.emit(t::ATTRIBUTE_EMPTY, p, q + 2);
                        } else {
                            lx.p = q + 1;
                            state = S::String;
                        }
                    } else if d == 36 {
                        lx.p = q + 1;
                        state = S::StringKv;
                    } else {
                        // 引用のない値は、閉じ括弧も改行も越えて次のコンマまで。
                        let e = lx.up_to(b',' as u16, p);
                        lx.emit(t::STR, p, e);
                    }
                }
            }
            S::String => {
                if c == 34 {
                    lx.p += 1;
                    state = S::Attribute;
                } else {
                    let e = lx.up_to(b'"' as u16, p);
                    lx.emit(t::STR, p, e);
                }
            }
            S::StringKv => {
                let e = lx.up_to(b'=' as u16, p);
                lx.emit(t::STR_KEY, p, e);
                state = S::StringKvKey;
            }
            S::StringKvKey => {
                if p >= n {
                    continue;
                }
                let mut q = p + 1;
                if c == 61 {
                    while lx.c(q) == 32 {
                        q += 1;
                    }
                }
                if c != 61 || lx.c(q) != 34 {
                    lx.emit(t::ERROR, p, p);
                    break;
                }
                lx.p = q + 1;
                state = S::StringKvValue;
            }
            S::StringKvValue => {
                if p >= n {
                    continue;
                }
                if c == 34 {
                    lx.p += 1;
                    state = S::Attribute;
                } else {
                    let e = lx.up_to(b'"' as u16, p);
                    lx.emit(t::STR_VALUE, p, e);
                }
            }
            S::Acc => {
                let e = lx.up_to(b'\n' as u16, p);
                lx.emit(acc_value, p, e);
                state = S::Initial;
            }
            S::AccDescrMultiline => {
                if c == 125 {
                    lx.p += 1;
                    state = S::Initial;
                } else {
                    let e = lx.up_to(b'}' as u16, p);
                    lx.emit(t::acc_descr_multiline_value, p, e);
                }
            }
        }
    }

    Tokens {
        types: lx.types,
        starts: lx.starts,
        ends: lx.ends,
    }
}
