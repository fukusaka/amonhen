//! xychart（pele の `src/diagrams/xychart/`）。

pub mod db;
pub mod lexer;
pub mod parser;
pub mod render;
pub mod ticks;
pub mod tokens;

use std::any::Any;

use crate::errors::Result;
use crate::preprocess::Config;
use crate::types::{Diagram, ParseLimits, RenderOptions, Rendered};
use db::XyChartDb;

fn parse(
    source: &str,
    _config: &Config,
    title: Option<&str>,
    _line_offset: usize,
    _limits: &ParseLimits,
) -> Result<Box<dyn Any>> {
    let mut db = XyChartDb {
        title: title.map(str::to_string),
        ..XyChartDb::default()
    };
    parser::parse_xy_chart(source, &mut db)?;
    Ok(Box::new(db))
}

fn render(model: &dyn Any, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let db = model.downcast_ref::<XyChartDb>().expect("xychart model");
    render::render_xy_chart(db, config, options)
}

pub static XYCHART: Diagram = Diagram {
    type_: "xychart",
    section: Some("xyChart"),
    keep_comments: false,
    math: false,
    parse,
    render,
};
