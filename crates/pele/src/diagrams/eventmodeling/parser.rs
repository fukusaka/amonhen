//! eventmodeling の読み手（pele の `src/diagrams/eventmodeling/parser.ts`）。Mermaid の Langium の文法を写す。

use std::sync::LazyLock;

use crate::diagrams::common::acc_descr::acc_descr;
use crate::diagrams::common::tokens::{
    ACC_TITLE, DIRECTIVE, Lexicon, Pattern, Reader, SINGLE_LINE_COMMENT, TokenType, YAML, keyword,
    title_before, tokenize,
};
use crate::errors::Result;
use crate::util::chars::{NONE, is_digit, is_line_end, is_space, is_word};

const LETTERS: &str = "_abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

fn at(src: &[u16], p: usize) -> u32 {
    src.get(p).map_or(NONE, |&c| c as u32)
}

/// `[_a-zA-Z][\w_]*`
fn em_id(src: &[u16], p: usize) -> Option<usize> {
    let c = at(src, p);
    if !(c == 95 || (65..=90).contains(&c) || (97..=122).contains(&c)) {
        return None;
    }
    let mut e = p + 1;
    while is_word(at(src, e)) {
        e += 1;
    }
    Some(e)
}

/// `\d{1,3}`
fn em_fid(src: &[u16], p: usize) -> Option<usize> {
    let mut e = p;
    while e < p + 3 && is_digit(at(src, e)) {
        e += 1;
    }
    (e > p).then_some(e)
}

/// `\{(.*)\}|"(.*)"|'(.*)'`: 開きの後、同じ行の最後の閉じまで。
fn em_data_inline(src: &[u16], p: usize) -> Option<usize> {
    let close = match at(src, p) {
        123 => 125,
        34 => 34,
        39 => 39,
        _ => return None,
    };
    let mut e = p + 1;
    let mut last = None;
    while e < src.len() && !is_line_end(src[e] as u32) {
        if src[e] as u32 == close {
            last = Some(e + 1);
        }
        e += 1;
    }
    last
}

/// `(?:\r?\n|(?!\S))`
fn line_tail(src: &[u16], p: usize) -> Option<usize> {
    let c = at(src, p);
    if c == 13 && at(src, p + 1) == 10 {
        Some(p + 2)
    } else if c == 10 {
        Some(p + 1)
    } else if c == NONE || is_space(c) {
        Some(p)
    } else {
        None
    }
}

/// `\{[\t ]*\r?\n(?:[\S\s]*?\r?\n)?\}(?:\r?\n|(?!\S))`
fn em_data_block(src: &[u16], p: usize) -> Option<usize> {
    if at(src, p) != 123 {
        return None;
    }
    let mut q = p + 1;
    while matches!(at(src, q), 9 | 32) {
        q += 1;
    }
    let h = if at(src, q) == 13 && at(src, q + 1) == 10 {
        q + 2
    } else if at(src, q) == 10 {
        q + 1
    } else {
        return None;
    };
    // 中身があれば、その最後の改行の後に閉じがある。改行ごとに早い順に試す。
    for j in h..src.len() {
        if src[j] == 10
            && at(src, j + 1) == 125
            && let Some(end) = line_tail(src, j + 2)
        {
            return Some(end);
        }
    }
    if at(src, h) == 125 {
        return line_tail(src, h + 1);
    }
    None
}

/// `\s+`
fn em_ws(src: &[u16], p: usize) -> Option<usize> {
    let mut e = p;
    while is_space(at(src, e)) {
        e += 1;
    }
    Some(e)
}

/// `\/\*[\s\S]*?\*\/`
fn em_ml_comment(src: &[u16], p: usize) -> Option<usize> {
    if at(src, p) != 47 || at(src, p + 1) != 42 {
        return None;
    }
    (p + 2..src.len().saturating_sub(1))
        .find(|&k| src[k] == 42 && src[k + 1] == 47)
        .map(|k| k + 2)
}

/// `\/\/[^\n\r]*`
fn em_sl_comment(src: &[u16], p: usize) -> Option<usize> {
    if at(src, p) != 47 || at(src, p + 1) != 47 {
        return None;
    }
    let mut e = p + 2;
    while e < src.len() && src[e] != 10 && src[e] != 13 {
        e += 1;
    }
    Some(e)
}

/// Langium は識別子にもなるキーワードに、長い方として識別子を与える。
fn word(text: &str) -> TokenType {
    TokenType::literal(text, text).longer(&["EM_ID"])
}

fn sign(text: &str) -> TokenType {
    TokenType::literal(text, text)
}

const WORDS: [&str; 23] = [
    "resetframe",
    "readmodel",
    "processor",
    "timeframe",
    "command",
    "entity",
    "event",
    "jsobj",
    "figma",
    "given",
    "json",
    "salt",
    "html",
    "text",
    "data",
    "note",
    "when",
    "then",
    "rmo",
    "cmd",
    "evt",
    "pcr",
    "uri",
];

pub static EVENTMODELING_TOKENS: LazyLock<Lexicon> = LazyLock::new(|| {
    let mut types = vec![keyword("eventmodeling").longer(&["EM_ID"])];
    types.extend(WORDS.iter().map(|w| word(w)));
    types.extend([
        sign("->>"),
        word("gwt"),
        word("ui"),
        word("md"),
        word("tf"),
        sign("[["),
        sign("]]"),
        word("rf"),
        sign("`"),
        sign("."),
        TokenType::new("EM_ID", Pattern::regex(em_id)).first(LETTERS),
        TokenType::new("EM_FID", Pattern::regex(em_fid)).first("0123456789"),
        TokenType::new("EM_DATA_INLINE", Pattern::regex(em_data_inline)).first("{\"'"),
        TokenType::new("EM_DATA_BLOCK", Pattern::regex(em_data_block)).first("{"),
        acc_descr("EM_ACC_DESCR"),
        ACC_TITLE.clone().named("EM_ACC_TITLE"),
        title_before(|src, p| is_word(at(src, p))).named("EM_TITLE"),
        TokenType::new("EM_WS", Pattern::regex(em_ws)).hidden(),
        YAML.clone().named("EM_YAML"),
        DIRECTIVE.clone().named("EM_DIRECTIVE"),
        SINGLE_LINE_COMMENT.clone().named("EM_SINGLE_LINE_COMMENT"),
        TokenType::new("EM_ML_COMMENT", Pattern::regex(em_ml_comment))
            .hidden()
            .first("/"),
        TokenType::new("EM_SL_COMMENT", Pattern::regex(em_sl_comment))
            .hidden()
            .first("/"),
    ]);
    Lexicon::new(types)
});

/// Mermaid が読むとおりの参照: 文字で、つながない。
#[derive(Clone, Debug, PartialEq)]
pub struct EmFrame {
    pub reset: bool,
    pub name: String,
    pub model_entity_type: String,
    pub entity_identifier: String,
    pub source_frames: Vec<String>,
    pub data_reference: Option<String>,
    pub data_type: Option<String>,
    pub data_inline_value: Option<String>,
    /// 元の中の枠の位置（UTF-16）。知らせのため。
    pub offset: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmDataEntity {
    pub name: String,
    pub data_type: Option<String>,
    pub data_block_value: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmNoteEntity {
    pub source_frame: String,
    pub data_type: Option<String>,
    pub data_block_value: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmGwt {
    pub source_frame: String,
    pub given_statements: Vec<String>,
    pub when_statements: Vec<String>,
    pub then_statements: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EventModelAst {
    /// 字句のまま、キーワードも含めて: 文法自身の終端は値に直さない。
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub model_entities: Vec<String>,
    pub frames: Vec<EmFrame>,
    pub data_entities: Vec<EmDataEntity>,
    pub note_entities: Vec<EmNoteEntity>,
    pub gwt_entities: Vec<EmGwt>,
    pub source: Vec<u16>,
}

struct Kinds {
    arrow: i32,
    open: i32,
    close: i32,
    tick: i32,
    dot: i32,
    id: i32,
    frame_id: i32,
    inline: i32,
    block: i32,
    acc_descr: i32,
    acc_title: i32,
    title: i32,
    entity: i32,
    data: i32,
    note: i32,
    gwt: i32,
    given: i32,
    when: i32,
    then: i32,
    time_frame: [i32; 2],
    reset_frame: [i32; 2],
    entity_type: [i32; 9],
    data_type: [i32; 8],
}

fn kinds(lex: &Lexicon) -> Kinds {
    let i = |name: &str| lex.index(name).map_or(-1, |k| k as i32);
    let many = |names: &[&str]| names.iter().map(|n| i(n)).collect::<Vec<_>>();
    Kinds {
        arrow: i("->>"),
        open: i("[["),
        close: i("]]"),
        tick: i("`"),
        dot: i("."),
        id: i("EM_ID"),
        frame_id: i("EM_FID"),
        inline: i("EM_DATA_INLINE"),
        block: i("EM_DATA_BLOCK"),
        acc_descr: i("EM_ACC_DESCR"),
        acc_title: i("EM_ACC_TITLE"),
        title: i("EM_TITLE"),
        entity: i("entity"),
        data: i("data"),
        note: i("note"),
        gwt: i("gwt"),
        given: i("given"),
        when: i("when"),
        then: i("then"),
        time_frame: many(&["tf", "timeframe"]).try_into().unwrap(),
        reset_frame: many(&["rf", "resetframe"]).try_into().unwrap(),
        entity_type: many(&[
            "rmo",
            "readmodel",
            "ui",
            "cmd",
            "command",
            "evt",
            "event",
            "pcr",
            "processor",
        ])
        .try_into()
        .unwrap(),
        data_type: many(&[
            "json", "jsobj", "figma", "salt", "uri", "md", "html", "text",
        ])
        .try_into()
        .unwrap(),
    }
}

pub fn parse_event_model(src: &str) -> Result<EventModelAst> {
    let lex = &*EVENTMODELING_TOKENS;
    let t = kinds(lex);
    let mut r = Reader::new(tokenize(src, lex, "eventmodeling")?, lex, "eventmodeling");
    let mut ast = EventModelAst::default();

    fn one(r: &mut Reader, set: &[i32], expected: &str) -> Result<String> {
        if set.contains(&r.kind()) {
            Ok(r.take())
        } else {
            Err(r.fail(expected))
        }
    }
    let qualified_name = |r: &mut Reader| -> Result<String> {
        let mut name = r.expect(t.id)?;
        while r.accept(t.dot) {
            name.push('.');
            name.push_str(&r.expect(t.id)?);
        }
        Ok(name)
    };
    let data_type = |r: &mut Reader| -> Result<Option<String>> {
        if !r.accept(t.tick) {
            return Ok(None);
        }
        let type_ = one(r, &t.data_type, "a data type")?;
        r.expect(t.tick)?;
        Ok(Some(type_))
    };
    let statements = |r: &mut Reader| -> Result<Vec<String>> {
        let mut out = Vec::new();
        loop {
            one(r, &t.entity_type, "an entity type")?;
            out.push(r.expect(t.id)?);
            if !t.entity_type.contains(&r.kind()) {
                break;
            }
        }
        Ok(out)
    };

    r.expect(0)?;
    while r.kind() != -1 {
        let kind = r.kind();
        if kind == t.acc_descr {
            ast.acc_descr = Some(r.take());
        } else if kind == t.acc_title {
            ast.acc_title = Some(r.take());
        } else if kind == t.title {
            ast.title = Some(r.take());
        } else if kind == t.entity {
            r.i += 1;
            let name = qualified_name(&mut r)?;
            ast.model_entities.push(name);
        } else if t.time_frame.contains(&kind) || t.reset_frame.contains(&kind) {
            let offset = r.tokens.starts[r.i];
            r.i += 1;
            let name = r.expect(t.frame_id)?;
            let model_entity_type = one(&mut r, &t.entity_type, "an entity type")?;
            let entity_identifier = qualified_name(&mut r)?;
            let mut frame = EmFrame {
                reset: !t.time_frame.contains(&kind),
                name,
                model_entity_type,
                entity_identifier,
                source_frames: Vec::new(),
                data_reference: None,
                data_type: None,
                data_inline_value: None,
                offset,
            };
            while r.accept(t.arrow) {
                frame.source_frames.push(r.expect(t.frame_id)?);
            }
            if r.accept(t.open) {
                frame.data_reference = Some(r.expect(t.id)?);
                r.expect(t.close)?;
            }
            if r.kind() == t.tick || r.kind() == t.inline {
                frame.data_type = data_type(&mut r)?;
                frame.data_inline_value = Some(r.expect(t.inline)?);
            }
            ast.frames.push(frame);
        } else if kind == t.data {
            r.i += 1;
            let name = r.expect(t.id)?;
            let data_type = data_type(&mut r)?;
            let data_block_value = r.expect(t.block)?;
            ast.data_entities.push(EmDataEntity {
                name,
                data_type,
                data_block_value,
            });
        } else if kind == t.note {
            r.i += 1;
            let source_frame = r.expect(t.frame_id)?;
            let data_type = data_type(&mut r)?;
            let data_block_value = r.expect(t.block)?;
            ast.note_entities.push(EmNoteEntity {
                source_frame,
                data_type,
                data_block_value,
            });
        } else if kind == t.gwt {
            r.i += 1;
            let source_frame = r.expect(t.frame_id)?;
            r.expect(t.given)?;
            let given_statements = statements(&mut r)?;
            let when_statements = if r.accept(t.when) {
                statements(&mut r)?
            } else {
                Vec::new()
            };
            r.expect(t.then)?;
            let then_statements = statements(&mut r)?;
            ast.gwt_entities.push(EmGwt {
                source_frame,
                given_statements,
                when_statements,
                then_statements,
            });
        } else {
            return Err(r.fail("a frame, an entity, a data block, a note, or a specification"));
        }
    }
    ast.source = r.tokens.src;
    Ok(ast)
}
