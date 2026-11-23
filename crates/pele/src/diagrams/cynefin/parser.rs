//! cynefin の読み手（pele の `src/diagrams/cynefin/parser.ts`）。

use std::sync::LazyLock;

use crate::diagrams::common::tokens::{
    ACC_DESCR, ACC_TITLE, DIRECTIVE, Lexicon, NEWLINE, Pattern, Reader, SINGLE_LINE_COMMENT,
    STRING, TITLE, TokenType, WHITESPACE, YAML, acc_descr_value, acc_title_value, keyword,
    string_value, title_value, tokenize,
};
use crate::errors::Result;

mod t {
    pub const KEYWORD_COLON: i32 = 0;
    pub const KEYWORD: i32 = 1;
    pub const ARROW: i32 = 2;
    pub const COLON: i32 = 3;
    pub const DOMAIN: i32 = 4;
    pub const ACC_DESCR: i32 = 5;
    pub const ACC_TITLE: i32 = 6;
    pub const TITLE: i32 = 7;
    pub const STRING: i32 = 8;
    pub const NEWLINE: i32 = 9;
}

pub const DOMAIN_NAMES: [&str; 5] = ["complex", "complicated", "clear", "chaotic", "confusion"];

/// `complex|complicated|clear|chaotic|confusion`
fn domain_name(src: &[u16], p: usize) -> Option<usize> {
    DOMAIN_NAMES.iter().find_map(|name| {
        let n = name.len();
        src.get(p..p + n)
            .is_some_and(|s| name.bytes().zip(s).all(|(w, &c)| w as u16 == c))
            .then_some(p + n)
    })
}

pub static CYNEFIN_TOKENS: LazyLock<Lexicon> = LazyLock::new(|| {
    Lexicon::new(vec![
        TokenType::literal("cynefin-beta:", "cynefin-beta:"),
        keyword("cynefin-beta"),
        TokenType::literal("-->", "-->"),
        TokenType::literal(":", ":"),
        TokenType::new("DOMAIN_NAME", Pattern::regex(domain_name)).first("c"),
        ACC_DESCR.clone(),
        ACC_TITLE.clone(),
        TITLE.clone(),
        STRING.clone(),
        NEWLINE.clone(),
        WHITESPACE.clone(),
        YAML.clone(),
        DIRECTIVE.clone(),
        SINGLE_LINE_COMMENT.clone(),
    ])
});

#[derive(Clone, Debug)]
pub struct DomainBlockAst {
    pub domain: String,
    pub items: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct TransitionAst {
    pub from: String,
    pub to: String,
    pub label: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct CynefinAst {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub domains: Vec<DomainBlockAst>,
    pub transitions: Vec<TransitionAst>,
}

pub fn parse_cynefin(src: &str) -> Result<CynefinAst> {
    let lex = &*CYNEFIN_TOKENS;
    let mut r = Reader::new(tokenize(src, lex, "cynefin")?, lex, "cynefin");
    let mut ast = CynefinAst::default();

    fn skip_lines(r: &mut Reader) {
        while r.kind() == t::NEWLINE {
            r.i += 1;
        }
    }
    fn end_of_line(r: &mut Reader) -> Result<()> {
        if r.kind() == -1 {
            return Ok(());
        }
        if r.kind() != t::NEWLINE {
            return Err(r.fail("a line break or the end of input"));
        }
        skip_lines(r);
        Ok(())
    }

    skip_lines(&mut r);
    if r.kind() != t::KEYWORD && r.kind() != t::KEYWORD_COLON {
        return Err(r.fail("'cynefin-beta'"));
    }
    r.i += 1;
    while r.kind() != -1 {
        match r.kind() {
            t::NEWLINE => r.i += 1,
            t::ACC_DESCR => {
                ast.acc_descr = Some(acc_descr_value(&r.take()));
                end_of_line(&mut r)?;
            }
            t::ACC_TITLE => {
                ast.acc_title = Some(acc_title_value(&r.take()));
                end_of_line(&mut r)?;
            }
            t::TITLE => {
                ast.title = Some(title_value(&r.take()));
                end_of_line(&mut r)?;
            }
            t::DOMAIN => {
                let name = r.take();
                if r.accept(t::ARROW) {
                    let to = r.expect(t::DOMAIN)?;
                    let mut transition = TransitionAst {
                        from: name,
                        to,
                        label: None,
                    };
                    if r.accept(t::COLON) {
                        transition.label = Some(string_value(&r.expect(t::STRING)?));
                    }
                    end_of_line(&mut r)?;
                    ast.transitions.push(transition);
                    continue;
                }
                let mut block = DomainBlockAst {
                    domain: name,
                    items: Vec::new(),
                };
                skip_lines(&mut r);
                while r.kind() == t::STRING {
                    block.items.push(string_value(&r.take()));
                    skip_lines(&mut r);
                }
                ast.domains.push(block);
            }
            _ => return Err(r.fail("a domain, a transition, a title, or a line break")),
        }
    }
    Ok(ast)
}
