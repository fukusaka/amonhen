//! treeView（pele の `src/diagrams/treeview/`）。

pub mod box_drawing;
pub mod icons;
pub mod model;
pub mod parser;
pub mod render;

use std::any::Any;

use crate::errors::Result;
use crate::preprocess::Config;
use crate::types::{Diagram, ParseLimits, RenderOptions, Rendered};
use model::{TreeViewModel, build_tree_view, read_tree_view};

fn parse(
    source: &str,
    _config: &Config,
    title: Option<&str>,
    _line_offset: usize,
    _limits: &ParseLimits,
) -> Result<Box<dyn Any>> {
    Ok(Box::new(build_tree_view(read_tree_view(source)?, title)))
}

fn render(model: &dyn Any, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let model = model
        .downcast_ref::<TreeViewModel>()
        .expect("treeView model");
    render::render_tree_view(model, config, options)
}

pub static TREE_VIEW: Diagram = Diagram {
    type_: "treeView",
    section: None,
    keep_comments: false,
    math: false,
    parse,
    render,
};
