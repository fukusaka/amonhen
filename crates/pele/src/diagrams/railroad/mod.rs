//! railroad（pele の `src/diagrams/railroad/`）。Mermaid の 4 つの書き方（`railroad-beta`・
//! `railroad-ebnf-beta`・`railroad-abnf-beta`・`railroad-peg-beta`）を読む。

pub mod abnf;
pub mod common;
pub mod db;
pub mod ebnf;
pub mod ir;
pub mod peg;
pub mod render;
pub mod types;

use std::any::Any;

use crate::errors::Result;
use crate::preprocess::Config;
use crate::types::{Diagram, ParseLimits, RenderOptions, Rendered};
use types::RailroadModel;

fn parse(
    source: &str,
    _config: &Config,
    title: Option<&str>,
    _line_offset: usize,
    _limits: &ParseLimits,
) -> Result<Box<dyn Any>> {
    Ok(Box::new(db::parse_railroad(source, title)?))
}

fn render(model: &dyn Any, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let model = model
        .downcast_ref::<RailroadModel>()
        .expect("railroad model");
    render::render_railroad(model, config, options)
}

pub static RAILROAD: Diagram = Diagram {
    type_: "railroad",
    section: None,
    keep_comments: false,
    math: false,
    parse,
    render,
};
