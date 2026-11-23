//! Graphviz の DOT の図を SVG に描く（Markdown の ` ```dot `・` ```graphviz `・` ```gv ` のブロック）。Graphviz の
//! コードは使わず、公開の文法・属性の説明と論文の手法で自分で書いたもの。配置は層状（ランクはネットワーク単体法、
//! 並びは中央値と入れ替え、横の位置は補助グラフのネットワーク単体法）。
//!
//! 色は pele の図と同じく CSS の変数（`var(--_l)` など）で書く。文が決めない色（既定の線・文字・ノードの枠・地）は
//! テーマに従い（core の `diagram::colorize`）、文が決めた色はそのまま使う。描けないもの（画像・HTML の表・ortho の
//! 線など）は近いもので描き、`Drawing::notes` に理由を残す。

pub mod graph;
pub mod parse;

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
