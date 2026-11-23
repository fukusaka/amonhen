//! use case の形（pele の `src/diagrams/usecase/types.ts`）。元の位置を知らせる文の並び（`GraphStatement`）は
//! 描くのに使わないので移していない。

use std::collections::HashMap;

use super::json::Json;

/// 入力の始まりと終わり（UTF-16 の位置、終わりは含まない）。
pub type Span = (usize, usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelType {
    Text,
    Markdown,
}

/// 矢印の種類（`Arrow`）。
pub mod arrow {
    pub const SOLID_ARROW: u8 = 0;
    pub const BACK_ARROW: u8 = 1;
    pub const LINE_SOLID: u8 = 2;
    pub const CIRCLE_ARROW: u8 = 3;
    pub const CROSS_ARROW: u8 = 4;
    pub const CIRCLE_ARROW_REVERSED: u8 = 5;
    pub const CROSS_ARROW_REVERSED: u8 = 6;
}

/// JS の `Map<string, V>`: 入れた順を保ち、ある鍵に入れ直しても場所は変わらない。
#[derive(Clone, Debug)]
pub struct OMap<V> {
    slots: Vec<Option<(String, V)>>,
    index: HashMap<String, usize>,
}

impl<V> Default for OMap<V> {
    fn default() -> Self {
        OMap {
            slots: Vec::new(),
            index: HashMap::new(),
        }
    }
}

impl<V> OMap<V> {
    pub fn get(&self, key: &str) -> Option<&V> {
        self.index
            .get(key)
            .and_then(|&i| self.slots[i].as_ref().map(|(_, v)| v))
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut V> {
        match self.index.get(key) {
            Some(&i) => self.slots[i].as_mut().map(|(_, v)| v),
            None => None,
        }
    }

    pub fn has(&self, key: &str) -> bool {
        self.index.contains_key(key)
    }

    pub fn set(&mut self, key: &str, value: V) {
        match self.index.get(key) {
            Some(&i) => self.slots[i] = Some((key.to_string(), value)),
            None => {
                self.index.insert(key.to_string(), self.slots.len());
                self.slots.push(Some((key.to_string(), value)));
            }
        }
    }

    pub fn delete(&mut self, key: &str) {
        if let Some(i) = self.index.remove(key) {
            self.slots[i] = None;
        }
    }

    pub fn clear(&mut self) {
        self.slots.clear();
        self.index.clear();
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.slots.iter().filter_map(|s| s.as_ref().map(|(_, v)| v))
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.slots
            .iter_mut()
            .filter_map(|s| s.as_mut().map(|(_, v)| v))
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }
}

#[derive(Clone, Debug)]
pub struct Actor {
    pub id: String,
    pub label: String,
    pub label_type: LabelType,
    /// normal、hollow、awesome、icon。
    pub type_: &'static str,
    pub icon: Option<String>,
    pub business: bool,
    pub stereotype: Option<String>,
    pub parent_id: Option<String>,
    pub classes: Vec<String>,
    pub styles: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct UseCase {
    pub id: String,
    pub label: String,
    pub label_type: LabelType,
    /// ellipse か rect。
    pub shape: &'static str,
    pub business: bool,
    pub stereotype: Option<String>,
    pub parent_id: Option<String>,
    pub classes: Vec<String>,
    pub styles: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct SystemBoundary {
    pub id: String,
    pub label: String,
    pub label_type: LabelType,
    /// rect か package。
    pub type_: &'static str,
    pub members: Vec<String>,
    pub classes: Vec<String>,
    pub styles: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Relationship {
    pub id: String,
    pub explicit_id: bool,
    pub source: String,
    pub target: String,
    /// association、include、extend、generalization。
    pub type_: &'static str,
    pub arrow_type: u8,
    pub label: Option<String>,
    pub label_type: Option<LabelType>,
    pub minlen: f64,
    pub classes: Vec<String>,
    pub styles: Vec<String>,
    pub animate: bool,
    /// fast か slow。
    pub animation: Option<&'static str>,
}

#[derive(Clone, Debug)]
pub struct UsecaseNote {
    pub id: String,
    pub target: String,
    pub label: String,
    pub label_type: LabelType,
}

#[derive(Clone, Debug)]
pub struct UsecaseJsonNode {
    pub id: String,
    pub value: Json,
    /// 各オブジェクトの鍵を書いた順に、JSON Pointer ごとに。
    pub property_order: HashMap<String, Vec<String>>,
    pub classes: Vec<String>,
    pub styles: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct ClassDef {
    pub id: String,
    pub styles: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct UsecaseModel {
    pub actors: OMap<Actor>,
    pub use_cases: OMap<UseCase>,
    pub system_boundaries: OMap<SystemBoundary>,
    pub relationships: Vec<Relationship>,
    pub notes: OMap<UsecaseNote>,
    pub json_nodes: OMap<UsecaseJsonNode>,
    pub class_defs: OMap<ClassDef>,
    pub direction: String,
    pub title: Option<String>,
    pub acc_title: String,
    pub acc_descr: String,
    /// 書いたままのアクセシブルな説明（行の字下げを落とす前）。
    pub acc_description: String,
}
