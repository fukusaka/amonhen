//! 描ける図の種類（pele の `src/diagrams/registry.ts` と `register()`）。ほかの種類を移したら、ここに足す。

use crate::types::Diagram;

static DIAGRAMS: &[&Diagram] = &[&super::flowchart::FLOWCHART];

/// 種類の名前（pele の `DiagramType`）の図。
pub fn diagram(type_: &str) -> Option<&'static Diagram> {
    DIAGRAMS.iter().copied().find(|d| d.type_ == type_)
}

pub fn registered(type_: &str) -> bool {
    diagram(type_).is_some()
}
