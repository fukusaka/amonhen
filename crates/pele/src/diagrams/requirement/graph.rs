//! 配置するノードと辺（pele の `src/diagrams/requirement/graph.ts`）。

use super::db::{RequirementDb, relationships};
use super::types::{Requirement, RequirementElement};

pub struct GraphNode<'a> {
    pub id: &'a str,
    pub requirement: Option<&'a Requirement>,
    pub element: Option<&'a RequirementElement>,
    pub css_styles: &'a [String],
    pub css_classes: String,
}

pub struct GraphEdge<'a> {
    pub id: String,
    pub start: Option<&'a str>,
    pub end: Option<&'a str>,
    pub type_: &'a str,
    pub contains: bool,
}

pub struct RequirementGraph<'a> {
    pub nodes: Vec<GraphNode<'a>>,
    pub edges: Vec<GraphEdge<'a>>,
    pub direction: &'a str,
}

/// 配置するノードと辺: Mermaid と同じく要求が先、要素が後。
pub fn build_requirement_graph(model: &RequirementDb) -> RequirementGraph<'_> {
    let mut nodes = Vec::new();
    for requirement in &model.requirements {
        nodes.push(GraphNode {
            id: &requirement.name,
            requirement: Some(requirement),
            element: None,
            css_styles: &requirement.css_styles,
            css_classes: requirement.classes.join(" "),
        });
    }
    for element in &model.elements {
        nodes.push(GraphNode {
            id: &element.name,
            requirement: None,
            element: Some(element),
            css_styles: &element.css_styles,
            css_classes: element.classes.join(" "),
        });
    }
    let has = |id: &'_ str| model.has(id);
    let edges = model
        .relations
        .iter()
        .enumerate()
        .map(|(index, relation)| GraphEdge {
            id: format!("{}-{}-{}", relation.src, relation.dst, index),
            start: has(&relation.src).then_some(relation.src.as_str()),
            end: has(&relation.dst).then_some(relation.dst.as_str()),
            type_: &relation.type_,
            contains: relation.type_ == relationships::CONTAINS,
        })
        .collect();
    RequirementGraph {
        nodes,
        edges,
        direction: &model.direction,
    }
}
