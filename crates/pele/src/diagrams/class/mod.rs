//! class（pele の `src/diagrams/class/`）。

pub mod db;
pub mod graph;
pub mod lexer;
pub mod markers;
pub mod members;
pub mod parser;
pub mod render;
pub mod tokens;
pub mod types;

use std::any::Any;

use crate::errors::Result;
use crate::preprocess::Config;
use crate::types::{Diagram, ParseLimits, RenderOptions, Rendered};
use db::ClassDb;

fn parse(
    source: &str,
    _config: &Config,
    title: Option<&str>,
    _line_offset: usize,
    _limits: &ParseLimits,
) -> Result<Box<dyn Any>> {
    let mut db = ClassDb::new();
    if let Some(t) = title.filter(|t| !t.is_empty()) {
        db.title = Some(t.to_string());
    }
    parser::parse_class_diagram(source, &mut db)?;
    Ok(Box::new(db))
}

fn render(model: &dyn Any, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let db = model.downcast_ref::<ClassDb>().expect("class model");
    render::render_class(db, config, options)
}

pub static CLASS: Diagram = Diagram {
    type_: "class",
    section: None,
    keep_comments: false,
    math: true,
    parse,
    render,
};
