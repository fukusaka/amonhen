//! gantt（pele の `src/diagrams/gantt/`）。
//!
//! pele は日付を地方時で数えるので、出力はホストの時間帯によって変わる。ここでは地方時の時間帯を
//! `RenderOptions.time_zone` で選び（`js_date`）、選ばなければ UTC として、どの機械でも同じ図を描く。

pub mod axis;
pub mod dates;
pub mod db;
pub mod js_date;
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
use db::GanttDb;

fn parse(
    source: &str,
    config: &Config,
    title: Option<&str>,
    _line_offset: usize,
    _limits: &ParseLimits,
) -> Result<Box<dyn Any>> {
    let mut db = GanttDb::new();
    if let Some(t) = title.filter(|t| !t.is_empty()) {
        db.title = Some(t.to_string());
    }
    if let Some(Value::Str(mode)) = config
        .get("gantt")
        .and_then(Value::as_obj)
        .and_then(|g| g.get("displayMode"))
    {
        db.display_mode = mode.clone();
    }
    parser::parse_gantt(source, &mut db)?;
    Ok(Box::new(db))
}

fn render(model: &dyn Any, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let db = model.downcast_ref::<GanttDb>().expect("gantt model");
    render::render_gantt(db, config, options)
}

pub static GANTT: Diagram = Diagram {
    type_: "gantt",
    section: None,
    keep_comments: false,
    math: false,
    parse,
    render,
};
