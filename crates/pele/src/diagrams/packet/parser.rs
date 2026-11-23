//! packet の読み手（pele の `src/diagrams/packet/parser.ts`）。

use std::sync::LazyLock;

use crate::diagrams::common::tokens::{
    ACC_DESCR, ACC_TITLE, DIRECTIVE, Lexicon, NEWLINE, Pattern, Reader, SINGLE_LINE_COMMENT,
    STRING, TITLE, TokenType, WHITESPACE, YAML, acc_descr_value, acc_title_value, keyword,
    string_value, title_value, tokenize,
};
use crate::errors::Result;
use crate::js;

mod t {
    pub const PACKET_BETA: i32 = 0;
    pub const PACKET: i32 = 1;
    pub const DASH: i32 = 2;
    pub const PLUS: i32 = 3;
    pub const COLON: i32 = 4;
    pub const ACC_DESCR: i32 = 5;
    pub const ACC_TITLE: i32 = 6;
    pub const TITLE: i32 = 7;
    pub const INT: i32 = 8;
    pub const STRING: i32 = 9;
    pub const NEWLINE: i32 = 10;
}

fn is_digit(src: &[u16], p: usize) -> bool {
    src.get(p).is_some_and(|c| (48..=57).contains(c))
}

/// `0|[1-9][0-9]*(?!\.)`。後ろが `.` なら数字を 1 つ戻す（正規表現の後戻りと同じ）。
fn int(src: &[u16], p: usize) -> Option<usize> {
    match src.get(p) {
        Some(48) => Some(p + 1),
        Some(c) if (49..=57).contains(c) => {
            let mut e = p + 1;
            while is_digit(src, e) {
                e += 1;
            }
            if src.get(e) != Some(&46) {
                Some(e)
            } else if e - 1 > p {
                Some(e - 1)
            } else {
                None
            }
        }
        _ => None,
    }
}

pub static PACKET_TOKENS: LazyLock<Lexicon> = LazyLock::new(|| {
    Lexicon::new(vec![
        TokenType::literal("packet-beta", "packet-beta"),
        keyword("packet"),
        // 前付けの区画もダッシュで始まるので、Langium はダッシュよりそれを取る。
        TokenType::literal("-", "-").longer(&["YAML"]),
        TokenType::literal("+", "+"),
        TokenType::literal(":", ":"),
        ACC_DESCR.clone(),
        ACC_TITLE.clone(),
        TITLE.clone(),
        TokenType::new("INT", Pattern::regex(int)),
        STRING.clone(),
        NEWLINE.clone(),
        WHITESPACE.clone(),
        YAML.clone(),
        DIRECTIVE.clone(),
        SINGLE_LINE_COMMENT.clone(),
    ])
});

pub struct PacketBlockAst {
    pub start: Option<f64>,
    pub end: Option<f64>,
    pub bits: Option<f64>,
    pub label: String,
}

#[derive(Default)]
pub struct PacketAst {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub blocks: Vec<PacketBlockAst>,
}

pub fn parse_packet(src: &str) -> Result<PacketAst> {
    let mut r = Reader::new(
        tokenize(src, &PACKET_TOKENS, "packet")?,
        &PACKET_TOKENS,
        "packet",
    );
    let mut ast = PacketAst::default();

    let end_of_line = |r: &mut Reader| -> Result<()> {
        if r.kind() == -1 {
            return Ok(());
        }
        if r.kind() != t::NEWLINE {
            return Err(r.fail("a line break or the end of input"));
        }
        while r.kind() == t::NEWLINE {
            r.i += 1;
        }
        Ok(())
    };

    while r.kind() == t::NEWLINE {
        r.i += 1;
    }
    if !r.accept(t::PACKET_BETA) {
        r.expect(t::PACKET)?;
    }
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
            t::INT | t::PLUS => {
                let mut block = PacketBlockAst {
                    start: None,
                    end: None,
                    bits: None,
                    label: String::new(),
                };
                if r.accept(t::PLUS) {
                    block.bits = Some(js::number(&r.expect(t::INT)?));
                } else {
                    block.start = Some(js::number(&r.take()));
                    if r.accept(t::DASH) {
                        block.end = Some(js::number(&r.expect(t::INT)?));
                    }
                }
                r.expect(t::COLON)?;
                block.label = string_value(&r.expect(t::STRING)?);
                ast.blocks.push(block);
                end_of_line(&mut r)?;
            }
            _ => return Err(r.fail("a field, a title, or a line break")),
        }
    }
    Ok(ast)
}
