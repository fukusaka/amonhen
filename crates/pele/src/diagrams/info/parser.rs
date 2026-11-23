//! info の読み手（pele の `src/diagrams/info/parser.ts`）。

use std::sync::LazyLock;

use crate::diagrams::common::tokens::{
    ACC_DESCR, ACC_TITLE, DIRECTIVE, Lexicon, NEWLINE, Reader, SINGLE_LINE_COMMENT, TITLE,
    WHITESPACE, YAML, acc_descr_value, acc_title_value, keyword, title_value, tokenize,
};
use crate::errors::Result;

mod t {
    pub const SHOW_INFO: i32 = 0;
    pub const INFO: i32 = 1;
    pub const ACC_DESCR: i32 = 2;
    pub const ACC_TITLE: i32 = 3;
    pub const TITLE: i32 = 4;
    pub const NEWLINE: i32 = 5;
}

pub static INFO_TOKENS: LazyLock<Lexicon> = LazyLock::new(|| {
    Lexicon::new(vec![
        keyword("showInfo"),
        keyword("info"),
        ACC_DESCR.clone(),
        ACC_TITLE.clone(),
        TITLE.clone(),
        NEWLINE.clone(),
        WHITESPACE.clone(),
        YAML.clone(),
        DIRECTIVE.clone(),
        SINGLE_LINE_COMMENT.clone(),
    ])
});

#[derive(Default)]
pub struct InfoAst {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
}

pub fn parse_info(src: &str) -> Result<InfoAst> {
    let mut r = Reader::new(tokenize(src, &INFO_TOKENS, "info")?, &INFO_TOKENS, "info");
    let mut ast = InfoAst::default();

    while r.kind() == t::NEWLINE {
        r.i += 1;
    }
    r.expect(t::INFO)?;
    while r.kind() == t::NEWLINE {
        r.i += 1;
    }
    if r.accept(t::SHOW_INFO) {
        while r.kind() == t::NEWLINE {
            r.i += 1;
        }
    }
    while r.kind() != -1 {
        match r.kind() {
            t::ACC_DESCR => ast.acc_descr = Some(acc_descr_value(&r.take())),
            t::ACC_TITLE => ast.acc_title = Some(acc_title_value(&r.take())),
            t::TITLE => ast.title = Some(title_value(&r.take())),
            _ => return Err(r.fail("a title or the end of input")),
        }
        if r.kind() == -1 {
            break;
        }
        if r.kind() != t::NEWLINE {
            return Err(r.fail("a line break or the end of input"));
        }
        while r.kind() == t::NEWLINE {
            r.i += 1;
        }
    }
    Ok(ast)
}
