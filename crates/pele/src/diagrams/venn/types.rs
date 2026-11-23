//! venn の形（pele の `src/diagrams/venn/types.ts`）。

/// 集合（id が 1 つ）か集合の重なり（id が 2 つ以上、並べたもの）と、その相対の広さ。
#[derive(Clone, Debug, PartialEq)]
pub struct VennSubset {
    pub sets: Vec<String>,
    pub size: f64,
    pub label: Option<String>,
}

/// その集合の領域の中に書く 1 行の文字。
#[derive(Clone, Debug, PartialEq)]
pub struct VennText {
    pub sets: Vec<String>,
    pub id: String,
    pub label: Option<String>,
}

/// 対象は集合か重なりの並べた id、または文字の id。宣言は入れた順（JS の `Map`）。
#[derive(Clone, Debug, PartialEq)]
pub struct VennStyle {
    pub targets: Vec<String>,
    pub styles: Vec<(String, String)>,
}
