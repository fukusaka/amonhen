//! 4 つの railroad の文法が分け合うもの（pele の `src/diagrams/railroad/common.ts`）: 題と読み上げの字句
//! （Mermaid の共通の文法のものと違い、頭の空白を取らない）、隠れた字句、最初の規則の前の文、文字列の値、
//! 構文の木から規則の形への歩き方。
//!
//! 構文の木は `Parts` に置く（各文法の節を `tag` で区別する）。木は子を番号で指し、`fold` は再帰しない。

use crate::diagrams::common::tokens::{
    DIRECTIVE, Lexicon, Pattern, Reader, SINGLE_LINE_COMMENT, STRING, TokenType, YAML,
    acc_descr_value, acc_title_value, keyword, title_value, tokenize,
};
use crate::errors::Result;
use crate::util::chars::{NONE, is_space, is_word};

use super::types::RailroadNode;

pub const LETTERS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

pub fn at(src: &[u16], p: usize) -> u32 {
    src.get(p).map_or(NONE, |&c| c as u32)
}

pub fn has(src: &[u16], p: usize, word: &str) -> bool {
    word.bytes()
        .enumerate()
        .all(|(k, w)| at(src, p + k) == w as u32)
}

/// 括弧の形は次の閉じ括弧まで読む。1 度見つからなければ先にもないので、後の括弧の説明は読み直さずに失敗する。
fn acc_descr_scanner() -> crate::diagrams::common::tokens::Scanner {
    let mut unclosed = usize::MAX;
    Box::new(move |src, p, _| {
        if !has(src, p, "accDescr") {
            return None;
        }
        let n = src.len();
        let mut i = p + 8;
        while i < n && matches!(src[i], 32 | 9) {
            i += 1;
        }
        if at(src, i) == 58 {
            i += 1;
            while i < n {
                let c = src[i];
                if c == 10 || c == 13 || (c == 37 && at(src, i + 1) == 37) {
                    break;
                }
                i += 1;
            }
            return Some(i);
        }
        while i < n && is_space(src[i] as u32) {
            i += 1;
        }
        if at(src, i) != 123 || i >= unclosed {
            return None;
        }
        match (i + 1..n).find(|&k| src[k] == 125) {
            Some(close) => Some(close + 1),
            None => {
                unclosed = i;
                None
            }
        }
    })
}

/// `[^\n\r]*?(?=%%)|[^\n\r]*`: 行の中の最初の `%%` の前まで、なければ行の終わりまで。
fn to_comment(src: &[u16], from: usize) -> usize {
    let mut stop = from;
    while stop < src.len() && src[stop] != 10 && src[stop] != 13 {
        stop += 1;
    }
    (from..stop)
        .find(|&k| src[k] == 37 && at(src, k + 1) == 37)
        .unwrap_or(stop)
}

/// 名前を分けないよう、`rest` は名前の続きの 1 文字（`rest` の正規表現が合うか）を答える。
/// `title(?:[\t ][^\n\r]*?(?=%%)|[\t ][^\n\r]*|(?!rest))`
pub fn title(rest: fn(u32) -> bool) -> TokenType {
    TokenType::new(
        "TITLE",
        Pattern::regex(move |src, p| {
            if !has(src, p, "title") {
                return None;
            }
            let q = p + 5;
            if matches!(at(src, q), 9 | 32) {
                Some(to_comment(src, q + 1))
            } else if !rest(at(src, q)) {
                Some(q)
            } else {
                None
            }
        }),
    )
    .first("t")
}

/// `[\w-]`
pub fn word_or_dash(c: u32) -> bool {
    is_word(c) || c == 45
}

pub fn acc_title() -> TokenType {
    TokenType::new(
        "ACC_TITLE",
        Pattern::regex(|src, p| {
            if !has(src, p, "accTitle") {
                return None;
            }
            let mut q = p + 8;
            while matches!(at(src, q), 9 | 32) {
                q += 1;
            }
            (at(src, q) == 58).then(|| to_comment(src, q + 1))
        }),
    )
    .first("a")
}

pub fn acc_descr() -> TokenType {
    TokenType::new("ACC_DESCR", Pattern::scanner(acc_descr_scanner)).first("a")
}

pub fn literal(name: &str, longer: &[&str]) -> TokenType {
    TokenType::literal(name, name).longer(longer)
}

/// 図のキーワード。それで始まり続く名前は名前。
pub fn opening(word: &str, name: &str) -> TokenType {
    keyword(word).longer(&[name])
}

/// `[A-Z_a-z][\w-]*`
pub fn identifier(name: &str) -> TokenType {
    TokenType::new(
        name,
        Pattern::regex(|src, p| {
            let c = at(src, p);
            if !(c == 95 || (65..=90).contains(&c) || (97..=122).contains(&c)) {
                return None;
            }
            let mut e = p + 1;
            while word_or_dash(at(src, e)) {
                e += 1;
            }
            Some(e)
        }),
    )
    .first(&format!("{LETTERS}_"))
}

pub fn quoted(name: &str) -> TokenType {
    STRING.clone().named(name)
}

/// 空白、front matter、ディレクティブ、`%%` のコメント。どの railroad の文法もこの順に並べる。
pub fn hidden_tokens(prefix: &str) -> Vec<TokenType> {
    vec![
        TokenType::new(
            format!("{prefix}_WHITESPACE"),
            Pattern::regex(|src, p| {
                let mut e = p;
                while matches!(at(src, e), 9 | 32 | 13 | 10) {
                    e += 1;
                }
                Some(e)
            }),
        )
        .hidden()
        .first("\t \r\n"),
        YAML.clone().named(format!("{prefix}_YAML")).first("-"),
        DIRECTIVE
            .clone()
            .named(format!("{prefix}_DIRECTIVE"))
            .first("\t %"),
        SINGLE_LINE_COMMENT
            .clone()
            .named(format!("{prefix}_SINGLE_LINE_COMMENT"))
            .first("\t %"),
    ]
}

/// `/\*[\s\S]*?\*/` の形の、`open` から `close` までの隠れたコメント。
pub fn block_comment(name: &str, open: &'static str, close: &'static str) -> TokenType {
    TokenType::new(
        name,
        Pattern::regex(move |src, p| {
            if !has(src, p, open) {
                return None;
            }
            (p + open.len()..src.len())
                .find(|&k| has(src, k, close))
                .map(|k| k + close.len())
        }),
    )
    .hidden()
    .first(&open[..1])
    .opener(move |src, p| has(src, p, open).then_some(p + open.len()))
}

/// Mermaid の railroad の値の変換は 3 つの逃がしを知る。ほかの逃がした文字はその文字。
pub fn unescape(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    if !text.contains('\\') {
        if chars.len() < 2 {
            return String::new();
        }
        return chars[1..chars.len() - 1].iter().collect();
    }
    let end = chars.len() - 1;
    let mut out = String::new();
    let mut i = 1;
    while i < end {
        let mut c = chars[i];
        if c == '\\' && i + 1 < end {
            i += 1;
            c = chars[i];
            out.push(match c {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                c => c,
            });
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

/// 両端の 1 文字ずつを除く（`slice(1, -1)`。両端は ASCII の引用符）。
pub fn strip_ends(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() < 2 {
        return String::new();
    }
    chars[1..chars.len() - 1].iter().collect()
}

/// 構文の木の節。`tag` は文法ごとの節の種類、`text` は値・名前・演算子、`ops` は EBNF の後置の種類。
#[derive(Clone, Debug, Default)]
pub struct Part {
    pub tag: u8,
    pub text: Option<String>,
    pub kids: Vec<usize>,
    pub ops: Vec<u8>,
}

#[derive(Default)]
pub struct Parts(pub Vec<Part>);

impl Parts {
    pub fn add(&mut self, tag: u8, text: Option<String>, kids: Vec<usize>) -> usize {
        self.0.push(Part {
            tag,
            text,
            kids,
            ops: Vec::new(),
        });
        self.0.len() - 1
    }
}

pub struct Grammar {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    /// (名前, 定義の節)
    pub rules: Vec<(String, usize)>,
    pub parts: Parts,
}

/// キーワード、題と読み上げの文、そして文字の終わりまでの規則を読む。どの表もキーワードで始まり、`meta` から
/// TITLE・ACC_TITLE・ACC_DESCR が並ぶ。
pub fn parse_grammar(
    src: &str,
    lexicon: &Lexicon,
    meta: i32,
    unquote: fn(&str) -> String,
    rule: fn(&mut Reader, &mut Parts) -> Result<(String, usize)>,
) -> Result<Grammar> {
    let mut r = Reader::new(tokenize(src, lexicon, "railroad")?, lexicon, "railroad");
    let mut g = Grammar {
        title: None,
        acc_title: None,
        acc_descr: None,
        rules: Vec::new(),
        parts: Parts::default(),
    };
    r.expect(0)?;
    loop {
        let kind = r.kind();
        if kind == meta {
            let title = title_value(&r.take());
            let quote = title.chars().next();
            g.title = Some(match quote {
                Some(q @ ('"' | '\'')) if title.ends_with(q) => unquote(&title),
                _ => title,
            });
        } else if kind == meta + 1 {
            g.acc_title = Some(acc_title_value(&r.take()));
        } else if kind == meta + 2 {
            g.acc_descr = Some(acc_descr_value(&r.take()));
        } else {
            break;
        }
    }
    while r.kind() != -1 {
        let rule = rule(&mut r, &mut g.parts)?;
        g.rules.push(rule);
    }
    Ok(g)
}

/// 形のノードを足す小物。1 つだけの並びと選びはその 1 つ（どの書き方でも）。
pub struct Nodes<'a>(pub &'a mut Vec<RailroadNode>);

impl Nodes<'_> {
    pub fn add(&mut self, node: RailroadNode) -> usize {
        self.0.push(node);
        self.0.len() - 1
    }

    pub fn terminal(&mut self, value: &str) -> usize {
        self.add(RailroadNode::Terminal(value.to_string()))
    }

    pub fn nonterminal(&mut self, name: &str) -> usize {
        self.add(RailroadNode::Nonterminal(name.to_string()))
    }

    pub fn special(&mut self, text: &str) -> usize {
        self.add(RailroadNode::Special(text.to_string()))
    }

    pub fn optional(&mut self, element: usize) -> usize {
        self.add(RailroadNode::Optional(element))
    }

    pub fn repetition(&mut self, element: usize, min: f64, max: f64) -> usize {
        self.add(RailroadNode::Repetition(element, min, max))
    }

    pub fn sequence(&mut self, elements: Vec<usize>) -> usize {
        if elements.len() == 1 {
            elements[0]
        } else {
            self.add(RailroadNode::Sequence(elements))
        }
    }

    pub fn choice(&mut self, alternatives: Vec<usize>) -> usize {
        if alternatives.len() == 1 {
            alternatives[0]
        } else {
            self.add(RailroadNode::Choice(alternatives))
        }
    }
}

/// 木を下から組み直す。再帰しないので、入れ子の深さはスタックでなく記憶で決まる。
pub fn fold<R>(
    root: usize,
    children: impl Fn(usize) -> Vec<usize>,
    mut build: impl FnMut(usize, Vec<R>) -> R,
) -> R {
    let mut nodes: Vec<usize> = vec![root];
    let mut lists: Vec<Vec<usize>> = vec![children(root)];
    let mut done: Vec<Vec<R>> = vec![Vec::new()];
    loop {
        let top = nodes.len() - 1;
        if done[top].len() < lists[top].len() {
            let child = lists[top][done[top].len()];
            nodes.push(child);
            lists.push(children(child));
            done.push(Vec::new());
        } else {
            let results = done.pop().unwrap();
            let node = nodes.pop().unwrap();
            lists.pop();
            let value = build(node, results);
            if top == 0 {
                return value;
            }
            done[top - 1].push(value);
        }
    }
}

/// 1 つの節をその子から組む関数。
pub type Build = fn(&Part, Vec<usize>, &mut Nodes) -> usize;

/// 各規則の構文の木を共通の形に。1 つの節をその子からどう組むかを受ける。
pub fn to_rules(g: &Grammar, nodes: &mut Vec<RailroadNode>, build: Build) -> Vec<(String, usize)> {
    let parts = &g.parts.0;
    g.rules
        .iter()
        .map(|(name, root)| {
            let mut n = Nodes(nodes);
            let definition = fold(
                *root,
                |k| parts[k].kids.clone(),
                |k, kids| build(&parts[k], kids, &mut n),
            );
            (name.clone(), definition)
        })
        .collect()
}
