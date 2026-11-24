//! Graphviz の DOT の図を SVG に描く（Markdown の ` ```dot `・` ```graphviz `・` ```gv ` のブロック）。Graphviz の
//! コードは使わず、公開の文法・属性の説明と論文の手法で自分で書いたもの。配置は層状（ランクはネットワーク単体法、
//! 並びは中央値と入れ替え、横の位置は補助グラフのネットワーク単体法）。
//!
//! 色は pele の図と同じく CSS の変数（`var(--_l)` など）で書く。文が決めない色（既定の線・文字・ノードの枠・地）は
//! テーマに従い（core の `diagram::colorize`）、文が決めた色はそのまま使う。描けないもの（画像・知らない形・知らない
//! 色など）は近いもので描き、`Drawing::notes` に理由を残す。

pub mod arrows;
mod brewer;
pub mod color;
pub mod curve;
pub mod graph;
pub mod html;
pub mod layout;
pub mod parse;
pub mod record;
pub mod render;
pub mod route;
pub mod shapes;
pub mod simplex;
pub mod text;
mod x11;

/// 描けない理由。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub message: String,
}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Error {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

use std::collections::BTreeSet;

/// 近いもので描いたものの理由（図の下に添える）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Note {
    /// 画像は空の枠にした。
    Images,
    /// 知らない形は箱にした。
    UnknownShapes,
    /// `splines=ortho` は折れ線にした。
    Ortho,
    /// 読めない色は既定の色にした。
    UnknownColors,
    /// 持たない配色の色は既定の色にした。
    ColorSchemes,
    /// dot のほかの配置（neato など）は dot の配置にした。
    OtherLayouts,
}

impl Note {
    /// 英語の理由（FE は翻訳して見せる）。
    pub fn reason(self) -> &'static str {
        match self {
            Note::Images => "images are drawn as empty frames",
            Note::UnknownShapes => "unknown shapes are drawn as boxes",
            Note::Ortho => "orthogonal edges are drawn as polylines",
            Note::UnknownColors => "unknown colors are drawn in the default color",
            Note::ColorSchemes => "unknown color schemes are drawn in the default color",
            Note::OtherLayouts => "other layout engines are drawn with the dot layout",
        }
    }
}

/// 描いた図。
#[derive(Clone, Debug, PartialEq)]
pub struct Drawing {
    /// 色は CSS の変数（pele の図と同じ）。
    pub svg: String,
    /// 近いもので描いたものの理由。空なら Graphviz と同じものを描いた。
    pub notes: Vec<Note>,
}

/// 読む文の長さの上限（UTF-16 の数。pele と同じ）。
pub const MAX_SOURCE: usize = 50_000;
/// ノードと辺の上限。
pub const MAX_NODES: usize = 2_000;
pub const MAX_EDGES: usize = 5_000;
/// 出力の上限（UTF-16 の数。pele と同じ）。
pub const MAX_OUTPUT: usize = 4_000_000;

/// DOT の文を描く。
pub fn render(source: &str) -> Result<Drawing, Error> {
    if source.len() > MAX_SOURCE && source.encode_utf16().count() > MAX_SOURCE {
        return Err(Error::new(format!(
            "The graph is longer than the limit of {MAX_SOURCE} characters"
        )));
    }
    let parsed = parse::parse(source)?;
    let model = graph::build(
        &parsed,
        graph::Limits {
            nodes: MAX_NODES,
            edges: MAX_EDGES,
        },
    )?;
    let size = model.nodes.len() + model.edges.len();
    let budget = render::Budget {
        rank_iter: 5_000,
        x_iter: 5_000,
        sweeps: if size > 3000 { 8 } else { 24 },
    };
    let mut notes = BTreeSet::new();
    let svg = render::render(&model, budget, &mut notes);
    if svg.len() > MAX_OUTPUT && svg.encode_utf16().count() > MAX_OUTPUT {
        return Err(Error::new("The drawing is larger than the output limit"));
    }
    Ok(Drawing {
        svg,
        notes: notes.into_iter().collect(),
    })
}
