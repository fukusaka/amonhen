//! venn の読み手（pele の `src/diagrams/venn/parser.ts`）。Mermaid の venn の文法を写す。字句解析は字下げした
//! `text` が set の続きかを形に尋ね、それを形に伝えるのは文なので、字句は文法が見る必要のあるときにだけ読む。

use super::db::VennDb;
use super::lexer::{IndentHost, TOKEN_NAMES, VennLexer, t};
use crate::errors::{ErrorCode, PeleError, Result, syntax_error};
use crate::js;

const STATEMENT: &str = "'EOF', 'NEWLINE', 'TITLE', 'SET', 'UNION', 'TEXT', 'INDENT_TEXT', 'STYLE'";
const VALUE: &str = "'NUMERIC', 'IDENTIFIER', 'STRING', 'HEXCOLOR', 'RGBCOLOR', 'RGBACOLOR'";

impl IndentHost for VennDb {
    fn indent_mode(&self) -> bool {
        VennDb::indent_mode(self)
    }

    fn set_indent_mode(&mut self, enabled: bool) {
        VennDb::set_indent_mode(self, enabled)
    }
}

struct Parser<'a> {
    lexer: VennLexer,
    db: &'a mut VennDb,
    type_: i32,
}

impl Parser<'_> {
    fn peek(&mut self) -> i32 {
        if self.type_ == -1 {
            self.type_ = self.lexer.next(self.db);
        }
        self.type_
    }

    fn fail(&mut self, expected: &str) -> PeleError {
        if self.peek() == t::ERROR {
            return syntax_error("venn", self.lexer.src(), self.lexer.start, "", true);
        }
        syntax_error(
            "venn",
            self.lexer.src(),
            self.lexer.start,
            &format!(
                "Expecting {expected}, got '{}'",
                TOKEN_NAMES[self.type_ as usize]
            ),
            false,
        )
    }

    fn take(&mut self) -> String {
        self.type_ = -1;
        self.lexer.text.clone()
    }

    fn expect(&mut self, wanted: i32) -> Result<String> {
        if self.peek() != wanted {
            return Err(self.fail(&format!("'{}'", TOKEN_NAMES[wanted as usize])));
        }
        Ok(self.take())
    }

    fn identifier(&mut self) -> Result<String> {
        if self.peek() != t::IDENTIFIER && self.type_ != t::STRING {
            return Err(self.fail("'IDENTIFIER', 'STRING'"));
        }
        Ok(self.take())
    }

    fn identifiers(&mut self) -> Result<Vec<String>> {
        let mut list = vec![self.identifier()?];
        while self.peek() == t::COMMA {
            self.take();
            list.push(self.identifier()?);
        }
        Ok(list)
    }

    fn subset(&mut self, ids: Vec<String>, union: bool) -> Result<()> {
        let label = if self.peek() == t::BRACKET_LABEL {
            Some(self.take())
        } else {
            None
        };
        let mut size = None;
        if self.peek() == t::COLON {
            self.take();
            size = Some(js::parse_float(&self.expect(t::NUMERIC)?));
        }
        if union {
            if ids.len() < 2 {
                return Err(PeleError::typed(
                    "union requires multiple identifiers",
                    ErrorCode::Semantic,
                    "venn",
                ));
            }
            self.db.validate_union_identifiers(&ids)?;
        }
        self.db.add_subset_data(&ids, label.as_deref(), size);
        self.db.set_indent_mode(true);
        Ok(())
    }

    /// 文字の id と、数でない id が持てる括弧のラベル。`sets` が None なら今の集合。
    fn text(&mut self, sets: Option<Vec<String>>) -> Result<()> {
        let kind = self.peek();
        if kind != t::IDENTIFIER && kind != t::STRING && kind != t::NUMERIC {
            return Err(self.fail("'NUMERIC', 'IDENTIFIER', 'STRING'"));
        }
        let id = self.take();
        let label = if kind != t::NUMERIC && self.peek() == t::BRACKET_LABEL {
            Some(self.take())
        } else {
            None
        };
        let sets = match sets {
            Some(s) => s,
            None => match self.db.current_sets() {
                Some(c) => c.clone(),
                None => {
                    return Err(PeleError::typed(
                        "text requires set",
                        ErrorCode::Semantic,
                        "venn",
                    ));
                }
            },
        };
        self.db.add_text_data(&sets, &id, label.as_deref());
        Ok(())
    }

    fn style(&mut self) -> Result<()> {
        let ids = self.identifiers()?;
        let mut styles: Vec<(String, String)> = Vec::new();
        loop {
            let key = self.expect(t::IDENTIFIER)?;
            self.expect(t::COLON)?;
            let value = if self.peek() == t::STRING {
                self.take()
            } else {
                let mut parts: Vec<String> = Vec::new();
                while self.peek() == t::IDENTIFIER
                    || (self.type_ >= t::NUMERIC && self.type_ <= t::RGBACOLOR)
                {
                    parts.push(self.take());
                }
                if parts.is_empty() {
                    return Err(self.fail(VALUE));
                }
                parts.join(" ")
            };
            styles.push((key, value));
            if self.peek() != t::COMMA {
                break;
            }
            self.take();
        }
        self.db.add_style_data(&ids, &styles);
        Ok(())
    }
}

pub fn parse_venn(src: &str, db: &mut VennDb) -> Result<()> {
    let mut p = Parser {
        lexer: VennLexer::new(src),
        db,
        type_: -1,
    };
    while p.peek() == t::NEWLINE {
        p.take();
    }
    p.expect(t::VENN)?;
    loop {
        let statement = p.peek();
        if statement == t::EOF {
            return Ok(());
        }
        if !(t::NEWLINE..=t::STYLE).contains(&statement) || statement == t::VENN {
            return Err(p.fail(STATEMENT));
        }
        p.take();
        match statement {
            t::TITLE => {
                let text: Vec<u16> = p.lexer.text.encode_utf16().skip(6).collect();
                p.db.set_diagram_title(&js::from16(&text));
            }
            t::SET => {
                let id = p.identifier()?;
                p.subset(vec![id], false)?;
            }
            t::UNION => {
                let ids = p.identifiers()?;
                p.subset(ids, true)?;
            }
            t::TEXT => {
                let ids = p.identifiers()?;
                p.text(Some(ids))?;
            }
            t::INDENT_TEXT => p.text(None)?,
            t::STYLE => p.style()?,
            _ => {}
        }
    }
}
