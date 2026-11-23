//! 描ける図の種類（pele の `src/diagrams/registry.ts` と `register()`）。
//!
//! 種類を足すには:
//!
//! 1. `diagrams/<dir>/mod.rs` に `pub static <NAME>: Diagram` を置く（`flowchart/mod.rs` と同じ形。`type_` は pele の
//!    `DiagramType` の名前、`detect` が返す名前）。`<dir>` は pele の `src/diagrams/<dir>` と同じ名前。
//! 2. `diagrams/mod.rs` に `pub mod <dir>;` を 1 行足す。
//! 3. 下の `DIAGRAMS` に `&super::<dir>::<NAME>,` を 1 行足す。
//!
//! どちらの並びも `<dir>` のアルファベット順に保つ（並行して足しても、合わせるのが 1 行ずつで済むように）。
//! 足した種類の pele の例（`crates/core/tests/data/pele/<corpus>-docs-*`）は `tests/oracles.rs` が自ずと確かめる。

use crate::types::Diagram;

// 1 行に 1 つの種類（rustfmt にまとめさせない）。
#[rustfmt::skip]
static DIAGRAMS: &[&Diagram] = &[
    &super::agentflow::AGENTFLOW,
    &super::architecture::ARCHITECTURE,
    &super::block::BLOCK,
    &super::c4::C4,
    &super::class::CLASS,
    &super::cynefin::CYNEFIN,
    &super::er::ER,
    &super::eventmodeling::EVENTMODELING,
    &super::flowchart::FLOWCHART,
    &super::gantt::GANTT,
    &super::git::GIT_GRAPH,
    &super::info::INFO,
    &super::ishikawa::ISHIKAWA,
    &super::journey::JOURNEY,
    &super::kanban::KANBAN,
    &super::mindmap::MINDMAP,
    &super::packet::PACKET,
    &super::pie::PIE,
    &super::quadrant::QUADRANT_CHART,
    &super::radar::RADAR,
    &super::railroad::RAILROAD,
    &super::requirement::REQUIREMENT,
    &super::sankey::SANKEY,
    &super::sequence::SEQUENCE,
    &super::state::STATE,
    &super::swimlane::SWIMLANE,
    &super::timeline::TIMELINE,
    &super::treemap::TREEMAP,
    &super::treeview::TREE_VIEW,
    &super::usecase::USECASE,
    &super::venn::VENN,
    &super::wardley::WARDLEY,
    &super::xychart::XYCHART,
];

/// 種類の名前（pele の `DiagramType`）の図。
pub fn diagram(type_: &str) -> Option<&'static Diagram> {
    DIAGRAMS.iter().copied().find(|d| d.type_ == type_)
}

pub fn registered(type_: &str) -> bool {
    diagram(type_).is_some()
}
