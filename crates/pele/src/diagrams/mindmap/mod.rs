//! mindmap（pele の `src/diagrams/mindmap/`）。

pub mod db;
pub mod parser;
pub mod render;

use std::any::Any;

use crate::errors::Result;
use crate::preprocess::Config;
use crate::types::{Diagram, ParseLimits, RenderOptions, Rendered};
use db::MindmapDb;

fn parse(
    source: &str,
    _config: &Config,
    title: Option<&str>,
    _line_offset: usize,
    _limits: &ParseLimits,
) -> Result<Box<dyn Any>> {
    let mut db = MindmapDb::default();
    db.title = title.map(str::to_string);
    parser::parse_mindmap(source, &mut db)?;
    Ok(Box::new(db))
}

fn render(model: &dyn Any, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let model = model.downcast_ref::<MindmapDb>().expect("mindmap model");
    render::render_mindmap(model, config, options)
}

pub static MINDMAP: Diagram = Diagram {
    type_: "mindmap",
    section: None,
    keep_comments: false,
    math: true,
    parse,
    render,
};
