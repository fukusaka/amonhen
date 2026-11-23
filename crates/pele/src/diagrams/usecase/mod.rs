//! use case（pele の `src/diagrams/usecase/`）。

pub mod db;
pub mod graph;
pub mod json;
pub mod lexer;
pub mod parser;
pub mod render;
pub mod tokens;
pub mod types;

use std::any::Any;

use crate::errors::Result;
use crate::preprocess::Config;
use crate::types::{Diagram, ParseLimits, RenderOptions, Rendered};
use types::UsecaseModel;

fn parse(
    source: &str,
    _config: &Config,
    title: Option<&str>,
    _line_offset: usize,
    _limits: &ParseLimits,
) -> Result<Box<dyn Any>> {
    Ok(Box::new(parser::parse_usecase(source, title)?))
}

fn render(model: &dyn Any, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let model = model.downcast_ref::<UsecaseModel>().expect("usecase model");
    render::render_usecase(model, config, options)
}

pub static USECASE: Diagram = Diagram {
    type_: "usecase",
    section: None,
    keep_comments: false,
    math: true,
    parse,
    render,
};
