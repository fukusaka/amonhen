//! block（pele の `src/diagrams/block/`）。

pub mod arrows;
pub mod db;
pub mod layout;
pub mod lexer;
pub mod parser;
pub mod render;
pub mod tokens;
pub mod types;

use std::any::Any;

use crate::errors::Result;
use crate::preprocess::Config;
use crate::types::{Diagram, ParseLimits, RenderOptions, Rendered};
use db::BlockDb;

fn parse(
    source: &str,
    _config: &Config,
    title: Option<&str>,
    _line_offset: usize,
    _limits: &ParseLimits,
) -> Result<Box<dyn Any>> {
    let mut db = BlockDb::new();
    db.title = title.map(String::from);
    let src: Vec<u16> = source.encode_utf16().collect();
    parser::parse_block(&src, &mut db)?;
    Ok(Box::new(db))
}

fn render(model: &dyn Any, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let db = model.downcast_ref::<BlockDb>().expect("block model");
    render::render_block(db, config, options)
}

pub static BLOCK: Diagram = Diagram {
    type_: "block",
    section: None,
    keep_comments: false,
    math: true,
    parse,
    render,
};
