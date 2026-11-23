//! block の形（pele の `src/diagrams/block/types.ts`）。

/// ブロック、辺、列・クラス・スタイルを決める文に共通の 1 つの形（Mermaid と同じ）。TS では同じものを何か所からも
/// 指すので、ここでは `BlockDb::arena` に置いて位置で指す。
#[derive(Clone, Debug, Default)]
pub struct Block {
    pub id: Option<String>,
    pub type_: Option<String>,
    pub label: Option<String>,
    /// 組み上げた後の子（`arena` の位置）。
    pub children: Vec<usize>,
    /// 読んだばかりの子の文（`setHierarchy` が `children` に移す）。
    pub inner: Option<Vec<Statement>>,
    pub columns: Option<f64>,
    pub width_in_columns: Option<f64>,
    /// `space` の文が求めたマスの数。
    pub width: Option<f64>,
    pub directions: Option<Vec<String>>,
    pub classes: Option<Vec<String>>,
    pub styles: Option<Vec<String>>,
    pub color_index: Option<usize>,
    pub start: Option<String>,
    pub end: Option<String>,
    pub arrow_type_start: Option<String>,
    pub arrow_type_end: Option<String>,
    pub thickness: Option<String>,
    pub pattern: Option<String>,
    pub css: Option<String>,
    pub style_class: Option<String>,
    pub styles_str: Option<String>,
}

impl Block {
    pub fn is(&self, type_: &str) -> bool {
        self.type_.as_deref() == Some(type_)
    }
}

/// リンクのある文は、ブロック、辺、ブロック…の並び。
#[derive(Clone, Debug)]
pub enum Statement {
    One(Box<Block>),
    Many(Vec<Block>),
}

#[derive(Clone, Debug, Default)]
pub struct BlockClassDef {
    pub id: String,
    pub styles: Vec<String>,
    pub text_styles: Vec<String>,
}
