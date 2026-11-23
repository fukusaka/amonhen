//! wardley の読み手（pele の `src/diagrams/wardley/parser.ts`）。Mermaid の Langium の文法を写す。

use std::sync::LazyLock;

use crate::diagrams::common::acc_descr::acc_descr;
use crate::diagrams::common::tokens::{
    ACC_TITLE, DIRECTIVE, Lexicon, NEWLINE, Pattern, Reader, SINGLE_LINE_COMMENT, STRING, TITLE,
    TokenType, WHITESPACE, YAML, acc_descr_value, acc_title_value, string_value, title_value,
    tokenize,
};
use crate::errors::Result;
use crate::js;
use crate::util::chars::{NONE, is_digit, is_space, is_word};

fn at(src: &[u16], p: usize) -> u32 {
    src.get(p).map_or(NONE, |&c| c as u32)
}

fn has(src: &[u16], p: usize, word: &str) -> bool {
    word.bytes()
        .enumerate()
        .all(|(k, w)| at(src, p + k) == w as u32)
}

fn digits(src: &[u16], from: usize) -> usize {
    let mut i = from;
    while is_digit(at(src, i)) {
        i += 1;
    }
    i
}

/// `[0-9]+\.[0-9]+`
fn number(src: &[u16], p: usize) -> Option<usize> {
    let end = digits(src, p);
    if end == p || at(src, end) != 46 {
        return None;
    }
    let stop = digits(src, end + 1);
    (stop > end + 1).then_some(stop)
}

/// `->`
fn arrow(src: &[u16], p: usize) -> Option<usize> {
    has(src, p, "->").then_some(p + 2)
}

/// `\+<>|\+>|\+<`
fn link_port(src: &[u16], p: usize) -> Option<usize> {
    if has(src, p, "+<>") {
        Some(p + 3)
    } else if has(src, p, "+>") || has(src, p, "+<") {
        Some(p + 2)
    } else {
        None
    }
}

/// `-->|-\.->|>|\+'[^']*'<>|\+'[^']*'<|\+'[^']*'>`
fn link_arrow(src: &[u16], p: usize) -> Option<usize> {
    if has(src, p, "-->") {
        return Some(p + 3);
    }
    if has(src, p, "-.->") {
        return Some(p + 4);
    }
    if at(src, p) == 62 {
        return Some(p + 1);
    }
    if !has(src, p, "+'") {
        return None;
    }
    let close = (p + 2..src.len()).find(|&k| src[k] == 39)?;
    if has(src, close + 1, "<>") {
        Some(close + 3)
    } else if has(src, close + 1, "<") || has(src, close + 1, ">") {
        Some(close + 2)
    } else {
        None
    }
}

/// `;[^\n\r]+`
fn link_label(src: &[u16], p: usize) -> Option<usize> {
    if at(src, p) != 59 {
        return None;
    }
    let mut e = p + 1;
    while e < src.len() && src[e] != 10 && src[e] != 13 {
        e += 1;
    }
    (e > p + 1).then_some(e)
}

/// `build|buy|outsource|market`
fn strategy(src: &[u16], p: usize) -> Option<usize> {
    ["build", "buy", "outsource", "market"]
        .iter()
        .find(|w| has(src, p, w))
        .map(|w| p + w.len())
}

fn is_alpha(c: u32) -> bool {
    (65..=90).contains(&c) || (97..=122).contains(&c)
}

/// `(?:[A-Za-z0-9_()&]|-(?!>))*` の終わり。
fn name_rest(src: &[u16], mut p: usize) -> usize {
    loop {
        let c = at(src, p);
        if is_alpha(c)
            || is_digit(c)
            || matches!(c, 95 | 40 | 41 | 38)
            || (c == 45 && at(src, p + 1) != 62)
        {
            p += 1;
        } else {
            return p;
        }
    }
}

/// `(?!title\s|accTitle|accDescr)[A-Za-z](?:[A-Za-z0-9_()&]|-(?!>))*(?:[ \t]+[A-Za-z(](?:[A-Za-z0-9_()&]|-(?!>))*)*`
fn name_with_spaces(src: &[u16], p: usize) -> Option<usize> {
    if (has(src, p, "title") && is_space(at(src, p + 5)))
        || has(src, p, "accTitle")
        || has(src, p, "accDescr")
    {
        return None;
    }
    if !is_alpha(at(src, p)) {
        return None;
    }
    let mut end = name_rest(src, p + 1);
    loop {
        let mut q = end;
        while matches!(at(src, q), 9 | 32) {
            q += 1;
        }
        let c = at(src, q);
        if q == end || !(is_alpha(c) || c == 40) {
            return Some(end);
        }
        end = name_rest(src, q + 1);
    }
}

/// `[ \t]+`
fn ws(src: &[u16], p: usize) -> Option<usize> {
    let mut e = p;
    while matches!(at(src, e), 9 | 32) {
        e += 1;
    }
    Some(e)
}

/// `0|[1-9][0-9]*(?!\.)`
fn int(src: &[u16], p: usize) -> Option<usize> {
    let c = at(src, p);
    if c == 48 {
        return Some(p + 1);
    }
    if !(49..=57).contains(&c) {
        return None;
    }
    let end = digits(src, p + 1);
    if at(src, end) != 46 {
        Some(end)
    } else if end - 1 > p {
        Some(end - 1)
    } else {
        None
    }
}

/// `[\w]([-\w]*\w)?`
fn id(src: &[u16], p: usize) -> Option<usize> {
    if !is_word(at(src, p)) {
        return None;
    }
    let mut end = p + 1;
    let mut k = p + 1;
    loop {
        let c = at(src, k);
        if is_word(c) {
            k += 1;
            end = k;
        } else if c == 45 {
            k += 1;
        } else {
            return Some(end);
        }
    }
}

const LETTERS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

pub static WARDLEY_TOKENS: LazyLock<Lexicon> = LazyLock::new(|| {
    let word = |name: &str, text: &str| TokenType::literal(name, text);
    Lexicon::new(vec![
        TokenType::literal("[", "["),
        TokenType::literal(",", ","),
        TokenType::literal("]", "]"),
        TokenType::literal("@", "@"),
        TokenType::literal("/", "/"),
        TokenType::literal("(", "("),
        TokenType::literal(")", ")"),
        TokenType::literal("-", "-").longer(&["ARROW", "LINK_ARROW", "YAML"]),
        TokenType::literal("{", "{"),
        TokenType::literal("}", "}"),
        TokenType::new("WARDLEY_NUMBER", Pattern::regex(number)).first("0123456789"),
        TokenType::new("ARROW", Pattern::regex(arrow)).first("-"),
        TokenType::new("LINK_PORT", Pattern::regex(link_port)).first("+"),
        TokenType::new("LINK_ARROW", Pattern::regex(link_arrow)).first("->+"),
        TokenType::new("LINK_LABEL", Pattern::regex(link_label)).first(";"),
        TokenType::new("STRATEGY", Pattern::regex(strategy)).first("bom"),
        word("KW_WARDLEY", "wardley-beta"),
        word("KW_SIZE", "size"),
        word("KW_EVOLUTION", "evolution"),
        word("KW_ANCHOR", "anchor"),
        word("KW_COMPONENT", "component"),
        word("KW_LABEL", "label"),
        word("KW_INERTIA", "inertia"),
        word("KW_EVOLVE", "evolve"),
        word("KW_PIPELINE", "pipeline"),
        word("KW_NOTE", "note"),
        word("KW_ANNOTATIONS", "annotations"),
        word("KW_ANNOTATION", "annotation"),
        word("KW_ACCELERATOR", "accelerator"),
        word("KW_DEACCELERATOR", "deaccelerator"),
        TokenType::new("NAME_WITH_SPACES", Pattern::regex(name_with_spaces)).first(LETTERS),
        TokenType::new("WS", Pattern::regex(ws))
            .hidden()
            .first("\t "),
        acc_descr("ACC_DESCR"),
        ACC_TITLE.clone(),
        TITLE.clone(),
        TokenType::new("INT", Pattern::regex(int)).first("0123456789"),
        STRING.clone(),
        TokenType::new("ID", Pattern::regex(id))
            .first("_0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"),
        NEWLINE.clone(),
        WHITESPACE.clone(),
        YAML.clone(),
        DIRECTIVE.clone(),
        SINGLE_LINE_COMMENT.clone(),
    ])
});

// 字句の種類の番号（表の順）。
const LBRACKET: i32 = 0;
const COMMA: i32 = 1;
const RBRACKET: i32 = 2;
const AT: i32 = 3;
const SLASH: i32 = 4;
const LPAREN: i32 = 5;
const RPAREN: i32 = 6;
const MINUS: i32 = 7;
const LBRACE: i32 = 8;
const RBRACE: i32 = 9;
const NUMBER: i32 = 10;
const ARROW: i32 = 11;
const LINK_PORT: i32 = 12;
const LINK_ARROW: i32 = 13;
const LINK_LABEL: i32 = 14;
const STRATEGY: i32 = 15;
const WARDLEY: i32 = 16;
const SIZE: i32 = 17;
const EVOLUTION: i32 = 18;
const ANCHOR: i32 = 19;
const COMPONENT: i32 = 20;
const LABEL: i32 = 21;
const INERTIA: i32 = 22;
const EVOLVE: i32 = 23;
const PIPELINE: i32 = 24;
const NOTE: i32 = 25;
const ANNOTATIONS: i32 = 26;
const ANNOTATION: i32 = 27;
const ACCELERATOR: i32 = 28;
const DEACCELERATOR: i32 = 29;
const NAME: i32 = 30;
const ACC_DESCR_KIND: i32 = 32;
const ACC_TITLE_KIND: i32 = 33;
const TITLE_KIND: i32 = 34;
const INT: i32 = 35;
const STRING_KIND: i32 = 36;
const ID: i32 = 37;
const NEWLINE_KIND: i32 = 38;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LabelAst {
    pub neg_x: bool,
    pub offset_x: f64,
    pub neg_y: bool,
    pub offset_y: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StageAst {
    pub name: String,
    pub boundary: Option<f64>,
    pub second_name: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ComponentAst {
    pub name: String,
    pub visibility: f64,
    pub evolution: f64,
    pub label: Option<LabelAst>,
    pub decorator: Option<String>,
    pub inertia: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PipelineComponentAst {
    pub name: String,
    pub evolution: f64,
    pub label: Option<LabelAst>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LinkAst {
    pub from: String,
    pub from_port: Option<String>,
    pub arrow: Option<String>,
    pub to: String,
    pub to_port: Option<String>,
    pub link_label: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnchorAst {
    pub name: String,
    pub visibility: f64,
    pub evolution: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PipelineAst {
    pub parent: String,
    pub components: Vec<PipelineComponentAst>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NoteAst {
    pub text: String,
    pub visibility: f64,
    pub evolution: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnnotationAst {
    pub number: f64,
    pub x: f64,
    pub y: f64,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlacedAst {
    pub name: String,
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WardleyAst {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub size: Option<(f64, f64)>,
    pub evolution: Option<Vec<StageAst>>,
    pub anchors: Vec<AnchorAst>,
    pub components: Vec<ComponentAst>,
    pub links: Vec<LinkAst>,
    /// (部品, 行き先)
    pub evolves: Vec<(String, f64)>,
    pub pipelines: Vec<PipelineAst>,
    pub notes: Vec<NoteAst>,
    /// (x, y)
    pub annotations: Vec<(f64, f64)>,
    pub annotation: Vec<AnnotationAst>,
    pub accelerators: Vec<PlacedAst>,
    pub deaccelerators: Vec<PlacedAst>,
}

struct Parser<'a> {
    r: Reader<'a>,
}

impl Parser<'_> {
    fn at(&self, kind: i32) -> bool {
        self.r.kind() == kind
    }

    fn is_name(&self) -> bool {
        self.at(STRING_KIND) || self.at(ID) || self.at(NAME)
    }

    fn name(&mut self) -> Result<String> {
        if self.at(STRING_KIND) {
            return Ok(string_value(&self.r.take()));
        }
        if self.at(ID) || self.at(NAME) {
            return Ok(self.r.take());
        }
        Err(self.r.fail("a name"))
    }

    fn number(&mut self) -> Result<f64> {
        Ok(js::number(&self.r.expect(NUMBER)?))
    }

    fn int(&mut self) -> Result<f64> {
        Ok(js::parse_int(&self.r.expect(INT)?, 10))
    }

    fn coordinate(&mut self) -> Result<f64> {
        if self.at(NUMBER) {
            Ok(js::number(&self.r.take()))
        } else {
            self.int()
        }
    }

    fn end_of_line(&mut self) -> Result<()> {
        if self.at(-1) {
            return Ok(());
        }
        if !self.at(NEWLINE_KIND) {
            return Err(self.r.fail("a line break or the end of input"));
        }
        while self.at(NEWLINE_KIND) {
            self.r.i += 1;
        }
        Ok(())
    }

    fn pair(&mut self, read: fn(&mut Self) -> Result<f64>) -> Result<(f64, f64)> {
        self.r.expect(LBRACKET)?;
        let first = read(self)?;
        self.r.expect(COMMA)?;
        let second = read(self)?;
        self.r.expect(RBRACKET)?;
        Ok((first, second))
    }

    fn label(&mut self) -> Result<LabelAst> {
        self.r.expect(LABEL)?;
        self.r.expect(LBRACKET)?;
        let neg_x = self.r.accept(MINUS);
        let offset_x = self.int()?;
        self.r.expect(COMMA)?;
        let neg_y = self.r.accept(MINUS);
        let offset_y = self.int()?;
        self.r.expect(RBRACKET)?;
        Ok(LabelAst {
            neg_x,
            offset_x,
            neg_y,
            offset_y,
        })
    }

    fn stage(&mut self) -> Result<StageAst> {
        let mut out = StageAst {
            name: self.name()?,
            boundary: None,
            second_name: None,
        };
        if self.r.accept(AT) {
            out.boundary = Some(self.number()?);
        }
        if self.r.accept(SLASH) {
            out.second_name = Some(self.name()?);
        }
        Ok(out)
    }
}

pub fn parse_wardley(src: &str) -> Result<WardleyAst> {
    let lex = &*WARDLEY_TOKENS;
    let mut p = Parser {
        r: Reader::new(tokenize(src, lex, "wardley")?, lex, "wardley"),
    };
    let mut ast = WardleyAst::default();

    while p.at(NEWLINE_KIND) {
        p.r.i += 1;
    }
    p.r.expect(WARDLEY)?;
    while !p.at(-1) {
        match p.r.kind() {
            NEWLINE_KIND => p.r.i += 1,
            ACC_DESCR_KIND => {
                ast.acc_descr = Some(acc_descr_value(&p.r.take()));
                p.end_of_line()?;
            }
            ACC_TITLE_KIND => {
                ast.acc_title = Some(acc_title_value(&p.r.take()));
                p.end_of_line()?;
            }
            TITLE_KIND => {
                ast.title = Some(title_value(&p.r.take()));
                p.end_of_line()?;
            }
            SIZE => {
                p.r.i += 1;
                let size = p.pair(Parser::int)?;
                p.end_of_line()?;
                ast.size = Some(size);
            }
            EVOLUTION => {
                p.r.i += 1;
                let mut stages = vec![p.stage()?];
                p.r.expect(ARROW)?;
                stages.push(p.stage()?);
                while p.r.accept(ARROW) {
                    stages.push(p.stage()?);
                }
                p.end_of_line()?;
                ast.evolution = Some(stages);
            }
            ANCHOR => {
                p.r.i += 1;
                let name = p.name()?;
                let (visibility, evolution) = p.pair(Parser::number)?;
                p.end_of_line()?;
                ast.anchors.push(AnchorAst {
                    name,
                    visibility,
                    evolution,
                });
            }
            COMPONENT => {
                p.r.i += 1;
                let name = p.name()?;
                let (visibility, evolution) = p.pair(Parser::number)?;
                let mut component = ComponentAst {
                    name,
                    visibility,
                    evolution,
                    label: None,
                    decorator: None,
                    inertia: false,
                };
                if p.at(LABEL) {
                    component.label = Some(p.label()?);
                }
                if p.at(LPAREN) && p.r.kind_at(p.r.i + 1) == STRATEGY {
                    p.r.i += 1;
                    component.decorator = Some(p.r.take());
                    p.r.expect(RPAREN)?;
                }
                if p.r.accept(INERTIA) {
                    component.inertia = true;
                } else if p.r.accept(LPAREN) {
                    p.r.expect(INERTIA)?;
                    p.r.expect(RPAREN)?;
                    component.inertia = true;
                }
                p.end_of_line()?;
                ast.components.push(component);
            }
            EVOLVE => {
                p.r.i += 1;
                let component = p.name()?;
                let target = p.number()?;
                p.end_of_line()?;
                ast.evolves.push((component, target));
            }
            PIPELINE => {
                p.r.i += 1;
                let parent = p.name()?;
                p.r.expect(LBRACE)?;
                p.r.expect(NEWLINE_KIND)?;
                while p.at(NEWLINE_KIND) {
                    p.r.i += 1;
                }
                let mut components = Vec::new();
                loop {
                    p.r.expect(COMPONENT)?;
                    let name = p.name()?;
                    p.r.expect(LBRACKET)?;
                    let evolution = p.number()?;
                    p.r.expect(RBRACKET)?;
                    let label = if p.at(LABEL) { Some(p.label()?) } else { None };
                    p.end_of_line()?;
                    components.push(PipelineComponentAst {
                        name,
                        evolution,
                        label,
                    });
                    if !p.at(COMPONENT) {
                        break;
                    }
                }
                p.r.expect(RBRACE)?;
                p.end_of_line()?;
                ast.pipelines.push(PipelineAst { parent, components });
            }
            NOTE => {
                p.r.i += 1;
                let text = string_value(&p.r.expect(STRING_KIND)?);
                let (visibility, evolution) = p.pair(Parser::number)?;
                p.end_of_line()?;
                ast.notes.push(NoteAst {
                    text,
                    visibility,
                    evolution,
                });
            }
            ANNOTATIONS => {
                p.r.i += 1;
                let xy = p.pair(Parser::coordinate)?;
                p.end_of_line()?;
                ast.annotations.push(xy);
            }
            ANNOTATION => {
                p.r.i += 1;
                let number = p.int()?;
                p.r.expect(COMMA)?;
                let (x, y) = p.pair(Parser::coordinate)?;
                let text = string_value(&p.r.expect(STRING_KIND)?);
                p.end_of_line()?;
                ast.annotation.push(AnnotationAst { number, x, y, text });
            }
            ACCELERATOR | DEACCELERATOR => {
                let slow = p.at(DEACCELERATOR);
                p.r.i += 1;
                let name = p.name()?;
                let (x, y) = p.pair(Parser::number)?;
                p.end_of_line()?;
                let force = PlacedAst { name, x, y };
                if slow {
                    ast.deaccelerators.push(force);
                } else {
                    ast.accelerators.push(force);
                }
            }
            _ => {
                if !p.is_name() {
                    return Err(p.r.fail("a statement"));
                }
                let from = p.name()?;
                let from_port = if p.at(LINK_PORT) {
                    Some(p.r.take())
                } else {
                    None
                };
                let arrow = if p.at(LINK_ARROW) || p.at(ARROW) {
                    Some(p.r.take())
                } else {
                    None
                };
                let to = p.name()?;
                let to_port = if p.at(LINK_PORT) {
                    Some(p.r.take())
                } else {
                    None
                };
                let link_label = if p.at(LINK_LABEL) {
                    let text = p.r.take();
                    Some(js::trim(&text[1..]).to_string())
                } else {
                    None
                };
                p.end_of_line()?;
                ast.links.push(LinkAst {
                    from,
                    from_port,
                    arrow,
                    to,
                    to_port,
                    link_label,
                });
            }
        }
    }
    Ok(ast)
}
