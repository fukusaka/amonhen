//! wardley を描く（pele の `src/diagrams/wardley/render.ts`）。

use std::collections::{HashMap, HashSet};

use super::builder::{Flow, SourceStrategy};
use super::model::WardleyModel;
use crate::diagrams::common::fit_width::fit_width;
use crate::errors::Result;
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num, shared_style, span_style, text_style};
use crate::svg::edges::{marker, marker_trim};
use crate::svg::root::{Accessible, svg_document};
use crate::svg::theme::ROUNDED;
use crate::text::label::{Label, layout_label};
use crate::text::measurer::{TextMeasurer, default_measurer, style as st};
use crate::types::{RenderOptions, Rendered};
use crate::util::fdlibm;
use crate::util::yaml::Value;

const DEFAULT_STAGES: [&str; 4] = ["Genesis", "Custom Built", "Product", "Commodity"];
/// Mermaid の画布は図のまわりにこれだけの余白を持つ。
const CANVAS_MARGIN: f64 = 48.0;
const R: f64 = 6.0;
const SQUARE: f64 = 10.0;
const LABEL_OFFSET: f64 = 8.0;
const PIPELINE_PAD: f64 = 15.0;
const FORCE_WIDTH: f64 = 60.0;
const FORCE_HEIGHT: f64 = 30.0;
const FORCE_HEAD: f64 = 20.0;
const ACCENT: &str = "var(--_c)";
const LINE: &str = "var(--_l)";

#[derive(Clone, Copy)]
struct Placed {
    x: f64,
    y: f64,
    /// `model.built.nodes` の番号。
    node: usize,
}

fn section(config: &Config) -> Option<&crate::util::yaml::Map> {
    match config.get("wardley-beta") {
        Some(Value::Obj(m)) => Some(m),
        _ => None,
    }
}

fn setting(config: &Config, key: &str, fallback: f64, min: f64, max: f64) -> f64 {
    match section(config).and_then(|s| s.get(key)) {
        Some(Value::Num(v)) if *v >= min && *v <= max => *v,
        _ => fallback,
    }
}

fn clamp(value: f64, min: f64, max: f64) -> f64 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// 行を x から始め、最初の基線を y に置いてラベルを書く。
fn start_label(label: &Label, x: f64, y: f64, attrs: &str) -> String {
    let shared = shared_style(label);
    let mut body = String::new();
    let mut baseline = y;
    for line in &label.lines {
        let mut first = true;
        for span in line {
            if span.icon.is_some() {
                continue;
            }
            let style = span_style(span.style & !shared);
            let pos = if first {
                format!(" x=\"{}\" y=\"{}\"", num(x), num(baseline))
            } else {
                String::new()
            };
            body.push_str(&format!("<tspan{pos}{style}>{}</tspan>", esc(&span.text)));
            first = false;
        }
        baseline += label.line_height;
    }
    if body.is_empty() {
        String::new()
    } else {
        format!(
            "<text{} xml:space=\"preserve\">{body}</text>",
            text_style(attrs, shared)
        )
    }
}

fn dimension(config: &Config, given: Option<f64>, key: &str, fallback: f64) -> f64 {
    let value = match given {
        Some(g) if g.is_finite() => g,
        _ => setting(config, key, fallback, 1.0, 1e6),
    };
    clamp(value, 200.0, 10000.0) - 2.0 * CANVAS_MARGIN
}

pub fn render_wardley(
    model: &WardleyModel,
    config: &Config,
    options: &RenderOptions,
) -> Result<Rendered> {
    let natural = dimension(config, model.built.size.map(|s| s.0), "width", 900.0);
    fit_width(options, natural, 160.0, |plot_w, tight| {
        Ok(draw(model, config, options, plot_w, natural, tight))
    })
}

struct Bounds {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl Bounds {
    fn grow(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) {
        if x0 < self.min_x {
            self.min_x = x0;
        }
        if y0 < self.min_y {
            self.min_y = y0;
        }
        if x1 > self.max_x {
            self.max_x = x1;
        }
        if y1 > self.max_y {
            self.max_y = y1;
        }
    }
}

fn draw(
    model: &WardleyModel,
    config: &Config,
    options: &RenderOptions,
    plot_w: f64,
    natural_w: f64,
    tight: bool,
) -> Rendered {
    let built = &model.built;
    let size = options.font_size.unwrap_or(16.0);
    let small = js::round(size * 0.8125);
    let tiny = js::round(size * 0.6875);
    let measurer: &dyn TextMeasurer = match &options.measurer {
        Some(m) => m.as_ref(),
        None => default_measurer(),
    };
    let pad = options.padding.unwrap_or(8.0);
    let icons = options.icons.as_ref();

    // 狭い図はそれほど低くしない。部品の間に少し場所が残るように。
    let plot_h = dimension(config, built.size.map(|s| s.1), "height", 600.0)
        * js::max(0.75, plot_w / natural_w);
    let show_grid = section(config).and_then(|s| s.get("showGrid")) == Some(&Value::Bool(true));

    let mut b = Bounds {
        min_x: 0.0,
        min_y: 0.0,
        max_x: plot_w,
        max_y: plot_h,
    };
    let centered = |b: &mut Bounds, label: &Label, cx: f64, cy: f64, attrs: &str| -> String {
        b.grow(
            cx - label.width / 2.0,
            cy - label.height / 2.0,
            cx + label.width / 2.0,
            cy + label.height / 2.0,
        );
        label_svg(label, cx, cy, attrs, icons)
    };
    let starting = |b: &mut Bounds, label: &Label, x: f64, y: f64, attrs: &str| -> String {
        b.grow(
            x,
            y - label.size,
            x + label.width,
            y - label.size + label.height + 4.0,
        );
        start_label(label, x, y, attrs)
    };
    let default_width = if tight {
        js::max(64.0, js::min(120.0, plot_w / 2.0))
    } else {
        4000.0
    };
    let text = |raw: &str, font_size: f64, max_width: Option<f64>, style: u32| -> Label {
        layout_label(
            Some(raw),
            false,
            measurer,
            font_size,
            max_width.unwrap_or(default_width),
            style,
        )
    };
    let project_x = |value: f64| (value / 100.0) * plot_w;
    let project_y = |value: f64| plot_h - (value / 100.0) * plot_h;

    let stages: Vec<&str> = match &built.axes.stages {
        Some(s) if !s.is_empty() => s.iter().map(String::as_str).collect(),
        _ => DEFAULT_STAGES.to_vec(),
    };
    let boundaries = built.axes.stage_boundaries.as_ref();
    let custom = boundaries.is_some_and(|b| b.len() == stages.len());
    let mut frame = String::new();
    let mut stage_labels = String::new();
    let mut stage_bottom = plot_h;
    let mut previous = 0.0;
    let count = stages.len() as f64;
    for (index, stage) in stages.iter().enumerate() {
        let start = if custom {
            previous
        } else {
            index as f64 / count
        };
        let end = if custom {
            clamp(boundaries.unwrap()[index], 0.0, 1.0)
        } else {
            (index + 1) as f64 / count
        };
        previous = end;
        if index > 0 {
            frame.push_str(&format!(
                "<path class=\"pele-wardley-stage\" d=\"M{},0V{}\" stroke=\"var(--_b)\" stroke-dasharray=\"4 4\"/>",
                num(start * plot_w),
                num(plot_h)
            ));
        }
        let label = text(
            stage,
            small,
            Some(js::max(48.0, (end - start).abs() * plot_w - 12.0)),
            0,
        );
        stage_labels.push_str(&centered(
            &mut b,
            &label,
            ((start + end) / 2.0) * plot_w,
            plot_h + 8.0 + label.height / 2.0,
            &format!(
                " class=\"pele-wardley-stage-label\" font-size=\"{}\" fill=\"var(--_m)\"",
                js::fmt(small)
            ),
        ));
        stage_bottom = js::max(stage_bottom, plot_h + 8.0 + label.height);
    }
    if show_grid {
        for i in 1..4 {
            let i = i as f64;
            frame.push_str(&format!(
                "<path class=\"pele-wardley-grid\" d=\"M{},0V{}M0,{}H{}\" stroke=\"var(--_b)\" stroke-dasharray=\"2 6\"/>",
                num((plot_w * i) / 4.0),
                num(plot_h),
                num((plot_h * i) / 4.0),
                num(plot_w)
            ));
        }
    }
    frame.push_str(&format!(
        "<path class=\"pele-wardley-axis\" d=\"M0,0V{}H{}\" stroke=\"{LINE}\"/>",
        num(plot_h),
        num(plot_w)
    ));
    let x_title = text(
        built.axes.x_label.as_deref().unwrap_or("Evolution"),
        small,
        None,
        0,
    );
    let y_title = text(
        built.axes.y_label.as_deref().unwrap_or("Visibility"),
        small,
        None,
        0,
    );
    let axis_attrs = format!(
        " class=\"pele-wardley-axis-label\" font-size=\"{}\" font-weight=\"var(--_hw)\"",
        js::fmt(small)
    );
    let axis_labels = centered(
        &mut b,
        &x_title,
        plot_w / 2.0,
        stage_bottom + 6.0 + x_title.height / 2.0,
        &axis_attrs,
    ) + &label_svg(
        &y_title,
        -16.0,
        plot_h / 2.0,
        &format!(
            "{axis_attrs} transform=\"rotate(-90 -16 {})\"",
            num(plot_h / 2.0)
        ),
        None,
    );
    b.grow(-16.0 - y_title.height / 2.0, 0.0, 0.0, 0.0);
    let legend_top = stage_bottom + 6.0 + x_title.height + 10.0;

    let mut positions: Vec<Placed> = Vec::new();
    let mut position_of: HashMap<&str, usize> = HashMap::new();
    for (k, node) in built.nodes.iter().enumerate() {
        position_of.insert(&node.id, positions.len());
        positions.push(Placed {
            x: project_x(node.x.unwrap()),
            y: project_y(node.y.unwrap()),
            node: k,
        });
    }

    // パイプライン: 部品を囲む箱と、その上の辺の上の四角としての親。
    let mut pipelines = String::new();
    let mut members: HashMap<&str, HashSet<&str>> = HashMap::new();
    for pipeline in &built.pipelines {
        members.insert(
            &pipeline.node_id,
            pipeline.component_ids.iter().map(String::as_str).collect(),
        );
        let mut parts: Vec<Placed> = Vec::new();
        for id in &pipeline.component_ids {
            if let Some(&k) = position_of.get(id.as_str()) {
                parts.push(positions[k]);
            }
        }
        if parts.is_empty() {
            continue;
        }
        parts.sort_by(|a, b| js::order(a.x - b.x));
        let left = parts[0].x;
        let right = parts[parts.len() - 1].x;
        let y = parts[0].y;
        let top = y - 2.0 * R;
        if let Some(&k) = position_of.get(pipeline.node_id.as_str()) {
            positions[k].x = (left + right) / 2.0;
            positions[k].y = top - SQUARE / 6.0;
        }
        b.grow(
            left - PIPELINE_PAD,
            top,
            right + PIPELINE_PAD,
            top + 4.0 * R,
        );
        pipelines.push_str(&format!(
            "<g class=\"pele-wardley-pipeline\" data-id=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_s)\" stroke=\"var(--_b)\"{ROUNDED}/>{}</g>",
            esc_text(&pipeline.node_id),
            num(left - PIPELINE_PAD),
            num(top),
            num(right - left + 2.0 * PIPELINE_PAD),
            num(4.0 * R),
            if parts.len() > 1 {
                format!(
                    "<path d=\"M{},{}H{}\" stroke=\"{LINE}\" stroke-dasharray=\"4 4\"/>",
                    num(left),
                    num(y),
                    num(right)
                )
            } else {
                String::new()
            }
        ));
    }

    // ノードの中心から、そこへの線が止まるまで。
    let anchor_width = if tight {
        js::max(64.0, plot_w / 2.0)
    } else {
        4000.0
    };
    let mut anchor_labels: HashMap<usize, Label> = HashMap::new();
    let reach = |anchor_labels: &mut HashMap<usize, Label>, at: &Placed, ux: f64, uy: f64| -> f64 {
        let node = &built.nodes[at.node];
        if node.class_name.as_deref() == Some("anchor") {
            let label = anchor_labels
                .entry(at.node)
                .or_insert_with(|| text(&node.label, small, Some(anchor_width), st::BOLD));
            return 1.0 / js::hypot(ux / (label.width / 2.0 + 5.0), uy / (small * 0.8));
        }
        if node.is_pipeline_parent {
            return SQUARE / std::f64::consts::SQRT_2;
        }
        if node.source_strategy.is_some() {
            2.0 * R
        } else {
            R
        }
    };

    // 線が通るノードのまわりの象限: 1 は右上、2 は右下、4 は左上、8 は左下。
    let mut taken: HashMap<usize, u32> = HashMap::new();
    let mut occupy = |at: usize, ux: f64, uy: f64| {
        if ux.abs() < 0.25 || uy.abs() < 0.25 {
            return;
        }
        let bit = if ux > 0.0 {
            if uy < 0.0 { 1 } else { 2 }
        } else if uy < 0.0 {
            4
        } else {
            8
        };
        *taken.entry(at).or_insert(0) |= bit;
    };

    let mut links = String::new();
    for link in &built.links {
        let (Some(&fk), Some(&tk)) = (
            position_of.get(link.source.as_str()),
            position_of.get(link.target.as_str()),
        ) else {
            continue;
        };
        if fk == tk {
            continue;
        }
        // パイプラインの部品はすでに親のもの。
        if members
            .get(link.target.as_str())
            .is_some_and(|m| m.contains(link.source.as_str()))
        {
            continue;
        }
        let from = positions[fk];
        let to = positions[tk];
        let dx = to.x - from.x;
        let dy = to.y - from.y;
        let distance = js::hypot(dx, dy);
        if distance < 0.01 {
            continue;
        }
        let ux = dx / distance;
        let uy = dy / distance;
        let start = reach(&mut anchor_labels, &from, ux, uy);
        let end = reach(&mut anchor_labels, &to, ux, uy);
        if start + end >= distance {
            continue;
        }
        occupy(fk, ux, uy);
        occupy(tk, -ux, -uy);
        let mut x1 = from.x + ux * start;
        let mut y1 = from.y + uy * start;
        let mut x2 = to.x - ux * end;
        let mut y2 = to.y - uy * end;
        let mut heads = String::new();
        let trim = marker_trim("arrow_point");
        if matches!(link.flow, Some(Flow::Forward | Flow::Bidirectional)) {
            heads.push_str(&marker("arrow_point", x2, y2, ux, uy, LINE, "none"));
            x2 -= ux * trim;
            y2 -= uy * trim;
        }
        if matches!(link.flow, Some(Flow::Backward | Flow::Bidirectional)) {
            heads.push_str(&marker("arrow_point", x1, y1, -ux, -uy, LINE, "none"));
            x1 += ux * trim;
            y1 += uy * trim;
        }
        let mut caption = String::new();
        if let Some(l) = link.label.as_deref().filter(|l| !l.is_empty()) {
            let label = text(l, tiny + 1.0, Some(200.0), 0);
            // 線の真ん中の横、その上の側に。
            let side = if ux >= 0.0 { 1.0 } else { -1.0 };
            let px = uy * side;
            let py = -ux * side;
            let away = 5.0 + (px.abs() * label.width) / 2.0 + (py.abs() * label.height) / 2.0;
            caption = centered(
                &mut b,
                &label,
                (from.x + to.x) / 2.0 + px * away,
                (from.y + to.y) / 2.0 + py * away,
                &format!(
                    " class=\"pele-edge-label\" font-size=\"{}\" fill=\"var(--_m)\"",
                    js::fmt(tiny + 1.0)
                ),
            );
        }
        links.push_str(&format!(
            "<g class=\"pele-edge{}\" data-id=\"{}-{}\"><path d=\"M{},{}L{},{}\" stroke=\"{LINE}\"{}/>{heads}{caption}</g>",
            if link.dashed { " pele-wardley-dashed" } else { "" },
            esc_text(&link.source),
            esc_text(&link.target),
            num(x1),
            num(y1),
            num(x2),
            num(y2),
            if link.dashed { " stroke-dasharray=\"6 6\"" } else { "" }
        ));
    }

    // 進化: 部品が向かう所への点線の矢印と、そこの白い丸。
    let mut trends = String::new();
    for trend in &built.trends {
        let Some(&k) = position_of.get(trend.node_id.as_str()) else {
            continue;
        };
        let origin = positions[k];
        let tx = project_x(trend.target_x);
        let ty = project_y(trend.target_y);
        let dx = tx - origin.x;
        let dy = ty - origin.y;
        let distance = js::hypot(dx, dy);
        let mut arrow = String::new();
        let start = if distance > 0.0 {
            reach(&mut anchor_labels, &origin, dx / distance, dy / distance)
        } else {
            0.0
        };
        if distance > start + R + 4.0 {
            let ux = dx / distance;
            let uy = dy / distance;
            let ex = tx - ux * (R + 2.0);
            let ey = ty - uy * (R + 2.0);
            let trim = marker_trim("arrow_point");
            arrow = format!(
                "<path d=\"M{},{}L{},{}\" stroke=\"{ACCENT}\" stroke-dasharray=\"4 4\"/>{}",
                num(origin.x + ux * start),
                num(origin.y + uy * start),
                num(ex - ux * trim),
                num(ey - uy * trim),
                marker("arrow_point", ex, ey, ux, uy, ACCENT, "none")
            );
        }
        b.grow(tx - R, ty - R, tx + R, ty + R);
        trends.push_str(&format!(
            "<g class=\"pele-wardley-trend\" data-id=\"{}\">{arrow}<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"var(--_bg)\" stroke=\"{ACCENT}\"/></g>",
            esc_text(&trend.node_id),
            num(tx),
            num(ty),
            js::fmt(R)
        ));
    }

    let mut nodes = String::new();
    let tri = 1.2 * R;
    let dot = 0.7 * R;
    for (k, at) in positions.iter().enumerate() {
        let node = &built.nodes[at.node];
        let (x, y) = (at.x, at.y);
        let anchor = node.class_name.as_deref() == Some("anchor");
        let strategy = node.source_strategy;
        let mut shape = String::new();
        if let Some(s) = strategy {
            let fill = match s {
                SourceStrategy::Outsource => "var(--_m)",
                SourceStrategy::Buy => "var(--_a)",
                SourceStrategy::Build => "var(--_s)",
                SourceStrategy::Market => "var(--_bg)",
            };
            shape.push_str(&format!(
                "<circle class=\"pele-wardley-{}\" cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{fill}\" stroke=\"{LINE}\"/>",
                s.name(),
                num(x),
                num(y),
                js::fmt(2.0 * R)
            ));
        }
        if strategy == Some(SourceStrategy::Market) {
            let bx = tri * fdlibm::cos(std::f64::consts::PI / 6.0);
            let by = y + tri * fdlibm::sin(std::f64::consts::PI / 6.0);
            shape.push_str(&format!(
                "<path d=\"M{},{}L{},{}H{}Z\" fill=\"none\" stroke=\"{LINE}\"/>",
                num(x),
                num(y - tri),
                num(x - bx),
                num(by),
                num(x + bx)
            ));
            for (cx, cy) in [(x, y - tri), (x - bx, by), (x + bx, by)] {
                shape.push_str(&format!(
                    "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"var(--_bg)\" stroke=\"{LINE}\"/>",
                    num(cx),
                    num(cy),
                    num(dot)
                ));
            }
        } else if node.is_pipeline_parent {
            shape.push_str(&format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_bg)\" stroke=\"{LINE}\"/>",
                num(x - SQUARE / 2.0),
                num(y - SQUARE / 2.0),
                js::fmt(SQUARE),
                js::fmt(SQUARE)
            ));
        } else if !anchor {
            shape.push_str(&format!(
                "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"var(--_bg)\" stroke=\"{LINE}\"/>",
                num(x),
                num(y),
                js::fmt(R)
            ));
        }
        if node.inertia == Some(true) {
            let offset = (if node.is_pipeline_parent {
                SQUARE / 2.0
            } else {
                R
            }) + 10.0
                + if strategy.is_some() { R + 4.0 } else { 0.0 };
            shape.push_str(&format!(
                "<rect class=\"pele-wardley-inertia\" x=\"{}\" y=\"{}\" width=\"4\" height=\"{}\" fill=\"{LINE}\"/>",
                num(x + offset),
                num(y - R),
                js::fmt(2.0 * R)
            ));
        }
        let ox = node.label_offset_x.map(|v| clamp(v, -5000.0, 5000.0));
        let oy = node.label_offset_y.map(|v| clamp(v, -5000.0, 5000.0));
        let caption = if anchor {
            let label = match anchor_labels.get(&at.node) {
                Some(l) => l.clone(),
                None => text(&node.label, small, Some(anchor_width), st::BOLD),
            };
            centered(
                &mut b,
                &label,
                x + ox.unwrap_or(0.0),
                y + oy.unwrap_or(-3.0),
                &format!(
                    " class=\"pele-label\" font-size=\"{}\" font-weight=\"var(--_hw)\"",
                    js::fmt(small)
                ),
            )
        } else {
            let label = text(&node.label, small, None, 0);
            let attrs = format!(" class=\"pele-label\" font-size=\"{}\"", js::fmt(small));
            let lift = LABEL_OFFSET + if strategy.is_some() { 10.0 } else { 0.0 };
            if ox.is_some() || oy.is_some() {
                starting(
                    &mut b,
                    &label,
                    x + ox.unwrap_or(lift),
                    y + oy.unwrap_or(-lift),
                    &attrs,
                )
            } else if node.in_pipeline {
                centered(
                    &mut b,
                    &label,
                    x,
                    y + 2.0 * R + 4.0 + label.height / 2.0,
                    &attrs,
                )
            } else {
                // ラベルは右上へ。線がそちらへ出ていれば、次の空いた角へ。
                let busy = taken.get(&k).copied().unwrap_or(0);
                let corner = if busy & 1 == 0 {
                    1
                } else if busy & 2 == 0 {
                    2
                } else if busy & 4 == 0 {
                    4
                } else if busy & 8 == 0 {
                    8
                } else {
                    1
                };
                let right = corner == 1 || corner == 2;
                let up = corner == 1 || corner == 4;
                starting(
                    &mut b,
                    &label,
                    if right {
                        x + lift
                    } else {
                        x - lift - label.width
                    },
                    if up {
                        y - lift
                    } else {
                        y + lift + small * 0.75
                    },
                    &attrs,
                )
            }
        };
        b.grow(x - 2.0 * R, y - 2.0 * R, x + 2.0 * R + 20.0, y + 2.0 * R);
        let class = if anchor {
            "anchor"
        } else if node.is_pipeline_parent {
            "pipeline-parent"
        } else if node.in_pipeline {
            "pipeline-component"
        } else {
            "component"
        };
        nodes.push_str(&format!(
            "<g class=\"pele-node pele-wardley-{class}\" data-id=\"{}\">{shape}{caption}</g>",
            esc_text(&node.id)
        ));
    }

    let mut annotations = String::new();
    for annotation in &built.annotations {
        let number = js::fmt(annotation.number);
        let label = text(&number, tiny, Some(4000.0), st::BOLD);
        let mut marks = String::new();
        for &(cx, cy) in &annotation.coordinates {
            let x = project_x(cx);
            let y = project_y(cy);
            let r = js::max(10.0, label.width / 2.0 + 4.0);
            b.grow(x - r, y - r, x + r, y + r);
            marks.push_str(&format!(
                "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"var(--_bg)\" stroke=\"{LINE}\"/>{}",
                num(x),
                num(y),
                num(r),
                label_svg(
                    &label,
                    x,
                    y,
                    &format!(
                        " font-size=\"{}\" font-weight=\"var(--_hw)\"",
                        js::fmt(tiny)
                    ),
                    None
                )
            ));
        }
        annotations.push_str(&format!(
            "<g class=\"pele-wardley-annotation\" data-id=\"{}\">{marks}</g>",
            esc_text(&number)
        ));
    }
    let mut listed: Vec<_> = built
        .annotations
        .iter()
        .filter(|a| a.text.as_deref().is_some_and(|t| !t.is_empty()))
        .collect();
    listed.sort_by(|a, b| js::order(a.number - b.number));
    if !listed.is_empty() {
        let lines: Vec<Label> = listed
            .iter()
            .map(|a| {
                text(
                    &format!("{}. {}", js::fmt(a.number), a.text.as_deref().unwrap_or("")),
                    small,
                    Some(if tight {
                        js::max(80.0, plot_w - 20.0)
                    } else {
                        320.0
                    }),
                    0,
                )
            })
            .collect();
        let mut width = 0.0;
        let mut height = 0.0;
        for line in &lines {
            width = js::max(width, line.width);
            height += line.height;
        }
        width += 20.0;
        height += 14.0;
        // 置き場所がなければ、何も隠さない図の下に並べる。
        let bx = match built.annotations_box {
            Some((x, _)) => clamp(project_x(x), 0.0, js::max(0.0, plot_w - width)),
            None => 0.0,
        };
        let by = match built.annotations_box {
            Some((_, y)) => clamp(project_y(y), 0.0, js::max(0.0, plot_h - height)),
            None => legend_top,
        };
        b.grow(bx, by, bx + width, by + height);
        let mut rows = String::new();
        let mut y = by + 7.0;
        for line in &lines {
            rows.push_str(&start_label(
                line,
                bx + 10.0,
                y + line.line_height / 2.0 + small * 0.35,
                &format!(" font-size=\"{}\"", js::fmt(small)),
            ));
            y += line.height;
        }
        annotations.push_str(&format!(
            "<g class=\"pele-wardley-annotations\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_bg)\" stroke=\"var(--_b)\"{ROUNDED}/>{rows}</g>",
            num(bx),
            num(by),
            num(width),
            num(height)
        ));
    }

    let mut notes = String::new();
    for note in &built.notes {
        let label = text(
            &note.text,
            small,
            Some(js::min(360.0, plot_w * 0.6)),
            st::BOLD,
        );
        notes.push_str(&starting(
            &mut b,
            &label,
            project_x(note.x),
            project_y(note.y),
            &format!(
                " class=\"pele-wardley-note\" font-size=\"{}\" font-weight=\"var(--_hw)\"",
                js::fmt(small)
            ),
        ));
    }

    // 加速は右を、減速は左を指す。
    let mut forces = String::new();
    let mut force = |b: &mut Bounds, name: &str, px: f64, py: f64, forward: bool| {
        let x = project_x(px);
        let y = project_y(py);
        let half = FORCE_HEIGHT / 2.0;
        let tip = if forward { x + FORCE_WIDTH } else { x };
        let tail = if forward { x } else { x + FORCE_WIDTH };
        let neck = if forward {
            x + FORCE_WIDTH - FORCE_HEAD
        } else {
            x + FORCE_HEAD
        };
        let d = format!(
            "M{},{}H{}V{}L{},{}L{},{}V{}H{}Z",
            num(tail),
            num(y - half),
            num(neck),
            num(y - half - 8.0),
            num(tip),
            num(y),
            num(neck),
            num(y + half + 8.0),
            num(y + half),
            num(tail)
        );
        b.grow(x, y - half - 8.0, x + FORCE_WIDTH, y + half + 8.0);
        let label = text(name, tiny + 1.0, Some(160.0), st::BOLD);
        let caption = centered(
            b,
            &label,
            x + FORCE_WIDTH / 2.0,
            y + half + 8.0 + 12.0,
            &format!(
                " font-size=\"{}\" font-weight=\"var(--_hw)\"",
                js::fmt(tiny + 1.0)
            ),
        );
        forces.push_str(&format!(
            "<g class=\"pele-wardley-{}\" data-id=\"{}\"><path d=\"{d}\" fill=\"var(--_s)\" stroke=\"{LINE}\"/>{caption}</g>",
            if forward { "accelerator" } else { "deaccelerator" },
            esc_text(name)
        ));
    };
    for a in &built.accelerators {
        force(&mut b, &a.name, a.x, a.y, true);
    }
    for d in &built.deaccelerators {
        force(&mut b, &d.name, d.x, d.y, false);
    }

    let title = layout_label(
        model.title.as_deref(),
        false,
        measurer,
        size,
        if tight { js::max(80.0, plot_w) } else { 4000.0 },
        st::BOLD,
    );
    let title_height = if title.height > 0.0 {
        title.height + 12.0
    } else {
        0.0
    };
    let left = js::min(b.min_x, plot_w / 2.0 - title.width / 2.0);
    let right = js::max(b.max_x, plot_w / 2.0 + title.width / 2.0);
    let tx = pad - left;
    let ty = pad + title_height - b.min_y;
    let total_width = (right - left + 2.0 * pad).ceil();
    let total_height = (b.max_y - b.min_y + title_height + 2.0 * pad).ceil();
    let mut content = label_svg(
        &title,
        tx + plot_w / 2.0,
        pad + title.height / 2.0,
        " class=\"pele-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    content.push_str(&format!(
        "<g transform=\"translate({},{})\"><g class=\"pele-wardley-frame\" fill=\"none\">{frame}</g>{stage_labels}{axis_labels}{pipelines}",
        num(tx),
        num(ty)
    ));
    if !links.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-edges\" fill=\"none\">{links}</g>"
        ));
    }
    if !trends.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-wardley-trends\" fill=\"none\">{trends}</g>"
        ));
    }
    content.push_str(&nodes);
    content.push_str(&annotations);
    content.push_str(&notes);
    content.push_str(&forces);
    content.push_str("</g>");

    Rendered {
        svg: svg_document(
            "wardley",
            total_width,
            total_height,
            size,
            options,
            &Accessible {
                acc_title: model.acc_title.as_deref(),
                acc_descr: model.acc_descr.as_deref(),
            },
            &content,
        ),
        width: total_width,
        height: total_height,
        links: Vec::new(),
    }
}
