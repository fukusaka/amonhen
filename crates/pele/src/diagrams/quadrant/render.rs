//! quadrantChart を描く（pele の `src/diagrams/quadrant/render.ts`）。

use crate::diagrams::common::fit_width::cap_width;
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc_text, label_svg, num};
use crate::svg::root::{Accessible, svg_document};
use crate::svg::theme::{ROUNDED, class_names, resolve_style, series_color};
use crate::text::label::{Label, layout_label};
use crate::text::measurer::{TextMeasurer, default_measurer, style};
use crate::types::{RenderOptions, Rendered};
use crate::util::yaml::Value;

use super::db::{QuadrantDb, QuadrantText};

const GAP: f64 = 4.0;
const CAREFUL: usize = 300;

struct Dot {
    cx: f64,
    cy: f64,
    reach: f64,
    text: Label,
}

fn setting<'a>(config: &'a Config, key: &str) -> Option<&'a Value> {
    config
        .get("quadrantChart")
        .and_then(Value::as_obj)
        .and_then(|c| c.get(key))
}

fn length(config: &Config, key: &str, fallback: f64, max: f64) -> f64 {
    match setting(config, key) {
        Some(Value::Num(v)) if *v >= 0.0 => js::min(*v, max),
        _ => fallback,
    }
}

/// Mermaid の quadrantChart の文字の大きさの設定。なければ fontSize に従う。
fn text_size(config: &Config, key: &str, fallback: f64) -> f64 {
    match setting(config, key) {
        Some(Value::Num(v)) if v.is_finite() => js::min(72.0, js::max(4.0, *v)),
        _ => fallback,
    }
}

fn hex(color: &str) -> String {
    if color.starts_with('#') {
        color.to_string()
    } else {
        format!("#{color}")
    }
}

/// 受け継ぐ大きさと違うときだけの大きさ。
fn size_attr(own: f64, inherited: f64) -> String {
    if own == inherited {
        String::new()
    } else {
        format!(" font-size=\"{}\"", js::fmt(own))
    }
}

pub fn render_quadrant(model: &QuadrantDb, config: &Config, options: &RenderOptions) -> Rendered {
    let size = options.font_size.unwrap_or(16.0);
    let title_size = text_size(config, "titleFontSize", size);
    let quadrant_size = text_size(config, "quadrantLabelFontSize", size);
    let x_axis_size = text_size(config, "xAxisLabelFontSize", js::round(size * 0.875));
    let y_axis_size = text_size(config, "yAxisLabelFontSize", js::round(size * 0.875));
    let point_size = text_size(config, "pointLabelFontSize", js::round(size * 0.75));
    let measurer: &dyn TextMeasurer = match &options.measurer {
        Some(m) => m.as_ref(),
        None => default_measurer(),
    };
    let natural = js::round(js::max(100.0, length(config, "chartWidth", 500.0, 4000.0)));
    let width = js::round(cap_width(options, natural, 280.0, 0.0));
    let height =
        js::round((js::max(100.0, length(config, "chartHeight", 500.0, 4000.0)) * width) / natural);
    let outer = length(
        config,
        "quadrantPadding",
        options.padding.unwrap_or(8.0),
        200.0,
    );
    let x_pad = length(config, "xAxisLabelPadding", 5.0, 200.0);
    let y_pad = length(config, "yAxisLabelPadding", 5.0, 200.0);
    let has_points = !model.points.is_empty();

    let label = |text: Option<&QuadrantText>, font_size: f64, max: f64| -> Label {
        layout_label(
            text.map(|t| t.text.as_str()),
            text.is_some_and(|t| t.markdown),
            measurer,
            font_size,
            js::max(max, 40.0),
            0,
        )
    };

    let title = layout_label(
        model.title.as_deref(),
        false,
        measurer,
        title_size,
        js::max(width - 2.0 * outer, 40.0),
        style::BOLD,
    );
    let title_pad = length(config, "titlePadding", 10.0, 200.0);
    let title_space = if title.height > 0.0 {
        title.height + title_pad
    } else {
        0.0
    };

    let filled = |t: &Option<QuadrantText>| t.as_ref().is_some_and(|t| !t.text.is_empty());
    let has_x = filled(&model.x_axis_left) || filled(&model.x_axis_right);
    let has_y = filled(&model.y_axis_bottom) || filled(&model.y_axis_top);
    // Mermaid は点があればすぐ x 軸のラベルを図の下に移す。
    let x_below =
        has_points || setting(config, "xAxisPosition").and_then(Value::as_str) == Some("bottom");
    let y_right = setting(config, "yAxisPosition").and_then(Value::as_str) == Some("right");

    let side = width - 2.0 * outer;
    let x_left = label(
        model.x_axis_left.as_ref(),
        x_axis_size,
        if model.x_axis_right.is_some() {
            side / 2.0 - 16.0
        } else {
            side
        },
    );
    let x_right = label(model.x_axis_right.as_ref(), x_axis_size, side / 2.0 - 16.0);
    let x_height = js::max(x_left.height, x_right.height);
    let x_space = if has_x { x_height + x_pad } else { 0.0 };
    let tall = height - 2.0 * outer - title_space - x_space;
    let y_bottom = label(
        model.y_axis_bottom.as_ref(),
        y_axis_size,
        if model.y_axis_top.is_some() {
            tall / 2.0 - 16.0
        } else {
            tall
        },
    );
    let y_top = label(model.y_axis_top.as_ref(), y_axis_size, tall / 2.0 - 16.0);
    let y_height = js::max(y_bottom.height, y_top.height);
    let y_space = if has_y { y_height + y_pad } else { 0.0 };

    let left = outer + if y_right { 0.0 } else { y_space };
    let top = outer + title_space + if x_below { 0.0 } else { x_space };
    let w = js::max(0.0, side - y_space);
    let h = js::max(0.0, tall);
    let tile_w = js::max(0.0, (w - GAP) / 2.0);
    let tile_h = js::max(0.0, (h - GAP) / 2.0);

    // 点のラベルが避ける箱: 象限の間のすき間、象限のラベル、点、先に置いたラベル。
    let mut taken: Vec<f64> = vec![
        left + tile_w,
        top,
        left + w - tile_w,
        top + h,
        left,
        top + tile_h,
        left + w,
        top + h - tile_h,
    ];
    let is_clear = |taken: &[f64], from: usize, x0: f64, y0: f64, x1: f64, y1: f64| -> bool {
        let mut k = from;
        while k < taken.len() {
            // 空の象限のラベルは点のラベルを妨げない。
            if !(taken[k] >= taken[k + 2] || taken[k + 1] >= taken[k + 3])
                && x0 < taken[k + 2]
                && x1 > taken[k]
                && y0 < taken[k + 3]
                && y1 > taken[k + 1]
            {
                return false;
            }
            k += 4;
        }
        true
    };

    let text_top = length(config, "quadrantTextTopPadding", 5.0, 200.0);
    let mut quadrants = String::new();
    for k in 0..4 {
        let x = if k == 0 || k == 3 {
            left + w - tile_w
        } else {
            left
        };
        let y = if k < 2 { top } else { top + h - tile_h };
        let text = label(model.quadrants[k].as_ref(), quadrant_size, tile_w - 16.0);
        let cx = x + tile_w / 2.0;
        let cy = if has_points {
            y + text_top + text.height / 2.0
        } else {
            y + tile_h / 2.0
        };
        taken.extend([
            cx - text.width / 2.0,
            cy - text.height / 2.0,
            cx + text.width / 2.0,
            cy + text.height / 2.0,
        ]);
        quadrants.push_str(&format!(
            "<g class=\"pele-quadrant\" data-id=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{ROUNDED}/>{}</g>",
            k + 1,
            num(x),
            num(y),
            num(tile_w),
            num(tile_h),
            label_svg(
                &text,
                cx,
                cy,
                &format!(
                    " class=\"pele-label\"{} fill=\"var(--_fg)\" stroke=\"none\"",
                    size_attr(quadrant_size, size)
                ),
                None
            )
        ));
    }

    let mut axes = String::new();
    if has_x {
        let y = if x_below {
            top + h + x_pad + x_height / 2.0
        } else {
            top - x_pad - x_height / 2.0
        };
        axes.push_str(&label_svg(
            &x_left,
            if x_right.height > 0.0 {
                left + w / 4.0
            } else {
                left + x_left.width / 2.0
            },
            y,
            "",
            None,
        ));
        axes.push_str(&label_svg(&x_right, left + (w * 3.0) / 4.0, y, "", None));
    }
    if has_y {
        let x = if y_right {
            left + w + y_pad + y_height / 2.0
        } else {
            left - y_pad - y_height / 2.0
        };
        let turned = |text: &Label, y: f64| -> String {
            if text.height > 0.0 {
                format!(
                    "<g transform=\"translate({},{}) rotate(-90)\"{}>{}</g>",
                    num(x),
                    num(y),
                    size_attr(y_axis_size, x_axis_size),
                    label_svg(text, 0.0, 0.0, "", None)
                )
            } else {
                String::new()
            }
        };
        axes.push_str(&turned(
            &y_bottom,
            if y_top.height > 0.0 {
                top + (h * 3.0) / 4.0
            } else {
                top + h - y_bottom.width / 2.0
            },
        ));
        axes.push_str(&turned(&y_top, top + h / 4.0));
    }

    let radius = length(config, "pointRadius", 5.0, 200.0);
    let text_pad = length(config, "pointTextPadding", 5.0, 200.0);
    let fill = series_color(0);
    let mut dots: Vec<Dot> = Vec::new();
    let mut circles: Vec<String> = Vec::new();
    for point in &model.points {
        if !(point.x >= 0.0 && point.x <= 1.0 && point.y >= 0.0 && point.y <= 1.0) {
            continue;
        }
        let shared = model.class(&point.class_name);
        let r = js::min(
            point
                .style
                .radius
                .or(shared.and_then(|s| s.radius))
                .unwrap_or(radius),
            2000.0,
        );
        let color = point
            .style
            .color
            .as_ref()
            .or(shared.and_then(|s| s.color.as_ref()));
        let stroke_color = point
            .style
            .stroke_color
            .as_ref()
            .or(shared.and_then(|s| s.stroke_color.as_ref()));
        let stroke_width = point
            .style
            .stroke_width
            .as_ref()
            .or(shared.and_then(|s| s.stroke_width.as_ref()));
        let mut styles: Vec<String> = Vec::new();
        if let Some(c) = color.filter(|c| !c.is_empty()) {
            styles.push(format!("fill:{}", hex(c)));
        }
        let stroke_width = stroke_width.filter(|s| !s.is_empty());
        let stroke_color = stroke_color.filter(|s| !s.is_empty());
        if let Some(sw) = stroke_width {
            if let Some(sc) = stroke_color {
                styles.push(format!("stroke:{}", hex(sc)));
            }
            styles.push(format!("stroke-width:{sw}"));
        }
        let cx = left + point.x * w;
        let cy = top + (1.0 - point.y) * h;
        dots.push(Dot {
            cx,
            cy,
            reach: r + match stroke_width {
                Some(sw) => js::min(js::parse_int(sw, 10), 400.0) / 2.0,
                None => 0.0,
            },
            // Mermaid は点のラベルを 1 行に保つ。図より広いものだけ折り返す。
            text: label(Some(&point.text), point_size, w),
        });
        circles.push(format!(
            "<circle class=\"pele-point{}\" data-id=\"{}\" cx=\"{}\" cy=\"{}\" r=\"{}\"{}{}/>",
            class_names(&point.class_name),
            esc_text(&point.text.text),
            num(cx),
            num(cy),
            num(r),
            if stroke_width.is_some() && stroke_color.is_none() {
                format!(" stroke=\"{fill}\"")
            } else {
                String::new()
            },
            resolve_style(&styles).shape
        ));
    }

    // ラベルは点の下・上・右・左か、象限の題の下の、図の中で空いている最初の所に置く。2 巡目は象限の間の
    // すき間をまたいでよい。大きな図は空いた所を探さない。
    let careful = dots.len() <= CAREFUL;
    if careful {
        for d in &dots {
            taken.extend([
                d.cx - d.reach,
                d.cy - d.reach,
                d.cx + d.reach,
                d.cy + d.reach,
            ]);
        }
    }
    let mut labels = String::new();
    for d in &dots {
        let (cx, cy, reach, text) = (d.cx, d.cy, d.reach, &d.text);
        let half_w = text.width / 2.0;
        let half_h = text.size * 0.6 + (text.height - text.line_height) / 2.0;
        let away = reach + text_pad;
        let inset = js::min(away, js::max(0.0, (tile_w - text.width) / 2.0));
        let heading = 8 + 4 * if cx < left + w / 2.0 {
            if cy < top + h / 2.0 { 1 } else { 2 }
        } else if cy < top + h / 2.0 {
            0
        } else {
            3
        };
        let below = js::max(cy + away, taken[heading + 3] + text_pad) + half_h;
        let spots = [
            cx,
            cy + away + half_h,
            cx,
            cy - away - half_h,
            cx + away + half_w,
            cy,
            cx - away - half_w,
            cy,
            cx,
            below,
        ];
        let mut tx = cx;
        let mut ty = spots[1];
        let mut found = false;
        let mut k = 0;
        while k < 20 {
            let spot = k % 10;
            // 題の下に移したラベルは図の縁から離す。
            let margin = if spot == 8 { inset } else { 0.0 };
            let x = if (spot < 4 || spot == 8) && text.width < w {
                js::min(
                    js::max(spots[spot], left + half_w + margin),
                    left + w - half_w - margin,
                )
            } else {
                spots[spot]
            };
            let y = spots[spot + 1];
            k += 2;
            if x - half_w < left
                || x + half_w > left + w
                || y - half_h < top
                || y + half_h > top + h
            {
                continue;
            }
            let free = !careful
                || is_clear(
                    &taken,
                    if k - 2 < 10 { 0 } else { 8 },
                    x - half_w,
                    y - half_h,
                    x + half_w,
                    y + half_h,
                );
            if !found || free {
                tx = x;
                ty = y;
                found = true;
            }
            if free {
                break;
            }
        }
        if careful {
            taken.extend([tx - half_w, ty - half_h, tx + half_w, ty + half_h]);
        }
        labels.push_str(&label_svg(text, tx, ty, " class=\"pele-label\"", None));
    }

    // Mermaid は新しい点を前の点の下に置く。
    let mut content = label_svg(
        &title,
        width / 2.0,
        title_pad / 2.0 + title.height / 2.0,
        &format!(
            " class=\"pele-title\"{} font-weight=\"var(--_tw)\"",
            size_attr(title_size, size)
        ),
        None,
    );
    content.push_str(&format!(
        "<g class=\"pele-quadrants\" fill=\"var(--_s)\" stroke=\"var(--_b)\">{quadrants}</g>"
    ));
    if !axes.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-axis-labels\" font-size=\"{}\" fill=\"var(--_m)\">{axes}</g>",
            js::fmt(x_axis_size)
        ));
    }
    if !dots.is_empty() {
        circles.reverse();
        content.push_str(&format!(
            "<g class=\"pele-points\" fill=\"{fill}\">{}</g><g class=\"pele-point-labels\" font-size=\"{}\">{labels}</g>",
            circles.join(""),
            js::fmt(point_size)
        ));
    }

    let acc = Accessible {
        acc_title: model.acc_title.as_deref(),
        acc_descr: model.acc_descr.as_deref(),
    };
    Rendered {
        svg: svg_document(
            "quadrantChart",
            width,
            height,
            size,
            options,
            &acc,
            &content,
        ),
        width,
        height,
        links: Vec::new(),
    }
}
