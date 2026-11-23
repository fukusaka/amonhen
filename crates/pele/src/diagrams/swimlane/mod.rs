//! swimlane（pele の `src/diagrams/swimlane/`）。上の段の subgraph をレーンにした flowchart。書き方も読んだ形も
//! flowchart と同じ。

use std::any::Any;

use super::flowchart::db::FlowDb;
use super::flowchart::graph::{FlowGraph, GraphNode};
use super::flowchart::render::{FlowVariant, render_flowchart};
use super::flowchart::types::LabelType;
use crate::errors::Result;
use crate::layout::compound::{CEdge, CNode, CompoundResult, Dir};
use crate::layout::lanes::{LANE_PAD, lane_layout};
use crate::layout::layered::LayeredOptions;
use crate::preprocess::Config;
use crate::svg::builder::{label_svg, num};
use crate::svg::ink::ink_fill;
use crate::svg::theme::ResolvedStyle;
use crate::text::label::Label;
use crate::types::{Diagram, IconResolver, ParseLimits, RenderOptions, Rendered};

const DEFAULT_LANE: &str = "__swimlane_default__";

struct Swimlane;

fn sideways(dir: Dir) -> bool {
    matches!(dir, Dir::LR | Dir::RL)
}

impl FlowVariant for Swimlane {
    fn type_(&self) -> &str {
        "swimlane"
    }

    // どのレーンにも入らないノードは、Mermaid と同じく名前のない 1 つのレーンにまとめる。レーンはみな同じ向きに
    // 流れるので、subgraph の中の `direction` は効かない。
    fn prepare(&self, graph: &mut FlowGraph) {
        let mut loose = false;
        for node in &mut graph.nodes {
            if node.is_group {
                node.dir = None;
            } else if node.parent_id.is_none() {
                loose = true;
            }
        }
        if !loose {
            return;
        }
        for node in &mut graph.nodes {
            if !node.is_group && node.parent_id.is_none() {
                node.parent_id = Some(DEFAULT_LANE.to_string());
            }
        }
        graph.nodes.push(GraphNode {
            id: DEFAULT_LANE.to_string(),
            label: None,
            label_type: LabelType::Text,
            shape: "rect".to_string(),
            is_group: true,
            parent_id: None,
            css_styles: Vec::new(),
            css_compiled_styles: Vec::new(),
            css_classes: String::new(),
            dir: None,
            link: None,
            link_target: None,
            tooltip: None,
            icon: None,
            pos: None,
            img: None,
            asset_width: None,
            asset_height: None,
            constraint: None,
            color_index: None,
            metadata: None,
        });
    }

    fn layout(
        &self,
        nodes: &mut [CNode],
        edges: &mut [CEdge],
        dir: Dir,
        opt: &LayeredOptions,
        tight: bool,
    ) -> CompoundResult {
        lane_layout(nodes, edges, dir, opt, if tight { 12.0 } else { LANE_PAD })
    }

    // レーン: 図の長さの枠と、始まりの帯の中の題。
    fn draw_group(
        &self,
        c: &CNode,
        label: &Label,
        style: &ResolvedStyle,
        classes: &str,
        id: &str,
        dir: Dir,
        icons: Option<&IconResolver>,
    ) -> Option<String> {
        let left = c.x - c.w / 2.0;
        let top = c.y - c.h / 2.0;
        let side = sideways(dir);
        let cx = if side { left + c.pad_top / 2.0 } else { c.x };
        let cy = if side { c.y } else { top + c.pad_top / 2.0 };
        let attrs = format!(
            " class=\"pele-cluster-label\"{}{}",
            ink_fill(&style.shape, &style.text, Some("var(--_m)")),
            style.text
        );
        let text = label_svg(label, cx, cy, &attrs, icons);
        let mut out = format!(
            "<g class=\"pele-cluster pele-lane{classes}\" data-id=\"{id}\">\
             <rect x=\"{l}\" y=\"{t}\" width=\"{w}\" height=\"{h}\" fill=\"none\" stroke=\"var(--_b)\"/>\
             <rect x=\"{l}\" y=\"{t}\" width=\"{bw}\" height=\"{bh}\" fill=\"var(--_a)\" stroke=\"var(--_b)\"{shape}/>",
            l = num(left),
            t = num(top),
            w = num(c.w),
            h = num(c.h),
            bw = num(if side { c.pad_top } else { c.w }),
            bh = num(if side { c.h } else { c.pad_top }),
            shape = style.shape,
        );
        if side && !text.is_empty() {
            out.push_str(&format!(
                "<g transform=\"rotate(-90 {} {})\">{text}</g>",
                num(cx),
                num(cy)
            ));
        } else {
            out.push_str(&text);
        }
        out.push_str("</g>");
        Some(out)
    }

    fn filled_box(&self, c: &CNode, dir: Dir) -> Option<[f64; 4]> {
        let left = c.x - c.w / 2.0;
        let top = c.y - c.h / 2.0;
        Some(if sideways(dir) {
            [left, top, left + c.pad_top, top + c.h]
        } else {
            [left, top, left + c.w, top + c.pad_top]
        })
    }
}

fn parse(
    source: &str,
    config: &Config,
    title: Option<&str>,
    line_offset: usize,
    limits: &ParseLimits,
) -> Result<Box<dyn Any>> {
    (super::flowchart::FLOWCHART.parse)(source, config, title, line_offset, limits)
}

fn render(model: &dyn Any, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let db = model.downcast_ref::<FlowDb>().expect("swimlane model");
    render_flowchart(db, config, options, Some(&Swimlane))
}

/// swimlane は、上の段の subgraph をレーンにした flowchart。
pub static SWIMLANE: Diagram = Diagram {
    type_: "swimlane",
    section: None,
    keep_comments: false,
    math: true,
    parse,
    render,
};
