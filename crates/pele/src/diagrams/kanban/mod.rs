//! kanban（pele の `src/diagrams/kanban/`）。

pub mod db;
pub mod parser;
pub mod render;

use std::any::Any;

use crate::errors::Result;
use crate::preprocess::Config;
use crate::types::{Diagram, ParseLimits, RenderOptions, Rendered};
use db::KanbanDb;

fn parse(
    source: &str,
    _config: &Config,
    title: Option<&str>,
    _line_offset: usize,
    _limits: &ParseLimits,
) -> Result<Box<dyn Any>> {
    let mut db = KanbanDb::default();
    db.title = title.map(str::to_string);
    parser::parse_kanban(source, &mut db)?;
    Ok(Box::new(db))
}

fn render(model: &dyn Any, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let model = model.downcast_ref::<KanbanDb>().expect("kanban model");
    Ok(render::render_kanban(model, config, options))
}

pub static KANBAN: Diagram = Diagram {
    type_: "kanban",
    section: None,
    keep_comments: false,
    math: true,
    parse,
    render,
};
