//! agentflow の形（pele の `src/diagrams/agentflow/types.ts`）。位置の対応（`mappings`）と意味のモデル
//! （`semanticModel`）は描くことに届かないので移していない。

pub use crate::diagrams::flowchart::types::{FlowText, LabelType};
use crate::util::yaml::{Map, Value};

/// Jison の知らせる範囲: 行は 1 から、列は 0 から。
#[derive(Clone, Copy, Debug, Default)]
pub struct Loc {
    pub first_line: usize,
    pub first_column: usize,
    pub last_line: usize,
    pub last_column: usize,
}

pub type Metadata = Map;

#[derive(Clone, Debug)]
pub struct AgentNode {
    pub id: String,
    pub text: Option<String>,
    pub label_type: LabelType,
    pub type_: Option<String>,
    pub styles: Vec<String>,
    pub classes: Vec<String>,
    pub dir: Option<String>,
    pub props: Option<Vec<(String, String)>>,
    pub link: Option<String>,
    pub link_target: Option<String>,
    pub metadata: Option<Metadata>,
    pub is_connector: bool,
}

#[derive(Clone, Debug)]
pub struct AgentEdge {
    pub start: String,
    pub end: String,
    pub type_: Option<String>,
    pub text: String,
    pub label_type: LabelType,
    pub stroke: Option<String>,
    pub edge_semantic: Option<&'static str>,
    pub length: Option<f64>,
    pub style: Option<Vec<String>>,
    pub classes: Vec<String>,
    pub id: Option<String>,
    pub is_user_defined_id: bool,
    /// メタデータの `curve` は文字列とは限らない。
    pub interpolate: Option<Value>,
    pub animate: Option<Value>,
    pub animation: Option<Value>,
    pub metadata: Option<Metadata>,
}

#[derive(Clone, Debug)]
pub struct AgentClassDef {
    pub id: String,
    pub styles: Vec<String>,
    pub text_styles: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct AgentSubgraph {
    pub id: String,
    pub nodes: Vec<String>,
    pub title: String,
    pub classes: Vec<String>,
    pub dir: Option<String>,
    pub label_type: LabelType,
    pub metadata: Option<Metadata>,
}

#[derive(Clone, Debug)]
pub struct LinkInfo {
    pub type_: String,
    pub stroke: String,
    pub length: Option<f64>,
    pub edge_semantic: Option<&'static str>,
    pub text: Option<FlowText>,
    pub id: Option<String>,
}

#[derive(Clone, Debug)]
pub enum DocItem {
    Str(String),
    List(Vec<String>),
    Dir(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Diagnostic {
    pub id: &'static str,
    pub severity: &'static str,
    pub message: String,
    pub node_id: Option<String>,
    pub edge_id: Option<String>,
}
