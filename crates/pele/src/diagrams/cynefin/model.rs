//! cynefin の形（pele の `src/diagrams/cynefin/model.ts`）。

use super::parser::CynefinAst;
use crate::js;

#[derive(Clone, Debug)]
pub struct CynefinDomain {
    pub name: String,
    pub items: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct CynefinTransition {
    pub from: String,
    pub to: String,
    pub label: Option<String>,
}

pub struct CynefinModel {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    /// 書いた領域（入れた順）。5 つの領域はどちらにしても描く。
    pub domains: Vec<CynefinDomain>,
    pub transitions: Vec<CynefinTransition>,
}

impl CynefinModel {
    pub fn domain(&self, name: &str) -> Option<&CynefinDomain> {
        self.domains.iter().find(|d| d.name == name)
    }
}

/// `.replace(/\n\s+/g, '\n')`
pub fn squeeze_line_starts(text: &str) -> String {
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
    out
}

pub fn build_cynefin(ast: CynefinAst, title: Option<&str>) -> CynefinModel {
    let mut domains: Vec<CynefinDomain> = Vec::new();
    // 同じ領域の後の塊が前のものに代わる。
    for block in ast.domains {
        let domain = CynefinDomain {
            name: block.domain,
            items: block.items,
        };
        match domains.iter_mut().find(|d| d.name == domain.name) {
            Some(slot) => *slot = domain,
            None => domains.push(domain),
        }
    }
    let transitions = ast
        .transitions
        .into_iter()
        .filter(|t| t.from != t.to)
        .map(|t| CynefinTransition {
            from: t.from,
            to: t.to,
            label: t.label.filter(|l| !l.is_empty()),
        })
        .collect();
    CynefinModel {
        title: ast
            .title
            .filter(|t| !t.is_empty())
            .or(title.map(str::to_string)),
        acc_title: ast
            .acc_title
            .filter(|t| !t.is_empty())
            .map(|t| js::trim_start(&t).to_string()),
        acc_descr: ast
            .acc_descr
            .filter(|d| !d.is_empty())
            .map(|d| squeeze_line_starts(&d)),
        domains,
        transitions,
    }
}
