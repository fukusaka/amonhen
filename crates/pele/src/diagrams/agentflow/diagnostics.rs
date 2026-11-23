//! agentflow の知らせの名前（pele の `src/diagrams/agentflow/diagnostics.ts`）。読み手が出すのは `EMITTED` の
//! ものだけで、ほかは図の意味を確かめる道具のために取ってある。

pub const ALL: [&str; 12] = [
    "SHAPE_UNSUPPORTED",
    "SHAPE_REMOVED",
    "EDGE_OPERATOR_UNSUPPORTED",
    "REFERENCE_EDGE_LABEL_REJECTED",
    "CONNECTOR_REF_UNRESOLVED",
    "CONNECTOR_REF_NOT_A_CONNECTOR",
    "METADATA_KEY_MISAPPLIED",
    "DUPLICATE_ID_NODE",
    "RESERVED_SYNTHETIC_ID",
    "CONTAINMENT_VIOLATION",
    "EDGE_SEMANTIC_CONTRADICTION",
    "FLOW_NO_INPUT",
];

pub const EMITTED: [&str; 3] = [
    "SHAPE_UNSUPPORTED",
    "SHAPE_REMOVED",
    "CONTAINMENT_VIOLATION",
];

pub fn reserved() -> impl Iterator<Item = &'static str> {
    ALL.into_iter().filter(|id| !EMITTED.contains(id))
}
