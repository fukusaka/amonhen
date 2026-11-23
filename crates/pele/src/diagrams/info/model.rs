//! info の形（pele の `src/diagrams/info/model.ts`）。

use super::parser::InfoAst;
use crate::js;

pub struct InfoModel {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
}

/// `ast.title || title`。
pub fn or_title(own: Option<String>, title: Option<&str>) -> Option<String> {
    match own {
        Some(t) if !t.is_empty() => Some(t),
        _ => title.map(str::to_string),
    }
}

/// `accTitle.replace(/^\s+/g, '')`（空なら None）。
pub fn acc_title(text: Option<String>) -> Option<String> {
    text.filter(|t| !t.is_empty())
        .map(|t| js::trim_start(&t).to_string())
}

/// `accDescr.replace(/\n\s+/g, '\n')`（空なら None）。
pub fn acc_descr(text: Option<String>) -> Option<String> {
    let text = text.filter(|t| !t.is_empty())?;
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '\n' {
            while chars.peek().is_some_and(|&c| js::is_js_space(c)) {
                chars.next();
            }
        }
    }
    Some(out)
}

pub fn build_info(ast: InfoAst, title: Option<&str>) -> InfoModel {
    InfoModel {
        title: or_title(ast.title, title),
        acc_title: acc_title(ast.acc_title),
        acc_descr: acc_descr(ast.acc_descr),
    }
}
