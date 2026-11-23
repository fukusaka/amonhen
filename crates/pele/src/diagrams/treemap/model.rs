//! treemap の形（pele の `src/diagrams/treemap/model.ts`）。

use super::parser::{ItemAst, RowAst, TreemapAst};
use crate::diagrams::cynefin::model::squeeze_line_starts;
use crate::js;

#[derive(Clone, Debug)]
pub struct TreemapNode {
    pub name: String,
    /// 節は（空かもしれない）子の並び（`TreemapModel::nodes` の位置）を持つ。葉は持たない。
    pub children: Option<Vec<usize>>,
    pub value: Option<f64>,
    pub class_selector: Option<String>,
    pub css_compiled_styles: Option<Vec<String>>,
}

pub struct TreemapItem {
    pub level: usize,
    pub name: String,
    pub leaf: bool,
    pub value: Option<f64>,
    pub class_selector: Option<String>,
    pub css_compiled_styles: Option<Vec<String>>,
}

pub struct TreemapModel {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub nodes: Vec<TreemapNode>,
    pub roots: Vec<usize>,
    pub classes: Vec<(String, Vec<String>)>,
}

/// 行は、それより浅く字下げした、上のいちばん近い節に入る。
pub fn build_hierarchy(items: Vec<TreemapItem>) -> (Vec<TreemapNode>, Vec<usize>) {
    let mut nodes: Vec<TreemapNode> = Vec::new();
    let mut roots = Vec::new();
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for item in items {
        let index = nodes.len();
        nodes.push(TreemapNode {
            name: item.name,
            children: if item.leaf { None } else { Some(Vec::new()) },
            value: if item.leaf { item.value } else { None },
            class_selector: item.class_selector,
            css_compiled_styles: item.css_compiled_styles,
        });
        while stack.last().is_some_and(|&(_, level)| level >= item.level) {
            stack.pop();
        }
        match stack.last() {
            None => roots.push(index),
            Some(&(parent, _)) => nodes[parent].children.as_mut().unwrap().push(index),
        }
        if !item.leaf {
            stack.push((index, item.level));
        }
    }
    (nodes, roots)
}

pub fn build_treemap(ast: TreemapAst, title: Option<&str>) -> TreemapModel {
    let mut classes: Vec<(String, Vec<String>)> = Vec::new();
    for row in &ast.rows {
        let RowAst::ClassDef {
            class_name,
            style_text,
            ..
        } = row
        else {
            continue;
        };
        let k = match classes.iter().position(|(n, _)| n == class_name) {
            Some(k) => k,
            None => {
                classes.push((class_name.clone(), Vec::new()));
                classes.len() - 1
            }
        };
        // 逃がしたカンマは値に残り、ほかのカンマは宣言を分ける。
        let text = style_text
            .as_deref()
            .unwrap_or("")
            .replace("\\,", "§§§")
            .replace(',', ";")
            .replace("§§§", ",");
        for style in text.split(';') {
            classes[k].1.push(style.to_string());
        }
    }

    let mut items = Vec::new();
    for row in ast.rows {
        let RowAst::Row { indent, item } = row else {
            continue;
        };
        let (name, leaf, value, class_selector) = match item {
            ItemAst::Leaf {
                name,
                value,
                class_selector,
            } => (name, true, Some(value), class_selector),
            ItemAst::Section {
                name,
                class_selector,
            } => (name, false, None, class_selector),
        };
        let styles = class_selector
            .as_ref()
            .filter(|s| !s.is_empty())
            .and_then(|s| classes.iter().find(|(n, _)| n == s))
            .map(|(_, styles)| styles)
            .filter(|s| !s.is_empty())
            .cloned();
        items.push(TreemapItem {
            level: indent.unwrap_or(0),
            name,
            leaf,
            value,
            class_selector,
            css_compiled_styles: styles,
        });
    }

    let (nodes, roots) = build_hierarchy(items);
    TreemapModel {
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
        nodes,
        roots,
        classes,
    }
}
