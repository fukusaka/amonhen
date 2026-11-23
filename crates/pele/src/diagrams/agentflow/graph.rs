//! 描くグラフ（pele の `src/diagrams/agentflow/graph.ts`）。読んだ agentflow を、描くノードと辺にする: flow の
//! 中身、たたんだ flow、agentflow の持つ形に寄せた形、クラスのスタイル、パレットの枠。形と入れ子の問題は、
//! 知らせとしてモデルに残す。

use std::collections::{HashMap, HashSet};

use super::color_slots::{SlotNode, assign_color_slots};
use super::db::AgentflowDb;
use super::shapes::{ShapedNode, normalise_node_shapes, resolve_shape_alias};
use super::types::LabelType;
use crate::util::yaml::Value;

#[derive(Clone, Debug)]
pub struct GraphNode {
    pub id: String,
    pub label: Option<String>,
    pub label_type: LabelType,
    pub shape: String,
    pub is_group: bool,
    pub parent_id: Option<String>,
    pub css_styles: Vec<String>,
    pub css_compiled_styles: Vec<String>,
    pub css_classes: String,
    pub dir: Option<String>,
    pub link: Option<String>,
    pub link_target: Option<String>,
    pub tooltip: Option<String>,
    /// 入れ物のパレットの枠。
    pub color_index: Option<i64>,
    /// ノードが何か。パレットの枠を決める。
    pub kind: Option<String>,
}

#[derive(Clone, Debug)]
pub struct GraphEdge {
    pub id: String,
    pub is_user_defined_id: bool,
    pub start: String,
    pub end: String,
    pub type_: String,
    pub label: String,
    pub label_type: LabelType,
    pub thickness: Option<String>,
    pub pattern: Option<String>,
    pub minlen: Option<f64>,
    pub arrow_type_start: String,
    pub arrow_type_end: String,
    pub css_compiled_styles: Vec<String>,
    pub style: Vec<String>,
    pub classes: String,
    pub animate: Option<Value>,
    pub animation: Option<Value>,
    /// 文字列でない値は、どの曲線の名前にも合わない空の文字列にする。
    pub curve: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct AgentGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

fn is_collapsed(metadata: Option<&crate::util::yaml::Map>) -> bool {
    metadata.and_then(|m| m.get("view")) == Some(&Value::Str("collapsed".into()))
}

/// `x || y` の、文字列と値の曲線。
fn curve_of(v: &Value) -> Option<String> {
    if !v.truthy() {
        return None;
    }
    Some(match v {
        Value::Str(s) => s.clone(),
        _ => String::new(),
    })
}

pub fn build_agent_graph(
    db: &mut AgentflowDb,
    palette_length: i64,
    default_curve: Option<&str>,
) -> AgentGraph {
    let mut nodes: Vec<GraphNode> = Vec::new();
    let mut edges: Vec<GraphEdge> = Vec::new();

    let compiled = |db: &AgentflowDb, class_names: &[String]| -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for name in class_names {
            if let Some(def) = db.classes.get(name) {
                out.extend(def.styles.iter().map(|s| crate::js::trim(s).to_string()));
                out.extend(
                    def.text_styles
                        .iter()
                        .map(|s| crate::js::trim(s).to_string()),
                );
            }
        }
        out
    };

    // たたんだ flow の中はすべて隠し、いちばん外のたたんだ flow で表す。Mermaid はたたんだ flow ごとに、閉じた
    // 順に中をすべて歩き、最後に着いた歩みが勝つ。ものは 1 つの入れ物にしか入らないので、後の歩みが前のものに
    // 入れるのはその根からだけ: そこで止まり、根の持ち主は後でたどる。
    let mut hidden: Vec<String> = Vec::new();
    let mut hidden_set: HashSet<String> = HashSet::new();
    let mut owner: HashMap<String, String> = HashMap::new();
    let mut walked: HashSet<String> = HashSet::new();
    for sg in &db.subgraphs {
        if !is_collapsed(sg.metadata.as_ref()) || hidden_set.contains(&sg.id) {
            continue;
        }
        let mut pending = vec![sg.id.clone()];
        while let Some(id) = pending.pop() {
            let Some(members) = db.subgraph(&id).map(|s| &s.nodes) else {
                continue;
            };
            for child in members {
                if *child == sg.id || hidden_set.contains(child) {
                    continue;
                }
                hidden_set.insert(child.clone());
                hidden.push(child.clone());
                owner.insert(child.clone(), sg.id.clone());
                if !walked.contains(child) {
                    pending.push(child.clone());
                }
            }
        }
        walked.insert(sg.id.clone());
    }
    let mut collapsed_into: HashMap<String, String> = HashMap::new();
    for id in &hidden {
        let mut path = vec![id.clone()];
        let mut top = owner[id].clone();
        loop {
            let known = collapsed_into.get(&top).cloned();
            let up = known.clone().or_else(|| owner.get(&top).cloned());
            let Some(up) = up else {
                break;
            };
            if known.is_none() {
                path.push(top.clone());
            }
            top = up;
            if known.is_some() {
                break;
            }
        }
        for p in path {
            collapsed_into.insert(p, top.clone());
        }
    }

    let n = db.subgraphs.len();
    let mut position: HashMap<String, usize> = HashMap::new();
    for (i, sg) in db.subgraphs.iter().enumerate() {
        position.insert(sg.id.clone(), i);
    }

    // 2 つの flow がそれぞれ相手を挙げることがある。輪を閉じる入れ子は落とす。
    let mut parent_of: HashMap<String, String> = HashMap::new();
    let closes_loop =
        |parent_of: &HashMap<String, String>, child: &str, parent: &str, at: usize| {
            // この歩みで先に扱った flow だけが、すでに親の上にありうる。
            match position.get(child) {
                None => return false,
                Some(&c) if c < at => return false,
                _ => {}
            }
            let mut current = Some(parent.to_string());
            let mut hops = 0;
            while let Some(c) = current {
                if hops > n {
                    break;
                }
                if c == child {
                    return true;
                }
                current = parent_of.get(&c).cloned();
                hops += 1;
            }
            false
        };
    let mut warnings: Vec<(String, String)> = Vec::new();
    for i in (0..n).rev() {
        let sg = &db.subgraphs[i];
        if hidden_set.contains(&sg.id) {
            continue;
        }
        for id in &sg.nodes {
            if closes_loop(&parent_of, id, &sg.id, i) {
                warnings.push((
                    format!(
                        "Container \"{}\" cannot contain \"{id}\" because \"{id}\" already contains it. The nesting that would close the loop is dropped.",
                        sg.id
                    ),
                    id.clone(),
                ));
                continue;
            }
            parent_of.insert(id.clone(), sg.id.clone());
        }
    }
    for (message, id) in warnings {
        db.emit_warning("CONTAINMENT_VIOLATION", message, Some(&id));
    }

    let mut by_id: HashMap<String, usize> = HashMap::new();
    for i in (0..n).rev() {
        let sg = &db.subgraphs[i];
        if hidden_set.contains(&sg.id) {
            continue;
        }
        let collapsed = is_collapsed(sg.metadata.as_ref());
        by_id.insert(sg.id.clone(), nodes.len());
        nodes.push(GraphNode {
            id: sg.id.clone(),
            label: Some(sg.title.clone()),
            label_type: sg.label_type,
            parent_id: parent_of.get(&sg.id).cloned(),
            css_styles: Vec::new(),
            css_compiled_styles: compiled(db, &sg.classes),
            css_classes: sg.classes.join(" "),
            shape: if collapsed {
                "collapsedGroup"
            } else {
                "flowGroup"
            }
            .into(),
            dir: sg.dir.clone(),
            is_group: !collapsed,
            link: None,
            link_target: None,
            tooltip: None,
            color_index: None,
            kind: None,
        });
    }

    for vertex in &db.nodes {
        if hidden_set.contains(&vertex.id) {
            continue;
        }
        if let Some(&e) = by_id.get(&vertex.id) {
            let existing = &mut nodes[e];
            existing.css_styles = vertex.styles.clone();
            existing.css_compiled_styles = compiled(db, &vertex.classes);
            existing.css_classes = vertex.classes.join(" ");
            continue;
        }
        let type_ = resolve_shape_alias(vertex.type_.as_deref());
        let mut names = vec!["default".to_string(), "node".to_string()];
        names.extend(vertex.classes.iter().cloned());
        by_id.insert(vertex.id.clone(), nodes.len());
        nodes.push(GraphNode {
            id: vertex.id.clone(),
            label: vertex.text.clone(),
            label_type: vertex.label_type,
            parent_id: parent_of.get(&vertex.id).cloned(),
            css_styles: vertex.styles.clone(),
            css_compiled_styles: compiled(db, &names),
            css_classes: format!("default {}", vertex.classes.join(" ")),
            dir: vertex.dir.clone(),
            link: vertex.link.clone(),
            link_target: vertex.link_target.clone(),
            tooltip: db.tooltips.get(&vertex.id).cloned(),
            is_group: false,
            shape: match type_.as_deref() {
                None | Some("square") => "squareRect".into(),
                Some(t) => t.to_string(),
            },
            color_index: None,
            kind: None,
        });
    }

    for (index, raw) in db.edges.iter().enumerate() {
        let start = collapsed_into.get(&raw.start).unwrap_or(&raw.start).clone();
        let end = collapsed_into.get(&raw.end).unwrap_or(&raw.end).clone();
        // たたんだ 1 つの flow の中の 2 つの間の辺は、つなぐものが残っていない。
        if start == end && raw.start != raw.end {
            continue;
        }
        let mut styles = db.default_edge_style.clone().unwrap_or_default();
        if let Some(s) = &raw.style {
            styles.extend(s.iter().cloned());
        }
        let arrow = match raw.type_.as_deref() {
            Some("arrow_open") => "none",
            Some(t @ ("arrow_cross" | "arrow_point")) => t,
            _ => "arrow_point",
        };
        let curve = raw
            .interpolate
            .as_ref()
            .and_then(curve_of)
            .or_else(|| db.default_interpolate.clone().filter(|s| !s.is_empty()))
            .or_else(|| default_curve.map(str::to_string));
        edges.push(GraphEdge {
            id: match raw.id.as_deref().filter(|s| !s.is_empty()) {
                Some(id) => id.to_string(),
                None => format!("L_{start}_{end}_{index}"),
            },
            is_user_defined_id: raw.is_user_defined_id,
            start,
            end,
            type_: raw.type_.clone().unwrap_or_else(|| "normal".into()),
            label: raw.text.clone(),
            label_type: raw.label_type,
            thickness: raw.stroke.clone(),
            pattern: raw.stroke.clone(),
            minlen: raw.length,
            classes: "edge-thickness-normal edge-pattern-solid flowchart-link".into(),
            arrow_type_start: "none".into(),
            arrow_type_end: arrow.into(),
            css_compiled_styles: compiled(db, &raw.classes),
            style: styles,
            animate: raw.animate.clone(),
            animation: raw.animation.clone(),
            curve,
        });
    }

    normalise_node_shapes(
        nodes
            .iter_mut()
            .map(|n| ShapedNode {
                id: &n.id,
                shape: &mut n.shape,
                is_group: n.is_group,
            })
            .collect(),
        db,
    );

    // 入れ物は書いた順に番号をつける: 入れ子を親から歩く順。`subgraphs` 自身は塊の閉じた順で、入れ子の flow が
    // 親より先。
    let mut children: HashMap<String, Vec<String>> = HashMap::new();
    for sg in &db.subgraphs {
        if let Some(parent) = parent_of.get(&sg.id) {
            children
                .entry(parent.clone())
                .or_default()
                .push(sg.id.clone());
        }
    }
    let mut order: HashMap<String, i64> = HashMap::new();
    for sg in &db.subgraphs {
        if parent_of.contains_key(&sg.id) {
            continue;
        }
        let mut pending = vec![sg.id.clone()];
        while let Some(id) = pending.pop() {
            if order.contains_key(&id) {
                continue;
            }
            let k = order.len() as i64;
            order.insert(id.clone(), k);
            if let Some(kids) = children.get(&id) {
                for kid in kids.iter().rev() {
                    pending.push(kid.clone());
                }
            }
        }
    }

    let db_ref: &AgentflowDb = db;
    assign_color_slots(
        nodes
            .iter_mut()
            .map(|n| SlotNode {
                id: &n.id,
                shape: &n.shape,
                is_group: n.is_group,
                color_index: &mut n.color_index,
                kind: &mut n.kind,
            })
            .collect(),
        |id| {
            if db_ref.connectors.contains(id) {
                return Some("connector");
            }
            db_ref.node(id).map(AgentflowDb::vertex_kind)
        },
        &order,
        palette_length,
    );

    AgentGraph { nodes, edges }
}
