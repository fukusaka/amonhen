//! quadrantChart の読み手（pele の `src/diagrams/quadrant/parser.ts`）。Mermaid の Jison の文法を手で写したもの。

use super::db::{QuadrantDb, QuadrantText};
use super::lexer::tokenize;
use super::tokens::*;
use crate::errors::{PeleError, Result, syntax_error};
use crate::js;

fn is_word(t: u8) -> bool {
    matches!(
        t,
        PUNCTUATION
            | AMP
            | NUM
            | ALPHA
            | COMMA
            | PLUS
            | EQUALS
            | MULT
            | DOT
            | BRKT
            | UNDERSCORE
            | UNICODE_TEXT
    )
}

fn is_id(t: u8) -> bool {
    matches!(
        t,
        ALPHA | NUM | MINUS | COMMA | COLON | AMP | BRKT | MULT | UNICODE_TEXT
    )
}

fn is_style(t: u8) -> bool {
    matches!(t, ALPHA | NUM | COLON | SPACE | BRKT | MINUS)
}

fn is_eol(t: u8) -> bool {
    matches!(t, NEWLINE | SEMI | EOF)
}

const TEXT_TOKENS: &str = "'STR', 'MD_STR', 'ALPHA', 'NUM', 'UNICODE_TEXT', 'PUNCTUATION'";

struct Parser {
    src: Vec<u16>,
    types: Vec<u8>,
    texts: Vec<String>,
    starts: Vec<usize>,
    i: usize,
}

impl Parser {
    fn t(&self) -> u8 {
        self.types.get(self.i).copied().unwrap_or(u8::MAX)
    }

    fn fail(&self, expected: &str) -> PeleError {
        let t = self.t();
        if t == ERROR {
            return syntax_error("quadrantChart", &self.src, self.starts[self.i], "", true);
        }
        syntax_error(
            "quadrantChart",
            &self.src,
            self.starts[self.i],
            &format!("Expecting {expected}, got '{}'", TOKEN_NAMES[t as usize]),
            false,
        )
    }

    fn expect(&mut self, type_: u8) -> Result<String> {
        if self.t() != type_ {
            return Err(self.fail(&format!("'{}'", TOKEN_NAMES[type_ as usize])));
        }
        self.i += 1;
        Ok(self.texts[self.i - 1].clone())
    }

    fn is_text(t: u8) -> bool {
        t == STR || t == MD_STR || is_word(t)
    }

    fn take(&mut self) -> &str {
        self.i += 1;
        &self.texts[self.i - 1]
    }

    fn text(&mut self) -> Result<QuadrantText> {
        let first = self.t();
        if !Self::is_text(first) {
            return Err(self.fail(TEXT_TOKENS));
        }
        let mut s = self.take().to_string();
        while is_word(self.t()) || self.t() == SPACE || self.t() == MINUS {
            let t = self.take().to_string();
            s.push_str(&t);
        }
        Ok(QuadrantText {
            text: s,
            markdown: first == MD_STR,
        })
    }

    fn styles(&mut self) -> Result<Vec<String>> {
        let mut out = Vec::new();
        loop {
            if !is_style(self.t()) {
                return Err(self.fail("'ALPHA', 'NUM', 'COLON', 'SPACE', 'BRKT', 'MINUS'"));
            }
            let mut s = self.take().to_string();
            while is_style(self.t()) {
                let t = self.take().to_string();
                s.push_str(&t);
            }
            out.push(js::trim(&s).to_string());
            if self.t() != COMMA {
                return Ok(out);
            }
            self.i += 1;
        }
    }
}

pub fn parse_quadrant(src: &str, db: &mut QuadrantDb) -> Result<()> {
    let src16 = js::utf16(src);
    let tokens = tokenize(&src16);
    let mut r = Parser {
        src: src16,
        types: tokens.types,
        texts: tokens.texts,
        starts: tokens.starts,
        i: 0,
    };

    while is_eol(r.t()) || r.t() == SPACE {
        r.i += 1;
    }
    r.expect(QUADRANT)?;

    while r.t() != END {
        while r.t() == SPACE {
            r.i += 1;
        }
        let t = r.t();
        match t {
            CLASSDEF => {
                r.i += 1;
                r.expect(SPACE)?;
                if !is_id(r.t()) {
                    return Err(r.fail("'ALPHA', 'NUM', 'UNICODE_TEXT'"));
                }
                let mut id = r.take().to_string();
                while is_id(r.t()) {
                    let t = r.take().to_string();
                    id.push_str(&t);
                }
                r.expect(SPACE)?;
                let styles = r.styles()?;
                db.add_class(id, &styles)?;
            }
            X_AXIS | Y_AXIS => {
                r.i += 1;
                let mut from = r.text()?;
                let mut to = None;
                if r.t() == DELIMITER {
                    r.i += 1;
                    if Parser::is_text(r.t()) {
                        to = Some(r.text()?);
                    } else {
                        from.text.push_str(" ⟶ ");
                    }
                }
                if t == X_AXIS {
                    db.set_x_axis_left_text(from);
                    if let Some(to) = to {
                        db.set_x_axis_right_text(to);
                    }
                } else {
                    db.set_y_axis_bottom_text(from);
                    if let Some(to) = to {
                        db.set_y_axis_top_text(to);
                    }
                }
            }
            QUADRANT_1..=QUADRANT_4 => {
                r.i += 1;
                let text = r.text()?;
                db.set_quadrant_text((t - QUADRANT_1) as usize, text);
            }
            TITLE => {
                r.i += 1;
                let v = r.expect(TITLE_VALUE)?;
                db.set_diagram_title(js::trim(&v).to_string());
            }
            ACC_TITLE => {
                r.i += 1;
                let v = r.expect(ACC_TITLE_VALUE)?;
                db.set_acc_title(js::trim(&v));
            }
            ACC_DESCR => {
                r.i += 1;
                let v = r.expect(ACC_DESCR_VALUE)?;
                db.set_acc_description(js::trim(&v));
            }
            ACC_DESCR_MULTILINE_VALUE => {
                let v = r.take().to_string();
                db.set_acc_description(js::trim(&v));
            }
            _ => {
                if Parser::is_text(t) {
                    let label = r.text()?;
                    let class_name = if r.t() == CLASS_NAME {
                        r.take().to_string()
                    } else {
                        String::new()
                    };
                    r.expect(POINT_START)?;
                    let x = r.expect(POINT_X)?;
                    let y = r.expect(POINT_Y)?;
                    let styles = if is_style(r.t()) {
                        r.styles()?
                    } else {
                        Vec::new()
                    };
                    db.add_point(label, class_name, &x, &y, &styles)?;
                }
            }
        }
        if !is_eol(r.t()) {
            return Err(r.fail("'NEWLINE', 'SEMI', 'EOF'"));
        }
        r.i += 1;
    }
    Ok(())
}
