//! block の文法（pele の `src/diagrams/block/parser.ts`）。

use super::db::{
    BlockDb, edge_str_to_edge_data, edge_str_to_edge_start_data, edge_str_to_pattern,
    edge_str_to_thickness, type_str_to_type,
};
use super::lexer::tokenize;
use super::tokens as t;
use super::types::{Block, Statement};
use crate::errors::{PeleError, Result, syntax_error};
use crate::js;

struct NodeInfo {
    id: String,
    label: Option<String>,
    type_str: Option<String>,
    directions: Option<Vec<String>>,
}

struct Frame {
    list: Vec<Statement>,
    header: Option<Statement>,
}

const STATEMENT_START: &str = "'NODE_ID', 'COLUMNS', 'SPACE_BLOCK', 'BLOCK_DIAGRAM_KEY', 'id-block', 'classDef', 'class', 'style'";

/// 文法は、文書の最後の文が並びなら、それを開いて入れる。
fn close_document(mut list: Vec<Statement>) -> Vec<Statement> {
    if let Some(Statement::Many(_)) = list.last()
        && let Some(Statement::Many(last)) = list.pop()
    {
        list.extend(last.into_iter().map(|b| Statement::One(Box::new(b))));
    }
    list
}

/// `parseInt(text, 10)`。字句は数字だけか `-1` なので、正しく丸める `parse` で読める。
fn parse_int(text: &str) -> f64 {
    text.parse::<f64>().unwrap_or(f64::NAN)
}

struct Parser<'a> {
    src: &'a [u16],
    types: Vec<u8>,
    texts: Vec<String>,
    starts: Vec<usize>,
    i: usize,
}

impl Parser<'_> {
    fn kind(&self) -> u8 {
        self.types.get(self.i).copied().unwrap_or(t::END)
    }

    fn fail(&self, expected: &str) -> PeleError {
        let k = self.kind();
        let at = self.starts.get(self.i).copied().unwrap_or(self.src.len());
        if k == t::ERROR {
            return syntax_error("block", self.src, at, "", true);
        }
        syntax_error(
            "block",
            self.src,
            at,
            &format!("Expecting {expected}, got '{}'", t::TOKEN_NAMES[k as usize]),
            false,
        )
    }

    fn expect(&mut self, type_: u8) -> Result<String> {
        if self.kind() != type_ {
            return Err(self.fail(&format!("'{}'", t::TOKEN_NAMES[type_ as usize])));
        }
        let text = self.texts[self.i].clone();
        self.i += 1;
        Ok(text)
    }

    fn take(&mut self) -> String {
        let text = self.texts[self.i].clone();
        self.i += 1;
        text
    }

    fn node(&mut self) -> Result<NodeInfo> {
        let id = self.expect(t::NODE_ID)?;
        if self.kind() == t::NODE_DSTART {
            let open = self.take();
            let label = self.expect(t::STR)?;
            let close = self.expect(t::NODE_DEND)?;
            return Ok(NodeInfo {
                id,
                label: Some(label),
                type_str: Some(open + &close),
                directions: None,
            });
        }
        if self.kind() == t::BLOCK_ARROW_START {
            let open = self.take();
            let label = self.expect(t::STR)?;
            let mut directions = vec![self.expect(t::DIR)?];
            while self.kind() == t::DIR {
                directions.push(self.take());
            }
            let close = self.expect(t::BLOCK_ARROW_END)?;
            return Ok(NodeInfo {
                id,
                label: Some(label),
                type_str: Some(open + &close),
                directions: Some(directions),
            });
        }
        Ok(NodeInfo {
            id,
            label: None,
            type_str: None,
            directions: None,
        })
    }

    fn node_statement(&mut self) -> Result<Statement> {
        let first = self.node()?;
        let size = if self.kind() == t::SIZE {
            let text = self.take();
            parse_int(&text)
        } else {
            1.0
        };
        let mut from = Block {
            id: Some(first.id),
            label: first.label,
            type_: Some(type_str_to_type(first.type_str.as_deref()).into()),
            directions: first.directions,
            width_in_columns: Some(size),
            ..Default::default()
        };
        let mut chain: Option<Vec<Block>> = None;
        while self.kind() == t::LINK || self.kind() == t::START_LINK {
            let mut label = String::new();
            let k = self.kind();
            self.i += 1;
            if k == t::START_LINK {
                self.expect(t::LINK_LABEL)?;
                label = self.expect(t::STR)?;
                self.expect(t::LINK)?;
            }
            let link = self.texts[self.i - 1].clone();
            let to = self.node()?;
            let arrow_type_end = edge_str_to_edge_data(&link);
            let arrow_type_start = edge_str_to_edge_start_data(&link);
            let thickness = edge_str_to_thickness(&link);
            let pattern = edge_str_to_pattern(&link);
            // Mermaid は `A --> B --> C` の最後のリンクだけを残し、その始まりも失う。ここではどのリンクも数える。
            let chain = chain.get_or_insert_with(|| {
                vec![Block {
                    id: from.id.clone(),
                    label: from.label.clone(),
                    type_: from.type_.clone(),
                    directions: from.directions.clone(),
                    ..Default::default()
                }]
            });
            from = Block {
                id: Some(to.id.clone()),
                label: to.label,
                type_: Some(type_str_to_type(to.type_str.as_deref()).into()),
                directions: to.directions.clone(),
                ..Default::default()
            };
            let prev = chain.last().unwrap().id.clone().unwrap_or_default();
            chain.push(Block {
                id: Some(format!("{prev}-{}", to.id)),
                start: Some(prev),
                end: Some(to.id),
                label: Some(label),
                type_: Some("edge".into()),
                thickness: Some(thickness.into()),
                pattern: Some(pattern.into()),
                directions: to.directions,
                arrow_type_end: Some(arrow_type_end.into()),
                arrow_type_start: Some(arrow_type_start.into()),
                ..Default::default()
            });
            chain.push(from.clone());
        }
        Ok(match chain {
            Some(c) => Statement::Many(c),
            None => Statement::One(Box::new(from)),
        })
    }
}

pub fn parse_block(src: &[u16], db: &mut BlockDb) -> Result<()> {
    let tokens = tokenize(src);
    let mut p = Parser {
        src,
        types: tokens.types,
        texts: tokens.texts,
        starts: tokens.starts,
        i: 0,
    };
    p.expect(t::KEY)?;
    // まとまりは再帰せずに入れ子にする。開いているまとまりは、それぞれ親の文の並びを持つ。
    let mut open: Vec<Frame> = Vec::new();
    let mut list: Vec<Statement> = Vec::new();
    loop {
        match p.kind() {
            t::NODE_ID => list.push(p.node_statement()?),
            t::COLUMNS => {
                let text = p.take();
                list.push(Statement::One(Box::new(Block {
                    type_: Some("column-setting".into()),
                    columns: Some(parse_int(&text)),
                    ..Default::default()
                })));
            }
            t::SPACE_BLOCK => {
                let text = p.take();
                let width = parse_int(&text);
                list.push(Statement::One(Box::new(Block {
                    id: Some(db.generate_id()),
                    type_: Some("space".into()),
                    label: Some(String::new()),
                    width: Some(width),
                    inner: Some(Vec::new()),
                    ..Default::default()
                })));
            }
            t::KEY => {
                p.i += 1;
                open.push(Frame {
                    list: std::mem::take(&mut list),
                    header: None,
                });
            }
            t::ID_BLOCK => {
                p.i += 1;
                if p.kind() != t::NODE_ID {
                    return Err(p.fail("'NODE_ID'"));
                }
                let header = p.node_statement()?;
                open.push(Frame {
                    list: std::mem::take(&mut list),
                    header: Some(header),
                });
            }
            t::END_KW => {
                if open.is_empty() || list.is_empty() {
                    return Err(p.fail(STATEMENT_START));
                }
                let frame = open.pop().unwrap();
                p.i += 1;
                let id = db.generate_id();
                let children = close_document(std::mem::take(&mut list));
                list = frame.list;
                list.push(Statement::One(Box::new(match frame.header {
                    None => Block {
                        id: Some(id),
                        type_: Some("composite".into()),
                        label: Some(String::new()),
                        inner: Some(children),
                        ..Default::default()
                    },
                    Some(Statement::One(header)) => Block {
                        type_: Some("composite".into()),
                        inner: Some(children),
                        ..*header
                    },
                    // 並びを広げたものには名前もラベルもない。
                    Some(Statement::Many(_)) => Block {
                        type_: Some("composite".into()),
                        inner: Some(children),
                        ..Default::default()
                    },
                })));
            }
            t::CLASSDEF => {
                p.i += 1;
                let id = js::trim(&p.expect(t::CLASSDEF_ID)?).to_string();
                let css = js::trim(&p.expect(t::CLASSDEF_STYLEOPTS)?).to_string();
                list.push(Statement::One(Box::new(Block {
                    type_: Some("classDef".into()),
                    id: Some(id),
                    css: Some(css),
                    ..Default::default()
                })));
            }
            t::CLASS => {
                p.i += 1;
                let id = js::trim(&p.expect(t::CLASSENTITY_IDS)?).to_string();
                let style_class = js::trim(&p.expect(t::STYLECLASS)?).to_string();
                list.push(Statement::One(Box::new(Block {
                    type_: Some("applyClass".into()),
                    id: Some(id),
                    style_class: Some(style_class),
                    ..Default::default()
                })));
            }
            t::STYLE => {
                p.i += 1;
                let id = js::trim(&p.expect(t::STYLE_ENTITY_IDS)?).to_string();
                let styles = js::trim(&p.expect(t::STYLE_DEFINITION_DATA)?).to_string();
                list.push(Statement::One(Box::new(Block {
                    type_: Some("applyStyles".into()),
                    id: Some(id),
                    styles_str: Some(styles),
                    ..Default::default()
                })));
            }
            t::EOF => {
                if !open.is_empty() {
                    return Err(p.fail(&format!("{STATEMENT_START}, 'end'")));
                }
                if list.is_empty() {
                    return Err(p.fail(STATEMENT_START));
                }
                return db.set_hierarchy(close_document(list));
            }
            _ => {
                return Err(p.fail(&if list.is_empty() {
                    STATEMENT_START.to_string()
                } else if !open.is_empty() {
                    format!("{STATEMENT_START}, 'end'")
                } else {
                    format!("{STATEMENT_START}, 'EOF'")
                }));
            }
        }
    }
}
