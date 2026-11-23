//! gantt の字句解析（pele の `src/diagrams/gantt/lexer.ts`）。Mermaid の jison の字句の規則を手で写したもの。
//! 粘着の正規表現は手で書いた照合に置き換えた。位置は UTF-16 の符号単位。

use super::tokens as t;
use crate::diagrams::common::caseless::word_at;
use crate::util::chars::{NONE, is_space, is_word};

pub struct Tokens {
    pub types: Vec<u8>,
    pub starts: Vec<usize>,
    pub ends: Vec<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum S {
    Initial,
    AccTitle,
    AccDescr,
    AccDescrMultiline,
    Href,
    Callbackname,
    Callbackargs,
    Click,
}

fn c_at(src: &[u16], i: usize) -> u32 {
    src.get(i).map_or(NONE, |&c| c as u32)
}

fn spaces(src: &[u16], mut i: usize) -> usize {
    while is_space(c_at(src, i)) {
        i += 1;
    }
    i
}

/// `\b` の後ろ側: 次が `\w` でない（終わりも）。
fn boundary(src: &[u16], i: usize) -> bool {
    !is_word(c_at(src, i))
}

/// `<word>\s*<c>\s*`
fn word_sep(src: &[u16], p: usize, word: &str, c: u8) -> Option<usize> {
    if !word_at(src, p, word) {
        return None;
    }
    let i = spaces(src, p + word.len());
    if c_at(src, i) != c as u32 {
        return None;
    }
    Some(spaces(src, i + 1))
}

/// `<word>\s+`（`then` が続くもの）。
fn word_spaces(src: &[u16], p: usize, word: &str) -> Option<usize> {
    if !word_at(src, p, word) {
        return None;
    }
    let i = spaces(src, p + word.len());
    (i > p + word.len()).then_some(i)
}

/// `<word>\b`
fn word_bound(src: &[u16], p: usize, word: &str) -> Option<usize> {
    let e = p + word.len();
    (word_at(src, p, word) && boundary(src, e)).then_some(e)
}

/// `<word>\s[^<stop>]+`
fn word_rest(src: &[u16], p: usize, word: &str, stop: &[u8]) -> Option<usize> {
    if !word_at(src, p, word) || !is_space(c_at(src, p + word.len())) {
        return None;
    }
    let from = p + word.len() + 1;
    let mut e = from;
    while e < src.len() && !stop.iter().any(|&s| src[e] == s as u16) {
        e += 1;
    }
    (e > from).then_some(e)
}

/// `title[^\S\r\n][^\n]+`
fn title_rest(src: &[u16], p: usize) -> Option<usize> {
    let c = c_at(src, p + 5);
    if !word_at(src, p, "title") || !is_space(c) || c == 13 || c == 10 {
        return None;
    }
    let from = p + 6;
    let mut e = from;
    while e < src.len() && src[e] != 10 {
        e += 1;
    }
    (e > from).then_some(e)
}

/// `title[^\S\r\n]*(?=[\r\n]|$)`
fn bare_title(src: &[u16], p: usize) -> Option<usize> {
    if !word_at(src, p, "title") {
        return None;
    }
    let mut e = p + 5;
    while e < src.len() && is_space(src[e] as u32) && src[e] != 13 && src[e] != 10 {
        e += 1;
    }
    (e >= src.len() || src[e] == 13 || src[e] == 10).then_some(e)
}

const DAYS: [&str; 7] = [
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];

/// `weekday\s+(monday|…|sunday)\b` の日の番号と終わり。
fn weekday(src: &[u16], p: usize) -> Option<(usize, usize)> {
    let i = word_spaces(src, p, "weekday")?;
    DAYS.iter()
        .enumerate()
        .find_map(|(k, day)| word_bound(src, i, day).map(|e| (k, e)))
}

/// `weekend\s+(?:(friday)|saturday)\b`。金曜なら true。
fn weekend(src: &[u16], p: usize) -> Option<(bool, usize)> {
    let i = word_spaces(src, p, "weekend")?;
    if let Some(e) = word_bound(src, i, "friday") {
        return Some((true, e));
    }
    word_bound(src, i, "saturday").map(|e| (false, e))
}

/// `\d\d\d\d-\d\d-\d\d\b`
fn date_at(src: &[u16], p: usize) -> bool {
    let d = |i: usize| (48..=57).contains(&c_at(src, p + i));
    (0..4).all(d)
        && c_at(src, p + 4) == 45
        && d(5)
        && d(6)
        && c_at(src, p + 7) == 45
        && d(8)
        && d(9)
        && boundary(src, p + 10)
}

const KEYWORD_STOP: &[u8] = b"#\n;";

type Rule = (fn(&[u16], usize) -> Option<usize>, u8);

/// 決まった語で始まる文（語の頭の文字ごとに、文法の規則の順）。
fn keyword_rules(src: &[u16], p: usize, c: u32) -> Option<(u8, usize)> {
    let rules: &[Rule] = match c {
        97 => &[
            (
                |s, p| word_rest(s, p, "axisformat", KEYWORD_STOP),
                t::axisFormat,
            ),
            (
                |s, p| word_rest(s, p, "accdescription", KEYWORD_STOP),
                t::accDescription,
            ),
        ],
        103 => &[(|s, p| word_bound(s, p, "gantt"), t::gantt)],
        100 => &[(
            |s, p| word_rest(s, p, "dateformat", KEYWORD_STOP),
            t::dateFormat,
        )],
        105 => &[
            (
                |s, p| word_bound(s, p, "inclusiveenddates"),
                t::inclusiveEndDates,
            ),
            (
                |s, p| word_rest(s, p, "includes", KEYWORD_STOP),
                t::includes,
            ),
        ],
        116 => &[
            (|s, p| word_bound(s, p, "topaxis"), t::topAxis),
            (
                |s, p| word_rest(s, p, "tickinterval", KEYWORD_STOP),
                t::tickInterval,
            ),
            (
                |s, p| word_rest(s, p, "todaymarker", b"\n;"),
                t::todayMarker,
            ),
            (title_rest, t::title),
        ],
        101 => &[(
            |s, p| word_rest(s, p, "excludes", KEYWORD_STOP),
            t::excludes,
        )],
        115 => &[(|s, p| word_rest(s, p, "section", b"\n"), t::section)],
        _ => &[],
    };
    rules
        .iter()
        .find_map(|(rule, type_)| rule(src, p).map(|e| (*type_, e)))
}

pub fn tokenize(src: &[u16]) -> Tokens {
    let n = src.len();
    let mut out = Tokens {
        types: Vec::new(),
        starts: Vec::new(),
        ends: Vec::new(),
    };
    let mut state = S::Initial;
    let mut p = 0usize;

    fn emit(out: &mut Tokens, type_: u8, s: usize, e: usize) -> usize {
        out.types.push(type_);
        out.starts.push(s);
        out.ends.push(e);
        e
    }
    let until = |ch: u16, from: usize| -> usize {
        if from >= n {
            return n;
        }
        src[from..]
            .iter()
            .position(|&c| c == ch)
            .map_or(n, |k| k + from)
    };

    loop {
        if p >= n {
            let type_ = match state {
                S::AccTitle => t::acc_title_value,
                S::AccDescr => t::acc_descr_value,
                S::AccDescrMultiline => t::acc_descr_multiline_value,
                S::Href => t::href,
                S::Callbackname => t::callbackname,
                S::Callbackargs => t::callbackargs,
                S::Click => t::click,
                S::Initial => t::EOF,
            };
            emit(&mut out, type_, n, n);
            break;
        }

        let c = src[p] as u32;

        match state {
            S::Initial => {
                let d = c_at(src, p + 1);
                if c == 37 {
                    if d != 37 {
                        let mut e = until(10, p + 1);
                        while c_at(src, e) == 10 {
                            e += 1;
                        }
                        p = e;
                    } else if c_at(src, p + 2) == 123 {
                        // 文法は定めていない字句の状態に入るので、後には何も続けられない。
                        p = emit(&mut out, t::open_directive, p, p + 3);
                        emit(&mut out, t::ERROR, p, p);
                        return out;
                    } else {
                        p = until(10, p + 2);
                    }
                } else if c == 10 {
                    let mut e = p + 1;
                    while c_at(src, e) == 10 {
                        e += 1;
                    }
                    p = emit(&mut out, t::NL, p, e);
                } else if is_space(c) {
                    p += 1;
                    while p < n && is_space(src[p] as u32) {
                        p += 1;
                    }
                } else if let Some((type_, e, enter)) = keyword(src, p, c) {
                    match type_ {
                        Some(type_) => p = emit(&mut out, type_, p, e),
                        None => p = e,
                    }
                    if let Some(s) = enter {
                        state = s;
                    }
                } else if (48..=57).contains(&c) && date_at(src, p) {
                    p = emit(&mut out, t::date, p, p + 10);
                } else if c != 58 {
                    let mut e = p + 1;
                    while e < n && src[e] != 58 && src[e] != 10 {
                        e += 1;
                    }
                    p = emit(&mut out, t::taskTxt, p, e);
                } else {
                    let mut e = p + 1;
                    while e < n && !matches!(src[e], 35 | 10 | 59) {
                        e += 1;
                    }
                    let type_ = if e > p + 1 { t::taskData } else { t::COLON };
                    p = emit(&mut out, type_, p, e);
                }
            }
            S::AccTitle | S::AccDescr => {
                let type_ = if state == S::AccTitle {
                    t::acc_title_value
                } else {
                    t::acc_descr_value
                };
                state = S::Initial;
                let e = until(10, p);
                p = emit(&mut out, type_, p, e);
            }
            S::AccDescrMultiline => {
                if c == 125 {
                    state = S::Initial;
                    p += 1;
                } else {
                    let e = until(125, p);
                    p = emit(&mut out, t::acc_descr_multiline_value, p, e);
                }
            }
            S::Href => {
                if c == 34 {
                    state = S::Initial;
                    p += 1;
                } else {
                    let e = until(34, p);
                    p = emit(&mut out, t::href, p, e);
                }
            }
            S::Callbackname => {
                if c == 40 {
                    // `\(\s*\)`
                    let i = spaces(src, p + 1);
                    if c_at(src, i) == 41 {
                        state = S::Initial;
                        p = i + 1;
                    } else {
                        state = S::Callbackargs;
                        p += 1;
                    }
                } else {
                    let e = until(40, p);
                    p = emit(&mut out, t::callbackname, p, e);
                }
            }
            S::Callbackargs => {
                if c == 41 {
                    state = S::Initial;
                    p += 1;
                } else {
                    let e = until(41, p);
                    p = emit(&mut out, t::callbackargs, p, e);
                }
            }
            S::Click => {
                if is_space(c) {
                    state = S::Initial;
                    p += 1;
                } else {
                    let mut e = p + 1;
                    while e < n && !is_space(src[e] as u32) {
                        e += 1;
                    }
                    p = emit(&mut out, t::click, p, e);
                }
            }
        }
    }

    out.types.push(t::END);
    out.starts.push(n);
    out.ends.push(n);
    out
}

/// 決まった語で始まるもの: (出す字句、なければ読み飛ばすだけ, 終わり, 入る状態)。
#[allow(clippy::type_complexity)]
fn keyword(src: &[u16], p: usize, c: u32) -> Option<(Option<u8>, usize, Option<S>)> {
    let lower = c | 32;
    match lower {
        97 => {
            if let Some(e) = word_sep(src, p, "acctitle", b':') {
                return Some((Some(t::acc_title), e, Some(S::AccTitle)));
            }
            if let Some(e) = word_sep(src, p, "accdescr", b':') {
                return Some((Some(t::acc_descr), e, Some(S::AccDescr)));
            }
            if let Some(e) = word_sep(src, p, "accdescr", b'{') {
                return Some((None, e, Some(S::AccDescrMultiline)));
            }
        }
        104 => {
            if let Some(i) = word_spaces(src, p, "href")
                && c_at(src, i) == 34
            {
                return Some((None, i + 1, Some(S::Href)));
            }
        }
        99 => {
            if let Some(e) = word_spaces(src, p, "call") {
                return Some((None, e, Some(S::Callbackname)));
            }
            if let Some(e) = word_spaces(src, p, "click") {
                return Some((None, e, Some(S::Click)));
            }
        }
        116 => {
            if let Some(e) = bare_title(src, p) {
                return Some((None, e, None));
            }
        }
        119 => {
            if let Some((k, e)) = weekday(src, p) {
                return Some((Some(t::weekday_monday + k as u8), e, None));
            }
            if let Some((friday, e)) = weekend(src, p) {
                let type_ = if friday {
                    t::weekend_friday
                } else {
                    t::weekend_saturday
                };
                return Some((Some(type_), e, None));
            }
        }
        _ => {}
    }
    keyword_rules(src, p, lower).map(|(type_, e)| (Some(type_), e, None))
}
