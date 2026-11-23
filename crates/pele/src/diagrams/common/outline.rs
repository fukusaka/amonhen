//! Mermaid の mindmap と kanban に共通の、字下げの見出しの文法（pele の `src/diagrams/common/outline.ts`）。
//! kanban はノードの後に `@{ ... }` のメタデータを足し、ノードの id に `@` を入れない。JS と同じく UTF-16 の符号
//! 単位で読む。

use crate::errors::{Result, syntax_error};
use crate::js;
use crate::util::chars::{NONE, is_line_end, is_space, is_word};

/// 字句の種類（TS の `const enum T`）。
pub mod t {
    pub const END: u8 = 0;
    pub const ERROR: u8 = 1;
    pub const SPACELINE: u8 = 2;
    pub const NL: u8 = 3;
    pub const EOF: u8 = 4;
    pub const KEYWORD: u8 = 5;
    pub const SPACELIST: u8 = 6;
    pub const ICON: u8 = 7;
    pub const CLASS: u8 = 8;
    pub const NODE_DSTART: u8 = 9;
    pub const NODE_ID: u8 = 10;
    pub const NODE_DESCR: u8 = 11;
    pub const NODE_DEND: u8 = 12;
    pub const SHAPE_DATA: u8 = 13;
}

/// 生成した読み手が書く名前。入力の終わりには名前がなく、番号を書く。
const NAMES: [&str; 14] = [
    "1",
    "INVALID",
    "SPACELINE",
    "NL",
    "EOF",
    "",
    "SPACELIST",
    "ICON",
    "CLASS",
    "NODE_DSTART",
    "NODE_ID",
    "NODE_DESCR",
    "NODE_DEND",
    "SHAPE_DATA",
];

pub fn token_name(type_: u8, kanban: bool) -> &'static str {
    if type_ == t::KEYWORD {
        if kanban { "KANBAN" } else { "MINDMAP" }
    } else {
        NAMES[type_ as usize]
    }
}

pub struct Tokens {
    pub types: Vec<u8>,
    pub texts: Vec<String>,
    pub starts: Vec<usize>,
}

/// ノードの形（TS の `NODE_TYPE`）。
pub mod node_type {
    pub const DEFAULT: i32 = 0;
    pub const NO_BORDER: i32 = 0;
    pub const ROUNDED_RECT: i32 = 1;
    pub const RECT: i32 = 2;
    pub const CIRCLE: i32 = 3;
    pub const CLOUD: i32 = 4;
    pub const BANG: i32 = 5;
    pub const HEXAGON: i32 = 6;
}

/// Mermaid と同じく、どの開きの印もどの閉じの印と対になる。
pub fn get_type(start: &str, end: &str) -> i32 {
    match start {
        "[" => node_type::RECT,
        "(" => {
            if end == ")" {
                node_type::ROUNDED_RECT
            } else {
                node_type::CLOUD
            }
        }
        "((" => node_type::CIRCLE,
        ")" => node_type::CLOUD,
        "))" => node_type::BANG,
        "{{" => node_type::HEXAGON,
        _ => node_type::DEFAULT,
    }
}

/// ノードの飾り。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decoration {
    Icon(String),
    Class(String),
}

/// 文法の動作が呼ぶもの。
pub trait OutlineDb {
    fn get_type(&self, start: &str, end: &str) -> i32 {
        get_type(start, end)
    }
    fn add_node(
        &mut self,
        level: usize,
        id: &str,
        descr: &str,
        type_: i32,
        shape_data: Option<&str>,
    ) -> Result<()>;
    fn decorate_node(&mut self, decoration: Decoration);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum S {
    Initial,
    Class,
    Icon,
    Node,
    Nstr,
    Nstr2,
    Data,
    DataStr,
}

fn at(src: &[u16], p: usize) -> u32 {
    src.get(p).map_or(NONE, |&c| c as u32)
}

/// Jison の大文字小文字を区別しない印は、ASCII の英字だけをたたむ。
fn letters(src: &[u16], p: usize, word: &str) -> bool {
    word.bytes()
        .enumerate()
        .all(|(k, w)| at(src, p + k) | 32 == w as u32)
}

/// 粘着の正規表現の代わり: `p` から、`ok` な符号単位が 1 つ以上続けば、その終わり。
fn run(src: &[u16], p: usize, ok: impl Fn(u32) -> bool) -> Option<usize> {
    let mut e = p;
    while e < src.len() && ok(src[e] as u32) {
        e += 1;
    }
    (e > p).then_some(e)
}

/// `.+`
fn re_line(src: &[u16], p: usize) -> Option<usize> {
    run(src, p, |c| !is_line_end(c))
}

/// `[^([\n){}]+`（kanban は `@` も除く）
fn re_id(src: &[u16], p: usize, kanban: bool) -> Option<usize> {
    run(src, p, |c| {
        !matches!(c, 40 | 91 | 10 | 41 | 123 | 125) && !(kanban && c == 64)
    })
}

/// `[^)\](}]+`
fn re_descr(src: &[u16], p: usize) -> Option<usize> {
    run(src, p, |c| !matches!(c, 41 | 93 | 40 | 125))
}

/// `` [^`"]+ ``
fn re_md(src: &[u16], p: usize) -> Option<usize> {
    run(src, p, |c| !matches!(c, 96 | 34))
}

/// `[^}^"]+`
fn re_data(src: &[u16], p: usize) -> Option<usize> {
    run(src, p, |c| !matches!(c, 125 | 94 | 34))
}

/// `.replace(/\n\s*/g, '<br/>')`
fn breaks(text: &[u16]) -> String {
    let mut out: Vec<u16> = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if text[i] == 10 {
            i += 1;
            while i < text.len() && is_space(text[i] as u32) {
                i += 1;
            }
            out.extend("<br/>".encode_utf16());
        } else {
            out.push(text[i]);
            i += 1;
        }
    }
    js::from16(&out)
}

struct Lexer<'a> {
    src: &'a [u16],
    types: Vec<u8>,
    texts: Vec<String>,
    starts: Vec<usize>,
    state: S,
    p: usize,
}

impl Lexer<'_> {
    fn put(&mut self, type_: u8, text: String, end: usize) {
        self.types.push(type_);
        self.texts.push(text);
        self.starts.push(self.p);
        self.p = end;
    }

    fn emit(&mut self, type_: u8, end: usize, next: S) {
        let text = js::from16(&self.src[self.p..end]);
        self.put(type_, text, end);
        self.state = next;
    }

    fn up_to(&self, ch: u16) -> usize {
        self.src[self.p..]
            .iter()
            .position(|&c| c == ch)
            .map_or(self.src.len(), |k| self.p + k)
    }
}

pub fn tokenize(src: &[u16], kanban: bool) -> Tokens {
    let n = src.len();
    let keyword = if kanban { "kanban" } else { "mindmap" };
    let mut lx = Lexer {
        src,
        types: Vec::new(),
        texts: Vec::new(),
        starts: Vec::new(),
        state: S::Initial,
        p: 0,
    };
    let mut next_break: Option<usize> = None;

    while lx.p < n {
        let p = lx.p;
        let c = src[p] as u32;
        let d = at(src, p + 1);
        match lx.state {
            S::Initial => {
                let w = if c <= 32 || c >= 160 {
                    run(src, p, is_space).unwrap_or(p)
                } else {
                    p
                };
                if kanban && c == 64 && d == 123 {
                    lx.put(t::SHAPE_DATA, String::new(), p + 2);
                    lx.state = S::Data;
                } else if at(src, w) == 37 && at(src, w + 1) == 37 {
                    lx.emit(t::SPACELINE, re_line(src, w).unwrap(), S::Initial);
                } else if w > p {
                    if next_break.is_none_or(|b| b < p) {
                        next_break = Some(lx.up_to(10));
                    }
                    if next_break.unwrap() >= w {
                        lx.emit(t::SPACELIST, w, S::Initial);
                    } else if let Some(e) =
                        src[..w].iter().rposition(|&c| c == 10).filter(|&e| e > p)
                    {
                        lx.emit(t::SPACELINE, e + 1, S::Initial);
                    } else {
                        lx.emit(t::NL, p + 1, S::Initial);
                    }
                } else if letters(src, p, keyword) && !is_word(at(src, p + keyword.len())) {
                    lx.emit(t::KEYWORD, p + keyword.len(), S::Initial);
                } else if c == 58 && d == 58 && at(src, p + 2) == 58 {
                    lx.p += 3;
                    lx.state = S::Class;
                } else if c == 58 && d == 58 && letters(src, p + 2, "icon") && at(src, p + 6) == 40
                {
                    lx.p += 7;
                    lx.state = S::Icon;
                } else if (c == 45 && d == 41)
                    || (c == 40 && (d == 45 || d == 40))
                    || ((c == 41 || c == 123) && d == c)
                {
                    lx.emit(t::NODE_DSTART, p + 2, S::Node);
                } else if c == 41 || c == 40 || c == 91 {
                    lx.emit(t::NODE_DSTART, p + 1, S::Node);
                } else if let Some(e) = re_id(src, p, kanban) {
                    lx.emit(t::NODE_ID, e, S::Initial);
                } else {
                    break;
                }
            }
            S::Node => {
                if c == 34 {
                    lx.state = if d == 96 { S::Nstr2 } else { S::Nstr };
                    lx.p += if d == 96 { 2 } else { 1 };
                } else if c == 41 || c == 40 {
                    let end = if d == c || (c == 40 && d == 45) {
                        p + 2
                    } else {
                        p + 1
                    };
                    lx.emit(t::NODE_DEND, end, S::Initial);
                } else if c == 93 {
                    lx.emit(t::NODE_DEND, p + 1, S::Initial);
                } else if (c == 125 && d == 125) || (c == 45 && d == 41) {
                    lx.emit(t::NODE_DEND, p + 2, S::Initial);
                } else {
                    // 1 つだけの閉じの波かっこは、行の残りを文字に取る。
                    let e = if c == 125 {
                        re_line(src, p)
                    } else {
                        re_descr(src, p)
                    };
                    lx.emit(t::NODE_DESCR, e.expect("descr"), S::Node);
                }
            }
            S::Nstr => {
                if c == 34 {
                    lx.p += 1;
                    lx.state = S::Node;
                } else {
                    let e = lx.up_to(34);
                    lx.emit(t::NODE_DESCR, e, S::Nstr);
                }
            }
            S::Nstr2 => {
                if c == 96 && d == 34 {
                    lx.p += 2;
                    lx.state = S::Node;
                } else if let Some(e) = re_md(src, p) {
                    lx.emit(t::NODE_DESCR, e, S::Nstr2);
                } else {
                    break;
                }
            }
            S::Class => {
                if let Some(e) = re_line(src, p) {
                    lx.emit(t::CLASS, e, S::Initial);
                } else if c == 10 {
                    lx.p += 1;
                    lx.state = S::Initial;
                } else {
                    break;
                }
            }
            S::Icon => {
                if c == 41 {
                    lx.p += 1;
                    lx.state = S::Initial;
                } else {
                    let e = lx.up_to(41);
                    lx.emit(t::ICON, e, S::Icon);
                }
            }
            S::Data => {
                if c == 34 {
                    lx.emit(t::SHAPE_DATA, p + 1, S::DataStr);
                } else if c == 125 {
                    lx.p += 1;
                    lx.state = S::Initial;
                } else if let Some(e) = re_data(src, p) {
                    lx.emit(t::SHAPE_DATA, e, S::Data);
                } else {
                    break;
                }
            }
            S::DataStr => {
                if c == 34 {
                    lx.emit(t::SHAPE_DATA, p + 1, S::Data);
                } else {
                    let e = lx.up_to(34);
                    let text = breaks(&src[p..e]);
                    lx.put(t::SHAPE_DATA, text, e);
                }
            }
        }
    }

    if lx.p < n {
        lx.put(t::ERROR, String::new(), lx.p);
    } else {
        if lx.state == S::Initial {
            lx.put(t::EOF, String::new(), n);
        }
        lx.put(t::END, String::new(), n);
    }
    Tokens {
        types: lx.types,
        texts: lx.texts,
        starts: lx.starts,
    }
}

const STOP: &str = "'SPACELINE', 'NL', 'EOF'";

fn is_start(type_: u8) -> bool {
    matches!(
        type_,
        t::SPACELINE | t::SPACELIST | t::ICON | t::CLASS | t::NODE_DSTART | t::NODE_ID
    )
}

fn is_stop(type_: u8) -> bool {
    type_ == t::SPACELINE || type_ == t::NL || type_ == t::EOF
}

/// 文法の動作と同じ順に `db` を呼び、生成した読み手と同じ状態で失敗を知らせる。
pub fn parse_outline(src: &str, db: &mut dyn OutlineDb, kanban: bool) -> Result<()> {
    let diagram = if kanban { "kanban" } else { "mindmap" };
    let src16 = js::utf16(src);
    let Tokens {
        types,
        texts,
        starts,
    } = tokenize(&src16, kanban);
    let data = if kanban { ", 'SHAPE_DATA'" } else { "" };
    let mut i = 0;

    let fail = |i: usize, expected: &str| {
        let t_ = types[i];
        if t_ == t::ERROR {
            return syntax_error(diagram, &src16, starts[i], "", true);
        }
        syntax_error(
            diagram,
            &src16,
            starts[i],
            &format!("Expecting {expected}, got '{}'", token_name(t_, kanban)),
            false,
        )
    };

    let keyword = format!("'{}'", token_name(t::KEYWORD, kanban));
    if types[i] == t::SPACELINE {
        i += 1;
        while types[i] == t::SPACELINE || types[i] == t::NL {
            i += 1;
        }
        if types[i] != t::KEYWORD {
            return Err(fail(i, &format!("'SPACELINE', 'NL', {keyword}")));
        }
    } else if types[i] != t::KEYWORD {
        return Err(fail(i, &format!("'SPACELINE', {keyword}")));
    }
    i += 1;
    let mut first = "'SPACELINE', 'NL', 'SPACELIST', 'ICON', 'CLASS', 'NODE_DSTART', 'NODE_ID'";
    if types[i] == t::NL {
        i += 1;
        first = "'SPACELINE', 'SPACELIST', 'ICON', 'CLASS', 'NODE_DSTART', 'NODE_ID'";
    }

    loop {
        let mut t_ = types[i];
        let mut level = 0;
        let indented = t_ == t::SPACELIST;
        if indented {
            level = js::len16(&texts[i]);
            i += 1;
            t_ = types[i];
        }
        if t_ == t::NODE_ID || t_ == t::NODE_DSTART {
            let mut id: Option<&str> = None;
            let descr: &str;
            let mut type_ = 0;
            if t_ == t::NODE_ID {
                id = Some(&texts[i]);
                i += 1;
            }
            if types[i] == t::NODE_DSTART {
                let start = &texts[i];
                i += 1;
                if types[i] != t::NODE_DESCR {
                    return Err(fail(i, "'NODE_DESCR'"));
                }
                descr = &texts[i];
                i += 1;
                if types[i] != t::NODE_DEND {
                    return Err(fail(i, "'NODE_DEND'"));
                }
                let end = &texts[i];
                i += 1;
                if !is_stop(types[i]) && types[i] != t::SHAPE_DATA {
                    return Err(fail(i, &format!("{STOP}{data}")));
                }
                type_ = db.get_type(start, end);
            } else {
                if !is_stop(types[i]) && types[i] != t::SHAPE_DATA {
                    return Err(fail(i, &format!("{STOP}, 'NODE_DSTART'{data}")));
                }
                descr = id.unwrap();
            }
            if types[i] == t::SHAPE_DATA {
                let mut shape_data = texts[i].clone();
                i += 1;
                while types[i] == t::SHAPE_DATA {
                    shape_data.push_str(&texts[i]);
                    i += 1;
                }
                if !is_stop(types[i]) {
                    return Err(fail(i, &format!("{STOP}{data}")));
                }
                db.add_node(level, id.unwrap_or(descr), descr, type_, Some(&shape_data))?;
            } else {
                db.add_node(level, id.unwrap_or(descr), descr, type_, None)?;
            }
        } else if t_ == t::ICON || t_ == t::CLASS {
            let text = texts[i].clone();
            i += 1;
            if !is_stop(types[i]) {
                return Err(fail(i, STOP));
            }
            db.decorate_node(if t_ == t::ICON {
                Decoration::Icon(text)
            } else {
                Decoration::Class(text)
            });
        } else if indented {
            if !is_stop(t_) {
                return Err(fail(
                    i,
                    &format!("{STOP}, 'ICON', 'CLASS', 'NODE_DSTART', 'NODE_ID'"),
                ));
            }
        } else if t_ == t::SPACELINE {
            i += 1;
            if !is_stop(types[i]) {
                return Err(fail(i, STOP));
            }
        } else {
            return Err(fail(i, first));
        }

        i += 1;
        while types[i] == t::NL || types[i] == t::EOF {
            i += 1;
        }
        if types[i] == t::END {
            return Ok(());
        }
        if !is_start(types[i]) {
            return Err(fail(
                i,
                &format!("{STOP}, 'SPACELIST', 'ICON', 'CLASS', 'NODE_DSTART', 'NODE_ID'"),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Log(Vec<String>);

    impl OutlineDb for Log {
        fn add_node(
            &mut self,
            level: usize,
            id: &str,
            descr: &str,
            type_: i32,
            shape_data: Option<&str>,
        ) -> Result<()> {
            self.0
                .push(format!("{level} {id}|{descr}|{type_}|{shape_data:?}"));
            Ok(())
        }
        fn decorate_node(&mut self, decoration: Decoration) {
            self.0.push(format!("{decoration:?}"));
        }
    }

    fn parse(src: &str, kanban: bool) -> std::result::Result<Vec<String>, String> {
        let mut log = Log::default();
        match parse_outline(src, &mut log, kanban) {
            Ok(()) => Ok(log.0),
            Err(e) => Err(e.message),
        }
    }

    #[test]
    fn parses_outlines() {
        assert_eq!(
            parse(
                "mindmap\n  root((Root))\n    a[A]\n    ::icon(fa fa-book)\n    b\n    :::urgent\n",
                false
            )
            .unwrap(),
            vec![
                "2 root|Root|3|None",
                "4 a|A|2|None",
                "Icon(\"fa fa-book\")",
                "4 b|b|0|None",
                "Class(\"urgent\")",
            ]
        );
        assert_eq!(
            parse(
                "kanban\n  Todo\n    t1[Task]@{ ticket: \"MC-1\n  x\" }\n",
                true
            )
            .unwrap(),
            vec![
                "2 Todo|Todo|0|None",
                "4 t1|Task|2|Some(\" ticket: \\\"MC-1<br/>x\\\" \")"
            ]
        );
        assert_eq!(
            parse("%% c\nmindmap\n  a\n  %% c\n  b\n", false).unwrap(),
            vec!["2 a|a|0|None", "2 b|b|0|None"]
        );
    }

    #[test]
    fn reports_errors() {
        assert_eq!(
            parse("mindmap\n  a[A\n", false).unwrap_err(),
            "Parse error on line 2:\n  a[A\n-----^\nExpecting 'NODE_DEND', got '1'"
        );
        assert_eq!(
            parse("graph\n", false).unwrap_err(),
            "Parse error on line 1:\ngraph\n^\nExpecting 'SPACELINE', 'MINDMAP', got 'NODE_ID'"
        );
    }
}
