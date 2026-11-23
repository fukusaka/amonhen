//! requirement の構文解析（pele の `src/diagrams/requirement/parser.ts`）。知らせの文は Mermaid（Jison）と同じ。

use super::db::{
    RequirementDb, relationships as rel, requirement_type as rt, risk_level as risk,
    verify_type as vt,
};
use super::lexer::tokenize;
use super::tokens::{self as t, TOKEN_NAMES};
use crate::errors::{PeleError, Result, syntax_error};

/// キーワードの字句が表す値。REQUIREMENT から TRACES まで字句の順に。
const VALUES: [&str; 21] = [
    rt::REQUIREMENT,
    rt::FUNCTIONAL_REQUIREMENT,
    rt::INTERFACE_REQUIREMENT,
    rt::PERFORMANCE_REQUIREMENT,
    rt::PHYSICAL_REQUIREMENT,
    rt::DESIGN_CONSTRAINT,
    risk::LOW_RISK,
    risk::MED_RISK,
    risk::HIGH_RISK,
    vt::VERIFY_ANALYSIS,
    vt::VERIFY_DEMONSTRATION,
    vt::VERIFY_INSPECTION,
    vt::VERIFY_TEST,
    "",
    rel::CONTAINS,
    rel::COPIES,
    rel::DERIVES,
    rel::SATISFIES,
    rel::VERIFIES,
    rel::REFINES,
    rel::TRACES,
];

fn is_style_part(ty: u8) -> bool {
    matches!(ty, t::ALPHA | t::COLON | t::BRKT | t::MINUS | t::SEMICOLON)
}

struct Parser<'a> {
    src: &'a [u16],
    types: Vec<u8>,
    texts: Vec<String>,
    starts: Vec<usize>,
    i: usize,
}

impl Parser<'_> {
    /// Jison の文で失敗する。`from` から `to` までの字句の種類と `others` を待っていたものとして挙げる。
    fn fail(&self, from: u8, to: u8, others: &[u8]) -> PeleError {
        let ty = self.types[self.i];
        if ty == t::ERROR {
            return syntax_error("requirement", self.src, self.starts[self.i], "", true);
        }
        let expected: Vec<String> = (from..=to)
            .chain(others.iter().copied())
            .map(|k| format!("'{}'", TOKEN_NAMES[k as usize]))
            .collect();
        syntax_error(
            "requirement",
            self.src,
            self.starts[self.i],
            &format!(
                "Expecting {}, got '{}'",
                expected.join(", "),
                TOKEN_NAMES[ty as usize]
            ),
            false,
        )
    }

    fn ty(&self) -> u8 {
        self.types[self.i]
    }

    fn take(&mut self) -> String {
        let s = std::mem::take(&mut self.texts[self.i]);
        self.i += 1;
        s
    }

    fn expect(&mut self, ty: u8) -> Result<String> {
        if self.ty() != ty {
            return Err(self.fail(ty, ty, &[]));
        }
        Ok(self.take())
    }

    /// 次の字句が種類の範囲のキーワードなら、その値。
    fn keyword(&mut self, from: u8, to: u8) -> Result<&'static str> {
        let ty = self.ty();
        if ty < from || ty > to {
            return Err(self.fail(from, to, &[]));
        }
        self.i += 1;
        Ok(VALUES[(ty - t::REQUIREMENT) as usize])
    }

    fn string(&mut self) -> Result<String> {
        let ty = self.ty();
        if ty != t::unqString && ty != t::qString {
            return Err(self.fail(t::qString, t::unqString, &[]));
        }
        Ok(self.take())
    }

    fn directive(&mut self, db: &mut RequirementDb) -> Result<bool> {
        let ty = self.ty();
        if ty == t::acc_title {
            self.i += 1;
            let v = self.expect(t::acc_title_value)?;
            db.set_acc_title(crate::js::trim(&v));
        } else if ty == t::acc_descr {
            self.i += 1;
            let v = self.expect(t::acc_descr_value)?;
            db.set_acc_description(crate::js::trim(&v));
        } else if ty == t::acc_descr_multiline_value {
            let v = self.take();
            db.set_acc_description(crate::js::trim(&v));
        } else {
            return Ok(false);
        }
        Ok(true)
    }

    fn id_list(&mut self) -> Result<Vec<String>> {
        let mut list = Vec::new();
        loop {
            let ty = self.ty();
            if ty != t::ALPHA && ty != t::unqString && ty != t::qString {
                return Err(self.fail(t::qString, t::unqString, &[t::ALPHA]));
            }
            list.push(self.take());
            if self.ty() != t::COMMA {
                return Ok(list);
            }
            self.i += 1;
        }
    }

    fn styles_opt(&mut self) -> Result<Vec<String>> {
        let mut styles = Vec::new();
        loop {
            if !is_style_part(self.ty()) {
                return Err(self.fail(t::ALPHA, t::SEMICOLON, &[t::MINUS, t::BRKT]));
            }
            let mut style = self.take();
            while is_style_part(self.ty()) {
                style.push_str(&self.take());
            }
            styles.push(style);
            if self.ty() != t::COMMA {
                return Ok(styles);
            }
            self.i += 1;
        }
    }

    fn class_list(&mut self) -> Result<Option<Vec<String>>> {
        let mut classes = None;
        if self.ty() == t::STYLE_SEPARATOR {
            self.i += 1;
            classes = Some(self.id_list()?);
            self.expect(t::STRUCT_START)?;
        } else if self.ty() == t::STRUCT_START {
            self.i += 1;
        } else {
            return Err(self.fail(t::STRUCT_START, t::STRUCT_START, &[t::STYLE_SEPARATOR]));
        }
        self.expect(t::NEWLINE)?;
        Ok(classes)
    }

    /// 閉じ括弧までの本体を読む。文法は右再帰なので、Mermaid は項目を後ろから当て、同じ項目が 2 つあれば最初のものが
    /// 勝つ。
    fn body(&mut self, db: &mut RequirementDb, requirement: bool) -> Result<()> {
        let mut fields: Vec<u8> = Vec::new();
        let mut values: Vec<String> = Vec::new();
        loop {
            let ty = self.ty();
            if ty == t::STRUCT_STOP {
                self.i += 1;
                break;
            }
            if ty == t::NEWLINE {
                self.i += 1;
                continue;
            }
            let wrong = if requirement {
                !(t::ID..=t::VERIFYMTHD).contains(&ty)
            } else {
                ty != t::TYPE && ty != t::DOCREF
            };
            if wrong {
                if requirement {
                    return Err(self.fail(t::ID, t::VERIFYMTHD, &[t::NEWLINE, t::STRUCT_STOP]));
                }
                return Err(self.fail(t::TYPE, t::DOCREF, &[t::NEWLINE, t::STRUCT_STOP]));
            }
            self.i += 1;
            self.expect(t::COLONSEP)?;
            let value = if ty == t::RISK {
                self.keyword(t::LOW_RISK, t::HIGH_RISK)?.to_string()
            } else if ty == t::VERIFYMTHD {
                self.keyword(t::VERIFY_ANALYSIS, t::VERIFY_TEST)?
                    .to_string()
            } else {
                self.string()?
            };
            values.push(value);
            fields.push(ty);
            self.expect(t::NEWLINE)?;
        }
        for k in (0..fields.len()).rev() {
            let value = &values[k];
            match fields[k] {
                t::ID => db.set_new_req_id(value),
                t::TEXT => db.set_new_req_text(value),
                t::RISK => db.set_new_req_risk(value),
                t::VERIFYMTHD => db.set_new_req_verify_method(value),
                t::TYPE => db.set_new_element_type(value),
                _ => db.set_new_element_doc_ref(value),
            }
        }
        Ok(())
    }
}

pub fn parse_requirement(source: &str, db: &mut RequirementDb) -> Result<()> {
    let src: Vec<u16> = source.encode_utf16().collect();
    let tokens = tokenize(&src);
    let mut p = Parser {
        src: &src,
        types: tokens.types,
        texts: tokens.texts,
        starts: tokens.starts,
        i: 0,
    };

    while p.directive(db)? {
        if p.ty() == t::NEWLINE {
            p.i += 1;
        }
    }
    if p.ty() != t::RD {
        return Err(p.fail(
            t::RD,
            t::RD,
            &[t::acc_title, t::acc_descr, t::acc_descr_multiline_value],
        ));
    }
    p.i += 1;
    p.expect(t::NEWLINE)?;

    loop {
        let ty = p.ty();
        if ty == t::EOF {
            return Ok(());
        }
        if ty == t::NEWLINE {
            p.i += 1;
        } else if p.directive(db)? {
            continue;
        } else if (t::direction_tb..=t::direction_lr).contains(&ty) {
            p.i += 1;
            db.set_direction(match ty {
                t::direction_tb => "TB",
                t::direction_bt => "BT",
                t::direction_rl => "RL",
                _ => "LR",
            });
        } else if (t::REQUIREMENT..=t::DESIGN_CONSTRAINT).contains(&ty) {
            let type_ = p.keyword(t::REQUIREMENT, t::DESIGN_CONSTRAINT)?;
            let name = p.string()?;
            let classes = p.class_list()?;
            p.body(db, true)?;
            db.add_requirement(&name, type_);
            if let Some(classes) = classes {
                db.set_class(&[name], &classes)?;
            }
        } else if ty == t::ELEMENT {
            p.i += 1;
            let name = p.string()?;
            let classes = p.class_list()?;
            p.body(db, false)?;
            db.add_element(&name);
            if let Some(classes) = classes {
                db.set_class(&[name], &classes)?;
            }
        } else if ty == t::STYLE {
            p.i += 1;
            let ids = p.id_list()?;
            let styles = p.styles_opt()?;
            db.set_css_style(&ids, &styles)?;
        } else if ty == t::CLASSDEF {
            p.i += 1;
            let ids = p.id_list()?;
            let styles = p.styles_opt()?;
            db.define_class(&ids, &styles)?;
        } else if ty == t::CLASS {
            p.i += 1;
            let ids = p.id_list()?;
            let names = p.id_list()?;
            db.set_class(&ids, &names)?;
        } else if ty == t::unqString || ty == t::qString {
            let id = p.take();
            let next = p.ty();
            if next == t::STYLE_SEPARATOR {
                p.i += 1;
                let names = p.id_list()?;
                db.set_class(&[id], &names)?;
            } else if next == t::END_ARROW_L || next == t::LINE {
                p.i += 1;
                let relationship = p.keyword(t::CONTAINS, t::TRACES)?;
                p.expect(if next == t::LINE {
                    t::END_ARROW_R
                } else {
                    t::LINE
                })?;
                let other = p.string()?;
                if next == t::LINE {
                    db.add_relationship(relationship, &id, &other);
                } else {
                    db.add_relationship(relationship, &other, &id);
                }
            } else {
                return Err(p.fail(
                    t::STYLE_SEPARATOR,
                    t::STYLE_SEPARATOR,
                    &[t::END_ARROW_L, t::LINE],
                ));
            }
        } else {
            return Err(p.fail(
                t::NEWLINE,
                t::EOF,
                &[
                    t::acc_title,
                    t::acc_descr,
                    t::acc_descr_multiline_value,
                    t::direction_tb,
                    t::direction_bt,
                    t::direction_rl,
                    t::direction_lr,
                    t::REQUIREMENT,
                    t::FUNCTIONAL_REQUIREMENT,
                    t::INTERFACE_REQUIREMENT,
                    t::PERFORMANCE_REQUIREMENT,
                    t::PHYSICAL_REQUIREMENT,
                    t::DESIGN_CONSTRAINT,
                    t::ELEMENT,
                    t::CLASSDEF,
                    t::CLASS,
                    t::STYLE,
                    t::unqString,
                    t::qString,
                ],
            ));
        }
    }
}
