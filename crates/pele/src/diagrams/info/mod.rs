//! info（pele の `src/diagrams/info/`）。

pub mod model;
pub mod parser;
pub mod render;

use std::any::Any;

use crate::errors::Result;
use crate::preprocess::Config;
use crate::types::{Diagram, ParseLimits, RenderOptions, Rendered};
use model::{InfoModel, build_info};

fn parse(
    source: &str,
    _config: &Config,
    title: Option<&str>,
    _line_offset: usize,
    _limits: &ParseLimits,
) -> Result<Box<dyn Any>> {
    Ok(Box::new(build_info(parser::parse_info(source)?, title)))
}

fn render(model: &dyn Any, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let model = model.downcast_ref::<InfoModel>().expect("info model");
    Ok(render::render_info(model, config, options))
}

pub static INFO: Diagram = Diagram {
    type_: "info",
    section: None,
    keep_comments: false,
    math: false,
    parse,
    render,
};
