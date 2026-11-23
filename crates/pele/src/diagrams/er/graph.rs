//! 描くノードと辺（pele の `src/diagrams/er/graph.ts`）。

use std::collections::{HashMap, HashSet};

use super::db::ErDb;
use super::types::ErAttribute;
use crate::js;

#[derive(Clone, Debug)]
pub struct ErGraphNode<'a> {
    pub id: String,
    pub label: String,
    pub alias: String,
    pub attributes: &'a [ErAttribute],
    pub is_group: bool,
    pub parent_id: Option<String>,
    pub css_styles: &'a [String],
    pub css_compiled_styles: Vec<String>,
    pub css_classes: String,
    pub dir: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ErGraphEdge {
    pub id: String,
    pub start: String,
    pub end: String,
    pub label: String,
    pub arrow_type_start: String,
    pub arrow_type_end: String,
    pub dashed: bool,
}

pub struct ErGraph<'a> {
    pub nodes: Vec<ErGraphNode<'a>>,
    pub edges: Vec<ErGraphEdge>,
}

/// 読んだ ER 図を描くものにする: まとまりはグループ、実体はクラスのスタイルを持つノード、関係は両端に印の種類を
/// 持つ辺。
pub fn build_er_graph(model: &ErDb) -> ErGraph<'_> {
    let mut nodes: Vec<ErGraphNode> = Vec::new();
    let mut edges: Vec<ErGraphEdge> = Vec::new();
    let subgraphs = &model.subgraphs;

    let compiled = |class_names: &mut dyn Iterator<Item = &str>| -> Vec<String> {
        let mut out = Vec::new();
        for name in class_names {
            let Some(def) = model.classes.get(name) else {
                continue;
            };
            for style in &def.styles {
                out.push(js::trim(style).to_string());
            }
            for style in &def.text_styles {
                out.push(js::trim(style).to_string());
            }
        }
        out
    };

    let mut parent_of: HashMap<&str, &str> = HashMap::new();
    let mut subgraph_ids: HashSet<&str> = HashSet::new();
    for subgraph in subgraphs.iter().rev() {
        subgraph_ids.insert(&subgraph.id);
        for id in &subgraph.nodes {
            parent_of.insert(id, &subgraph.id);
        }
    }

    for subgraph in subgraphs.iter().rev() {
        nodes.push(ErGraphNode {
            id: subgraph.id.clone(),
            label: subgraph.title.clone(),
            alias: String::new(),
            attributes: &[],
            is_group: true,
            parent_id: parent_of.get(subgraph.id.as_str()).map(|s| s.to_string()),
            css_styles: &subgraph.css_styles,
            css_compiled_styles: compiled(&mut subgraph.classes.iter().map(String::as_str)),
            css_classes: subgraph.classes.join(" "),
            dir: subgraph.dir.clone(),
        });
    }

    // まとまりの id でもある名前は、そのまとまりを指す。Mermaid はその実体を捨て、まとまりの前に書いた関係を何も
    // 指さないまま残す。ここではまとまりに付いていく。
    let mut moved: HashMap<&str, &str> = HashMap::new();
    for (name, entity) in &model.entities {
        if subgraph_ids.contains(name.as_str()) {
            moved.insert(&entity.id, name);
            continue;
        }
        nodes.push(ErGraphNode {
            id: entity.id.clone(),
            label: entity.label.clone(),
            alias: entity.alias.clone(),
            attributes: &entity.attributes,
            is_group: false,
            parent_id: parent_of.get(name.as_str()).map(|s| s.to_string()),
            css_styles: &entity.css_styles,
            css_compiled_styles: compiled(&mut entity.css_classes.split(' ')),
            css_classes: entity.css_classes.clone(),
            dir: None,
        });
    }

    for (index, r) in model.relationships.iter().enumerate() {
        edges.push(ErGraphEdge {
            id: format!("id_{}_{}_{index}", r.entity_a, r.entity_b),
            start: moved
                .get(r.entity_a.as_str())
                .map_or(r.entity_a.clone(), |s| s.to_string()),
            end: moved
                .get(r.entity_b.as_str())
                .map_or(r.entity_b.clone(), |s| s.to_string()),
            label: r.role_a.clone(),
            arrow_type_start: r.rel_spec.card_b.to_lowercase(),
            arrow_type_end: r.rel_spec.card_a.to_lowercase(),
            dashed: !r.rel_spec.identifying,
        });
    }

    ErGraph { nodes, edges }
}
