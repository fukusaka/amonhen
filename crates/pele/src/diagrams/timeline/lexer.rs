//! timeline の字句（pele の `src/diagrams/timeline/lexer.ts`）。

use crate::diagrams::common::scan::{Tokens, accessibility, keyword_at, line_end, skip_trivia};
use crate::js;
use crate::util::chars::{NONE, is_space, is_word};

pub mod t {
    pub const END: u8 = 0;
    pub const EOF: u8 = 1;
    pub const INVALID: u8 = 2;
    pub const NEWLINE: u8 = 3;
    pub const TIMELINE: u8 = 4;
    pub const TIMELINE_LR: u8 = 5;
    pub const TIMELINE_TD: u8 = 6;
    pub const TITLE: u8 = 7;
    pub const ACC_TITLE: u8 = 8;
    pub const ACC_TITLE_VALUE: u8 = 9;
    pub const ACC_DESCR: u8 = 10;
    pub const ACC_DESCR_VALUE: u8 = 11;
    pub const ACC_DESCR_MULTILINE_VALUE: u8 = 12;
    pub const SECTION: u8 = 13;
    pub const PERIOD: u8 = 14;
    pub const EVENT: u8 = 15;
}

pub const TOKEN_NAMES: [&str; 16] = [
    "$end",
    "EOF",
    "INVALID",
    "NEWLINE",
    "timeline",
    "timeline_lr",
    "timeline_td",
    "title",
    "acc_title",
    "acc_title_value",
    "acc_descr",
    "acc_descr_value",
    "acc_descr_multiline_value",
    "section",
    "period",
    "event",
];

fn at(src: &[u16], p: usize) -> u32 {
    src.get(p).map_or(NONE, |&c| c as u32)
}

pub fn tokenize(src: &[u16]) -> Tokens {
    let n = src.len();
    let mut out = Tokens {
        types: Vec::new(),
        texts: Vec::new(),
        starts: Vec::new(),
    };
    let mut p = 0;

    let emit = |out: &mut Tokens, p: &mut usize, type_: u8, e: usize| {
        out.types.push(type_);
        out.texts.push(js::from16(&src[*p..e]));
        out.starts.push(*p);
        *p = e;
    };

    loop {
        p = skip_trivia(src, p);
        if p >= n {
            break;
        }
        let c = src[p] as u32;
        let mut e = p + 1;
        if c == 10 {
            while at(src, e) == 10 {
                e += 1;
            }
            emit(&mut out, &mut p, t::NEWLINE, e);
            continue;
        }
        if keyword_at(src, p, "timeline") {
            e = p + 8;
            while at(src, e) == 32 || at(src, e) == 9 {
                e += 1;
            }
            let lr = keyword_at(src, e, "lr");
            if e > p + 8 && (lr || keyword_at(src, e, "td")) && !is_word(at(src, e + 2)) {
                emit(
                    &mut out,
                    &mut p,
                    if lr { t::TIMELINE_LR } else { t::TIMELINE_TD },
                    e + 2,
                );
                continue;
            }
            if !is_word(at(src, p + 8)) {
                let e = p + 8;
                emit(&mut out, &mut p, t::TIMELINE, e);
                continue;
            }
        }
        if keyword_at(src, p, "title") {
            // Mermaid と違い、中身のない題は無視する。
            e = p + 5;
            while at(src, e) != 10 && is_space(at(src, e)) {
                e += 1;
            }
            if e == n || at(src, e) == 10 {
                p = e;
                continue;
            }
            let sep = at(src, p + 5);
            if sep != 13 && is_space(sep) {
                let e = line_end(src, p + 6);
                emit(&mut out, &mut p, t::TITLE, e);
                continue;
            }
        }
        e = accessibility(src, p, &mut out, t::ACC_TITLE);
        if e != p {
            p = e;
            continue;
        }
        // Mermaid と同じく、中身のない節は次の行を名前にする。
        if keyword_at(src, p, "section") && is_space(at(src, p + 7)) {
            e = p + 8;
            while e < n && src[e] != 58 && src[e] != 10 {
                e += 1;
            }
            if e > p + 8 {
                emit(&mut out, &mut p, t::SECTION, e);
                continue;
            }
        }
        if c != 58 {
            e = p + 1;
            while e < n && !matches!(src[e], 35 | 58 | 10) {
                e += 1;
            }
            emit(&mut out, &mut p, t::PERIOD, e);
            continue;
        }
        // 出来事は行の終わりか、空白の続くコロンまで。
        if is_space(at(src, p + 1)) {
            e = p + 2;
            while e < n {
                let k = src[e] as u32;
                if k == 10 || (k == 58 && is_space(at(src, e + 1))) {
                    break;
                }
                e += 1;
            }
            if e > p + 2 {
                emit(&mut out, &mut p, t::EVENT, e);
                continue;
            }
        }
        let e = p + 1;
        emit(&mut out, &mut p, t::INVALID, e);
    }

    // Jison は空の入力に規則が合うと字句解析を終えるので、文字の終わりで何にも合わなかった accTitle などの値の
    // 後には EOF の字句がない。
    if out.starts.last().is_none_or(|&s| s < n) {
        emit(&mut out, &mut p, t::EOF, n);
    }
    emit(&mut out, &mut p, t::END, n);
    out
}
