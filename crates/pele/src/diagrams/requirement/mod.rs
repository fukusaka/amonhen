//! requirement（pele の `src/diagrams/requirement/`）。

pub mod db;
pub mod graph;
pub mod lexer;
pub mod parser;
pub mod render;
pub mod tokens;
pub mod types;

use std::any::Any;

use crate::errors::Result;
use crate::preprocess::Config;
use crate::types::{Diagram, ParseLimits, RenderOptions, Rendered};
use db::RequirementDb;

fn parse(
    source: &str,
    _config: &Config,
    title: Option<&str>,
    _line_offset: usize,
    _limits: &ParseLimits,
) -> Result<Box<dyn Any>> {
    let mut db = RequirementDb::new();
    if let Some(t) = title.filter(|t| !t.is_empty()) {
        db.title = Some(t.to_string());
    }
    parser::parse_requirement(source, &mut db)?;
    Ok(Box::new(db))
}

fn render(model: &dyn Any, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let db = model
        .downcast_ref::<RequirementDb>()
        .expect("requirement model");
    render::render_requirement(db, config, options)
}

pub static REQUIREMENT: Diagram = Diagram {
    type_: "requirement",
    section: None,
    keep_comments: false,
    math: true,
    parse,
    render,
};
