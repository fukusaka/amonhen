//! state の読みの置き場（pele の `src/diagrams/state/db.ts`）。Mermaid が `stateDiagram` にも `stateDiagram-v2` にも
//! 使う、版 2 の StateDB。`[*]` に番号をつける古い版は移していない。

use std::collections::HashMap;

use super::graph::{StateGraph, build_state_graph};
use super::types::{StateClassDef, StateLink, StateStmt, Stmt};
use crate::errors::Result;
use crate::js;

/// 状態の表の 1 つ。描くのに要るものだけ。
#[derive(Clone, Debug, Default)]
pub struct DbState {
    pub classes: Vec<String>,
    pub styles: Vec<String>,
}

#[derive(Default)]
pub struct StateDb {
    /// 状態の文（文書はこの添字で指す）。
    pub items: Vec<StateStmt>,
    pub root_doc: Vec<Stmt>,
    pub states: HashMap<String, DbState>,
    pub classes: HashMap<String, StateClassDef>,
    pub links: HashMap<String, StateLink>,
    pub graph: StateGraph,
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    divider_count: usize,
}

/// 文書の向き: その最後の `direction` の文。
pub fn doc_direction(doc: &[Stmt]) -> String {
    let mut dir = "TB".to_string();
    for item in doc {
        if let Stmt::Dir(v) = item {
            dir = v.clone();
        }
    }
    dir
}

enum Pending {
    Root,
    Item(usize),
}

impl StateDb {
    pub fn new() -> Self {
        Self::default()
    }

    /// 状態の文を置き、その添字を返す。
    pub fn item(&mut self, stmt: StateStmt) -> usize {
        self.items.push(stmt);
        self.items.len() - 1
    }

    /// 根の文書を流す向き: その最後の `direction` の文。
    pub fn direction(&self) -> String {
        doc_direction(&self.root_doc)
    }

    pub fn set_root_doc(&mut self, doc: Vec<Stmt>) -> Result<()> {
        self.root_doc = doc;
        self.translate();
        self.extract()
    }

    /// 根の文書の文だけが、ここで状態・関係・クラスになる。合成の状態の中の文は、グラフを作るときに読む。
    fn extract(&mut self) -> Result<()> {
        self.states = HashMap::new();
        self.classes = HashMap::new();
        let doc = std::mem::take(&mut self.root_doc);
        for item in &doc {
            match item {
                Stmt::State(k) => {
                    let id = js::trim(&self.items[*k].id).to_string();
                    self.add_state(&id, None);
                }
                Stmt::Relation { state1, state2, .. } => {
                    for k in [*state1, *state2] {
                        let id = self.items[k].id.clone();
                        let classes = self.items[k].classes.clone();
                        self.add_state(&id, classes.as_deref());
                    }
                }
                Stmt::ClassDef { id, classes } => {
                    self.add_style_class(js::trim(id), classes);
                }
                Stmt::Style { id, style_class } => {
                    let styles: Vec<String> = style_class
                        .split(',')
                        .map(|s| js::trim(&s.replace(';', "")).to_string())
                        .collect();
                    for key in self.named(js::trim(id)) {
                        self.states.get_mut(&key).unwrap().styles = styles.clone();
                    }
                }
                Stmt::ApplyClass { id, style_class } => {
                    self.set_css_class(js::trim(id), style_class);
                }
                Stmt::Click { id, url, tooltip } => {
                    let id = self.items[*id].id.clone();
                    self.links.insert(
                        id,
                        StateLink {
                            url: url.clone(),
                            tooltip: tooltip.clone(),
                        },
                    );
                }
                _ => {}
            }
        }
        self.root_doc = doc;
        self.graph = build_state_graph(&self.root_doc, &self.items, &self.states, &self.classes)?;
        Ok(())
    }

    /// Mermaid の docTranslator: `[*]` に親で決まる id をつけ、`--` で分けた文書を部分ごとの領域にする。
    fn translate(&mut self) {
        let mut pending: Vec<(String, Pending, bool)> = vec![("root".into(), Pending::Root, true)];
        while let Some((parent_id, node, first)) = pending.pop() {
            let (node_id, doc) = match node {
                Pending::Root => ("root".to_string(), std::mem::take(&mut self.root_doc)),
                Pending::Item(k) => {
                    let item = &mut self.items[k];
                    if item.id == "[*]" {
                        item.id = format!("{parent_id}{}", if first { "_start" } else { "_end" });
                        item.start = Some(first);
                    } else {
                        item.id = js::trim(&item.id).to_string();
                    }
                    let Some(doc) = item.doc.take() else {
                        continue;
                    };
                    (item.id.clone(), doc)
                }
            };

            let mut regions: Vec<Stmt> = Vec::new();
            let mut current: Vec<Stmt> = Vec::new();
            for item in doc {
                let split = match &item {
                    Stmt::State(k) => {
                        let s = &self.items[*k];
                        s.type_.as_deref() == Some("divider") && s.doc.is_none()
                    }
                    _ => false,
                };
                if split {
                    let Stmt::State(k) = item else { unreachable!() };
                    let mut region = self.items[k].clone();
                    region.doc = Some(std::mem::take(&mut current));
                    let r = self.item(region);
                    regions.push(Stmt::State(r));
                } else {
                    current.push(item);
                }
            }
            // Mermaid は最後の区切りの後の領域にでたらめの id をつける。ここでは区切りの番号を続ける。Mermaid は区切りで
            // 終わる文書をまとめないまま描けずに終わるが、ここではその区切りの前の領域を残す。
            let doc = if regions.is_empty() {
                current
            } else {
                if !current.is_empty() {
                    let id = self.get_divider_id();
                    let r = self.item(StateStmt {
                        id,
                        type_: Some("divider".into()),
                        doc: Some(current),
                        ..Default::default()
                    });
                    regions.push(Stmt::State(r));
                }
                regions
            };
            for item in doc.iter().rev() {
                match item {
                    Stmt::State(k) => pending.push((node_id.clone(), Pending::Item(*k), true)),
                    Stmt::Relation { state1, state2, .. } => {
                        pending.push((node_id.clone(), Pending::Item(*state2), false));
                        pending.push((node_id.clone(), Pending::Item(*state1), true));
                    }
                    _ => {}
                }
            }
            match node {
                Pending::Root => self.root_doc = doc,
                Pending::Item(k) => self.items[k].doc = Some(doc),
            }
        }
    }

    fn add_state(&mut self, id: &str, classes: Option<&[String]>) {
        let trimmed = js::trim(id).to_string();
        self.states.entry(trimmed.clone()).or_default();
        if let Some(classes) = classes {
            for name in classes {
                self.set_css_class(&trimmed, js::trim(name));
            }
        }
    }

    pub fn get_divider_id(&mut self) -> String {
        self.divider_count += 1;
        format!("divider-id-{}", self.divider_count)
    }

    fn add_style_class(&mut self, id: &str, style_attributes: &str) {
        let found = self
            .classes
            .entry(id.to_string())
            .or_insert_with(|| StateClassDef {
                id: id.to_string(),
                styles: Vec::new(),
                text_styles: Vec::new(),
            });
        if style_attributes.is_empty() {
            return;
        }
        for attrib in style_attributes.split(',') {
            let fixed = js::trim(&attrib.replacen(';', "", 1)).to_string();
            if attrib.contains("color") {
                found.text_styles.push(
                    fixed
                        .replacen("fill", "bgFill", 1)
                        .replacen("color", "fill", 1),
                );
            }
            found.styles.push(fixed);
        }
    }

    fn set_css_class(&mut self, item_ids: &str, css_class: &str) {
        for key in self.named(item_ids) {
            self.states
                .get_mut(&key)
                .unwrap()
                .classes
                .push(css_class.to_string());
        }
    }

    pub fn set_acc_title(&mut self, text: &str) {
        self.acc_title = Some(js::trim_start(text).to_string());
    }

    pub fn set_acc_description(&mut self, text: &str) {
        self.acc_descr = Some(collapse_indent(text));
    }

    /// カンマで区切った id の状態。なければ作る。Mermaid と同じく、id は書いたとおりに探し、作るときだけ削る。
    fn named(&mut self, ids: &str) -> Vec<String> {
        let mut out = Vec::new();
        for id in ids.split(',') {
            if self.states.contains_key(id) {
                out.push(id.to_string());
                continue;
            }
            self.add_state(id, None);
            let trimmed = js::trim(id);
            if self.states.contains_key(trimmed) {
                out.push(trimmed.to_string());
            }
        }
        out
    }
}

/// `text.replace(/\n\s+/g, '\n')`
fn collapse_indent(text: &str) -> String {
    let mut out = String::new();
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
