//! treemap の読み手（pele の `src/diagrams/treemap/parser.ts`）。

use std::sync::LazyLock;

use crate::diagrams::common::tokens::{
    ACC_DESCR, ACC_TITLE, Lexicon, Pattern, Reader, TokenType, acc_descr_value, acc_title_value,
    title_before, title_value, tokenize,
};
use crate::errors::Result;
use crate::js;
use crate::util::chars::{NONE, is_space, is_word};

mod t {
    pub const ACC_DESCR: i32 = 0;
    pub const ACC_TITLE: i32 = 1;
    pub const TITLE: i32 = 2;
    pub const KEYWORD: i32 = 3;
    pub const CLASS_DEF: i32 = 4;
    pub const STYLE_SEPARATOR: i32 = 5;
    pub const SEPARATOR: i32 = 6;
    pub const COMMA: i32 = 7;
    pub const INDENTATION: i32 = 8;
    pub const ID: i32 = 12;
    pub const NUMBER: i32 = 13;
    pub const STRING: i32 = 14;
}

fn at(src: &[u16], p: usize) -> u32 {
    src.get(p).map_or(NONE, |&c| c as u32)
}

fn has(src: &[u16], p: usize, word: &str) -> bool {
    src.get(p..)
        .is_some_and(|s| s.len() >= word.len() && word.bytes().zip(s).all(|(w, &c)| w as u16 == c))
}

fn is_letter_(c: u32) -> bool {
    (65..=90).contains(&c) || (97..=122).contains(&c) || c == 95
}

/// `classDef\s+([a-zA-Z_][a-zA-Z0-9_]+)(?:\s+([^;\r\n]*))?(?:;)?` の終わりと、2 つの組（の位置）。
/// 字句の終わりと、名前と（あれば）スタイルの範囲。
type ClassDefMatch = (usize, (usize, usize), Option<(usize, usize)>);

fn class_def(src: &[u16], p: usize) -> Option<ClassDefMatch> {
    if !has(src, p, "classDef") {
        return None;
    }
    let mut q = p + 8;
    let s = q;
    while is_space(at(src, q)) {
        q += 1;
    }
    if q == s || !is_letter_(at(src, q)) {
        return None;
    }
    let name = q;
    q += 1;
    let w = q;
    while is_word(at(src, q)) {
        q += 1;
    }
    if q == w {
        return None;
    }
    let name = (name, q);
    let mut style = None;
    let mut r = q;
    while is_space(at(src, r)) {
        r += 1;
    }
    if r > q {
        let from = r;
        while r < src.len() && !matches!(src[r], 59 | 13 | 10) {
            r += 1;
        }
        style = Some((from, r));
        q = r;
    }
    if at(src, q) == 59 {
        q += 1;
    }
    Some((q, name, style))
}

/// `"[^"]*"|'[^']*'`
fn quoted(src: &[u16], p: usize) -> Option<usize> {
    let quote = at(src, p);
    if quote != 34 && quote != 39 {
        return None;
    }
    (p + 1..src.len())
        .find(|&k| src[k] as u32 == quote)
        .map(|k| k + 1)
}

fn blank_run(src: &[u16], mut p: usize) -> usize {
    while matches!(at(src, p), 9 | 32) {
        p += 1;
    }
    p
}

/// Mermaid の treemap の文法は自分の終端を持つ。空白とタブの並びはどれも INDENTATION の字句（WS より前にある）で、
/// 改行は隠れるので、行は字句だけで分かれ、行の字下げはその前の空白になる。
pub static TREEMAP_TOKENS: LazyLock<Lexicon> = LazyLock::new(|| {
    Lexicon::new(vec![
        ACC_DESCR.clone(),
        ACC_TITLE.clone(),
        title_before(|src, q| is_word(at(src, q))),
        TokenType::new(
            "TREEMAP_KEYWORD",
            Pattern::regex(|src, p| {
                if has(src, p, "treemap-beta") {
                    Some(p + 12)
                } else if has(src, p, "treemap") {
                    Some(p + 7)
                } else {
                    None
                }
            }),
        )
        .first("t"),
        TokenType::new(
            "CLASS_DEF",
            Pattern::regex(|src, p| class_def(src, p).map(|m| m.0)),
        )
        .first("c"),
        TokenType::literal("STYLE_SEPARATOR", ":::"),
        TokenType::literal("SEPARATOR", ":"),
        TokenType::literal("COMMA", ","),
        TokenType::new(
            "INDENTATION",
            Pattern::regex(|src, p| Some(blank_run(src, p))),
        )
        .first("\t "),
        TokenType::new("WS", Pattern::regex(|src, p| Some(blank_run(src, p))))
            .hidden()
            .first("\t "),
        TokenType::new(
            "ML_COMMENT",
            Pattern::regex(|src, p| {
                has(src, p, "%%").then(|| {
                    let mut e = p + 2;
                    while e < src.len() && src[e] != 10 {
                        e += 1;
                    }
                    e
                })
            }),
        )
        .hidden()
        .first("%"),
        TokenType::new(
            "NL",
            Pattern::regex(|src, p| {
                if at(src, p) == 13 && at(src, p + 1) == 10 {
                    Some(p + 2)
                } else if at(src, p) == 10 {
                    Some(p + 1)
                } else {
                    None
                }
            }),
        )
        .hidden()
        .first("\r\n"),
        TokenType::new(
            "ID2",
            Pattern::regex(|src, p| {
                if !is_letter_(at(src, p)) {
                    return None;
                }
                let mut e = p + 1;
                while is_word(at(src, e)) {
                    e += 1;
                }
                Some(e)
            }),
        ),
        TokenType::new(
            "NUMBER2",
            Pattern::regex(|src, p| {
                let mut e = p;
                while matches!(at(src, e), 48..=57 | 95 | 46 | 44) {
                    e += 1;
                }
                Some(e)
            }),
        )
        .first("0123456789_.,"),
        TokenType::new("STRING2", Pattern::regex(quoted)).first("\"'"),
    ])
});

#[derive(Clone, Debug)]
pub enum ItemAst {
    Section {
        name: String,
        class_selector: Option<String>,
    },
    Leaf {
        name: String,
        value: f64,
        class_selector: Option<String>,
    },
}

#[derive(Clone, Debug)]
pub enum RowAst {
    Row {
        indent: Option<usize>,
        item: ItemAst,
    },
    ClassDef {
        indent: Option<usize>,
        class_name: String,
        style_text: Option<String>,
    },
}

#[derive(Clone, Debug, Default)]
pub struct TreemapAst {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub rows: Vec<RowAst>,
}

fn unquote(text: &str) -> String {
    let u = js::utf16(text);
    js::from16(&u[1..u.len() - 1])
}

pub fn parse_treemap(src: &str) -> Result<TreemapAst> {
    let lex = &*TREEMAP_TOKENS;
    let mut r = Reader::new(tokenize(src, lex, "treemap")?, lex, "treemap");
    let mut ast = TreemapAst::default();

    r.expect(t::KEYWORD)?;
    while r.kind() != -1 {
        let kind = r.kind();
        if kind == t::ACC_DESCR {
            ast.acc_descr = Some(acc_descr_value(&r.take()));
            continue;
        }
        if kind == t::ACC_TITLE {
            ast.acc_title = Some(acc_title_value(&r.take()));
            continue;
        }
        if kind == t::TITLE {
            ast.title = Some(title_value(&r.take()));
            continue;
        }
        if kind != t::INDENTATION && kind != t::STRING && kind != t::CLASS_DEF {
            return Err(r.fail("a node, a class definition, or a title"));
        }

        let indent = (kind == t::INDENTATION).then(|| js::len16(&r.take()));
        if r.kind() == t::CLASS_DEF {
            let text = js::utf16(&r.take());
            let (_, name, style) = class_def(&text, 0).unwrap();
            let style_text = style
                .map(|(a, b)| js::from16(&text[a..b]))
                .filter(|s| !s.is_empty());
            ast.rows.push(RowAst::ClassDef {
                indent,
                class_name: js::from16(&text[name.0..name.1]),
                style_text,
            });
            continue;
        }

        let name = unquote(&r.expect(t::STRING)?);
        // 葉は名前の後にコロンかカンマが続くもの（前に空白があってもなくても）。
        let after = if r.kind() == t::INDENTATION {
            r.i + 1
        } else {
            r.i
        };
        let mut item = if r.kind_at(after) == t::SEPARATOR || r.kind_at(after) == t::COMMA {
            r.i = after + 1;
            r.accept(t::INDENTATION);
            let number = r.expect(t::NUMBER)?;
            ItemAst::Leaf {
                name,
                value: js::parse_float(&number.replace(',', "")),
                class_selector: None,
            }
        } else {
            ItemAst::Section {
                name,
                class_selector: None,
            }
        };
        if r.accept(t::STYLE_SEPARATOR) {
            let selector = r.expect(t::ID)?;
            match &mut item {
                ItemAst::Section { class_selector, .. } | ItemAst::Leaf { class_selector, .. } => {
                    *class_selector = Some(selector)
                }
            }
        }
        ast.rows.push(RowAst::Row { indent, item });
    }
    Ok(ast)
}
