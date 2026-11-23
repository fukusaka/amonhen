//! class の読んだ形（pele の `src/diagrams/class/types.ts`）。

use super::members::ClassMember;

/// 関係の端の印（Mermaid の relationType の番号）。None は `'none'`。
pub type RelationEnd = Option<u8>;

#[derive(Clone, Debug)]
pub struct ClassNode {
    pub id: String,
    pub type_: String,
    pub label: String,
    pub css_classes: String,
    pub methods: Vec<ClassMember>,
    pub members: Vec<ClassMember>,
    pub annotations: Vec<String>,
    pub styles: Vec<String>,
    pub parent: Option<String>,
    pub link: Option<String>,
    pub link_target: Option<String>,
    pub have_callback: bool,
    pub tooltip: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Relation {
    pub type1: RelationEnd,
    pub type2: RelationEnd,
    pub line_type: u8,
}

#[derive(Clone, Debug)]
pub struct ClassRelation {
    pub id1: String,
    pub id2: String,
    pub relation_title1: String,
    pub relation_title2: String,
    pub title: Option<String>,
    pub relation: Relation,
}

#[derive(Clone, Debug)]
pub struct ClassNote {
    pub id: String,
    pub class: Option<String>,
    pub text: String,
    pub index: usize,
    pub parent: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ClassInterface {
    pub id: String,
    pub label: String,
    pub class_id: String,
}

#[derive(Clone, Debug)]
pub struct NamespaceNode {
    pub id: String,
    pub label: String,
    pub parent: Option<String>,
    /// `A.B.C` のような点のある名前の途中に作った祖先では偽。
    pub explicit: bool,
}

#[derive(Clone, Debug)]
pub struct StyleClass {
    pub id: String,
    pub styles: Vec<String>,
    pub text_styles: Vec<String>,
}
