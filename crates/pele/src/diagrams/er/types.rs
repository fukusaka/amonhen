//! er の読んだ形（pele の `src/diagrams/er/types.ts`）。

/// Mermaid の多重度の名前（`ZERO_OR_ONE` など）。
pub type Cardinality = &'static str;

#[derive(Clone, Debug)]
pub struct ErAttribute {
    pub type_: String,
    pub name: String,
    pub keys: Vec<String>,
    pub comment: String,
}

#[derive(Clone, Debug)]
pub struct ErEntity {
    pub id: String,
    pub label: String,
    pub alias: String,
    pub attributes: Vec<ErAttribute>,
    pub css_classes: String,
    pub css_styles: Vec<String>,
}

/// cardB は entityA の側の線の端に、cardA は entityB の側に。Mermaid と同じ。
#[derive(Clone, Debug)]
pub struct RelSpec {
    pub card_a: Cardinality,
    pub identifying: bool,
    pub card_b: Cardinality,
}

/// entityA・entityB は実体の id か、そのときその名がまとまり（subgraph）だったならまとまりの id。
#[derive(Clone, Debug)]
pub struct ErRelationship {
    pub entity_a: String,
    pub role_a: String,
    pub entity_b: String,
    pub rel_spec: RelSpec,
}

#[derive(Clone, Debug)]
pub struct ErClassDef {
    pub id: String,
    pub styles: Vec<String>,
    pub text_styles: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct ErSubgraph {
    pub id: String,
    pub nodes: Vec<String>,
    pub title: String,
    pub classes: Vec<String>,
    pub css_styles: Vec<String>,
    pub dir: Option<String>,
}

#[derive(Clone, Debug)]
pub enum DocItem {
    Text(String),
    Dir(String),
}
