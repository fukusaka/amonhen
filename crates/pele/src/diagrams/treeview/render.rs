//! treeView を描く（pele の `src/diagrams/treeview/render.ts`）。

use super::icons::{BUILTIN_PACK, get_node_icon, glyph, icon_config};
use super::model::{NodeType, TreeViewModel};
use crate::diagrams::common::fit_width::{tighten, title_room};
use crate::diagrams::common::literal_label::{left_label_svg, literal_label};
use crate::errors::Result;
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num};
use crate::svg::root::{Accessible, svg_document};
use crate::svg::theme::{FONT_MONO, ROUNDED, class_names};
use crate::text::entities::decode_entities;
use crate::text::label::{Label, layout_label};
use crate::text::measurer::{TextMeasurer, default_measurer, style};
use crate::types::{RenderOptions, Rendered};
use crate::util::yaml::Value;

const ICON_GAP: f64 = 6.0;
const DESC_GAP: f64 = 16.0;
const GUIDE_INSET: f64 = 7.0;
const ELBOW: f64 = 4.0;
const HIGHLIGHT_OVERHANG: f64 = 4.0;

fn setting(config: &Config, key: &str, fallback: f64, min: f64, max: f64) -> f64 {
    match config
        .get("treeView")
        .and_then(Value::as_obj)
        .and_then(|t| t.get(key))
    {
        Some(Value::Num(v)) if *v >= min && *v <= max => *v,
        _ => fallback,
    }
}

pub struct TreeSettings {
    pub padding_x: f64,
    pub padding_y: f64,
    pub row_indent: f64,
    pub line_thickness: f64,
}

/// Mermaid の配置の設定と、pele の既定。
pub fn tree_settings(config: &Config) -> TreeSettings {
    TreeSettings {
        padding_x: setting(config, "paddingX", 5.0, 0.0, 100.0),
        padding_y: setting(config, "paddingY", 2.0, 0.0, 100.0),
        row_indent: setting(config, "rowIndent", 15.0, 0.0, 400.0),
        line_thickness: setting(config, "lineThickness", 1.0, 0.25, 12.0),
    }
}

struct Row {
    node: usize,
    x: f64,
    cy: f64,
    icon: Option<String>,
    text: String,
    label_x: f64,
    highlighted: bool,
    label: Option<Label>,
    description: Option<Label>,
    top: f64,
    height: f64,
}

pub fn render_tree_view(
    model: &TreeViewModel,
    config: &Config,
    options: &RenderOptions,
) -> Result<Rendered> {
    tighten(options, |tight| Ok(draw(model, config, options, tight)))
}

fn draw(model: &TreeViewModel, config: &Config, options: &RenderOptions, tight: bool) -> Rendered {
    let size = options.font_size.unwrap_or(16.0);
    let measurer: &dyn TextMeasurer = match &options.measurer {
        Some(m) => m.as_ref(),
        None => default_measurer(),
    };
    let pad = options.padding.unwrap_or(8.0);
    let icons = icon_config(config);
    let mono = options
        .monospace
        .as_ref()
        .is_some_and(|m| m.iter().any(|s| s == "files"));
    let family = if mono {
        format!(" font-family=\"{FONT_MONO}\"")
    } else {
        String::new()
    };
    let nodes = &model.nodes;

    let TreeSettings {
        padding_x,
        padding_y,
        row_indent,
        line_thickness,
    } = tree_settings(config);
    let mut max_depth = 0.0;
    let mut depths: Vec<(usize, f64)> = nodes[0].children.iter().map(|&n| (n, 0.0)).collect();
    let mut i = 0;
    while tight && i < depths.len() {
        let (node, depth) = depths[i];
        max_depth = js::max(max_depth, depth);
        for &child in &nodes[node].children {
            depths.push((child, depth + 1.0));
        }
        i += 1;
    }
    let max_width = options.max_width.unwrap_or(320.0);
    let step = if tight {
        js::min(
            row_indent + padding_x,
            js::max(2.0, (max_width - 120.0) / js::max(1.0, max_depth)),
        )
    } else {
        row_indent + padding_x
    };
    let row_height = js::round(size * 1.5) + 2.0 * padding_y;
    let icon_size = size;

    let title = layout_label(
        model.title.as_deref(),
        false,
        measurer,
        size,
        if tight { title_room(options) } else { 4000.0 },
        style::BOLD,
    );
    let title_height = if title.height > 0.0 {
        title.height + 12.0
    } else {
        0.0
    };

    let node_icon = |n: usize| {
        let node = &nodes[n];
        get_node_icon(node.icon.as_deref(), &node.name, node.node_type, &icons)
    };
    // 兄弟はそろえる: フォルダの項目の 1 つにアイコンがあれば、ほかもその場所を空ける。
    let has_icon = |list: &[usize]| list.iter().any(|&c| node_icon(c).is_some());
    let mut rows: Vec<Row> = Vec::new();
    let mut row_of: Vec<usize> = vec![usize::MAX; nodes.len()];
    let mut pending: Vec<(usize, f64, bool)> = Vec::new();
    let push = |pending: &mut Vec<(usize, f64, bool)>, list: &[usize], depth: f64| {
        let slot = has_icon(list);
        for &n in list.iter().rev() {
            pending.push((n, depth, slot));
        }
    };
    push(&mut pending, &nodes[0].children, 0.0);
    let mut label_right = 0.0;
    let mut any_highlight = false;
    let mut rows_height = 0.0;
    while let Some((n, depth, slot)) = pending.pop() {
        let node = &nodes[n];
        let x = depth * step;
        let text = decode_entities(&node.name);
        let label_x = x + if slot { icon_size + ICON_GAP } else { 0.0 };
        let room = js::max(
            40.0,
            max_width - 2.0 * pad - 2.0 * HIGHLIGHT_OVERHANG - label_x,
        );
        let st = (if node.node_type == NodeType::Directory {
            style::BOLD
        } else {
            0
        }) | if mono { style::MONO } else { 0 };
        let label = tight.then(|| literal_label(&text, measurer, size, room, st));
        let description = match &node.description {
            Some(d) if tight && !d.is_empty() => Some(literal_label(
                &decode_entities(d),
                measurer,
                size,
                room,
                style::ITALIC,
            )),
            _ => None,
        };
        let width = match &label {
            Some(l) => l.width,
            None => measurer.width(&text, size, st),
        };
        if label_x + width > label_right {
            label_right = label_x + width;
        }
        let highlighted = node.css_class.as_ref().is_some_and(|c| {
            c.split(|ch: char| js::is_js_space(ch))
                .any(|p| p == "highlight")
        });
        if highlighted {
            any_highlight = true;
        }
        let label_h = label.as_ref().map_or(0.0, |l| l.height);
        let height = if tight {
            js::max(
                row_height,
                label_h + description.as_ref().map_or(0.0, |d| d.height) + 2.0 * padding_y,
            )
        } else {
            row_height
        };
        let cy = rows_height
            + if tight {
                padding_y + js::max(icon_size, label_h) / 2.0
            } else {
                height / 2.0
            };
        row_of[n] = rows.len();
        rows.push(Row {
            node: n,
            x,
            cy,
            icon: node_icon(n),
            text,
            label_x,
            highlighted,
            label,
            description,
            top: rows_height,
            height,
        });
        rows_height += height;
        push(&mut pending, &node.children, depth + 1.0);
    }

    let desc_x = label_right + DESC_GAP;
    let mut width = label_right;
    for row in &rows {
        let Some(description) = nodes[row.node]
            .description
            .as_deref()
            .filter(|d| !d.is_empty())
        else {
            continue;
        };
        let w = match &row.description {
            Some(d) => d.width,
            None => measurer.width(&decode_entities(description), size, style::ITALIC),
        };
        width = js::max(width, (if tight { row.label_x } else { desc_x }) + w);
    }
    width = js::max(width, title.width);
    let inset = pad
        + if any_highlight {
            HIGHLIGHT_OVERHANG
        } else {
            0.0
        };
    let top = pad + title_height;
    let total_width = (width + 2.0 * inset).ceil();
    let total_height = (top + rows_height + pad).ceil();

    let mut guides = String::new();
    for row in &rows {
        let children = &nodes[row.node].children;
        if children.is_empty() {
            continue;
        }
        let gx = inset + row.x + GUIDE_INSET;
        let tick = js::max(gx, inset + row.x + step - padding_x);
        let last = top + rows[row_of[*children.last().unwrap()]].cy;
        let r = js::min(ELBOW, tick - gx);
        let mut d = format!(
            "M{},{}V{}",
            num(gx),
            num(top + row.top + row.height - 2.0),
            num(last - r)
        );
        if r > 0.0 {
            d.push_str(&format!(
                "Q{},{} {},{}H{}",
                num(gx),
                num(last),
                num(gx + r),
                num(last),
                num(tick)
            ));
        }
        let mut i = 0;
        while i + 1 < children.len() && tick > gx {
            d.push_str(&format!(
                "M{},{}H{}",
                num(gx),
                num(top + rows[row_of[children[i]]].cy),
                num(tick)
            ));
            i += 1;
        }
        guides.push_str(&format!("<path d=\"{d}\"/>"));
    }

    let baseline = size * 0.35;
    let mut body = String::new();
    for row in &rows {
        let node = &nodes[row.node];
        let cy = top + row.cy;
        let directory = node.node_type == NodeType::Directory;
        let weight = if directory {
            " font-weight=\"var(--_hw)\""
        } else {
            ""
        };
        let mut inner = String::new();
        if row.highlighted {
            let x = inset + row.x - HIGHLIGHT_OVERHANG;
            inner.push_str(&format!(
                "<rect class=\"pele-tree-highlight\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_c)\" fill-opacity=\"0.14\"{ROUNDED}/>",
                num(x),
                num(top + row.top + 1.0),
                num(total_width - inset + HIGHLIGHT_OVERHANG - x),
                num(row.height - 2.0)
            ));
        }
        if let Some(icon) = &row.icon {
            let name = icon
                .strip_prefix(BUILTIN_PACK)
                .and_then(|r| r.strip_prefix(':'))
                .unwrap_or("");
            let g = glyph(name)
                .or_else(|| glyph(if directory { "folder" } else { "file" }))
                .unwrap();
            let drawn = options
                .icons
                .as_ref()
                .and_then(|f| f(icon))
                .unwrap_or_else(|| {
                    format!("<path d=\"{g}\" stroke=\"var(--_m)\" stroke-width=\"1.5\"/>")
                });
            inner.push_str(&format!(
                "<svg class=\"pele-icon\" data-icon=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\">{drawn}</svg>",
                esc_text(icon),
                num(inset + row.x),
                num(cy - icon_size / 2.0),
                num(icon_size),
                num(icon_size)
            ));
        }
        if let Some(label) = &row.label {
            inner.push_str(&left_label_svg(
                label,
                inset + row.label_x,
                top + row.top + padding_y,
                &format!(" class=\"pele-label\"{weight}{family}"),
            ));
        } else if !row.text.is_empty() {
            inner.push_str(&format!(
                "<text class=\"pele-label\" x=\"{}\" y=\"{}\"{weight}{family} xml:space=\"preserve\">{}</text>",
                num(inset + row.label_x),
                num(cy + baseline),
                esc(&row.text)
            ));
        }
        if let Some(description) = &row.description {
            inner.push_str(&left_label_svg(
                description,
                inset + row.label_x,
                top + row.top + padding_y + row.label.as_ref().map_or(0.0, |l| l.height),
                " class=\"pele-tree-description\" font-style=\"italic\" fill=\"var(--_m)\"",
            ));
        } else if let Some(d) = node.description.as_deref().filter(|d| !d.is_empty()) {
            inner.push_str(&format!(
                "<text class=\"pele-tree-description\" x=\"{}\" y=\"{}\" font-style=\"italic\" fill=\"var(--_m)\" xml:space=\"preserve\">{}</text>",
                num(inset + desc_x),
                num(cy + baseline),
                esc(&decode_entities(d))
            ));
        }
        body.push_str(&format!(
            "<g class=\"pele-node pele-tree-{}{}\" data-id=\"{}\">{inner}</g>",
            if directory { "dir" } else { "file" },
            class_names(node.css_class.as_deref().unwrap_or("")),
            esc_text(&node.name)
        ));
    }

    let mut content = label_svg(
        &title,
        total_width / 2.0,
        pad + title.height / 2.0,
        " class=\"pele-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    if !guides.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-tree-guides\" fill=\"none\" stroke=\"var(--_b)\" stroke-width=\"{}\">{guides}</g>",
            num(line_thickness)
        ));
    }
    content.push_str(&body);

    let acc = Accessible {
        acc_title: model.acc_title.as_deref(),
        acc_descr: model.acc_descr.as_deref(),
    };
    Rendered {
        svg: svg_document(
            "treeView",
            total_width,
            total_height,
            size,
            options,
            &acc,
            &content,
        ),
        width: total_width,
        height: total_height,
        links: Vec::new(),
    }
}
