//! railroad の読んだもの（pele の `src/diagrams/railroad/db.ts`）。

use super::common::{Build, Grammar, to_rules};
use super::types::{Notation, RailroadModel, RailroadRule};
use super::{abnf, ebnf, ir, peg};
use crate::errors::{ErrorCode, PeleError, Result};
use crate::js;

/// Mermaid はキーワードごとに見分け手を持ち、どれも大文字小文字を区別しない。
/// `^\s*railroad-(ebnf-|abnf-|peg-)?beta`（i）
pub fn notation_of(source: &str) -> Option<Notation> {
    let rest = source.trim_start_matches(|c: char| js::is_js_space(c));
    let lower = |s: &str, n: usize| s.get(..n).map(|p| p.to_ascii_lowercase());
    if lower(rest, 9).as_deref() != Some("railroad-") {
        return None;
    }
    let rest = &rest[9..];
    for (infix, notation) in [
        ("ebnf-", Notation::Ebnf),
        ("abnf-", Notation::Abnf),
        ("peg-", Notation::Peg),
    ] {
        if lower(rest, infix.len()).as_deref() == Some(infix)
            && lower(&rest[infix.len()..], 4).as_deref() == Some("beta")
        {
            return Some(notation);
        }
    }
    (lower(rest, 4).as_deref() == Some("beta")).then_some(Notation::Railroad)
}

/// `.replace(/\n\s+/g, '\n')`
fn squeeze_indents(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '\n' {
            while chars.peek().is_some_and(|&n| js::is_js_space(n)) {
                chars.next();
            }
        }
    }
    out
}

/// ひとつの書き方で読み、Mermaid の 4 つの読み手と同じく形に入れる。
pub fn parse_railroad(source: &str, title: Option<&str>) -> Result<RailroadModel> {
    let Some(notation) = notation_of(source) else {
        return Err(PeleError::typed(
            "No railroad diagram keyword found.",
            ErrorCode::Syntax,
            "railroad",
        ));
    };
    let (grammar, build): (Grammar, Build) = match notation {
        Notation::Ebnf => (ebnf::parse_ebnf(source)?, ebnf::node),
        Notation::Abnf => (abnf::parse_abnf(source)?, abnf::node),
        Notation::Peg => (peg::parse_peg(source)?, peg::node),
        Notation::Railroad => (ir::parse_ir(source)?, ir::node),
    };
    let mut nodes = Vec::new();
    let rules = to_rules(&grammar, &mut nodes, build);
    let mut model = RailroadModel {
        notation,
        title: title.filter(|t| !t.is_empty()).map(str::to_string),
        acc_title: None,
        acc_descr: None,
        rules: Vec::new(),
        nodes,
    };
    if let Some(d) = grammar.acc_descr.as_deref().filter(|d| !d.is_empty()) {
        model.acc_descr = Some(squeeze_indents(d));
    }
    if let Some(t) = grammar.acc_title.as_deref().filter(|t| !t.is_empty()) {
        model.acc_title = Some(js::trim_start(t).to_string());
    }
    if let Some(t) = grammar.title.as_deref().filter(|t| !t.is_empty()) {
        model.title = Some(t.to_string());
    }
    for (name, definition) in rules {
        model.rules.push(RailroadRule { name, definition });
    }
    Ok(model)
}
