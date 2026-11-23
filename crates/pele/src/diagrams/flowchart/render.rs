//! flowchart を描く（pele の `src/diagrams/flowchart/render.ts`）。

use std::collections::HashMap;

use super::db::FlowDb;
use super::graph::{FlowGraph, GraphNode, build_flow_graph};
use super::shapes::canonical_shape;
use super::types::LabelType;
use crate::diagrams::common::fit_width::{runs_across, tighten, title_room, turn_to_fit};
use crate::errors::Result;
use crate::js;
use crate::layout::compound::{
    CEdge, CNode, CompoundResult, Dir, cnode, compound_layout, direction,
    inherit_crossed_directions, shift_layout,
};
use crate::layout::layered::LayeredOptions;
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num};
use crate::svg::cluster::{
    Crossings, cluster_title_x, mark_crossings, struck_title, title_crossed,
};
use crate::svg::edges::{
    EdgePath, Fill, casings_svg, edge_casing, edge_label_svg, loop_path, marker, marker_trim,
    over_fills, route_path, side_reach,
};
use crate::svg::ink::{ink_fill, paints};
use crate::svg::root::{Accessible, svg_document};
use crate::svg::shapes::{
    draw_shape, end_side, inset_route, shape_filled, shape_has_label, shape_size, shape_span,
};
use crate::svg::theme::{ResolvedStyle, class_names, resolve_style, rounded, sets_fill};
use crate::svg::title::title_label;
use crate::text::entities::decode_entities;
use crate::text::label::{Label, layout_label};
use crate::text::measurer::{TextMeasurer, default_measurer, style};
use crate::types::{IconResolver, LinkInfo, RenderOptions, Rendered};
use crate::util::url::{image_url, internal_link, linked, rel_attr, safe_url};
use crate::util::yaml::Value;

const NODE_SEP: f64 = 40.0;
const EDGE_SEP: f64 = 16.0;
const RANK_SEP: f64 = 48.0;
const GROUP_PAD: f64 = 20.0;
const LOOP: f64 = 26.0;
const SHAPE_ATTRS: &str = " fill=\"var(--_s)\" stroke=\"var(--_b)\"";
const LINE_ATTRS: &str = " stroke=\"var(--_b)\"";

fn number_option(section: &Config, key: &str, fallback: f64) -> f64 {
    match section.get(key) {
        Some(Value::Num(v)) if *v > 0.0 => js::min(*v, 2000.0),
        _ => fallback,
    }
}

/// 配置の違う flowchart の種類。上の段のまとまりを自分で描いてよい。
pub trait FlowVariant {
    fn type_(&self) -> &str;
    fn prepare(&self, graph: &mut FlowGraph);
    fn layout(
        &self,
        nodes: &mut [CNode],
        edges: &mut [CEdge],
        dir: Dir,
        opt: &LayeredOptions,
        tight: bool,
    ) -> CompoundResult;
    /// 上の段のまとまりを描く（描かなければ None）。
    #[allow(clippy::too_many_arguments)]
    fn draw_group(
        &self,
        _c: &CNode,
        _label: &Label,
        _style: &ResolvedStyle,
        _classes: &str,
        _id: &str,
        _dir: Dir,
        _icons: Option<&IconResolver>,
    ) -> Option<String> {
        None
    }
    /// `draw_group` のまとまりのうち塗る所（[left, top, right, bottom]）。
    fn filled_box(&self, _c: &CNode, _dir: Dir) -> Option<[f64; 4]> {
        None
    }
}

fn asset_size(value: Option<f64>, fallback: f64) -> f64 {
    match value {
        Some(v) if v > 0.0 && v <= 4000.0 => v,
        _ => fallback,
    }
}

struct NodeView<'a> {
    node: &'a GraphNode,
    style: ResolvedStyle,
    label: Label,
    shape: String,
    w: f64,
    h: f64,
    dy: f64,
    loops: usize,
}

pub fn render_flowchart(
    db: &FlowDb,
    config: &Config,
    options: &RenderOptions,
    variant: Option<&dyn FlowVariant>,
) -> Result<Rendered> {
    let fitted = |down: bool| {
        tighten(options, |tight| {
            draw(db, config, options, variant, down, tight)
        })
    };
    if runs_across(db.direction.as_deref()) {
        turn_to_fit(options, fitted)
    } else {
        fitted(false)
    }
}

/// `turned` は横に流れる flowchart を縦に流して描く。`tight` はノードを詰める。
fn draw(
    db: &FlowDb,
    config: &Config,
    options: &RenderOptions,
    variant: Option<&dyn FlowVariant>,
    turned: bool,
    tight: bool,
) -> Result<Rendered> {
    let group_pad = if tight { 12.0 } else { GROUP_PAD };
    let empty = Config::new();
    let own = variant.and_then(|v| config.get(v.type_()));
    let flow = match own {
        Some(Value::Obj(m)) => m,
        _ => match config.get("flowchart") {
            Some(Value::Obj(m)) => m,
            _ => &empty,
        },
    };
    let curve = match flow.get("curve") {
        Some(Value::Str(s)) => Some(s.as_str()),
        _ => None,
    };
    let mut graph = build_flow_graph(db, curve);
    if let Some(v) = variant {
        v.prepare(&mut graph);
    }
    let size = options.font_size.unwrap_or(16.0);
    let edge_size = js::round(size * 0.875);
    let measurer: &dyn TextMeasurer = match &options.measurer {
        Some(m) => m.as_ref(),
        None => default_measurer(),
    };
    let wrap_width = js::min(
        number_option(flow, "wrappingWidth", 200.0),
        if variant.is_some() && tight {
            100.0
        } else {
            f64::INFINITY
        },
    );
    let pad = options.padding.unwrap_or(8.0);
    let icons = options.icons.as_ref();

    let mut index: HashMap<&str, usize> = HashMap::new();
    for (i, node) in graph.nodes.iter().enumerate() {
        index.insert(&node.id, i);
    }
    let mut views: Vec<NodeView> = Vec::new();
    let mut cnodes: Vec<CNode> = Vec::new();

    for node in &graph.nodes {
        let declarations: Vec<String> = if !node.css_styles.is_empty() {
            node.css_compiled_styles
                .iter()
                .chain(node.css_styles.iter())
                .cloned()
                .collect()
        } else {
            node.css_compiled_styles.clone()
        };
        let st = resolve_style(&declarations);
        let text_style =
            (if st.bold { style::BOLD } else { 0 }) | (if st.italic { style::ITALIC } else { 0 });
        let markdown = node.label_type == LabelType::Markdown;
        let font_size = st.font_size.unwrap_or(size);
        let mut c;
        let view;
        if node.is_group {
            let max = if variant.is_some() && tight {
                js::max(80.0, title_room(options) / 2.0)
            } else {
                4000.0
            };
            let label = layout_label(
                node.label.as_deref(),
                markdown,
                measurer,
                font_size,
                max,
                text_style,
            );
            c = cnode(0.0, 0.0, -1);
            c.is_group = true;
            c.dir = node.dir.as_deref().filter(|d| !d.is_empty()).map(direction);
            c.pad_x = group_pad;
            c.pad_top = if label.height > 0.0 {
                label.height + 16.0
            } else {
                GROUP_PAD
            };
            c.pad_bottom = group_pad;
            c.min_w = label.width + 2.0 * group_pad - 12.0;
            view = NodeView {
                node,
                style: st,
                label,
                shape: "group".into(),
                w: 0.0,
                h: 0.0,
                dy: 0.0,
                loops: 0,
            };
        } else {
            let shape = canonical_shape(Some(&node.shape));
            let has_asset = node.img.as_deref().is_some_and(|s| !s.is_empty())
                || node.icon.as_deref().is_some_and(|s| !s.is_empty());
            let text = if has_asset || shape_has_label(&shape) {
                node.label.as_deref()
            } else {
                None
            };
            let label = layout_label(text, markdown, measurer, font_size, wrap_width, text_style);
            let (w, h, mut dy);
            if has_asset {
                let bx = if node.img.as_deref().is_some_and(|s| !s.is_empty()) {
                    80.0
                } else {
                    48.0
                };
                let aw = asset_size(node.asset_width, bx);
                let ah = asset_size(node.asset_height, bx);
                w = js::max(aw, label.width + 8.0);
                h = ah
                    + if label.height > 0.0 {
                        label.height + 6.0
                    } else {
                        0.0
                    };
                dy = if node.pos.as_deref() == Some("t") {
                    -ah / 2.0
                } else {
                    ah / 2.0
                };
                if label.height == 0.0 {
                    dy = 0.0;
                }
            } else {
                let s = shape_size(&shape, label.width, label.height);
                w = s.w;
                h = s.h;
                dy = s.dy;
            }
            c = cnode(w, h, -1);
            view = NodeView {
                node,
                style: st,
                label,
                shape,
                w,
                h,
                dy,
                loops: 0,
            };
        }
        c.seq = views.len() as f64;
        views.push(view);
        cnodes.push(c);
    }
    for (i, node) in graph.nodes.iter().enumerate() {
        cnodes[i].parent = match &node.parent_id {
            Some(p) => index.get(p.as_str()).map_or(-1, |&k| k as i64),
            None => -1,
        };
    }
    let ends: Vec<(i64, i64)> = graph
        .edges
        .iter()
        .map(|e| {
            (
                index.get(e.start.as_str()).map_or(-1, |&k| k as i64),
                index.get(e.end.as_str()).map_or(-1, |&k| k as i64),
            )
        })
        .collect();
    inherit_crossed_directions(&mut cnodes, ends);

    let root_dir = if turned {
        Dir::TB
    } else {
        direction(db.direction.as_deref().unwrap_or(""))
    };
    let flows_sideways = |cnodes: &[CNode], i: usize| -> bool {
        let mut dir: Option<Dir> = None;
        let mut p = cnodes[i].parent;
        let mut hops = 0;
        while p >= 0 && dir.is_none() && hops < 10000 {
            dir = cnodes[p as usize].dir;
            p = cnodes[p as usize].parent;
            hops += 1;
        }
        let dir = dir.unwrap_or(root_dir);
        dir == Dir::LR || dir == Dir::RL
    };
    // ノードの 1 辺の辺はそれに沿って広がる。画像やアイコンは真ん中に保つ。
    for i in 0..views.len() {
        let view = &views[i];
        if view.node.is_group {
            cnodes[i].span = f64::INFINITY;
        } else if !view.node.img.as_deref().is_some_and(|s| !s.is_empty())
            && !view.node.icon.as_deref().is_some_and(|s| !s.is_empty())
        {
            let sideways = flows_sideways(&cnodes, i);
            cnodes[i].span = shape_span(&view.shape, view.w, view.h, sideways);
        }
    }

    let mut cedges: Vec<CEdge> = Vec::new();
    let mut edge_labels: Vec<Label> = Vec::new();
    let mut drawn = Vec::new();
    for edge in &graph.edges {
        let (Some(&src), Some(&dst)) =
            (index.get(edge.start.as_str()), index.get(edge.end.as_str()))
        else {
            continue;
        };
        let label = layout_label(
            Some(&edge.label),
            edge.label_type == LabelType::Markdown,
            measurer,
            edge_size,
            wrap_width,
            0,
        );
        if src == dst {
            // 自分への輪はノードの横、流れを横切る側に置き、ノードの箱を輪の分だけ広げる。
            views[src].loops += 1;
            if flows_sideways(&cnodes, src) {
                cnodes[src].h += 2.0
                    * (LOOP
                        + if label.height > 0.0 {
                            label.height + 8.0
                        } else {
                            0.0
                        });
            } else {
                cnodes[src].w += 2.0
                    * (LOOP
                        + if label.width > 0.0 {
                            label.width + 12.0
                        } else {
                            0.0
                        });
            }
        }
        drawn.push(edge);
        cedges.push(CEdge {
            src,
            dst,
            minlen: edge.minlen.unwrap_or(1.0),
            label_w: if label.width > 0.0 {
                label.width + 12.0
            } else {
                0.0
            },
            label_h: if label.height > 0.0 {
                label.height + 4.0
            } else {
                0.0
            },
            route: Vec::new(),
            label_x: 0.0,
            label_y: 0.0,
        });
        edge_labels.push(label);
    }

    let layout_options = LayeredOptions {
        node_sep: number_option(flow, "nodeSpacing", NODE_SEP) * if tight { 0.5 } else { 1.0 },
        edge_sep: EDGE_SEP,
        rank_sep: number_option(flow, "rankSpacing", RANK_SEP) * if tight { 0.75 } else { 1.0 },
        port_sep: 20.0,
        tracks: curve.is_some_and(|c| c.starts_with("step")),
        straight: curve == Some("linear"),
        head_room: marker_trim("arrow_point"),
        ..Default::default()
    };
    let layout = match variant {
        Some(v) => v.layout(&mut cnodes, &mut cedges, root_dir, &layout_options, tight),
        None => compound_layout(&mut cnodes, &mut cedges, root_dir, &layout_options),
    };

    let title = title_label(
        db.title.as_deref(),
        measurer,
        size,
        if tight { title_room(options) } else { 4000.0 },
    );
    let title_height = if title.height > 0.0 {
        title.height + 12.0
    } else {
        0.0
    };
    let inner = js::max(layout.width, title.width);
    let ox = pad + (inner - layout.width) / 2.0;
    let oy = pad + title_height;
    shift_layout(&mut cnodes, &mut cedges, ox, oy);
    let width = (inner + 2.0 * pad).ceil();
    let height = (layout.height + title_height + 2.0 * pad).ceil();
    let mut links: Vec<LinkInfo> = Vec::new();

    let mut crossings: Crossings = HashMap::new();
    if cnodes.iter().any(|c| c.is_group) {
        for e in &cedges {
            mark_crossings(&mut crossings, &e.route);
        }
    }

    let mut clusters = String::new();
    let mut struck_titles = String::new();
    let mut nodes_out = String::new();
    let mut fills: Vec<Fill> = Vec::new();
    for i in 0..views.len() {
        let view = &views[i];
        let c = &cnodes[i];
        let node = view.node;
        let x = c.x;
        let y = c.y;
        let classes = class_names(strip_default(&node.css_classes));
        let id = esc_text(&node.id);
        if c.is_group
            && c.parent < 0
            && let Some(v) = variant
            && let Some(group) =
                v.draw_group(c, &view.label, &view.style, &classes, &id, root_dir, icons)
        {
            if paints(&view.style.shape)
                && let Some(b) = v.filled_box(c, root_dir)
            {
                fills.push(Fill {
                    box_: b,
                    shape: view.style.shape.clone(),
                });
            }
            clusters.push_str(&group);
            continue;
        }
        if c.is_group {
            if paints(&view.style.shape) {
                fills.push(Fill {
                    box_: [x - c.w / 2.0, y - c.h / 2.0, x + c.w / 2.0, y + c.h / 2.0],
                    shape: view.style.shape.clone(),
                });
            }
            let title_x = cluster_title_x(c, view.label.width, &crossings, group_pad);
            let title_y = y - c.h / 2.0 + 8.0 + view.label.height / 2.0;
            let struck = title_crossed(c, view.label.width, title_x, &crossings);
            // 辺の上に描く題は、テーマの色の縁取りの上に置く。
            let title_attrs = format!(
                " class=\"pele-cluster-label\"{}{}",
                if struck {
                    " fill=\"var(--_m)\"".to_string()
                } else {
                    ink_fill(&view.style.shape, &view.style.text, Some("var(--_m)"))
                },
                view.style.text
            );
            if struck {
                struck_titles.push_str(&struck_title(
                    &view.label,
                    title_x,
                    title_y,
                    &title_attrs,
                    &id,
                    &cnodes,
                    i,
                    icons,
                ));
            }
            clusters.push_str(&format!(
                "<g class=\"pele-cluster{classes}\" data-id=\"{id}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_a)\"{} stroke=\"var(--_b)\"{}/>{}</g>",
                num(x - c.w / 2.0),
                num(y - c.h / 2.0),
                num(c.w),
                num(c.h),
                if sets_fill(&view.style.shape) { "" } else { " fill-opacity=\"0.5\"" },
                rounded(&view.style.shape),
                if struck { String::new() } else { label_svg(&view.label, title_x, title_y, &title_attrs, icons) }
            ));
            continue;
        }

        let w = view.w;
        let h = view.h;
        let inner;
        let mut ink = String::new();
        let img = node.img.as_deref().filter(|s| !s.is_empty());
        let icon = node.icon.as_deref().filter(|s| !s.is_empty());
        if let Some(img) = img {
            let aw = asset_size(node.asset_width, 80.0);
            let ah = asset_size(node.asset_height, 80.0);
            let top = if view.label.height > 0.0 {
                if node.pos.as_deref() == Some("t") {
                    h / 2.0 - ah
                } else {
                    -h / 2.0
                }
            } else {
                -ah / 2.0
            };
            inner = match image_url(&safe_url(img), options) {
                None => String::new(),
                Some(src) => format!(
                    "<image href=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"{}\"/>",
                    esc(&src),
                    num(-aw / 2.0),
                    num(top),
                    num(aw),
                    num(ah),
                    if node.constraint.as_deref() == Some("on") {
                        "xMidYMid meet"
                    } else {
                        "none"
                    }
                ),
            };
        } else if let Some(icon) = icon {
            let bx = asset_size(node.asset_height, 48.0);
            let top = if view.label.height > 0.0 {
                if node.pos.as_deref() == Some("t") {
                    h / 2.0 - bx
                } else {
                    -h / 2.0
                }
            } else {
                -bx / 2.0
            };
            let frame = if node.shape == "icon" {
                String::new()
            } else if node.shape == "iconCircle" {
                format!(
                    "<circle cy=\"{}\" r=\"{}\"{SHAPE_ATTRS}{}/>",
                    num(top + bx / 2.0),
                    num(bx / 2.0),
                    view.style.shape
                )
            } else {
                format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\"{SHAPE_ATTRS}{}/>",
                    num(-bx / 2.0),
                    num(top),
                    num(bx),
                    num(bx),
                    if node.shape == "iconRounded" { 8 } else { 0 },
                    view.style.shape
                )
            };
            let glyph = bx * 0.6;
            inner = format!(
                "{frame}<svg class=\"pele-icon\" data-icon=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\">{}</svg>",
                esc_text(icon),
                num(-glyph / 2.0),
                num(top + (bx - glyph) / 2.0),
                num(glyph),
                num(glyph),
                icons.and_then(|f| f(icon)).unwrap_or_default()
            );
        } else {
            inner = draw_shape(
                &view.shape,
                w,
                h,
                &format!("{SHAPE_ATTRS}{}", view.style.shape),
                &format!("{LINE_ATTRS}{}", view.style.line),
            );
            if shape_filled(&view.shape) {
                ink = ink_fill(&view.style.shape, &view.style.text, None);
            }
        }

        let mut text = label_svg(
            &view.label,
            0.0,
            view.dy,
            &format!(" class=\"pele-label\"{ink}{}", view.style.text),
            icons,
        );
        let internal = format!("{classes} ").contains(" internal-link ");
        if let Some(label) = node
            .label
            .as_deref()
            .filter(|l| internal && !l.is_empty() && options.links != Some(false))
        {
            // ラベルはノートの名前だが、href にも入るので、URL と同じ確かめをする。
            let name = decode_entities(label);
            let safe = internal_link(&name, options);
            let target = esc(&name);
            text = format!(
                "<a class=\"internal-link\"{} data-href=\"{target}\">{text}</a>",
                if safe {
                    format!(" href=\"{target}\"{}", rel_attr(options))
                } else {
                    String::new()
                }
            );
            links.push(LinkInfo {
                id: node.id.clone(),
                href: name,
                internal: true,
            });
        }
        let mut body = inner + &text;
        if let Some(tt) = node.tooltip.as_deref().filter(|t| !t.is_empty()) {
            body = format!("<title>{}</title>{body}", esc_text(tt));
        }
        if let Some(link) = node.link.as_deref().filter(|l| !l.is_empty()) {
            let target = match node.link_target.as_deref().filter(|t| !t.is_empty()) {
                Some(t) => format!(" target=\"{}\"", esc(t)),
                None => String::new(),
            };
            body = linked(&body, link, &node.id, &mut links, options, &target);
        }
        nodes_out.push_str(&format!(
            "<g class=\"pele-node pele-shape-{}{classes}\" data-id=\"{id}\" transform=\"translate({},{})\">{body}</g>",
            view.shape,
            num(x),
            num(y)
        ));
    }

    let mut reach = side_reach();
    for (i, ce) in cedges.iter().enumerate() {
        if ce.src == ce.dst
            || drawn[i].thickness.as_deref() == Some("invisible")
            || ce.route.len() < 6
        {
            continue;
        }
        reach.add(
            ce.src,
            end_side(&ce.route, 0, &cnodes[ce.src]),
            &drawn[i].arrow_type_start,
        );
        reach.add(
            ce.dst,
            end_side(&ce.route, ce.route.len() - 3, &cnodes[ce.dst]),
            &drawn[i].arrow_type_end,
        );
    }

    let mut casings = String::new();
    let mut edges_out = String::new();
    let mut labels_out = String::new();
    let mut loop_seen: HashMap<usize, usize> = HashMap::new();
    for i in 0..cedges.len() {
        let edge = drawn[i];
        let label = &edge_labels[i];
        let st = if !edge.style.is_empty() || !edge.css_compiled_styles.is_empty() {
            let decl: Vec<String> = edge
                .css_compiled_styles
                .iter()
                .chain(edge.style.iter())
                .cloned()
                .collect();
            Some(resolve_style(&decl))
        } else {
            None
        };
        let line = st.as_ref().map_or("", |s| s.line.as_str());
        if edge.thickness.as_deref() != Some("invisible") {
            let start_type = edge.arrow_type_start.as_str();
            let end_type = edge.arrow_type_end.as_str();
            let (src, dst) = (cedges[i].src, cedges[i].dst);
            let path: EdgePath = if src == dst {
                let view = &views[src];
                let k = loop_seen.get(&src).copied().unwrap_or(0);
                loop_seen.insert(src, k + 1);
                let sideways = flows_sideways(&cnodes, src);
                let (cx, cy) = (cnodes[src].x, cnodes[src].y);
                loop_path(
                    cx,
                    cy,
                    view.w,
                    view.h,
                    k as f64,
                    sideways,
                    LOOP,
                    marker_trim(end_type),
                    &mut cedges[i],
                )
            } else {
                let mut route = cedges[i].route.clone();
                let leaves = reach.of(src, end_side(&route, 0, &cnodes[src]));
                let arrives = reach.of(dst, end_side(&route, route.len() - 3, &cnodes[dst]));
                inset_route(
                    &mut route,
                    0,
                    &views[src].shape,
                    views[src].w,
                    views[src].h,
                    &cnodes[src],
                );
                let at = route.len() - 3;
                inset_route(
                    &mut route,
                    at,
                    &views[dst].shape,
                    views[dst].w,
                    views[dst].h,
                    &cnodes[dst],
                );
                route_path(
                    &route,
                    edge.curve.as_deref(),
                    marker_trim(start_type),
                    marker_trim(end_type),
                    Some(leaves),
                    Some(arrives),
                )
            };
            let over = over_fills(
                &path.d,
                &[path.sx, path.sy, path.ex, path.ey],
                &fills,
                line,
                path.tokens.as_deref(),
            );
            let color = match over.as_deref() {
                Some(o) if !o.is_empty() => o.to_string(),
                _ => match st
                    .as_ref()
                    .and_then(|s| s.stroke.as_deref())
                    .filter(|s| !s.is_empty())
                {
                    Some(s) => esc(s),
                    None => "var(--_l)".into(),
                },
            };
            let attrs = match edge.thickness.as_deref() {
                Some("thick") => " stroke-width=\"2.5\"",
                Some("dotted") => " stroke-dasharray=\"3 4\"",
                _ => "",
            };
            let classes = class_names(
                &edge
                    .classes
                    .replace("edge-thickness-normal", "")
                    .replace("edge-pattern-solid", "")
                    .replace("flowchart-link", ""),
            );
            if over.as_deref() == Some("") {
                casings.push_str(&edge_casing(
                    &path,
                    start_type,
                    end_type,
                    line,
                    edge.thickness.as_deref() == Some("thick"),
                ));
            }
            edges_out.push_str(&format!(
                "<g class=\"pele-edge{classes}\" data-id=\"{}\"{}{}><path d=\"{}\"{attrs}/>{}{}</g>",
                esc_text(&edge.id),
                match over.as_deref() {
                    Some(o) if !o.is_empty() => format!(" stroke=\"{o}\""),
                    _ => String::new(),
                },
                line,
                path.d,
                marker(start_type, path.sx, path.sy, path.sdx, path.sdy, &color, "none"),
                marker(end_type, path.ex, path.ey, path.edx, path.edy, &color, "none")
            ));
        }
        if label.width > 0.0 {
            let text_attrs = st.as_ref().map_or("", |s| s.text.as_str());
            labels_out.push_str(&edge_label_svg(
                &edge.id,
                label,
                cedges[i].label_x,
                cedges[i].label_y,
                text_attrs,
                icons,
            ));
        }
    }

    let mut content = label_svg(
        &title,
        width / 2.0,
        pad + title.height / 2.0,
        " class=\"pele-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    if !clusters.is_empty() {
        content.push_str(&format!("<g class=\"pele-clusters\">{clusters}</g>"));
    }
    content.push_str(&casings_svg(&casings));
    if !edges_out.is_empty() {
        content.push_str(&format!("<g class=\"pele-edges\" fill=\"none\" stroke=\"var(--_l)\" stroke-linecap=\"round\">{edges_out}</g>"));
    }
    if !struck_titles.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-cluster-titles\">{struck_titles}</g>"
        ));
    }
    if !labels_out.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-edge-labels\" font-size=\"{}\">{labels_out}</g>",
            js::fmt(edge_size)
        ));
    }
    content.push_str(&format!("<g class=\"pele-nodes\">{nodes_out}</g>"));
    let acc = Accessible {
        acc_title: db.acc_title.as_deref(),
        acc_descr: db.acc_descr.as_deref(),
    };
    let svg = svg_document(
        variant.map_or("flowchart", |v| v.type_()),
        width,
        height,
        size,
        options,
        &acc,
        &content,
    );

    Ok(Rendered {
        svg,
        width,
        height,
        links,
    })
}

/// `raw.replace(/^default\s?/, '')`
fn strip_default(raw: &str) -> &str {
    match raw.strip_prefix("default") {
        Some(rest) => match rest.chars().next() {
            Some(c) if crate::util::chars::is_space_char(c) => &rest[c.len_utf8()..],
            _ => rest,
        },
        None => raw,
    }
}
