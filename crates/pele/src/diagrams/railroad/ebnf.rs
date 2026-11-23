//! `railroad-ebnf-beta`: W3C と ISO 14977 の書き方の EBNF（pele の `src/diagrams/railroad/ebnf.ts`）。

use std::sync::LazyLock;

use super::common::{
    Grammar, Nodes, Part, Parts, acc_descr, acc_title, at, block_comment, hidden_tokens,
    identifier, literal, opening, parse_grammar, quoted, strip_ends, title, unescape, word_or_dash,
};
use super::types::RailroadNode;
use crate::diagrams::common::tokens::{Lexicon, Pattern, Reader, TokenType};
use crate::errors::Result;
use crate::js;
use crate::util::chars::is_space;

const DEFINES: i32 = 1;
const EQUALS: i32 = 2;
const SEMICOLON: i32 = 3;
const BAR: i32 = 4;
const COMMA: i32 = 5;
const GROUP: i32 = 6;
const OPTION: i32 = 8;
const REPEAT: i32 = 10;
const QUESTION: i32 = 12;
const STAR: i32 = 13;
const PLUS: i32 = 14;
const MINUS: i32 = 15;
const TITLE: i32 = 16;
const ID: i32 = 19;
const STRING: i32 = 20;
const SPECIAL: i32 = 21;

/// 疑問符、空白でないものを含み `?` も `;` もない文字、疑問符。
fn special_sequence(src: &[u16], p: usize) -> Option<usize> {
    if at(src, p) != 63 {
        return None;
    }
    let mut solid = false;
    for (i, &c) in src.iter().enumerate().skip(p + 1) {
        let c = c as u32;
        if c == 63 {
            return solid.then_some(i + 1);
        }
        if c == 59 {
            return None;
        }
        solid = solid || !is_space(c);
    }
    None
}

pub static EBNF_TOKENS: LazyLock<Lexicon> = LazyLock::new(|| {
    let mut types: Vec<TokenType> = vec![
        opening("railroad-ebnf-beta", "EBNF_ID"),
        literal("::=", &[]),
        literal("=", &[]),
        literal(";", &[]),
        literal("|", &[]),
        literal(",", &[]),
        literal("(", &["EBNF_ISO_COMMENT"]),
        literal(")", &[]),
        literal("[", &[]),
        literal("]", &[]),
        literal("{", &[]),
        literal("}", &[]),
        literal("?", &["EBNF_SPECIAL_SEQUENCE"]),
        literal("*", &[]),
        literal("+", &[]),
        literal("-", &["EBNF_YAML"]),
        title(word_or_dash),
        acc_title(),
        acc_descr(),
        identifier("EBNF_ID"),
        quoted("EBNF_STRING"),
        TokenType::new("EBNF_SPECIAL_SEQUENCE", Pattern::regex(special_sequence)).first("?"),
    ];
    types.extend(hidden_tokens("EBNF"));
    types.push(block_comment("EBNF_BLOCK_COMMENT", "/*", "*/"));
    types.push(block_comment("EBNF_ISO_COMMENT", "(*", "*)"));
    Lexicon::new(types)
});

// 構文の木の節の種類。
const CHOICE: u8 = 0;
const SEQUENCE: u8 = 1;
const TERM: u8 = 2;
const TERMINAL: u8 = 3;
const NONTERMINAL: u8 = 4;
const SPECIAL_PART: u8 = 5;
const GROUP_PART: u8 = 6;
const OPTIONAL_PART: u8 = 7;
const REPETITION_PART: u8 = 8;

// 後置の種類。
const OPTIONAL_POSTFIX: u8 = 0;
const ZERO_OR_MORE: u8 = 1;
const ONE_OR_MORE: u8 = 2;
const EXCEPTION: u8 = 3;

/// 開いた括弧 1 つ、または積みの底の規則そのもの。
struct Frame {
    choice: usize,
    sequence: usize,
    term: Option<usize>,
    /// 括弧の節、その閉じの字句、例外の右側か。
    node: Option<usize>,
    close: i32,
    except: bool,
}

fn frame(parts: &mut Parts, node: Option<usize>, choice: usize, close: i32, except: bool) -> Frame {
    let sequence = parts.add(SEQUENCE, None, Vec::new());
    parts.0[choice].kids.push(sequence);
    Frame {
        choice,
        sequence,
        term: None,
        node,
        close,
        except,
    }
}

fn definition(r: &mut Reader, parts: &mut Parts) -> Result<usize> {
    let mut stack: Vec<Frame> = Vec::new();
    let root = parts.add(CHOICE, None, Vec::new());
    let mut top = frame(parts, None, root, -2, false);
    let mut except = false;
    loop {
        let kind = r.kind();
        let mut primary = match kind {
            STRING => parts.add(TERMINAL, Some(unescape(&r.take())), Vec::new()),
            ID => parts.add(NONTERMINAL, Some(r.take()), Vec::new()),
            SPECIAL => {
                let text = js::trim(&strip_ends(&r.take())).to_string();
                parts.add(SPECIAL_PART, Some(text), Vec::new())
            }
            GROUP | OPTION | REPEAT => {
                r.i += 1;
                let element = parts.add(CHOICE, None, Vec::new());
                let tag =
                    [GROUP_PART, OPTIONAL_PART, REPETITION_PART][((kind - GROUP) >> 1) as usize];
                let node = parts.add(tag, None, vec![element]);
                let next = frame(parts, Some(node), element, kind + 1, except);
                stack.push(std::mem::replace(&mut top, next));
                except = false;
                continue;
            }
            _ => return Err(r.fail("a terminal, a name, or an opening bracket")),
        };
        loop {
            if except {
                let term = top.term.unwrap();
                parts.0[term].kids.push(primary);
                parts.0[term].ops.push(EXCEPTION);
                except = false;
            } else {
                let term = parts.add(TERM, None, vec![primary]);
                top.term = Some(term);
                parts.0[top.sequence].kids.push(term);
            }
            let term = top.term.unwrap();
            loop {
                let op = match r.kind() {
                    QUESTION => OPTIONAL_POSTFIX,
                    STAR => ZERO_OR_MORE,
                    PLUS => ONE_OR_MORE,
                    _ => break,
                };
                r.i += 1;
                parts.0[term].ops.push(op);
            }
            let k = r.kind();
            if k == top.close {
                r.i += 1;
                primary = top.node.unwrap();
                except = top.except;
                top = stack.pop().unwrap();
                continue;
            }
            if k == MINUS {
                r.i += 1;
                except = true;
            } else if k == COMMA {
                r.i += 1;
            } else if k == BAR {
                r.i += 1;
                top.sequence = parts.add(SEQUENCE, None, Vec::new());
                parts.0[top.choice].kids.push(top.sequence);
            } else if !matches!(k, STRING | ID | SPECIAL | GROUP | OPTION | REPEAT) {
                if stack.is_empty() {
                    return Ok(top.choice);
                }
                let name = &r.lexicon.types[top.close as usize].name;
                return Err(r.fail(&format!("token of type '{name}'")));
            }
            break;
        }
    }
}

fn rule(r: &mut Reader, parts: &mut Parts) -> Result<(String, usize)> {
    let name = r.expect(ID)?;
    if !r.accept(EQUALS) {
        r.expect(DEFINES)?;
    }
    let definition = definition(r, parts)?;
    r.expect(SEMICOLON)?;
    Ok((name, definition))
}

pub fn parse_ebnf(src: &str) -> Result<Grammar> {
    parse_grammar(src, &EBNF_TOKENS, TITLE, unescape, rule)
}

pub fn node(part: &Part, kids: Vec<usize>, n: &mut Nodes) -> usize {
    let text = || part.text.as_deref().unwrap_or("");
    match part.tag {
        CHOICE => n.choice(kids),
        SEQUENCE => n.sequence(kids),
        TERM => {
            // 後置の演算子は、項の中のその前のすべてに掛かる。
            let mut result = kids[0];
            let mut next = 1;
            for &op in &part.ops {
                result = match op {
                    OPTIONAL_POSTFIX => n.optional(result),
                    EXCEPTION => {
                        let minus = n.terminal("-");
                        let except = kids[next];
                        next += 1;
                        n.add(RailroadNode::Sequence(vec![result, minus, except]))
                    }
                    _ => n.repetition(
                        result,
                        if op == ONE_OR_MORE { 1.0 } else { 0.0 },
                        f64::INFINITY,
                    ),
                };
            }
            result
        }
        TERMINAL => n.terminal(text()),
        NONTERMINAL => n.nonterminal(text()),
        SPECIAL_PART => n.special(text()),
        GROUP_PART => kids[0],
        OPTIONAL_PART => n.optional(kids[0]),
        _ => n.repetition(kids[0], 0.0, f64::INFINITY),
    }
}
