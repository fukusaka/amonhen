//! C4 の構文解析（pele の `src/diagrams/c4/parser.ts`）。Jison が c4Diagram.jison から作る読み手が受け付ける
//! ものを受け付け、その意味の動作と同じ順に中身を集める。境界の入れ子は再帰でなく数で数える。

use super::db::C4Db;
use super::lexer::tokenize;
use super::tokens::{self as t, TOKEN_NAMES};
use super::types::{C4Attr, C4Named};
use crate::errors::{PeleError, Result, syntax_error};
use crate::js;

/// 形のキーワードごとの Mermaid の typeC4Shape（T.PERSON から）。
const KINDS: [&str; 20] = [
    "person",
    "external_person",
    "system",
    "system_db",
    "system_queue",
    "external_system",
    "external_system_db",
    "external_system_queue",
    "container",
    "container_db",
    "container_queue",
    "external_container",
    "external_container_db",
    "external_container_queue",
    "component",
    "component_db",
    "component_queue",
    "external_component",
    "external_component_db",
    "external_component_queue",
];
const REL_TYPES: [&str; 8] = [
    "rel", "birel", "rel_u", "rel_d", "rel_l", "rel_r", "rel_b", "rel",
];
const NODE_TYPES: [&str; 3] = ["node", "nodeL", "nodeR"];

const HEADERS: &str = "'direction_tb', 'direction_bt', 'direction_rl', 'direction_lr', 'C4_CONTEXT', 'C4_CONTAINER', 'C4_COMPONENT', 'C4_DYNAMIC', 'C4_DEPLOYMENT'";
const ATTRIBUTES: &str = "'STR', 'STR_KEY', 'ATTRIBUTE', 'ATTRIBUTE_EMPTY'";

#[derive(Clone, Copy, PartialEq, Eq)]
enum P {
    /// 文が要る。
    Item,
    /// 次に来てよいのは図の文だけ: 境界の最初の文か、題のすぐ後に書いた文。
    Diagram,
    ItemOrEnd,
    AfterOther,
    AfterDiagram,
}

fn is_other(ty: u8) -> bool {
    matches!(
        ty,
        t::title | t::accDescription | t::acc_title | t::acc_descr | t::acc_descr_multiline_value
    )
}

fn is_diagram(ty: u8) -> bool {
    (t::PERSON..=t::UPDATE_LAYOUT_CONFIG).contains(&ty)
}

struct Parser<'a> {
    src: &'a [u16],
    types: Vec<u8>,
    starts: Vec<usize>,
    ends: Vec<usize>,
    i: usize,
}

impl Parser<'_> {
    fn text(&self, at: usize) -> String {
        js::from16(&self.src[self.starts[at]..self.ends[at]])
    }

    /// `text(at).substring(from)`
    fn text_from(&self, at: usize, from: usize) -> String {
        let s = (self.starts[at] + from).min(self.ends[at]);
        js::from16(&self.src[s..self.ends[at]])
    }

    fn fail(&self, expected: &str) -> PeleError {
        let ty = self.types[self.i];
        if ty == t::ERROR {
            return syntax_error("c4", self.src, self.starts[self.i], "", true);
        }
        syntax_error(
            "c4",
            self.src,
            self.starts[self.i],
            &format!("Expecting {expected}, got '{}'", TOKEN_NAMES[ty as usize]),
            false,
        )
    }

    fn expect(&mut self, ty: u8) -> Result<()> {
        if self.types[self.i] != ty {
            return Err(self.fail(&format!("'{}'", TOKEN_NAMES[ty as usize])));
        }
        self.i += 1;
        Ok(())
    }

    fn attributes(&mut self) -> Result<Vec<C4Attr>> {
        let mut attrs = Vec::new();
        loop {
            let ty = self.types[self.i];
            if ty == t::STR {
                attrs.push(C4Attr::Str(js::trim(&self.text(self.i)).to_string()));
                self.i += 1;
            } else if ty == t::ATTRIBUTE_EMPTY {
                attrs.push(C4Attr::Str(String::new()));
                self.i += 1;
            } else if ty == t::STR_KEY {
                self.i += 1;
                if self.types[self.i] != t::STR_VALUE {
                    return Err(self.fail("'STR_VALUE'"));
                }
                attrs.push(C4Attr::Named(C4Named {
                    key: js::trim(&self.text(self.i - 1)).to_string(),
                    value: js::trim(&self.text(self.i)).to_string(),
                }));
                self.i += 1;
            } else {
                break;
            }
        }
        if attrs.is_empty() {
            return Err(self.fail(ATTRIBUTES));
        }
        Ok(attrs)
    }

    fn other(&mut self, db: &mut C4Db, ty: u8) -> Result<()> {
        if ty == t::title {
            db.set_title(&self.text_from(self.i, 6));
            self.i += 1;
        } else if ty == t::accDescription {
            db.set_acc_description(&self.text_from(self.i, 15));
            self.i += 1;
        } else if ty == t::acc_descr_multiline_value {
            db.set_acc_description(js::trim(&self.text(self.i)));
            self.i += 1;
        } else {
            self.i += 1;
            if ty == t::acc_title {
                if self.types[self.i] != t::acc_title_value {
                    return Err(self.fail("'acc_title_value'"));
                }
                // 文法は `accTitle:` を setTitle に渡すので、見える題になる。
                db.set_title(js::trim(&self.text(self.i)));
                self.i += 1;
            } else {
                if self.types[self.i] != t::acc_descr_value {
                    return Err(self.fail("'acc_descr_value'"));
                }
                db.set_acc_description(js::trim(&self.text(self.i)));
                self.i += 1;
            }
        }
        Ok(())
    }

    /// 文が境界を開くとき true。
    fn diagram(&mut self, db: &mut C4Db, ty: u8) -> Result<bool> {
        self.i += 1;
        let mut attrs = self.attributes()?;
        let insert = |attrs: &mut Vec<C4Attr>, s: &str| {
            // `attrs.splice(2, 0, s)`
            let at = attrs.len().min(2);
            attrs.insert(at, C4Attr::Str(s.to_string()));
        };
        if ty <= t::SYSTEM_EXT_QUEUE {
            db.add_person_or_system(KINDS[(ty - t::PERSON) as usize], &attrs);
        } else if ty <= t::CONTAINER_EXT_QUEUE {
            db.add_container(KINDS[(ty - t::PERSON) as usize], &attrs);
        } else if ty <= t::COMPONENT_EXT_QUEUE {
            db.add_component(KINDS[(ty - t::PERSON) as usize], &attrs);
        } else if ty <= t::NODE_R {
            if ty == t::ENTERPRISE_BOUNDARY {
                insert(&mut attrs, "ENTERPRISE");
            } else if ty == t::SYSTEM_BOUNDARY {
                insert(&mut attrs, "SYSTEM");
            } else if ty == t::CONTAINER_BOUNDARY {
                insert(&mut attrs, "CONTAINER");
            }
            if ty >= t::NODE {
                db.add_deployment_node(NODE_TYPES[(ty - t::NODE) as usize], &attrs);
            } else if ty == t::CONTAINER_BOUNDARY {
                db.add_container_boundary(&attrs);
            } else {
                db.add_person_or_system_boundary(&attrs);
            }
            if self.types[self.i] == t::LBRACE {
                self.i += 1;
                self.expect(t::NEWLINE)?;
            } else if self.types[self.i] == t::NEWLINE {
                self.i += 1;
                self.expect(t::LBRACE)?;
                if self.types[self.i] == t::NEWLINE {
                    self.i += 1;
                }
            } else {
                return Err(self.fail("'NEWLINE', 'LBRACE'"));
            }
            return Ok(true);
        } else if ty <= t::REL_INDEX {
            // RelIndex は先に番号を取り、Mermaid はそれを使わない。
            if ty == t::REL_INDEX {
                attrs.remove(0);
            }
            db.add_rel(REL_TYPES[(ty - t::REL) as usize], &attrs);
        } else if ty == t::UPDATE_EL_STYLE {
            db.update_el_style("update_el_style", &attrs);
        } else if ty == t::UPDATE_REL_STYLE {
            db.update_rel_style("update_rel_style", &attrs);
        } else {
            db.update_layout_config("update_layout_config", &attrs);
        }
        Ok(false)
    }
}

pub fn parse_c4(source: &str, db: &mut C4Db) -> Result<()> {
    let src: Vec<u16> = source.encode_utf16().collect();
    let tokens = tokenize(&src);
    let mut p = Parser {
        src: &src,
        types: tokens.types,
        starts: tokens.starts,
        ends: tokens.ends,
        i: 0,
    };

    let header = p.types[p.i];
    if !(t::C4_CONTEXT..=t::C4_DEPLOYMENT).contains(&header) {
        return Err(p.fail(HEADERS));
    }
    p.i += 1;
    p.expect(t::NEWLINE)?;

    let mut depth = 0usize;
    let mut state = P::Item;
    loop {
        let ty = p.types[p.i];
        if state == P::AfterOther || state == P::AfterDiagram {
            if ty == t::NEWLINE {
                p.i += 1;
                state = P::ItemOrEnd;
                continue;
            }
            if state == P::AfterOther && is_diagram(ty) {
                state = P::Diagram;
                continue;
            }
        } else if is_diagram(ty) {
            if p.diagram(db, ty)? {
                depth += 1;
                state = P::Diagram;
            } else {
                state = P::AfterDiagram;
            }
            continue;
        } else if is_other(ty) && state != P::Diagram {
            p.other(db, ty)?;
            state = P::AfterOther;
            continue;
        } else if state != P::ItemOrEnd {
            return Err(p.fail(
                "'title', 'accDescription', 'acc_title', 'acc_descr', 'acc_descr_multiline_value', 'PERSON', 'REL'",
            ));
        }

        if depth > 0 {
            if ty != t::RBRACE {
                return Err(p.fail("'NEWLINE', 'RBRACE'"));
            }
            p.i += 1;
            db.pop_boundary_parse_stack();
            depth -= 1;
            state = P::AfterDiagram;
        } else {
            if ty != t::EOF {
                return Err(p.fail("'NEWLINE', 'EOF'"));
            }
            p.i += 1;
            p.expect(t::END)?;
            db.set_c4_type(&p.text(0));
            return Ok(());
        }
    }
}
