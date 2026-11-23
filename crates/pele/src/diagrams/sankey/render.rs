//! sankey を描く（pele の `src/diagrams/sankey/render.ts`）。

use std::collections::HashMap;

use crate::diagrams::common::fit_width::{fit_width, title_room};
use crate::errors::Result;
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num};
use crate::svg::root::{Accessible, svg_document};
use crate::svg::theme::{resolve_style, series_color};
use crate::text::entities::decode_entities;
use crate::text::label::layout_label;
use crate::text::measurer::{TextMeasurer, default_measurer, style};
use crate::types::{RenderOptions, Rendered};
use crate::util::yaml::Value;

use super::db::SankeyDb;
use super::layout::{Alignment, LayoutOptions, SankeyLayout, layout_sankey};

const LABEL_GAP: f64 = 6.0;
const VALUE_GAP: f64 = 15.0;
const LABEL_ROOM: f64 = 1.8;
const CAREFUL: usize = 300;

fn base36(mut n: u32) -> String {
    if n == 0 {
        return "0".into();
    }
    let mut out = Vec::new();
    while n > 0 {
        out.push(char::from_digit(n % 36, 36).unwrap());
        n /= 36;
    }
    out.iter().rev().collect()
}

/// 32 ビットのハッシュを 2 つ。違うグラデーションが id を分け合うことはまずない。
fn content_id(text: &str) -> String {
    let mut a: u32 = 0x811c9dc5;
    let mut b: i32 = 5381;
    for c in text.encode_utf16() {
        a = (a ^ c as u32).wrapping_mul(0x01000193);
        b = b.wrapping_mul(33).wrapping_add(c as i32);
    }
    base36(a) + &base36(b as u32)
}

fn number_option(conf: &Config, key: &str, fallback: f64, min: f64, max: f64) -> f64 {
    match conf.get(key) {
        Some(Value::Num(v)) if *v >= min => js::min(*v, max),
        _ => fallback,
    }
}

fn text_option(conf: &Config, key: &str) -> String {
    match conf.get(key) {
        Some(Value::Str(s)) => s.clone(),
        Some(Value::Num(v)) if v.is_finite() => js::fmt(*v),
        _ => String::new(),
    }
}

/// Mermaid が値を見せるとおり小数 2 桁に。それには大きすぎる数はそのまま。
fn rounded(value: f64) -> f64 {
    if value.abs() < 1e15 {
        js::round(value * 100.0) / 100.0
    } else {
        value
    }
}

/// 設定の色を、系列の色に勝る書き込みのスタイルに。
fn fill_style(color: Option<&Value>) -> String {
    match color {
        Some(Value::Str(s)) => resolve_style(&[format!("fill:{s}")]).shape,
        _ => String::new(),
    }
}

/// `/^[\w-]+$/`
fn plain_id(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

pub fn render_sankey(
    model: &SankeyDb,
    config: &Config,
    options: &RenderOptions,
) -> Result<Rendered> {
    let empty = Config::new();
    let conf = match config.get("sankey") {
        Some(Value::Obj(m)) => m,
        _ => &empty,
    };
    let natural = number_option(conf, "width", 600.0, 1.0, 10000.0);
    fit_width(options, natural, 160.0, |chart_width, _| {
        draw(model, conf, options, chart_width, natural)
    })
}

struct Placed {
    value: String,
    width: f64,
    after: bool,
    left: f64,
    y: f64,
}

fn draw(
    model: &SankeyDb,
    conf: &Config,
    options: &RenderOptions,
    chart_width: f64,
    natural: f64,
) -> Result<Rendered> {
    let size = options.font_size.unwrap_or(16.0);
    let least = js::round(size * 0.625);
    // 幅に合わせて狭めた図では、絵全体を縮めたときのようにラベルも縮める。
    let mut small = js::max(
        least,
        js::round(size * 0.875 * js::min(1.0, chart_width / natural)),
    );
    let measurer: &dyn TextMeasurer = match &options.measurer {
        Some(m) => m.as_ref(),
        None => default_measurer(),
    };
    let pad = options.padding.unwrap_or(8.0);

    let chart_height = number_option(conf, "height", 400.0, 1.0, 10000.0);
    let node_width = js::min(
        number_option(conf, "nodeWidth", 10.0, 1.0, 200.0),
        chart_width,
    );
    let show_values = conf.get("showValues") != Some(&Value::Bool(false));
    let prefix = decode_entities(&text_option(conf, "prefix"));
    let suffix = decode_entities(&text_option(conf, "suffix"));
    let outlined = conf.get("labelStyle").and_then(Value::as_str) == Some("outlined");
    let alignment = match conf.get("nodeAlignment").and_then(Value::as_str) {
        Some("left") => Alignment::Left,
        Some("right") => Alignment::Right,
        Some("center") => Alignment::Center,
        _ => Alignment::Justify,
    };
    let link_color = match conf.get("linkColor") {
        Some(Value::Str(s)) => s.as_str(),
        _ => "gradient",
    };

    let lay = |label_size: f64| -> Result<SankeyLayout> {
        layout_sankey(
            model,
            &LayoutOptions {
                width: chart_width,
                height: chart_height,
                node_width,
                node_padding: number_option(conf, "nodePadding", 12.0, 0.0, 200.0)
                    + if show_values { VALUE_GAP } else { 0.0 },
                label_height: label_size * 1.15,
                alignment,
            },
        )
    };
    let mut layout = lay(small)?;
    // ラベルは最も混んだ列でも縮み、どれもおよそ LABEL_ROOM 行を保つ。
    let mut rows: HashMap<usize, usize> = HashMap::new();
    let mut fullest = 1;
    for node in &layout.nodes {
        let count = rows.get(&node.layer).copied().unwrap_or(0) + 1;
        rows.insert(node.layer, count);
        if count > fullest {
            fullest = count;
        }
    }
    let crowded = js::max(least, js::round(chart_height / fullest as f64 / LABEL_ROOM));
    if crowded < small {
        small = crowded;
        layout = lay(small)?;
    }
    let SankeyLayout { nodes, links } = layout;
    let line_height = small * 1.5;

    let mut custom: HashMap<&str, String> = HashMap::new();
    if let Some(Value::Obj(colors)) = conf.get("nodeColors") {
        for (id, color) in colors.iter() {
            let style = fill_style(Some(color));
            if !style.is_empty() {
                custom.insert(id.as_str(), style);
            }
        }
    }
    let node_style = |i: usize| -> String {
        custom
            .get(model.nodes[i].id.as_str())
            .cloned()
            .unwrap_or_default()
    };
    // 符号の中の `;` を保つよう、塗りの全体に合わせる（`/^ style="fill:(.*);"$/`）。
    let node_color = |i: usize| -> String {
        let s = node_style(i);
        s.strip_prefix(" style=\"fill:")
            .and_then(|r| r.strip_suffix(";\""))
            .filter(|inner| !inner.contains(['\n', '\r', '\u{2028}', '\u{2029}']))
            .map(str::to_string)
            .unwrap_or_else(|| series_color(i as i64))
    };

    // ラベルは棒の横の、混んでいなければ図の中ほどを向く側に置く。
    let mut central = 0;
    let mut most = 0.0;
    for node in &nodes {
        if node.value > most {
            most = node.value;
            central = node.layer;
        }
    }
    let mut min_x = 0.0;
    let mut max_x = chart_width;
    let mut min_y = 0.0;
    let mut max_y = chart_height;
    let mut labels: Vec<Placed> = Vec::with_capacity(nodes.len());
    for (i, node) in nodes.iter().enumerate() {
        let name = decode_entities(&model.nodes[i].id);
        let value = if show_values {
            format!("{prefix}{}{suffix}", js::fmt(rounded(node.value)))
        } else {
            String::new()
        };
        let width = measurer.width(
            &if value.is_empty() {
                name.clone()
            } else {
                format!("{name} {value}")
            },
            small,
            0,
        );
        let after = if outlined {
            node.layer >= central
        } else {
            node.x < chart_width / 2.0
        };
        let x = if after {
            node.x + node_width + LABEL_GAP
        } else {
            node.x - LABEL_GAP
        };
        let y = node.y + node.height / 2.0;
        min_x = js::min(min_x, if after { x } else { x - width });
        max_x = js::max(max_x, if after { x + width } else { x });
        min_y = js::min(min_y, y - small * 0.75);
        max_y = js::max(max_y, y + small * 0.75);
        labels.push(Placed {
            value,
            width,
            after,
            left: if after { x } else { x - width },
            y,
        });
    }
    // ラベルが動くのは高々 1 行。両側に 2 つずつの帯を持つ。
    let band_of = |y: f64| (y / line_height).floor() as i64;
    let mut bands: HashMap<i64, Vec<usize>> = HashMap::new();
    if nodes.len() <= CAREFUL {
        for (k, label) in labels.iter().enumerate() {
            let band = band_of(label.y);
            for key in [band - 2, band - 1, band, band + 1, band + 2] {
                bands.entry(key).or_default().push(k);
            }
        }
    }
    let mut labels_out = String::new();
    let mut nodes_out = String::new();
    for (i, node) in nodes.iter().enumerate() {
        let id = &model.nodes[i].id;
        let name = decode_entities(id);
        let height = js::max(node.height, 1.0);
        nodes_out.push_str(&format!(
            "<rect class=\"pele-sankey-node\" data-id=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\" fill=\"{}\"{}/>",
            esc_text(id),
            num(node.x),
            num(node.y),
            num(node_width),
            num(height),
            num(js::min(2.0, height / 2.0)),
            series_color(i as i64),
            node_style(i)
        ));

        let overlaps_label = |labels: &[Placed], left: f64, y: f64| -> bool {
            let width = labels[i].width;
            bands.get(&band_of(y)).is_some_and(|list| {
                list.iter().any(|&o| {
                    let other = &labels[o];
                    o != i
                        && (other.y - y).abs() < line_height
                        && left < other.left + other.width + LABEL_GAP
                        && left + width > other.left - LABEL_GAP
                })
            })
        };
        // 反対の側を、両側が混んでいれば隣の行を試す。
        if nodes.len() <= CAREFUL && overlaps_label(&labels, labels[i].left, labels[i].y) {
            'place: for dy in [0.0, -line_height, line_height] {
                let first = labels[i].after;
                for after in [first, !first] {
                    let width = labels[i].width;
                    let left = if after {
                        node.x + node_width + LABEL_GAP
                    } else {
                        node.x - LABEL_GAP - width
                    };
                    let y = labels[i].y + dy;
                    if overlaps_label(&labels, left, y)
                        || left < min_x
                        || left + width > max_x
                        || y - small * 0.75 < min_y
                        || y + small * 0.75 > max_y
                    {
                        continue;
                    }
                    if nodes.iter().enumerate().any(|(j, bar)| {
                        j != i
                            && left < bar.x + node_width + LABEL_GAP
                            && left + width > bar.x - LABEL_GAP
                            && y - small * 0.75 < bar.y + bar.height
                            && y + small * 0.75 > bar.y
                    }) {
                        continue;
                    }
                    // ラベルは列のどの棒よりも自分の棒に近く、ほかの棒の名に見えない。
                    let own = js::max(js::max(0.0, node.y - y), y - node.y - node.height);
                    if nodes.iter().enumerate().any(|(j, bar)| {
                        j != i
                            && bar.x == node.x
                            && js::max(js::max(0.0, bar.y - y), y - bar.y - bar.height) <= own
                    }) {
                        continue;
                    }
                    labels[i].left = left;
                    labels[i].y = y;
                    labels[i].after = after;
                    break 'place;
                }
            }
        }
        let label = &labels[i];
        let (value, after, y) = (&label.value, label.after, label.y);
        let x = if after {
            node.x + node_width + LABEL_GAP
        } else {
            node.x - LABEL_GAP
        };
        let position = format!(
            " x=\"{}\" y=\"{}\" text-anchor=\"{}\"",
            num(x),
            num(y + small * 0.35),
            if after { "start" } else { "end" }
        );
        let text = esc(&name)
            + &if value.is_empty() {
                String::new()
            } else {
                format!(
                    " <tspan class=\"pele-sankey-value\" fill=\"var(--_m)\">{}</tspan>",
                    esc(value)
                )
            };
        labels_out.push_str(&format!(
            "<g class=\"pele-label\" data-id=\"{}\">",
            esc_text(id)
        ));
        if outlined {
            labels_out.push_str(&format!(
                "<text{position} fill=\"none\" stroke=\"var(--_bg)\" stroke-width=\"3\" stroke-linejoin=\"round\">{}{}</text>",
                esc(&name),
                if value.is_empty() {
                    String::new()
                } else {
                    format!(" {}", esc(value))
                }
            ));
        }
        labels_out.push_str(&format!("<text{position}>{text}</text></g>"));
    }

    let fixed = if matches!(link_color, "gradient" | "source" | "target") {
        String::new()
    } else {
        fill_style(conf.get("linkColor"))
    };
    // idPrefix がなければ図どうしがグラデーションの id を分け合い、Chrome は隠れた、あるいは別のテーマの写しの
    // グラデーションを使う。ほかの文字は url() を終わらせ、塗り全体を無効にする。
    let id_prefix = options.id_prefix.as_deref().unwrap_or("");
    let gradient = link_color == "gradient" && plain_id(id_prefix);
    let mut gradients: Vec<(String, String)> = Vec::new();
    let mut links_out = String::new();
    for link in &links {
        let end = if link_color == "target" {
            link.target
        } else {
            link.source
        };
        let half = js::max(link.thickness, 1.0) / 2.0;
        let x0 = nodes[link.source].x + node_width;
        let x1 = nodes[link.target].x;
        let mut paint = format!(
            "fill=\"{}\"{}",
            series_color(end as i64),
            if fixed.is_empty() {
                node_style(end)
            } else {
                fixed.clone()
            }
        );
        if gradient {
            // id はグラデーションそのものから作る。同じ頭の図どうしで合う。url() の後の色は代わり。
            let stops = format!(
                "x1=\"{}\" x2=\"{}\"><stop offset=\"0\" stop-color=\"{}\"/><stop offset=\"1\" stop-color=\"{}\"/>",
                num(x0),
                num(x1),
                node_color(link.source),
                node_color(link.target)
            );
            let id = format!("{id_prefix}-sankey-{}", content_id(&stops));
            let def = format!(
                "<linearGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" {stops}</linearGradient>"
            );
            match gradients.iter_mut().find(|(k, _)| *k == id) {
                Some(slot) => slot.1 = def,
                None => gradients.push((id.clone(), def)),
            }
            paint = format!("fill=\"url(#{id}) {}\"", node_color(link.source));
        }
        let mid = num((x0 + x1) / 2.0);
        let from = esc_text(&model.nodes[link.source].id);
        let to = esc_text(&model.nodes[link.target].id);
        links_out.push_str(&format!(
            "<path class=\"pele-sankey-link\" data-id=\"{from}-&gt;{to}\" d=\"M{},{}C{mid},{} {mid},{} {},{}L{},{}C{mid},{} {mid},{} {},{}Z\" {paint}><title>{from} → {to}: {}{}{}</title></path>",
            num(x0),
            num(link.source_y - half),
            num(link.source_y - half),
            num(link.target_y - half),
            num(x1),
            num(link.target_y - half),
            num(x1),
            num(link.target_y + half),
            num(link.target_y + half),
            num(link.source_y + half),
            num(x0),
            num(link.source_y + half),
            esc(&prefix),
            js::fmt(rounded(link.value)),
            esc(&suffix)
        ));
    }

    let title = layout_label(
        model.title.as_deref(),
        false,
        measurer,
        size,
        title_room(options),
        style::BOLD,
    );
    let title_height = if title.height > 0.0 {
        title.height + 12.0
    } else {
        0.0
    };
    let inner = js::max(max_x - min_x, title.width);
    let ox = pad - min_x + (inner - (max_x - min_x)) / 2.0;
    let oy = pad + title_height - min_y;
    let width = (inner + 2.0 * pad).ceil();
    let height = (max_y - min_y + title_height + 2.0 * pad).ceil();

    let mut content = label_svg(
        &title,
        width / 2.0,
        pad + title.height / 2.0,
        " class=\"pele-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    content.push_str(&format!(
        "<g transform=\"translate({},{})\">",
        num(ox),
        num(oy)
    ));
    if !gradients.is_empty() {
        content.push_str("<defs>");
        for (_, g) in &gradients {
            content.push_str(g);
        }
        content.push_str("</defs>");
    }
    content.push_str(&format!(
        "<g class=\"pele-sankey-links\" fill-opacity=\"0.4\">{links_out}</g><g class=\"pele-sankey-nodes\">{nodes_out}</g><g class=\"pele-labels\" font-size=\"{}\">{labels_out}</g></g>",
        js::fmt(small)
    ));

    let acc = Accessible {
        acc_title: model.acc_title.as_deref(),
        acc_descr: model.acc_descr.as_deref(),
    };
    let svg = svg_document("sankey", width, height, size, options, &acc, &content);
    Ok(Rendered {
        svg,
        width,
        height,
        links: Vec::new(),
    })
}
