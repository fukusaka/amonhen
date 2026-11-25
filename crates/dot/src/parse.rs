//! DOT の文を読む（字句と構文）。Graphviz の公開の文法の説明（The DOT Language）に従って自分で書いたもの。
//!
//! - 字句: 名前（英字・数字・`_`・ASCII の外の文字）、数（`-.5`・`1.2`）、二重引用符の文字（`\"` だけを戻し、行末の
//!   `\` は続きの行。ほかの `\` はラベルの処理に残す。`+` でつなぐ）、HTML の文字（`<…>` の入れ子）、記号。
//! - 注釈: `//`・`/* */`、`#` から行の終わり（C の前処理の出力）。
//! - 入れ子の深さには上限を置く（スタックがあふれないように）。

use crate::{Error, Syntax};

/// 値。`html` は `<…>` で書いたもの。
#[derive(Clone, Debug, PartialEq)]
pub struct Value {
    pub text: String,
    pub html: bool,
}

impl Value {
    pub fn plain(text: impl Into<String>) -> Self {
        Value {
            text: text.into(),
            html: false,
        }
    }
}

pub type AttrList = Vec<(String, Value)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttrKind {
    Graph,
    Node,
    Edge,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NodeRef {
    pub id: String,
    /// `a:port:compass` の `port:compass`（なければ None）。
    pub port: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EdgeEnd {
    Node(NodeRef),
    Subgraph(Subgraph),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Subgraph {
    pub id: Option<String>,
    pub stmts: Vec<Stmt>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    Attr(AttrKind, AttrList),
    Assign(String, Value),
    Node(NodeRef, AttrList),
    Edge(Vec<EdgeEnd>, AttrList),
    Subgraph(Subgraph),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Graph {
    pub strict: bool,
    pub directed: bool,
    pub id: Option<String>,
    pub stmts: Vec<Stmt>,
}

/// 入れ子（subgraph・`{}`）の深さの上限。
pub const MAX_DEPTH: usize = 200;

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    /// 名前・数・引用符の文字。`quoted` は引用符つき（キーワードにならない）。
    Id {
        text: String,
        quoted: bool,
    },
    Html(String),
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Semi,
    Comma,
    Eq,
    Colon,
    Arrow,
    Dash,
    Plus,
}

struct Lexer<'a> {
    s: &'a [u8],
    src: &'a str,
    i: usize,
    line: usize,
}

fn is_id_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c >= 0x80
}

fn is_id_char(c: u8) -> bool {
    is_id_start(c) || c.is_ascii_digit()
}

impl<'a> Lexer<'a> {
    fn new(src: &'a str) -> Self {
        Lexer {
            s: src.as_bytes(),
            src,
            i: 0,
            line: 1,
        }
    }

    fn err(&self, kind: Syntax) -> Error {
        Error::Syntax {
            kind,
            line: self.line,
        }
    }

    /// 空白と注釈を飛ばす。
    fn skip(&mut self) -> Result<(), Error> {
        while self.i < self.s.len() {
            let c = self.s[self.i];
            match c {
                b'\n' => {
                    self.line += 1;
                    self.i += 1;
                }
                b' ' | b'\t' | b'\r' | 0x0b | 0x0c => self.i += 1,
                // `#` から行の終わりまで（C の前処理の出力の行。Graphviz と同じく行の途中でも）
                b'#' => {
                    while self.i < self.s.len() && self.s[self.i] != b'\n' {
                        self.i += 1;
                    }
                }
                b'/' if self.s.get(self.i + 1) == Some(&b'/') => {
                    while self.i < self.s.len() && self.s[self.i] != b'\n' {
                        self.i += 1;
                    }
                }
                b'/' if self.s.get(self.i + 1) == Some(&b'*') => {
                    self.i += 2;
                    loop {
                        if self.i + 1 >= self.s.len() {
                            return Err(self.err(Syntax::UnterminatedComment));
                        }
                        if self.s[self.i] == b'*' && self.s[self.i + 1] == b'/' {
                            self.i += 2;
                            break;
                        }
                        if self.s[self.i] == b'\n' {
                            self.line += 1;
                        }
                        self.i += 1;
                    }
                }
                _ => break,
            }
        }
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Tok>, Error> {
        self.skip()?;
        let Some(&c) = self.s.get(self.i) else {
            return Ok(None);
        };
        let one = |t: Tok, me: &mut Self| {
            me.i += 1;
            Ok(Some(t))
        };
        match c {
            b'{' => one(Tok::LBrace, self),
            b'}' => one(Tok::RBrace, self),
            b'[' => one(Tok::LBracket, self),
            b']' => one(Tok::RBracket, self),
            b';' => one(Tok::Semi, self),
            b',' => one(Tok::Comma, self),
            b'=' => one(Tok::Eq, self),
            b':' => one(Tok::Colon, self),
            b'+' => one(Tok::Plus, self),
            b'-' if self.s.get(self.i + 1) == Some(&b'>') => {
                self.i += 2;
                Ok(Some(Tok::Arrow))
            }
            b'-' if self.s.get(self.i + 1) == Some(&b'-') => {
                self.i += 2;
                Ok(Some(Tok::Dash))
            }
            b'"' => self.quoted().map(Some),
            b'<' => self.html().map(Some),
            b'-' | b'.' | b'0'..=b'9' => self.number().map(Some),
            c if is_id_start(c) => {
                let start = self.i;
                while self.i < self.s.len() && is_id_char(self.s[self.i]) {
                    self.i += 1;
                }
                Ok(Some(Tok::Id {
                    text: self.src[start..self.i].to_owned(),
                    quoted: false,
                }))
            }
            _ => {
                let ch = self.src[self.i..].chars().next().unwrap_or('?');
                Err(self.err(Syntax::UnexpectedCharacter(ch)))
            }
        }
    }

    fn number(&mut self) -> Result<Tok, Error> {
        let start = self.i;
        if self.s[self.i] == b'-' {
            self.i += 1;
        }
        let mut digits = 0;
        let mut dot = false;
        while self.i < self.s.len() {
            match self.s[self.i] {
                b'0'..=b'9' => digits += 1,
                b'.' if !dot => dot = true,
                _ => break,
            }
            self.i += 1;
        }
        if digits == 0 {
            return Err(self.err(Syntax::MalformedNumber));
        }
        // 数のすぐあとの名前の文字は、Graphviz と同じく別の名前として読む（警告だけ）。
        Ok(Tok::Id {
            text: self.src[start..self.i].to_owned(),
            quoted: false,
        })
    }

    fn quoted(&mut self) -> Result<Tok, Error> {
        self.i += 1;
        let mut out = String::new();
        let mut run = self.i;
        loop {
            let Some(&c) = self.s.get(self.i) else {
                return Err(self.err(Syntax::UnterminatedString));
            };
            match c {
                b'"' => {
                    out.push_str(&self.src[run..self.i]);
                    self.i += 1;
                    break;
                }
                b'\\' => {
                    out.push_str(&self.src[run..self.i]);
                    match self.s.get(self.i + 1) {
                        Some(b'"') => {
                            out.push('"');
                            self.i += 2;
                        }
                        Some(b'\n') => {
                            self.line += 1;
                            self.i += 2;
                        }
                        Some(b'\r') if self.s.get(self.i + 2) == Some(&b'\n') => {
                            self.line += 1;
                            self.i += 3;
                        }
                        Some(_) => {
                            // `\\` は 2 文字のまま残す（ラベルの処理で `\` になる）。次の `"` を閉じと読まないよう、
                            // 2 文字をまとめて写す。
                            let next_len = self.src[self.i + 1..]
                                .chars()
                                .next()
                                .map_or(1, char::len_utf8);
                            out.push_str(&self.src[self.i..self.i + 1 + next_len]);
                            self.i += 1 + next_len;
                        }
                        None => return Err(self.err(Syntax::UnterminatedString)),
                    }
                    run = self.i;
                }
                b'\n' => {
                    self.line += 1;
                    self.i += 1;
                }
                _ => self.i += 1,
            }
        }
        Ok(Tok::Id {
            text: out,
            quoted: true,
        })
    }

    fn html(&mut self) -> Result<Tok, Error> {
        let start = self.i + 1;
        let mut depth = 0usize;
        while let Some(&c) = self.s.get(self.i) {
            match c {
                b'<' => depth += 1,
                b'>' => {
                    depth -= 1;
                    if depth == 0 {
                        let text = self.src[start..self.i].to_owned();
                        self.i += 1;
                        return Ok(Tok::Html(text));
                    }
                }
                b'\n' => self.line += 1,
                _ => {}
            }
            self.i += 1;
        }
        Err(self.err(Syntax::UnterminatedHtml))
    }
}

struct Parser<'a> {
    lex: Lexer<'a>,
    peeked: Option<Option<Tok>>,
}

fn keyword(t: &Tok, word: &str) -> bool {
    matches!(t, Tok::Id { text, quoted: false } if text.eq_ignore_ascii_case(word))
}

impl<'a> Parser<'a> {
    fn peek(&mut self) -> Result<Option<&Tok>, Error> {
        if self.peeked.is_none() {
            self.peeked = Some(self.lex.next()?);
        }
        Ok(self.peeked.as_ref().and_then(|t| t.as_ref()))
    }

    fn bump(&mut self) -> Result<Option<Tok>, Error> {
        match self.peeked.take() {
            Some(t) => Ok(t),
            None => self.lex.next(),
        }
    }

    fn err(&self, kind: Syntax) -> Error {
        self.lex.err(kind)
    }

    fn expect(&mut self, want: Tok, what: &'static str) -> Result<(), Error> {
        match self.bump()? {
            Some(t) if t == want => Ok(()),
            Some(_) => Err(self.err(Syntax::Expected(what))),
            None => Err(self.err(Syntax::ExpectedBeforeEnd(what))),
        }
    }

    /// ID（名前・数・引用符の文字を `+` でつないだもの・HTML の文字）。
    fn id(&mut self) -> Result<Value, Error> {
        match self.bump()? {
            Some(Tok::Id { text, quoted }) => {
                let mut text = text;
                if quoted {
                    while matches!(self.peek()?, Some(Tok::Plus)) {
                        self.bump()?;
                        match self.bump()? {
                            Some(Tok::Id { text: more, .. }) => text.push_str(&more),
                            _ => return Err(self.err(Syntax::ExpectedStringAfterPlus)),
                        }
                    }
                }
                Ok(Value { text, html: false })
            }
            Some(Tok::Html(text)) => Ok(Value { text, html: true }),
            _ => Err(self.err(Syntax::ExpectedName)),
        }
    }

    fn graph(&mut self) -> Result<Graph, Error> {
        let mut strict = false;
        let mut t = self.bump()?;
        if t.as_ref().is_some_and(|t| keyword(t, "strict")) {
            strict = true;
            t = self.bump()?;
        }
        let directed = match &t {
            Some(t) if keyword(t, "digraph") => true,
            Some(t) if keyword(t, "graph") => false,
            _ => return Err(self.err(Syntax::ExpectedGraph)),
        };
        let id = match self.peek()? {
            Some(Tok::LBrace) => None,
            Some(_) => Some(self.id()?.text),
            None => return Err(self.err(Syntax::Expected("'{'"))),
        };
        self.expect(Tok::LBrace, "'{'")?;
        let stmts = self.stmts(directed, 1)?;
        Ok(Graph {
            strict,
            directed,
            id,
            stmts,
        })
    }

    /// `}` までの文。
    fn stmts(&mut self, directed: bool, depth: usize) -> Result<Vec<Stmt>, Error> {
        if depth > MAX_DEPTH {
            return Err(self.err(Syntax::NestedTooDeeply));
        }
        let mut out = Vec::new();
        loop {
            match self.peek()? {
                None => return Err(self.err(Syntax::ExpectedBeforeEnd("'}'"))),
                Some(Tok::RBrace) => {
                    self.bump()?;
                    return Ok(out);
                }
                Some(Tok::Semi) | Some(Tok::Comma) => {
                    self.bump()?;
                }
                Some(_) => out.push(self.stmt(directed, depth)?),
            }
        }
    }

    fn attr_list(&mut self) -> Result<AttrList, Error> {
        let mut out = Vec::new();
        while matches!(self.peek()?, Some(Tok::LBracket)) {
            self.bump()?;
            loop {
                match self.peek()? {
                    Some(Tok::RBracket) => {
                        self.bump()?;
                        break;
                    }
                    Some(Tok::Comma) | Some(Tok::Semi) => {
                        self.bump()?;
                    }
                    None => return Err(self.err(Syntax::ExpectedBeforeEnd("']'"))),
                    Some(_) => {
                        let key = self.id()?.text;
                        if matches!(self.peek()?, Some(Tok::Eq)) {
                            self.bump()?;
                            let value = self.id()?;
                            out.push((key, value));
                        } else {
                            // 値のない属性は true（Graphviz と同じ）。
                            out.push((key, Value::plain("true")));
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    fn subgraph(&mut self, directed: bool, depth: usize) -> Result<Subgraph, Error> {
        let mut id = None;
        if self.peek()?.is_some_and(|t| keyword(t, "subgraph")) {
            self.bump()?;
            if !matches!(self.peek()?, Some(Tok::LBrace)) {
                id = Some(self.id()?.text);
            }
        }
        if !matches!(self.peek()?, Some(Tok::LBrace)) {
            // `subgraph name` だけ（前に作ったものを指す）
            return Ok(Subgraph { id, stmts: vec![] });
        }
        self.bump()?;
        let stmts = self.stmts(directed, depth + 1)?;
        Ok(Subgraph { id, stmts })
    }

    fn node_ref(&mut self) -> Result<NodeRef, Error> {
        let id = self.id()?.text;
        let mut port: Option<String> = None;
        while matches!(self.peek()?, Some(Tok::Colon)) {
            self.bump()?;
            let part = self.id()?.text;
            port = Some(match port {
                None => part,
                Some(p) => format!("{p}:{part}"),
            });
        }
        Ok(NodeRef { id, port })
    }

    fn edge_end(&mut self, directed: bool, depth: usize) -> Result<EdgeEnd, Error> {
        match self.peek()? {
            Some(Tok::LBrace) => Ok(EdgeEnd::Subgraph(self.subgraph(directed, depth)?)),
            Some(t) if keyword(t, "subgraph") => {
                Ok(EdgeEnd::Subgraph(self.subgraph(directed, depth)?))
            }
            _ => Ok(EdgeEnd::Node(self.node_ref()?)),
        }
    }

    fn edge_rest(&mut self, first: EdgeEnd, directed: bool, depth: usize) -> Result<Stmt, Error> {
        let mut ends = vec![first];
        // 有向・無向のどちらの記号も受ける（Graphviz は合わないと誤りにするが、描けるものは描く）。
        while let Some(Tok::Arrow) | Some(Tok::Dash) = self.peek()? {
            self.bump()?;
            ends.push(self.edge_end(directed, depth)?);
        }
        let attrs = self.attr_list()?;
        Ok(Stmt::Edge(ends, attrs))
    }

    fn stmt(&mut self, directed: bool, depth: usize) -> Result<Stmt, Error> {
        let t = self.peek()?.cloned();
        match t {
            Some(Tok::LBrace) => {
                let sub = self.subgraph(directed, depth)?;
                if matches!(self.peek()?, Some(Tok::Arrow) | Some(Tok::Dash)) {
                    return self.edge_rest(EdgeEnd::Subgraph(sub), directed, depth);
                }
                Ok(Stmt::Subgraph(sub))
            }
            Some(ref t) if keyword(t, "subgraph") => {
                let sub = self.subgraph(directed, depth)?;
                if matches!(self.peek()?, Some(Tok::Arrow) | Some(Tok::Dash)) {
                    return self.edge_rest(EdgeEnd::Subgraph(sub), directed, depth);
                }
                Ok(Stmt::Subgraph(sub))
            }
            Some(ref t) if keyword(t, "graph") || keyword(t, "node") || keyword(t, "edge") => {
                let kind = if keyword(t, "graph") {
                    AttrKind::Graph
                } else if keyword(t, "node") {
                    AttrKind::Node
                } else {
                    AttrKind::Edge
                };
                self.bump()?;
                if !matches!(self.peek()?, Some(Tok::LBracket)) {
                    return Err(self.err(Syntax::Expected("'['")));
                }
                Ok(Stmt::Attr(kind, self.attr_list()?))
            }
            Some(Tok::Id { .. }) | Some(Tok::Html(_)) => {
                let node = self.node_ref()?;
                if node.port.is_none() && matches!(self.peek()?, Some(Tok::Eq)) {
                    self.bump()?;
                    let value = self.id()?;
                    return Ok(Stmt::Assign(node.id, value));
                }
                if matches!(self.peek()?, Some(Tok::Arrow) | Some(Tok::Dash)) {
                    return self.edge_rest(EdgeEnd::Node(node), directed, depth);
                }
                let attrs = self.attr_list()?;
                Ok(Stmt::Node(node, attrs))
            }
            _ => Err(self.err(Syntax::UnexpectedSymbol)),
        }
    }
}

/// 最初のグラフを読む。後ろに続くグラフは読まない。
pub fn parse(src: &str) -> Result<Graph, Error> {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let mut p = Parser {
        lex: Lexer::new(src),
        peeked: None,
    };
    p.graph()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(src: &str) -> Graph {
        parse(src).unwrap_or_else(|e| panic!("{src}: {e}"))
    }

    #[test]
    fn reads_headers() {
        let g = ok("strict digraph G { }");
        assert!(g.strict && g.directed && g.id.as_deref() == Some("G"));
        let g = ok("GRAPH { }");
        assert!(!g.strict && !g.directed && g.id.is_none());
        assert!(parse("digraph").is_err());
        assert!(parse("node { }").is_err());
    }

    #[test]
    fn reads_statements() {
        let g = ok(r#"digraph {
            // comment
            /* block */
            # preprocessor line
            graph [rankdir=LR]; node [shape=box] edge [color=red]
            a; b [label="B" color=blue, style=filled;];
            a -> b -> c [label=x];
            {a b} -> c;
            subgraph cluster_1 { d; }
            size="7,7";
            a:p1:n -> b:s;
            -1.5 -> .5;
        }"#);
        assert_eq!(g.stmts.len(), 11);
        assert!(matches!(&g.stmts[0], Stmt::Attr(AttrKind::Graph, a) if a[0].0 == "rankdir"));
        match &g.stmts[4] {
            Stmt::Node(n, attrs) => {
                assert_eq!(n.id, "b");
                assert_eq!(attrs.len(), 3);
            }
            other => panic!("{other:?}"),
        }
        match &g.stmts[5] {
            Stmt::Edge(ends, attrs) => {
                assert_eq!(ends.len(), 3);
                assert_eq!(attrs[0].1.text, "x");
            }
            other => panic!("{other:?}"),
        }
        assert!(
            matches!(&g.stmts[6], Stmt::Edge(ends, _) if matches!(ends[0], EdgeEnd::Subgraph(_)))
        );
        assert!(matches!(&g.stmts[7], Stmt::Subgraph(s) if s.id.as_deref() == Some("cluster_1")));
        assert!(matches!(&g.stmts[8], Stmt::Assign(k, v) if k == "size" && v.text == "7,7"));
        match &g.stmts[9] {
            Stmt::Edge(ends, _) => match (&ends[0], &ends[1]) {
                (EdgeEnd::Node(a), EdgeEnd::Node(b)) => {
                    assert_eq!(a.port.as_deref(), Some("p1:n"));
                    assert_eq!(b.port.as_deref(), Some("s"));
                }
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        }
        assert!(
            matches!(&g.stmts[10], Stmt::Edge(ends, _) if matches!(&ends[0], EdgeEnd::Node(n) if n.id == "-1.5"))
        );
    }

    #[test]
    fn reads_strings() {
        let g = ok(
            "digraph { a [label=\"x\\\"y\" + \"z\\n\\l\" xlabel=<<b>B</b>>]; \"multi\\\nline\" }",
        );
        match &g.stmts[0] {
            Stmt::Node(_, attrs) => {
                assert_eq!(attrs[0].1.text, "x\"yz\\n\\l");
                assert_eq!(
                    attrs[1].1,
                    Value {
                        text: "<b>B</b>".into(),
                        html: true
                    }
                );
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(&g.stmts[1], Stmt::Node(n, _) if n.id == "multiline"));
        // 引用符の中の `\\` のあとの `"` は閉じ
        let g = ok(r#"digraph { a [label="x\\"] }"#);
        assert!(matches!(&g.stmts[0], Stmt::Node(_, a) if a[0].1.text == "x\\\\"));
    }

    #[test]
    fn refuses_broken_sources() {
        for src in [
            "digraph { a -> }",
            "digraph { a [label=] }",
            "digraph { \"open }",
            "digraph { a [x=<b> }",
            "digraph { /* x }",
            "digraph { a ",
            "digraph { @ }",
        ] {
            assert!(parse(src).is_err(), "{src}");
        }
    }

    #[test]
    fn limits_nesting() {
        let deep = format!("digraph {{ {} }}", "{".repeat(MAX_DEPTH + 5));
        let e = parse(&deep).unwrap_err();
        assert!(
            matches!(
                e,
                Error::Syntax {
                    kind: Syntax::NestedTooDeeply,
                    ..
                }
            ),
            "{e}"
        );
        let fine = format!("digraph {{ {}a{} }}", "{".repeat(50), "}".repeat(50));
        ok(&fine);
    }
}
