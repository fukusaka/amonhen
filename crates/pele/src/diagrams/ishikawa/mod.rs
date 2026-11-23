//! ishikawa（pele の `src/diagrams/ishikawa/`）。

pub mod db;
pub mod lexer;
pub mod parser;
pub mod render;
pub mod types;

use std::any::Any;

use crate::errors::Result;
use crate::preprocess::Config;
use crate::types::{Diagram, ParseLimits, RenderOptions, Rendered};
use db::IshikawaDb;

fn parse(
    source: &str,
    _config: &Config,
    title: Option<&str>,
    _line_offset: usize,
    _limits: &ParseLimits,
) -> Result<Box<dyn Any>> {
    let mut db = IshikawaDb::default();
    db.title = title.map(str::to_string);
    parser::parse_ishikawa(source, &mut db)?;
    Ok(Box::new(db))
}

fn render(model: &dyn Any, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let db = model.downcast_ref::<IshikawaDb>().expect("ishikawa model");
    render::render_ishikawa(db, config, options)
}

pub static ISHIKAWA: Diagram = Diagram {
    type_: "ishikawa",
    section: None,
    keep_comments: false,
    math: false,
    parse,
    render,
};
