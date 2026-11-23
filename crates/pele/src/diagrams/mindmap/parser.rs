//! mindmap の読み手（pele の `src/diagrams/mindmap/parser.ts`）。

use crate::diagrams::common::outline::{OutlineDb, Tokens, parse_outline, tokenize as outline};
use crate::errors::Result;
use crate::js;

pub fn tokenize(src: &str) -> Tokens {
    outline(&js::utf16(src), false)
}

pub fn parse_mindmap(src: &str, db: &mut dyn OutlineDb) -> Result<()> {
    parse_outline(src, db, false)
}
