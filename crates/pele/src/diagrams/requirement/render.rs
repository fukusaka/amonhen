//! requirement を描く（pele の `src/diagrams/requirement/render.ts`）。

use std::collections::HashMap;

use super::db::RequirementDb;
use super::graph::{GraphEdge, GraphNode, build_requirement_graph};
use crate::diagrams::common::fit_width::{runs_across, turn_to_fit};
use crate::errors::Result;
use crate::js;
use crate::layout::compound::{CEdge, CNode, Dir, cnode, compound_layout, direction};
use crate::layout::layered::LayeredOptions;
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num, shared_style, span_style, text_style};
use crate::svg::edges::{EdgePath, edge_label_svg, route_path};
use crate::svg::ink::ink_fill;
use crate::svg::root::{Accessible, svg_document};
use crate::svg::theme::{ResolvedStyle, class_names, resolve_style, rounded};
use crate::svg::title::title_label;
use crate::text::label::{Label, has_math, label_line, layout_label, line_middle};
use crate::text::measurer::{TextMeasurer, default_measurer, style as st};
use crate::types::{RenderOptions, Rendered};

const NODE_SEP: f64 = 40.0;
const EDGE_SEP: f64 = 16.0;
const RANK_SEP: f64 = 56.0;
const PAD_X: f64 = 14.0;
const PAD_Y: f64 = 10.0;
const ROW_GAP: f64 = 2.0;
const COLUMN_GAP: f64 = 12.0;
const MIN_WIDTH: f64 = 120.0;
const NAME_WRAP: f64 = 280.0;
const VALUE_WRAP: f64 = 240.0;
const LOOP: f64 = 26.0;
const CONTAINS_RADIUS: f64 = 6.0;
const ARROW_LENGTH: f64 = 9.0;
const ARROW_HALF: f64 = 4.5;
const END_STEP: f64 = 28.0;
const END_MARGIN: f64 = 14.0;

struct Row {
    key: Label,
    value: Label,
    height: f64,
}

struct NodeView<'a> {
    node: &'a GraphNode<'a>,
    style: ResolvedStyle,
    kind: Label,
    name: Label,
    rows: Vec<Row>,
    key_width: f64,
    head_height: f64,
    w: f64,
    h: f64,
}

/// markdown を読んだ後で、ラベル全体に太字や斜体を足す。強調の印は、ラベルが始めから持つ飾りを切り替えるだけ
/// なので。
fn restyle(label: Label, bits: u32, measurer: &dyn TextMeasurer) -> Label {
    if bits == 0 || label.lines.is_empty() {
        return label;
    }
    let mut width = 0.0;
    let mut widths = Vec::new();
    let size = label.size;
    let lines: Vec<_> = label
        .lines
        .iter()
        .map(|line| {
            let mut total = 0.0;
            let spans: Vec<_> = line
                .iter()
                .map(|span| {
                    let style = span.style | bits;
                    let w = if span.icon.is_some() || span.math.is_some() {
                        span.width
                    } else {
                        measurer.width(&span.text, size, style)
                    };
                    total += w;
                    let mut s = span.clone();
                    s.style = style;
                    s.width = w;
                    s
                })
                .collect();
            widths.push(total);
            if total > width {
                width = total;
            }
            spans
        })
        .collect();
    Label {
        lines,
        widths,
        width,
        ..label
    }
}

/// 測ったラベルを、行の左を x に、最初の行の箱の上を top にして書く。
fn start_text(label: &Label, x: f64, top: f64, attrs: &str) -> String {
    let shared = shared_style(label);
    let mut body = String::new();
    let mut math = String::new();
    for (i, line) in label.lines.iter().enumerate() {
        let middle = top + line_middle(label, i);
        if has_math(line) {
            math.push_str(&label_svg(
                &label_line(label, i),
                x + label.widths[i] / 2.0,
                middle,
                attrs,
                None,
            ));
            continue;
        }
        let y = middle + label.size * 0.35;
        let mut first = true;
        for span in line {
            if span.icon.is_some() || span.text.is_empty() {
                continue;
            }
            body.push_str("<tspan");
            if first {
                body.push_str(&format!(" x=\"{}\" y=\"{}\"", num(x), num(y)));
            }
            body.push_str(&span_style(span.style & !shared));
            body.push('>');
            body.push_str(&esc(&span.text));
            body.push_str("</tspan>");
            first = false;
        }
    }
    let text = if body.is_empty() {
        String::new()
    } else {
        format!(
            "<text{} text-anchor=\"start\" xml:space=\"preserve\">{body}</text>",
            text_style(attrs, shared)
        )
    };
    text + &math
}

/// 包含の印: 縁が (x, y) に触れる丸の中の十字。(dx, dy) はノードを向く。
fn contains_marker(x: f64, y: f64, dx: f64, dy: f64) -> String {
    let r = CONTAINS_RADIUS;
    let cx = x - dx * r;
    let cy = y - dy * r;
    format!(
        "<circle class=\"pele-marker\" cx=\"{}\" cy=\"{}\" r=\"{}\"/><path class=\"pele-marker\" d=\"M{},{}L{},{}M{},{}L{},{}\"/>",
        num(cx),
        num(cy),
        js::fmt(r),
        num(cx - dx * r),
        num(cy - dy * r),
        num(cx + dx * r),
        num(cy + dy * r),
        num(cx + dy * r),
        num(cy - dx * r),
        num(cx - dy * r),
        num(cy + dx * r)
    )
}

/// 先が (x, y) にあり、(dx, dy) を向く開いた矢じり。
fn arrow_marker(x: f64, y: f64, dx: f64, dy: f64) -> String {
    let bx = x - dx * ARROW_LENGTH;
    let by = y - dy * ARROW_LENGTH;
    format!(
        "<path class=\"pele-marker\" d=\"M{},{}L{},{}L{},{}\" stroke-linejoin=\"round\"/>",
        num(bx - dy * ARROW_HALF),
        num(by + dx * ARROW_HALF),
        num(x),
        num(y),
        num(bx + dy * ARROW_HALF),
        num(by - dx * ARROW_HALF)
    )
}

struct End {
    edge: usize,
    at: usize,
    toward: f64,
}

/// 配置はどの辺もノードの辺の真ん中につなぐ。同じ辺に着く端は、向かう先の順にその辺に沿って広げ、印が重ならない
/// ようにする。
fn spread_ends(cnodes: &[CNode], cedges: &mut [CEdge], views: &[NodeView]) {
    let mut keys: Vec<usize> = Vec::new();
    let mut sides: HashMap<usize, Vec<End>> = HashMap::new();
    for (e, edge) in cedges.iter().enumerate() {
        let route = &edge.route;
        if edge.src == edge.dst || route.len() < 6 {
            continue;
        }
        for at in [0, route.len() - 3] {
            let node = if at == 0 { edge.src } else { edge.dst };
            let c = &cnodes[node];
            let dx = route[at] - c.x;
            let dy = route[at + 1] - c.y;
            let next = if at == 0 { 3 } else { at - 3 };
            let side = if (dy.abs() - c.h / 2.0).abs() < 0.5 {
                if dy < 0.0 { 0 } else { 2 }
            } else if (dx.abs() - c.w / 2.0).abs() < 0.5 {
                if dx < 0.0 { 3 } else { 1 }
            } else {
                continue;
            };
            let key = node * 4 + side;
            let end = End {
                edge: e,
                at,
                toward: route[if side % 2 == 0 { next } else { next + 1 }],
            };
            sides
                .entry(key)
                .or_insert_with(|| {
                    keys.push(key);
                    Vec::new()
                })
                .push(end);
        }
    }
    for key in keys {
        let list = sides.get_mut(&key).unwrap();
        if list.len() < 2 {
            continue;
        }
        let node = key >> 2;
        let vertical = key % 2 == 1;
        let span = (if vertical {
            views[node].h
        } else {
            views[node].w
        }) - 2.0 * END_MARGIN;
        let step = js::max(0.0, js::min(END_STEP, span / (list.len() - 1) as f64));
        list.sort_by(|a, b| js::order(a.toward - b.toward));
        let count = list.len();
        for (j, end) in list.iter().enumerate() {
            let offset = (j as f64 - (count - 1) as f64 / 2.0) * step;
            let route = &mut cedges[end.edge].route;
            if vertical {
                route[end.at + 1] = cnodes[node].y + offset;
            } else {
                route[end.at] = cnodes[node].x + offset;
            }
        }
    }
}

pub fn render_requirement(
    model: &RequirementDb,
    config: &Config,
    options: &RenderOptions,
) -> Result<Rendered> {
    if !runs_across(Some(&model.direction)) {
        return Ok(draw(model, config, options, false));
    }
    turn_to_fit(options, |down| Ok(draw(model, config, options, down)))
}

/// `replace(/^default\s?/, '')`
fn strip_default(raw: &str) -> &str {
    match raw.strip_prefix("default") {
        Some(rest) => match rest.chars().next() {
            Some(c) if crate::util::chars::is_space_char(c) => &rest[c.len_utf8()..],
            _ => rest,
        },
        None => raw,
    }
}

/// `turned` は横に流れる図を縦に流して描く。
fn draw(
    model: &RequirementDb,
    _config: &Config,
    options: &RenderOptions,
    turned: bool,
) -> Rendered {
    let graph = build_requirement_graph(model);
    let size = options.font_size.unwrap_or(16.0);
    let small = js::round(size * 0.875);
    let measurer: &dyn TextMeasurer = match &options.measurer {
        Some(m) => m.as_ref(),
        None => default_measurer(),
    };
    let pad = options.padding.unwrap_or(8.0);

    let mut index: HashMap<&str, usize> = HashMap::new();
    let mut views: Vec<NodeView> = Vec::new();
    let mut cnodes: Vec<CNode> = Vec::new();
    for node in &graph.nodes {
        let style = resolve_style(node.css_styles);
        let base =
            (if style.bold { st::BOLD } else { 0 }) | (if style.italic { st::ITALIC } else { 0 });
        let name_size = style.font_size.unwrap_or(size);
        let body_size = style.font_size.unwrap_or(small);
        let kind_text = format!(
            "«{}»",
            node.requirement.map_or("Element", |r| r.type_.as_str())
        );
        let kind = restyle(
            layout_label(Some(&kind_text), false, measurer, body_size, 4000.0, 0),
            base,
            measurer,
        );
        let name = restyle(
            layout_label(Some(node.id), true, measurer, name_size, NAME_WRAP, 0),
            base | st::BOLD,
            measurer,
        );
        let fields: Vec<(&str, &str)> = match (node.requirement, node.element) {
            (Some(r), _) => vec![
                ("ID", &r.requirement_id),
                ("Text", &r.text),
                ("Risk", &r.risk),
                ("Verification", &r.verify_method),
            ],
            (None, Some(e)) => vec![("Type", &e.type_), ("Doc Ref", &e.doc_ref)],
            (None, None) => Vec::new(),
        };
        let mut rows = Vec::new();
        let mut key_width: f64 = 0.0;
        let mut value_width: f64 = 0.0;
        let mut body_height = 0.0;
        for (label, text) in fields {
            if text.is_empty() {
                continue;
            }
            let key = restyle(
                layout_label(Some(label), false, measurer, body_size, 4000.0, 0),
                base,
                measurer,
            );
            let value = restyle(
                layout_label(Some(text), true, measurer, body_size, VALUE_WRAP, 0),
                base,
                measurer,
            );
            let height = js::max(key.height, value.height);
            key_width = js::max(key_width, key.width);
            value_width = js::max(value_width, value.width);
            body_height += height + ROW_GAP;
            rows.push(Row { key, value, height });
        }
        if !rows.is_empty() {
            body_height += 2.0 * PAD_Y - ROW_GAP;
        }
        let head_height = 2.0 * PAD_Y + kind.height + name.height;
        let w = js::max_of(&[
            MIN_WIDTH,
            js::max(kind.width, name.width) + 2.0 * PAD_X,
            if rows.is_empty() {
                0.0
            } else {
                key_width + COLUMN_GAP + value_width + 2.0 * PAD_X
            },
        ]);
        // 要求と要素は名前を分けあえる。そのときの関係は要求に向かう。
        index.entry(node.id).or_insert(views.len());
        let mut c = cnode(w, head_height + body_height, -1);
        c.seq = views.len() as f64;
        views.push(NodeView {
            node,
            style,
            kind,
            name,
            rows,
            key_width,
            head_height,
            w,
            h: head_height + body_height,
        });
        cnodes.push(c);
    }

    let root_dir = if turned {
        Dir::TB
    } else {
        direction(graph.direction)
    };
    let sideways = root_dir == Dir::LR || root_dir == Dir::RL;
    let mut cedges: Vec<CEdge> = Vec::new();
    let mut edge_labels: Vec<Label> = Vec::new();
    let mut drawn: Vec<&GraphEdge> = Vec::new();
    for edge in &graph.edges {
        let src = edge.start.and_then(|s| index.get(s).copied());
        let dst = edge.end.and_then(|s| index.get(s).copied());
        let (Some(src), Some(dst)) = (src, dst) else {
            continue;
        };
        let label = layout_label(
            Some(&format!("«{}»", edge.type_)),
            false,
            measurer,
            small,
            4000.0,
            0,
        );
        if src == dst {
            // 自分への輪は流れを横切ってノードの横に置き、ノードの箱はそれを収めるだけ大きくなる。
            if sideways {
                cnodes[src].h += 2.0 * (LOOP + label.height + 8.0);
            } else {
                cnodes[src].w += 2.0 * (LOOP + label.width + 12.0);
            }
        }
        drawn.push(edge);
        cedges.push(CEdge {
            src,
            dst,
            minlen: 1.0,
            label_w: label.width + 12.0,
            label_h: label.height + 4.0,
            route: Vec::new(),
            label_x: 0.0,
            label_y: 0.0,
        });
        edge_labels.push(label);
    }

    let layout = compound_layout(
        &mut cnodes,
        &mut cedges,
        root_dir,
        &LayeredOptions {
            node_sep: NODE_SEP,
            edge_sep: EDGE_SEP,
            rank_sep: RANK_SEP,
            port_sep: 20.0,
            ..Default::default()
        },
    );

    spread_ends(&cnodes, &mut cedges, &views);

    let title = title_label(model.title.as_deref(), measurer, size, 4000.0);
    let title_height = if title.height > 0.0 {
        title.height + 12.0
    } else {
        0.0
    };
    let inner = js::max(layout.width, title.width);
    let ox = pad + (inner - layout.width) / 2.0;
    let oy = pad + title_height;
    let width = (inner + 2.0 * pad).ceil();
    let height = (layout.height + title_height + 2.0 * pad).ceil();

    let mut nodes_out = String::new();
    for (i, view) in views.iter().enumerate() {
        let (w, h, style) = (view.w, view.h, &view.style);
        let color = ink_fill(&style.shape, &style.text, None);
        let muted = if color.is_empty() {
            " fill=\"var(--_m)\"".to_string()
        } else {
            color.clone()
        };
        let left = -w / 2.0;
        let top = -h / 2.0;
        let mut body = format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_s)\" stroke=\"var(--_b)\"{}/>",
            num(left),
            num(top),
            num(w),
            num(h),
            rounded(&style.shape)
        );
        body.push_str(&label_svg(
            &view.kind,
            0.0,
            top + PAD_Y + view.kind.height / 2.0,
            &format!(
                " class=\"pele-stereotype\" font-size=\"{}\"{muted}{}",
                js::fmt(view.kind.size),
                style.text
            ),
            None,
        ));
        body.push_str(&label_svg(
            &view.name,
            0.0,
            top + PAD_Y + view.kind.height + view.name.height / 2.0,
            &format!(
                " class=\"pele-label\" font-size=\"{}\" font-weight=\"var(--_hw)\"{color}{}",
                js::fmt(view.name.size),
                style.text
            ),
            None,
        ));
        if !view.rows.is_empty() {
            let divider = top + view.head_height;
            body.push_str(&format!(
                "<path class=\"pele-divider\" d=\"M{},{}H{}\" stroke=\"var(--_b)\"{}/>",
                num(left),
                num(divider),
                num(-left),
                style.line
            ));
            let mut y = divider + PAD_Y;
            let mut rows = String::new();
            for row in &view.rows {
                rows.push_str(&start_text(
                    &row.key,
                    left + PAD_X,
                    y,
                    &format!(" class=\"pele-field\"{muted}{}", style.text),
                ));
                rows.push_str(&start_text(
                    &row.value,
                    left + PAD_X + view.key_width + COLUMN_GAP,
                    y,
                    &format!(" class=\"pele-value\"{color}{}", style.text),
                ));
                y += row.height + ROW_GAP;
            }
            body.push_str(&format!(
                "<g class=\"pele-fields\" font-size=\"{}\">{rows}</g>",
                js::fmt(view.rows[0].key.size)
            ));
        }
        nodes_out.push_str(&format!(
            "<g class=\"pele-node {}{}\" data-id=\"{}\" transform=\"translate({},{})\">{body}</g>",
            if view.node.requirement.is_some() {
                "pele-requirement"
            } else {
                "pele-element"
            },
            class_names(strip_default(&view.node.css_classes)),
            esc_text(view.node.id),
            num(cnodes[i].x + ox),
            num(cnodes[i].y + oy)
        ));
    }

    let mut edges_out = String::new();
    let mut labels_out = String::new();
    let mut loop_seen: HashMap<usize, usize> = HashMap::new();
    for (i, ce) in cedges.iter().enumerate() {
        let edge = drawn[i];
        let label = &edge_labels[i];
        let start_trim = if edge.contains {
            2.0 * CONTAINS_RADIUS
        } else {
            0.0
        };
        let path: EdgePath;
        let mut label_x = ce.label_x + ox;
        let mut label_y = ce.label_y + oy;
        if ce.src == ce.dst {
            let view = &views[ce.src];
            let cx = cnodes[ce.src].x + ox;
            let cy = cnodes[ce.src].y + oy;
            let k = *loop_seen.get(&ce.src).unwrap_or(&0);
            loop_seen.insert(ce.src, k + 1);
            let half = (if sideways { view.w } else { view.h }) / 2.0;
            let spread = js::min(half - 4.0, 12.0 + k as f64 * 6.0);
            let reach = LOOP + k as f64 * 8.0;
            // 局所の向き: `out` はノードから離れる向き、`along` はその辺に沿う向き。
            let at = |out: f64, along: f64| -> String {
                if sideways {
                    format!("{},{}", num(cx + along), num(cy + view.h / 2.0 + out))
                } else {
                    format!("{},{}", num(cx + view.w / 2.0 + out), num(cy + along))
                }
            };
            // 制御点がこれだけ外にある 3 次曲線は `reach` で頂に達する。
            let pull = reach / 0.75;
            path = EdgePath {
                d: format!(
                    "M{}C{} {} {}",
                    at(start_trim, -spread),
                    at(pull, -spread),
                    at(pull, spread),
                    at(0.0, spread)
                ),
                tokens: None,
                sx: if sideways {
                    cx - spread
                } else {
                    cx + view.w / 2.0
                },
                sy: if sideways {
                    cy + view.h / 2.0
                } else {
                    cy - spread
                },
                sdx: if sideways { 0.0 } else { -1.0 },
                sdy: if sideways { -1.0 } else { 0.0 },
                ex: if sideways {
                    cx + spread
                } else {
                    cx + view.w / 2.0
                },
                ey: if sideways {
                    cy + view.h / 2.0
                } else {
                    cy + spread
                },
                edx: if sideways { 0.0 } else { -1.0 },
                edy: if sideways { -1.0 } else { 0.0 },
            };
            if sideways {
                label_x = cx;
                label_y = cy + view.h / 2.0 + reach + 4.0 + ce.label_h / 2.0;
            } else {
                label_x = cx + view.w / 2.0 + reach + 4.0 + ce.label_w / 2.0;
                label_y = cy;
            }
        } else {
            let mut route = ce.route.clone();
            let mut k = 0;
            while k + 1 < route.len() {
                route[k] += ox;
                route[k + 1] += oy;
                k += 3;
            }
            path = route_path(&route, None, start_trim, 0.0, None, None);
        }
        edges_out.push_str(&format!(
            "<g class=\"pele-edge pele-relationship-{}\" data-id=\"{}\"><path d=\"{}\"{}/>{}</g>",
            if edge.contains { "contains" } else { "arrow" },
            esc_text(&edge.id),
            path.d,
            if edge.contains {
                ""
            } else {
                " stroke-dasharray=\"5 4\""
            },
            if edge.contains {
                contains_marker(path.sx, path.sy, path.sdx, path.sdy)
            } else {
                arrow_marker(path.ex, path.ey, path.edx, path.edy)
            }
        ));
        labels_out.push_str(&edge_label_svg(&edge.id, label, label_x, label_y, "", None));
    }

    let mut content = label_svg(
        &title,
        width / 2.0,
        pad + title.height / 2.0,
        " class=\"pele-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    if !edges_out.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-edges\" fill=\"none\" stroke=\"var(--_l)\" stroke-linecap=\"round\">{edges_out}</g>"
        ));
    }
    if !labels_out.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-edge-labels\" font-size=\"{}\" fill=\"var(--_m)\">{labels_out}</g>",
            js::fmt(small)
        ));
    }
    content.push_str(&format!("<g class=\"pele-nodes\">{nodes_out}</g>"));
    let acc = Accessible {
        acc_title: model.acc_title.as_deref(),
        acc_descr: model.acc_descr.as_deref(),
    };
    let svg = svg_document("requirement", width, height, size, options, &acc, &content);
    Rendered {
        svg,
        width,
        height,
        links: Vec::new(),
    }
}
