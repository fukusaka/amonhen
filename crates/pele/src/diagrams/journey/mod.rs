//! journey（pele の `src/diagrams/journey/`）。

pub mod db;
pub mod lexer;
pub mod parser;
pub mod render;

use std::any::Any;

use crate::errors::Result;
use crate::preprocess::Config;
use crate::types::{Diagram, ParseLimits, RenderOptions, Rendered};
use db::JourneyDb;

fn parse(
    source: &str,
    _config: &Config,
    title: Option<&str>,
    _line_offset: usize,
    _limits: &ParseLimits,
) -> Result<Box<dyn Any>> {
    let mut db = JourneyDb::default();
    db.title = title.map(str::to_string);
    parser::parse_journey(source, &mut db)?;
    Ok(Box::new(db))
}

fn render(model: &dyn Any, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let model = model.downcast_ref::<JourneyDb>().expect("journey model");
    render::render_journey(model, config, options)
}

pub static JOURNEY: Diagram = Diagram {
    type_: "journey",
    section: None,
    keep_comments: false,
    math: false,
    parse,
    render,
};
