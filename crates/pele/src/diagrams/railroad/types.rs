//! railroad の形（pele の `src/diagrams/railroad/types.ts`）。
//!
//! 入れ子の深い文法でもスタックを使い切らないよう、ノードは `RailroadModel::nodes` に置き、子は番号で指す
//! （TS の `fold` が再帰しないのと同じ理由。Rust では木を捨てるときの再帰も避けられる）。

#[derive(Clone, Debug, PartialEq)]
pub enum RailroadNode {
    Terminal(String),
    Nonterminal(String),
    Special(String),
    Sequence(Vec<usize>),
    Choice(Vec<usize>),
    Optional(usize),
    /// 要素、最小、最大（上限がなければ Infinity）。
    Repetition(usize, f64, f64),
}

impl RailroadNode {
    /// 子の番号（pele の `parts`）。
    pub fn parts(&self) -> &[usize] {
        match self {
            RailroadNode::Sequence(k) | RailroadNode::Choice(k) => k,
            RailroadNode::Optional(k) | RailroadNode::Repetition(k, _, _) => {
                std::slice::from_ref(k)
            }
            _ => &[],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RailroadRule {
    pub name: String,
    /// `RailroadModel::nodes` の番号。
    pub definition: usize,
}

/// Mermaid の 4 つの文法のどれで書いたか。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notation {
    Railroad,
    Ebnf,
    Abnf,
    Peg,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RailroadModel {
    pub notation: Notation,
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    /// 元の順のすべての規則。2 度決めた名前は 2 度ある。
    pub rules: Vec<RailroadRule>,
    pub nodes: Vec<RailroadNode>,
}
