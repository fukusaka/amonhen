//! er の読み手（pele の `src/diagrams/er/parser.ts`）。

use super::db::ErDb;
use super::lexer::tokenize;
use super::tokens::{self as t, TOKEN_NAMES};
use super::types::{Cardinality, DocItem, ErAttribute, RelSpec};
use crate::errors::{Result, syntax_error};
use crate::js;

fn cardinality(ty: u8) -> Option<Cardinality> {
    match ty {
        t::ZERO_OR_ONE => Some("ZERO_OR_ONE"),
        t::ZERO_OR_MORE => Some("ZERO_OR_MORE"),
        t::ONE_OR_MORE => Some("ONE_OR_MORE"),
        t::ONLY_ONE => Some("ONLY_ONE"),
        t::MD_PARENT => Some("MD_PARENT"),
        _ => None,
    }
}

const DIRECTIONS: [&str; 4] = ["TB", "BT", "RL", "LR"];

const NAME: &str = "'UNICODE_TEXT', 'NUM', 'ENTITY_NAME', 'DECIMAL_NUM', 'ENTITY_ONE'";
const CARDS: &str = "'ZERO_OR_ONE', 'ZERO_OR_MORE', 'ONE_OR_MORE', 'ONLY_ONE', 'MD_PARENT'";
const LINE: &str = "'EOF', 'NEWLINE', 'title', 'acc_title', 'acc_descr', 'acc_descr_multiline_value', 'SUBGRAPH', 'direction_tb', 'direction_bt', 'direction_rl', 'direction_lr', 'CLASSDEF', 'CLASS', 'STYLE', 'UNICODE_TEXT', 'NUM', 'ENTITY_NAME', 'DECIMAL_NUM', 'ENTITY_ONE'";

struct Frame {
    id: String,
    title: String,
    /// Mermaid の文法が addSubGraph に渡すもの: 中のどの文の値も。実体の名前のほか、アクセシビリティの文字と、
    /// `style`・`class`・`classDef` の語も入る。
    list: Vec<DocItem>,
}

fn is_name(ty: u8) -> bool {
    matches!(
        ty,
        t::UNICODE_TEXT | t::ENTITY_NAME | t::NUM | t::DECIMAL_NUM | t::ENTITY_ONE
    )
}

fn is_style_component(ty: u8) -> bool {
    matches!(ty, t::STYLE_TEXT | t::NUM | t::COLON | t::BRKT)
}

/// `text.slice(1, -1)`
fn unquote(text: &str) -> String {
    let mut chars = text.chars();
    chars.next();
    chars.next_back();
    chars.as_str().to_string()
}

struct Parser<'a> {
    src: &'a [u16],
    types: Vec<u8>,
    texts: Vec<String>,
    starts: Vec<usize>,
    i: usize,
}

impl Parser<'_> {
    fn ty(&self) -> u8 {
        self.types[self.i]
    }

    fn take(&mut self) -> String {
        let s = self.texts[self.i].clone();
        self.i += 1;
        s
    }

    fn fail<T>(&self, expected: &str) -> Result<T> {
        let ty = self.ty();
        if ty == t::ERROR {
            return Err(syntax_error("er", self.src, self.starts[self.i], "", true));
        }
        let got = if ty == t::CHAR {
            self.texts[self.i].as_str()
        } else {
            TOKEN_NAMES[ty as usize]
        };
        Err(syntax_error(
            "er",
            self.src,
            self.starts[self.i],
            &format!("Expecting {expected}, got '{got}'"),
            false,
        ))
    }

    fn expect(&mut self, ty: u8) -> Result<String> {
        if self.ty() != ty {
            return self.fail(&format!("'{}'", TOKEN_NAMES[ty as usize]));
        }
        Ok(self.take())
    }

    fn separator(&mut self) -> Result<()> {
        let ty = self.ty();
        if ty != t::NEWLINE && ty != t::SEMI && ty != t::EOF {
            return self.fail("'EOF', 'NEWLINE', 'SEMI'");
        }
        self.i += 1;
        Ok(())
    }

    fn entity_name(&mut self) -> Result<String> {
        let ty = self.ty();
        if !is_name(ty) {
            return self.fail(NAME);
        }
        let text = self.take();
        Ok(if ty == t::ENTITY_NAME {
            unquote(&text)
        } else {
            text
        })
    }

    fn id_list(&mut self) -> Result<Vec<String>> {
        let mut ids = Vec::new();
        loop {
            let ty = self.ty();
            if ty != t::UNICODE_TEXT && ty != t::STYLE_TEXT {
                return self.fail("'UNICODE_TEXT', 'STYLE_TEXT'");
            }
            ids.push(self.take());
            if self.ty() != t::COMMA {
                return Ok(ids);
            }
            self.i += 1;
        }
    }

    fn styles_opt(&mut self) -> Result<Vec<String>> {
        let mut styles = Vec::new();
        loop {
            if !is_style_component(self.ty()) {
                return self.fail("'STYLE_TEXT', 'COLON', 'NUM', 'BRKT'");
            }
            let mut style = self.take();
            while is_style_component(self.ty()) {
                style.push_str(&self.take());
            }
            styles.push(style);
            if self.ty() != t::COMMA {
                return Ok(styles);
            }
            self.i += 1;
        }
    }

    fn rel_spec(&mut self) -> Result<RelSpec> {
        let card_b = cardinality(self.ty()).unwrap();
        self.i += 1;
        let r = self.ty();
        if r != t::IDENTIFYING && r != t::NON_IDENTIFYING {
            return self.fail("'NON_IDENTIFYING', 'IDENTIFYING'");
        }
        self.i += 1;
        let Some(card_a) = cardinality(self.ty()) else {
            return self.fail(CARDS);
        };
        self.i += 1;
        Ok(RelSpec {
            card_a,
            identifying: r == t::IDENTIFYING,
            card_b,
        })
    }

    fn role(&mut self) -> Result<String> {
        let ty = self.ty();
        if ty != t::WORD && ty != t::ENTITY_NAME && ty != t::UNICODE_TEXT {
            return self.fail("'UNICODE_TEXT', 'ENTITY_NAME', 'WORD'");
        }
        let text = self.take();
        Ok(if ty == t::UNICODE_TEXT {
            text
        } else {
            unquote(&text)
        })
    }

    fn is_char(&self, ch: &str) -> bool {
        self.ty() == t::CHAR && self.texts[self.i] == ch
    }

    /// 属性を後ろから先へ返す。Mermaid の右再帰の規則が作るとおり。
    fn attributes(&mut self) -> Result<Vec<ErAttribute>> {
        let mut list = Vec::new();
        loop {
            let mut type_ = self.expect(t::ATTRIBUTE_WORD)?;
            if self.is_char("?") {
                type_.push_str(&self.take());
            }
            if self.ty() != t::ATTRIBUTE_WORD {
                return self.fail(if type_.ends_with('?') {
                    "'ATTRIBUTE_WORD'"
                } else {
                    "'ATTRIBUTE_WORD', '?'"
                });
            }
            let name = self.take();
            let mut keys = Vec::new();
            if self.ty() == t::ATTRIBUTE_KEY {
                keys.push(self.take());
                while self.is_char(",") {
                    self.i += 1;
                    keys.push(self.expect(t::ATTRIBUTE_KEY)?);
                }
            }
            let mut comment = String::new();
            if self.ty() == t::COMMENT {
                comment = unquote(&self.take());
            }
            list.push(ErAttribute {
                type_,
                name,
                keys,
                comment,
            });
            if self.ty() == t::BLOCK_STOP {
                break;
            }
        }
        list.reverse();
        Ok(list)
    }

    /// `{ ... }` の塊を読む。なければ、または空なら None。
    fn block(&mut self) -> Result<Option<Vec<ErAttribute>>> {
        if self.ty() != t::BLOCK_START {
            return Ok(None);
        }
        self.i += 1;
        if self.ty() == t::BLOCK_STOP {
            self.i += 1;
            return Ok(None);
        }
        let list = self.attributes()?;
        self.i += 1;
        Ok(Some(list))
    }

    fn entity_statement(&mut self, db: &mut ErDb) -> Result<Vec<String>> {
        let a = self.entity_name()?;
        let mut ty = self.ty();
        if ty == t::SQS {
            self.i += 1;
            let alias = self.entity_name()?;
            self.expect(t::SQE)?;
            let mut classes = None;
            if self.ty() == t::STYLE_SEPARATOR {
                self.i += 1;
                classes = Some(self.id_list()?);
            }
            let attrs = self.block()?;
            db.add_entity(&a, &alias);
            if let Some(attrs) = attrs {
                db.add_attributes(&a, attrs);
            }
            if let Some(classes) = classes {
                db.set_class(std::slice::from_ref(&a), &classes);
            }
            return Ok(vec![a]);
        }
        let mut classes = None;
        if ty == t::STYLE_SEPARATOR {
            self.i += 1;
            classes = Some(self.id_list()?);
            ty = self.ty();
        }
        if cardinality(ty).is_some() {
            let spec = self.rel_spec()?;
            let b = self.entity_name()?;
            let mut classes_b = None;
            if self.ty() == t::STYLE_SEPARATOR {
                self.i += 1;
                classes_b = Some(self.id_list()?);
            }
            self.expect(t::COLON)?;
            let label = self.role()?;
            db.add_entity(&a, "");
            db.add_entity(&b, "");
            db.add_relationship(&a, &label, &b, spec);
            if let Some(classes) = classes {
                db.set_class(std::slice::from_ref(&a), &classes);
            }
            if let Some(classes) = classes_b {
                db.set_class(std::slice::from_ref(&b), &classes);
            }
            return Ok(vec![a, b]);
        }
        let attrs = self.block()?;
        db.add_entity(&a, "");
        if let Some(attrs) = attrs {
            db.add_attributes(&a, attrs);
        }
        if let Some(classes) = classes {
            db.set_class(std::slice::from_ref(&a), &classes);
        }
        Ok(vec![a])
    }
}

pub fn parse_er(source: &str, db: &mut ErDb) -> Result<()> {
    let src: Vec<u16> = source.encode_utf16().collect();
    let tokens = tokenize(&src);
    let mut p = Parser {
        src: &src,
        types: tokens.types,
        texts: tokens.texts,
        starts: tokens.starts,
        i: 0,
    };

    p.expect(t::ER_DIAGRAM)?;
    // まとまりは呼び出しの積み重ねではなくここに積む。最初の枠は図そのもの。
    let mut frames: Vec<Frame> = vec![Frame {
        id: String::new(),
        title: String::new(),
        list: Vec::new(),
    }];
    let push = |frames: &mut Vec<Frame>, item: DocItem| {
        if frames.len() > 1 {
            frames.last_mut().unwrap().list.push(item);
        }
    };

    loop {
        let ty = p.ty();
        let nested = frames.len() > 1;
        match ty {
            t::NEWLINE => p.i += 1,
            t::EOF => {
                p.i += 1;
                if nested {
                    return p.fail(&format!("'END', {LINE}"));
                }
                return Ok(());
            }
            t::END_KW => {
                if !nested {
                    return p.fail(LINE);
                }
                p.i += 1;
                let frame = frames.pop().unwrap();
                let id = db.add_sub_graph(&frame.id, &frame.list, &frame.title);
                push(&mut frames, DocItem::Text(id));
            }
            t::SUBGRAPH => {
                p.i += 1;
                let id = p.entity_name()?;
                let mut title = id.clone();
                if p.ty() == t::SQS {
                    p.i += 1;
                    title = p.entity_name()?;
                    while is_name(p.ty()) {
                        title.push(' ');
                        title.push_str(&p.entity_name()?);
                    }
                    p.expect(t::SQE)?;
                }
                p.separator()?;
                frames.push(Frame {
                    id,
                    title,
                    list: Vec::new(),
                });
            }
            t::acc_title => {
                p.i += 1;
                let value = js::trim(&p.expect(t::acc_title_value)?).to_string();
                db.set_acc_title(&value);
                push(&mut frames, DocItem::Text(value));
            }
            t::acc_descr => {
                p.i += 1;
                let value = js::trim(&p.expect(t::acc_descr_value)?).to_string();
                db.set_acc_description(&value);
                push(&mut frames, DocItem::Text(value));
            }
            t::acc_descr_multiline_value => {
                let value = js::trim(&p.take()).to_string();
                db.set_acc_description(&value);
                push(&mut frames, DocItem::Text(value));
            }
            t::direction_tb | t::direction_bt | t::direction_rl | t::direction_lr => {
                p.i += 1;
                let value = DIRECTIONS[(ty - t::direction_tb) as usize];
                if nested {
                    push(&mut frames, DocItem::Dir(value.into()));
                } else {
                    db.set_direction(value);
                }
            }
            t::CLASSDEF => {
                let keyword = p.take();
                let ids = p.id_list()?;
                let styles = p.styles_opt()?;
                p.separator()?;
                db.add_class(&ids, &styles);
                push(&mut frames, DocItem::Text(keyword));
            }
            t::CLASS => {
                let keyword = p.take();
                let ids = p.id_list()?;
                let names = p.id_list()?;
                db.set_class(&ids, &names);
                push(&mut frames, DocItem::Text(keyword));
            }
            t::STYLE => {
                let keyword = p.take();
                let ids = p.id_list()?;
                let styles = p.styles_opt()?;
                p.separator()?;
                db.add_css_styles(&ids, &styles);
                push(&mut frames, DocItem::Text(keyword));
            }
            _ => {
                if !is_name(ty) {
                    return p.fail(&if nested {
                        format!("'END', {LINE}")
                    } else {
                        LINE.to_string()
                    });
                }
                let names = p.entity_statement(db)?;
                for name in names {
                    push(&mut frames, DocItem::Text(name));
                }
            }
        }
    }
}
