//! 描くノードと辺（pele の `src/diagrams/usecase/graph.ts`）。Mermaid の use case の DB の getData を移したもの。

use super::json::Json;
use super::types::{LabelType, Relationship, UsecaseJsonNode, UsecaseModel, arrow};
use crate::js;
use crate::util::yaml::Map;

pub struct JsonRow {
    /// 配列の 2 つめ以降の値では空。
    pub key: String,
    pub value: String,
}

pub struct UsecaseGraphNode {
    pub id: String,
    pub label: String,
    pub label_type: LabelType,
    /// Mermaid の形の名前。
    pub shape: &'static str,
    pub is_group: bool,
    pub padding: f64,
    pub css_classes: String,
    pub css_styles: Vec<String>,
    pub css_compiled_styles: Vec<String>,
    pub actor_type: Option<&'static str>,
    pub business: bool,
    pub icon: Option<String>,
    pub stereotype: Option<String>,
    pub parent_id: Option<String>,
    pub json_rows: Option<Vec<JsonRow>>,
    pub boundary_type: Option<&'static str>,
}

pub struct UsecaseGraphEdge {
    pub id: String,
    pub start: String,
    pub end: String,
    /// association、include、extend、generalization、note。
    pub relationship_type: &'static str,
    /// solid か dotted。
    pub pattern: &'static str,
    pub arrow_type_start: &'static str,
    pub arrow_type_end: &'static str,
    pub label: Option<String>,
    pub label_type: Option<LabelType>,
    pub classes: String,
    pub style: Vec<String>,
    pub css_compiled_styles: Vec<String>,
    pub animate: bool,
    pub minlen: f64,
    /// 注釈からそれが指すものへの点線。
    pub internal: bool,
}

pub struct UsecaseGraph {
    pub nodes: Vec<UsecaseGraphNode>,
    pub edges: Vec<UsecaseGraphEdge>,
}

fn actor_shape(type_: &str) -> &'static str {
    match type_ {
        "hollow" => "usecaseActorHollow",
        "awesome" => "usecaseActorAwesome",
        "icon" => "usecaseActorIcon",
        _ => "usecaseActor",
    }
}

/// 矢印の種類ごとの始まりと終わりの印（`Arrow` の順）。
const MARKERS: [[&str; 2]; 7] = [
    ["none", "arrow_point"],
    ["arrow_point", "none"],
    ["none", "none"],
    ["none", "arrow_circle"],
    ["none", "arrow_cross"],
    ["arrow_circle", "none"],
    ["arrow_cross", "none"],
];

fn scalar(value: &Json) -> String {
    match value {
        Json::Str(s) => s.clone(),
        Json::Null => "null".to_string(),
        Json::Bool(b) => b.to_string(),
        Json::Num(n) => js::fmt(*n),
        Json::Arr(_) | Json::Obj(_) => String::new(),
    }
}

/// JSON のオブジェクトの葉ごとに 1 行。深さ優先で、鍵を書いた順に。入れ子は自前の積みで持つ。
pub fn flatten_json_rows(json: &UsecaseJsonNode) -> Vec<JsonRow> {
    let mut rows = Vec::new();
    let mut stack: Vec<(Option<&Json>, String, String)> =
        vec![(Some(&json.value), String::new(), String::new())];
    let mut children: Vec<(Option<&Json>, String, String)> = Vec::new();
    while let Some((current, path, pointer)) = stack.pop() {
        children.clear();
        match current {
            Some(Json::Arr(items)) => {
                if items.is_empty() {
                    rows.push(JsonRow {
                        key: path.clone(),
                        value: "[]".to_string(),
                    });
                } else if items
                    .iter()
                    .all(|i| !matches!(i, Json::Arr(_) | Json::Obj(_)))
                {
                    for (i, item) in items.iter().enumerate() {
                        rows.push(JsonRow {
                            key: if i == 0 { path.clone() } else { String::new() },
                            value: scalar(item),
                        });
                    }
                } else {
                    for (i, item) in items.iter().enumerate() {
                        children.push((
                            Some(item),
                            format!("{path}[{i}]"),
                            format!("{pointer}/{i}"),
                        ));
                    }
                }
            }
            Some(Json::Obj(object)) => {
                let keys = match json.property_order.get(&pointer) {
                    Some(k) => k.clone(),
                    None => object.keys(),
                };
                if keys.is_empty() {
                    rows.push(JsonRow {
                        key: path.clone(),
                        value: "{}".to_string(),
                    });
                }
                for key in &keys {
                    children.push((
                        object.get(key),
                        if path.is_empty() {
                            key.clone()
                        } else {
                            format!("{path}.{key}")
                        },
                        format!("{pointer}/{}", key.replace('~', "~0").replace('/', "~1")),
                    ));
                }
            }
            Some(other) => rows.push(JsonRow {
                key: path.clone(),
                value: scalar(other),
            }),
            None => rows.push(JsonRow {
                key: path.clone(),
                value: "undefined".to_string(),
            }),
        }
        while let Some(c) = children.pop() {
            stack.push(c);
        }
    }
    rows
}

fn class_list(names: &[Option<&str>]) -> String {
    names
        .iter()
        .filter_map(|n| n.filter(|s| !s.is_empty()))
        .collect::<Vec<_>>()
        .join(" ")
}

fn markers(relationship: &Relationship) -> [&'static str; 2] {
    if relationship.type_ == "generalization" {
        return ["none", "extension"];
    }
    if relationship.type_ != "association" {
        return MARKERS[arrow::SOLID_ARROW as usize];
    }
    MARKERS
        .get(relationship.arrow_type as usize)
        .copied()
        .unwrap_or(MARKERS[arrow::LINE_SOLID as usize])
}

/// 読んだ use case の図を描くものにまとめる: actor・use case・注釈・JSON の表・囲みをクラスのスタイルつきの
/// ノードに、関係と注釈の線を印つきの辺に。
pub fn build_usecase_graph(model: &UsecaseModel) -> UsecaseGraph {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    // 後のクラスが性質を勝ち取るが、その性質が最初に取った場所は保つ。
    let compiled = |class_names: &[String]| -> Vec<String> {
        let mut out = Map::new();
        for name in std::iter::once("default").chain(class_names.iter().map(String::as_str)) {
            let Some(definition) = model.class_defs.get(name) else {
                continue;
            };
            for raw in &definition.styles {
                let style = js::trim(raw);
                let property = js::trim(match style.find(':') {
                    Some(c) => &style[..c],
                    None => style,
                });
                if !property.is_empty() {
                    out.set(property, crate::util::yaml::Value::Str(style.to_string()));
                }
            }
        }
        out.0
            .into_iter()
            .map(|(_, v)| match v {
                crate::util::yaml::Value::Str(s) => s,
                _ => String::new(),
            })
            .collect()
    };

    for actor in model.actors.values() {
        let kind = format!("usecase-actor-{}", actor.type_);
        let mut names = vec![
            Some("default"),
            Some("usecase-actor"),
            Some(kind.as_str()),
            actor.business.then_some("usecase-business"),
        ];
        names.extend(actor.classes.iter().map(|c| Some(c.as_str())));
        nodes.push(UsecaseGraphNode {
            id: actor.id.clone(),
            label: actor.label.clone(),
            label_type: actor.label_type,
            shape: actor_shape(actor.type_),
            is_group: false,
            padding: 10.0,
            css_classes: class_list(&names),
            css_styles: actor.styles.clone(),
            css_compiled_styles: compiled(&actor.classes),
            actor_type: Some(actor.type_),
            business: actor.business,
            icon: actor.icon.clone().filter(|s| !s.is_empty()),
            stereotype: actor.stereotype.clone().filter(|s| !s.is_empty()),
            parent_id: actor.parent_id.clone().filter(|s| !s.is_empty()),
            json_rows: None,
            boundary_type: None,
        });
    }
    for use_case in model.use_cases.values() {
        let ellipse = use_case.shape == "ellipse";
        let kind = format!("usecase-{}", use_case.shape);
        let mut names = vec![
            Some("default"),
            Some("usecase-element"),
            Some(kind.as_str()),
            use_case.business.then_some("usecase-business"),
        ];
        names.extend(use_case.classes.iter().map(|c| Some(c.as_str())));
        nodes.push(UsecaseGraphNode {
            id: use_case.id.clone(),
            label: use_case.label.clone(),
            label_type: use_case.label_type,
            shape: if !ellipse {
                "rect"
            } else if use_case.business {
                "usecaseBusiness"
            } else {
                "usecaseEllipse"
            },
            is_group: false,
            padding: if ellipse { 20.0 } else { 10.0 },
            css_classes: class_list(&names),
            css_styles: use_case.styles.clone(),
            css_compiled_styles: compiled(&use_case.classes),
            actor_type: None,
            business: use_case.business,
            icon: None,
            stereotype: use_case.stereotype.clone().filter(|s| !s.is_empty()),
            parent_id: use_case.parent_id.clone().filter(|s| !s.is_empty()),
            json_rows: None,
            boundary_type: None,
        });
    }
    for note in model.notes.values() {
        nodes.push(UsecaseGraphNode {
            id: note.id.clone(),
            label: note.label.clone(),
            label_type: note.label_type,
            shape: "note",
            is_group: false,
            padding: 10.0,
            css_classes: "default usecase-note".to_string(),
            css_styles: Vec::new(),
            css_compiled_styles: compiled(&[]),
            actor_type: None,
            business: false,
            icon: None,
            stereotype: None,
            parent_id: None,
            json_rows: None,
            boundary_type: None,
        });
    }
    for json in model.json_nodes.values() {
        let mut names = vec![Some("default"), Some("usecase-json-table")];
        names.extend(json.classes.iter().map(|c| Some(c.as_str())));
        nodes.push(UsecaseGraphNode {
            id: json.id.clone(),
            label: json.id.clone(),
            label_type: LabelType::Text,
            shape: "usecaseJsonTable",
            is_group: false,
            padding: 10.0,
            css_classes: class_list(&names),
            css_styles: json.styles.clone(),
            css_compiled_styles: compiled(&json.classes),
            actor_type: None,
            business: false,
            icon: None,
            stereotype: None,
            parent_id: None,
            json_rows: Some(flatten_json_rows(json)),
            boundary_type: None,
        });
    }
    for boundary in model.system_boundaries.values() {
        let kind = format!("system-boundary-{}", boundary.type_);
        let mut names = vec![
            Some("default"),
            Some("system-boundary"),
            Some(kind.as_str()),
        ];
        names.extend(boundary.classes.iter().map(|c| Some(c.as_str())));
        nodes.push(UsecaseGraphNode {
            id: boundary.id.clone(),
            label: boundary.label.clone(),
            label_type: boundary.label_type,
            shape: "usecaseSystemBoundary",
            is_group: true,
            padding: 20.0,
            css_classes: class_list(&names),
            css_styles: boundary.styles.clone(),
            css_compiled_styles: compiled(&boundary.classes),
            actor_type: None,
            business: false,
            icon: None,
            stereotype: None,
            parent_id: None,
            json_rows: None,
            boundary_type: Some(boundary.type_),
        });
    }

    for relationship in &model.relationships {
        let semantic = relationship.type_ == "include" || relationship.type_ == "extend";
        let [start, end] = markers(relationship);
        let animated = relationship.animate || relationship.animation.is_some();
        let kind = format!("relationship-{}", relationship.type_);
        let animation = format!(
            "edge-animation-{}",
            relationship.animation.unwrap_or("fast")
        );
        let mut names = vec![Some("default"), Some("relationship"), Some(kind.as_str())];
        names.extend(relationship.classes.iter().map(|c| Some(c.as_str())));
        names.push(animated.then_some(animation.as_str()));
        let mut edge = UsecaseGraphEdge {
            id: relationship.id.clone(),
            start: relationship.source.clone(),
            end: relationship.target.clone(),
            relationship_type: relationship.type_,
            pattern: if semantic { "dotted" } else { "solid" },
            arrow_type_start: start,
            arrow_type_end: end,
            label: None,
            label_type: None,
            classes: class_list(&names),
            style: relationship.styles.clone(),
            css_compiled_styles: compiled(&relationship.classes),
            animate: relationship.animate,
            minlen: relationship.minlen,
            internal: false,
        };
        // include と extend は、関係がどんなラベルを持っても、その種類でラベルを付ける。
        if semantic {
            edge.label = Some(relationship.type_.to_string());
            edge.label_type = Some(LabelType::Text);
        } else if relationship.type_ == "association" {
            if let Some(l) = relationship.label.as_ref().filter(|l| !l.is_empty()) {
                edge.label = Some(l.clone());
            }
            if relationship.label_type.is_some() {
                edge.label_type = relationship.label_type;
            }
        }
        edges.push(edge);
    }
    for note in model.notes.values() {
        edges.push(UsecaseGraphEdge {
            id: format!("{}-edge", note.id),
            start: note.id.clone(),
            end: note.target.clone(),
            relationship_type: "note",
            pattern: "dotted",
            arrow_type_start: "none",
            arrow_type_end: "none",
            label: None,
            label_type: None,
            classes: "default relationship relationship-note".to_string(),
            style: Vec::new(),
            css_compiled_styles: compiled(&[]),
            animate: false,
            minlen: 1.0,
            internal: true,
        });
    }
    UsecaseGraph { nodes, edges }
}
