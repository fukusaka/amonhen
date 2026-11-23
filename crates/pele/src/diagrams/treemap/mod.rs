//! treemap（pele の `src/diagrams/treemap/`）。

pub mod format;
pub mod layout;
pub mod model;
pub mod parser;
pub mod render;

use std::any::Any;

use crate::errors::Result;
use crate::preprocess::Config;
use crate::types::{Diagram, ParseLimits, RenderOptions, Rendered};
use model::{TreemapModel, build_treemap};

fn parse(
    source: &str,
    _config: &Config,
    title: Option<&str>,
    _line_offset: usize,
    _limits: &ParseLimits,
) -> Result<Box<dyn Any>> {
    Ok(Box::new(build_treemap(
        parser::parse_treemap(source)?,
        title,
    )))
}

fn render(model: &dyn Any, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let model = model.downcast_ref::<TreemapModel>().expect("treemap model");
    render::render_treemap(model, config, options)
}

pub static TREEMAP: Diagram = Diagram {
    type_: "treemap",
    section: None,
    keep_comments: false,
    math: false,
    parse,
    render,
};
