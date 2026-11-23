//! gitGraph の読み手（pele の `src/diagrams/git/parser.ts`）。字句は共通の `tokens` で切り、Langium の文法が作る
//! 形（AST）を手で組む。

use std::sync::LazyLock;

use crate::diagrams::common::tokens::{
    ACC_DESCR, ACC_TITLE, DIRECTIVE, Lexicon, NEWLINE, Pattern, Reader, SINGLE_LINE_COMMENT,
    STRING, TokenType, WHITESPACE, YAML, acc_descr_value, acc_title_value, keyword, string_value,
    title_before, title_value, tokenize,
};
use crate::errors::Result;
use crate::js;
use crate::util::chars::{NONE, is_space};

// 字句の種類（TS の `const enum T`）。表の並びと同じ番号。
const CHERRY_PICK: i32 = 0;
const GIT_GRAPH_COLON: i32 = 1;
const HIGHLIGHT: i32 = 2;
const GIT_GRAPH: i32 = 3;
const CHECKOUT: i32 = 4;
const REVERSE: i32 = 5;
const PARENT: i32 = 6;
const COMMIT: i32 = 7;
const NORMAL: i32 = 8;
const BRANCH: i32 = 9;
const ORDER: i32 = 10;
const SWITCH: i32 = 11;
const TYPE: i32 = 12;
const MERGE: i32 = 13;
const MSG: i32 = 14;
const TAG: i32 = 15;
const ID: i32 = 16;
const LR: i32 = 17;
const TB: i32 = 18;
const BT: i32 = 19;
const COLON: i32 = 20;
const ACC_DESCR_T: i32 = 21;
const ACC_TITLE_T: i32 = 22;
const TITLE_T: i32 = 23;
const INT: i32 = 24;
const STRING_T: i32 = 25;
const NEWLINE_T: i32 = 26;
const REFERENCE: i32 = 31;

fn at(src: &[u16], p: usize) -> u32 {
    src.get(p).map_or(NONE, |&c| c as u32)
}

/// `\w`（ASCII の英数字と `_`）。
fn word_char(c: u32) -> bool {
    c < 128 && (c as u8).is_ascii_alphanumeric() || c == 95
}

/// `[-\w]`。
fn name_end(c: u32) -> bool {
    word_char(c) || c == 45
}

/// `[-./\w]`。
fn name_char(c: u32) -> bool {
    name_end(c) || c == 46 || c == 47
}

/// `\w([-\./\w]*[-\w])?`
fn reference(src: &[u16], p: usize) -> Option<usize> {
    if !word_char(at(src, p)) {
        return None;
    }
    let mut end = p + 1;
    let mut q = p + 1;
    while name_char(at(src, q)) {
        if name_end(at(src, q)) {
            end = q + 1;
        }
        q += 1;
    }
    Some(end)
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
    let mut q = p + 1;
    while (48..=57).contains(&at(src, q)) {
        q += 1;
    }
    if at(src, q) != 46 {
        return Some(q);
    }
    // 後戻り: 1 つ短くすれば次は数字で、`.` ではない。
    (q - 1 > p).then_some(q - 1)
}

/// `[\t ]*accDescr\s*{`
fn acc_descr_opener(src: &[u16], p: usize) -> Option<usize> {
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

/// `[-./\w]*[-\w]` がそこに合うか。
fn name_rest(src: &[u16], mut q: usize) -> bool {
    while name_char(at(src, q)) {
        if name_end(at(src, q)) {
            return true;
        }
        q += 1;
    }
    false
}

/// 長い名前の頭になる語は、その名前の一部（`commits` はブランチ）。
fn word(name: &str) -> TokenType {
    TokenType::literal(name, name).longer(&["REFERENCE"])
}

pub static GIT_TOKENS: LazyLock<Lexicon> = LazyLock::new(|| {
    Lexicon::new(vec![
        word("cherry-pick"),
        TokenType::literal("gitGraph:", "gitGraph:"),
        word("HIGHLIGHT"),
        keyword("gitGraph").longer(&["REFERENCE"]),
        word("checkout"),
        word("REVERSE"),
        TokenType::literal("parent:", "parent:"),
        word("commit"),
        word("NORMAL"),
        word("branch"),
        TokenType::literal("order:", "order:"),
        word("switch"),
        TokenType::literal("type:", "type:"),
        word("merge"),
        TokenType::literal("msg:", "msg:"),
        TokenType::literal("tag:", "tag:"),
        TokenType::literal("id:", "id:"),
        word("LR"),
        word("TB"),
        word("BT"),
        TokenType::literal(":", ":"),
        // 閉じの波かっこがなければ、かっこの形は文字の終わりまで走査する。波かっこは単独の字句ではないので、
        // 最初の失敗で決まる。
        ACC_DESCR.clone().opener(acc_descr_opener),
        ACC_TITLE.clone(),
        title_before(name_rest),
        TokenType::new("INT", Pattern::regex(int)),
        STRING.clone(),
        NEWLINE.clone(),
        WHITESPACE.clone(),
        YAML.clone(),
        DIRECTIVE.clone(),
        SINGLE_LINE_COMMENT.clone(),
        TokenType::new("REFERENCE", Pattern::regex(reference)),
    ])
});

/// コミットの種類の名前。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitTypeName {
    Normal,
    Reverse,
    Highlight,
}

#[derive(Clone, Debug, Default)]
pub struct CommitAst {
    pub id: Option<String>,
    pub message: Option<String>,
    pub tags: Vec<String>,
    pub type_: Option<CommitTypeName>,
}

#[derive(Clone, Debug)]
pub struct BranchAst {
    pub name: String,
    pub order: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct MergeAst {
    pub branch: String,
    pub id: Option<String>,
    pub tags: Vec<String>,
    pub type_: Option<CommitTypeName>,
}

#[derive(Clone, Debug)]
pub struct CheckoutAst {
    pub branch: String,
}

#[derive(Clone, Debug, Default)]
pub struct CherryPickingAst {
    pub id: Option<String>,
    pub tags: Vec<String>,
    pub parent: Option<String>,
}

#[derive(Clone, Debug)]
pub enum StatementAst {
    Commit(CommitAst),
    Branch(BranchAst),
    Merge(MergeAst),
    Checkout(CheckoutAst),
    CherryPicking(CherryPickingAst),
}

/// Langium は頭に向きがあると、根を `Direction` の規則の名前で呼ぶ（ここでは `dir` があるかで分かる）。
#[derive(Clone, Debug, Default)]
pub struct GitGraphAst {
    pub dir: Option<String>,
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub statements: Vec<StatementAst>,
}

fn end_of_line(r: &mut Reader) -> Result<()> {
    if r.kind() == -1 {
        return Ok(());
    }
    if r.kind() != NEWLINE_T {
        return Err(r.fail("a line break or the end of input"));
    }
    while r.kind() == NEWLINE_T {
        r.i += 1;
    }
    Ok(())
}

fn name(r: &mut Reader) -> Result<String> {
    if r.kind() == REFERENCE {
        return Ok(r.take());
    }
    if r.kind() == STRING_T {
        return Ok(string_value(&r.take()));
    }
    Err(r.fail("a branch name"))
}

fn string(r: &mut Reader) -> Result<String> {
    Ok(string_value(&r.expect(STRING_T)?))
}

fn commit_type(r: &mut Reader) -> Result<CommitTypeName> {
    let t = match r.kind() {
        NORMAL => CommitTypeName::Normal,
        REVERSE => CommitTypeName::Reverse,
        HIGHLIGHT => CommitTypeName::Highlight,
        _ => return Err(r.fail("NORMAL, REVERSE, or HIGHLIGHT")),
    };
    r.i += 1;
    Ok(t)
}

pub fn parse_git(src: &str) -> Result<GitGraphAst> {
    let lex = &*GIT_TOKENS;
    let mut r = Reader::new(tokenize(src, lex, "gitGraph")?, lex, "gitGraph");
    let mut ast = GitGraphAst::default();

    while r.kind() == NEWLINE_T {
        r.i += 1;
    }
    if !r.accept(GIT_GRAPH_COLON) {
        r.expect(GIT_GRAPH)?;
        let next = r.kind_at(r.i + 1);
        if matches!(r.kind(), LR | TB | BT) && next == COLON {
            ast.dir = Some(r.take());
            r.i += 1;
        } else {
            r.accept(COLON);
        }
    }

    while r.kind() != -1 {
        match r.kind() {
            NEWLINE_T => r.i += 1,
            ACC_DESCR_T => {
                ast.acc_descr = Some(acc_descr_value(&r.take()));
                end_of_line(&mut r)?;
            }
            ACC_TITLE_T => {
                ast.acc_title = Some(acc_title_value(&r.take()));
                end_of_line(&mut r)?;
            }
            TITLE_T => {
                ast.title = Some(title_value(&r.take()));
                end_of_line(&mut r)?;
            }
            COMMIT => {
                r.i += 1;
                let mut node = CommitAst::default();
                loop {
                    if r.accept(ID) {
                        node.id = Some(string(&mut r)?);
                    } else if r.accept(TAG) {
                        node.tags.push(string(&mut r)?);
                    } else if r.accept(TYPE) {
                        node.type_ = Some(commit_type(&mut r)?);
                    } else if r.accept(MSG) || r.kind() == STRING_T {
                        node.message = Some(string(&mut r)?);
                    } else {
                        break;
                    }
                }
                end_of_line(&mut r)?;
                ast.statements.push(StatementAst::Commit(node));
            }
            BRANCH => {
                r.i += 1;
                let mut node = BranchAst {
                    name: name(&mut r)?,
                    order: None,
                };
                if r.accept(ORDER) {
                    node.order = Some(js::parse_int(&r.expect(INT)?, 10));
                }
                end_of_line(&mut r)?;
                ast.statements.push(StatementAst::Branch(node));
            }
            MERGE => {
                r.i += 1;
                let mut node = MergeAst {
                    branch: name(&mut r)?,
                    id: None,
                    tags: Vec::new(),
                    type_: None,
                };
                loop {
                    if r.accept(ID) {
                        node.id = Some(string(&mut r)?);
                    } else if r.accept(TAG) {
                        node.tags.push(string(&mut r)?);
                    } else if r.accept(TYPE) {
                        node.type_ = Some(commit_type(&mut r)?);
                    } else {
                        break;
                    }
                }
                end_of_line(&mut r)?;
                ast.statements.push(StatementAst::Merge(node));
            }
            CHECKOUT | SWITCH => {
                r.i += 1;
                let node = CheckoutAst {
                    branch: name(&mut r)?,
                };
                end_of_line(&mut r)?;
                ast.statements.push(StatementAst::Checkout(node));
            }
            CHERRY_PICK => {
                r.i += 1;
                let mut node = CherryPickingAst::default();
                loop {
                    if r.accept(ID) {
                        node.id = Some(string(&mut r)?);
                    } else if r.accept(TAG) {
                        node.tags.push(string(&mut r)?);
                    } else if r.accept(PARENT) {
                        node.parent = Some(string(&mut r)?);
                    } else {
                        break;
                    }
                }
                end_of_line(&mut r)?;
                ast.statements.push(StatementAst::CherryPicking(node));
            }
            _ => return Err(r.fail("a statement, a title, or a line break")),
        }
    }
    Ok(ast)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_names_and_numbers() {
        let s = js::utf16("a.b/ ab. x-");
        assert_eq!(reference(&s, 0), Some(3));
        assert_eq!(reference(&s, 5), Some(7));
        assert_eq!(reference(&s, 9), Some(11));
        assert_eq!(int(&js::utf16("12.5"), 0), Some(1));
        assert_eq!(int(&js::utf16("1.5"), 0), None);
        assert_eq!(int(&js::utf16("0.5"), 0), Some(1));
        assert_eq!(int(&js::utf16("120"), 0), Some(3));
    }
}
