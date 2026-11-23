//! eventmodeling の形（pele の `src/diagrams/eventmodeling/model.ts`）。

use std::collections::{HashMap, HashSet};

use super::parser::EventModelAst;
use crate::diagrams::common::tokens::{acc_descr_value, acc_title_value, title_value};
use crate::errors::{ErrorCode, PeleError, Result, locate};
use crate::js;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EntityKind {
    Ui,
    Processor,
    Command,
    Readmodel,
    Event,
}

impl EntityKind {
    pub fn name(self) -> &'static str {
        match self {
            EntityKind::Ui => "ui",
            EntityKind::Processor => "processor",
            EntityKind::Command => "command",
            EntityKind::Readmodel => "readmodel",
            EntityKind::Event => "event",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LaneGroup {
    Trigger,
    State,
    Stream,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmLane {
    /// Mermaid の並べる鍵: UI/Automation は 0、Command/Read Model は 100、Events は 200、名前空間ごとにその後の
    /// 空いた数。
    pub index: f64,
    pub label: String,
    pub group: LaneGroup,
    pub namespace: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmBox {
    /// 書いたとおりの枠の番号（"01" など）。
    pub id: String,
    pub kind: EntityKind,
    pub entity: String,
    pub name: String,
    pub namespace: Option<String>,
    pub reset: bool,
    /// `EventModelingModel::lanes_made` の番号。
    pub lane: usize,
    /// 枠の、または枠が指すデータの塊の例。
    pub data: Option<String>,
    pub data_type: Option<String>,
}

/// 箱の番号。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EmRelation {
    pub source: usize,
    pub target: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmNote {
    pub frame: String,
    pub text: String,
    pub data_type: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmSpecification {
    pub frame: String,
    pub given: Vec<String>,
    pub when: Vec<String>,
    pub then: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EventModelingModel {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub boxes: Vec<EmBox>,
    /// 作った順のレーン。
    pub lanes_made: Vec<EmLane>,
    /// 上から下へ並べたレーン（`lanes_made` の番号）。
    pub lanes: Vec<usize>,
    pub relations: Vec<EmRelation>,
    pub notes: Vec<EmNote>,
    pub specifications: Vec<EmSpecification>,
    pub entities: Vec<String>,
}

fn kind_of(t: &str) -> EntityKind {
    match t {
        "ui" => EntityKind::Ui,
        "pcr" | "processor" => EntityKind::Processor,
        "rmo" | "readmodel" => EntityKind::Readmodel,
        "cmd" | "command" => EntityKind::Command,
        _ => EntityKind::Event,
    }
}

/// [最初の番号, ラベル, 名前空間のラベルの頭]
fn group_info(group: LaneGroup) -> (f64, &'static str, &'static str) {
    match group {
        LaneGroup::Trigger => (0.0, "UI/Automation", "UI/A: "),
        LaneGroup::State => (100.0, "Command/Read Model", "C/RM: "),
        LaneGroup::Stream => (200.0, "Events", "Stream: "),
    }
}

fn group_of(kind: EntityKind) -> LaneGroup {
    match kind {
        EntityKind::Ui | EntityKind::Processor => LaneGroup::Trigger,
        EntityKind::Event => LaneGroup::Stream,
        _ => LaneGroup::State,
    }
}

/// 外側の括弧の間の文字から、行が分け合う字下げを除いたもの。
fn block_text(raw: &str) -> String {
    let open = raw.find('{');
    let close = raw.rfind('}');
    let inner = match (open, close) {
        (Some(o), Some(c)) if c > o => &raw[o + 1..c],
        (None, _) if js::len16(raw) >= 2 => {
            // 引用符で囲んだ値。両端は ASCII。
            let mut cs = raw.chars();
            cs.next();
            cs.next_back();
            cs.as_str()
        }
        _ => "",
    };
    let joined = inner.replace('\r', "");
    let mut lines: Vec<&str> = joined.split('\n').collect();
    while !lines.is_empty() && js::trim(lines[0]).is_empty() {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| js::trim(l).is_empty()) {
        lines.pop();
    }
    let mut indent = usize::MAX;
    for line in &lines {
        if js::trim(line).is_empty() {
            continue;
        }
        indent = indent.min(js::len16(line) - js::len16(js::trim_start(line)));
    }
    lines
        .iter()
        .map(|line| {
            // 字下げは JS の空白だけで、どれも 1 つの UTF-16 の単位。
            let n = indent.min(js::len16(line));
            let start = line.char_indices().nth(n).map_or(line.len(), |(k, _)| k);
            js::trim_end(&line[start..]).to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `.replace(/\n\s+/g, '\n')`
fn squeeze_indents(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '\n' {
            while chars.peek().is_some_and(|&n| js::is_js_space(n)) {
                chars.next();
            }
        }
    }
    out
}

pub fn build_event_model(ast: &EventModelAst, title: Option<&str>) -> Result<EventModelingModel> {
    let mut ids: HashSet<&str> = HashSet::new();
    for frame in &ast.frames {
        if ids.contains(frame.name.as_str()) {
            let line = locate(&ast.source, frame.offset).line;
            let mut e = PeleError::typed(
                format!(
                    "Duplicate event modeling frame ID \"{}\" on line {line}",
                    frame.name
                ),
                ErrorCode::Semantic,
                "eventmodeling",
            );
            e.line = line;
            return Err(e);
        }
        ids.insert(&frame.name);
    }

    let mut data: HashMap<&str, &str> = HashMap::new();
    for entity in &ast.data_entities {
        data.entry(&entity.name)
            .or_insert(entity.data_block_value.as_str());
    }

    // グループごとに 1 つのレーンと、グループの中の名前空間ごとにもう 1 つ。
    let mut lanes_made: Vec<EmLane> = Vec::new();
    let mut lane_of: HashMap<(LaneGroup, Option<String>), usize> = HashMap::new();
    let mut next: HashMap<LaneGroup, f64> = HashMap::new();
    let mut lane = |kind: EntityKind, namespace: Option<&str>| -> usize {
        let group = group_of(kind);
        let key = (group, namespace.map(str::to_string));
        if let Some(&found) = lane_of.get(&key) {
            return found;
        }
        let (first, label, prefix) = group_info(group);
        let mut index = first;
        if namespace.is_some() {
            index = next.get(&group).copied().unwrap_or(first + 1.0);
            next.insert(group, index + 1.0);
        }
        lanes_made.push(EmLane {
            index,
            label: match namespace {
                None => label.to_string(),
                Some(ns) => format!("{prefix}{ns}"),
            },
            group,
            namespace: namespace.map(str::to_string),
        });
        lane_of.insert(key, lanes_made.len() - 1);
        lanes_made.len() - 1
    };

    let mut boxes: Vec<EmBox> = Vec::new();
    let mut relations: Vec<EmRelation> = Vec::new();
    for (index, frame) in ast.frames.iter().enumerate() {
        // 2 つの部分の名前だけが名前空間を持つ。
        let parts: Vec<&str> = frame.entity_identifier.split('.').collect();
        let namespace = (parts.len() == 2).then(|| parts[0]);
        let kind = kind_of(&frame.model_entity_type);
        let mut text = frame.data_inline_value.as_deref().map(block_text);
        if let Some(reference) = &frame.data_reference
            && let Some(block) = data.get(reference.as_str())
        {
            text = Some(block_text(block));
        }
        let b = EmBox {
            id: frame.name.clone(),
            kind,
            entity: frame.entity_identifier.clone(),
            name: if parts.len() == 2 {
                parts[1].to_string()
            } else {
                frame.entity_identifier.clone()
            },
            namespace: namespace.map(str::to_string),
            reset: frame.reset,
            lane: lane(kind, namespace),
            data: text,
            data_type: frame.data_type.clone(),
        };
        let target = boxes.len();
        let (reset, box_lane) = (b.reset, b.lane);
        boxes.push(b);

        if !frame.source_frames.is_empty() {
            // 元は枠の順に、すでに箱のある枠だけをつなぐ。
            let wanted: HashSet<&str> = frame.source_frames.iter().map(String::as_str).collect();
            for (source, s) in boxes.iter().enumerate() {
                if wanted.contains(s.id.as_str()) {
                    relations.push(EmRelation { source, target });
                }
            }
        } else if index > 0 && !reset {
            // でなければ、別のレーンのいちばん近い前の枠がこれに送る。
            for i in (0..index).rev() {
                if boxes[i].lane != box_lane {
                    relations.push(EmRelation { source: i, target });
                    break;
                }
            }
        }
    }

    let mut lanes: Vec<usize> = (0..lanes_made.len()).collect();
    lanes.sort_by(|&a, &b| js::order(lanes_made[a].index - lanes_made[b].index));

    let own_title = ast
        .title
        .as_deref()
        .map(title_value)
        .filter(|t| !t.is_empty());
    Ok(EventModelingModel {
        title: own_title.or_else(|| title.map(str::to_string)),
        acc_title: ast
            .acc_title
            .as_deref()
            .filter(|t| !t.is_empty())
            .map(|t| js::trim_start(&acc_title_value(t)).to_string()),
        acc_descr: ast
            .acc_descr
            .as_deref()
            .filter(|t| !t.is_empty())
            .map(|t| squeeze_indents(&acc_descr_value(t))),
        boxes,
        lanes_made,
        lanes,
        relations,
        notes: ast
            .note_entities
            .iter()
            .map(|n| EmNote {
                frame: n.source_frame.clone(),
                text: block_text(&n.data_block_value),
                data_type: n.data_type.clone(),
            })
            .collect(),
        specifications: ast
            .gwt_entities
            .iter()
            .map(|g| EmSpecification {
                frame: g.source_frame.clone(),
                given: g.given_statements.clone(),
                when: g.when_statements.clone(),
                then: g.then_statements.clone(),
            })
            .collect(),
        entities: ast.model_entities.clone(),
    })
}
