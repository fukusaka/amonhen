//! pie の読み手（pele の `src/diagrams/pie/parser.ts`）。

use std::sync::LazyLock;

use crate::diagrams::common::tokens::{
    ACC_DESCR, ACC_TITLE, DIRECTIVE, Lexicon, NEWLINE, Pattern, Reader, SINGLE_LINE_COMMENT,
    STRING, Scanner, TITLE, TokenType, WHITESPACE, YAML, acc_descr_value, acc_title_value, keyword,
    string_value, title_value, tokenize,
};
use crate::errors::Result;
use crate::js;

mod t {
    pub const SHOW_DATA: i32 = 0;
    pub const PIE: i32 = 1;
    pub const COLON: i32 = 2;
    pub const NUMBER: i32 = 3;
    pub const ACC_DESCR: i32 = 4;
    pub const ACC_TITLE: i32 = 5;
    pub const TITLE: i32 = 6;
    pub const STRING: i32 = 7;
    pub const NEWLINE: i32 = 8;
}

fn code(src: &[u16], p: usize) -> u32 {
    src.get(p).map_or(u32::MAX, |&c| c as u32)
}

fn digits(src: &[u16], from: usize) -> usize {
    let mut i = from;
    while (48..=57).contains(&code(src, i)) {
        i += 1;
    }
    i
}

/// NUMBER_PIE の正規表現（`(?:-?[0-9]+\.[0-9]+(?!\.))|(?:-?(0|[1-9][0-9]*)(?!\.))`）と同じに答える。正規表現は
/// 小数がないと分かるまで数字の並びを読み、長い 0 の並びではそれを数字ごとにやり直す。
fn number_scanner() -> Scanner {
    let mut from = 0usize;
    let mut end = 0usize;
    let mut fraction: Option<usize> = None;
    Box::new(move |src, p, _| {
        let at = if code(src, p) == 45 { p + 1 } else { p };
        if at < from || at >= end {
            from = at;
            end = digits(src, at);
            fraction = None;
            if end > at && code(src, end) == 46 {
                let stop = digits(src, end + 1);
                let length = stop - end - 1;
                if length > 0 && code(src, stop) != 46 {
                    fraction = Some(stop);
                } else if length > 1 {
                    fraction = Some(stop - 1);
                }
            }
        }
        if end == at {
            return None;
        }
        if let Some(f) = fraction {
            return Some(f);
        }
        if code(src, at) == 48 {
            return if code(src, at + 1) == 46 {
                None
            } else {
                Some(at + 1)
            };
        }
        if code(src, end) != 46 {
            return Some(end);
        }
        if end - 1 > at { Some(end - 1) } else { None }
    })
}

pub static PIE_TOKENS: LazyLock<Lexicon> = LazyLock::new(|| {
    Lexicon::new(vec![
        keyword("showData"),
        keyword("pie"),
        TokenType::literal(":", ":"),
        TokenType::new("NUMBER_PIE", Pattern::scanner(number_scanner)).first("-0123456789"),
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

#[derive(Default)]
pub struct PieAst {
    pub show_data: bool,
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub sections: Vec<(String, f64)>,
}

pub fn parse_pie(src: &str) -> Result<PieAst> {
    let mut r = Reader::new(tokenize(src, &PIE_TOKENS, "pie")?, &PIE_TOKENS, "pie");
    let mut ast = PieAst::default();

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
    r.expect(t::PIE)?;
    ast.show_data = r.accept(t::SHOW_DATA);
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
            t::STRING => {
                let label = string_value(&r.take());
                r.expect(t::COLON)?;
                let value = js::number(&r.expect(t::NUMBER)?);
                ast.sections.push((label, value));
                end_of_line(&mut r)?;
            }
            _ => return Err(r.fail("a section, a title, or a line break")),
        }
    }
    Ok(ast)
}
