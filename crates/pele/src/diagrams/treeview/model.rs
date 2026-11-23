//! treeView の形（pele の `src/diagrams/treeview/model.ts`）。

use std::collections::HashMap;

use super::box_drawing::{line_start, preprocess_box_drawing, remap_error_lines};
use super::parser::{TreeViewAst, parse_tree_view};
use crate::errors::{PeleError, Result, locate};
use crate::js;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeType {
    File,
    Directory,
}

#[derive(Clone, Debug)]
pub struct TreeNode {
    pub id: usize,
    pub level: i64,
    pub name: String,
    pub node_type: NodeType,
    /// 書いたとおりのアイコンの名前（`pack:name`・`file`・`folder`）か、アイコンを隠す `none`。
    pub icon: Option<String>,
    pub css_class: Option<String>,
    pub description: Option<String>,
    /// 子の番号（`TreeViewModel::nodes` の位置）。
    pub children: Vec<usize>,
}

pub struct TreeViewModel {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    /// ノード。0 は根（段 -1 の `/` という名のディレクトリで、上の段の項目を持つ）。
    pub nodes: Vec<TreeNode>,
    pub count: usize,
}

/// Mermaid の treeView のデータベース: ノードは書いた順に、字下げとともに来る。
pub struct TreeViewDb {
    pub count: usize,
    pub nodes: Vec<TreeNode>,
    stack: Vec<usize>,
}

impl TreeViewDb {
    pub fn new() -> Self {
        TreeViewDb {
            count: 1,
            nodes: vec![TreeNode {
                id: 0,
                level: -1,
                name: "/".to_string(),
                node_type: NodeType::Directory,
                icon: None,
                css_class: None,
                description: None,
                children: Vec::new(),
            }],
            stack: vec![0],
        }
    }

    pub fn add_node(
        &mut self,
        level: i64,
        name: String,
        node_type: NodeType,
        css_class: Option<String>,
        icon: Option<String>,
        description: Option<String>,
    ) {
        while level <= self.nodes[*self.stack.last().unwrap()].level {
            self.stack.pop();
        }
        let index = self.nodes.len();
        self.nodes.push(TreeNode {
            id: self.count,
            level,
            name,
            node_type,
            icon,
            css_class,
            description,
            children: Vec::new(),
        });
        self.count += 1;
        let parent = *self.stack.last().unwrap();
        self.nodes[parent].children.push(index);
        self.stack.push(index);
    }
}

impl Default for TreeViewDb {
    fn default() -> Self {
        Self::new()
    }
}

fn non_empty(s: Option<String>) -> Option<String> {
    s.filter(|s| !s.is_empty())
}

pub fn populate(ast: TreeViewAst, db: &mut TreeViewDb) {
    for node in ast.nodes {
        let level = node.indent.unwrap_or(0) as i64;
        let mut name = node.name;
        let is_directory = name.ends_with('/');
        if is_directory {
            name.pop();
        }
        db.add_node(
            level,
            name,
            if is_directory {
                NodeType::Directory
            } else {
                NodeType::File
            },
            non_empty(node.class_annotation),
            node.icon_annotation
                .map(|i| if i.is_empty() { "none".to_string() } else { i }),
            non_empty(node.desc_annotation),
        );
    }
}

/// 文字を構文の木に読む。罫線の書き方も受ける。失敗は元の文字の行で知らせる。
pub fn read_tree_view(source: &str) -> Result<TreeViewAst> {
    let pre = preprocess_box_drawing(source)?;
    let error = match parse_tree_view(&pre.text) {
        Ok(ast) => return Ok(ast),
        Err(e) => e,
    };
    // 読み手は、変えていない文字では位置をもう示している。
    if pre.text == source {
        return Err(error);
    }
    let Some(&line) = pre.line_map.get(&error.line) else {
        return Err(error);
    };
    let at = if error.column > 0 {
        match pre.columns.get(&error.line) {
            Some(f) => f(error.column - 1),
            None => error.column - 1,
        }
    } else {
        0
    };
    let src = js::utf16(source);
    let written = locate(&src, line_start(&src, line) + at);
    let head = error.message.find('\n').unwrap_or(error.message.len());
    let rest = &error.message[head..];
    let rest = if error.snippet.is_empty() {
        rest.to_string()
    } else {
        rest.replacen(&error.snippet, &written.snippet, 1)
    };
    let message = remap_error_lines(
        &error.message[..head],
        &HashMap::from([(error.line, written.line)]),
    ) + &rest;
    Err(PeleError {
        message,
        code: error.code,
        type_: error.type_,
        mermaid_type: None,
        line: written.line,
        column: if error.column > 0 { written.column } else { 0 },
        snippet: if error.snippet.is_empty() {
            String::new()
        } else {
            written.snippet
        },
    })
}

pub fn build_tree_view(ast: TreeViewAst, title: Option<&str>) -> TreeViewModel {
    let mut db = TreeViewDb::new();
    let ast_title = ast.title.clone().filter(|t| !t.is_empty());
    let acc_title = non_empty(ast.acc_title.clone()).map(|t| js::trim_start(&t).to_string());
    let acc_descr = non_empty(ast.acc_descr.clone()).map(|d| {
        // `/\n\s+/g` を `\n` に。
        let mut out = String::with_capacity(d.len());
        let mut chars = d.chars().peekable();
        while let Some(c) = chars.next() {
            out.push(c);
            if c == '\n' {
                while chars.peek().is_some_and(|&c| js::is_js_space(c)) {
                    chars.next();
                }
            }
        }
        out
    });
    populate(ast, &mut db);
    TreeViewModel {
        title: ast_title.or(title.map(str::to_string)),
        acc_title,
        acc_descr,
        count: db.count,
        nodes: db.nodes,
    }
}
