//! quadrantChart（pele の `src/diagrams/quadrant/`）。

pub mod db;
pub mod lexer;
pub mod parser;
pub mod render;
pub mod tokens;

use std::any::Any;

use crate::errors::Result;
use crate::preprocess::Config;
use crate::types::{Diagram, ParseLimits, RenderOptions, Rendered};
use db::QuadrantDb;

fn parse(
    source: &str,
    _config: &Config,
    title: Option<&str>,
    _line_offset: usize,
    _limits: &ParseLimits,
) -> Result<Box<dyn Any>> {
    let mut db = QuadrantDb {
        title: title.map(str::to_string),
        ..QuadrantDb::default()
    };
    parser::parse_quadrant(source, &mut db)?;
    Ok(Box::new(db))
}

fn render(model: &dyn Any, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let db = model
        .downcast_ref::<QuadrantDb>()
        .expect("quadrantChart model");
    Ok(render::render_quadrant(db, config, options))
}

pub static QUADRANT_CHART: Diagram = Diagram {
    type_: "quadrantChart",
    section: None,
    keep_comments: false,
    math: false,
    parse,
    render,
};
