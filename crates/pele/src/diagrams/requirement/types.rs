//! requirement の形（pele の `src/diagrams/requirement/types.ts`）。

#[derive(Clone, Debug)]
pub struct Requirement {
    pub name: String,
    pub type_: String,
    pub requirement_id: String,
    pub text: String,
    pub risk: String,
    pub verify_method: String,
    pub css_styles: Vec<String>,
    pub classes: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct RequirementElement {
    pub name: String,
    pub type_: String,
    pub doc_ref: String,
    pub css_styles: Vec<String>,
    pub classes: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Relation {
    pub type_: String,
    pub src: String,
    pub dst: String,
}

#[derive(Clone, Debug)]
pub struct RequirementClass {
    pub id: String,
    pub styles: Vec<String>,
    pub text_styles: Vec<String>,
}
