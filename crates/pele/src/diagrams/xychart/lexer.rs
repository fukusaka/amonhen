//! xychart の字句解析（pele の `src/diagrams/xychart/lexer.ts`）。Mermaid の Jison の字句規則を、状態を持つ
//! 手書きの走査に置き換えたもの。位置は UTF-16 の符号単位。

use std::sync::LazyLock;

use super::tokens::*;
use crate::diagrams::common::caseless::word_at;
use crate::js;
use crate::util::chars::{NONE, is_digit, is_letter, is_space, is_word};

pub struct Tokens {
    pub types: Vec<u8>,
    pub texts: Vec<String>,
    pub starts: Vec<usize>,
}

// 状態（TS の `const enum S`）。
const INITIAL: usize = 0;
const AXIS_DATA: usize = 1;
const AXIS_BAND_DATA: usize = 2;
const DATA: usize = 3;
const DATA_INNER: usize = 4;
const STRING: usize = 5;
const S_ACC_TITLE: usize = 6;
const S_ACC_DESCR: usize = 7;
const S_ACC_DESCR_MULTILINE: usize = 8;

/// 状態ごとに、入力の空の残りに対して出す字句。
const AT_END: [u8; 9] = [
    EOF,
    EOF,
    EOF,
    EOF,
    EOF,
    STR,
    ACC_TITLE_VALUE,
    ACC_DESCR_VALUE,
    ACC_DESCR_MULTILINE_VALUE,
];

/// Mermaid の文法は `"` と逆引用符で Markdown の文字列を開くつもりだが、引用のせいで規則がこの文字に合い、
/// 今の状態を出るものになっている。Markdown の文字列は読まれない。
static BROKEN_RULE: LazyLock<String> = LazyLock::new(|| {
    format!(
        "`){}{{ this.pushstate(md_string); }}\n<md_string>(?:(?!`\").)+{}{{ return md_str; }}\n<md_string>(?:`",
        " ".repeat(36),
        " ".repeat(18)
    )
});

fn single(c: u32) -> u8 {
    match c {
        58 => COLON,
        43 => PLUS,
        44 => COMMA,
        61 => EQUALS,
        42 => MULT,
        35 => BRKT,
        95 => UNDERSCORE,
        46 => DOT,
        38 => AMP,
        45 => MINUS,
        59 => SEMI,
        _ => 0,
    }
}

pub fn tokenize(src: &[u16]) -> Tokens {
    let n = src.len();
    let mut types: Vec<u8> = Vec::new();
    let mut texts: Vec<String> = Vec::new();
    let mut starts: Vec<usize> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut state = INITIAL;
    let mut p = 0;

    let at = |q: usize| src.get(q).map_or(NONE, |&c| c as u32);
    let line_end = |q: usize| {
        src.get(q..)
            .and_then(|s| s.iter().position(|&c| c == 10))
            .map_or(n, |k| k + q)
    };
    let index_of = |c: u16, q: usize| {
        src.get(q..)
            .and_then(|s| s.iter().position(|&x| x == c))
            .map_or(n, |k| k + q)
    };
    let run = |mut q: usize| {
        while q < n && is_space(at(q)) {
            q += 1;
        }
        q
    };
    let digits = |mut q: usize| {
        while is_digit(at(q)) {
            q += 1;
        }
        q
    };
    let keyword = |p: usize, word: &str| word_at(src, p, word) && !is_word(at(p + word.len()));

    macro_rules! emit {
        ($t:expr, $s:expr, $e:expr) => {{
            let (s, e) = ($s, $e);
            types.push($t);
            texts.push(js::from16(&src[s..e]));
            starts.push(s);
            p = e;
        }};
    }
    macro_rules! push {
        ($s:expr) => {{
            stack.push(state);
            state = $s;
        }};
    }
    macro_rules! pop {
        () => {{
            if let Some(s) = stack.pop() {
                state = s;
            }
        }};
    }

    while p < n {
        let c = at(p);

        if state == STRING {
            if c == 34 {
                p += 1;
                pop!();
            } else {
                emit!(STR, p, index_of(34, p));
            }
            continue;
        }
        if state == S_ACC_TITLE || state == S_ACC_DESCR {
            emit!(AT_END[state], p, line_end(p));
            pop!();
            continue;
        }
        if state == S_ACC_DESCR_MULTILINE {
            if c == 125 {
                p += 1;
                pop!();
            } else {
                emit!(ACC_DESCR_MULTILINE_VALUE, p, index_of(125, p));
            }
            continue;
        }

        let next = at(p + 1);
        if c == 37 && next == 37 {
            p = line_end(p + 2);
            continue;
        }
        if c == 10 || c == 13 {
            // 軸や系列のすぐ後の改行だけがそれを終える。空白の後の改行は空白と一緒に飛ばし、状態は次の行に続く。
            if (state == AXIS_DATA || state == DATA) && (c == 10 || next == 10) {
                emit!(NEWLINE, p, if c == 10 { p + 1 } else { p + 2 });
                pop!();
                continue;
            }
            let mut e = p + 1;
            while matches!(at(e), 10 | 13) {
                e += 1;
            }
            emit!(NEWLINE, p, e);
            continue;
        }

        match c | 32 {
            116 if keyword(p, "title") => {
                emit!(TITLE, p, p + 5);
                continue;
            }
            97 if word_at(src, p, "acc") => {
                let descr = word_at(src, p + 3, "descr");
                if descr || word_at(src, p + 3, "title") {
                    let k = run(p + 8);
                    let d = at(k);
                    if d == 58 {
                        emit!(if descr { ACC_DESCR } else { ACC_TITLE }, p, run(k + 1));
                        push!(if descr { S_ACC_DESCR } else { S_ACC_TITLE });
                        continue;
                    }
                    if d == 123 && descr {
                        p = run(k + 1);
                        push!(S_ACC_DESCR_MULTILINE);
                        continue;
                    }
                }
            }
            120 => {
                if keyword(p, "xychart-beta") {
                    emit!(XYCHART, p, p + 12);
                    continue;
                }
                if keyword(p, "xychart") {
                    emit!(XYCHART, p, p + 7);
                    continue;
                }
                if keyword(p, "x-axis") {
                    emit!(X_AXIS, p, p + 6);
                    push!(AXIS_DATA);
                    continue;
                }
            }
            121 if keyword(p, "y-axis") => {
                emit!(Y_AXIS, p, p + 6);
                push!(AXIS_DATA);
                continue;
            }
            118 if word_at(src, p, "vertical") => {
                emit!(CHART_ORIENTATION, p, p + 8);
                continue;
            }
            104 if word_at(src, p, "horizontal") => {
                emit!(CHART_ORIENTATION, p, p + 10);
                continue;
            }
            108 if keyword(p, "line") => {
                emit!(LINE, p, p + 4);
                push!(DATA);
                continue;
            }
            98 if keyword(p, "bar") => {
                emit!(BAR, p, p + 3);
                push!(DATA);
                continue;
            }
            _ => {}
        }

        if c == 91 {
            if state == AXIS_DATA {
                push!(AXIS_BAND_DATA);
            } else if state == DATA {
                push!(DATA_INNER);
            }
            emit!(SQUARE_BRACES_START, p, p + 1);
            continue;
        }
        if c == 93 {
            if state == DATA_INNER || state == AXIS_BAND_DATA {
                pop!();
            }
            emit!(SQUARE_BRACES_END, p, p + 1);
            continue;
        }
        if state == AXIS_DATA && c == 45 && next == 45 && at(p + 2) == 62 {
            emit!(ARROW_DELIMITER, p, p + 3);
            continue;
        }
        if state == AXIS_DATA || state == DATA_INNER {
            let q = if c == 43 || c == 45 { p + 1 } else { p };
            let d = at(q);
            let mut e = None;
            if is_digit(d) {
                let mut k = digits(q + 1);
                if at(k) == 46 && is_digit(at(k + 1)) {
                    k = digits(k + 2);
                }
                e = Some(k);
            } else if d == 46 && is_digit(at(q + 1)) {
                e = Some(digits(q + 2));
            }
            if let Some(e) = e {
                emit!(NUMBER_WITH_DECIMAL, p, e);
                continue;
            }
        }
        if c == 96 && word_at(src, p, &BROKEN_RULE) {
            p += BROKEN_RULE.len();
            pop!();
            continue;
        }
        if c == 34 {
            p += 1;
            push!(STRING);
            continue;
        }
        if is_letter(c) {
            let mut e = p + 1;
            while is_letter(at(e)) {
                e += 1;
            }
            emit!(ALPHA, p, e);
            continue;
        }
        if is_digit(c) {
            emit!(NUM, p, digits(p + 1));
            continue;
        }
        if is_space(c) {
            p = run(p + 1);
            continue;
        }
        let s = if c < 128 { single(c) } else { 0 };
        if s != 0 {
            emit!(s, p, p + 1);
            continue;
        }
        break;
    }

    if p < n {
        types.push(ERROR);
        texts.push(String::new());
        starts.push(p);
        return Tokens {
            types,
            texts,
            starts,
        };
    }
    // Jison は、今の状態の規則を空の残りに対してもう一度試す。
    types.extend([AT_END[state], END]);
    texts.extend([String::new(), String::new()]);
    starts.extend([n, n]);
    Tokens {
        types,
        texts,
        starts,
    }
}
