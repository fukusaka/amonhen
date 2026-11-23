//! block の中身（pele の `src/diagrams/block/db.ts`）。

use std::collections::{HashMap, HashSet};

use super::types::{Block, BlockClassDef, Statement};
use crate::errors::{ErrorCode, PeleError, Result};
use crate::js;

const MAX_SPACES: f64 = 100000.0;

pub fn type_str_to_type(type_str: Option<&str>) -> &'static str {
    match type_str {
        Some("[]") => "square",
        Some("()") => "round",
        Some("(())") => "circle",
        Some(">]") => "rect_left_inv_arrow",
        Some("{}") => "diamond",
        Some("{{}}") => "hexagon",
        Some("([])") => "stadium",
        Some("[[]]") => "subroutine",
        Some("[()]") => "cylinder",
        Some("((()))") => "doublecircle",
        Some("[//]") => "lean_right",
        Some("[\\\\]") => "lean_left",
        Some("[/\\]") => "trapezoid",
        Some("[\\/]") => "inv_trapezoid",
        Some("<[]>") => "block_arrow",
        _ => "na",
    }
}

pub fn edge_str_to_edge_data(type_str: &str) -> &'static str {
    match js::trim(type_str).chars().last() {
        Some('x') => "arrow_cross",
        Some('o') => "arrow_circle",
        Some('>') => "arrow_point",
        _ => "",
    }
}

pub fn edge_str_to_edge_start_data(type_str: &str) -> &'static str {
    match js::trim(type_str).chars().next() {
        Some('x') => "arrow_cross",
        Some('o') => "arrow_circle",
        Some('<') => "arrow_point",
        _ => "arrow_open",
    }
}

pub fn edge_str_to_thickness(type_str: &str) -> &'static str {
    if type_str.contains("==") {
        "thick"
    } else {
        "normal"
    }
}

pub fn edge_str_to_pattern(type_str: &str) -> &'static str {
    if type_str.contains(".-") {
        "dotted"
    } else {
        "solid"
    }
}

struct Frame {
    list: Vec<Block>,
    at: usize,
    parent: usize,
    children: Vec<usize>,
}

fn frame(statements: Vec<Statement>, parent: usize) -> Frame {
    let mut list = Vec::new();
    for s in statements {
        match s {
            Statement::One(b) => list.push(*b),
            Statement::Many(v) => list.extend(v),
        }
    }
    Frame {
        list,
        at: 0,
        parent,
        children: Vec::new(),
    }
}

pub struct BlockDb {
    pub title: Option<String>,
    /// すべてのブロック。0 が根。
    pub arena: Vec<Block>,
    /// 根の子。
    pub blocks: Vec<usize>,
    pub edges: Vec<usize>,
    pub classes: HashMap<String, BlockClassDef>,
    /// Mermaid は根もここに入れるので、`root` という名前のブロックが消える。ここでは入れない。
    pub by_id: HashMap<String, usize>,
    /// ブロックより先にクラスやスタイルの文が名指した名前。
    pending: HashSet<String>,
    edge_count: HashMap<String, usize>,
    ids: usize,
    colors: usize,
    spaces: f64,
}

impl Default for BlockDb {
    fn default() -> Self {
        Self::new()
    }
}

impl BlockDb {
    pub fn new() -> Self {
        BlockDb {
            title: None,
            arena: vec![Block {
                id: Some("root".into()),
                type_: Some("composite".into()),
                columns: Some(-1.0),
                ..Default::default()
            }],
            blocks: Vec::new(),
            edges: Vec::new(),
            classes: HashMap::new(),
            by_id: HashMap::new(),
            pending: HashSet::new(),
            edge_count: HashMap::new(),
            ids: 0,
            colors: 0,
            spaces: 0.0,
        }
    }

    pub fn root(&self) -> &Block {
        &self.arena[0]
    }

    /// Mermaid の名前は乱数。ここでは数えるので、同じ文はいつも同じに描く。
    pub fn generate_id(&mut self) -> String {
        self.ids += 1;
        format!("id-{}", self.ids)
    }

    pub fn add_style_class(&mut self, id: &str, style_attributes: &str) {
        let found = self
            .classes
            .entry(id.to_string())
            .or_insert_with(|| BlockClassDef {
                id: id.to_string(),
                styles: Vec::new(),
                text_styles: Vec::new(),
            });
        for attribute in style_attributes.split(',') {
            let fixed = match attribute.find(';') {
                None => js::trim(attribute).to_string(),
                Some(semi) => {
                    js::trim(&format!("{}{}", &attribute[..semi], &attribute[semi + 1..]))
                        .to_string()
                }
            };
            if attribute.contains("color") {
                found.text_styles.push(
                    fixed
                        .replacen("fill", "bgFill", 1)
                        .replacen("color", "fill", 1),
                );
            }
            found.styles.push(fixed);
        }
    }

    pub fn add_style_to_node(&mut self, id: &str, styles: &str) {
        let b = self.named(id);
        self.arena[b].styles = Some(styles.split(',').map(String::from).collect());
    }

    /// Mermaid は名前を整える前に探すので、`class a, b hot` は b に当たらない。ここでは先に整える。
    pub fn set_css_class(&mut self, item_ids: &str, css_class_name: &str) {
        for id in item_ids.split(',') {
            let b = self.named(js::trim(id));
            self.arena[b]
                .classes
                .get_or_insert_with(Vec::new)
                .push(css_class_name.to_string());
        }
    }

    /// Mermaid は知らない名前へのスタイルで止まり、クラスはその名前のブロックを後で消す。ここではどちらもブロックを待つ。
    fn named(&mut self, id: &str) -> usize {
        if let Some(&b) = self.by_id.get(id) {
            return b;
        }
        let b = self.arena.len();
        self.arena.push(Block {
            id: Some(id.to_string()),
            type_: Some("na".into()),
            ..Default::default()
        });
        self.by_id.insert(id.to_string(), b);
        self.pending.insert(id.to_string());
        b
    }

    pub fn set_hierarchy(&mut self, statements: Vec<Statement>) -> Result<()> {
        let mut stack = vec![frame(statements, 0)];
        while let Some(f) = stack.last_mut() {
            if f.at == f.list.len() {
                let f = stack.pop().unwrap();
                self.arena[f.parent].children = f.children;
                continue;
            }
            let mut block = std::mem::take(&mut f.list[f.at]);
            f.at += 1;
            let parent = f.parent;
            match block.type_.as_deref() {
                Some("classDef") => {
                    let id = block.id.unwrap_or_default();
                    self.add_style_class(&id, block.css.as_deref().unwrap_or(""));
                    continue;
                }
                Some("applyClass") => {
                    let id = block.id.unwrap_or_default();
                    self.set_css_class(&id, block.style_class.as_deref().unwrap_or(""));
                    continue;
                }
                Some("applyStyles") => {
                    if let Some(s) = block.styles_str.as_deref().filter(|s| !s.is_empty()) {
                        let id = block.id.unwrap_or_default();
                        self.add_style_to_node(&id, s);
                    }
                    continue;
                }
                Some("column-setting") => {
                    self.arena[parent].columns = Some(block.columns.unwrap_or(-1.0));
                    continue;
                }
                Some("edge") => {
                    let id = block.id.clone().unwrap_or_default();
                    let count = self.edge_count.get(&id).copied().unwrap_or(0) + 1;
                    self.edge_count.insert(id.clone(), count);
                    block.id = Some(format!("{count}-{id}"));
                    self.edges.push(self.arena.len());
                    self.arena.push(block);
                    continue;
                }
                _ => {}
            }

            // 見出しがリンクの文だったまとまりには自分の名前がない。
            if block.id.is_none() {
                block.id = Some(self.generate_id());
            }
            let id = block.id.clone().unwrap();
            if block.label.as_deref().is_none_or(str::is_empty) {
                block.label = Some(if block.is("composite") {
                    String::new()
                } else {
                    id.clone()
                });
            }
            let known = self.by_id.get(&id).copied();
            let fresh = known.is_none() || self.pending.remove(&id);
            let inner = block.inner.take();
            block.children = Vec::new();
            let b = self.arena.len();
            if !fresh {
                let k = known.unwrap();
                if !block.is("na") {
                    self.arena[k].type_ = block.type_.clone();
                }
                if block.label.as_deref() != Some(id.as_str()) {
                    self.arena[k].label = block.label.clone();
                }
                self.arena.push(block);
            } else {
                if let Some(k) = known {
                    block.classes = self.arena[k].classes.clone();
                    block.styles = self.arena[k].styles.clone();
                }
                if block.is("composite") {
                    block.color_index = Some(self.colors);
                    self.colors += 1;
                }
                self.arena.push(block);
                self.by_id.insert(id.clone(), b);
            }

            if self.arena[b].is("space") {
                let width = self.arena[b].width.unwrap_or(1.0);
                // `!(width <= MAX_SPACES - spaces)`（NaN も止める）。
                if width.is_nan() || width > MAX_SPACES - self.spaces {
                    return Err(PeleError::typed(
                        format!(
                            "A block diagram can hold at most {} space blocks.",
                            js::fmt(MAX_SPACES)
                        ),
                        ErrorCode::Limit,
                        "block",
                    ));
                }
                self.spaces += js::max(width, 0.0);
                let mut j = 0.0;
                while j < width {
                    let mut copy = self.arena[b].clone();
                    copy.id = Some(format!("{id}-{}", js::fmt(j)));
                    copy.children = Vec::new();
                    let c = self.arena.len();
                    self.by_id.insert(copy.id.clone().unwrap(), c);
                    self.arena.push(copy);
                    stack.last_mut().unwrap().children.push(c);
                    j += 1.0;
                }
            } else if fresh {
                stack.last_mut().unwrap().children.push(b);
            }
            if let Some(inner) = inner.filter(|v| !v.is_empty()) {
                stack.push(frame(inner, b));
            }
        }
        self.blocks = self.arena[0].children.clone();
        Ok(())
    }
}
