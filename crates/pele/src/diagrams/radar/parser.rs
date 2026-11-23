//! radar の読み手（pele の `src/diagrams/radar/parser.ts`）。字句は共通の `tokens` で切り、Langium の文法が作る
//! 形（AST）を手で組む。

use std::sync::LazyLock;

use crate::diagrams::common::tokens::{
    ACC_DESCR, ACC_TITLE, DIRECTIVE, Lexicon, NEWLINE, Pattern, Reader, SINGLE_LINE_COMMENT,
    STRING, Scanner, TokenType, WHITESPACE, YAML, acc_descr_value, acc_title_value, keyword,
    string_value, title_before, title_value, tokenize,
};
use crate::errors::Result;
use crate::js;
use crate::util::chars::{NONE, is_space, is_word};

// 字句の種類（TS の `const enum T`）。表の並びと同じ番号。
const RADAR_COLON: i32 = 0;
const RADAR: i32 = 1;
const SHOW_LEGEND: i32 = 2;
const GRATICULE: i32 = 3;
const CURVE: i32 = 4;
const TICKS: i32 = 5;
const AXIS: i32 = 6;
const MAX: i32 = 7;
const MIN: i32 = 8;
const COLON: i32 = 9;
const COMMA: i32 = 10;
const OPEN: i32 = 11;
const CLOSE: i32 = 12;
const OPEN_BRACE: i32 = 13;
const CLOSE_BRACE: i32 = 14;
const GRATICULE_VALUE: i32 = 15;
const BOOLEAN: i32 = 16;
const ACC_DESCR_T: i32 = 17;
const ACC_TITLE_T: i32 = 18;
const TITLE_T: i32 = 19;
const NUMBER: i32 = 20;
const STRING_T: i32 = 21;
const ID: i32 = 22;
const NEWLINE_T: i32 = 23;

fn at(src: &[u16], p: usize) -> u32 {
    src.get(p).map_or(NONE, |&c| c as u32)
}

fn digits(src: &[u16], mut end: usize) -> usize {
    while (48..=57).contains(&at(src, end)) {
        end += 1;
    }
    end
}

/// 後戻りのない NUMBER（`(?:[0-9]+\.[0-9]+(?!\.))|(?:0|[1-9][0-9]*(?!\.))`）。正規表現では、長い数字の並びの
/// どの位置からも並びの終わりまで読むので、長い 0 の並びで二乗になる。並びの終わりを覚えておく。
fn match_number(src: &[u16], at_: usize, run: &mut (usize, usize)) -> Option<usize> {
    if at_ < run.0 || at_ >= run.1 {
        *run = (at_, digits(src, at_));
    }
    let whole = run.1;
    if whole == at_ {
        return None;
    }
    let dot = at(src, whole) == 46;
    if dot {
        let fraction = digits(src, whole + 1);
        let length = fraction - whole - 1;
        if length > 0 && at(src, fraction) != 46 {
            return Some(fraction);
        }
        if length > 1 {
            return Some(fraction - 1);
        }
    }
    if at(src, at_) == 48 {
        return Some(at_ + 1);
    }
    if !dot {
        return Some(whole);
    }
    (whole - 1 > at_).then_some(whole - 1)
}

/// `[\t ]*accDescr\s*{`
fn acc_descr_block(src: &[u16], p: usize) -> Option<usize> {
    let mut q = p;
    while matches!(at(src, q), 9 | 32) {
        q += 1;
    }
    if !src
        .get(q..)
        .is_some_and(|s| s.starts_with(&b"accDescr".map(u16::from)))
    {
        return None;
    }
    q += 8;
    while is_space(at(src, q)) {
        q += 1;
    }
    (at(src, q) == 123).then_some(q + 1)
}

/// ほかの文法では、閉じない `accDescr {` で字句解析が終わる。ここでは `accDescr` は名前でもあり `{` は字句なので、
/// 閉じないものごとに文字の終わりまで読み直すことになる。最後の `}` より先に閉じはない。
fn acc_descr_scanner() -> Scanner {
    let Pattern::Regex(re) = ACC_DESCR.pattern.clone() else {
        unreachable!("ACC_DESCR is a regex")
    };
    let mut last_brace: Option<isize> = None;
    Box::new(move |src, p, previous| {
        if let Some(end) = acc_descr_block(src, p) {
            let last = *last_brace.get_or_insert_with(|| {
                src.iter()
                    .rposition(|&c| c == 125)
                    .map_or(-1, |k| k as isize)
            });
            if end as isize > last {
                return None;
            }
        }
        re(src, p, previous)
    })
}

/// `[\w]([-\w]*\w)?`
fn id(src: &[u16], p: usize) -> Option<usize> {
    if !is_word(at(src, p)) {
        return None;
    }
    let mut end = p + 1;
    let mut q = p + 1;
    while is_word(at(src, q)) || at(src, q) == 45 {
        if is_word(at(src, q)) {
            end = q + 1;
        }
        q += 1;
    }
    Some(end)
}

/// `[-\w]*\w` がそこに合うか。
fn name_rest(src: &[u16], mut q: usize) -> bool {
    while is_word(at(src, q)) || at(src, q) == 45 {
        if is_word(at(src, q)) {
            return true;
        }
        q += 1;
    }
    false
}

/// 長い名前に譲る。ただしその設定のすぐ後では譲らない（`graticule polygonaxis`）。
fn option_value(words: &'static [&'static str], option: i32) -> Pattern {
    Pattern::matcher(move |src, p, previous| {
        let end = words.iter().find_map(|w| {
            let w16: Vec<u16> = w.encode_utf16().collect();
            src.get(p..)
                .is_some_and(|s| s.starts_with(&w16))
                .then_some(p + w16.len())
        })?;
        if previous == option || !name_rest(src, end) {
            Some(end)
        } else {
            None
        }
    })
}

fn word(text: &str) -> TokenType {
    TokenType::literal(text, text).longer(&["ID"])
}

fn mark(text: &str) -> TokenType {
    TokenType::literal(text, text)
}

pub static RADAR_TOKENS: LazyLock<Lexicon> = LazyLock::new(|| {
    Lexicon::new(vec![
        mark("radar-beta:"),
        keyword("radar-beta").longer(&["ID"]),
        word("showLegend"),
        word("graticule"),
        word("curve"),
        word("ticks"),
        word("axis"),
        word("max"),
        word("min"),
        mark(":"),
        mark(","),
        mark("["),
        mark("]"),
        mark("{"),
        mark("}"),
        TokenType::new("GRATICULE", option_value(&["circle", "polygon"], GRATICULE)),
        TokenType::new("BOOLEAN", option_value(&["true", "false"], SHOW_LEGEND)),
        ACC_DESCR
            .clone()
            .with_pattern(Pattern::scanner(acc_descr_scanner)),
        ACC_TITLE.clone(),
        title_before(name_rest),
        TokenType::new(
            "NUMBER",
            Pattern::scanner(|| {
                let mut run = (0, 0);
                Box::new(move |src, p, _| match_number(src, p, &mut run))
            }),
        ),
        STRING.clone(),
        TokenType::new("ID", Pattern::regex(id)),
        NEWLINE.clone(),
        WHITESPACE.clone(),
        YAML.clone(),
        DIRECTIVE.clone(),
        SINGLE_LINE_COMMENT.clone(),
    ])
});

#[derive(Clone, Debug)]
pub struct AxisAst {
    pub name: String,
    pub label: Option<String>,
}

#[derive(Clone, Debug)]
pub struct EntryAst {
    /// Langium は参照を解かない。Mermaid は文字だけを読む。
    pub axis: Option<String>,
    pub value: f64,
}

#[derive(Clone, Debug)]
pub struct CurveAst {
    pub name: String,
    pub label: Option<String>,
    pub entries: Vec<EntryAst>,
}

#[derive(Clone, Debug)]
pub enum OptionValue {
    Bool(bool),
    Num(f64),
    Str(String),
}

#[derive(Clone, Debug)]
pub struct OptionAst {
    pub name: String,
    pub value: OptionValue,
}

#[derive(Clone, Debug, Default)]
pub struct RadarAst {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub axes: Vec<AxisAst>,
    pub curves: Vec<CurveAst>,
    pub options: Vec<OptionAst>,
}

fn newlines(r: &mut Reader) {
    while r.kind() == NEWLINE_T {
        r.i += 1;
    }
}

fn end_of_line(r: &mut Reader) -> Result<()> {
    if r.kind() == -1 {
        return Ok(());
    }
    if r.kind() != NEWLINE_T {
        return Err(r.fail("a line break or the end of input"));
    }
    newlines(r);
    Ok(())
}

fn label(r: &mut Reader) -> Result<Option<String>> {
    if !r.accept(OPEN) {
        return Ok(None);
    }
    let text = string_value(&r.expect(STRING_T)?);
    r.expect(CLOSE)?;
    Ok(Some(text))
}

fn entry(r: &mut Reader, detailed: bool) -> Result<EntryAst> {
    if !detailed {
        return Ok(EntryAst {
            axis: None,
            value: js::number(&r.expect(NUMBER)?),
        });
    }
    let axis = Some(r.expect(ID)?);
    r.accept(COLON);
    Ok(EntryAst {
        axis,
        value: js::number(&r.expect(NUMBER)?),
    })
}

fn curve(r: &mut Reader) -> Result<CurveAst> {
    let name = r.expect(ID)?;
    let label = label(r)?;
    let mut node = CurveAst {
        name,
        label,
        entries: Vec::new(),
    };
    r.expect(OPEN_BRACE)?;
    newlines(r);
    if r.kind() != NUMBER && r.kind() != ID {
        return Err(r.fail("a number or an axis name"));
    }
    let detailed = r.kind() == ID;
    node.entries.push(entry(r, detailed)?);
    while r.accept(COMMA) {
        newlines(r);
        node.entries.push(entry(r, detailed)?);
    }
    newlines(r);
    r.expect(CLOSE_BRACE)?;
    Ok(node)
}

fn axis(r: &mut Reader) -> Result<AxisAst> {
    let name = r.expect(ID)?;
    let label = label(r)?;
    Ok(AxisAst { name, label })
}

fn option(r: &mut Reader) -> Result<OptionAst> {
    let kind = r.kind();
    let name = r.take();
    let value = if kind == SHOW_LEGEND {
        OptionValue::Bool(r.expect(BOOLEAN)? == "true")
    } else if kind == GRATICULE {
        OptionValue::Str(r.expect(GRATICULE_VALUE)?)
    } else {
        OptionValue::Num(js::number(&r.expect(NUMBER)?))
    };
    Ok(OptionAst { name, value })
}

fn is_option(r: &Reader) -> bool {
    matches!(r.kind(), SHOW_LEGEND | TICKS | MAX | MIN | GRATICULE)
}

pub fn parse_radar(src: &str) -> Result<RadarAst> {
    let mut r = Reader::new(
        tokenize(src, &RADAR_TOKENS, "radar")?,
        &RADAR_TOKENS,
        "radar",
    );
    let mut ast = RadarAst::default();
    let r = &mut r;

    newlines(r);
    if !r.accept(RADAR_COLON) {
        r.expect(RADAR)?;
        r.accept(COLON);
    }
    newlines(r);
    while r.kind() != -1 {
        match r.kind() {
            NEWLINE_T => r.i += 1,
            ACC_DESCR_T => {
                ast.acc_descr = Some(acc_descr_value(&r.take()));
                end_of_line(r)?;
            }
            ACC_TITLE_T => {
                ast.acc_title = Some(acc_title_value(&r.take()));
                end_of_line(r)?;
            }
            TITLE_T => {
                ast.title = Some(title_value(&r.take()));
                end_of_line(r)?;
            }
            AXIS => {
                r.i += 1;
                loop {
                    ast.axes.push(axis(r)?);
                    if !r.accept(COMMA) {
                        break;
                    }
                }
            }
            CURVE => {
                r.i += 1;
                loop {
                    ast.curves.push(curve(r)?);
                    if !r.accept(COMMA) {
                        break;
                    }
                }
            }
            _ => {
                if !is_option(r) {
                    return Err(r.fail("an axis, a curve, an option, a title, or a line break"));
                }
                ast.options.push(option(r)?);
                while r.accept(COMMA) {
                    if !is_option(r) {
                        return Err(r.fail("an option"));
                    }
                    ast.options.push(option(r)?);
                }
            }
        }
    }
    Ok(ast)
}
