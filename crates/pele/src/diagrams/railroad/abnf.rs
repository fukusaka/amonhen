//! `railroad-abnf-beta`: RFC 5234 の ABNF。各規則はセミコロンで閉じる（pele の `src/diagrams/railroad/abnf.ts`）。

use std::sync::LazyLock;

use super::common::{
    Grammar, LETTERS, Nodes, Part, Parts, acc_descr, acc_title, at, hidden_tokens, literal,
    opening, parse_grammar, strip_ends, title,
};
use crate::diagrams::common::tokens::{Lexicon, Pattern, Reader, TokenType};
use crate::errors::Result;
use crate::js;
use crate::util::chars::{is_digit, is_hex};

const EQUALS: i32 = 1;
const SEMICOLON: i32 = 2;
const SLASH: i32 = 3;
const GROUP: i32 = 4;
const OPTION: i32 = 6;
const TITLE: i32 = 8;
const NAME: i32 = 11;
const STRING: i32 = 12;
const NUMBER: i32 = 13;
const REPEAT: i32 = 14;
const EXACT: i32 = 15;

const DIGITS: &str = "0123456789";

fn is_alpha(c: u32) -> bool {
    (65..=90).contains(&c) || (97..=122).contains(&c)
}

/// `[A-Za-z0-9-]`
fn name_char(c: u32) -> bool {
    is_alpha(c) || is_digit(c) || c == 45
}

/// `[A-Za-z][A-Za-z0-9-]*`
fn rule_name(src: &[u16], p: usize) -> Option<usize> {
    if !is_alpha(at(src, p)) {
        return None;
    }
    let mut e = p + 1;
    while name_char(at(src, e)) {
        e += 1;
    }
    Some(e)
}

/// `;[^\n\r]*`
fn comment(src: &[u16], p: usize) -> Option<usize> {
    if at(src, p) != 59 {
        return None;
    }
    let mut e = p + 1;
    while e < src.len() && src[e] != 10 && src[e] != 13 {
        e += 1;
    }
    Some(e)
}

/// `"[^"]*"`
fn string(src: &[u16], p: usize) -> Option<usize> {
    if at(src, p) != 34 {
        return None;
    }
    (p + 1..src.len()).find(|&k| src[k] == 34).map(|k| k + 1)
}

fn hex_run(src: &[u16], mut p: usize) -> usize {
    while is_hex(at(src, p)) {
        p += 1;
    }
    p
}

/// `%[xXdDbB][0-9A-Fa-f]+(?:-[0-9A-Fa-f]+|\.[0-9A-Fa-f]+)*`
fn numval(src: &[u16], p: usize) -> Option<usize> {
    if at(src, p) != 37 || !matches!(at(src, p + 1), 120 | 88 | 100 | 68 | 98 | 66) {
        return None;
    }
    let mut e = hex_run(src, p + 2);
    if e == p + 2 {
        return None;
    }
    while matches!(at(src, e), 45 | 46) {
        let f = hex_run(src, e + 1);
        if f == e + 1 {
            break;
        }
        e = f;
    }
    Some(e)
}

fn digits(src: &[u16], mut p: usize) -> usize {
    while is_digit(at(src, p)) {
        p += 1;
    }
    p
}

/// `[0-9]*\*[0-9]*`
fn repeat(src: &[u16], p: usize) -> Option<usize> {
    let e = digits(src, p);
    (at(src, e) == 42).then(|| digits(src, e + 1))
}

pub static ABNF_TOKENS: LazyLock<Lexicon> = LazyLock::new(|| {
    let mut types: Vec<TokenType> = vec![
        opening("railroad-abnf-beta", "ABNF_RULENAME"),
        literal("=", &[]),
        // セミコロンは行の終わりでだけ規則を閉じる。後に何かあれば、コメントを始める。
        literal(";", &["ABNF_COMMENT"]),
        literal("/", &[]),
        literal("(", &[]),
        literal(")", &[]),
        literal("[", &[]),
        literal("]", &[]),
        title(name_char),
        acc_title(),
        acc_descr(),
        TokenType::new("ABNF_RULENAME", Pattern::regex(rule_name)).first(LETTERS),
        TokenType::new("ABNF_STRING", Pattern::regex(string)).first("\""),
        TokenType::new("ABNF_NUMVAL", Pattern::regex(numval)).first("%"),
        TokenType::new("ABNF_REPEAT", Pattern::regex(repeat)).first(&format!("{DIGITS}*")),
        TokenType::new(
            "ABNF_EXACT_REPEAT",
            Pattern::regex(|src, p| {
                let e = digits(src, p);
                (e > p).then_some(e)
            }),
        )
        .first(DIGITS),
    ];
    types.extend(hidden_tokens("ABNF"));
    types.push(
        TokenType::new("ABNF_COMMENT", Pattern::regex(comment))
            .hidden()
            .first(";"),
    );
    Lexicon::new(types)
});

const ALTERNATION: u8 = 0;
const CONCATENATION: u8 = 1;
const ELEMENT: u8 = 2;
const STRING_LITERAL: u8 = 3;
const NUM_VAL: u8 = 4;
const RULE_NAME: u8 = 5;
const GROUP_PART: u8 = 6;
const OPTIONAL_GROUP: u8 = 7;

/// 開いた括弧 1 つ、または積みの底の規則そのもの。
struct Frame {
    alternation: usize,
    concatenation: usize,
    /// 括弧の節、その閉じの字句、その前に書いた繰り返し。
    node: Option<usize>,
    close: i32,
    repeat: Option<String>,
}

fn frame(
    parts: &mut Parts,
    node: Option<usize>,
    alternation: usize,
    close: i32,
    repeat: Option<String>,
) -> Frame {
    let concatenation = parts.add(CONCATENATION, None, Vec::new());
    parts.0[alternation].kids.push(concatenation);
    Frame {
        alternation,
        concatenation,
        node,
        close,
        repeat,
    }
}

fn alternation(r: &mut Reader, parts: &mut Parts) -> Result<usize> {
    let mut stack: Vec<Frame> = Vec::new();
    let root = parts.add(ALTERNATION, None, Vec::new());
    let mut top = frame(parts, None, root, -2, None);
    loop {
        let mut repeat = if r.kind() == REPEAT || r.kind() == EXACT {
            Some(r.take())
        } else {
            None
        };
        let kind = r.kind();
        let mut primary = match kind {
            STRING => parts.add(STRING_LITERAL, Some(strip_ends(&r.take())), Vec::new()),
            NUMBER => parts.add(NUM_VAL, Some(r.take()), Vec::new()),
            NAME => parts.add(RULE_NAME, Some(r.take()), Vec::new()),
            GROUP | OPTION => {
                r.i += 1;
                let element = parts.add(ALTERNATION, None, Vec::new());
                let tag = if kind == GROUP {
                    GROUP_PART
                } else {
                    OPTIONAL_GROUP
                };
                let node = parts.add(tag, None, vec![element]);
                let next = frame(parts, Some(node), element, kind + 1, repeat);
                stack.push(std::mem::replace(&mut top, next));
                continue;
            }
            _ => return Err(r.fail("a string, a number, a name, or an opening bracket")),
        };
        loop {
            let element = parts.add(ELEMENT, repeat.take(), vec![primary]);
            parts.0[top.concatenation].kids.push(element);
            let k = r.kind();
            if k == top.close {
                r.i += 1;
                primary = top.node.unwrap();
                let done = stack.pop().unwrap();
                repeat = std::mem::replace(&mut top, done).repeat;
                continue;
            }
            if k == SLASH {
                r.i += 1;
                top.concatenation = parts.add(CONCATENATION, None, Vec::new());
                parts.0[top.alternation].kids.push(top.concatenation);
            } else if !matches!(k, STRING | NUMBER | NAME | GROUP | OPTION | REPEAT | EXACT) {
                if stack.is_empty() {
                    return Ok(top.alternation);
                }
                let name = &r.lexicon.types[top.close as usize].name;
                return Err(r.fail(&format!("token of type '{name}'")));
            }
            break;
        }
    }
}

fn rule(r: &mut Reader, parts: &mut Parts) -> Result<(String, usize)> {
    let name = r.expect(NAME)?;
    r.expect(EQUALS)?;
    let definition = alternation(r, parts)?;
    r.expect(SEMICOLON)?;
    Ok((name, definition))
}

pub fn parse_abnf(src: &str) -> Result<Grammar> {
    parse_grammar(src, &ABNF_TOKENS, TITLE, strip_ends, rule)
}

pub fn node(part: &Part, kids: Vec<usize>, n: &mut Nodes) -> usize {
    let text = || part.text.as_deref().unwrap_or("");
    match part.tag {
        ALTERNATION => n.choice(kids),
        CONCATENATION => n.sequence(kids),
        ELEMENT => {
            let repeat = match part.text.as_deref() {
                Some(r) if !r.is_empty() => r,
                _ => return kids[0],
            };
            let star = repeat.find('*');
            let exact = star.is_none();
            let min = if exact || star.unwrap() > 0 {
                js::parse_int(repeat, 10)
            } else {
                0.0
            };
            let max = match star {
                None => min,
                Some(s) if s < repeat.len() - 1 => js::parse_int(&repeat[s + 1..], 10),
                Some(_) => f64::INFINITY,
            };
            if min == 0.0 && max == 1.0 {
                n.optional(kids[0])
            } else {
                n.repetition(kids[0], min, max)
            }
        }
        STRING_LITERAL | NUM_VAL => n.terminal(text()),
        RULE_NAME => n.nonterminal(text()),
        GROUP_PART => kids[0],
        _ => n.optional(kids[0]),
    }
}
