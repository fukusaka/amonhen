//! `railroad-peg-beta`: 解析表現文法（pele の `src/diagrams/railroad/peg.ts`）。

use std::sync::LazyLock;

use super::common::{
    Grammar, Nodes, Part, Parts, acc_descr, acc_title, at, hidden_tokens, identifier, literal,
    opening, parse_grammar, quoted, title, unescape, word_or_dash,
};
use super::types::RailroadNode;
use crate::diagrams::common::tokens::{Lexicon, Pattern, Reader, TokenType};
use crate::errors::Result;

const ARROW: i32 = 1;
const SEMICOLON: i32 = 2;
const SLASH: i32 = 3;
const AND: i32 = 4;
const NOT: i32 = 5;
const QUESTION: i32 = 6;
const STAR: i32 = 7;
const PLUS: i32 = 8;
const GROUP: i32 = 9;
const GROUP_END: i32 = 10;
const DOT: i32 = 11;
const TITLE: i32 = 12;
const ID: i32 = 15;
const STRING: i32 = 16;

pub static PEG_TOKENS: LazyLock<Lexicon> = LazyLock::new(|| {
    let mut types: Vec<TokenType> = vec![
        opening("railroad-peg-beta", "PEG_ID"),
        literal("<-", &[]),
        literal(";", &[]),
        literal("/", &[]),
        literal("&", &[]),
        literal("!", &[]),
        literal("?", &[]),
        literal("*", &[]),
        literal("+", &[]),
        literal("(", &[]),
        literal(")", &[]),
        literal(".", &[]),
        title(word_or_dash),
        acc_title(),
        acc_descr(),
        identifier("PEG_ID"),
        quoted("PEG_STRING"),
    ];
    types.extend(hidden_tokens("PEG"));
    types.push(
        TokenType::new(
            "PEG_LINE_COMMENT",
            Pattern::regex(|src, p| {
                if at(src, p) != 35 {
                    return None;
                }
                let mut e = p + 1;
                while e < src.len() && src[e] != 10 && src[e] != 13 {
                    e += 1;
                }
                Some(e)
            }),
        )
        .hidden()
        .first("#"),
    );
    Lexicon::new(types)
});

const CHOICE: u8 = 0;
const SEQUENCE: u8 = 1;
const PREFIX: u8 = 2;
const SUFFIX: u8 = 3;
const LITERAL: u8 = 4;
const IDENTIFIER: u8 = 5;
const ANY: u8 = 6;
const GROUP_PART: u8 = 7;

/// 開いたかっこ 1 つ、または積みの底の規則そのもの。
struct Frame {
    choice: usize,
    sequence: usize,
    /// かっこの節と、その前に書いた述語。
    node: Option<usize>,
    operator: Option<String>,
}

fn frame(parts: &mut Parts, node: Option<usize>, choice: usize, operator: Option<String>) -> Frame {
    let sequence = parts.add(SEQUENCE, None, Vec::new());
    parts.0[choice].kids.push(sequence);
    Frame {
        choice,
        sequence,
        node,
        operator,
    }
}

fn definition(r: &mut Reader, parts: &mut Parts) -> Result<usize> {
    let mut stack: Vec<Frame> = Vec::new();
    let root = parts.add(CHOICE, None, Vec::new());
    let mut top = frame(parts, None, root, None);
    loop {
        let mut operator = if r.kind() == AND || r.kind() == NOT {
            Some(r.take())
        } else {
            None
        };
        let kind = r.kind();
        let mut primary = match kind {
            STRING => parts.add(LITERAL, Some(unescape(&r.take())), Vec::new()),
            ID => parts.add(IDENTIFIER, Some(r.take()), Vec::new()),
            DOT => parts.add(ANY, Some(r.take()), Vec::new()),
            GROUP => {
                r.i += 1;
                let element = parts.add(CHOICE, None, Vec::new());
                let node = parts.add(GROUP_PART, None, vec![element]);
                let next = frame(parts, Some(node), element, operator);
                stack.push(std::mem::replace(&mut top, next));
                continue;
            }
            _ => {
                return Err(r.fail("a literal, a name, a dot, or an opening parenthesis"));
            }
        };
        loop {
            let suffix_op = if matches!(r.kind(), QUESTION | STAR | PLUS) {
                Some(r.take())
            } else {
                None
            };
            let suffix = parts.add(SUFFIX, suffix_op, vec![primary]);
            let prefix = parts.add(PREFIX, operator.take(), vec![suffix]);
            parts.0[top.sequence].kids.push(prefix);
            let k = r.kind();
            if k == GROUP_END && !stack.is_empty() {
                r.i += 1;
                primary = top.node.unwrap();
                let done = stack.pop().unwrap();
                operator = std::mem::replace(&mut top, done).operator;
                continue;
            }
            if k == SLASH {
                r.i += 1;
                top.sequence = parts.add(SEQUENCE, None, Vec::new());
                parts.0[top.choice].kids.push(top.sequence);
            } else if !matches!(k, AND | NOT | STRING | ID | DOT | GROUP) {
                if stack.is_empty() {
                    return Ok(top.choice);
                }
                return Err(r.fail("token of type ')'"));
            }
            break;
        }
    }
}

fn rule(r: &mut Reader, parts: &mut Parts) -> Result<(String, usize)> {
    let name = r.expect(ID)?;
    r.expect(ARROW)?;
    let definition = definition(r, parts)?;
    r.expect(SEMICOLON)?;
    Ok((name, definition))
}

pub fn parse_peg(src: &str) -> Result<Grammar> {
    parse_grammar(src, &PEG_TOKENS, TITLE, unescape, rule)
}

/// 述語が見せる、先読みする式。
fn label(node: &RailroadNode) -> String {
    match node {
        RailroadNode::Terminal(value) => format!("\"{value}\""),
        RailroadNode::Nonterminal(name) => name.clone(),
        RailroadNode::Special(text) => text.clone(),
        _ => "(...)".to_string(),
    }
}

pub fn node(part: &Part, kids: Vec<usize>, n: &mut Nodes) -> usize {
    let text = || part.text.as_deref().unwrap_or("");
    let operator = part.text.as_deref().filter(|o| !o.is_empty());
    match part.tag {
        CHOICE => n.choice(kids),
        SEQUENCE => n.sequence(kids),
        PREFIX => match operator {
            Some(op) => {
                let text = format!("{op}{}", label(&n.0[kids[0]]));
                n.special(&text)
            }
            None => kids[0],
        },
        SUFFIX => match operator {
            None => kids[0],
            Some("?") => n.optional(kids[0]),
            Some(op) => n.repetition(kids[0], if op == "+" { 1.0 } else { 0.0 }, f64::INFINITY),
        },
        LITERAL => n.terminal(text()),
        IDENTIFIER => n.nonterminal(text()),
        ANY => n.special(text()),
        _ => kids[0],
    }
}
