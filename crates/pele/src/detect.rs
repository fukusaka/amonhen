//! 図の種類を見分ける（pele の `src/detect.ts`）。Mermaid が登録する順に試し、最初に合ったものを取る。

use crate::util::chars::{is_space_char, is_word_char};

/// 見分けた種類（pele の `DiagramType`）と、Mermaid の言い方（`MermaidType`、`graph TD` なら `flowchart-v2`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Detected {
    pub type_: &'static str,
    pub mermaid_type: &'static str,
}

/// 頭の空白の後ろがどう続くか。
enum Rule {
    /// `^\s*word`
    Prefix(&'static str),
    /// `^\s*word` で大文字小文字を問わない。
    PrefixI(&'static str),
    /// `^\s*word\b`
    Word(&'static str),
    /// `^\s*word\b` で大文字小文字を問わない。
    WordI(&'static str),
    /// `^\s*(?:a|b|…)`
    Any(&'static [&'static str]),
    /// `^\s*word(?:[\s:]|$)`
    SpaceColon(&'static str),
    /// `^\s*word(?:\s|$)`
    Space(&'static str),
}

const DETECTORS: &[(&str, &str, Rule)] = &[
    ("flowchart", "flowchart-elk", Rule::Prefix("flowchart-elk")),
    ("mindmap", "mindmap", Rule::Prefix("mindmap")),
    ("architecture", "architecture", Rule::Prefix("architecture")),
    ("agentflow", "agentflow", Rule::Word("agentflow-beta")),
    (
        "c4",
        "c4",
        Rule::Any(&[
            "C4Context",
            "C4Container",
            "C4Component",
            "C4Dynamic",
            "C4Deployment",
        ]),
    ),
    ("kanban", "kanban", Rule::Prefix("kanban")),
    ("class", "classDiagram", Rule::Prefix("classDiagram")),
    ("er", "er", Rule::Prefix("erDiagram")),
    ("gantt", "gantt", Rule::Prefix("gantt")),
    ("info", "info", Rule::Prefix("info")),
    ("pie", "pie", Rule::Prefix("pie")),
    ("requirement", "requirement", Rule::Prefix("requirement")),
    ("sequence", "sequence", Rule::Prefix("sequenceDiagram")),
    ("swimlane", "swimlane", Rule::Word("swimlane-beta")),
    (
        "flowchart",
        "flowchart-v2",
        Rule::Any(&["graph", "flowchart"]),
    ),
    ("timeline", "timeline", Rule::Prefix("timeline")),
    ("gitGraph", "gitGraph", Rule::Prefix("gitGraph")),
    ("state", "stateDiagram", Rule::Prefix("stateDiagram")),
    ("journey", "journey", Rule::Prefix("journey")),
    (
        "quadrantChart",
        "quadrantChart",
        Rule::Prefix("quadrantChart"),
    ),
    ("sankey", "sankey", Rule::Prefix("sankey")),
    ("packet", "packet", Rule::Prefix("packet")),
    ("xychart", "xychart", Rule::Prefix("xychart")),
    ("block", "block", Rule::Prefix("block")),
    (
        "eventmodeling",
        "eventmodeling",
        Rule::Prefix("eventmodeling"),
    ),
    ("treeView", "treeView", Rule::Prefix("treeView-beta")),
    ("radar", "radar", Rule::Prefix("radar-beta")),
    ("ishikawa", "ishikawa", Rule::WordI("ishikawa")),
    ("treemap", "treemap", Rule::Prefix("treemap")),
    ("railroad", "railroad", Rule::PrefixI("railroad-beta")),
    (
        "railroad",
        "railroadEbnf",
        Rule::PrefixI("railroad-ebnf-beta"),
    ),
    (
        "railroad",
        "railroadAbnf",
        Rule::PrefixI("railroad-abnf-beta"),
    ),
    (
        "railroad",
        "railroadPeg",
        Rule::PrefixI("railroad-peg-beta"),
    ),
    ("venn", "venn", Rule::Prefix("venn-beta")),
    ("wardley", "wardley", Rule::PrefixI("wardley-beta")),
    ("cynefin", "cynefin", Rule::SpaceColon("cynefin-beta")),
    ("usecase", "usecase", Rule::Space("usecase-beta")),
];

fn starts_i(s: &str, word: &str) -> bool {
    s.len() >= word.len() && s.as_bytes()[..word.len()].eq_ignore_ascii_case(word.as_bytes())
}

/// `\b` の後ろ側（語の文字の後ろに語でない文字か終わり）。
fn boundary_after(rest: &str) -> bool {
    !rest.chars().next().is_some_and(is_word_char)
}

fn matches(rule: &Rule, s: &str) -> bool {
    match rule {
        Rule::Prefix(w) => s.starts_with(w),
        Rule::PrefixI(w) => starts_i(s, w),
        Rule::Word(w) => s.starts_with(w) && boundary_after(&s[w.len()..]),
        Rule::WordI(w) => {
            // `ishikawa(-beta)?\b`: `-beta` の後ろか、`ishikawa` の後ろが境目なら合う。
            if !starts_i(s, w) {
                return false;
            }
            let rest = &s[w.len()..];
            boundary_after(rest) || (starts_i(rest, "-beta") && boundary_after(&rest[5..]))
        }
        Rule::Any(words) => words.iter().any(|w| s.starts_with(w)),
        Rule::SpaceColon(w) => {
            s.starts_with(w)
                && s[w.len()..]
                    .chars()
                    .next()
                    .is_none_or(|c| is_space_char(c) || c == ':')
        }
        Rule::Space(w) => s.starts_with(w) && s[w.len()..].chars().next().is_none_or(is_space_char),
    }
}

pub fn detect_diagram(cleaned: &str) -> Option<Detected> {
    let s = cleaned.trim_start_matches(is_space_char);
    for (type_, mermaid_type, rule) in DETECTORS {
        if matches(rule, s) {
            return Some(Detected {
                type_,
                mermaid_type,
            });
        }
    }
    None
}

pub fn detect(cleaned: &str) -> Option<&'static str> {
    detect_diagram(cleaned).map(|d| d.type_)
}
