//! class（pele の `src/diagrams/class/`）。

pub mod db;
pub mod graph;
pub mod lexer;
pub mod markers;
pub mod members;
pub mod parser;
pub mod render;
pub mod tokens;
pub mod types;

use std::any::Any;

use crate::errors::{ErrorCode, PeleError, Result};
use crate::preprocess::Config;
use crate::types::{Diagram, ParseLimits, RenderOptions, Rendered};
use crate::util::yaml::Value;
use db::ClassDb;

fn parse(
    source: &str,
    config: &Config,
    title: Option<&str>,
    _line_offset: usize,
    limits: &ParseLimits,
) -> Result<Box<dyn Any>> {
    let mut db = ClassDb::new();
    if let Some(t) = title.filter(|t| !t.is_empty()) {
        db.title = Some(t.to_string());
    }
    parser::parse_class_diagram(source, &mut db)?;
    // 名前空間の枠はそれぞれ全体の名前を書くので、点で区切った長い名前は描く前に出力の上限を超えると分かる。
    let hierarchical = match config.get("class") {
        Some(Value::Obj(c)) => c.get("hierarchicalNamespaces") != Some(&Value::Bool(false)),
        _ => true,
    };
    if let Some(e) = PeleError::output_at_least(db.namespace_ink(hierarchical), limits.output_limit)
    {
        return Err(e);
    }
    if db.namespaces_cut() {
        return Err(PeleError::new(
            "Class diagram namespace names are too long to draw.",
            ErrorCode::Limit,
        ));
    }
    Ok(Box::new(db))
}

fn render(model: &dyn Any, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let db = model.downcast_ref::<ClassDb>().expect("class model");
    render::render_class(db, config, options)
}

pub static CLASS: Diagram = Diagram {
    type_: "class",
    section: None,
    keep_comments: false,
    math: true,
    parse,
    render,
};
