//! gitGraph（pele の `src/diagrams/git/`）。

pub mod db;
pub mod parser;
pub mod render;

use std::any::Any;

use crate::errors::Result;
use crate::preprocess::Config;
use crate::types::{Diagram, ParseLimits, RenderOptions, Rendered};
use db::{GitModel, build_git};

fn parse(
    source: &str,
    config: &Config,
    title: Option<&str>,
    _line_offset: usize,
    _limits: &ParseLimits,
) -> Result<Box<dyn Any>> {
    Ok(Box::new(build_git(
        parser::parse_git(source)?,
        config,
        title,
    )?))
}

fn render(model: &dyn Any, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let model = model.downcast_ref::<GitModel>().expect("gitGraph model");
    render::render_git(model, config, options)
}

pub static GIT_GRAPH: Diagram = Diagram {
    type_: "gitGraph",
    section: None,
    keep_comments: false,
    math: false,
    parse,
    render,
};
