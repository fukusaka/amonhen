//! er を描く（pele の `src/diagrams/er/render.ts`）。

use std::collections::HashMap;

use super::db::ErDb;
use super::graph::{ErGraphEdge, build_er_graph};
use super::markers::{MARKER_LENGTH, er_marker};
use crate::diagrams::common::fit_width::{runs_across, turn_to_fit};
use crate::diagrams::common::generics::parse_generic_types;
use crate::errors::Result;
use crate::js;
use crate::layout::compound::{CEdge, CNode, Dir, cnode, compound_layout, direction, shift_layout};
use crate::layout::layered::LayeredOptions;
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num, shared_style, span_style, text_style};
use crate::svg::cluster::{Crossings, cluster_title_x, mark_crossings};
use crate::svg::edges::{
    Fill, Under, casing, casings_svg, edge_ink, edge_label_svg, fill_under, hollow_on, route_path,
};
use crate::svg::ink::{ON_DARK, ON_LIGHT, ink, ink_fill, paints};
use crate::svg::root::{Accessible, svg_document};
use crate::svg::theme::{ROUNDED, ResolvedStyle, class_names, resolve_style, rounded, sets_fill};
use crate::svg::title::title_label;
use crate::text::entities::decode_entities;
use crate::text::label::{Label, has_math, label_line, layout_label, line_middle};
use crate::text::measurer::{TextMeasurer, default_measurer, style};
use crate::types::{IconResolver, LinkInfo, RenderOptions, Rendered};
use crate::util::url::{internal_link, rel_attr};
use crate::util::yaml::Value;

const NODE_SEP: f64 = 48.0;
const EDGE_SEP: f64 = 20.0;
const RANK_SEP: f64 = 64.0;
const GROUP_PAD: f64 = 20.0;
const LOOP: f64 = 28.0;
const LOOP_BAND: f64 = 38.0;
const MIN_WIDTH: f64 = 96.0;
const BOX_PAD_X: f64 = 18.0;
const BOX_PAD_Y: f64 = 12.0;
const HEAD_PAD_Y: f64 = 9.0;
const CELL_PAD_X: f64 = 12.0;
const CELL_GAP: f64 = 16.0;
const ROW_PAD: f64 = 9.0;
const COMMENT_WRAP: f64 = 260.0;
const NO_WRAP: f64 = 1e6;
const PORT_GAP: f64 = 18.0;
const PORT_MARGIN: f64 = 14.0;
const BOX_ATTRS: &str = " fill=\"var(--_s)\" stroke=\"var(--_b)\"";

fn number_option(config: &Config, key: &str, fallback: f64) -> f64 {
    match config.get("er") {
        Some(Value::Obj(s)) => match s.get(key) {
            Some(Value::Num(v)) if *v > 0.0 => js::min(*v, 2000.0),
            _ => fallback,
        },
        _ => fallback,
    }
}

struct Row {
    /// 型、名前、キー、コメント。
    cells: [Label; 4],
    h: f64,
}

struct NodeView {
    style: ResolvedStyle,
    label: Label,
    w: f64,
    h: f64,
    head_h: f64,
    rows: Vec<Row>,
    /// 各列の左端。箱の左端から測る。
    cols: Vec<f64>,
    row_size: f64,
    key_size: f64,
    mono: u32,
    /// 自分への関係が右ではなく下の辺から輪を出すか。
    below: bool,
}

/// 実体から自分への関係と、それが輪を出す辺の受け持ち。
struct Loop {
    label: Label,
    band: f64,
    at: f64,
}

/// 測ったラベルを、x から始め、最初の行の基線を y にして書く。
fn text_left(
    label: &Label,
    x: f64,
    mut y: f64,
    attrs: &str,
    icons: Option<&IconResolver>,
    mono: u32,
) -> String {
    let lines = &label.lines;
    if lines.is_empty() {
        return String::new();
    }
    // 短い形は等幅の文字の続く空白を詰めてしまう。
    if lines.len() == 1
        && lines[0].len() == 1
        && let only = &lines[0][0]
        && only.icon.is_none()
        && only.math.is_none()
        && only.style == mono
        && !(mono != 0 && only.text.contains("  "))
    {
        return format!(
            "<text{} x=\"{}\" y=\"{}\">{}</text>",
            text_style(attrs, mono),
            num(x),
            num(y),
            esc(&only.text)
        );
    }
    let shared = shared_style(label);
    let shift = label.size * 0.35;
    let top = y - shift - label.line_height / 2.0;
    let mut body = String::new();
    let mut extra = String::new();
    for (i, line) in lines.iter().enumerate() {
        let middle = top + line_middle(label, i);
        if has_math(line) {
            extra.push_str(&label_svg(
                &label_line(label, i),
                x + label.widths[i] / 2.0,
                middle,
                attrs,
                icons,
            ));
            continue;
        }
        y = middle + shift;
        let has_icon = line.iter().any(|s| s.icon.is_some());
        let mut sx = x;
        let mut placed = false;
        for span in line {
            if let Some(icon) = &span.icon {
                let side = label.size * 1.1;
                extra.push_str(&format!(
                    "<svg class=\"pele-icon\" data-icon=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\">{}</svg>",
                    esc_text(icon),
                    num(sx),
                    num(y - label.size * 0.35 - side / 2.0),
                    num(side),
                    num(side),
                    icons.and_then(|f| f(icon)).unwrap_or_default()
                ));
            } else if !span.text.is_empty() {
                let pos = if has_icon || !placed {
                    format!(" x=\"{}\" y=\"{}\"", num(sx), num(y))
                } else {
                    String::new()
                };
                body.push_str(&format!(
                    "<tspan{pos}{}>{}</tspan>",
                    span_style(span.style & !shared),
                    esc(&span.text)
                ));
                placed = true;
            }
            sx += span.width;
        }
    }
    format!(
        "<text{} xml:space=\"preserve\">{body}</text>{extra}",
        text_style(attrs, shared)
    )
}

/// Mermaid は `~` の対を山括弧と読む。ラベルの読み手が `<int>` をタグと取らないよう、符号で書く。
fn cell_text(raw: &str) -> String {
    let converted = parse_generic_types(raw);
    if converted == raw {
        raw.to_string()
    } else {
        converted.replace('<', "&lt;").replace('>', "&gt;")
    }
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

pub fn render_er(db: &ErDb, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    if !runs_across(Some(&db.direction)) {
        return Ok(draw(db, config, options, false));
    }
    turn_to_fit(options, |down| Ok(draw(db, config, options, down)))
}

/// `turned` は横に流れる図を縦に流して描く。
fn draw(db: &ErDb, config: &Config, options: &RenderOptions, turned: bool) -> Rendered {
    let graph = build_er_graph(db);
    let monospaced = options
        .monospace
        .as_ref()
        .is_some_and(|m| m.iter().any(|s| s == "attributes"));
    let size = options.font_size.unwrap_or(16.0);
    let small_size = js::round(size * 0.875);
    let measurer: &dyn TextMeasurer = match &options.measurer {
        Some(m) => m.as_ref(),
        None => default_measurer(),
    };
    let pad = options.padding.unwrap_or(8.0);
    let icons = options.icons.as_ref();

    let mut index: HashMap<&str, usize> = HashMap::new();
    for (i, node) in graph.nodes.iter().enumerate() {
        index.insert(&node.id, i);
    }
    let mut views: Vec<NodeView> = Vec::new();
    let mut cnodes: Vec<CNode> = Vec::new();

    let mut cedges: Vec<CEdge> = Vec::new();
    let mut edge_labels: Vec<Label> = Vec::new();
    let mut drawn: Vec<&ErGraphEdge> = Vec::new();
    let mut all_loops: Vec<Loop> = Vec::new();
    let mut loops: Vec<(usize, Vec<usize>)> = Vec::new();
    let mut loops_at: HashMap<usize, usize> = HashMap::new();
    let mut loop_of: Vec<Option<usize>> = Vec::new();
    for edge in &graph.edges {
        let (Some(&src), Some(&dst)) =
            (index.get(edge.start.as_str()), index.get(edge.end.as_str()))
        else {
            continue;
        };
        let label = layout_label(Some(&edge.label), true, measurer, small_size, 200.0, 0);
        let mut lp = None;
        if src == dst {
            let k = all_loops.len();
            all_loops.push(Loop {
                label: label.clone(),
                band: 0.0,
                at: 0.0,
            });
            match loops_at.get(&src) {
                Some(&at) => loops[at].1.push(k),
                None => {
                    loops_at.insert(src, loops.len());
                    loops.push((src, vec![k]));
                }
            }
            lp = Some(k);
        }
        loop_of.push(lp);
        drawn.push(edge);
        cedges.push(CEdge {
            src,
            dst,
            minlen: 1.0,
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

    // ノードを配置する段の向き: 向きを決める最も近いまとまりのもの、なければ図のもの。
    let horizontal = |i: usize| -> bool {
        let mut at = i;
        // まとまりは互いを中身に挙げうるので、上へのたどりに限りをつける。
        for _ in 0..graph.nodes.len() {
            let Some(up) = graph.nodes[at]
                .parent_id
                .as_deref()
                .and_then(|p| index.get(p).copied())
            else {
                break;
            };
            if let Some(dir) = graph.nodes[up].dir.as_deref().filter(|d| !d.is_empty()) {
                return dir == "LR" || dir == "RL";
            }
            at = up;
        }
        db.direction == "LR" || db.direction == "RL"
    };

    for (i, node) in graph.nodes.iter().enumerate() {
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
        let base =
            (if st.bold { style::BOLD } else { 0 }) | (if st.italic { style::ITALIC } else { 0 });
        let mono = if monospaced
            && !(st.text.contains("\"font-family:") || st.text.contains(";font-family:"))
        {
            style::MONO
        } else {
            0
        };
        let name_size = st.font_size.unwrap_or(size);
        let row_size = st.font_size.unwrap_or(small_size);
        let key_size = st.font_size.unwrap_or(js::round(size * 0.75));
        let label = if node.is_group {
            layout_label(Some(&node.label), true, measurer, name_size, 4000.0, base)
        } else {
            let text = if node.alias.is_empty() {
                &node.label
            } else {
                &node.alias
            };
            layout_label(Some(text), true, measurer, name_size, 240.0, base)
        };
        let own: &[usize] = loops_at.get(&i).map_or(&[], |&k| &loops[k].1);
        // 輪は、配置がほかの辺を通さない辺から出る: ランクが下に流れるなら右、横に流れるなら下。
        let below = !own.is_empty() && !node.is_group && horizontal(i);
        let mut view = NodeView {
            style: st,
            label,
            w: 0.0,
            h: 0.0,
            head_h: 0.0,
            rows: Vec::new(),
            cols: Vec::new(),
            row_size,
            key_size,
            mono,
            below,
        };
        let mut reach: f64 = 0.0;
        let mut bands = 0.0;
        for &k in own {
            let lp = &mut all_loops[k];
            lp.band = js::max(
                LOOP_BAND,
                if below {
                    lp.label.width + 12.0
                } else {
                    lp.label.height + 6.0
                },
            );
            bands += lp.band;
            reach = js::max(
                reach,
                if below {
                    lp.label.height
                } else {
                    lp.label.width
                },
            );
        }
        let min_w = js::max(MIN_WIDTH, if below { bands + 8.0 } else { 0.0 });
        let mut c;
        if node.is_group {
            c = cnode(0.0, 0.0, -1);
            c.is_group = true;
            c.dir = node.dir.as_deref().filter(|d| !d.is_empty()).map(direction);
            c.pad_x = GROUP_PAD;
            c.pad_top = if view.label.height > 0.0 {
                view.label.height + 16.0
            } else {
                GROUP_PAD
            };
            c.pad_bottom = GROUP_PAD;
            c.min_w = js::max(view.label.width + 2.0 * GROUP_PAD - 12.0, MIN_WIDTH);
        } else {
            let name_h = js::max(view.label.height, js::round(name_size * 1.5));
            if node.attributes.is_empty() {
                view.w = js::max(view.label.width + 2.0 * BOX_PAD_X, min_w);
                view.h = js::max(
                    name_h + 2.0 * BOX_PAD_Y,
                    if below { 0.0 } else { bands + 8.0 },
                );
            } else {
                let row_line = js::round(row_size * 1.5);
                let mut widths = [0.0f64; 4];
                for attribute in node.attributes {
                    let cells = [
                        layout_label(
                            Some(&cell_text(&attribute.type_)),
                            true,
                            measurer,
                            row_size,
                            NO_WRAP,
                            base | mono,
                        ),
                        layout_label(
                            Some(&cell_text(&attribute.name)),
                            true,
                            measurer,
                            row_size,
                            NO_WRAP,
                            base | mono,
                        ),
                        layout_label(
                            Some(&attribute.keys.join(", ")),
                            false,
                            measurer,
                            key_size,
                            NO_WRAP,
                            base | mono,
                        ),
                        layout_label(
                            Some(&cell_text(&attribute.comment)),
                            true,
                            measurer,
                            row_size,
                            COMMENT_WRAP,
                            base,
                        ),
                    ];
                    let mut h = row_line;
                    for k in 0..4 {
                        if cells[k].width > widths[k] {
                            widths[k] = cells[k].width;
                        }
                        if cells[k].height > h {
                            h = cells[k].height;
                        }
                    }
                    view.rows.push(Row {
                        cells,
                        h: h + ROW_PAD,
                    });
                    view.h += h + ROW_PAD;
                }
                let mut columns = 0.0;
                let mut inner = 0.0;
                for &w in &widths {
                    if w == 0.0 {
                        continue;
                    }
                    inner += w + if columns > 0.0 { CELL_GAP } else { 0.0 };
                    columns += 1.0;
                }
                view.w = js::max_of(&[
                    inner + 2.0 * CELL_PAD_X,
                    view.label.width + 2.0 * BOX_PAD_X,
                    min_w,
                ]);
                // 表より広い箱は、余りの幅を列で分ける。
                let spare = if columns > 0.0 {
                    (view.w - inner - 2.0 * CELL_PAD_X) / columns
                } else {
                    0.0
                };
                let mut x = CELL_PAD_X;
                for &w in &widths {
                    view.cols.push(x);
                    if w > 0.0 {
                        x += w + spare + CELL_GAP;
                    }
                }
                view.head_h = name_h + 2.0 * HEAD_PAD_Y;
                view.h += view.head_h;
            }
            // 配置はノードを箱の真ん中に保つので、輪の余地は両側に足す。
            let room = if !own.is_empty() {
                2.0 * (MARKER_LENGTH + LOOP * 0.75 + 10.0 + reach)
            } else {
                0.0
            };
            c = if below {
                cnode(view.w, view.h + room, -1)
            } else {
                cnode(view.w + room, view.h, -1)
            };
        }
        c.seq = views.len() as f64;
        views.push(view);
        cnodes.push(c);
    }
    for (i, node) in graph.nodes.iter().enumerate() {
        cnodes[i].parent = node
            .parent_id
            .as_deref()
            .and_then(|p| index.get(p))
            .map_or(-1, |&k| k as i64);
    }

    let layout = compound_layout(
        &mut cnodes,
        &mut cedges,
        if turned {
            Dir::TB
        } else {
            direction(&db.direction)
        },
        &LayeredOptions {
            node_sep: number_option(config, "nodeSpacing", NODE_SEP),
            edge_sep: EDGE_SEP,
            rank_sep: number_option(config, "rankSpacing", RANK_SEP),
            port_sep: 24.0,
            ..Default::default()
        },
    );

    spread_ends(&mut cedges, &cnodes, &views);

    // まとまりの大きさは配置で決まるので、その輪は外に下がり、図を広げうる。
    let mut layout_width = layout.width;
    for (i, own) in &loops {
        let c = &cnodes[*i];
        if !c.is_group {
            continue;
        }
        let mut reach: f64 = 0.0;
        for &k in own {
            reach = js::max(reach, all_loops[k].label.width);
        }
        layout_width = js::max(
            layout_width,
            c.x + c.w / 2.0 + MARKER_LENGTH + LOOP * 0.75 + 10.0 + reach,
        );
    }

    let title = title_label(db.title.as_deref(), measurer, size, 4000.0);
    let title_height = if title.height > 0.0 {
        title.height + 12.0
    } else {
        0.0
    };
    let inner = js::max(layout_width, title.width);
    let ox = pad + (inner - layout_width) / 2.0;
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
    let mut fills: Vec<Fill> = Vec::new();
    let mut nodes_out = String::new();
    for i in 0..views.len() {
        let c = &cnodes[i];
        let node = &graph.nodes[i];
        let view = &mut views[i];
        let classes = class_names(strip_default(&node.css_classes));
        if c.is_group {
            view.w = c.w;
            view.h = c.h;
            let x = c.x - c.w / 2.0;
            let y = c.y - c.h / 2.0;
            let title_x = cluster_title_x(c, view.label.width, &crossings, GROUP_PAD);
            if paints(&view.style.shape) {
                fills.push(Fill {
                    box_: [x, y, x + c.w, y + c.h],
                    shape: view.style.shape.clone(),
                });
            }
            clusters.push_str(&format!(
                "<g class=\"pele-cluster{classes}\" data-id=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_a)\"{} stroke=\"var(--_b)\"{}/>{}</g>",
                esc_text(&node.id),
                num(x),
                num(y),
                num(c.w),
                num(c.h),
                if sets_fill(&view.style.shape) {
                    ""
                } else {
                    " fill-opacity=\"0.5\""
                },
                rounded(&view.style.shape),
                label_svg(
                    &view.label,
                    title_x,
                    y + 8.0 + view.label.height / 2.0,
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
        let x0 = -w / 2.0;
        let y0 = -h / 2.0;
        let text = &view.style.text;
        let color = ink_fill(&view.style.shape, &view.style.text, None);
        let muted = if color.is_empty() {
            " fill=\"var(--_m)\"".to_string()
        } else {
            color.clone()
        };
        let shape = format!(
            "x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"",
            num(x0),
            num(y0),
            num(w),
            num(h)
        );
        let mut name = label_svg(
            &view.label,
            0.0,
            if !view.rows.is_empty() {
                y0 + view.head_h / 2.0
            } else {
                0.0
            },
            &format!(" class=\"pele-label\"{color}{text}"),
            icons,
        );
        let display = decode_entities(if node.alias.is_empty() {
            &node.label
        } else {
            &node.alias
        });
        if format!("{classes} ").contains(" internal-link ")
            && !name.is_empty()
            && options.links != Some(false)
        {
            // 名前は href に入るので、URL と同じ確かめをする。
            let safe = internal_link(&display, options);
            let target = esc(&display);
            name = format!(
                "<a class=\"internal-link\"{} data-href=\"{target}\">{name}</a>",
                if safe {
                    format!(" href=\"{target}\"{}", rel_attr(options))
                } else {
                    String::new()
                }
            );
            links.push(LinkInfo {
                id: node.label.clone(),
                href: display.clone(),
                internal: true,
            });
        }

        let body = if view.rows.is_empty() {
            format!(
                "<rect {shape}{}/>{name}",
                rounded(&format!("{BOX_ATTRS}{}", view.style.shape))
            )
        } else {
            let row_line = js::round(view.row_size * 1.5);
            let line_color = match &view.style.stroke {
                Some(s) if !s.is_empty() => esc(s),
                _ => "var(--_b)".into(),
            };
            let mut stripes = String::new();
            let mut cells = String::new();
            let mut y = y0 + view.head_h;
            for (r, row) in view.rows.iter().enumerate() {
                if r % 2 == 0 {
                    let span = format!("x=\"{}\" width=\"{}\"", num(x0), num(w));
                    if r == view.rows.len() - 1 {
                        // 最後の行は箱の丸い下の角に沿う: 丸い角の四角を、上だけ角にする。
                        stripes.push_str(&format!(
                            "<rect {span} y=\"{}\" height=\"{}\"{ROUNDED}/><rect {span} y=\"{}\" height=\"{}\"/>",
                            num(y),
                            num(row.h),
                            num(y),
                            num(row.h / 2.0)
                        ));
                    } else {
                        stripes.push_str(&format!(
                            "<rect {span} y=\"{}\" height=\"{}\"/>",
                            num(y),
                            num(row.h)
                        ));
                    }
                }
                let baseline = y + ROW_PAD / 2.0 + row_line / 2.0 + view.row_size * 0.35;
                let [type_, attribute, keys, comment] = &row.cells;
                cells.push_str(&text_left(
                    type_,
                    x0 + view.cols[0],
                    baseline,
                    &format!(" class=\"pele-er-type\"{muted}{text}"),
                    icons,
                    view.mono,
                ));
                cells.push_str(&text_left(
                    attribute,
                    x0 + view.cols[1],
                    baseline,
                    &format!(" class=\"pele-er-name\"{color}{text}"),
                    icons,
                    view.mono,
                ));
                cells.push_str(&text_left(
                    keys,
                    x0 + view.cols[2],
                    baseline,
                    &format!(
                        " class=\"pele-er-keys\" font-size=\"{}\"{muted}{text}",
                        js::fmt(view.key_size)
                    ),
                    icons,
                    view.mono,
                ));
                cells.push_str(&text_left(
                    comment,
                    x0 + view.cols[3],
                    baseline,
                    &format!(" class=\"pele-er-comment\"{muted}{text}"),
                    icons,
                    0,
                ));
                y += row.h;
            }
            let divider_y = num(y0 + view.head_h);
            // 明るい塗りの上の行の影は暗く、暗い塗りの上では明るくして、文字が読めるように。
            let tone = ink(&view.style.shape, "");
            let shade = if tone == Some(ON_LIGHT) {
                ON_DARK
            } else if tone == Some(ON_DARK) {
                ON_LIGHT
            } else {
                "var(--_bg)"
            };
            // 図の選んだ塗りの上では、影はその塗りの色合い。最後の行の 2 つの部分が重なるので、不透明度は
            // まとまりにつける。
            format!(
                "<rect {shape}{}/><g class=\"pele-er-stripes\" fill=\"{shade}\"{}>{stripes}</g><path class=\"pele-er-divider\" d=\"M{},{divider_y}H{}\" stroke=\"{line_color}\"/><rect {shape} fill=\"none\" stroke=\"var(--_b)\"{}/>{name}<g class=\"pele-er-attributes\" font-size=\"{}\">{cells}</g>",
                rounded(&format!("{BOX_ATTRS}{}", view.style.shape)),
                if view.style.shape.contains("fill:") {
                    " opacity=\"0.6\""
                } else {
                    ""
                },
                num(x0),
                num(x0 + w),
                rounded(&view.style.line),
                js::fmt(view.row_size)
            )
        };
        nodes_out.push_str(&format!(
            "<g class=\"pele-node pele-entity{classes}\" data-id=\"{}\" transform=\"translate({},{})\">{body}</g>",
            esc_text(&node.label),
            num(c.x),
            num(c.y)
        ));
    }

    // 自分への関係は輪を出す辺を分け合い、短すぎれば詰め合う。
    for (i, own) in &loops {
        let view = &views[*i];
        let mut total = 0.0;
        for &k in own {
            total += all_loops[k].band;
        }
        let scale = js::min(
            1.0,
            js::max((if view.below { view.w } else { view.h }) - 8.0, 0.0) / total,
        );
        let mut at = (-total * scale) / 2.0;
        for &k in own {
            let lp = &mut all_loops[k];
            lp.band *= scale;
            lp.at = at + lp.band / 2.0;
            at += lp.band;
        }
    }

    let mut edges_out = String::new();
    let mut casings = String::new();
    let mut labels_out = String::new();
    for i in 0..cedges.len() {
        let edge = drawn[i];
        let label = &edge_labels[i];
        let d: String;
        let mut start: String;
        let mut end: String;
        if let Some(k) = loop_of[i] {
            let lp = &all_loops[k];
            let src = cedges[i].src;
            let c = &cnodes[src];
            let view = &views[src];
            let spread = js::max(js::min(lp.band / 2.0 - 6.0, 13.0), 1.0);
            let far = MARKER_LENGTH + LOOP * 0.75 + 6.0;
            if view.below {
                let x = c.x + lp.at;
                let y = c.y + view.h / 2.0;
                let turn = num(y + MARKER_LENGTH);
                let out = num(y + MARKER_LENGTH + LOOP);
                d = format!(
                    "M{},{}V{turn}C{},{out} {},{out} {},{turn}V{}",
                    num(x - spread),
                    num(y),
                    num(x - spread),
                    num(x + spread),
                    num(x + spread),
                    num(y)
                );
                start = er_marker(&edge.arrow_type_start, x - spread, y, 0.0, -1.0);
                end = er_marker(&edge.arrow_type_end, x + spread, y, 0.0, -1.0);
                cedges[i].label_x = x;
                cedges[i].label_y = y + far + label.height / 2.0;
            } else {
                let x = c.x + view.w / 2.0;
                let y = c.y + lp.at;
                let turn = num(x + MARKER_LENGTH);
                let out = num(x + MARKER_LENGTH + LOOP);
                d = format!(
                    "M{},{}H{turn}C{out},{} {out},{} {turn},{}H{}",
                    num(x),
                    num(y - spread),
                    num(y - spread),
                    num(y + spread),
                    num(y + spread),
                    num(x)
                );
                start = er_marker(&edge.arrow_type_start, x, y - spread, -1.0, 0.0);
                end = er_marker(&edge.arrow_type_end, x, y + spread, -1.0, 0.0);
                cedges[i].label_x = x + far + label.width / 2.0;
                cedges[i].label_y = y;
            }
        } else {
            let mut route = straighten(&cedges[i].route);
            if route.len() < 6 {
                continue;
            }
            let (src, dst) = (cedges[i].src, cedges[i].dst);
            inset(&mut route, 0, &views[src], &cnodes[src]);
            let last = route.len() - 3;
            inset(&mut route, last, &views[dst], &cnodes[dst]);
            // 曲がる線は箱の手前で止め、印がまっすぐな線の上に乗るように。
            let dx = (route[route.len() - 3] - route[0]).abs();
            let dy = (route[route.len() - 2] - route[1]).abs();
            let direct = route.len() == 6
                && (dx < 0.01 || dy < 0.01 || js::hypot(dx, dy) <= 3.0 * MARKER_LENGTH);
            let stub = if direct { 0.0 } else { MARKER_LENGTH };
            let path = route_path(&route, None, stub, stub, None, None);
            d = if stub > 0.0 {
                format!(
                    "M{},{}L{}L{},{}",
                    num(path.sx),
                    num(path.sy),
                    &path.d[1..],
                    num(path.ex),
                    num(path.ey)
                )
            } else {
                path.d.clone()
            };
            start = er_marker(&edge.arrow_type_start, path.sx, path.sy, path.sdx, path.sdy);
            end = er_marker(&edge.arrow_type_end, path.ex, path.ey, path.edx, path.edy);
        }
        let under = fill_under(&d, &[], &fills, None);
        let over: Option<String> = match &under {
            Under::In(f) => Some(edge_ink(f, "")),
            Under::Crosses => Some(String::new()),
            Under::Off => None,
        };
        if over.as_deref() == Some("") {
            casings.push_str(&casing(&d, &format!("{start}{end}"), "", false));
        }
        if let Under::In(f) = &under
            && over.as_deref().is_some_and(|o| !o.is_empty())
        {
            start = hollow_on(&start, f);
            end = hollow_on(&end, f);
        }
        let dashed = edge.dashed;
        edges_out.push_str(&format!(
            "<g class=\"pele-edge pele-er-{}\" data-id=\"{}\"{}><path d=\"{d}\"{}/>{start}{end}</g>",
            if dashed {
                "non-identifying"
            } else {
                "identifying"
            },
            esc_text(&edge.id),
            match over.as_deref() {
                Some(o) if !o.is_empty() => format!(" stroke=\"{o}\""),
                _ => String::new(),
            },
            if dashed {
                " stroke-dasharray=\"4 3\""
            } else {
                ""
            }
        ));
        if label.width > 0.0 {
            labels_out.push_str(&edge_label_svg(
                &edge.id,
                label,
                cedges[i].label_x,
                cedges[i].label_y,
                "",
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
            js::fmt(small_size)
        ));
    }
    content.push_str(&format!("<g class=\"pele-nodes\">{nodes_out}</g>"));
    let acc = Accessible {
        acc_title: db.acc_title.as_deref(),
        acc_descr: db.acc_descr.as_deref(),
    };
    let svg = svg_document("er", width, height, size, options, &acc, &content);
    Rendered {
        svg,
        width,
        height,
        links,
    }
}

fn straighten(route: &[f64]) -> Vec<f64> {
    let mut out: Vec<f64> = route[..3.min(route.len())].to_vec();
    let mut k = 3;
    while k < route.len() {
        let prev = out.len() - 3;
        let flat = |axis: usize| -> bool {
            (out[prev + axis] - route[k + axis]).abs() < 0.01
                && (route[k + axis] - route[k + 3 + axis]).abs() < 0.01
        };
        if k + 3 < route.len() && (flat(0) || flat(1)) {
            k += 3;
            continue;
        }
        out.extend_from_slice(&route[k..k + 3]);
        k += 3;
    }
    out
}

struct End {
    edge: usize,
    at: usize,
    toward: f64,
}

/// 配置は箱の 1 辺に着く辺をみなその辺の真ん中に連れてきて、印が重なる。それを、広がる順に辺に沿って離す。
fn spread_ends(cedges: &mut [CEdge], cnodes: &[CNode], views: &[NodeView]) {
    let mut sides: Vec<(usize, Vec<End>)> = Vec::new();
    let mut side_of: HashMap<usize, usize> = HashMap::new();
    let mut add = |node: usize, edge: usize, route: &[f64], at: usize, next: usize| {
        let c = &cnodes[node];
        let dx = route[at] - c.x;
        let dy = route[at + 1] - c.y;
        let side = if (dy.abs() - c.h / 2.0).abs() < 0.5 {
            if dy < 0.0 { 0 } else { 2 }
        } else if (dx.abs() - c.w / 2.0).abs() < 0.5 {
            if dx < 0.0 { 3 } else { 1 }
        } else {
            return;
        };
        let key = node * 4 + side;
        let end = End {
            edge,
            at,
            toward: route[next + (side & 1)],
        };
        match side_of.get(&key) {
            Some(&k) => sides[k].1.push(end),
            None => {
                side_of.insert(key, sides.len());
                sides.push((key, vec![end]));
            }
        }
    };
    for (ei, e) in cedges.iter().enumerate() {
        if e.src == e.dst || e.route.len() < 6 {
            continue;
        }
        let last = e.route.len() - 3;
        add(e.src, ei, &e.route, 0, 3);
        add(e.dst, ei, &e.route, last, last - 3);
    }
    for (key, ends) in &mut sides {
        if ends.len() < 2 {
            continue;
        }
        let node = *key >> 2;
        let vertical = *key & 1 == 1;
        let (bw, bh) = if cnodes[node].is_group {
            (cnodes[node].w, cnodes[node].h)
        } else {
            (views[node].w, views[node].h)
        };
        let length = if vertical { bh } else { bw };
        let step = js::max(
            js::min(
                PORT_GAP,
                (length - 2.0 * PORT_MARGIN) / (ends.len() - 1) as f64,
            ),
            0.0,
        );
        let center = if vertical {
            cnodes[node].y
        } else {
            cnodes[node].x
        };
        ends.sort_by(|a, b| js::order(a.toward - b.toward));
        let count = ends.len() as f64;
        for (i, end) in ends.iter().enumerate() {
            cedges[end.edge].route[end.at + usize::from(vertical)] =
                center + (i as f64 - (count - 1.0) / 2.0) * step;
        }
    }
}

/// 自分への関係のある実体は描くより大きく配置する。経路の端を、その大きな箱の辺から実体の辺へ動かす。
fn inset(route: &mut [f64], at: usize, view: &NodeView, c: &CNode) {
    if c.is_group {
        return;
    }
    let dx = route[at] - c.x;
    let dy = route[at + 1] - c.y;
    if c.w != view.w && (dx.abs() - c.w / 2.0).abs() < 0.5 {
        route[at] -= (js::sign(dx) * (c.w - view.w)) / 2.0;
    }
    if c.h != view.h && (dy.abs() - c.h / 2.0).abs() < 0.5 {
        route[at + 1] -= (js::sign(dy) * (c.h - view.h)) / 2.0;
    }
}
