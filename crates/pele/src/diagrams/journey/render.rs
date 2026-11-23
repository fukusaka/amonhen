//! journey を描く（pele の `src/diagrams/journey/render.ts`）。

use std::collections::HashMap;

use super::db::{JourneyDb, journey_actors};
use crate::diagrams::common::fit::{fit_label, plain_label};
use crate::diagrams::common::fit_width::turn_to_fit;
use crate::errors::Result;
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc_text, label_svg, num};
use crate::svg::root::{Accessible, svg_document};
use crate::svg::theme::{ROUNDED, series_color};
use crate::text::label::Label;
use crate::text::measurer::{TextMeasurer, default_measurer, style};
use crate::types::{RenderOptions, Rendered};

const PAD_X: f64 = 10.0;
const PAD_Y: f64 = 8.0;
const MIN_TEXT: f64 = 124.0;
const MAX_TEXT: f64 = 200.0;
const GAP: f64 = 12.0;
const BAND_PAD: f64 = 6.0;
const GUTTER: f64 = 24.0;
const LEVEL: f64 = 26.0;
const FACE: f64 = 11.0;
const DOT: f64 = 5.0;
const DOT_STEP: f64 = 13.0;
const LEGEND_GAP: f64 = 20.0;

const EYES: &str =
    "<circle cx=\"-3.6\" cy=\"-2.8\" r=\"1.3\"/><circle cx=\"3.6\" cy=\"-2.8\" r=\"1.3\"/>";

fn mouth(mood: &str) -> &'static str {
    match mood {
        "happy" => "M-5,2A6,6 0 0 0 5,2",
        "neutral" => "M-4.5,4.5H4.5",
        _ => "M-5,6.5A6,6 0 0 1 5,6.5",
    }
}

fn mood(score: f64) -> &'static str {
    if score > 3.0 {
        "happy"
    } else if score < 3.0 {
        "sad"
    } else {
        "neutral"
    }
}

fn face(x: f64, y: f64, mood: &str) -> String {
    format!(
        "<g class=\"pele-face pele-face-{mood}\" transform=\"translate({},{})\"><circle r=\"{}\" fill=\"var(--_bg)\" stroke=\"var(--_l)\"/><g fill=\"var(--_l)\">{EYES}</g><path d=\"{}\" fill=\"none\" stroke=\"var(--_l)\" stroke-linecap=\"round\"/></g>",
        num(x),
        num(y),
        js::fmt(FACE),
        mouth(mood)
    )
}

fn measurer_of(options: &RenderOptions) -> &dyn TextMeasurer {
    match &options.measurer {
        Some(m) => m.as_ref(),
        None => default_measurer(),
    }
}

pub fn render_journey(
    model: &JourneyDb,
    config: &Config,
    options: &RenderOptions,
) -> Result<Rendered> {
    turn_to_fit(options, |down| Ok(draw(model, config, options, down)))
}

fn accessible(model: &JourneyDb) -> Accessible<'_> {
    Accessible {
        acc_title: model.acc_title.as_deref(),
        acc_descr: model.acc_descr.as_deref(),
    }
}

struct LegendItem {
    name: String,
    label: Label,
    x: f64,
    y: f64,
    h: f64,
}

fn draw(model: &JourneyDb, _config: &Config, options: &RenderOptions, down: bool) -> Rendered {
    let size = options.font_size.unwrap_or(16.0);
    let small = js::round(size * 0.875);
    let tiny = js::round(size * 0.75);
    let measurer = measurer_of(options);
    let pad = options.padding.unwrap_or(8.0);
    if down {
        return vertical_journey(model, options);
    }
    let tasks = &model.tasks;
    let n = tasks.len();

    // `Task: 5:` は名のない人を挙げる。Mermaid はそれに点と空の凡例の行を与える。
    let names: Vec<String> = journey_actors(tasks)
        .into_iter()
        .filter(|name| !name.is_empty())
        .collect();
    let actor: HashMap<&str, i64> = names
        .iter()
        .enumerate()
        .map(|(i, name)| (name.as_str(), i as i64))
        .collect();

    // カードは、収まらない単語のためにある所まで広がる。その先は単語を切る。
    let mut text_w = MIN_TEXT;
    let mut labels: Vec<Label> = tasks
        .iter()
        .map(|task| plain_label(Some(&task.task), measurer, small, text_w, 0))
        .collect();
    let mut widest: f64 = 0.0;
    for label in &labels {
        widest = js::max(widest, label.width);
    }
    if widest > text_w {
        text_w = js::min(widest.ceil(), MAX_TEXT);
        labels = tasks
            .iter()
            .map(|task| fit_label(Some(&task.task), measurer, small, text_w, 0))
            .collect();
    }

    let card_w = text_w + 2.0 * PAD_X;
    let pitch = card_w + GAP;
    let plot_w = if n > 0 { n as f64 * pitch - GAP } else { 0.0 };

    let line_h = js::round(small * 1.5);
    let limit = js::max(plot_w, 240.0);
    let mut legend: Vec<LegendItem> = Vec::new();
    let mut legend_w: f64 = 0.0;
    let mut legend_h: f64 = 0.0;
    let mut lx = 0.0;
    for name in &names {
        let label = fit_label(Some(name), measurer, small, limit - 2.0 * DOT - 6.0, 0);
        let w = 2.0 * DOT + 6.0 + label.width;
        if lx > 0.0 && lx + w > limit {
            lx = 0.0;
            legend_h += 4.0;
        }
        let y = if lx > 0.0 {
            legend.last().map_or(0.0, |l| l.y)
        } else {
            legend_h
        };
        let h = js::max(label.height, line_h);
        legend.push(LegendItem {
            name: name.clone(),
            label,
            x: lx,
            y,
            h,
        });
        lx += w;
        legend_w = js::max(legend_w, lx);
        legend_h = js::max(legend_h, y + h);
        lx += LEGEND_GAP;
    }

    let body_w = if n > 0 { GUTTER + plot_w } else { 0.0 };
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
    let width = js::max_of(&[
        body_w,
        title.width,
        if n > 0 { GUTTER + legend_w } else { 0.0 },
    ]);
    let left = pad + (width - body_w) / 2.0 + GUTTER;
    let top = pad + title_h;

    // 帯は、Mermaid と同じく節を同じくする作業の並びを覆う。作業のない節は何も描かない。
    let mut runs: Vec<(usize, usize, Option<Label>)> = Vec::new();
    let mut band_h: f64 = 0.0;
    let mut i = 0;
    while i < n {
        let mut end = i + 1;
        while end < n && tasks[end].section == tasks[i].section {
            end += 1;
        }
        let mut label = None;
        if !tasks[i].section.is_empty() {
            let l = fit_label(
                Some(&tasks[i].section),
                measurer,
                small,
                (end - i) as f64 * pitch - GAP - 2.0 * PAD_X,
                style::BOLD,
            );
            band_h = js::max(band_h, js::max(l.height, line_h) + 2.0 * BAND_PAD);
            label = Some(l);
        }
        runs.push((i, end - i, label));
        i = end;
    }

    let mut text_h = line_h;
    for label in &labels {
        text_h = js::max(text_h, label.height);
    }
    let dots_h = if names.is_empty() {
        0.0
    } else {
        2.0 * DOT + 6.0
    };
    let card_h = text_h + dots_h + 2.0 * PAD_Y;
    let cards_y = top + if band_h > 0.0 { band_h + BAND_PAD } else { 0.0 };
    let chart_top = cards_y + card_h + FACE + 16.0;
    let chart_bottom = chart_top + 4.0 * LEVEL;
    let right = left + plot_w;

    let mut body = String::new();
    let mut bottom = top;
    if n > 0 {
        let mut grid = String::new();
        let mut scale = String::new();
        for score in (1..=5).rev() {
            let y = chart_top + (5 - score) as f64 * LEVEL;
            grid.push_str(&format!("M{},{}H{}", num(left - 6.0), num(y), num(right)));
            scale.push_str(&format!(
                "<text x=\"{}\" y=\"{}\">{score}</text>",
                num(left - 12.0),
                num(y + tiny * 0.35)
            ));
        }
        body.push_str(&format!(
            "<path class=\"pele-grid\" d=\"{grid}\" fill=\"none\" stroke=\"var(--_a)\"/><g class=\"pele-scale\" font-size=\"{}\" text-anchor=\"end\" fill=\"var(--_m)\">{scale}</g>",
            js::fmt(tiny)
        ));

        let mut line = String::new();
        let mut stems = String::new();
        let mut faces = String::new();
        let mut cards: Vec<String> = Vec::new();
        for (i, task) in tasks.iter().enumerate() {
            let x = left + i as f64 * pitch;
            let cx = x + card_w / 2.0;
            let score = task.score;
            let level = if score.is_nan() {
                3.0
            } else {
                js::min(5.0, js::max(1.0, score))
            };
            let y = chart_top + (5.0 - level) * LEVEL;
            line.push_str(&format!(
                "{}{},{}",
                if i == 0 { 'M' } else { 'L' },
                num(cx),
                num(y)
            ));
            stems.push_str(&format!(
                "M{},{}V{}",
                num(cx),
                num(cards_y + card_h),
                num(y - FACE)
            ));
            faces.push_str(&face(cx, y, mood(score)));

            let mut dots = String::new();
            let people: Vec<&String> = task.people.iter().filter(|n| !n.is_empty()).collect();
            let count = people.len() as f64;
            let step = js::min(
                DOT_STEP,
                (card_w - 2.0 * PAD_X - 2.0 * DOT) / js::max(count - 1.0, 1.0),
            );
            for (k, name) in people.iter().enumerate() {
                dots.push_str(&format!(
                    "<circle class=\"pele-actor\" cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{}\" stroke=\"var(--_s)\"><title>{}</title></circle>",
                    num(cx + (k as f64 - (count - 1.0) / 2.0) * step),
                    num(cards_y + card_h - PAD_Y - DOT),
                    js::fmt(DOT),
                    series_color(actor.get(name.as_str()).copied().unwrap_or(0)),
                    esc_text(name)
                ));
            }
            cards.push(format!(
                "<g class=\"pele-node pele-task\" data-id=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_s)\" stroke=\"var(--_b)\"{ROUNDED}/>{}{dots}</g>",
                esc_text(js::trim(&task.task)),
                num(x),
                num(cards_y),
                num(card_w),
                num(card_h),
                label_svg(
                    &labels[i],
                    cx,
                    cards_y + PAD_Y + text_h / 2.0,
                    &format!(" font-size=\"{}\"", js::fmt(small)),
                    None
                )
            ));
        }
        body.push_str(&format!(
            "<path class=\"pele-stems\" d=\"{stems}\" fill=\"none\" stroke=\"var(--_b)\" stroke-dasharray=\"2 3\"/>"
        ));
        if n > 1 {
            body.push_str(&format!(
                "<path class=\"pele-edge\" d=\"{line}\" fill=\"none\" stroke=\"var(--_l)\" stroke-linejoin=\"round\"/>"
            ));
        }
        body.push_str(&faces);

        for (start, count, label) in &runs {
            let members = cards[*start..start + count].concat();
            let Some(label) = label else {
                body.push_str(&members);
                continue;
            };
            let x = left + *start as f64 * pitch;
            let w = *count as f64 * pitch - GAP;
            body.push_str(&format!(
                "<g class=\"pele-cluster pele-section\" data-id=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_a)\"{ROUNDED}/>{}{members}</g>",
                esc_text(js::trim(&tasks[*start].section)),
                num(x),
                num(top),
                num(w),
                num(band_h),
                label_svg(
                    label,
                    x + w / 2.0,
                    top + band_h / 2.0,
                    &format!(" font-size=\"{}\" font-weight=\"var(--_hw)\"", js::fmt(small)),
                    None
                )
            ));
        }
        bottom = chart_bottom + FACE;

        if !legend.is_empty() {
            let x0 = left + (plot_w - legend_w) / 2.0;
            let y0 = bottom + 16.0;
            let mut items = String::new();
            for item in &legend {
                let cy = y0 + item.y + item.h / 2.0;
                items.push_str(&format!(
                    "<g class=\"pele-legend-item\" data-id=\"{}\"><circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{}\"/>{}</g>",
                    esc_text(&item.name),
                    num(x0 + item.x + DOT),
                    num(cy),
                    js::fmt(DOT),
                    series_color(actor.get(item.name.as_str()).copied().unwrap_or(0)),
                    label_svg(
                        &item.label,
                        x0 + item.x + 2.0 * DOT + 6.0 + item.label.width / 2.0,
                        cy,
                        "",
                        None
                    )
                ));
            }
            body.push_str(&format!(
                "<g class=\"pele-legend\" font-size=\"{}\">{items}</g>",
                js::fmt(small)
            ));
            bottom = y0 + legend_h;
        }
    }

    let total_width = (width + 2.0 * pad).ceil();
    let total_height = (js::max(bottom, pad) + pad).ceil();
    let content = label_svg(
        &title,
        pad + width / 2.0,
        pad + title.height / 2.0,
        " class=\"pele-title\" font-weight=\"var(--_tw)\"",
        None,
    ) + &body;
    Rendered {
        svg: svg_document(
            "journey",
            total_width,
            total_height,
            size,
            options,
            &accessible(model),
            &content,
        ),
        width: total_width,
        height: total_height,
        links: Vec::new(),
    }
}

fn vertical_journey(model: &JourneyDb, options: &RenderOptions) -> Rendered {
    let size = options.font_size.unwrap_or(16.0);
    let small = js::round(size * 0.875);
    let measurer = measurer_of(options);
    let pad = options.padding.unwrap_or(8.0);
    let room = js::max(140.0, options.max_width.unwrap_or(320.0) - 2.0 * pad);
    let title = fit_label(model.title.as_deref(), measurer, size, room, style::BOLD);
    let names: Vec<String> = journey_actors(&model.tasks)
        .into_iter()
        .filter(|name| !name.is_empty())
        .collect();
    let actors: HashMap<&str, i64> = names
        .iter()
        .enumerate()
        .map(|(i, name)| (name.as_str(), i as i64))
        .collect();
    let mut y = pad
        + if title.height != 0.0 {
            title.height + 16.0
        } else {
            0.0
        };
    let mut section: Option<&str> = None;
    let mut body = String::new();
    let mut width = title.width;
    let count = model.tasks.len();
    for (index, task) in model.tasks.iter().enumerate() {
        if section != Some(task.section.as_str()) {
            section = Some(&task.section);
            let head = fit_label(
                Some(&task.section),
                measurer,
                size,
                room - 16.0,
                style::BOLD,
            );
            if head.height != 0.0 {
                body.push_str(&label_svg(
                    &head,
                    pad + 8.0 + head.width / 2.0,
                    y + head.height / 2.0,
                    " class=\"pele-cluster-label\" font-weight=\"var(--_hw)\"",
                    None,
                ));
                width = js::max(width, head.width + 16.0);
                y += head.height + 8.0;
            }
        }
        let label = fit_label(Some(&task.task), measurer, small, room - 72.0, 0);
        let people: Vec<(&String, Label)> = task
            .people
            .iter()
            .filter(|name| !name.is_empty())
            .map(|name| (name, fit_label(Some(name), measurer, small, room - 44.0, 0)))
            .collect();
        let text_h = js::max(label.height, 42.0);
        let mut h = 2.0 * PAD_Y + text_h;
        for (_, label) in &people {
            h += js::max(16.0, label.height) + 4.0;
        }
        let w = js::max(room, label.width + 72.0);
        body.push_str(&format!(
            "<g class=\"pele-node pele-journey-step\" data-id=\"{index}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_s)\" stroke=\"var(--_b)\"{ROUNDED}/>",
            num(pad),
            num(y),
            num(w),
            num(h)
        ));
        body.push_str(&label_svg(
            &label,
            pad + PAD_X + label.width / 2.0,
            y + PAD_Y + label.height / 2.0,
            " class=\"pele-label\"",
            None,
        ));
        let cx = pad + w - 28.0;
        body.push_str(&face(cx, y + PAD_Y + FACE, mood(task.score)));
        if !task.score.is_nan() {
            body.push_str(&format!(
                "<text class=\"pele-journey-score\" x=\"{}\" y=\"{}\" font-size=\"{}\" text-anchor=\"middle\">{}</text>",
                num(cx),
                num(y + PAD_Y + 2.0 * FACE + small),
                js::fmt(small),
                num(js::min(5.0, js::max(1.0, task.score)))
            ));
        }
        let mut py = y + PAD_Y + text_h + 4.0;
        for (name, label) in &people {
            let ph = js::max(16.0, label.height);
            body.push_str(&format!(
                "<circle class=\"pele-actor\" cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{}\"/>{}",
                num(pad + 16.0),
                num(py + ph / 2.0),
                js::fmt(DOT),
                series_color(actors[name.as_str()]),
                label_svg(
                    label,
                    pad + 28.0 + label.width / 2.0,
                    py + ph / 2.0,
                    &format!(
                        " class=\"pele-journey-actor\" font-size=\"{}\"",
                        js::fmt(small)
                    ),
                    None
                )
            ));
            py += ph + 4.0;
        }
        body.push_str("</g>");
        y += h;
        if index + 1 < count {
            body.push_str(&format!(
                "<path class=\"pele-edge\" d=\"M{},{}v16\" fill=\"none\" stroke=\"var(--_l)\"/>",
                num(pad + w / 2.0),
                num(y)
            ));
        }
        y += 16.0;
        width = js::max(width, w);
    }
    let total_width = (width + 2.0 * pad).ceil();
    let height = (js::max(pad + title.height, y - if count > 0 { 16.0 } else { 0.0 }) + pad).ceil();
    let content = label_svg(
        &title,
        total_width / 2.0,
        pad + title.height / 2.0,
        " class=\"pele-title\" font-weight=\"var(--_tw)\"",
        None,
    ) + &body;
    Rendered {
        svg: svg_document(
            "journey",
            total_width,
            height,
            size,
            options,
            &accessible(model),
            &content,
        ),
        width: total_width,
        height,
        links: Vec::new(),
    }
}
