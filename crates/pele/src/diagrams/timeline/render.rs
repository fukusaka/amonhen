//! timeline を描く（pele の `src/diagrams/timeline/render.ts`）。

use super::db::TimelineDb;
use crate::diagrams::common::fit::{fit_label, plain_label};
use crate::diagrams::common::fit_width::turn_to_fit;
use crate::errors::Result;
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc_text, label_svg, num};
use crate::svg::edges::{ARROW, marker};
use crate::svg::palette::series_ink_fill;
use crate::svg::root::{Accessible, svg_document};
use crate::svg::theme::{rounded, series_color};
use crate::text::label::Label;
use crate::text::measurer::{TextMeasurer, default_measurer, style};
use crate::types::{RenderOptions, Rendered};
use crate::util::yaml::Value;

const PAD_X: f64 = 12.0;
const PAD_Y: f64 = 8.0;
const MIN_TEXT: f64 = 152.0;
const MAX_TEXT: f64 = 216.0;
const EVENT_TEXT_TD: f64 = 264.0;
const GAP: f64 = 20.0;
const AXIS: f64 = 20.0;
const SPINE: f64 = 14.0;
const EVENT_GAP: f64 = 8.0;
const SECTION_GAP: f64 = 8.0;
const TAIL: f64 = 12.0;
const ON_COLOR: &str = " fill=\"var(--_bg)\"";
const LINE: &str = " fill=\"none\" stroke=\"var(--_b)\"";

struct Group {
    section: i64,
    periods: Vec<usize>,
}

fn rect(x: f64, y: f64, w: f64, h: f64, attrs: &str) -> String {
    format!(
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}/>",
        num(x),
        num(y),
        num(w),
        num(h),
        rounded(attrs)
    )
}

pub fn render_timeline(
    model: &TimelineDb,
    config: &Config,
    options: &RenderOptions,
) -> Result<Rendered> {
    if model.down {
        return Ok(draw(model, config, options, true));
    }
    turn_to_fit(options, |down| Ok(draw(model, config, options, down)))
}

fn draw(model: &TimelineDb, config: &Config, options: &RenderOptions, vertical: bool) -> Rendered {
    let size = options.font_size.unwrap_or(16.0);
    let small = js::round(size * 0.875);
    let measurer: &dyn TextMeasurer = match &options.measurer {
        Some(m) => m.as_ref(),
        None => default_measurer(),
    };
    let pad = options.padding.unwrap_or(8.0);
    let multicolor = config
        .get("timeline")
        .and_then(Value::as_obj)
        .and_then(|t| t.get("disableMulticolor"))
        != Some(&Value::Bool(true));
    let periods = &model.periods;
    let has_sections = !model.sections.is_empty();

    // Mermaid は最初の節の前の期間を描かない。ここでは色を付けずに先頭に置く。
    let mut groups: Vec<Group> = (0..model.sections.len())
        .map(|section| Group {
            section: section as i64,
            periods: Vec::new(),
        })
        .collect();
    let mut leading = Group {
        section: -1,
        periods: Vec::new(),
    };
    for (i, period) in periods.iter().enumerate() {
        match usize::try_from(period.section_index)
            .ok()
            .and_then(|k| groups.get_mut(k))
        {
            Some(group) => group.periods.push(i),
            None => leading.periods.push(i),
        }
    }
    if !leading.periods.is_empty() {
        groups.insert(0, leading);
    }

    // 列は、収まらない単語のためにある所まで広がる。その先は単語を切る。縦に流すときは、2 つの列がホストの幅を
    // 分け合う。
    let room = match options.max_width {
        Some(m) if vertical && m > 0.0 => m - 2.0 * pad,
        _ => f64::INFINITY,
    };
    let taken = AXIS + SPINE * 2.0 + 4.0 * PAD_X;
    let period_text = js::min(MIN_TEXT, js::max(80.0, js::round((room - taken) * 0.38)));
    let event_text = js::min(EVENT_TEXT_TD, js::max(110.0, room - taken - period_text));
    let mut text_w = period_text;
    let event_text_w = |text_w: f64| if vertical { event_text } else { text_w };
    let mut period_labels: Vec<Label> = periods
        .iter()
        .map(|p| plain_label(Some(&p.text), measurer, size, text_w, 0))
        .collect();
    let mut event_labels: Vec<Vec<Label>> = periods
        .iter()
        .map(|p| {
            p.events
                .iter()
                .map(|e| plain_label(Some(e), measurer, small, event_text_w(text_w), 0))
                .collect()
        })
        .collect();
    let mut widest: f64 = 0.0;
    for label in &period_labels {
        widest = js::max(widest, label.width);
    }
    if !vertical {
        for labels in &event_labels {
            for label in labels {
                widest = js::max(widest, label.width);
            }
        }
    }
    let mut overflow = widest > text_w;
    if vertical {
        for labels in &event_labels {
            for label in labels {
                overflow = overflow || label.width > event_text;
            }
        }
    }
    if overflow {
        text_w = js::min(
            js::max(widest.ceil(), period_text),
            if room == f64::INFINITY {
                MAX_TEXT
            } else {
                period_text
            },
        );
        period_labels = periods
            .iter()
            .map(|p| fit_label(Some(&p.text), measurer, size, text_w, 0))
            .collect();
        event_labels = periods
            .iter()
            .map(|p| {
                p.events
                    .iter()
                    .map(|e| fit_label(Some(e), measurer, small, event_text_w(text_w), 0))
                    .collect()
            })
            .collect();
    }

    let line_height = js::round(size * 1.5);
    let col_w = text_w + 2.0 * PAD_X;
    let event_w = event_text_w(text_w) + 2.0 * PAD_X;
    let chip_height = |label: &Label| js::max(label.height, line_height) + 2.0 * PAD_Y;
    let event_height = |label: &Label| js::max(label.height, js::round(small * 1.5)) + 2.0 * PAD_Y;
    let series_of = |group: &Group, period: usize| -> Option<i64> {
        if group.section >= 0 {
            Some(group.section)
        } else if has_sections {
            None
        } else if multicolor {
            Some(period as i64)
        } else {
            Some(0)
        }
    };
    let color_of = |group: &Group, period: usize| -> String {
        match series_of(group, period) {
            None => "var(--_m)".to_string(),
            Some(series) => series_color(series),
        }
    };
    let on_color = |series: Option<i64>| -> String {
        let fill = series.map(series_ink_fill).unwrap_or_default();
        if fill.is_empty() {
            ON_COLOR.to_string()
        } else {
            fill
        }
    };

    let has_events = event_labels.iter().any(|labels| !labels.is_empty());
    let mut slots = 0;
    for group in &groups {
        slots += group.periods.len().max(1);
    }

    let pitch = col_w + GAP;
    let drawn = !periods.is_empty();
    let body_w = if vertical {
        col_w
            + if has_events {
                AXIS + SPINE * 2.0 + event_w
            } else if drawn {
                AXIS + TAIL
            } else {
                0.0
            }
    } else if slots > 0 {
        slots as f64 * pitch - GAP + if drawn { TAIL + ARROW } else { 0.0 }
    } else {
        0.0
    };
    let title = fit_label(
        model.title.as_deref(),
        measurer,
        size,
        js::max(body_w, 320.0),
        style::BOLD,
    );
    let title_h = if title.height > 0.0 {
        title.height + 16.0
    } else {
        0.0
    };
    let width = js::max(body_w, title.width);
    let left = pad + (width - body_w) / 2.0;
    let top = pad + title_h;

    let section_label = |group: &Group, w: f64| -> Label {
        fit_label(
            Some(&model.sections[group.section as usize]),
            measurer,
            size,
            w - 2.0 * PAD_X,
            style::BOLD,
        )
    };
    let header = |group: &Group, label: &Label, x: f64, y: f64, w: f64, h: f64| -> String {
        rect(
            x,
            y,
            w,
            h,
            &format!(" fill=\"{}\"", series_color(group.section)),
        ) + &label_svg(
            label,
            x + w / 2.0,
            y + h / 2.0,
            &format!(
                " font-weight=\"var(--_hw)\"{}",
                on_color(Some(group.section))
            ),
            None,
        )
    };
    let period = |i: usize, group: &Group, color: &str, x: f64, y: f64, h: f64| -> String {
        rect(x, y, col_w, h, &format!(" fill=\"{color}\""))
            + &label_svg(
                &period_labels[i],
                x + col_w / 2.0,
                y + h / 2.0,
                &on_color(series_of(group, i)),
                None,
            )
    };
    let event = |label: &Label, color: &str, x: f64, y: f64, h: f64| -> String {
        format!(
            "<g class=\"pele-node pele-event\">{}{}</g>",
            rect(
                x,
                y,
                event_w,
                h,
                &format!(" fill=\"{color}\" fill-opacity=\"0.16\"")
            ),
            label_svg(
                label,
                x + event_w / 2.0,
                y + h / 2.0,
                &format!(" font-size=\"{}\"", js::fmt(small)),
                None
            )
        )
    };
    let dot = |x: f64, y: f64, color: &str| -> String {
        format!(
            "<circle class=\"pele-dot\" cx=\"{}\" cy=\"{}\" r=\"4\" fill=\"{color}\" stroke=\"var(--_bg)\" stroke-width=\"2\"/>",
            num(x),
            num(y)
        )
    };
    let open = |group: &Group| -> String {
        if group.section >= 0 {
            format!(
                "<g class=\"pele-cluster pele-section\" data-id=\"{}\">",
                esc_text(js::trim(&model.sections[group.section as usize]))
            )
        } else {
            String::new()
        }
    };
    let open_period = |i: usize| -> String {
        format!(
            "<g class=\"pele-node pele-period\" data-id=\"{}\">",
            esc_text(js::trim(&periods[i].text))
        )
    };

    let mut body = String::new();
    let height = if vertical {
        let axis_x = left + col_w + AXIS;
        let spine_x = axis_x + SPINE;
        let events_x = spine_x + SPINE;
        let mut y = top;
        for group in &groups {
            body.push_str(&open(group));
            if group.section >= 0 {
                let label = section_label(group, body_w);
                let h = chip_height(&label);
                body.push_str(&header(group, &label, left, y, body_w, h));
                y += h + SECTION_GAP + 4.0;
            }
            for &i in &group.periods {
                let color = color_of(group, i);
                let h = chip_height(&period_labels[i]);
                // 期間と最初の出来事は中心の線を同じくし、間の線をまっすぐにする。
                let first = match event_labels[i].first() {
                    Some(label) => event_height(label),
                    None => h,
                };
                let cy = y + js::max(h, first) / 2.0;
                let mut d = format!("M{},{}H{}", num(left + col_w), num(cy), num(axis_x));
                let mut cards = String::new();
                let mut ey = cy - first / 2.0;
                let mut last = cy;
                for label in &event_labels[i] {
                    let eh = event_height(label);
                    last = ey + eh / 2.0;
                    d.push_str(&format!(
                        "M{},{}H{}",
                        num(spine_x),
                        num(last),
                        num(events_x)
                    ));
                    cards.push_str(&event(label, &color, events_x, ey, eh));
                    ey += eh + EVENT_GAP;
                }
                if !cards.is_empty() {
                    d.push_str(&format!(
                        "M{},{}H{}V{}",
                        num(axis_x),
                        num(cy),
                        num(spine_x),
                        num(last)
                    ));
                }
                body.push_str(&format!(
                    "{}{}<path class=\"pele-edge\" d=\"{d}\"{LINE}/>{}{cards}</g>",
                    open_period(i),
                    period(i, group, &color, left, cy - h / 2.0, h),
                    dot(axis_x, cy, &color)
                ));
                y = js::max(cy + h / 2.0, ey - EVENT_GAP) + GAP;
            }
            if group.periods.is_empty() {
                y += GAP - SECTION_GAP - 4.0;
            }
            if group.section >= 0 {
                body.push_str("</g>");
            }
        }
        if !groups.is_empty() {
            y -= GAP;
        }
        if drawn {
            let tip = y + TAIL + ARROW;
            body = format!(
                "<path class=\"pele-axis\" d=\"M{},{}V{}\" fill=\"none\" stroke=\"var(--_l)\"/>{}{body}",
                num(axis_x),
                num(top),
                num(tip - ARROW + 1.0),
                marker("arrow_point", axis_x, tip, 0.0, 1.0, "var(--_l)", "none")
            );
            y = tip;
        }
        y + pad
    } else {
        let mut header_h: f64 = 0.0;
        let section_labels: Vec<Option<Label>> = groups
            .iter()
            .map(|group| {
                if group.section < 0 {
                    return None;
                }
                let label = section_label(group, group.periods.len().max(1) as f64 * pitch - GAP);
                header_h = js::max(header_h, chip_height(&label));
                Some(label)
            })
            .collect();
        let mut chip_h: f64 = 0.0;
        for label in &period_labels {
            chip_h = js::max(chip_h, chip_height(label));
        }
        let periods_y = top
            + if has_sections {
                header_h + SECTION_GAP
            } else {
                0.0
            };
        let axis_y = periods_y + chip_h + if chip_h > 0.0 { AXIS } else { 0.0 };
        let events_y = axis_y + AXIS;
        let mut bottom = if drawn { axis_y + TAIL } else { top + header_h };
        let mut slot = 0;
        for (g, group) in groups.iter().enumerate() {
            let span = group.periods.len().max(1);
            let gx = left + slot as f64 * pitch;
            body.push_str(&open(group));
            if let Some(label) = &section_labels[g] {
                body.push_str(&header(
                    group,
                    label,
                    gx,
                    top,
                    span as f64 * pitch - GAP,
                    header_h,
                ));
            }
            for (k, &i) in group.periods.iter().enumerate() {
                let color = color_of(group, i);
                let x = gx + k as f64 * pitch;
                let cx = x + col_w / 2.0;
                let mut d = format!(
                    "M{},{}V{}",
                    num(cx),
                    num(periods_y + chip_h),
                    num(if event_labels[i].is_empty() {
                        axis_y
                    } else {
                        events_y
                    })
                );
                let mut cards = String::new();
                let mut ey = events_y;
                for event_label in &event_labels[i] {
                    let eh = event_height(event_label);
                    if ey > events_y {
                        d.push_str(&format!("M{},{}V{}", num(cx), num(ey - EVENT_GAP), num(ey)));
                    }
                    cards.push_str(&event(event_label, &color, x, ey, eh));
                    ey += eh + EVENT_GAP;
                }
                bottom = js::max(bottom, ey - EVENT_GAP);
                body.push_str(&format!(
                    "{}{}<path class=\"pele-edge\" d=\"{d}\"{LINE}/>{}{cards}</g>",
                    open_period(i),
                    period(i, group, &color, x, periods_y, chip_h),
                    dot(cx, axis_y, &color)
                ));
            }
            if group.section >= 0 {
                body.push_str("</g>");
            }
            slot += span;
        }
        if drawn {
            let tip = left + body_w;
            body = format!(
                "<path class=\"pele-axis\" d=\"M{},{}H{}\" fill=\"none\" stroke=\"var(--_l)\"/>{}{body}",
                num(left),
                num(axis_y),
                num(tip - ARROW + 1.0),
                marker("arrow_point", tip, axis_y, 1.0, 0.0, "var(--_l)", "none")
            );
        }
        bottom + pad
    };

    let total_width = (width + 2.0 * pad).ceil();
    let total_height = js::max(height, pad * 2.0).ceil();
    let content = label_svg(
        &title,
        total_width / 2.0,
        pad + title.height / 2.0,
        " class=\"pele-title\" font-weight=\"var(--_tw)\"",
        None,
    ) + &body;
    let acc = Accessible {
        acc_title: model.acc_title.as_deref(),
        acc_descr: model.acc_descr.as_deref(),
    };
    Rendered {
        svg: svg_document(
            "timeline",
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
