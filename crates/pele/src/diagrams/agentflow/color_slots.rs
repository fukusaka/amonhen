//! 色の枠（pele の `src/diagrams/agentflow/colorSlots.ts`）。Mermaid と同じく 2 つの決まり: ノードは種類の色を
//! 決まった枠から取るので、まわりを書き換えても色が変わらない。入れ物は種類の上の枠を書いた順に回るので、中の
//! ノードの色にならない。

use std::collections::HashMap;

pub const KIND_SLOT: [(&str, i64); 7] = [
    ("tool", 0),
    ("task", 1),
    ("decision", 2),
    ("input", 3),
    ("refdoc", 4),
    ("connector", 5),
    ("action", 6),
];

pub const KIND_COUNT: i64 = KIND_SLOT.len() as i64;

pub fn kind_slot(kind: &str) -> Option<i64> {
    KIND_SLOT.iter().find(|(k, _)| *k == kind).map(|&(_, s)| s)
}

/// 少なくとも 1 つ。種類より短いパレットでも入れ物に枠があるように。
pub fn container_slot_count(palette_length: i64) -> i64 {
    (palette_length - KIND_COUNT).max(1)
}

pub fn container_slot(n: i64, palette_length: i64) -> i64 {
    let slot = KIND_COUNT + n % container_slot_count(palette_length);
    if palette_length > 0 {
        slot % palette_length
    } else {
        slot
    }
}

pub struct SlotNode<'a> {
    pub id: &'a str,
    pub shape: &'a str,
    pub is_group: bool,
    pub color_index: &'a mut Option<i64>,
    pub kind: &'a mut Option<String>,
}

pub fn assign_color_slots(
    nodes: Vec<SlotNode>,
    kind_of: impl Fn(&str) -> Option<&'static str>,
    container_order: &HashMap<String, i64>,
    palette_length: i64,
) {
    let mut fallback = container_order.len() as i64;
    for node in nodes {
        // たたんだ入れ物はノードとして描くが枠は保つ。たたんでも後の入れ物の色が変わらないように。
        if node.is_group || node.shape == "collapsedGroup" {
            let n = match container_order.get(node.id) {
                Some(&n) => n,
                None => {
                    fallback += 1;
                    fallback - 1
                }
            };
            *node.color_index = Some(container_slot(n, palette_length));
            continue;
        }
        if let Some(kind) = kind_of(node.id).filter(|k| kind_slot(k).is_some()) {
            *node.kind = Some(kind.to_string());
        }
    }
}
