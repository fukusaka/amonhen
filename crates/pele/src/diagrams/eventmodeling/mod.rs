//! eventmodeling（pele の `src/diagrams/eventmodeling/`）。

pub mod model;
pub mod parser;
pub mod render;

use std::any::Any;

use crate::errors::Result;
use crate::preprocess::Config;
use crate::types::{Diagram, ParseLimits, RenderOptions, Rendered};
use model::{EventModelingModel, build_event_model};

fn parse(
    source: &str,
    _config: &Config,
    title: Option<&str>,
    _line_offset: usize,
    _limits: &ParseLimits,
) -> Result<Box<dyn Any>> {
    let ast = parser::parse_event_model(source)?;
    Ok(Box::new(build_event_model(&ast, title)?))
}

fn render(model: &dyn Any, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let model = model
        .downcast_ref::<EventModelingModel>()
        .expect("eventmodeling model");
    render::render_event_model(model, config, options)
}

pub static EVENTMODELING: Diagram = Diagram {
    type_: "eventmodeling",
    section: None,
    keep_comments: true,
    math: false,
    parse,
    render,
};
