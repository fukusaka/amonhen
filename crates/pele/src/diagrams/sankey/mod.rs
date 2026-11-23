//! sankey（pele の `src/diagrams/sankey/`）。

pub mod db;
pub mod layout;
pub mod lexer;
pub mod parser;
pub mod render;

use std::any::Any;

use crate::errors::Result;
use crate::preprocess::Config;
use crate::types::{Diagram, ParseLimits, RenderOptions, Rendered};
use db::SankeyDb;

fn parse(
    source: &str,
    _config: &Config,
    title: Option<&str>,
    _line_offset: usize,
    _limits: &ParseLimits,
) -> Result<Box<dyn Any>> {
    let mut db = SankeyDb::default();
    if let Some(t) = title.filter(|t| !t.is_empty()) {
        db.title = Some(t.to_string());
    }
    let prepared = parser::prepare_text_for_parsing(source, None);
    parser::parse_sankey(&prepared, &mut db, source)?;
    Ok(Box::new(db))
}

fn render(model: &dyn Any, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let db = model.downcast_ref::<SankeyDb>().expect("sankey model");
    render::render_sankey(db, config, options)
}

pub static SANKEY: Diagram = Diagram {
    type_: "sankey",
    section: None,
    keep_comments: false,
    math: false,
    parse,
    render,
};
