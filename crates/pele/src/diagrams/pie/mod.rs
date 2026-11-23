//! pie（pele の `src/diagrams/pie/`）。

pub mod model;
pub mod parser;
pub mod render;

use std::any::Any;

use crate::errors::Result;
use crate::preprocess::Config;
use crate::types::{Diagram, ParseLimits, RenderOptions, Rendered};
use model::{PieModel, build_pie};

fn parse(
    source: &str,
    _config: &Config,
    title: Option<&str>,
    _line_offset: usize,
    _limits: &ParseLimits,
) -> Result<Box<dyn Any>> {
    Ok(Box::new(build_pie(parser::parse_pie(source)?, title)?))
}

fn render(model: &dyn Any, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let model = model.downcast_ref::<PieModel>().expect("pie model");
    render::render_pie(model, config, options)
}

pub static PIE: Diagram = Diagram {
    type_: "pie",
    section: None,
    keep_comments: false,
    math: false,
    parse,
    render,
};
