//! architecture（pele の `src/diagrams/architecture/`）。

pub mod db;
pub mod icons;
pub mod layout;
pub mod parser;
pub mod render;

use std::any::Any;

use crate::errors::Result;
use crate::preprocess::Config;
use crate::types::{Diagram, ParseLimits, RenderOptions, Rendered};
use db::{ArchitectureDb, populate};

fn parse(
    source: &str,
    _config: &Config,
    title: Option<&str>,
    _line_offset: usize,
    _limits: &ParseLimits,
) -> Result<Box<dyn Any>> {
    let mut db = ArchitectureDb::default();
    if let Some(t) = title.filter(|t| !t.is_empty()) {
        db.title = Some(t.to_string());
    }
    populate(&parser::parse_architecture(source)?, &mut db)?;
    Ok(Box::new(db))
}

fn render(model: &dyn Any, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let db = model
        .downcast_ref::<ArchitectureDb>()
        .expect("architecture model");
    render::render_architecture(db, config, options)
}

pub static ARCHITECTURE: Diagram = Diagram {
    type_: "architecture",
    section: None,
    keep_comments: false,
    math: false,
    parse,
    render,
};
