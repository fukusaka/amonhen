//! `railroad-beta`: Mermaid 自身の書き方。ノードごとに 1 つの呼び出し（pele の `src/diagrams/railroad/ir.ts`）。

use std::sync::LazyLock;

use super::common::{
    Grammar, Nodes, Part, Parts, acc_descr, acc_title, block_comment, hidden_tokens, identifier,
    literal, opening, parse_grammar, quoted, title, unescape, word_or_dash,
};
use crate::diagrams::common::tokens::{Lexicon, Reader, TokenType};
use crate::errors::Result;

const NONTERMINAL: i32 = 1;
const ZERO_OR_MORE: i32 = 2;
const ONE_OR_MORE: i32 = 3;
const SEQUENCE: i32 = 4;
const OPTIONAL: i32 = 5;
const TERMINAL: i32 = 6;
const SPECIAL: i32 = 7;
const CHOICE: i32 = 8;
const EQUALS: i32 = 9;
const SEMICOLON: i32 = 10;
const OPEN: i32 = 11;
const COMMA: i32 = 12;
const CLOSE: i32 = 13;
const TITLE: i32 = 14;
const ID: i32 = 17;
const STRING: i32 = 18;

fn word(name: &str) -> TokenType {
    literal(name, &["RR_ID"])
}

pub static IR_TOKENS: LazyLock<Lexicon> = LazyLock::new(|| {
    let mut types: Vec<TokenType> = vec![
        opening("railroad-beta", "RR_ID"),
        word("nonterminal"),
        word("zeroOrMore"),
        word("oneOrMore"),
        word("sequence"),
        word("optional"),
        word("terminal"),
        word("special"),
        word("choice"),
        literal("=", &[]),
        literal(";", &[]),
        literal("(", &[]),
        literal(",", &[]),
        literal(")", &[]),
        title(word_or_dash),
        acc_title(),
        acc_descr(),
        identifier("RR_ID"),
        quoted("RR_STRING"),
    ];
    types.extend(hidden_tokens("RR"));
    types.push(block_comment("RR_BLOCK_COMMENT", "/*", "*/"));
    Lexicon::new(types)
});

// 構文の木の節の種類（字句の種類と同じ番号）。
const T_TERMINAL: u8 = TERMINAL as u8;
const T_NONTERMINAL: u8 = NONTERMINAL as u8;
const T_SPECIAL: u8 = SPECIAL as u8;
const T_SEQUENCE: u8 = SEQUENCE as u8;
const T_CHOICE: u8 = CHOICE as u8;
const T_OPTIONAL: u8 = OPTIONAL as u8;
const T_ONE_OR_MORE: u8 = ONE_OR_MORE as u8;

fn argument(r: &mut Reader) -> Result<String> {
    r.i += 1;
    r.expect(OPEN)?;
    let value = unescape(&r.expect(STRING)?);
    r.expect(CLOSE)?;
    Ok(value)
}

fn expression(r: &mut Reader, parts: &mut Parts) -> Result<usize> {
    let mut open: Vec<usize> = Vec::new();
    loop {
        let kind = r.kind();
        let mut value = match kind {
            TERMINAL | NONTERMINAL | SPECIAL => {
                let text = argument(r)?;
                parts.add(kind as u8, Some(text), Vec::new())
            }
            SEQUENCE | CHOICE | OPTIONAL | ONE_OR_MORE | ZERO_OR_MORE => {
                let container = parts.add(kind as u8, None, Vec::new());
                r.i += 1;
                r.expect(OPEN)?;
                open.push(container);
                continue;
            }
            _ => return Err(r.fail("an expression")),
        };
        loop {
            let Some(&parent) = open.last() else {
                return Ok(value);
            };
            let tag = parts.0[parent].tag;
            if tag == T_SEQUENCE || tag == T_CHOICE {
                parts.0[parent].kids.push(value);
                if r.accept(COMMA) {
                    break;
                }
            } else {
                // 1 つの要素を包むもの。
                parts.0[parent].kids = vec![value];
            }
            r.expect(CLOSE)?;
            open.pop();
            value = parent;
        }
    }
}

fn rule(r: &mut Reader, parts: &mut Parts) -> Result<(String, usize)> {
    let name = r.expect(ID)?;
    r.expect(EQUALS)?;
    let definition = expression(r, parts)?;
    r.expect(SEMICOLON)?;
    Ok((name, definition))
}

pub fn parse_ir(src: &str) -> Result<Grammar> {
    parse_grammar(src, &IR_TOKENS, TITLE, unescape, rule)
}

pub fn node(part: &Part, kids: Vec<usize>, n: &mut Nodes) -> usize {
    let text = || part.text.as_deref().unwrap_or("");
    match part.tag {
        T_TERMINAL => n.terminal(text()),
        T_NONTERMINAL => n.nonterminal(text()),
        T_SPECIAL => n.special(text()),
        T_SEQUENCE => n.sequence(kids),
        T_CHOICE => n.choice(kids),
        T_OPTIONAL => n.optional(kids[0]),
        _ => n.repetition(
            kids[0],
            if part.tag == T_ONE_OR_MORE { 1.0 } else { 0.0 },
            f64::INFINITY,
        ),
    }
}
