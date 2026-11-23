//! flowchart（pele の `src/diagrams/flowchart/`）。

pub mod db;
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
use db::{FlowDb, FlowDbOptions};

fn parse(
    source: &str,
    config: &Config,
    title: Option<&str>,
    _line_offset: usize,
    limits: &ParseLimits,
) -> Result<Box<dyn Any>> {
    let inherit_dir = config
        .get("flowchart")
        .and_then(Value::as_obj)
        .and_then(|f| f.get("inheritDir"))
        == Some(&Value::Bool(true));
    let mut db = FlowDb::new(FlowDbOptions {
        inherit_dir,
        max_edges: Some(limits.max_edges),
        direction: None,
    });
    if let Some(t) = title.filter(|t| !t.is_empty()) {
        db.title = Some(t.to_string());
    }
    parser::parse_flowchart(source, &mut db)?;
    Ok(Box::new(db))
}

fn render(model: &dyn Any, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let db = model.downcast_ref::<FlowDb>().expect("flowchart model");
    render::render_flowchart(db, config, options, None)
}

pub static FLOWCHART: Diagram = Diagram {
    type_: "flowchart",
    section: None,
    keep_comments: false,
    math: true,
    parse,
    render,
};
