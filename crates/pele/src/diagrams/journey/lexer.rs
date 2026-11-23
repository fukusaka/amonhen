//! journey の字句（pele の `src/diagrams/journey/lexer.ts`）。

use crate::diagrams::common::scan::{Tokens, accessibility, keyword_at, skip_trivia};
use crate::js;
use crate::util::chars::{NONE, is_space, is_word};

pub mod t {
    pub const END: u8 = 0;
    pub const EOF: u8 = 1;
    pub const INVALID: u8 = 2;
    pub const NEWLINE: u8 = 3;
    pub const JOURNEY: u8 = 4;
    pub const TITLE: u8 = 5;
    pub const ACC_TITLE: u8 = 6;
    pub const ACC_TITLE_VALUE: u8 = 7;
    pub const ACC_DESCR: u8 = 8;
    pub const ACC_DESCR_VALUE: u8 = 9;
    pub const ACC_DESCR_MULTILINE_VALUE: u8 = 10;
    pub const SECTION: u8 = 11;
    pub const TASK_NAME: u8 = 12;
    pub const TASK_DATA: u8 = 13;
    pub const COLON: u8 = 14;
}

pub const TOKEN_NAMES: [&str; 15] = [
    "$end",
    "EOF",
    "INVALID",
    "NEWLINE",
    "journey",
    "title",
    "acc_title",
    "acc_title_value",
    "acc_descr",
    "acc_descr_value",
    "acc_descr_multiline_value",
    "section",
    "taskName",
    "taskData",
    ":",
];

fn at(src: &[u16], p: usize) -> u32 {
    src.get(p).map_or(NONE, |&c| c as u32)
}

/// `#`・改行・`;` はどの文字も終える。コロンは作業の名前と節も終える。
fn text_end(src: &[u16], from: usize, colon: bool) -> usize {
    let n = src.len();
    let mut e = from;
    while e < n {
        let c = src[e];
        if c == 35 || c == 10 || c == 59 || (colon && c == 58) {
            break;
        }
        e += 1;
    }
    e
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
        if keyword_at(src, p, "journey") && !is_word(at(src, p + 7)) {
            {
                let e = p + 7;
                emit(&mut out, &mut p, t::JOURNEY, e);
            }
            continue;
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
                e = text_end(src, p + 6, false);
                if e > p + 6 {
                    emit(&mut out, &mut p, t::TITLE, e);
                    continue;
                }
            }
        }
        e = accessibility(src, p, &mut out, t::ACC_TITLE);
        if e != p {
            p = e;
            continue;
        }
        // Mermaid と同じく、中身のない節は次の行を名前にする。
        if keyword_at(src, p, "section") && is_space(at(src, p + 7)) {
            e = text_end(src, p + 8, true);
            if e > p + 8 {
                emit(&mut out, &mut p, t::SECTION, e);
                continue;
            }
        }
        if c == 58 {
            e = text_end(src, p + 1, false);
            if e > p + 1 {
                emit(&mut out, &mut p, t::TASK_DATA, e);
            } else {
                {
                    let e = p + 1;
                    emit(&mut out, &mut p, t::COLON, e);
                }
            }
        } else if c == 59 {
            {
                let e = p + 1;
                emit(&mut out, &mut p, t::INVALID, e);
            }
        } else {
            let e = text_end(src, p + 1, true);
            emit(&mut out, &mut p, t::TASK_NAME, e);
        }
    }

    // Jison は空の入力に規則が合うと字句解析を終えるので、文字の終わりで何にも合わなかった accTitle などの値の
    // 後には EOF の字句がない。
    if out.starts.last().is_none_or(|&s| s < n) {
        emit(&mut out, &mut p, t::EOF, n);
    }
    emit(&mut out, &mut p, t::END, n);
    out
}
