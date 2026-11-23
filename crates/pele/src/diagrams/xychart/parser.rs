//! xychart の読み手（pele の `src/diagrams/xychart/parser.ts`）。Mermaid の Jison の文法を手で写したもの。

use super::db::{DataPoint, XyChartDb, XyText};
use super::lexer::tokenize;
use super::tokens::*;
use crate::errors::{PeleError, Result, syntax_error};
use crate::js;

fn is_word(t: u8) -> bool {
    matches!(
        t,
        AMP | NUM | ALPHA | PLUS | EQUALS | MULT | DOT | BRKT | MINUS | UNDERSCORE
    )
}

fn is_eol(t: u8) -> bool {
    matches!(t, NEWLINE | SEMI | EOF)
}

fn is_text(t: u8) -> bool {
    t == STR || is_word(t)
}

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
            return syntax_error("xychart", &self.src, self.starts[self.i], "", true);
        }
        syntax_error(
            "xychart",
            &self.src,
            self.starts[self.i],
            &format!("Expecting {expected}, got '{}'", TOKEN_NAMES[t as usize]),
            false,
        )
    }

    fn take(&mut self) -> String {
        self.i += 1;
        self.texts[self.i - 1].clone()
    }

    fn expect(&mut self, type_: u8) -> Result<String> {
        if self.t() != type_ {
            return Err(self.fail(&format!("'{}'", TOKEN_NAMES[type_ as usize])));
        }
        Ok(self.take())
    }

    fn text(&mut self) -> Result<XyText> {
        if !is_text(self.t()) {
            return Err(self.fail("'STR', 'ALPHA', 'NUM'"));
        }
        let mut s = self.take();
        if self.types[self.i - 1] != STR {
            while is_word(self.t()) {
                s.push_str(&self.take());
            }
        }
        Ok(XyText { text: s })
    }

    fn range(&mut self) -> Result<(f64, f64)> {
        let min = js::number(&self.expect(NUMBER_WITH_DECIMAL)?);
        self.expect(ARROW_DELIMITER)?;
        Ok((min, js::number(&self.expect(NUMBER_WITH_DECIMAL)?)))
    }

    fn points(&mut self) -> Result<Vec<DataPoint>> {
        let mut out = Vec::new();
        self.expect(SQUARE_BRACES_START)?;
        loop {
            let value = js::number(&self.expect(NUMBER_WITH_DECIMAL)?);
            let label = if self.t() == STR {
                self.take()
            } else {
                String::new()
            };
            out.push(DataPoint { value, label });
            if self.t() != COMMA {
                break;
            }
            self.i += 1;
        }
        self.expect(SQUARE_BRACES_END)?;
        Ok(out)
    }
}

pub fn parse_xy_chart(src: &str, db: &mut XyChartDb) -> Result<()> {
    let src16 = js::utf16(src);
    let tokens = tokenize(&src16);
    let mut r = Parser {
        src: src16,
        types: tokens.types,
        texts: tokens.texts,
        starts: tokens.starts,
        i: 0,
    };

    loop {
        if is_eol(r.t()) {
            r.i += 1;
        } else if r.t() != XYCHART {
            break;
        } else {
            r.i += 1;
            if r.t() == CHART_ORIENTATION {
                let o = r.take();
                db.set_orientation(&o);
            }
        }
    }

    let empty = || XyText {
        text: String::new(),
    };
    while r.t() != END {
        let t = r.t();
        r.i += 1;
        match t {
            TITLE => {
                let text = r.text()?;
                db.set_diagram_title(js::trim(&text.text).to_string());
            }
            X_AXIS | Y_AXIS => {
                let title = if is_text(r.t()) {
                    Some(r.text()?)
                } else {
                    None
                };
                if t == X_AXIS && r.t() == SQUARE_BRACES_START {
                    let mut categories = Vec::new();
                    loop {
                        r.i += 1;
                        categories.push(r.text()?);
                        if r.t() != COMMA {
                            break;
                        }
                    }
                    r.expect(SQUARE_BRACES_END)?;
                    db.set_x_axis_band(&categories);
                } else if r.t() == NUMBER_WITH_DECIMAL || title.is_none() {
                    let (min, max) = r.range()?;
                    if t == X_AXIS {
                        db.set_x_axis_range_data(min, max);
                    } else {
                        db.set_y_axis_range_data(min, max);
                    }
                }
                let title = title.unwrap_or_else(empty);
                if t == X_AXIS {
                    db.set_x_axis_title(&title);
                } else {
                    db.set_y_axis_title(&title);
                }
            }
            LINE | BAR => {
                let title = if r.t() == SQUARE_BRACES_START {
                    empty()
                } else {
                    r.text()?
                };
                let data = r.points()?;
                if t == LINE {
                    db.set_line_data(&title, &data);
                } else {
                    db.set_bar_data(&title, &data);
                }
            }
            ACC_TITLE => {
                let v = r.expect(ACC_TITLE_VALUE)?;
                db.set_acc_title(js::trim(&v));
            }
            ACC_DESCR => {
                let v = r.expect(ACC_DESCR_VALUE)?;
                db.set_acc_description(js::trim(&v));
            }
            ACC_DESCR_MULTILINE_VALUE => {
                let v = r.texts[r.i - 1].clone();
                db.set_acc_description(js::trim(&v));
            }
            _ => {
                r.i -= 1;
                return Err(r.fail("'title', 'X_AXIS', 'Y_AXIS', 'LINE', 'BAR', 'acc_title', 'acc_descr', 'acc_descr_multiline_value'"));
            }
        }
        while is_eol(r.t()) {
            r.i += 1;
        }
    }
    Ok(())
}
