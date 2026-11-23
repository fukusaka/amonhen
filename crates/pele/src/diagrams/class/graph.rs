//! 描くノードと辺（pele の `src/diagrams/class/graph.ts`）。

use std::collections::HashMap;

use super::db::{ClassDb, LOLLIPOP};
use super::types::RelationEnd;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeShape {
    Rect,
    ClassBox,
    Note,
    Interface,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EndMarker {
    Aggregation,
    Extension,
    Composition,
    Dependency,
    Lollipop,
    None,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pattern {
    Solid,
    Dashed,
    Dotted,
}

#[derive(Clone, Debug)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub is_group: bool,
    pub shape: NodeShape,
    pub parent_id: Option<String>,
    /// これを描く名前空間のノードの添字。なければ -1。
    pub parent: i64,
    /// クラスなら `ClassDb::classes` の添字。
    pub class_node: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct GraphEdge {
    pub id: String,
    pub start: String,
    pub end: String,
    /// ノードの並びの添字。id だけでは決まらない: クラスの名前が `note0` や `interface0` のこともある。
    pub source: i64,
    pub target: i64,
    pub label: Option<String>,
    pub arrow_type_start: EndMarker,
    pub arrow_type_end: EndMarker,
    pub start_label_right: String,
    pub end_label_left: String,
    pub pattern: Pattern,
}

pub struct ClassGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

fn marker(type_: RelationEnd) -> EndMarker {
    match type_ {
        Some(0) => EndMarker::Aggregation,
        Some(1) => EndMarker::Extension,
        Some(2) => EndMarker::Composition,
        Some(3) => EndMarker::Dependency,
        Some(4) => EndMarker::Lollipop,
        _ => EndMarker::None,
    }
}

/// 読んだクラス図を、描くノードと辺にする。`hierarchical` が偽なら、ソースが宣言した名前空間だけを残し、それぞれを
/// 全体の名前で呼ぶ。
pub fn build_class_graph(model: &ClassDb, hierarchical: bool) -> ClassGraph {
    let mut nodes: Vec<GraphNode> = Vec::new();
    let mut edges: Vec<GraphEdge> = Vec::new();
    let mut namespace_at: HashMap<String, usize> = HashMap::new();
    let mut class_at: HashMap<String, usize> = HashMap::new();
    let mut interface_at: HashMap<String, usize> = HashMap::new();

    let mut declared: HashMap<String, Option<String>> = HashMap::new();
    let mut declared_ancestor = |id: Option<&str>| -> Option<String> {
        let id = id?;
        if hierarchical {
            return Some(id.to_string());
        }
        let mut chain: Vec<String> = Vec::new();
        let mut current = Some(id.to_string());
        let mut found: Option<String> = None;
        while let Some(cur) = current.filter(|c| !c.is_empty()) {
            if let Some(d) = declared.get(&cur) {
                found = d.clone();
                break;
            }
            let Some(ns) = model.namespace(&cur) else {
                break;
            };
            if ns.explicit {
                found = Some(cur);
                break;
            }
            chain.push(cur);
            current = ns.parent.clone();
        }
        for visited in chain {
            declared.insert(visited, found.clone());
        }
        found
    };
    let group = |namespace_at: &HashMap<String, usize>, id: Option<&str>| -> i64 {
        id.and_then(|id| namespace_at.get(id))
            .map_or(-1, |&k| k as i64)
    };

    for ns in &model.namespaces {
        if !hierarchical && !ns.explicit {
            continue;
        }
        namespace_at.insert(ns.id.clone(), nodes.len());
        nodes.push(GraphNode {
            id: ns.id.clone(),
            label: if hierarchical {
                ns.label.clone()
            } else {
                ns.id.clone()
            },
            is_group: true,
            shape: NodeShape::Rect,
            parent_id: if hierarchical {
                ns.parent.clone()
            } else {
                None
            },
            parent: -1,
            class_node: None,
        });
    }
    for node in &mut nodes {
        node.parent = group(&namespace_at, node.parent_id.as_deref());
    }

    for (k, class) in model.classes.iter().enumerate() {
        let parent_id = declared_ancestor(class.parent.as_deref());
        class_at.insert(class.id.clone(), nodes.len());
        nodes.push(GraphNode {
            id: class.id.clone(),
            label: class.label.clone(),
            is_group: false,
            shape: NodeShape::ClassBox,
            parent: group(&namespace_at, parent_id.as_deref()),
            parent_id,
            class_node: Some(k),
        });
    }

    for note in &model.notes {
        let parent_id = declared_ancestor(note.parent.as_deref());
        let target = note.class.as_ref().and_then(|c| class_at.get(c)).copied();
        if let Some(target) = target {
            edges.push(GraphEdge {
                id: format!("edgeNote{}", note.index),
                start: note.id.clone(),
                end: nodes[target].id.clone(),
                source: nodes.len() as i64,
                target: target as i64,
                label: None,
                arrow_type_start: EndMarker::None,
                arrow_type_end: EndMarker::None,
                start_label_right: String::new(),
                end_label_left: String::new(),
                pattern: Pattern::Dotted,
            });
        }
        nodes.push(GraphNode {
            id: note.id.clone(),
            label: note.text.clone(),
            is_group: false,
            shape: NodeShape::Note,
            parent: group(&namespace_at, parent_id.as_deref()),
            parent_id,
            class_node: None,
        });
    }

    // Mermaid はロリポップのインターフェースをどれも名前空間の外に置く。ここではクラスのそばに置く。
    for item in &model.interfaces {
        let owner = class_at.get(&item.class_id).copied();
        interface_at.insert(item.id.clone(), nodes.len());
        nodes.push(GraphNode {
            id: item.id.clone(),
            label: item.label.clone(),
            is_group: false,
            shape: NodeShape::Interface,
            parent_id: None,
            parent: owner.map_or(-1, |o| nodes[o].parent),
            class_node: None,
        });
    }

    for (count, relation) in model.relations.iter().enumerate() {
        let (type1, type2) = (relation.relation.type1, relation.relation.type2);
        // addRelation は、ほかの印のない関係のロリポップの端をインターフェースに替える。
        let start_iface = type1 == Some(LOLLIPOP) && type2.is_none();
        let end_iface = !start_iface && type2 == Some(LOLLIPOP) && type1.is_none();
        let start_at = if start_iface {
            &interface_at
        } else {
            &class_at
        };
        let end_at = if end_iface { &interface_at } else { &class_at };
        edges.push(GraphEdge {
            id: format!("id_{}_{}_{}", relation.id1, relation.id2, count + 1),
            start: relation.id1.clone(),
            end: relation.id2.clone(),
            source: start_at.get(&relation.id1).map_or(-1, |&k| k as i64),
            target: end_at.get(&relation.id2).map_or(-1, |&k| k as i64),
            label: relation.title.clone(),
            arrow_type_start: marker(type1),
            arrow_type_end: marker(type2),
            start_label_right: if relation.relation_title1 == "none" {
                String::new()
            } else {
                relation.relation_title1.clone()
            },
            end_label_left: if relation.relation_title2 == "none" {
                String::new()
            } else {
                relation.relation_title2.clone()
            },
            pattern: if relation.relation.line_type == 1 {
                Pattern::Dashed
            } else {
                Pattern::Solid
            },
        });
    }

    ClassGraph { nodes, edges }
}
