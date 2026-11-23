//! treeView の読み手（pele の `src/diagrams/treeview/parser.ts`）。

use std::sync::LazyLock;

use crate::diagrams::common::acc_descr::acc_descr;
use crate::diagrams::common::tokens::{
    ACC_TITLE, Lexicon, Pattern, Reader, TokenType, acc_descr_value, acc_title_value, keyword,
    title_before, title_value, tokenize,
};
use crate::errors::Result;
use crate::js;
use crate::util::chars::{NONE, is_blank, is_word};

mod t {
    pub const KEYWORD: i32 = 0;
    pub const ACC_DESCR: i32 = 1;
    pub const ACC_TITLE: i32 = 2;
    pub const TITLE: i32 = 3;
    pub const CLASS_ANNOTATION: i32 = 4;
    pub const ICON_ANNOTATION: i32 = 5;
    pub const DESC_ANNOTATION: i32 = 6;
    pub const INDENTATION: i32 = 7;
    pub const QUOTED_NAME: i32 = 8;
    pub const BARE_NAME: i32 = 12;
}

fn at(src: &[u16], p: usize) -> u32 {
    src.get(p).map_or(NONE, |&c| c as u32)
}

fn has(src: &[u16], p: usize, word: &str) -> bool {
    src.get(p..)
        .is_some_and(|s| s.len() >= word.len() && word.bytes().zip(s).all(|(w, &c)| w as u16 == c))
}

fn blank_run(src: &[u16], mut p: usize) -> usize {
    while is_blank(at(src, p)) {
        p += 1;
    }
    p
}

fn annotation_at(src: &[u16], at_: usize) -> bool {
    if has(src, at_, "##") || has(src, at_, "icon(") {
        return true;
    }
    if !has(src, at_, ":::") {
        return false;
    }
    let i = blank_run(src, at_ + 3);
    let c = at(src, i);
    (65..=90).contains(&c) || (97..=122).contains(&c) || c == 95
}

/// 正規表現は空白ごとに注釈を先読みし、長い空白を何度も走査する。ここでは空白の並びごとに 1 度見る。
fn match_bare_name(src: &[u16], at_: usize, _: i32) -> Option<usize> {
    let first = at(src, at_);
    if is_blank(first) || matches!(first, 10 | 13 | 34 | 39) {
        return None;
    }
    if has(src, at_, ":::") || has(src, at_, "icon(") || has(src, at_, "##") {
        return None;
    }
    let n = src.len();
    let mut i = at_ + 1;
    while i < n {
        let c = src[i] as u32;
        if c == 10 || c == 13 {
            break;
        }
        if is_blank(c) {
            let j = blank_run(src, i + 1);
            if annotation_at(src, j) {
                break;
            }
            i = j;
        } else {
            i += 1;
        }
    }
    Some(i)
}

/// `[^\n\r]*` の終わり。
fn line_stop(src: &[u16], mut p: usize) -> usize {
    while p < src.len() && src[p] != 10 && src[p] != 13 {
        p += 1;
    }
    p
}

/// `[\w-]*`
fn word_dash(src: &[u16], mut p: usize) -> usize {
    while is_word(at(src, p)) || at(src, p) == 45 {
        p += 1;
    }
    p
}

/// `[ \t]+:::[ \t]*[A-Za-z_][\w-]*`
fn class_annotation(src: &[u16], p: usize) -> Option<usize> {
    let q = blank_run(src, p);
    if q == p || !has(src, q, ":::") {
        return None;
    }
    let r = blank_run(src, q + 3);
    let c = at(src, r);
    if !((65..=90).contains(&c) || (97..=122).contains(&c) || c == 95) {
        return None;
    }
    Some(word_dash(src, r + 1))
}

/// `[ \t]+icon\([\w-]*(?::[\w-]+)?\)`
fn icon_annotation(src: &[u16], p: usize) -> Option<usize> {
    let q = blank_run(src, p);
    if q == p || !has(src, q, "icon(") {
        return None;
    }
    let mut r = word_dash(src, q + 5);
    if at(src, r) == 58 {
        let s = word_dash(src, r + 1);
        if s > r + 1 {
            r = s;
        }
    }
    (at(src, r) == 41).then_some(r + 1)
}

/// `[ \t]+##[^\n\r]*`
fn desc_annotation(src: &[u16], p: usize) -> Option<usize> {
    let q = blank_run(src, p);
    if q == p || !has(src, q, "##") {
        return None;
    }
    Some(line_stop(src, q + 2))
}

/// `"[^"]*"|'[^']*'`
fn quoted_name(src: &[u16], p: usize) -> Option<usize> {
    let quote = at(src, p);
    if quote != 34 && quote != 39 {
        return None;
    }
    (p + 1..src.len())
        .find(|&k| src[k] as u32 == quote)
        .map(|k| k + 1)
}

pub static TREEVIEW_TOKENS: LazyLock<Lexicon> = LazyLock::new(|| {
    Lexicon::new(vec![
        keyword("treeView-beta").longer(&["BARE_NAME"]),
        acc_descr("ACC_DESCR"),
        ACC_TITLE.clone(),
        // `(?!%%)[^\n\r]`
        title_before(|src, q| {
            let c = at(src, q);
            c != NONE && c != 10 && c != 13 && !(c == 37 && at(src, q + 1) == 37)
        }),
        TokenType::new("CLASS_ANNOTATION", Pattern::regex(class_annotation)).first("\t "),
        TokenType::new("ICON_ANNOTATION", Pattern::regex(icon_annotation)).first("\t "),
        TokenType::new("DESC_ANNOTATION", Pattern::regex(desc_annotation)).first("\t "),
        TokenType::new(
            "INDENTATION",
            Pattern::regex(|src, p| Some(blank_run(src, p))),
        )
        .first("\t "),
        TokenType::new("QUOTED_NAME", Pattern::regex(quoted_name)).first("\"'"),
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
        TokenType::new("BARE_NAME", Pattern::matcher(match_bare_name)),
    ])
});

#[derive(Clone, Debug, Default)]
pub struct TreeNodeAst {
    pub indent: Option<usize>,
    pub name: String,
    pub class_annotation: Option<String>,
    pub icon_annotation: Option<String>,
    pub desc_annotation: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct TreeViewAst {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub nodes: Vec<TreeNodeAst>,
}

fn trim_blanks_end(text: &str) -> String {
    text.trim_end_matches([' ', '\t']).to_string()
}

/// 先頭の UTF-16 の `n` 単位を除き、終わりの `m` 単位を除いた残り（`substring`）。
fn inner(text: &str, n: usize, m: usize) -> String {
    let u = js::utf16(text);
    let end = u.len().saturating_sub(m).max(n.min(u.len()));
    js::from16(&u[n.min(u.len())..end])
}

pub fn parse_tree_view(src: &str) -> Result<TreeViewAst> {
    let lex = &*TREEVIEW_TOKENS;
    let mut r = Reader::new(tokenize(src, lex, "treeView")?, lex, "treeView");
    let mut ast = TreeViewAst::default();
    r.expect(t::KEYWORD)?;
    loop {
        match r.kind() {
            t::ACC_DESCR => ast.acc_descr = Some(acc_descr_value(&r.take())),
            t::ACC_TITLE => ast.acc_title = Some(acc_title_value(&r.take())),
            t::TITLE => ast.title = Some(title_value(&r.take())),
            _ => break,
        }
    }
    while r.kind() != -1 {
        let mut node = TreeNodeAst::default();
        if r.kind() == t::INDENTATION {
            node.indent = Some(js::len16(&r.take()));
        }
        if r.kind() == t::QUOTED_NAME {
            node.name = inner(&r.take(), 1, 1);
        } else if r.kind() == t::BARE_NAME {
            node.name = trim_blanks_end(&r.take());
        } else {
            return Err(r.fail("a name"));
        }
        loop {
            match r.kind() {
                t::CLASS_ANNOTATION => {
                    let text = r.take();
                    node.class_annotation =
                        Some(js::trim(&inner(js::trim(&text), 3, 0)).to_string());
                }
                t::ICON_ANNOTATION => {
                    let text = r.take();
                    node.icon_annotation = Some(inner(js::trim(&text), 5, 1));
                }
                t::DESC_ANNOTATION => {
                    let text = r.take();
                    node.desc_annotation =
                        Some(js::trim(&inner(js::trim(&text), 2, 0)).to_string());
                }
                _ => break,
            }
        }
        ast.nodes.push(node);
    }
    Ok(ast)
}
