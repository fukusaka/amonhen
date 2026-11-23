//! wardley（pele の `src/diagrams/wardley/`）。

pub mod builder;
pub mod model;
pub mod parser;
pub mod render;

use std::any::Any;

use crate::errors::Result;
use crate::preprocess::Config;
use crate::types::{Diagram, ParseLimits, RenderOptions, Rendered};
use model::{WardleyModel, build_wardley};

fn parse(
    source: &str,
    _config: &Config,
    title: Option<&str>,
    _line_offset: usize,
    _limits: &ParseLimits,
) -> Result<Box<dyn Any>> {
    let ast = parser::parse_wardley(source)?;
    Ok(Box::new(build_wardley(&ast, title)?))
}

fn render(model: &dyn Any, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let model = model.downcast_ref::<WardleyModel>().expect("wardley model");
    render::render_wardley(model, config, options)
}

pub static WARDLEY: Diagram = Diagram {
    type_: "wardley",
    section: Some("wardley-beta"),
    keep_comments: false,
    math: false,
    parse,
    render,
};

#[cfg(test)]
mod tests {
    use super::parser::WARDLEY_TOKENS;

    #[test]
    fn token_kinds_follow_the_table() {
        let lex = &*WARDLEY_TOKENS;
        assert_eq!(lex.index("NAME_WITH_SPACES"), Some(30));
        assert_eq!(lex.index("ACC_DESCR"), Some(32));
        assert_eq!(lex.index("INT"), Some(35));
        assert_eq!(lex.index("ID"), Some(37));
        assert_eq!(lex.index("NEWLINE"), Some(38));
    }
}
