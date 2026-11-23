//! wardley の形を組むもの（pele の `src/diagrams/wardley/builder.ts`、Mermaid の db）。

use std::collections::HashMap;

use crate::errors::{ErrorCode, PeleError, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceStrategy {
    Build,
    Buy,
    Outsource,
    Market,
}

impl SourceStrategy {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "build" => Some(SourceStrategy::Build),
            "buy" => Some(SourceStrategy::Buy),
            "outsource" => Some(SourceStrategy::Outsource),
            "market" => Some(SourceStrategy::Market),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            SourceStrategy::Build => "build",
            SourceStrategy::Buy => "buy",
            SourceStrategy::Outsource => "outsource",
            SourceStrategy::Market => "market",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    Forward,
    Backward,
    Bidirectional,
}

/// 座標は百分率: x は進化、y は見える度合い。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WardleyNode {
    pub id: String,
    pub label: String,
    pub x: Option<f64>,
    pub y: Option<f64>,
    /// 'anchor'・'component'・'pipeline-component'
    pub class_name: Option<String>,
    pub label_offset_x: Option<f64>,
    pub label_offset_y: Option<f64>,
    pub in_pipeline: bool,
    pub is_pipeline_parent: bool,
    pub inertia: Option<bool>,
    pub source_strategy: Option<SourceStrategy>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WardleyLink {
    pub source: String,
    pub target: String,
    pub dashed: bool,
    pub label: Option<String>,
    pub flow: Option<Flow>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WardleyTrend {
    pub node_id: String,
    pub target_x: f64,
    pub target_y: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WardleyPipeline {
    pub node_id: String,
    pub component_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WardleyAnnotation {
    pub number: f64,
    pub coordinates: Vec<(f64, f64)>,
    pub text: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WardleyNote {
    pub text: String,
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WardleyForce {
    pub name: String,
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WardleyAxesConfig {
    pub x_label: Option<String>,
    pub y_label: Option<String>,
    pub stages: Option<Vec<String>>,
    /// 段階ごとの右の端（0〜1）。
    pub stage_boundaries: Option<Vec<f64>>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WardleyBuildResult {
    pub nodes: Vec<WardleyNode>,
    pub links: Vec<WardleyLink>,
    pub trends: Vec<WardleyTrend>,
    pub pipelines: Vec<WardleyPipeline>,
    pub annotations: Vec<WardleyAnnotation>,
    pub notes: Vec<WardleyNote>,
    pub accelerators: Vec<WardleyForce>,
    pub deaccelerators: Vec<WardleyForce>,
    pub annotations_box: Option<(f64, f64)>,
    pub axes: WardleyAxesConfig,
    pub size: Option<(f64, f64)>,
}

/// 足すノード。`inertia`・`source_strategy` は JS で値が `undefined` でもキーがあるときの上書きを写すため、
/// 外側の Option がキーのあるなし。
pub struct NewNode {
    pub id: String,
    pub label: String,
    pub x: f64,
    pub y: f64,
    pub class_name: Option<String>,
    pub label_offset_x: Option<f64>,
    pub label_offset_y: Option<f64>,
    pub inertia: Option<Option<bool>>,
    pub source_strategy: Option<Option<SourceStrategy>>,
}

#[derive(Default)]
pub struct WardleyBuilder {
    /// 入れた順（JS の `Map`）。
    nodes: Vec<WardleyNode>,
    index: HashMap<String, usize>,
    labels: Option<HashMap<String, String>>,
    links: Vec<WardleyLink>,
    trends: Vec<WardleyTrend>,
    pipelines: Vec<WardleyPipeline>,
    annotations: Vec<WardleyAnnotation>,
    notes: Vec<WardleyNote>,
    accelerators: Vec<WardleyForce>,
    deaccelerators: Vec<WardleyForce>,
    annotations_box: Option<(f64, f64)>,
    axes: WardleyAxesConfig,
    size: Option<(f64, f64)>,
}

impl WardleyBuilder {
    /// もう一度足したノードは、新しいものが決めないかぎり種類とラベルのずれを保つ。
    pub fn add_node(&mut self, node: NewNode) {
        let merged = |existing: WardleyNode| WardleyNode {
            id: node.id.clone(),
            label: node.label.clone(),
            x: Some(node.x),
            y: Some(node.y),
            class_name: node.class_name.clone().or(existing.class_name),
            label_offset_x: node.label_offset_x.or(existing.label_offset_x),
            label_offset_y: node.label_offset_y.or(existing.label_offset_y),
            in_pipeline: existing.in_pipeline,
            is_pipeline_parent: existing.is_pipeline_parent,
            inertia: node.inertia.unwrap_or(existing.inertia),
            source_strategy: node.source_strategy.unwrap_or(existing.source_strategy),
        };
        match self.index.get(&node.id) {
            Some(&k) => {
                let existing = std::mem::take(&mut self.nodes[k]);
                self.nodes[k] = merged(existing);
            }
            None => {
                let fresh = merged(WardleyNode {
                    id: node.id.clone(),
                    label: node.label.clone(),
                    ..Default::default()
                });
                self.index.insert(node.id.clone(), self.nodes.len());
                self.nodes.push(fresh);
            }
        }
        self.labels = None;
    }

    pub fn add_link(&mut self, link: WardleyLink) {
        self.links.push(link);
    }

    pub fn add_trend(&mut self, trend: WardleyTrend) {
        match self.trends.iter_mut().find(|t| t.node_id == trend.node_id) {
            Some(slot) => *slot = trend,
            None => self.trends.push(trend),
        }
    }

    pub fn start_pipeline(&mut self, node_id: &str) {
        let fresh = WardleyPipeline {
            node_id: node_id.to_string(),
            component_ids: Vec::new(),
        };
        match self.pipelines.iter_mut().find(|p| p.node_id == node_id) {
            Some(slot) => *slot = fresh,
            None => self.pipelines.push(fresh),
        }
        if let Some(&k) = self.index.get(node_id) {
            self.nodes[k].is_pipeline_parent = true;
        }
    }

    pub fn add_pipeline_component(&mut self, pipeline_node_id: &str, component_id: &str) {
        if let Some(p) = self
            .pipelines
            .iter_mut()
            .find(|p| p.node_id == pipeline_node_id)
        {
            p.component_ids.push(component_id.to_string());
        }
        if let Some(&k) = self.index.get(component_id) {
            self.nodes[k].in_pipeline = true;
        }
    }

    pub fn add_annotation(&mut self, annotation: WardleyAnnotation) {
        self.annotations.push(annotation);
    }

    pub fn add_note(&mut self, note: WardleyNote) {
        self.notes.push(note);
    }

    pub fn add_accelerator(&mut self, accelerator: WardleyForce) {
        self.accelerators.push(accelerator);
    }

    pub fn add_deaccelerator(&mut self, deaccelerator: WardleyForce) {
        self.deaccelerators.push(deaccelerator);
    }

    pub fn set_annotations_box(&mut self, x: f64, y: f64) {
        self.annotations_box = Some((x, y));
    }

    /// 段階と境を決める（Mermaid の `setAxes` のうち、この図が使う所）。
    pub fn set_axes(&mut self, stages: Vec<String>, stage_boundaries: Vec<f64>) {
        self.axes.stages = Some(stages);
        self.axes.stage_boundaries = Some(stage_boundaries);
    }

    pub fn set_size(&mut self, width: f64, height: f64) {
        self.size = Some((width, height));
    }

    pub fn get_node(&self, id: &str) -> Option<&WardleyNode> {
        self.index.get(id).map(|&k| &self.nodes[k])
    }

    /// 名前はノードの id か、でなければそれをラベルに持つ最初のノード。パイプラインの部品は "Parent_Child" の
    /// ような id を持ち、ラベルでつなぐ。
    pub fn resolve_node_id(&mut self, name: &str) -> String {
        if self.index.contains_key(name) {
            return name.to_string();
        }
        let nodes = &self.nodes;
        let labels = self.labels.get_or_insert_with(|| {
            let mut labels = HashMap::new();
            for node in nodes {
                labels
                    .entry(node.label.clone())
                    .or_insert_with(|| node.id.clone());
            }
            labels
        });
        labels
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string())
    }

    pub fn build(self) -> Result<WardleyBuildResult> {
        for node in &self.nodes {
            if node.x.is_none() || node.y.is_none() {
                return Err(PeleError::typed(
                    format!("Node \"{}\" is missing coordinates", node.label),
                    ErrorCode::Semantic,
                    "wardley",
                ));
            }
        }
        Ok(WardleyBuildResult {
            nodes: self.nodes,
            links: self.links,
            trends: self.trends,
            pipelines: self.pipelines,
            annotations: self.annotations,
            notes: self.notes,
            accelerators: self.accelerators,
            deaccelerators: self.deaccelerators,
            annotations_box: self.annotations_box,
            axes: self.axes,
            size: self.size,
        })
    }
}
