//! flowchart の形（pele の `src/diagrams/flowchart/types.ts`）。

use crate::util::yaml::{Map, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelType {
    Text,
    String,
    Markdown,
}

impl LabelType {
    pub fn as_str(self) -> &'static str {
        match self {
            LabelType::Text => "text",
            LabelType::String => "string",
            LabelType::Markdown => "markdown",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FlowText {
    pub text: String,
    pub type_: LabelType,
}

#[derive(Clone, Debug, Default)]
pub struct FlowNode {
    pub id: String,
    pub text: Option<String>,
    pub label_type: Option<LabelType>,
    pub type_: Option<String>,
    pub styles: Vec<String>,
    pub classes: Vec<String>,
    pub dir: Option<String>,
    pub props: Option<Vec<(String, String)>>,
    pub link: Option<String>,
    pub link_target: Option<String>,
    pub icon: Option<String>,
    pub form: Option<String>,
    pub pos: Option<String>,
    pub img: Option<String>,
    pub constraint: Option<String>,
    pub asset_width: Option<f64>,
    pub asset_height: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct FlowEdge {
    pub start: String,
    pub end: String,
    pub type_: Option<String>,
    pub text: String,
    pub label_type: LabelType,
    pub stroke: Option<String>,
    pub length: Option<f64>,
    pub style: Option<Vec<String>>,
    pub classes: Vec<String>,
    pub id: Option<String>,
    pub is_user_defined_id: bool,
    pub interpolate: Option<String>,
    pub animate: Option<Value>,
    pub animation: Option<Value>,
}

#[derive(Clone, Debug)]
pub struct FlowClassDef {
    pub id: String,
    pub styles: Vec<String>,
    pub text_styles: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct FlowSubgraph {
    pub id: String,
    pub nodes: Vec<String>,
    pub title: String,
    pub classes: Vec<String>,
    pub dir: Option<String>,
    pub label_type: LabelType,
    pub metadata: Option<Map>,
}

/// 辺の読み（`destructLink` の答え）。
#[derive(Clone, Debug)]
pub struct LinkInfo {
    pub type_: String,
    pub stroke: String,
    pub length: Option<f64>,
    pub text: Option<FlowText>,
    pub id: Option<String>,
}

/// 文の読み（subgraph の中身として集める）。
#[derive(Clone, Debug)]
pub enum DocItem {
    Str(String),
    List(Vec<String>),
    Dir(String),
}
