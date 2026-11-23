//! er（pele の `src/diagrams/er/`）。

pub mod db;
pub mod graph;
pub mod lexer;
pub mod markers;
pub mod parser;
pub mod render;
pub mod tokens;
pub mod types;

use std::any::Any;

use crate::errors::Result;
use crate::preprocess::Config;
use crate::types::{Diagram, ParseLimits, RenderOptions, Rendered};
use db::ErDb;

fn parse(
    source: &str,
    _config: &Config,
    title: Option<&str>,
    _line_offset: usize,
    _limits: &ParseLimits,
) -> Result<Box<dyn Any>> {
    let mut db = ErDb::new();
    if let Some(t) = title.filter(|t| !t.is_empty()) {
        db.title = Some(t.to_string());
    }
    parser::parse_er(source, &mut db)?;
    Ok(Box::new(db))
}

fn render(model: &dyn Any, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let db = model.downcast_ref::<ErDb>().expect("er model");
    render::render_er(db, config, options)
}

pub static ER: Diagram = Diagram {
    type_: "er",
    section: None,
    keep_comments: false,
    math: true,
    parse,
    render,
};
