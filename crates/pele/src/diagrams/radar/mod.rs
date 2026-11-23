//! radar（pele の `src/diagrams/radar/`）。

pub mod model;
pub mod parser;
pub mod render;

use std::any::Any;

use crate::errors::Result;
use crate::preprocess::Config;
use crate::types::{Diagram, ParseLimits, RenderOptions, Rendered};
use model::{RadarModel, build_radar};

fn parse(
    source: &str,
    _config: &Config,
    title: Option<&str>,
    _line_offset: usize,
    _limits: &ParseLimits,
) -> Result<Box<dyn Any>> {
    Ok(Box::new(build_radar(parser::parse_radar(source)?, title)?))
}

fn render(model: &dyn Any, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let model = model.downcast_ref::<RadarModel>().expect("radar model");
    Ok(render::render_radar(model, config, options))
}

pub static RADAR: Diagram = Diagram {
    type_: "radar",
    section: None,
    keep_comments: false,
    math: false,
    parse,
    render,
};
