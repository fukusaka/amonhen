//! treemap を描く（pele の `src/diagrams/treemap/render.ts`）。

use super::format::formatter;
use super::layout::{Box, Insets, layout_treemap};
use super::model::TreemapModel;
use crate::diagrams::common::fit_text::fit_text;
use crate::diagrams::common::fit_width::{cap_width, title_room};
use crate::errors::Result;
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num};
use crate::svg::ink::ink_fill;
use crate::svg::palette::series_ink_fill;
use crate::svg::root::{Accessible, svg_document};
use crate::svg::theme::{class_names, resolve_style, rounded, series_color, sets_fill};
use crate::text::entities::decode_entities;
use crate::text::label::layout_label;
use crate::text::measurer::{TextMeasurer, default_measurer, style};
use crate::types::{RenderOptions, Rendered};
use crate::util::yaml::Value;

const HEADER: f64 = 28.0;
const SHORT_HEADER: f64 = 18.0;
const SIDE: f64 = 6.0;
const LINE: f64 = 1.25;

fn title_part(raw: &str) -> String {
    js::trim(&esc(js::trim(&decode_entities(raw)))).to_string()
}

fn leaf_count(model: &TreemapModel) -> usize {
    let mut count = 0;
    let mut stack: Vec<usize> = model.roots.clone();
    while let Some(n) = stack.pop() {
        let node = &model.nodes[n];
        match &node.children {
            Some(children) => stack.extend(children),
            None => {
                if node.value.is_some_and(|v| v.is_finite() && v > 0.0) {
                    count += 1;
                }
            }
        }
    }
    count
}

/// 小さな箱の角は短い辺の 4 分の 1 までにする。4px が既定の半径。
fn corners(attrs: &str, b: &Box) -> String {
    let cap = js::min(b.w, b.h) / 4.0;
    if cap < 4.0 {
        rounded(attrs).replacen(
            "rx:var(--_r)",
            &format!("rx:min(var(--_r),{}px)", num(cap)),
            1,
        )
    } else {
        rounded(attrs)
    }
}

fn option(config: &Config, key: &str, fallback: f64, min: f64, max: f64) -> f64 {
    match config
        .get("treemap")
        .and_then(Value::as_obj)
        .and_then(|t| t.get(key))
    {
        Some(Value::Num(v)) if *v >= min => js::min(*v, max),
        _ => fallback,
    }
}

pub fn render_treemap(
    model: &TreemapModel,
    config: &Config,
    options: &RenderOptions,
) -> Result<Rendered> {
    let size = options.font_size.unwrap_or(16.0);
    let small = js::round(size * 0.875);
    let tiny = js::round(size * 0.75);
    let least = js::round(size * 0.5625);
    let measurer: &dyn TextMeasurer = match &options.measurer {
        Some(m) => m.as_ref(),
        None => default_measurer(),
    };
    let treemap = config.get("treemap").and_then(Value::as_obj);
    let pad = options
        .padding
        .unwrap_or_else(|| option(config, "diagramPadding", 8.0, 0.0, 2000.0));

    // Mermaid は図の大きさを `nodeWidth` の 10 倍 × `nodeHeight` の 10 倍にする。
    let width = js::round(cap_width(
        options,
        option(config, "nodeWidth", 64.0, 1.0, 400.0) * 10.0,
        200.0,
        2.0 * pad,
    ));
    let height = js::round(option(config, "nodeHeight", 40.0, 1.0, 400.0) * 10.0);
    let show_values = treemap.and_then(|t| t.get("showValues")) != Some(&Value::Bool(false));
    let format = formatter(
        treemap
            .and_then(|t| t.get("valueFormat"))
            .and_then(Value::as_str)
            .unwrap_or(","),
    );
    // 葉の平均の面積が小さくなるほど、隙間と余白を減らす。
    let crowd = js::min(
        1.0,
        js::max(
            0.0,
            ((width * height) / js::max(1.0, leaf_count(model) as f64) / 800.0).log10(),
        ),
    );
    let (boxes, placed) = layout_treemap(
        &model.nodes,
        &model.roots,
        width,
        height,
        &Insets {
            gap: option(config, "padding", js::round(2.0 + 2.0 * crowd), 0.0, 200.0),
            header: HEADER,
            short: SHORT_HEADER,
            side: js::round(3.0 + 3.0 * crowd),
        },
    );

    let title = layout_label(
        model.title.as_deref(),
        false,
        measurer,
        size,
        title_room(options),
        style::BOLD,
    );
    let title_height = if title.height > 0.0 {
        title.height + 8.0
    } else {
        0.0
    };
    let left = pad + js::max(0.0, (title.width - width) / 2.0);
    let top = pad + title_height;

    let mut sections = String::new();
    let mut leaves = String::new();
    for &k in &placed {
        let b = &boxes[k];
        let node = &model.nodes[b.node];
        let st = resolve_style(node.css_compiled_styles.as_deref().unwrap_or(&[]));
        let color = series_color(b.series as i64);
        let x = left + b.x;
        let y = top + b.y;
        let name = js::trim(&decode_entities(&node.name)).to_string();
        let value = if show_values {
            format.format(b.value)
        } else {
            String::new()
        };
        let group = format!("data-id=\"{}\">", esc_text(&node.name));
        // 見出しのない祖先は子に覆われるので、ヒントに入れる。
        let own = title_part(&node.name);
        let mut path: Vec<String> = if own.is_empty() {
            Vec::new()
        } else {
            vec![own.clone()]
        };
        let mut p = b.parent;
        while let Some(pk) = p.filter(|&pk| !boxes[pk].headed) {
            let section = title_part(&model.nodes[boxes[pk].node].name);
            if !section.is_empty() {
                path.insert(0, section);
            }
            p = boxes[pk].parent;
        }
        let full = path.join(" › ");
        let tooltip = |drawn: Option<&str>| -> String {
            if full.is_empty() || (drawn == Some(name.as_str()) && full == own) {
                String::new()
            } else {
                format!("<title>{full}</title>")
            }
        };
        let rect = format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{color}\"",
            num(x),
            num(y),
            num(b.w),
            num(b.h)
        );
        // 書き込んだ字の大きさは SVG の属性に勝つので、縮めずに切る。
        let fixed = st.font_size;
        let text_style =
            (if st.bold { style::BOLD } else { 0 }) | (if st.italic { style::ITALIC } else { 0 });
        let ink = if sets_fill(&st.shape) {
            ink_fill(&st.shape, &st.text, None)
        } else {
            series_ink_fill(b.series as i64)
        };
        let classes = class_names(node.class_selector.as_deref().unwrap_or(""));

        if node.children.is_some() {
            let mut text = String::new();
            let mut drawn: Option<String> = None;
            if b.headed {
                let short = b.header < HEADER;
                let label_size = fixed.unwrap_or(if short { tiny } else { small });
                let value_size = fixed.unwrap_or(tiny);
                let room = b.w - 2.0 * SIDE - 4.0;
                let value_width = if value.is_empty() {
                    0.0
                } else {
                    measurer.width(&value, value_size, text_style)
                };
                let with_value =
                    !short && value_width > 0.0 && room - value_width - 12.0 >= 3.0 * label_size;
                let label = if label_size * LINE < b.header {
                    fit_text(
                        &name,
                        if with_value {
                            room - value_width - 12.0
                        } else {
                            room
                        },
                        label_size,
                        label_size,
                        measurer,
                        text_style | style::BOLD,
                    )
                } else {
                    None
                };
                let baseline = y + b.header / 2.0 + 1.0;
                drawn = label.as_ref().map(|l| l.text.clone());
                if let Some(label) = &label {
                    text.push_str(&format!(
                        "<text x=\"{}\" y=\"{}\" font-weight=\"var(--_hw)\"{}{ink}{}>{}</text>",
                        num(x + SIDE + 2.0),
                        num(baseline + label_size * 0.35),
                        if short {
                            format!(" font-size=\"{}\"", js::fmt(label_size))
                        } else {
                            String::new()
                        },
                        st.text,
                        esc(&label.text)
                    ));
                }
                if with_value && value_size * LINE <= HEADER {
                    text.push_str(&format!(
                        "<text class=\"pele-value\" x=\"{}\" y=\"{}\" font-size=\"{}\" text-anchor=\"end\"{}{}>{}</text>",
                        num(x + b.w - SIDE - 2.0),
                        num(baseline + value_size * 0.35),
                        js::fmt(tiny),
                        if ink.is_empty() {
                            " fill=\"var(--_m)\""
                        } else {
                            &ink
                        },
                        st.text,
                        esc(&value)
                    ));
                }
            }
            // 節は色の薄い塗り（クラスが自分の塗りを決めなければ）。見出しのない節は箱を描かず、子が格子に溶け込む。
            let tint = if sets_fill(&st.shape) {
                ""
            } else {
                " fill-opacity=\"0.12\""
            };
            let shape = if b.headed {
                format!(
                    "{rect}{}/>",
                    corners(&format!("{tint} stroke=\"{color}\"{}", st.shape), b)
                )
            } else {
                format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\"/>",
                    num(x),
                    num(y),
                    num(b.w),
                    num(b.h)
                )
            };
            sections.push_str(&format!(
                "<g class=\"pele-node pele-section{classes}\" {group}{}{shape}{text}</g>",
                tooltip(drawn.as_deref())
            ));
            continue;
        }

        let mut text = String::new();
        let room = b.w - 8.0;
        let smallest = fixed.unwrap_or(least);
        let tallest = match fixed {
            None => js::min(size, ((b.h - 4.0) / LINE).floor()),
            Some(f) => {
                if f * LINE <= b.h - 4.0 {
                    f
                } else {
                    0.0
                }
            }
        };
        let label = if tallest >= smallest {
            fit_text(&name, room, tallest, smallest, measurer, text_style)
        } else {
            None
        };
        if let Some(label) = &label {
            let cx = num(x + b.w / 2.0);
            let cy = y + b.h / 2.0;
            let value_size = fixed.unwrap_or_else(|| js::max(least, js::round(label.size * 0.8)));
            let lines = (label.size + value_size) * LINE;
            if !value.is_empty()
                && lines <= b.h - 4.0
                && measurer.width(&value, value_size, text_style) <= room
            {
                let first = cy - lines / 2.0 + (label.size * LINE) / 2.0;
                let second = cy + lines / 2.0 - (value_size * LINE) / 2.0;
                text = format!(
                    "<text x=\"{cx}\" y=\"{}\" font-size=\"{}\"{ink}{}>{}</text><text class=\"pele-value\" x=\"{cx}\" y=\"{}\" font-size=\"{}\"{ink}{}>{}</text>",
                    num(first + label.size * 0.35),
                    num(label.size),
                    st.text,
                    esc(&label.text),
                    num(second + value_size * 0.35),
                    num(value_size),
                    st.text,
                    esc(&value)
                );
            } else {
                text = format!(
                    "<text x=\"{cx}\" y=\"{}\" font-size=\"{}\"{ink}{}>{}</text>",
                    num(cy + label.size * 0.35),
                    num(label.size),
                    st.text,
                    esc(&label.text)
                );
            }
        }
        leaves.push_str(&format!(
            "<g class=\"pele-node pele-leaf{classes}\" {group}{}{rect}{}/>{text}</g>",
            tooltip(label.as_ref().map(|l| l.text.as_str())),
            corners(&st.shape, b)
        ));
    }

    let total_width = (js::max(width, title.width) + 2.0 * pad).ceil();
    let total_height = (height + title_height + 2.0 * pad).ceil();
    let mut content = label_svg(
        &title,
        total_width / 2.0,
        pad + title.height / 2.0,
        " class=\"pele-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    if !sections.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-sections\" font-size=\"{}\">{sections}</g>",
            js::fmt(small)
        ));
    }
    if !leaves.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-leaves\" text-anchor=\"middle\" fill=\"var(--_bg)\">{leaves}</g>"
        ));
    }

    let acc = Accessible {
        acc_title: model.acc_title.as_deref(),
        acc_descr: model.acc_descr.as_deref(),
    };
    Ok(Rendered {
        svg: svg_document(
            "treemap",
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
    })
}
