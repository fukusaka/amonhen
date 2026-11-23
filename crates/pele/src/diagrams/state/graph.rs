//! 描くノードと辺（pele の `src/diagrams/state/graph.ts`）。

use std::collections::HashMap;

use super::db::{DbState, doc_direction};
use super::types::{Desc, StateClassDef, StateStmt, Stmt};
use crate::errors::{ErrorCode, PeleError, Result};
use crate::js;

#[derive(Clone, Debug, Default)]
pub struct StateNode {
    pub id: String,
    /// rect・rectWithTitle・stateStart・stateEnd・choice・fork・join・divider・roundedWithTitle・note・noteGroup。
    pub shape: Option<String>,
    pub label: Option<String>,
    pub description: Vec<String>,
    pub css_classes: String,
    pub css_compiled_styles: Vec<String>,
    pub css_styles: Vec<String>,
    pub type_: Option<String>,
    pub is_group: bool,
    pub parent_id: Option<String>,
    /// 合成の状態の中の向き: その最後の `direction` の文。なければ TB。
    pub dir: Option<String>,
    pub position: Option<String>,
    pub note_for: Option<String>,
    /// TS のノードに `position` の鍵があるか（ノートとそのまとまり）。
    aside: bool,
    /// ラベルと説明を、id の下書きの最後の形から取る。同じ id の後の訪れは前のノードを丸ごと上書きし、下書きは
    /// その id を訪れるときにしか変わらないので、最後の形がそのノードを作ったときの形。説明の多い状態で毎回写すと
    /// 2 乗になる。
    from_draft: bool,
}

/// ラベルは Mermaid と同じく、どれも markdown。
#[derive(Clone, Debug)]
pub struct StateEdge {
    pub id: String,
    pub start: String,
    pub end: String,
    pub label: String,
    pub dashed: bool,
}

#[derive(Clone, Debug, Default)]
pub struct StateGraph {
    pub nodes: Vec<StateNode>,
    pub edges: Vec<StateEdge>,
}

struct Draft {
    shape: Option<String>,
    description: Desc,
    css_classes: String,
    css_styles: Vec<String>,
    type_: Option<String>,
    dir: Option<String>,
}

fn desc_len(d: &Desc) -> usize {
    match d {
        Desc::One(s) => js::len16(s),
        Desc::Many(v) => v.len(),
    }
}

/// `[d].flat()`
fn flat(d: &Desc) -> Vec<String> {
    match d {
        Desc::One(s) => vec![s.clone()],
        Desc::Many(v) => v.clone(),
    }
}

struct Builder<'a> {
    states: &'a HashMap<String, DbState>,
    classes: &'a HashMap<String, StateClassDef>,
    nodes: Vec<StateNode>,
    edges: Vec<StateEdge>,
    by_id: HashMap<String, usize>,
    drafts: HashMap<String, Draft>,
    counter: usize,
}

impl Builder<'_> {
    fn insert(&mut self, mut node: StateNode) {
        // 2 つの決まった id は、Mermaid で DOMPurify が `<<fork>>` と `<<choice>>` から残すもの。同じにするために残す。
        if node.id.is_empty() || node.id == "</join></fork>" || node.id == "</choice>" {
            return;
        }
        if !node.css_classes.is_empty() {
            // クラスがいくつあっても 1 つの写し: class の文で何度も名を挙げた状態は長い並びを持つ。
            let mut compiled: Option<Vec<String>> = None;
            for name in node.css_classes.split(' ') {
                let Some(def) = self.classes.get(name) else {
                    continue;
                };
                compiled
                    .get_or_insert_with(|| node.css_compiled_styles.clone())
                    .extend(def.styles.iter().cloned());
            }
            if let Some(c) = compiled {
                node.css_compiled_styles = c;
            }
        }
        match self.by_id.get(&node.id) {
            Some(&k) => {
                let existing = &mut self.nodes[k];
                let parent_id = node.parent_id.clone().or(existing.parent_id.clone());
                let position = if node.aside {
                    node.position.clone()
                } else {
                    existing.position.clone()
                };
                let aside = node.aside || existing.aside;
                let note_for = node.note_for.clone().or(existing.note_for.clone());
                *existing = node;
                existing.parent_id = parent_id;
                existing.position = position;
                existing.aside = aside;
                existing.note_for = note_for;
            }
            None => {
                self.by_id.insert(node.id.clone(), self.nodes.len());
                self.nodes.push(node);
            }
        }
    }

    fn visit(&mut self, parent: Option<&str>, item: &StateStmt, alt: bool) {
        let id = item.id.clone();
        if id == "root" {
            return;
        }
        let known = self.states.get(&id);
        let class_str = known.map_or(String::new(), |k| k.classes.join(" "));
        let styles = known.map_or(Vec::new(), |k| k.styles.clone());

        let draft = self.drafts.entry(id.clone()).or_insert_with(|| {
            let mut shape = Some("rect".to_string());
            if item.start == Some(true) {
                shape = Some("stateStart".into());
            } else if item.start == Some(false) {
                shape = Some("stateEnd".into());
            }
            if item.type_.as_deref() != Some("default") {
                shape = item.type_.clone();
            }
            Draft {
                shape,
                description: Desc::One(id.clone()),
                css_classes: format!("{class_str} statediagram-state"),
                css_styles: styles.clone(),
                type_: None,
                dir: None,
            }
        });

        if let Some(d) = item.description.as_ref().filter(|d| d.truthy()) {
            match &mut draft.description {
                Desc::Many(v) => {
                    draft.shape = Some("rectWithTitle".into());
                    v.extend(flat(d));
                }
                Desc::One(s) if !s.is_empty() => {
                    draft.shape = Some("rectWithTitle".into());
                    draft.description = Desc::Many(if *s == id {
                        flat(d)
                    } else {
                        let mut v = vec![s.clone()];
                        v.extend(flat(d));
                        v
                    });
                }
                Desc::One(_) => {
                    draft.shape = Some("rect".into());
                    draft.description = d.clone();
                }
            }
        }
        if desc_len(&draft.description) == 1 && draft.shape.as_deref() == Some("rectWithTitle") {
            draft.shape = Some(
                if draft.type_.as_deref() == Some("group") {
                    "roundedWithTitle"
                } else {
                    "rect"
                }
                .into(),
            );
        }

        if draft.type_.is_none()
            && let Some(doc) = &item.doc
        {
            draft.type_ = Some("group".into());
            draft.dir = Some(doc_direction(doc));
            draft.shape = Some(
                if item.type_.as_deref() == Some("divider") {
                    "divider"
                } else {
                    "roundedWithTitle"
                }
                .into(),
            );
            draft.css_classes = format!(
                "{} statediagram-cluster {}",
                draft.css_classes,
                if alt { "statediagram-cluster-alt" } else { "" }
            );
        }

        let mut node = StateNode {
            id: id.clone(),
            shape: draft.shape.clone(),
            from_draft: true,
            css_classes: draft.css_classes.clone(),
            css_compiled_styles: Vec::new(),
            css_styles: draft.css_styles.clone(),
            type_: draft.type_.clone(),
            is_group: draft.type_.as_deref() == Some("group"),
            dir: draft.dir.clone(),
            ..Default::default()
        };
        if let Some(p) = parent.filter(|p| *p != "root") {
            node.parent_id = Some(p.to_string());
        }
        let draft_classes = draft.css_classes.clone();

        let Some(note) = &item.note else {
            self.insert(node);
            return;
        };
        // Mermaid は各ノートを、状態の名の自分だけのまとまりに入れる。pele はそのまとまりを形に残し、ノートは
        // 状態の横に置く。
        let aside =
            |aside_id: String, shape: &str, css_classes: String, is_group: bool| StateNode {
                id: aside_id,
                shape: Some(shape.into()),
                label: Some(note.text.clone()),
                css_classes,
                type_: Some(if is_group { "group" } else { "node" }.into()),
                is_group,
                position: note.position.clone(),
                aside: true,
                ..Default::default()
            };
        let mut note_node = aside(
            format!("{id}----note-{}", self.counter),
            "note",
            "statediagram-note".into(),
            false,
        );
        self.counter += 1;
        let group_id = format!("{id}----parent");
        note_node.parent_id = Some(group_id.clone());
        note_node.note_for = Some(id.clone());
        let note_id = note_node.id.clone();
        self.insert(aside(group_id, "noteGroup", draft_classes, true));
        self.insert(note_node);
        self.insert(node);
        let left = note.position.as_deref() == Some("left of");
        let (from, to) = if left { (note_id, id) } else { (id, note_id) };
        self.edges.push(StateEdge {
            id: format!("{from}-{to}"),
            start: from,
            end: to,
            label: String::new(),
            dashed: true,
        });
    }
}

enum DocRef {
    Root,
    Item(usize),
}

struct Frame {
    item: Option<usize>,
    doc: DocRef,
    at: usize,
    alt: bool,
}

/// Mermaid の dataFetcher: 文書をたどり、描くノードと辺にする。
pub fn build_state_graph(
    root_doc: &[Stmt],
    items: &[StateStmt],
    states: &HashMap<String, DbState>,
    classes: &HashMap<String, StateClassDef>,
) -> Result<StateGraph> {
    let mut b = Builder {
        states,
        classes,
        nodes: Vec::new(),
        edges: Vec::new(),
        by_id: HashMap::new(),
        drafts: HashMap::new(),
        counter: 0,
    };
    let mut stack = vec![Frame {
        item: None,
        doc: DocRef::Root,
        at: 0,
        alt: true,
    }];
    while let Some(frame) = stack.last_mut() {
        let doc: &[Stmt] = match frame.doc {
            DocRef::Root => root_doc,
            DocRef::Item(k) => items[k].doc.as_deref().unwrap_or(&[]),
        };
        if frame.at >= doc.len() {
            stack.pop();
            continue;
        }
        let item = &doc[frame.at];
        frame.at += 1;
        let (parent, alt) = (frame.item, frame.alt);
        let parent_id = parent.map(|k| items[k].id.clone());
        match item {
            Stmt::State(k) => {
                b.visit(parent_id.as_deref(), &items[*k], alt);
                if items[*k].doc.is_some() {
                    stack.push(Frame {
                        item: Some(*k),
                        doc: DocRef::Item(*k),
                        at: 0,
                        alt: !alt,
                    });
                }
            }
            Stmt::Relation {
                state1,
                state2,
                description,
            } => {
                b.visit(parent_id.as_deref(), &items[*state1], alt);
                b.visit(parent_id.as_deref(), &items[*state2], alt);
                b.edges.push(StateEdge {
                    id: format!("edge{}", b.counter),
                    start: items[*state1].id.clone(),
                    end: items[*state2].id.clone(),
                    label: description.clone().unwrap_or_default(),
                    dashed: false,
                });
                b.counter += 1;
            }
            _ => {}
        }
    }

    for node in &mut b.nodes {
        if !node.from_draft {
            continue;
        }
        let draft = &b.drafts[&node.id];
        (node.label, node.description) = match &draft.description {
            Desc::Many(v) => (v.first().cloned(), v.iter().skip(1).cloned().collect()),
            Desc::One(s) => (Some(s.clone()), Vec::new()),
        };
        if node.shape.as_deref() == Some("divider") {
            node.label = Some(String::new());
        }
    }
    for node in &b.nodes {
        if node.is_group && !node.description.is_empty() {
            return Err(PeleError::typed(
                format!(
                    "Group nodes can only have label. Remove the additional description for node [{}]",
                    node.id
                ),
                ErrorCode::Semantic,
                "state",
            ));
        }
    }
    Ok(StateGraph {
        nodes: b.nodes,
        edges: b.edges,
    })
}
