//! state の読んだ形（pele の `src/diagrams/state/types.ts`）。TS の文の木は互いを指すので、状態の文は
//! `StateDb::items` に置き、文書はその添字で指す。

#[derive(Clone, Debug)]
pub struct StateNote {
    pub position: Option<String>,
    pub text: String,
}

/// 状態の説明（`string | string[]`）。
#[derive(Clone, Debug)]
pub enum Desc {
    One(String),
    Many(Vec<String>),
}

impl Desc {
    /// JS の真偽（空の文字列だけが偽。空の配列は真）。
    pub fn truthy(&self) -> bool {
        match self {
            Desc::One(s) => !s.is_empty(),
            Desc::Many(_) => true,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct StateStmt {
    pub id: String,
    pub type_: Option<String>,
    pub description: Option<Desc>,
    pub doc: Option<Vec<Stmt>>,
    pub note: Option<StateNote>,
    pub start: Option<bool>,
    pub classes: Option<Vec<String>>,
}

/// Mermaid の文法は、文書の中で効かない文（`hide empty description`、`scale`、`state name` だけ、浮いたノート）の
/// 文字も残す。それが `Text`。
#[derive(Clone, Debug)]
pub enum Stmt {
    Text(String),
    /// `StateDb::items` の添字。
    State(usize),
    Relation {
        state1: usize,
        state2: usize,
        description: Option<String>,
    },
    ClassDef {
        id: String,
        classes: String,
    },
    Style {
        id: String,
        style_class: String,
    },
    ApplyClass {
        id: String,
        style_class: String,
    },
    Dir(String),
    Click {
        id: usize,
        url: String,
        tooltip: String,
    },
}

#[derive(Clone, Debug)]
pub struct StateClassDef {
    pub id: String,
    pub styles: Vec<String>,
    pub text_styles: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct StateLink {
    pub url: String,
    pub tooltip: String,
}
