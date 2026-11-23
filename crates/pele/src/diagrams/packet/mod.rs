//! packet（pele の `src/diagrams/packet/`）。

pub mod model;
pub mod parser;
pub mod render;

use std::any::Any;

use crate::errors::Result;
use crate::preprocess::Config;
use crate::types::{Diagram, ParseLimits, RenderOptions, Rendered};
use model::{PacketModel, build_packet};

fn parse(
    source: &str,
    config: &Config,
    title: Option<&str>,
    _line_offset: usize,
    _limits: &ParseLimits,
) -> Result<Box<dyn Any>> {
    Ok(Box::new(build_packet(
        parser::parse_packet(source)?,
        config,
        title,
    )?))
}

fn render(model: &dyn Any, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let model = model.downcast_ref::<PacketModel>().expect("packet model");
    render::render_packet(model, config, options)
}

pub static PACKET: Diagram = Diagram {
    type_: "packet",
    section: None,
    keep_comments: false,
    math: false,
    parse,
    render,
};
