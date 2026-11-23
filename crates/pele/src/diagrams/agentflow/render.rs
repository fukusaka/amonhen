//! agentflow を描く（pele の `src/diagrams/agentflow/render.ts`）。

use std::collections::HashMap;

use super::color_slots::{KIND_SLOT, kind_slot};
use super::db::AgentflowDb;
use super::graph::{GraphEdge, GraphNode};
use super::types::LabelType;
use crate::diagrams::common::fit_width::{runs_across, turn_to_fit};
use crate::diagrams::flowchart::shapes::canonical_shape;
use crate::errors::Result;
use crate::js;
use crate::layout::compound::{
    CEdge, CNode, Dir, cnode, compound_layout, direction, inherit_crossed_directions, shift_layout,
};
use crate::layout::layered::LayeredOptions;
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num};
use crate::svg::cluster::{Crossings, cluster_title_x, mark_crossings};
use crate::svg::edges::{
    EdgePath, Fill, casings_svg, edge_casing, edge_label_svg, loop_path, marker, marker_trim,
    over_fills, route_path,
};
use crate::svg::ink::{ink_fill, paints};
use crate::svg::root::{Accessible, svg_document};
use crate::svg::shapes::{PAD_X, PAD_Y, draw_shape, inset_route, shape_filled, shape_size};
use crate::svg::theme::{
    ResolvedStyle, class_names, resolve_style, rounded, series_color, sets_fill,
};
use crate::text::entities::decode_entities;
use crate::text::label::{Label, layout_label};
use crate::text::measurer::{TextMeasurer, default_measurer, style};
use crate::types::{LinkInfo, RenderOptions, Rendered};
use crate::util::url::{internal_link, linked, rel_attr};
use crate::util::yaml::Value;

/// 系列の色の数。Mermaid の枠の決まりを当てるパレット。
pub const PALETTE: i64 = 8;

const NODE_SEP: f64 = 40.0;
const EDGE_SEP: f64 = 16.0;
const RANK_SEP: f64 = 48.0;
const GROUP_PAD: f64 = 20.0;
const LOOP: f64 = 26.0;
const DOTS: f64 = 14.0;
const COLLAPSED: &str = "collapsed";

/// 流れの向かう辺。辺は上から時計回りに数える。
fn flow_end(dir: Dir) -> usize {
    match dir {
        Dir::TB => 2,
        Dir::RL => 3,
        Dir::BT => 0,
        Dir::LR => 1,
    }
}

fn number_option(config: &Config, key: &str, fallback: f64) -> f64 {
    match config
        .get("agentflow")
        .and_then(Value::as_obj)
        .and_then(|a| a.get(key))
    {
        Some(Value::Num(v)) if *v > 0.0 => js::min(*v, 2000.0),
        _ => fallback,
    }
}

/// パレットの色の縁と、中の同じ色の薄い塗り。書き手の決めた塗りは薄い塗りに代わり、不透明のまま。
fn tint(slot: i64, wash: &str, style: &ResolvedStyle) -> String {
    let color = series_color(slot);
    format!(
        " fill=\"{color}\"{} stroke=\"{color}\"",
        if sets_fill(&style.shape) {
            String::new()
        } else {
            format!(" fill-opacity=\"{wash}\"")
        }
    )
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

pub fn render_agentflow(
    db: &AgentflowDb,
    config: &Config,
    options: &RenderOptions,
) -> Result<Rendered> {
    if !runs_across(db.direction.as_deref()) {
        return draw(db, config, options, false);
    }
    turn_to_fit(options, |down| draw(db, config, options, down))
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

fn dot(x: f64, y: f64) -> String {
    format!(
        "M{},{}a1.75,1.75 0 1 0 3.5,0a1.75,1.75 0 1 0 -3.5,0Z",
        num(x - 1.75),
        num(y)
    )
}

struct Held<'a> {
    edge: &'a GraphEdge,
    label: Label,
    src: usize,
    dst: usize,
}

/// `turned` は横に流れる図を縦に流して描く。
fn draw(
    db: &AgentflowDb,
    config: &Config,
    options: &RenderOptions,
    turned: bool,
) -> Result<Rendered> {
    let empty = Default::default();
    let graph = db.graph.as_ref().unwrap_or(&empty);
    let size = options.font_size.unwrap_or(16.0);
    let edge_size = js::round(size * 0.875);
    let measurer: &dyn TextMeasurer = match &options.measurer {
        Some(m) => m.as_ref(),
        None => default_measurer(),
    };
    let wrap_width = number_option(config, "wrappingWidth", 120.0);
    let min_width = number_option(config, "minNodeWidth", 120.0);
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
        let view;
        let mut c;
        if node.is_group {
            let label = layout_label(
                node.label.as_deref(),
                markdown,
                measurer,
                font_size,
                4000.0,
                text_style,
            );
            c = cnode(0.0, 0.0, -1);
            c.is_group = true;
            c.dir = node.dir.as_deref().filter(|d| !d.is_empty()).map(direction);
            c.pad_x = GROUP_PAD;
            c.pad_top = if label.height > 0.0 {
                label.height + 16.0
            } else {
                GROUP_PAD
            };
            c.pad_bottom = GROUP_PAD;
            c.min_w = label.width + 2.0 * GROUP_PAD - 12.0;
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
        } else if node.shape == "collapsedGroup" {
            let label = layout_label(
                node.label.as_deref(),
                markdown,
                measurer,
                font_size,
                wrap_width,
                text_style,
            );
            let w = js::max(label.width, min_width) + 2.0 * PAD_X;
            let h = label.height + 2.0 * PAD_Y + DOTS;
            c = cnode(w, h, -1);
            view = NodeView {
                node,
                style: st,
                label,
                shape: COLLAPSED.into(),
                w,
                h,
                dy: -DOTS / 2.0,
                loops: 0,
            };
        } else {
            let shape = canonical_shape(Some(&node.shape));
            let label = layout_label(
                node.label.as_deref(),
                markdown,
                measurer,
                font_size,
                wrap_width,
                text_style,
            );
            // 菱形はラベルの倍の速さで広がるので、ラベル自身の幅で。
            let s = shape_size(
                &shape,
                if shape == "diam" {
                    label.width
                } else {
                    js::max(label.width, min_width)
                },
                label.height,
            );
            c = cnode(s.w, s.h, -1);
            view = NodeView {
                node,
                style: st,
                label,
                shape,
                w: s.w,
                h: s.h,
                dy: s.dy,
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
    // ノードのいる段の向き。
    let flow_of = |cnodes: &[CNode], i: usize| -> Dir {
        let mut dir: Option<Dir> = None;
        let mut p = cnodes[i].parent;
        let mut hops = 0;
        while p >= 0 && dir.is_none() && hops < 10000 {
            dir = cnodes[p as usize].dir;
            p = cnodes[p as usize].parent;
            hops += 1;
        }
        dir.unwrap_or(root_dir)
    };
    let flows_sideways =
        |cnodes: &[CNode], i: usize| -> bool { matches!(flow_of(cnodes, i), Dir::LR | Dir::RL) };

    let mut depth: Vec<i64> = vec![-1; cnodes.len()];
    for i in 0..cnodes.len() {
        let mut chain: Vec<usize> = Vec::new();
        let mut j = i as i64;
        while j >= 0 && depth[j as usize] < 0 && chain.len() <= cnodes.len() {
            chain.push(j as usize);
            j = cnodes[j as usize].parent;
        }
        let first = if j >= 0 { depth[j as usize] + 1 } else { 0 };
        for (d, &k) in (first..).zip(chain.iter().rev()) {
            depth[k] = d;
        }
    }
    let contains = |cnodes: &[CNode], group: usize, i: usize| -> bool {
        let mut j = i as i64;
        let mut d = depth[i];
        while d > depth[group] {
            j = cnodes[j as usize].parent;
            d -= 1;
        }
        j == group as i64 && i != group
    };

    let mut cedges: Vec<CEdge> = Vec::new();
    let mut edge_labels: Vec<Label> = Vec::new();
    let mut drawn: Vec<&GraphEdge> = Vec::new();
    // ノードとそれを囲む入れ物の間の辺。配置に置き場がないので、後でノードと入れ物の縁の間に描く。
    let mut held: Vec<Held> = Vec::new();
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
        if contains(&cnodes, src, dst) || contains(&cnodes, dst, src) {
            held.push(Held {
                edge,
                label,
                src,
                dst,
            });
            continue;
        }
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

    let layout = compound_layout(
        &mut cnodes,
        &mut cedges,
        root_dir,
        &LayeredOptions {
            node_sep: number_option(config, "nodeSpacing", NODE_SEP),
            edge_sep: EDGE_SEP,
            rank_sep: number_option(config, "rankSpacing", RANK_SEP),
            port_sep: 20.0,
            ..Default::default()
        },
    );

    let title = layout_label(
        db.title.as_deref(),
        false,
        measurer,
        size,
        4000.0,
        style::BOLD,
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

    // ノードとそれを囲む入れ物の間の辺は、ノードと入れ物の縁の間をまっすぐ走る: 流れに沿って出るか、逆らって
    // 入る。ほかのノードが道にあれば横を、それもだめなら向こうの端を取る。
    let mut held_paths: Vec<EdgePath> = Vec::new();
    let mut held_labels: Vec<f64> = Vec::new();
    let mut held_routes: Vec<Vec<f64>> = Vec::new();
    let careful = (held.len() * cnodes.len()) < 2_000_000;
    for h in &held {
        let out = contains(&cnodes, h.dst, h.src);
        let outer = if out { h.dst } else { h.src };
        let i = if out { h.src } else { h.dst };
        let g = &cnodes[outer];
        let nd = &cnodes[i];
        let end = flow_end(g.dir.unwrap_or_else(|| flow_of(&cnodes, outer)));
        let first = if out { end } else { (end + 2) & 3 };
        let toward = |side: usize| -> Vec<f64> {
            let upright = side & 1 == 0;
            let sign = if side == 0 || side == 3 { -1.0 } else { 1.0 };
            let nx = if upright {
                nd.x
            } else {
                nd.x + (sign * nd.w) / 2.0
            };
            let ny = if upright {
                nd.y + (sign * nd.h) / 2.0
            } else {
                nd.y
            };
            let bx = if upright {
                nd.x
            } else {
                g.x + (sign * g.w) / 2.0
            };
            let by = if upright {
                g.y + (sign * g.h) / 2.0
            } else {
                nd.y
            };
            let axis = if upright { 0.0 } else { 1.0 };
            if out {
                vec![nx, ny, axis, bx, by, axis]
            } else {
                vec![bx, by, axis, nx, ny, axis]
            }
        };
        let blocked = |r: &[f64]| -> bool {
            cnodes.iter().enumerate().any(|(k, c)| {
                k != i
                    && !c.is_group
                    && c.x + c.w / 2.0 > js::min(r[0], r[3]) - 1.0
                    && c.x - c.w / 2.0 < js::max(r[0], r[3]) + 1.0
                    && c.y + c.h / 2.0 > js::min(r[1], r[4]) - 1.0
                    && c.y - c.h / 2.0 < js::max(r[1], r[4]) + 1.0
            })
        };
        let mut route = toward(first);
        if careful && blocked(&route) {
            for side in [(first + 1) & 3, (first + 3) & 3, (first + 2) & 3] {
                let other = toward(side);
                if !blocked(&other) {
                    route = other;
                    break;
                }
            }
        }
        held_labels.push((route[0] + route[3]) / 2.0);
        held_labels.push((route[1] + route[4]) / 2.0);
        let mut drawn_route = route.clone();
        inset_route(
            &mut drawn_route,
            if out { 0 } else { 3 },
            &views[i].shape,
            views[i].w,
            views[i].h,
            nd,
        );
        held_paths.push(route_path(
            &drawn_route,
            Some("linear"),
            0.0,
            marker_trim(&h.edge.arrow_type_end),
            None,
            None,
        ));
        held_routes.push(route);
    }

    let mut crossings: Crossings = HashMap::new();
    if cnodes.iter().any(|c| c.is_group) {
        for e in &cedges {
            mark_crossings(&mut crossings, &e.route);
        }
        for r in &held_routes {
            mark_crossings(&mut crossings, r);
        }
    }

    let mut clusters = String::new();
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
        if c.is_group {
            if paints(&view.style.shape) {
                fills.push(Fill {
                    box_: [x - c.w / 2.0, y - c.h / 2.0, x + c.w / 2.0, y + c.h / 2.0],
                    shape: view.style.shape.clone(),
                });
            }
            let title_x = cluster_title_x(c, view.label.width, &crossings, GROUP_PAD);
            clusters.push_str(&format!(
                "<g class=\"pele-cluster pele-flow{classes}\" data-id=\"{id}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}/>{}</g>",
                num(x - c.w / 2.0),
                num(y - c.h / 2.0),
                num(c.w),
                num(c.h),
                rounded(&(tint(node.color_index.unwrap_or(KIND_SLOT.len() as i64), "0.06", &view.style) + &view.style.shape)),
                label_svg(
                    &view.label,
                    title_x,
                    y - c.h / 2.0 + 8.0 + view.label.height / 2.0,
                    &format!(
                        " class=\"pele-cluster-label\"{}{}",
                        ink_fill(&view.style.shape, &view.style.text, Some("var(--_m)")),
                        view.style.text
                    ),
                    icons
                )
            ));
            continue;
        }

        let w = view.w;
        let h = view.h;
        let mut kind = String::new();
        let shape;
        if view.shape == COLLAPSED {
            let slot = node.color_index.unwrap_or(KIND_SLOT.len() as i64);
            let color = series_color(slot);
            let line = h / 2.0 - DOTS - 2.0;
            let dots = line + (DOTS + 2.0) / 2.0;
            shape = format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}/><path d=\"M{},{}H{}\" fill=\"none\" stroke=\"{color}\" stroke-dasharray=\"3 3\"{}/><path class=\"pele-collapsed-mark\" d=\"{}{}{}\" fill=\"{color}\" stroke=\"none\"/>",
                num(-w / 2.0),
                num(-h / 2.0),
                num(w),
                num(h),
                rounded(&(tint(slot, "0.06", &view.style) + &view.style.shape)),
                num(-w / 2.0 + 10.0),
                num(line),
                num(w / 2.0 - 10.0),
                view.style.line,
                dot(-9.0, dots),
                dot(0.0, dots),
                dot(9.0, dots)
            );
        } else {
            let k = node.kind.as_deref().unwrap_or("task");
            let slot = kind_slot(k).unwrap_or(1);
            kind = format!(" pele-kind-{k}");
            shape = draw_shape(
                &view.shape,
                w,
                h,
                &(tint(slot, "0.14", &view.style) + &view.style.shape),
                &format!(" stroke=\"{}\"{}", series_color(slot), view.style.line),
            );
        }

        let ink = if view.shape == COLLAPSED || shape_filled(&view.shape) {
            ink_fill(&view.style.shape, &view.style.text, None)
        } else {
            String::new()
        };
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
        let mut body = shape + &text;
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
            "<g class=\"pele-node pele-shape-{}{kind}{classes}\" data-id=\"{id}\" transform=\"translate({},{})\">{body}</g>",
            view.shape,
            num(x),
            num(y)
        ));
    }

    let mut casings = String::new();
    let mut edges_out = String::new();
    let mut labels_out = String::new();
    let mut put = |edge: &GraphEdge, label: &Label, path: &EdgePath, x: f64, y: f64| {
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
        let end_type = edge.arrow_type_end.as_str();
        let over = over_fills(
            &path.d,
            &[path.ex, path.ey],
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
        let kind = if edge.thickness.as_deref() == Some("dotted") {
            "reference"
        } else if end_type == "arrow_cross" {
            "failure"
        } else {
            "sequence"
        };
        let classes = class_names(
            &edge
                .classes
                .replace("edge-thickness-normal", "")
                .replace("edge-pattern-solid", "")
                .replace("flowchart-link", ""),
        );
        if over.as_deref() == Some("") {
            casings.push_str(&edge_casing(path, "none", end_type, line, false));
        }
        edges_out.push_str(&format!(
            "<g class=\"pele-edge pele-edge-{kind}{classes}\" data-id=\"{}\"{}{line}><path d=\"{}\"{}/>{}</g>",
            esc_text(&edge.id),
            match over.as_deref() {
                Some(o) if !o.is_empty() => format!(" stroke=\"{o}\""),
                _ => String::new(),
            },
            path.d,
            if edge.thickness.as_deref() == Some("dotted") {
                " stroke-dasharray=\"3 4\""
            } else {
                ""
            },
            marker(end_type, path.ex, path.ey, path.edx, path.edy, &color, "none")
        ));
        if label.width > 0.0 {
            labels_out.push_str(&edge_label_svg(
                &edge.id,
                label,
                x,
                y,
                st.as_ref().map_or("", |s| s.text.as_str()),
                icons,
            ));
        }
    };

    let mut loop_seen: HashMap<usize, usize> = HashMap::new();
    for i in 0..cedges.len() {
        let edge = drawn[i];
        let label = &edge_labels[i];
        let end_type = edge.arrow_type_end.as_str();
        let (src, dst) = (cedges[i].src, cedges[i].dst);
        let path = if src == dst {
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
                0.0,
                marker_trim(end_type),
                None,
                None,
            )
        };
        put(edge, label, &path, cedges[i].label_x, cedges[i].label_y);
    }
    for (i, h) in held.iter().enumerate() {
        put(
            h.edge,
            &h.label,
            &held_paths[i],
            held_labels[2 * i],
            held_labels[2 * i + 1],
        );
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
    let svg = svg_document("agentflow", width, height, size, options, &acc, &content);
    Ok(Rendered {
        svg,
        width,
        height,
        links,
    })
}
