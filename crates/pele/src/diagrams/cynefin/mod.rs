//! cynefin（pele の `src/diagrams/cynefin/`）。

pub mod boundaries;
pub mod model;
pub mod parser;
pub mod render;

use std::any::Any;

use crate::errors::Result;
use crate::preprocess::Config;
use crate::types::{Diagram, ParseLimits, RenderOptions, Rendered};
use model::{CynefinModel, build_cynefin};

fn parse(
    source: &str,
    _config: &Config,
    title: Option<&str>,
    _line_offset: usize,
    _limits: &ParseLimits,
) -> Result<Box<dyn Any>> {
    Ok(Box::new(build_cynefin(
        parser::parse_cynefin(source)?,
        title,
    )))
}

fn render(model: &dyn Any, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let model = model.downcast_ref::<CynefinModel>().expect("cynefin model");
    render::render_cynefin(model, config, options)
}

pub static CYNEFIN: Diagram = Diagram {
    type_: "cynefin",
    section: None,
    keep_comments: false,
    math: false,
    parse,
    render,
};
