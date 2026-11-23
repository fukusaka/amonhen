//! use case を描く（pele の `src/diagrams/usecase/render.ts`）。

use std::collections::HashMap;

use super::graph::{UsecaseGraphEdge, UsecaseGraphNode, build_usecase_graph};
use super::types::{LabelType, UsecaseModel};
use crate::diagrams::common::fit_width::{runs_across, turn_to_fit};
use crate::errors::Result;
use crate::js;
use crate::layout::compound::{CEdge, CNode, Dir, cnode, compound_layout, direction, shift_layout};
use crate::layout::layered::LayeredOptions;
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num};
use crate::svg::cluster::{Crossings, mark_crossings};
use crate::svg::edges::{
    EdgePath, Fill, Under, casing, casings_svg, edge_ink, edge_label_svg, fill_under, hollow_on,
    marker, marker_trim, route_path,
};
use crate::svg::ink::{ink_fill, paints};
use crate::svg::root::{Accessible, svg_document};
use crate::svg::theme::{ResolvedStyle, class_names, resolve_style, rounded, sets_fill};
use crate::svg::title::title_label;
use crate::text::label::{Label, label_line, layout_label, line_middle};
use crate::text::measurer::{TextMeasurer, default_measurer, style as st};
use crate::types::{IconResolver, RenderOptions, Rendered};
use crate::util::yaml::Value;

const NODE_SEP: f64 = 32.0;
const EDGE_SEP: f64 = 16.0;
const RANK_SEP: f64 = 72.0;
const GROUP_PAD: f64 = 20.0;
const LOOP: f64 = 26.0;
const FIGURE_W: f64 = 40.0;
const FIGURE_H: f64 = 60.0;
const FIGURE_GAP: f64 = 6.0;
const TAB_PAD: f64 = 6.0;
const CELL_PAD: f64 = 10.0;
const ROW_PAD: f64 = 8.0;
const FOLD: f64 = 9.0;
const EXTENSION: f64 = 12.0;
const PORT_GAP: f64 = 14.0;
const SHAPE_ATTRS: &str = " fill=\"var(--_s)\" stroke=\"var(--_b)\"";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Actor,
    Ellipse,
    Box,
    Json,
    Boundary,
}

struct View<'a> {
    node: &'a UsecaseGraphNode,
    kind: Kind,
    style: ResolvedStyle,
    label: Label,
    stereotype: Label,
    /// 描くものの大きさ。自分への関係を持つノードの配置の箱はこれより大きい。
    w: f64,
    h: f64,
    rows: Vec<(Label, Label)>,
    key_w: f64,
    head_h: f64,
    loops: usize,
}

fn number_option(config: &Config, key: &str, fallback: f64) -> f64 {
    match config.get("usecase") {
        Some(Value::Obj(m)) => match m.get(key) {
            Some(Value::Num(v)) if *v > 0.0 => js::min(*v, 2000.0),
            _ => fallback,
        },
        _ => fallback,
    }
}

/// 飾りのないラベルは書いたままに見せる: タグも markdown も `\n` の改行もない。印の文字はラベルの読み手に文字
/// 参照として渡し、読み手が文字に戻す。
fn literal(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if matches!(c, '&' | '<' | '>' | '\\') {
            out.push_str(&format!("&#{};", c as u32));
        } else {
            out.push(c);
        }
    }
    out
}

fn text(
    raw: Option<&str>,
    markdown: bool,
    measurer: &dyn TextMeasurer,
    size: f64,
    wrap: f64,
    base: u32,
) -> Label {
    match raw {
        Some(r) if !r.is_empty() && !markdown => {
            layout_label(Some(&literal(r)), markdown, measurer, size, wrap, base)
        }
        _ => layout_label(raw, markdown, measurer, size, wrap, base),
    }
}

/// ノードの 4 辺の 1 つ（0 上、1 右、2 下、3 左）の輪郭の点。その辺の真ん中から `along` 離れた所。線は actor の
/// ラベルでなく人の形に着く。
fn anchor(view: &View, c: &CNode, side: usize, along: f64) -> (f64, f64) {
    let sign = if side == 1 || side == 2 { 1.0 } else { -1.0 };
    let upright = side == 1 || side == 3;
    let mut reach = (if upright { view.w } else { view.h }) / 2.0;
    let mut middle = 0.0;
    if view.kind == Kind::Actor {
        if upright {
            reach = FIGURE_W / 2.0 + 3.0;
            middle = (FIGURE_H - view.h) / 2.0;
        } else {
            reach += 2.0;
        }
    } else if view.kind == Kind::Ellipse {
        let span = (if upright { view.h } else { view.w }) / 2.0;
        reach *= js::max(1.0 - (along / span).powi(2), 0.0).sqrt();
    }
    if upright {
        (c.x + sign * reach, c.y + middle + along)
    } else {
        (c.x + along, c.y + sign * reach)
    }
}

/// 辺が広がってよい辺の長さ。
fn side_length(view: &View, side: usize) -> f64 {
    let upright = side == 1 || side == 3;
    if view.kind == Kind::Actor {
        return if upright {
            FIGURE_H * 0.5
        } else {
            FIGURE_W * 0.6
        };
    }
    let length = if upright { view.h } else { view.w };
    if view.kind == Kind::Ellipse {
        length * 0.6
    } else {
        length - 16.0
    }
}

struct End {
    edge: usize,
    at: usize,
    toward: f64,
}

/// 配置はノードの 1 辺に着くすべての辺をその辺の真ん中に集める。ここで、広がる順に辺に沿って並べ、それぞれを
/// ノードの輪郭に置く。
fn attach_ends(cedges: &mut [CEdge], cnodes: &[CNode], views: &[View]) {
    let mut keys: Vec<usize> = Vec::new();
    let mut sides: HashMap<usize, Vec<End>> = HashMap::new();
    let mut add = |index: usize, edge: usize, route: &[f64], at: usize, next: usize| {
        let c = &cnodes[index];
        if c.is_group {
            return;
        }
        let dx = route[at] - c.x;
        let dy = route[at + 1] - c.y;
        let side = if (dy.abs() - c.h / 2.0).abs() < 0.5 {
            if dy < 0.0 { 0 } else { 2 }
        } else if (dx.abs() - c.w / 2.0).abs() < 0.5 {
            if dx < 0.0 { 3 } else { 1 }
        } else {
            return;
        };
        let key = index * 4 + side;
        let end = End {
            edge,
            at,
            toward: route[next + if side & 1 == 1 { 1 } else { 0 }],
        };
        sides
            .entry(key)
            .or_insert_with(|| {
                keys.push(key);
                Vec::new()
            })
            .push(end);
    };
    for (i, e) in cedges.iter().enumerate() {
        if e.src == e.dst || e.route.len() < 6 {
            continue;
        }
        let last = e.route.len() - 3;
        add(e.src, i, &e.route, 0, 3);
        add(e.dst, i, &e.route, last, last - 3);
    }
    for key in keys {
        let ends = sides.get_mut(&key).unwrap();
        let index = key >> 2;
        let side = key & 3;
        let view = &views[index];
        let step = if ends.len() > 1 {
            js::min(PORT_GAP, side_length(view, side) / (ends.len() - 1) as f64)
        } else {
            0.0
        };
        ends.sort_by(|a, b| js::order(a.toward - b.toward));
        let count = ends.len();
        for (i, end) in ends.iter().enumerate() {
            let (x, y) = anchor(
                view,
                &cnodes[index],
                side,
                (i as f64 - (count - 1) as f64 / 2.0) * step,
            );
            cedges[end.edge].route[end.at] = x;
            cedges[end.edge].route[end.at + 1] = y;
        }
    }
}

/// 汎化の中空の三角。先は (x, y)。
fn extension(x: f64, y: f64, dx: f64, dy: f64, color: &str) -> String {
    let bx = x - dx * EXTENSION;
    let by = y - dy * EXTENSION;
    let half = EXTENSION * 0.5;
    format!(
        "<path class=\"pele-marker\" d=\"M{},{}L{},{}L{},{}Z\" fill=\"var(--_bg)\" stroke=\"{color}\" stroke-linejoin=\"round\"/>",
        num(x),
        num(y),
        num(bx - dy * half),
        num(by + dx * half),
        num(bx + dy * half),
        num(by - dx * half)
    )
}

fn trim(type_: &str) -> f64 {
    if type_ == "extension" {
        EXTENSION
    } else {
        marker_trim(type_)
    }
}

#[allow(clippy::too_many_arguments)]
fn mark(type_: &str, x: f64, y: f64, dx: f64, dy: f64, color: &str, stroke: &str) -> String {
    if type_ == "extension" {
        extension(x, y, dx, dy, color)
    } else {
        marker(type_, x, y, dx, dy, color, stroke)
    }
}

fn figure(view: &View, icons: Option<&IconResolver>) -> String {
    let node = view.node;
    let shape = &view.style.shape;
    let line = format!(" fill=\"none\" stroke=\"var(--_b)\"{}", view.style.line);
    let mut head = -21.0;
    let mut radius = 9.0;
    let mut out = match node.actor_type {
        Some("hollow") => {
            radius = 8.0;
            format!(
                "<circle cy=\"-21\" r=\"8\" fill=\"var(--_bg)\" stroke=\"var(--_b)\"{shape}/><path d=\"M-20,-9H20V0H5.5L20,15.5L12,25.5L0,12L-12,25.5L-20,15.5L-5.5,0H-20Z\" fill=\"var(--_bg)\" stroke=\"var(--_b)\" stroke-linejoin=\"round\"{shape}/>"
            )
        }
        Some("awesome") => format!(
            "<path d=\"M0,-30A11,11 0 1 1 0,-8A11,11 0 1 1 0,-30ZM-20,25C-20,9 -12,-3 0,-3C12,-3 20,9 20,25C20,28 18,30 15,30H-15C-18,30 -20,28 -20,25Z\" fill=\"var(--_b)\"{shape}/>"
        ),
        Some("icon") => {
            head = -14.0;
            radius = 12.0;
            let name = node.icon.as_deref().unwrap_or("");
            format!(
                "<rect x=\"-22\" y=\"-26\" width=\"44\" height=\"44\"{}/><svg class=\"pele-icon\" data-icon=\"{}\" x=\"-14\" y=\"-18\" width=\"28\" height=\"28\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\">{}</svg>",
                rounded(&format!("{SHAPE_ATTRS}{shape}")),
                esc_text(name),
                icons.and_then(|f| f(name)).unwrap_or_default()
            )
        }
        _ => format!(
            "<circle cy=\"-21\" r=\"9\"{SHAPE_ATTRS}{shape}/><path d=\"M0,-12V10M-14,-3H14M0,10L-12,30M0,10L12,30\"{line}/>"
        ),
    };
    if node.business {
        // business の actor の斜線: 頭の右下を横切る弦。
        out.push_str(&format!(
            "<path class=\"pele-business\" d=\"M{},{}L{},{}\"{line}/>",
            num(0.12 * radius),
            num(head + 0.99 * radius),
            num(0.92 * radius),
            num(head - 0.39 * radius)
        ));
    }
    out
}

pub fn render_usecase(
    model: &UsecaseModel,
    config: &Config,
    options: &RenderOptions,
) -> Result<Rendered> {
    if !runs_across(Some(&model.direction)) {
        return Ok(draw(model, config, options, false));
    }
    turn_to_fit(options, |down| Ok(draw(model, config, options, down)))
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

/// `raw.replace(/^default relationship relationship-\S+\s?/, '')`
fn strip_relationship(raw: &str) -> &str {
    let Some(rest) = raw.strip_prefix("default relationship relationship-") else {
        return raw;
    };
    let n = rest
        .char_indices()
        .find(|&(_, c)| crate::util::chars::is_space_char(c))
        .map_or(rest.len(), |(i, _)| i);
    if n == 0 {
        return raw;
    }
    let after = &rest[n..];
    match after.chars().next() {
        Some(c) if crate::util::chars::is_space_char(c) => &after[c.len_utf8()..],
        _ => after,
    }
}

/// `turned` は横に流れる図を縦に流して描く。
fn draw(model: &UsecaseModel, config: &Config, options: &RenderOptions, turned: bool) -> Rendered {
    let graph = build_usecase_graph(model);
    let size = options.font_size.unwrap_or(16.0);
    let small = js::round(size * 0.875);
    let tiny = js::round(size * 0.8125);
    let measurer: &dyn TextMeasurer = match &options.measurer {
        Some(m) => m.as_ref(),
        None => default_measurer(),
    };
    let pad = options.padding.unwrap_or(8.0);
    let icons = options.icons.as_ref();
    let root_dir = if turned {
        Dir::TB
    } else {
        direction(&model.direction)
    };
    let sideways = root_dir == Dir::LR || root_dir == Dir::RL;

    let mut index: HashMap<&str, usize> = HashMap::new();
    let mut views: Vec<View> = Vec::new();
    let mut cnodes: Vec<CNode> = Vec::new();
    for (i, node) in graph.nodes.iter().enumerate() {
        index.insert(&node.id, i);
    }

    for node in &graph.nodes {
        let style = if node.css_styles.is_empty() {
            resolve_style(&node.css_compiled_styles)
        } else {
            let mut all = node.css_compiled_styles.clone();
            all.extend(node.css_styles.iter().cloned());
            resolve_style(&all)
        };
        let base =
            (if style.bold { st::BOLD } else { 0 }) | (if style.italic { st::ITALIC } else { 0 });
        let font_size = style.font_size.unwrap_or(size);
        let markdown = node.label_type == LabelType::Markdown;
        let stereotype_text = node.stereotype.as_ref().map(|s| format!("«{s}»"));
        let stereotype = text(
            stereotype_text.as_deref(),
            false,
            measurer,
            tiny,
            200.0,
            base,
        );
        let mut view = View {
            node,
            kind: Kind::Box,
            style,
            label: stereotype.clone(),
            stereotype,
            w: 0.0,
            h: 0.0,
            rows: Vec::new(),
            key_w: 0.0,
            head_h: 0.0,
            loops: 0,
        };
        let mut c: CNode;
        if node.is_group {
            view.kind = Kind::Boundary;
            view.label = text(
                Some(&node.label),
                markdown,
                measurer,
                font_size,
                4000.0,
                base,
            );
            let title_h = view.label.height;
            c = cnode(0.0, 0.0, -1);
            c.is_group = true;
            c.pad_x = GROUP_PAD;
            c.pad_bottom = GROUP_PAD;
            c.pad_top = if node.boundary_type == Some("package") {
                title_h + 2.0 * TAB_PAD + 16.0
            } else if title_h > 0.0 {
                title_h + 16.0
            } else {
                GROUP_PAD
            };
            c.min_w = view.label.width + 2.0 * GROUP_PAD;
        } else {
            if let Some(rows) = &node.json_rows {
                view.kind = Kind::Json;
                view.label = text(
                    Some(&node.label),
                    false,
                    measurer,
                    font_size,
                    4000.0,
                    base | st::BOLD,
                );
                let mut value_w: f64 = 0.0;
                for row in rows {
                    let key = text(Some(&row.key), false, measurer, small, 4000.0, base);
                    let value = text(Some(&row.value), false, measurer, small, 260.0, base);
                    view.key_w = js::max(view.key_w, key.width);
                    value_w = js::max(value_w, value.width);
                    view.h +=
                        js::max_of(&[key.height, value.height, js::round(small * 1.5)]) + ROW_PAD;
                    view.rows.push((key, value));
                }
                // 空のオブジェクトの根は鍵なしの値を持ち、そのときは鍵の列がない。
                if view.key_w > 0.0 {
                    view.key_w += 2.0 * CELL_PAD;
                }
                view.head_h = js::max(view.label.height, js::round(font_size * 1.5)) + ROW_PAD;
                view.w = js::max_of(&[
                    view.label.width + 2.0 * CELL_PAD,
                    if view.rows.is_empty() {
                        0.0
                    } else {
                        view.key_w + value_w + 2.0 * CELL_PAD
                    },
                    72.0,
                ]);
                view.h += view.head_h;
            } else if node.actor_type.is_some() {
                view.kind = Kind::Actor;
                view.label = text(
                    Some(&node.label),
                    markdown,
                    measurer,
                    font_size,
                    140.0,
                    base,
                );
                view.w = js::max_of(&[FIGURE_W, view.label.width, view.stereotype.width]);
                view.h = FIGURE_H + FIGURE_GAP + view.stereotype.height + view.label.height;
            } else {
                view.label = text(
                    Some(&node.label),
                    markdown,
                    measurer,
                    if node.shape == "note" {
                        small
                    } else {
                        font_size
                    },
                    180.0,
                    base,
                );
                let tw = js::max(view.label.width, view.stereotype.width);
                let th = view.label.height + view.stereotype.height;
                if node.shape == "usecaseEllipse" || node.shape == "usecaseBusiness" {
                    view.kind = Kind::Ellipse;
                    // 楕円は箱より文字のまわりに場所が要り、business の楕円は斜線の場所も要る。
                    view.w = js::max(
                        tw + 2.0 * node.padding + if node.business { 16.0 } else { 4.0 },
                        88.0,
                    );
                    view.h = js::max(th + 28.0, 48.0);
                } else if node.shape == "note" {
                    view.w = tw + 2.0 * node.padding + 6.0;
                    view.h = th + 18.0;
                } else {
                    view.w = js::max(tw + 2.0 * node.padding + 12.0, 80.0);
                    view.h = js::max(th + 24.0, 44.0);
                }
            }
            c = cnode(view.w, view.h, -1);
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

    let mut cedges: Vec<CEdge> = Vec::new();
    let mut edge_labels: Vec<Label> = Vec::new();
    let mut drawn: Vec<&UsecaseGraphEdge> = Vec::new();
    for edge in &graph.edges {
        let (Some(&src), Some(&dst)) =
            (index.get(edge.start.as_str()), index.get(edge.end.as_str()))
        else {
            continue;
        };
        let semantic = edge.relationship_type == "include" || edge.relationship_type == "extend";
        let raw = if semantic {
            Some(format!(
                "«{}»",
                edge.label.as_deref().unwrap_or("undefined")
            ))
        } else {
            edge.label.clone()
        };
        let label = text(
            raw.as_deref(),
            edge.label_type == Some(LabelType::Markdown),
            measurer,
            small,
            200.0,
            0,
        );
        if src == dst {
            // 自分への関係は流れを横切って輪を描き、ノードの箱はそれを収めるだけ大きくなる。
            views[src].loops += 1;
            if sideways {
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
            minlen: edge.minlen,
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

    let title = title_label(model.title.as_deref(), measurer, size, 4000.0);
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

    // 辺が各囲みの上の縁を横切る所。題がそれをよけられるように。
    let mut crossings: Crossings = HashMap::new();
    for e in &cedges {
        mark_crossings(&mut crossings, &e.route);
    }
    attach_ends(&mut cedges, &cnodes, &views);

    let mut clusters = String::new();
    let mut fills: Vec<Fill> = Vec::new();
    let mut nodes_out = String::new();
    for (i, view) in views.iter().enumerate() {
        let c = &cnodes[i];
        let node = view.node;
        let classes = class_names(strip_default(&node.css_classes));
        let id = esc_text(&node.id);
        let text_attrs = &view.style.text;
        let shape = &view.style.shape;
        let color = ink_fill(shape, text_attrs, None);
        let muted = if color.is_empty() {
            " fill=\"var(--_m)\"".to_string()
        } else {
            color.clone()
        };
        let (w, h) = (view.w, view.h);

        if view.kind == Kind::Boundary {
            let x = c.x - c.w / 2.0;
            let y = c.y - c.h / 2.0;
            if paints(shape) {
                fills.push(Fill {
                    box_: [x, y, x + c.w, y + c.h],
                    shape: shape.clone(),
                });
            }
            let bx = format!(
                " fill=\"var(--_a)\"{} stroke=\"var(--_b)\"{shape}",
                if sets_fill(shape) {
                    ""
                } else {
                    " fill-opacity=\"0.5\""
                }
            );
            let title_attrs = format!(" class=\"pele-cluster-label\"{muted}{text_attrs}");
            let body = if node.boundary_type == Some("package") {
                let tab_w = js::min(view.label.width + 2.0 * TAB_PAD + 8.0, c.w);
                let tab_h = view.label.height + 2.0 * TAB_PAD;
                format!(
                    "<rect class=\"pele-boundary-tab\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{bx}/><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{bx}/>{}",
                    num(x),
                    num(y),
                    num(tab_w),
                    num(tab_h),
                    num(x),
                    num(y + tab_h),
                    num(c.w),
                    num(c.h - tab_h),
                    label_svg(
                        &view.label,
                        x + tab_w / 2.0,
                        y + tab_h / 2.0,
                        &title_attrs,
                        icons
                    )
                )
            } else {
                format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}/>{}",
                    num(x),
                    num(y),
                    num(c.w),
                    num(c.h),
                    rounded(&bx),
                    label_svg(
                        &view.label,
                        title_x(c, view.label.width, &crossings),
                        y + 8.0 + view.label.height / 2.0,
                        &title_attrs,
                        icons
                    )
                )
            };
            clusters.push_str(&format!(
                "<g class=\"pele-cluster pele-boundary{classes}\" data-id=\"{id}\">{body}</g>"
            ));
            continue;
        }

        #[allow(clippy::needless_late_init)]
        let body: String;
        let kind: &str;
        if view.kind == Kind::Actor {
            kind = "pele-actor";
            let top = -h / 2.0;
            let stereotype_y = top + FIGURE_H + FIGURE_GAP + view.stereotype.height / 2.0;
            body = format!(
                "<g class=\"pele-actor-figure\" transform=\"translate(0,{})\">{}</g>{}{}",
                num(top + FIGURE_H / 2.0),
                figure(view, icons),
                label_svg(
                    &view.stereotype,
                    0.0,
                    stereotype_y,
                    &format!(
                        " class=\"pele-stereotype\" font-size=\"{}\" fill=\"var(--_m)\"",
                        js::fmt(tiny)
                    ),
                    icons
                ),
                label_svg(
                    &view.label,
                    0.0,
                    h / 2.0 - view.label.height / 2.0,
                    &format!(" class=\"pele-label\"{text_attrs}"),
                    icons
                )
            );
        } else if view.kind == Kind::Json {
            kind = "pele-json";
            let x0 = -w / 2.0;
            let mut y = -h / 2.0 + view.head_h;
            let mut cells = String::new();
            let mut lines = format!("M{},{}H{}", num(x0), num(y), num(x0 + w));
            if view.key_w > 0.0 {
                lines.push_str(&format!(
                    "M{},{}V{}",
                    num(x0 + view.key_w),
                    num(y),
                    num(h / 2.0)
                ));
            }
            for (key, value) in &view.rows {
                let row_h =
                    js::max_of(&[key.height, value.height, js::round(small * 1.5)]) + ROW_PAD;
                cells.push_str(&label_svg(
                    key,
                    x0 + CELL_PAD + key.width / 2.0,
                    y + row_h / 2.0,
                    &format!(" class=\"pele-json-key\"{muted}"),
                    icons,
                ));
                cells.push_str(&left_label(
                    value,
                    x0 + view.key_w + CELL_PAD,
                    y + row_h / 2.0,
                    &format!(" class=\"pele-json-value\"{color}{text_attrs}"),
                    icons,
                ));
                y += row_h;
            }
            body = format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}/><path class=\"pele-json-lines\" d=\"{lines}\" fill=\"none\" stroke=\"var(--_b)\"{}/>{}<g font-size=\"{}\">{cells}</g>",
                num(x0),
                num(-h / 2.0),
                num(w),
                num(h),
                rounded(&format!("{SHAPE_ATTRS}{shape}")),
                view.style.line,
                label_svg(
                    &view.label,
                    0.0,
                    -h / 2.0 + view.head_h / 2.0,
                    &format!(" class=\"pele-label\" font-weight=\"var(--_hw)\"{color}{text_attrs}"),
                    icons
                ),
                js::fmt(small)
            );
        } else {
            let label_y = view.stereotype.height / 2.0;
            let mut outline: String;
            if view.kind == Kind::Ellipse {
                kind = "pele-usecase";
                outline = format!(
                    "<ellipse rx=\"{}\" ry=\"{}\"{SHAPE_ATTRS}{shape}/>",
                    num(w / 2.0),
                    num(h / 2.0)
                );
                if node.business {
                    // business の use case の斜線: 文字の横で楕円の右端を切る。
                    let from = w / 2.0 - 15.0;
                    let to = w / 2.0 - 5.0;
                    let arc = |at: f64| (h / 2.0) * (1.0 - (at / (w / 2.0)).powi(2)).sqrt();
                    outline.push_str(&format!(
                        "<path class=\"pele-business\" d=\"M{},{}L{},{}\" fill=\"none\" stroke=\"var(--_b)\"{}/>",
                        num(from),
                        num(arc(from)),
                        num(to),
                        num(-arc(to)),
                        view.style.line
                    ));
                }
            } else if node.shape == "note" {
                kind = "pele-note";
                let r = w / 2.0;
                let b = h / 2.0;
                outline = format!(
                    "<path d=\"M{},{}H{}L{},{}V{}H{}Z\" fill=\"var(--_a)\" stroke=\"var(--_b)\" stroke-linejoin=\"round\"{shape}/><path d=\"M{},{}V{}H{}\" fill=\"none\" stroke=\"var(--_b)\" stroke-linejoin=\"round\"{}/>",
                    num(-r),
                    num(-b),
                    num(r - FOLD),
                    num(r),
                    num(FOLD - b),
                    num(b),
                    num(-r),
                    num(r - FOLD),
                    num(-b),
                    num(FOLD - b),
                    num(r),
                    view.style.line
                );
            } else {
                kind = "pele-usecase";
                outline = format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}/>",
                    num(-w / 2.0),
                    num(-h / 2.0),
                    num(w),
                    num(h),
                    rounded(&format!("{SHAPE_ATTRS}{shape}"))
                );
            }
            body = outline
                + &label_svg(
                    &view.stereotype,
                    0.0,
                    -view.label.height / 2.0,
                    &format!(
                        " class=\"pele-stereotype\" font-size=\"{}\"{muted}",
                        js::fmt(tiny)
                    ),
                    icons,
                )
                + &label_svg(
                    &view.label,
                    0.0,
                    label_y,
                    &format!(
                        " class=\"pele-label\"{}{color}{text_attrs}",
                        if node.shape == "note" {
                            format!(" font-size=\"{}\"", js::fmt(small))
                        } else {
                            String::new()
                        }
                    ),
                    icons,
                );
        }
        nodes_out.push_str(&format!(
            "<g class=\"pele-node {kind}{classes}\" data-id=\"{id}\" transform=\"translate({},{})\">{body}</g>",
            num(c.x),
            num(c.y)
        ));
    }

    let mut casings = String::new();
    let mut edges_out = String::new();
    let mut labels_out = String::new();
    let mut loop_seen: HashMap<usize, usize> = HashMap::new();
    for i in 0..cedges.len() {
        let edge = drawn[i];
        let label = &edge_labels[i];
        let style = if !edge.style.is_empty() || !edge.css_compiled_styles.is_empty() {
            let mut all = edge.css_compiled_styles.clone();
            all.extend(edge.style.iter().cloned());
            Some(resolve_style(&all))
        } else {
            None
        };
        let line = style.as_ref().map_or("", |s| s.line.as_str());
        let start_type = edge.arrow_type_start;
        let end_type = edge.arrow_type_end;
        let path: EdgePath;
        let (src, dst) = (cedges[i].src, cedges[i].dst);
        if src == dst {
            let c = &cnodes[src];
            let view = &views[src];
            let k = *loop_seen.get(&src).unwrap_or(&0);
            loop_seen.insert(src, k + 1);
            let side = if sideways { 2 } else { 1 };
            let spread = js::min(side_length(view, side) / 2.0, 8.0 + k as f64 * 6.0);
            let reach = LOOP + k as f64 * 8.0;
            let (ax, ay) = anchor(view, c, side, -spread);
            let (bx, by) = anchor(view, c, side, spread);
            let len = js::hypot(reach, spread);
            let len = if js::truthy(len) { len } else { 1.0 };
            let ux = if sideways { spread / len } else { reach / len };
            let uy = if sideways { reach / len } else { spread / len };
            let s_trim = trim(start_type);
            let e_trim = trim(end_type);
            // 両端は互いから離れる向きにノードを出て、曲線はその外で 2 つを結ぶ。
            let out = |x: f64, y: f64, sign: f64, far: f64| -> String {
                let f = if js::truthy(far) { 1.0 } else { 0.0 };
                if sideways {
                    format!("{},{}", num(x + sign * spread * f), num(y + far))
                } else {
                    format!("{},{}", num(x + far), num(y + sign * spread * f))
                }
            };
            let sx = if sideways {
                ax - ux * s_trim
            } else {
                ax + ux * s_trim
            };
            let sy = if sideways {
                ay + uy * s_trim
            } else {
                ay - uy * s_trim
            };
            let ex = bx + ux * e_trim;
            let ey = by + uy * e_trim;
            path = EdgePath {
                d: format!(
                    "M{},{}C{} {} {},{}",
                    num(sx),
                    num(sy),
                    out(ax, ay, -1.0, reach),
                    out(bx, by, 1.0, reach),
                    num(ex),
                    num(ey)
                ),
                tokens: None,
                sx: ax,
                sy: ay,
                sdx: if sideways { ux } else { -ux },
                sdy: if sideways { -uy } else { uy },
                ex: bx,
                ey: by,
                edx: -ux,
                edy: -uy,
            };
            let ce = &mut cedges[i];
            if sideways {
                ce.label_x = c.x;
                ce.label_y = js::max(ay, by) + reach * 0.75 + 6.0 + label.height / 2.0;
            } else {
                ce.label_x = js::max(ax, bx) + reach * 0.75 + 8.0 + label.width / 2.0;
                ce.label_y = c.y;
            }
        } else {
            if cedges[i].route.len() < 6 {
                continue;
            }
            path = route_path(
                &cedges[i].route,
                None,
                trim(start_type),
                trim(end_type),
                None,
                None,
            );
        }
        let ce = &cedges[i];
        let under = fill_under(
            &path.d,
            &[path.sx, path.sy, path.ex, path.ey],
            &fills,
            path.tokens.as_deref(),
        );
        let over: Option<String> = match &under {
            Under::In(f) => Some(edge_ink(f, line)),
            Under::Crosses => Some(String::new()),
            Under::Off => None,
        };
        let color = match over.as_deref() {
            Some(o) if !o.is_empty() => o.to_string(),
            _ => match style.as_ref().and_then(|s| s.stroke.as_deref()) {
                Some(s) if !s.is_empty() => esc(s),
                _ => "var(--_l)".to_string(),
            },
        };
        if over.as_deref() == Some("") {
            let bg = "var(--_bg)";
            casings.push_str(&casing(
                &path.d,
                &(mark(start_type, path.sx, path.sy, path.sdx, path.sdy, bg, bg)
                    + &mark(end_type, path.ex, path.ey, path.edx, path.edy, bg, bg)),
                line,
                false,
            ));
        }
        // Mermaid はこの破線を動かす。止まった絵は破線を保つ。
        let dashes = if edge.internal {
            "2 3"
        } else if edge.pattern == "dotted" {
            "5 4"
        } else if edge.animate {
            "9 5"
        } else {
            ""
        };
        let marks = mark(
            start_type, path.sx, path.sy, path.sdx, path.sdy, &color, "none",
        ) + &mark(
            end_type, path.ex, path.ey, path.edx, path.edy, &color, "none",
        );
        let over_text = over.as_deref().unwrap_or("");
        edges_out.push_str(&format!(
            "<g class=\"pele-edge pele-{}{}\" data-id=\"{}\"{}{line}><path d=\"{}\"{}/>{}</g>",
            edge.relationship_type,
            class_names(strip_relationship(&edge.classes)),
            esc_text(&edge.id),
            if over_text.is_empty() {
                String::new()
            } else {
                format!(" stroke=\"{over_text}\"")
            },
            path.d,
            if dashes.is_empty() {
                String::new()
            } else {
                format!(" stroke-dasharray=\"{dashes}\"")
            },
            match &under {
                Under::In(f) if !over_text.is_empty() => hollow_on(&marks, f),
                _ => marks,
            }
        ));
        if label.width > 0.0 {
            let semantic =
                edge.relationship_type == "include" || edge.relationship_type == "extend";
            labels_out.push_str(&edge_label_svg(
                &edge.id,
                label,
                ce.label_x,
                ce.label_y,
                &format!(
                    "{}{}",
                    if semantic { " fill=\"var(--_m)\"" } else { "" },
                    style.as_ref().map_or("", |s| s.text.as_str())
                ),
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
        content.push_str(&format!(
            "<g class=\"pele-edges\" fill=\"none\" stroke=\"var(--_l)\" stroke-linecap=\"round\">{edges_out}</g>"
        ));
    }
    if !labels_out.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-edge-labels\" font-size=\"{}\">{labels_out}</g>",
            js::fmt(small)
        ));
    }
    content.push_str(&format!("<g class=\"pele-nodes\">{nodes_out}</g>"));
    let acc = Accessible {
        acc_title: Some(&model.acc_title),
        acc_descr: Some(&model.acc_descr),
    };
    let svg = svg_document("usecase", width, height, size, options, &acc, &content);
    Rendered {
        svg,
        width,
        height,
        links: Vec::new(),
    }
}

/// 囲みの題は上の縁の真ん中に置く。辺がその縁から入る所では、真ん中に最も近い辺の間に、どこにも場所がなければ
/// 最も広い間に動く。
fn title_x(c: &CNode, width: f64, crossings: &Crossings) -> f64 {
    let xs = crossings.get(&(js::round(c.y - c.h / 2.0) as i64));
    let Some(xs) = xs.filter(|_| width != 0.0) else {
        return c.x;
    };
    let left = c.x - c.w / 2.0 + 6.0;
    let right = c.x + c.w / 2.0 - 6.0;
    let mut cuts: Vec<f64> = xs
        .iter()
        .copied()
        .filter(|&x| x > left && x < right)
        .collect();
    cuts.sort_by(|a, b| js::order(a - b));
    cuts.push(right);
    let need = width / 2.0 + 6.0;
    let mut best = c.x;
    let mut best_off = f64::INFINITY;
    let mut widest = 0.0;
    let mut from = left;
    for cut in cuts {
        if cut - from >= 2.0 * need {
            let x = js::min(js::max(c.x, from + need), cut - need);
            if (x - c.x).abs() < best_off {
                best_off = (x - c.x).abs();
                best = x;
            }
        } else if best_off == f64::INFINITY && cut - from > widest {
            widest = cut - from;
            best = (from + cut) / 2.0;
        }
        from = cut;
    }
    best
}

fn left_label(label: &Label, x: f64, cy: f64, attrs: &str, icons: Option<&IconResolver>) -> String {
    if label.lines.len() < 2 {
        return label_svg(label, x + label.width / 2.0, cy, attrs, icons);
    }
    let mut out = String::new();
    for i in 0..label.lines.len() {
        out.push_str(&label_svg(
            &label_line(label, i),
            x + label.widths[i] / 2.0,
            cy - label.height / 2.0 + line_middle(label, i),
            attrs,
            icons,
        ));
    }
    out
}
