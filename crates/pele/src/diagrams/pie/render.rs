//! pie を描く（pele の `src/diagrams/pie/render.ts`）。

use std::f64::consts::PI;

use super::model::PieModel;
use crate::diagrams::common::fit_width::{fit_width, title_room};
use crate::errors::Result;
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc_text, label_svg, num};
use crate::svg::palette::series_ink_fill;
use crate::svg::root::{Accessible, svg_document};
use crate::svg::theme::series_color;
use crate::text::label::{Label, layout_label};
use crate::text::measurer::{TextMeasurer, default_measurer, style};
use crate::types::{RenderOptions, Rendered};
use crate::util::yaml::Value;

const RADIUS: f64 = 120.0;
const SWATCH: f64 = 12.0;
const ROW: f64 = 26.0;
const GAP: f64 = 32.0;
const OUTSIDE: f64 = 28.0;

fn option<'a>(config: &'a Config, key: &str) -> Option<&'a Value> {
    config.get("pie").and_then(Value::as_obj)?.get(key)
}

/// `x.toFixed(0)`（0 以上の x）。同じ近さなら大きい方を取る。
pub fn to_fixed0(x: f64) -> String {
    let f = x.floor();
    js::fmt(if x - f >= 0.5 { f + 1.0 } else { f })
}

fn point(angle: f64, radius: f64) -> String {
    format!(
        "{},{}",
        num(angle.sin() * radius),
        num(-angle.cos() * radius)
    )
}

fn slice(start: f64, end: f64, outer: f64, inner: f64) -> String {
    if end - start >= PI * 2.0 - 1e-9 {
        let ring = |r: f64, sweep: i32| -> String {
            format!(
                "M0,{}A{},{} 0 1 {sweep} 0,{}A{},{} 0 1 {sweep} 0,{}Z",
                num(-r),
                num(r),
                num(r),
                num(r),
                num(r),
                num(r),
                num(-r)
            )
        };
        return ring(outer, 1)
            + &if inner > 0.0 {
                ring(inner, 0)
            } else {
                String::new()
            };
    }
    let large = if end - start > PI { 1 } else { 0 };
    if inner <= 0.0 {
        return format!(
            "M0,0L{}A{},{} 0 {large} 1 {}Z",
            point(start, outer),
            num(outer),
            num(outer),
            point(end, outer)
        );
    }
    format!(
        "M{}A{},{} 0 {large} 1 {}L{}A{},{} 0 {large} 0 {}Z",
        point(start, outer),
        num(outer),
        num(outer),
        point(end, outer),
        point(end, inner),
        num(inner),
        num(inner),
        point(start, inner)
    )
}

pub fn render_pie(model: &PieModel, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    fit_width(options, RADIUS * 2.0, 140.0, |diameter, tight| {
        Ok(draw(model, config, options, diameter / 2.0, tight))
    })
}

/// `tight` は、凡例を横に置いて使える幅に収まらないとき。
fn draw(
    model: &PieModel,
    config: &Config,
    options: &RenderOptions,
    radius: f64,
    tight: bool,
) -> Rendered {
    let size = options.font_size.unwrap_or(16.0);
    let small = js::round(size * 0.875);
    let measurer: &dyn TextMeasurer = match &options.measurer {
        Some(m) => m.as_ref(),
        None => default_measurer(),
    };
    let pad = options.padding.unwrap_or(8.0);

    let text_position = match option(config, "textPosition") {
        Some(Value::Num(p)) if *p >= 0.0 && *p <= 1.0 => *p,
        _ => 0.75,
    };
    let inner = match option(config, "donutHole") {
        Some(Value::Num(h)) if *h > 0.0 && *h <= 0.9 => h * radius,
        _ => 0.0,
    };
    let placed = match option(config, "legendPosition") {
        None | Some(Value::Null) => "right".to_string(),
        Some(v) => v.to_js_string(),
    };
    let legend_position: &str = if tight && (placed == "left" || placed == "right") {
        "bottom"
    } else {
        &placed
    };

    let entries = &model.sections;
    let mut sum = 0.0;
    for (_, value) in entries {
        sum += value;
    }

    let legend: Vec<Label> = entries
        .iter()
        .map(|(label, value)| {
            let text = if model.show_data {
                format!("{label} [{}]", js::fmt(*value))
            } else {
                label.clone()
            };
            layout_label(Some(&text), false, measurer, small, 4000.0, 0)
        })
        .collect();
    let mut legend_width: f64 = 0.0;
    for label in &legend {
        legend_width = js::max(legend_width, label.width);
    }
    legend_width += if legend.is_empty() { 0.0 } else { SWATCH + 8.0 };
    let legend_height = legend.len() as f64 * ROW;

    let title = layout_label(
        model.title.as_deref(),
        false,
        measurer,
        size,
        title_room(options),
        style::BOLD,
    );
    let title_height = if title.height > 0.0 {
        title.height + 16.0
    } else {
        0.0
    };

    // 割合の文字に細すぎる切れは、文字を縁の外に置く。それには余白が要る。
    let shares: Vec<f64> = entries
        .iter()
        .map(|(_, value)| {
            let share = if sum > 0.0 { value / sum } else { 0.0 };
            if share * 100.0 >= 1.0 && to_fixed0(share * 100.0) != "0" {
                share
            } else {
                0.0
            }
        })
        .collect();
    let narrow: Vec<bool> = shares
        .iter()
        .map(|&share| {
            if share == 0.0 || share >= 0.5 {
                return false;
            }
            let chord = 2.0 * radius * text_position * (share * PI).sin();
            chord < measurer.width(&(to_fixed0(share * 100.0) + "%"), small, 0) + 6.0
        })
        .collect();
    let margin = if narrow.contains(&true) { OUTSIDE } else { 0.0 };

    let side = match legend_position {
        "left" | "right" | "center" => legend_position,
        _ => "",
    };
    let stacked = legend_position == "top" || legend_position == "bottom";
    let diameter = (radius + margin) * 2.0;
    let mut width = diameter;
    let mut height = diameter;
    if side == "left" || side == "right" {
        width += if legend_width > 0.0 {
            GAP + legend_width
        } else {
            0.0
        };
        height = js::max(height, legend_height);
    } else if stacked {
        width = js::max(width, legend_width);
        height += if legend_height > 0.0 {
            20.0 + legend_height
        } else {
            0.0
        };
    }
    width = js::max(width, title.width);

    let cx = pad
        + if side == "left" {
            legend_width + GAP + diameter / 2.0
        } else if side == "right" {
            diameter / 2.0
        } else {
            width / 2.0
        };
    let body_top = pad + title_height;
    let cy = body_top
        + if legend_position == "top" {
            legend_height + 20.0 + diameter / 2.0
        } else if stacked {
            diameter / 2.0
        } else {
            height / 2.0
        };

    let mut slices = String::new();
    let mut labels = String::new();
    let mut outside = String::new();
    let mut angle = 0.0;
    let rim = radius + OUTSIDE / 2.0;
    let mut placed_outside: Vec<(f64, f64, f64)> = Vec::new();
    for (index, (label, _)) in entries.iter().enumerate() {
        let share = shares[index];
        if share == 0.0 {
            continue;
        }
        let end = angle + share * PI * 2.0;
        slices.push_str(&format!(
            "<path class=\"pele-slice\" data-id=\"{}\" d=\"{}\" fill=\"{}\"/>",
            esc_text(label),
            slice(angle, end, radius, inner),
            series_color(index as i64)
        ));
        let mid = (angle + end) / 2.0;
        let text = to_fixed0(share * 100.0) + "%";
        if narrow[index] {
            let width = measurer.width(&text, small, 0);
            let clear = |x: f64, y: f64, placed: &[(f64, f64, f64)]| {
                placed.iter().all(|&(ox, oy, ow)| {
                    (x - ox).abs() >= (width + ow) / 2.0 + 2.0 || (y - oy).abs() >= small + 2.0
                })
            };
            let mut at = mid;
            let mut step = 0;
            while step < 360 && !clear(at.sin() * rim, -at.cos() * rim, &placed_outside) {
                at += 2.0 / rim;
                step += 1;
            }
            let x = at.sin() * rim;
            let y = -at.cos() * rim;
            placed_outside.push((x, y, width));
            outside.push_str(&format!(
                "<text x=\"{}\" y=\"{}\">{text}</text>",
                num(x),
                num(y + small * 0.35)
            ));
        } else {
            let r = radius * text_position;
            labels.push_str(&format!(
                "<text x=\"{}\" y=\"{}\"{}>{text}</text>",
                num(mid.sin() * r),
                num(-mid.cos() * r + small * 0.35),
                series_ink_fill(index as i64)
            ));
        }
        angle = end;
    }
    if slices.is_empty() {
        slices = format!(
            "<circle class=\"pele-slice\" r=\"{}\" fill=\"none\" stroke=\"var(--_b)\"/>",
            js::fmt(radius)
        );
    }

    let mut legend_out = String::new();
    let (lx, ly) = if side == "right" {
        (
            cx + diameter / 2.0 + GAP,
            body_top + (height - legend_height) / 2.0,
        )
    } else if side == "left" {
        (pad, body_top + (height - legend_height) / 2.0)
    } else if side == "center" {
        (cx - legend_width / 2.0, cy - legend_height / 2.0)
    } else {
        (
            pad + (width - legend_width) / 2.0,
            if legend_position == "top" {
                body_top
            } else {
                body_top + diameter + 20.0
            },
        )
    };
    for (index, label) in legend.iter().enumerate() {
        let y = ly + index as f64 * ROW + ROW / 2.0;
        legend_out.push_str(&format!(
            "<g class=\"pele-legend-item\" data-id=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"2\" fill=\"{}\"/>{}</g>",
            esc_text(&entries[index].0),
            num(lx),
            num(y - SWATCH / 2.0),
            js::fmt(SWATCH),
            js::fmt(SWATCH),
            series_color(index as i64),
            label_svg(label, lx + SWATCH + 8.0 + label.width / 2.0, y, "", None)
        ));
    }

    let total_width = (width + 2.0 * pad).ceil();
    let total_height = (height + title_height + 2.0 * pad).ceil();
    let mut content = label_svg(
        &title,
        total_width / 2.0,
        pad + title.height / 2.0,
        " class=\"pele-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    content.push_str(&format!(
        "<g class=\"pele-slices\" transform=\"translate({},{})\" stroke=\"var(--_bg)\" stroke-width=\"1\" stroke-linejoin=\"round\">{slices}</g>",
        num(cx),
        num(cy)
    ));
    if !labels.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-slice-labels\" transform=\"translate({},{})\" font-size=\"{}\" text-anchor=\"middle\" fill=\"var(--_bg)\">{labels}</g>",
            num(cx),
            num(cy),
            js::fmt(small)
        ));
    }
    if !outside.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-slice-labels\" transform=\"translate({},{})\" font-size=\"{}\" text-anchor=\"middle\">{outside}</g>",
            num(cx),
            num(cy),
            js::fmt(small)
        ));
    }
    if !legend_out.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-legend\" font-size=\"{}\">{legend_out}</g>",
            js::fmt(small)
        ));
    }

    let acc = Accessible {
        acc_title: model.acc_title.as_deref(),
        acc_descr: model.acc_descr.as_deref(),
    };
    Rendered {
        svg: svg_document(
            "pie",
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
