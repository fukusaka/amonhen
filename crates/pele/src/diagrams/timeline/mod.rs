//! timeline（pele の `src/diagrams/timeline/`）。

pub mod db;
pub mod lexer;
pub mod parser;
pub mod render;

use std::any::Any;

use crate::errors::Result;
use crate::preprocess::Config;
use crate::types::{Diagram, ParseLimits, RenderOptions, Rendered};
use db::TimelineDb;

fn parse(
    source: &str,
    _config: &Config,
    title: Option<&str>,
    _line_offset: usize,
    _limits: &ParseLimits,
) -> Result<Box<dyn Any>> {
    let mut db = TimelineDb::default();
    db.title = title.map(str::to_string);
    parser::parse_timeline(source, &mut db)?;
    Ok(Box::new(db))
}

fn render(model: &dyn Any, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let model = model.downcast_ref::<TimelineDb>().expect("timeline model");
    render::render_timeline(model, config, options)
}

pub static TIMELINE: Diagram = Diagram {
    type_: "timeline",
    section: None,
    keep_comments: false,
    math: false,
    parse,
    render,
};
