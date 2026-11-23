//! architecture の読み手（pele の `src/diagrams/architecture/parser.ts`）。Mermaid の Langium の文法を写す。

use std::sync::LazyLock;

use crate::diagrams::common::tokens::{
    ACC_DESCR, ACC_TITLE, DIRECTIVE, Lexicon, NEWLINE, Pattern, Reader, SINGLE_LINE_COMMENT,
    STRING, TokenType, WHITESPACE, YAML, acc_descr_value, acc_title_value, string_value,
    title_before, title_value, tokenize,
};
use crate::errors::Result;
use crate::js;
use crate::util::chars::{NONE, is_line_end, is_word};

fn at(src: &[u16], p: usize) -> u32 {
    src.get(p).map_or(NONE, |&c| c as u32)
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

/// `[-\w]*\w` がそこから合うか: `[-\w]` の続きに語の文字があるか。
fn word_rest(src: &[u16], mut q: usize) -> bool {
    loop {
        let c = at(src, q);
        if is_word(c) {
            return true;
        }
        if c != 45 {
            return false;
        }
        q += 1;
    }
}

/// `\{group\}`
fn arrow_group(src: &[u16], p: usize) -> Option<usize> {
    let word = js::utf16("{group}");
    src.get(p..)
        .is_some_and(|s| s.starts_with(&word))
        .then_some(p + word.len())
}

/// `\([\w-:]+\)`
fn arch_icon(src: &[u16], p: usize) -> Option<usize> {
    if at(src, p) != 40 {
        return None;
    }
    let mut e = p + 1;
    while is_word(at(src, e)) || matches!(at(src, e), 45 | 58) {
        e += 1;
    }
    (e > p + 1 && at(src, e) == 41).then_some(e + 1)
}

/// 引用符 `quote` の `"([^"\\]|\\.)*"` の終わり。
fn quoted(src: &[u16], p: usize, quote: u32) -> Option<usize> {
    if at(src, p) != quote {
        return None;
    }
    let mut i = p + 1;
    loop {
        let c = at(src, i);
        if c == NONE {
            return None;
        }
        if c == quote {
            return Some(i + 1);
        }
        if c == 92 {
            if at(src, i + 1) == NONE || is_line_end(at(src, i + 1)) {
                return None;
            }
            i += 2;
        } else {
            i += 1;
        }
    }
}

/// `\[(?:"([^"\\]|\\.)*"|'([^'\\]|\\.)*'|[^\[\]\r\n]+)\]`
fn arch_title(src: &[u16], p: usize) -> Option<usize> {
    if at(src, p) != 91 {
        return None;
    }
    for quote in [34, 39] {
        if let Some(e) = quoted(src, p + 1, quote)
            && at(src, e) == 93
        {
            return Some(e + 1);
        }
    }
    let mut e = p + 1;
    while e < src.len() && !matches!(src[e], 91 | 93 | 10 | 13) {
        e += 1;
    }
    (e > p + 1 && at(src, e) == 93).then_some(e + 1)
}

fn word(text: &str) -> TokenType {
    TokenType::literal(text, text).longer(&["ID"])
}

/// 向きの文字は識別子の前にあり、長い方を持たない。Mermaid は L・R・T・B で始まる id を、向きと残りとして読む。
pub static ARCHITECTURE_TOKENS: LazyLock<Lexicon> = LazyLock::new(|| {
    Lexicon::new(vec![
        word("architecture-beta"),
        word("junction"),
        word("service"),
        word("column"),
        word("group"),
        word("align"),
        word("row"),
        TokenType::literal("--", "--").longer(&["YAML"]),
        word("in"),
        TokenType::literal(":", ":"),
        TokenType::literal("-", "-").longer(&["YAML"]),
        TokenType::new(
            "ARROW_DIRECTION",
            Pattern::regex(|src, p| matches!(at(src, p), 76 | 82 | 84 | 66).then_some(p + 1)),
        )
        .first("LRTB"),
        TokenType::new("ARROW_GROUP", Pattern::regex(arrow_group)).first("{"),
        TokenType::new(
            "ARROW_INTO",
            Pattern::regex(|src, p| matches!(at(src, p), 60 | 62).then_some(p + 1)),
        )
        .first("<>"),
        ACC_DESCR.clone(),
        ACC_TITLE.clone(),
        title_before(word_rest),
        STRING.clone(),
        TokenType::new("ID", Pattern::regex(id))
            .first("_0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"),
        NEWLINE.clone(),
        WHITESPACE.clone(),
        YAML.clone(),
        DIRECTIVE.clone(),
        SINGLE_LINE_COMMENT.clone(),
        TokenType::new("ARCH_ICON", Pattern::regex(arch_icon)).first("("),
        TokenType::new("ARCH_TITLE", Pattern::regex(arch_title)).first("["),
    ])
});

// 字句の種類の番号（表の順）。
const HEADER: i32 = 0;
const JUNCTION: i32 = 1;
const SERVICE: i32 = 2;
const COLUMN: i32 = 3;
const GROUP: i32 = 4;
const ALIGN: i32 = 5;
const ROW: i32 = 6;
const DASHES: i32 = 7;
const IN: i32 = 8;
const COLON: i32 = 9;
const DASH: i32 = 10;
const DIRECTION: i32 = 11;
const ARROW_GROUP: i32 = 12;
const INTO: i32 = 13;
const ACC_DESCR_KIND: i32 = 14;
const ACC_TITLE_KIND: i32 = 15;
const TITLE_KIND: i32 = 16;
const STRING_KIND: i32 = 17;
const ID: i32 = 18;
const NEWLINE_KIND: i32 = 19;
const ICON: i32 = 24;
const LABEL: i32 = 25;

#[derive(Clone, Debug, PartialEq)]
pub struct GroupAst {
    pub id: String,
    pub icon: Option<String>,
    pub title: Option<String>,
    pub in_: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ServiceAst {
    pub id: String,
    pub icon: Option<String>,
    pub icon_text: Option<String>,
    pub title: Option<String>,
    pub in_: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JunctionAst {
    pub id: String,
    pub in_: Option<String>,
}

/// 辺の向き: 'L'・'R'・'T'・'B'。
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeAst {
    pub lhs_id: String,
    pub lhs_group: bool,
    pub lhs_dir: char,
    pub lhs_into: bool,
    pub title: Option<String>,
    pub rhs_into: bool,
    pub rhs_dir: char,
    pub rhs_id: String,
    pub rhs_group: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AlignmentAst {
    /// 'row' か 'column'。
    pub direction: String,
    pub members: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArchitectureAst {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub groups: Vec<GroupAst>,
    pub services: Vec<ServiceAst>,
    pub junctions: Vec<JunctionAst>,
    pub edges: Vec<EdgeAst>,
    pub alignments: Vec<AlignmentAst>,
}

/// 両端の 1 文字ずつを除く（どちらも ASCII）。
fn inner(text: &str) -> &str {
    if text.len() < 2 {
        return "";
    }
    &text[1..text.len() - 1]
}

/// Mermaid の ArchitectureValueConverter の ARCH_TITLE。
fn label_value(text: &str) -> String {
    let mut result = js::trim(inner(text)).to_string();
    if (result.starts_with('"') && result.ends_with('"'))
        || (result.starts_with('\'') && result.ends_with('\''))
    {
        result = inner(&result).replace("\\\"", "\"").replace("\\'", "'");
    }
    js::trim(&result).to_string()
}

pub fn parse_architecture(src: &str) -> Result<ArchitectureAst> {
    let lex = &*ARCHITECTURE_TOKENS;
    let mut r = Reader::new(tokenize(src, lex, "architecture")?, lex, "architecture");
    let mut ast = ArchitectureAst::default();

    while r.kind() == NEWLINE_KIND {
        r.i += 1;
    }
    r.expect(HEADER)?;
    while r.kind() != -1 {
        match r.kind() {
            NEWLINE_KIND => {
                r.i += 1;
                continue;
            }
            ACC_DESCR_KIND => ast.acc_descr = Some(acc_descr_value(&r.take())),
            ACC_TITLE_KIND => ast.acc_title = Some(acc_title_value(&r.take())),
            TITLE_KIND => ast.title = Some(title_value(&r.take())),
            GROUP => {
                r.i += 1;
                let mut group = GroupAst {
                    id: r.expect(ID)?,
                    icon: None,
                    title: None,
                    in_: None,
                };
                if r.kind() == ICON {
                    group.icon = Some(inner(&r.take()).to_string());
                }
                if r.kind() == LABEL {
                    group.title = Some(label_value(&r.take()));
                }
                if r.accept(IN) {
                    group.in_ = Some(r.expect(ID)?);
                }
                ast.groups.push(group);
            }
            SERVICE => {
                r.i += 1;
                let mut service = ServiceAst {
                    id: r.expect(ID)?,
                    icon: None,
                    icon_text: None,
                    title: None,
                    in_: None,
                };
                if r.kind() == STRING_KIND {
                    service.icon_text = Some(string_value(&r.take()));
                } else if r.kind() == ICON {
                    service.icon = Some(inner(&r.take()).to_string());
                }
                if r.kind() == LABEL {
                    service.title = Some(label_value(&r.take()));
                }
                if r.accept(IN) {
                    service.in_ = Some(r.expect(ID)?);
                }
                ast.services.push(service);
            }
            JUNCTION => {
                r.i += 1;
                let mut junction = JunctionAst {
                    id: r.expect(ID)?,
                    in_: None,
                };
                if r.accept(IN) {
                    junction.in_ = Some(r.expect(ID)?);
                }
                ast.junctions.push(junction);
            }
            ID => {
                let lhs_id = r.take();
                let lhs_group = r.accept(ARROW_GROUP);
                r.expect(COLON)?;
                let lhs_dir = r.expect(DIRECTION)?.chars().next().unwrap();
                let lhs_into = r.accept(INTO);
                let mut title = None;
                if !r.accept(DASHES) {
                    if r.kind() != DASH {
                        return Err(r.fail("'--' or '-'"));
                    }
                    r.i += 1;
                    title = Some(label_value(&r.expect(LABEL)?));
                    r.expect(DASH)?;
                }
                let rhs_into = r.accept(INTO);
                let rhs_dir = r.expect(DIRECTION)?.chars().next().unwrap();
                r.expect(COLON)?;
                let rhs_id = r.expect(ID)?;
                let rhs_group = r.accept(ARROW_GROUP);
                ast.edges.push(EdgeAst {
                    lhs_id,
                    lhs_group,
                    lhs_dir,
                    lhs_into,
                    title,
                    rhs_into,
                    rhs_dir,
                    rhs_id,
                    rhs_group,
                });
            }
            ALIGN => {
                r.i += 1;
                if r.kind() != ROW && r.kind() != COLUMN {
                    return Err(r.fail("'row' or 'column'"));
                }
                let direction = r.take();
                let mut members = vec![r.expect(ID)?, r.expect(ID)?];
                while r.kind() == ID {
                    members.push(r.take());
                }
                ast.alignments.push(AlignmentAst { direction, members });
            }
            _ => {
                return Err(r.fail(
                    "a group, a service, a junction, an edge, an alignment, a title, or a line break",
                ));
            }
        }
        // 行の終わり。
        if r.kind() == -1 {
            continue;
        }
        if r.kind() != NEWLINE_KIND {
            return Err(r.fail("a line break or the end of input"));
        }
        while r.kind() == NEWLINE_KIND {
            r.i += 1;
        }
    }
    Ok(ast)
}
