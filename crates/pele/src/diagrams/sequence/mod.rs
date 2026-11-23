//! sequence（pele の `src/diagrams/sequence/`）。

pub mod actors;
pub mod colors;
pub mod db;
pub mod lexer;
pub mod parser;
pub mod render;
pub mod tokens;
pub mod types;

use std::any::Any;

use crate::errors::Result;
use crate::preprocess::Config;
use crate::types::{Diagram, ParseLimits, RenderOptions, Rendered};
use crate::util::yaml::Value;
use db::{SeqDb, SeqDbOptions};

fn parse(
    source: &str,
    config: &Config,
    title: Option<&str>,
    _line_offset: usize,
    _limits: &ParseLimits,
) -> Result<Box<dyn Any>> {
    let seq = config.get("sequence").and_then(Value::as_obj);
    let mut db = SeqDb::new(SeqDbOptions {
        wrap: match config.get("wrap") {
            Some(Value::Bool(b)) => Some(*b),
            _ => None,
        },
        sequence_wrap: seq.and_then(|s| s.get("wrap")) == Some(&Value::Bool(true)),
    });
    if let Some(t) = title.filter(|t| !t.is_empty()) {
        db.set_diagram_title(t);
    }
    parser::parse_sequence(source, &mut db)?;
    Ok(Box::new(db))
}

fn render(model: &dyn Any, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let db = model.downcast_ref::<SeqDb>().expect("sequence model");
    render::render_sequence(db, config, options)
}

pub static SEQUENCE: Diagram = Diagram {
    type_: "sequence",
    section: None,
    keep_comments: false,
    math: true,
    parse,
    render,
};
