//! state の読み手（pele の `src/diagrams/state/parser.ts`）。

use super::db::StateDb;
use super::lexer::tokenize;
use super::tokens::{self as t, TOKEN_NAMES};
use super::types::{Desc, StateNote, StateStmt, Stmt};
use crate::errors::{ErrorCode, PeleError, Result, locate, syntax_error};
use crate::js;

const DIRECTIONS: [&str; 4] = ["TB", "BT", "RL", "LR"];
const STATEMENT: &str = "'NL', 'ID', 'HIDE_EMPTY', 'scale', 'COMPOSIT_STATE', 'STATE_DESCR', 'FORK', 'JOIN', 'CHOICE', 'CONCURRENT', 'note', 'acc_title', 'acc_descr', 'acc_descr_multiline_value', 'CLICK', 'classDef', 'style', 'class', 'direction_tb', 'direction_bt', 'direction_rl', 'direction_lr', 'EDGE_STATE'";

struct Parser<'a> {
    src: &'a [u16],
    types: Vec<u8>,
    texts: Vec<String>,
    starts: Vec<usize>,
    error: Option<String>,
    i: usize,
}

impl Parser<'_> {
    fn ty(&self) -> u8 {
        self.types[self.i]
    }

    fn take(&mut self) -> String {
        let s = std::mem::take(&mut self.texts[self.i]);
        self.i += 1;
        s
    }

    fn fail<T>(&self, expected: &str) -> Result<T> {
        let ty = self.ty();
        let at = self.starts[self.i];
        if ty == t::ERROR {
            if let Some(error) = &self.error {
                let loc = locate(self.src, at);
                let mut e = PeleError::typed(error.clone(), ErrorCode::Syntax, "state");
                e.line = loc.line;
                e.column = loc.column;
                e.snippet = loc.snippet;
                return Err(e);
            }
            return Err(syntax_error("state", self.src, at, "", true));
        }
        Err(syntax_error(
            "state",
            self.src,
            at,
            &format!("Expecting {expected}, got '{}'", TOKEN_NAMES[ty as usize]),
            false,
        ))
    }

    fn expect(&mut self, ty: u8) -> Result<String> {
        if self.ty() != ty {
            return self.fail(&format!("'{}'", TOKEN_NAMES[ty as usize]));
        }
        Ok(self.take())
    }

    fn id_statement(&mut self) -> Result<StateStmt> {
        let ty = self.ty();
        if ty != t::ID && ty != t::EDGE_STATE {
            return self.fail("'ID', 'EDGE_STATE'");
        }
        let id = js::trim(&self.take()).to_string();
        let mut stmt = StateStmt {
            id,
            type_: Some("default".into()),
            description: Some(Desc::One(String::new())),
            ..Default::default()
        };
        if self.ty() != t::STYLE_SEPARATOR {
            return Ok(stmt);
        }
        self.i += 1;
        let class = js::trim(&self.expect(t::ID)?).to_string();
        stmt.classes = Some(vec![class]);
        Ok(stmt)
    }
}

fn trim_colon(text: &str) -> String {
    match text.strip_prefix(':') {
        Some(rest) => js::trim(rest).to_string(),
        None => js::trim(text).to_string(),
    }
}

pub fn parse_state(source: &str, db: &mut StateDb) -> Result<()> {
    let src: Vec<u16> = source.encode_utf16().collect();
    let tokens = tokenize(&src);
    let mut p = Parser {
        src: &src,
        types: tokens.types,
        texts: tokens.texts,
        starts: tokens.starts,
        error: tokens.error,
        i: 0,
    };

    while p.ty() == t::NL {
        p.i += 1;
    }
    if p.ty() != t::SD {
        return p.fail("'NL', 'SD'");
    }
    p.i += 1;

    let mut doc: Vec<Stmt> = Vec::new();
    // 開いている合成の状態の、外の文書と、その状態。
    let mut outer: Vec<(Vec<Stmt>, usize)> = Vec::new();
    // 文法は、空の行に使う文字列を値とする文を捨てる。
    let add = |doc: &mut Vec<Stmt>, value: Stmt| {
        if !matches!(&value, Stmt::Text(s) if s == "nl") {
            doc.push(value);
        }
    };
    macro_rules! open {
        ($id:expr, $description:expr) => {{
            p.i += 1;
            let k = db.item(StateStmt {
                id: $id,
                type_: Some("default".into()),
                description: Some(Desc::One($description)),
                doc: Some(Vec::new()),
                ..Default::default()
            });
            doc.push(Stmt::State(k));
            outer.push((std::mem::take(&mut doc), k));
        }};
    }

    loop {
        let ty = p.ty();
        match ty {
            t::NL => p.i += 1,
            t::ID | t::EDGE_STATE => {
                let mut first = p.id_statement()?;
                if p.ty() == t::DESCR {
                    first.description = Some(Desc::One(trim_colon(&p.take())));
                    let k = db.item(first);
                    add(&mut doc, Stmt::State(k));
                } else if p.ty() == t::ARROW {
                    p.i += 1;
                    let second = p.id_statement()?;
                    let description = if p.ty() == t::DESCR {
                        Some(trim_colon(&p.take()))
                    } else {
                        None
                    };
                    let state1 = db.item(first);
                    let state2 = db.item(second);
                    add(
                        &mut doc,
                        Stmt::Relation {
                            state1,
                            state2,
                            description,
                        },
                    );
                } else {
                    let k = db.item(first);
                    add(&mut doc, Stmt::State(k));
                }
            }
            t::HIDE_EMPTY => {
                let s = p.take();
                add(&mut doc, Stmt::Text(s));
            }
            t::scale => {
                p.i += 1;
                p.expect(t::WIDTH)?;
                let s = p.texts[p.i - 2].clone();
                add(&mut doc, Stmt::Text(s));
            }
            t::COMPOSIT_STATE => {
                if p.types.get(p.i + 1) == Some(&t::STRUCT_START) {
                    let id = p.take();
                    open!(id, String::new());
                } else {
                    let s = p.take();
                    add(&mut doc, Stmt::Text(s));
                }
            }
            t::STATE_DESCR => {
                let description = p.take();
                p.expect(t::AS)?;
                let id = p.expect(t::ID)?;
                if p.ty() == t::STRUCT_START {
                    open!(id, description);
                } else if id.contains(':') {
                    let mut parts = id.split(':');
                    let first = parts.next().unwrap_or("").to_string();
                    let second = parts.next().unwrap_or("").to_string();
                    let k = db.item(StateStmt {
                        id: first,
                        type_: Some("default".into()),
                        description: Some(Desc::Many(vec![
                            js::trim(&description).to_string(),
                            second,
                        ])),
                        ..Default::default()
                    });
                    add(&mut doc, Stmt::State(k));
                } else {
                    let k = db.item(StateStmt {
                        id,
                        type_: Some("default".into()),
                        description: Some(Desc::One(js::trim(&description).to_string())),
                        ..Default::default()
                    });
                    add(&mut doc, Stmt::State(k));
                }
            }
            t::FORK | t::JOIN | t::CHOICE => {
                let id = p.take();
                let k = db.item(StateStmt {
                    id,
                    type_: Some(
                        match ty {
                            t::FORK => "fork",
                            t::JOIN => "join",
                            _ => "choice",
                        }
                        .into(),
                    ),
                    ..Default::default()
                });
                add(&mut doc, Stmt::State(k));
            }
            t::CONCURRENT => {
                p.i += 1;
                let id = db.get_divider_id();
                let k = db.item(StateStmt {
                    id,
                    type_: Some("divider".into()),
                    ..Default::default()
                });
                add(&mut doc, Stmt::State(k));
            }
            t::note => {
                let keyword = p.take();
                if p.ty() == t::left_of || p.ty() == t::right_of {
                    let position = js::trim(&p.take()).to_string();
                    let id = js::trim(&p.expect(t::ID)?).to_string();
                    let text = js::trim(&p.expect(t::NOTE_TEXT)?).to_string();
                    let k = db.item(StateStmt {
                        id,
                        note: Some(StateNote {
                            position: Some(position),
                            text,
                        }),
                        ..Default::default()
                    });
                    add(&mut doc, Stmt::State(k));
                } else {
                    if p.ty() != t::NOTE_TEXT {
                        return p.fail("'NOTE_TEXT', 'left_of', 'right_of'");
                    }
                    p.i += 1;
                    p.expect(t::AS)?;
                    p.expect(t::ID)?;
                    add(&mut doc, Stmt::Text(keyword));
                }
            }
            t::direction_tb | t::direction_bt | t::direction_rl | t::direction_lr => {
                let value = DIRECTIONS[(ty - t::direction_tb) as usize].to_string();
                p.i += 1;
                add(&mut doc, Stmt::Dir(value));
            }
            t::acc_title => {
                p.i += 1;
                let value = js::trim(&p.expect(t::acc_title_value)?).to_string();
                db.set_acc_title(&value);
                add(&mut doc, Stmt::Text(value));
            }
            t::acc_descr => {
                p.i += 1;
                let value = js::trim(&p.expect(t::acc_descr_value)?).to_string();
                db.set_acc_description(&value);
                add(&mut doc, Stmt::Text(value));
            }
            t::acc_descr_multiline_value => {
                let value = js::trim(&p.take()).to_string();
                db.set_acc_description(&value);
                add(&mut doc, Stmt::Text(value));
            }
            t::CLICK => {
                p.i += 1;
                let id = p.id_statement()?;
                let url;
                let mut tooltip = String::new();
                if p.ty() == t::HREF {
                    p.i += 1;
                    url = p.expect(t::STRING)?;
                } else {
                    if p.ty() != t::STRING {
                        return p.fail("'STRING', 'HREF'");
                    }
                    url = p.take();
                    tooltip = p.expect(t::STRING)?;
                }
                p.expect(t::NL)?;
                let k = db.item(id);
                add(
                    &mut doc,
                    Stmt::Click {
                        id: k,
                        url,
                        tooltip,
                    },
                );
            }
            t::classDef => {
                p.i += 1;
                let id = js::trim(&p.expect(t::CLASSDEF_ID)?).to_string();
                let classes = js::trim(&p.expect(t::CLASSDEF_STYLEOPTS)?).to_string();
                add(&mut doc, Stmt::ClassDef { id, classes });
            }
            t::style => {
                p.i += 1;
                let id = js::trim(&p.expect(t::STYLE_IDS)?).to_string();
                let style_class = js::trim(&p.expect(t::STYLEDEF_STYLEOPTS)?).to_string();
                add(&mut doc, Stmt::Style { id, style_class });
            }
            t::class => {
                p.i += 1;
                let id = js::trim(&p.expect(t::CLASSENTITY_IDS)?).to_string();
                let style_class = js::trim(&p.expect(t::STYLECLASS)?).to_string();
                add(&mut doc, Stmt::ApplyClass { id, style_class });
            }
            t::STRUCT_STOP => {
                if outer.is_empty() {
                    return p.fail(&format!("'$end', {STATEMENT}"));
                }
                p.i += 1;
                let (up, k) = outer.pop().unwrap();
                db.items[k].doc = Some(std::mem::replace(&mut doc, up));
            }
            t::END => {
                if !outer.is_empty() {
                    return p.fail(&format!("'STRUCT_STOP', {STATEMENT}"));
                }
                return db.set_root_doc(doc);
            }
            _ => {
                return p.fail(&if outer.is_empty() {
                    format!("'$end', {STATEMENT}")
                } else {
                    format!("'STRUCT_STOP', {STATEMENT}")
                });
            }
        }
    }
}
