//! agentflow（pele の `src/diagrams/agentflow/`）。

pub mod color_slots;
pub mod db;
pub mod diagnostics;
pub mod graph;
pub mod lexer;
pub mod parser;
pub mod render;
pub mod shapes;
pub mod tokens;
pub mod types;

use std::any::Any;

use crate::errors::Result;
use crate::preprocess::Config;
use crate::types::{Diagram, ParseLimits, RenderOptions, Rendered};
use crate::util::yaml::Value;
use db::{AgentflowDb, AgentflowDbOptions};

fn parse(
    source: &str,
    config: &Config,
    title: Option<&str>,
    line_offset: usize,
    limits: &ParseLimits,
) -> Result<Box<dyn Any>> {
    // Mermaid は agentflow でもこの設定を flowchart の節から読む。
    let flow = config.get("flowchart").and_then(Value::as_obj);
    let inherit_dir = flow.and_then(|f| f.get("inheritDir")) == Some(&Value::Bool(true));
    let mut db = AgentflowDb::new(AgentflowDbOptions {
        inherit_dir,
        direction: match config.get("direction") {
            Some(Value::Str(s)) => Some(s.clone()),
            _ => None,
        },
        line_offset,
        source: Some(source.encode_utf16().collect()),
        max_edges: Some(limits.max_edges),
    });
    if let Some(t) = title.filter(|t| !t.is_empty()) {
        db.title = Some(t.to_string());
    }
    parser::parse_agentflow(source, &mut db, false)?;
    let curve = match flow.and_then(|f| f.get("curve")) {
        Some(Value::Str(s)) => Some(s.clone()),
        _ => None,
    };
    let graph = graph::build_agent_graph(&mut db, render::PALETTE, curve.as_deref());
    db.graph = Some(graph);
    Ok(Box::new(db))
}

fn render(model: &dyn Any, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let db = model
        .downcast_ref::<AgentflowDb>()
        .expect("agentflow model");
    render::render_agentflow(db, config, options)
}

pub static AGENTFLOW: Diagram = Diagram {
    type_: "agentflow",
    section: None,
    keep_comments: true,
    math: false,
    parse,
    render,
};
