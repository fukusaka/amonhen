//! gantt を描く（pele の `src/diagrams/gantt/render.ts`）。

use std::collections::HashMap;

use super::axis::{auto_ticks, interval_ticks, time_format};
use super::dates::{Unit, add, add_days};
use super::db::{GanttDb, clock};
use super::js_date as d;
use super::types::GanttTask;
use crate::diagrams::common::fit_width::cap_width;
use crate::errors::Result;
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num};
use crate::svg::root::{Accessible, svg_document};
use crate::svg::theme::{CRITICAL, ROUNDED, resolve_style};
use crate::text::label::{Label, layout_label};
use crate::text::measurer::{TextMeasurer, default_measurer, style};
use crate::types::{LinkInfo, RenderOptions, Rendered};
use crate::util::url::linked;
use crate::util::yaml::Value;

const WIDTH: f64 = 800.0;
const MIN_PLOT: f64 = 240.0;
const LABEL_GAP: f64 = 8.0;

fn section(config: &Config) -> Option<&crate::util::yaml::Map> {
    config.get("gantt").and_then(Value::as_obj)
}

fn setting(config: &Config, key: &str, min: f64, max: f64) -> Option<f64> {
    match section(config)?.get(key)? {
        Value::Num(v) if v.is_finite() => Some(js::min(max, js::max(min, *v))),
        _ => None,
    }
}

fn text(config: &Config, key: &str) -> Option<String> {
    match section(config)?.get(key)? {
        Value::Str(s) => Some(s.clone()),
        _ => None,
    }
}

fn flag(config: &Config, key: &str) -> bool {
    section(config).and_then(|s| s.get(key)) == Some(&Value::Bool(true))
}

/// `raw.replace(/<br\s*\/?>/gi, ' ')`
fn without_breaks(raw: &str) -> String {
    let s: Vec<u16> = raw.encode_utf16().collect();
    let mut out: Vec<u16> = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == 60
            && s.get(i + 1).is_some_and(|&c| c | 32 == 98)
            && s.get(i + 2).is_some_and(|&c| c | 32 == 114)
        {
            let mut k = i + 3;
            while k < s.len() && crate::util::chars::is_space(s[k] as u32) {
                k += 1;
            }
            if s.get(k) == Some(&47) {
                k += 1;
            }
            if s.get(k) == Some(&62) {
                out.push(32);
                i = k + 1;
                continue;
            }
        }
        out.push(s[i]);
        i += 1;
    }
    js::from16(&out)
}

/// タスクのラベルは 1 行に置くので、最初の行だけ残す。
fn one_line(raw: &str, measurer: &dyn TextMeasurer, size: f64) -> Label {
    let label = layout_label(Some(&without_breaks(raw)), false, measurer, size, 4000.0, 0);
    if label.lines.len() <= 1 {
        return label;
    }
    Label {
        lines: vec![label.lines[0].clone()],
        widths: vec![label.widths[0]],
        width: label.widths[0],
        height: label.line_height,
        ..label
    }
}

/// 予定がタスクに始まりと終わりを与えたか。解けないタスクを待つものにはどちらも無い。
fn scheduled(task: &GanttTask) -> bool {
    task.start_time.is_some_and(f64::is_finite) && task.end_time.is_some_and(f64::is_finite)
}

fn start_of(task: &GanttTask) -> f64 {
    task.start_time.unwrap_or(f64::NAN)
}

fn end_of(task: &GanttTask) -> f64 {
    task.end_time.unwrap_or(f64::NAN)
}

/// Mermaid の compact: 節の中で、タスクは始まる時に空いている最初の行に置く。
fn pack_rows(
    tasks: &[usize],
    all: &[GanttTask],
    offset: usize,
    row_of: &mut HashMap<usize, usize>,
) -> usize {
    let mut sorted: Vec<usize> = tasks
        .iter()
        .copied()
        .filter(|&k| scheduled(&all[k]))
        .collect();
    sorted.sort_by(|&a, &b| {
        let by_start = start_of(&all[a]) - start_of(&all[b]);
        let key = if js::truthy(by_start) {
            by_start
        } else {
            (all[a].order - all[b].order) as f64
        };
        js::order(key)
    });
    let mut free: Vec<f64> = Vec::new();
    for &k in &sorted {
        let start = start_of(&all[k]);
        let mut row = 0;
        while row < free.len() && start < free[row] {
            row += 1;
        }
        if row == free.len() {
            free.push(end_of(&all[k]));
        } else {
            free[row] = end_of(&all[k]);
        }
        row_of.insert(k, offset + row);
    }
    let mut rows = free.len();
    for &k in tasks {
        if let std::collections::hash_map::Entry::Vacant(e) = row_of.entry(k) {
            e.insert(offset + rows);
            rows += 1;
        }
    }
    rows.max(1)
}

struct Bar {
    task: usize,
    label: Label,
    row: usize,
    x0: f64,
    x1: f64,
    label_x: f64,
    inside: bool,
}

pub fn render_gantt(model: &GanttDb, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let mut model = model.clone();
    let size = options.font_size.unwrap_or(16.0);
    let measurer: &dyn TextMeasurer = match &options.measurer {
        Some(m) => m.as_ref(),
        None => default_measurer(),
    };
    let pad = options.padding.unwrap_or(8.0);
    let now = clock(options.now);

    let label_size =
        setting(config, "fontSize", 4.0, 72.0).unwrap_or_else(|| js::round(size * 0.8125));
    let section_size = setting(config, "sectionFontSize", 4.0, 72.0).unwrap_or(label_size);
    let tick_size = js::round(size * 0.75);
    let bar_height =
        setting(config, "barHeight", 4.0, 400.0).unwrap_or_else(|| js::round(label_size * 1.85));
    let bar_gap =
        setting(config, "barGap", 0.0, 400.0).unwrap_or_else(|| js::round(bar_height / 4.0));
    let row_height = bar_height + bar_gap;
    let section_styles =
        js::round(setting(config, "numberSectionStyles", 1.0, 1000.0).unwrap_or(4.0));
    let compact = model.display_mode == "compact";
    let top_axis = model.top_axis || flag(config, "topAxis");

    model.get_tasks(now)?;
    let tasks = &model.tasks.clone();
    let row_tasks: Vec<usize> = (0..tasks.len()).filter(|&k| !tasks[k].vert).collect();
    let verts: Vec<usize> = (0..tasks.len())
        .filter(|&k| tasks[k].vert && scheduled(&tasks[k]))
        .collect();

    // 節の名前と、その現れた順の番号（JS の `Map`）。
    let mut categories: Vec<String> = Vec::new();
    for &k in &row_tasks {
        if !categories.contains(&tasks[k].section) {
            categories.push(tasks[k].section.clone());
        }
    }
    let category = |s: &str| categories.iter().position(|c| c == s).unwrap_or(0);

    let mut row_of: HashMap<usize, usize> = HashMap::new();
    let mut row_section: Vec<String> = Vec::new();
    if compact {
        let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
        for &k in &row_tasks {
            match groups.iter_mut().find(|(s, _)| *s == tasks[k].section) {
                Some((_, g)) => g.push(k),
                None => groups.push((tasks[k].section.clone(), vec![k])),
            }
        }
        for (section, group) in &groups {
            let rows = pack_rows(group, tasks, row_section.len(), &mut row_of);
            for _ in 0..rows {
                row_section.push(section.clone());
            }
        }
    } else {
        for &k in &row_tasks {
            row_of.insert(k, row_section.len());
            row_section.push(tasks[k].section.clone());
        }
    }
    let row_count = row_section.len();

    let mut start = f64::INFINITY;
    let mut stop = f64::NEG_INFINITY;
    for task in tasks {
        if !scheduled(task) {
            continue;
        }
        start = js::min(start, start_of(task));
        stop = js::max(stop, end_of(task));
    }
    let has_plot = start <= stop;

    let title = layout_label(
        model.title.as_deref(),
        false,
        measurer,
        size,
        4000.0,
        style::BOLD,
    );
    let title_height = if title.height > 0.0 {
        title.height + 16.0
    } else {
        0.0
    };
    let (acc_title, acc_descr) = (model.acc_title.clone(), model.acc_descr.clone());
    let acc = Accessible {
        acc_title: acc_title.as_deref(),
        acc_descr: acc_descr.as_deref(),
    };

    if !has_plot {
        let width = (js::max(title.width, 1.0) + 2.0 * pad).ceil();
        let height = (js::max(title.height, 1.0) + 2.0 * pad).ceil();
        let content = label_svg(
            &title,
            width / 2.0,
            pad + title.height / 2.0,
            " class=\"pele-title\" font-weight=\"var(--_tw)\"",
            None,
        );
        return Ok(Rendered {
            svg: svg_document("gantt", width, height, size, options, &acc, &content),
            width,
            height,
            links: Vec::new(),
        });
    }

    let mut section_labels: Vec<Label> = Vec::new();
    let mut section_width: f64 = 0.0;
    for section in &categories {
        let label = layout_label(Some(section), false, measurer, section_size, 180.0, 0);
        section_width = js::max(section_width, label.width);
        section_labels.push(label);
    }

    let axis_format = if !model.axis_format.is_empty() {
        model.axis_format.clone()
    } else if model.date_format == "D" {
        "%d".to_string()
    } else {
        text(config, "axisFormat").unwrap_or_else(|| "%Y-%m-%d".to_string())
    };
    let tick_label = |t: f64| time_format(&axis_format, d::time_clip(t));
    let sample_width = js::max(
        measurer.width(&tick_label(start), tick_size, 0),
        measurer.width(&tick_label(stop), tick_size, 0),
    );

    let left = pad
        + setting(config, "leftPadding", 0.0, 2000.0).unwrap_or_else(|| {
            js::max(
                if section_width > 0.0 {
                    section_width + 24.0
                } else {
                    0.0
                },
                sample_width / 2.0,
            )
        });
    let right_padding = setting(config, "rightPadding", 0.0, 2000.0)
        .unwrap_or_else(|| js::max(16.0, sample_width / 2.0));
    let target = cap_width(
        options,
        setting(config, "useWidth", 100.0, 20000.0).unwrap_or(WIDTH),
        280.0,
        0.0,
    );
    let plot_width = js::max(MIN_PLOT, target - left - right_padding - pad);
    let right = left + plot_width;
    let scale = if stop > start {
        plot_width / (stop - start)
    } else {
        0.0
    };
    let x = |t: f64| {
        if scale == 0.0 {
            left + plot_width / 2.0
        } else {
            left + (t - start) * scale
        }
    };

    let tick_height = js::round(tick_size * 1.75);
    let top_padding = setting(config, "topPadding", 0.0, 2000.0);
    let grid_start = setting(config, "gridLineStartPadding", 0.0, 2000.0);
    let head_height = pad + title_height + if top_axis { tick_height } else { 0.0 };
    let plot_top = js::max(head_height, top_padding.unwrap_or(0.0));
    // Mermaid は格子の線を、この 2 つの設定の差だけ最初の行の上から引く。ここでは大きい `topPadding` が空けた所
    // だけを使う。
    let grid_lead = match (top_padding, grid_start) {
        (Some(tp), Some(gs)) => js::max(0.0, js::min(tp - gs, plot_top - head_height)),
        _ => 0.0,
    };
    let plot_height = js::max(row_count as f64, 1.0) * row_height;
    let plot_bottom = plot_top + plot_height;

    // 棒とラベルの置き場所: 収まれば中、そうでなければ棒の横の空いている側。
    let mut bars: Vec<Bar> = Vec::new();
    let mut extent = right + right_padding;
    for &k in &row_tasks {
        let task = &tasks[k];
        let label = one_line(&task.task, measurer, label_size);
        let row = row_of[&k];
        if !scheduled(task) {
            let label_x = left + LABEL_GAP + label.width / 2.0;
            extent = js::max(extent, label_x + label.width / 2.0);
            bars.push(Bar {
                task: k,
                label,
                row,
                x0: f64::NAN,
                x1: f64::NAN,
                label_x,
                inside: false,
            });
            continue;
        }
        let mut x0 = x(start_of(task));
        let mut x1 = js::max(
            x(task.render_end_time.unwrap_or(end_of(task))),
            x0 + if task.milestone { 0.0 } else { 2.0 },
        );
        if task.milestone {
            let mid = x0 + (x(end_of(task)) - x0) / 2.0;
            x0 = mid - bar_height / 2.0;
            x1 = mid + bar_height / 2.0;
        }
        let inside = !task.milestone && label.width + 2.0 * LABEL_GAP <= x1 - x0;
        let mut label_x = (x0 + x1) / 2.0;
        if !inside {
            let fits_right = x1 + LABEL_GAP + label.width <= right + right_padding;
            let fits_left = x0 - LABEL_GAP - label.width >= left;
            label_x = if fits_right || !fits_left {
                x1 + LABEL_GAP + label.width / 2.0
            } else {
                x0 - LABEL_GAP - label.width / 2.0
            };
            extent = js::max(extent, label_x + label.width / 2.0);
        }
        bars.push(Bar {
            task: k,
            label,
            row,
            x0,
            x1,
            label_x,
            inside,
        });
    }

    let interval = match model.tick_interval.as_deref() {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => text(config, "tickInterval")
            .filter(|s| !s.is_empty())
            .unwrap_or_default(),
    };
    let weekday = model
        .weekday
        .clone()
        .or_else(|| text(config, "weekday"))
        .unwrap_or_else(|| "sunday".to_string());
    let count = js::max(
        2.0,
        js::min(10.0, (plot_width / (sample_width + 24.0)).floor()),
    );
    let ticks = interval_ticks(&interval, &weekday, start, stop, (plot_width / 3.0).ceil())
        .unwrap_or_else(|| auto_ticks(start, stop, count));

    let mut grid = String::new();
    let mut axis_bottom = String::new();
    let mut axis_top = String::new();
    let mut last_right = f64::NEG_INFINITY;
    let mut last_label = String::new();
    let bottom_y = plot_bottom + tick_height / 2.0 + 2.0;
    let top_y = plot_top - grid_lead - tick_height / 2.0 - 2.0;
    for &t in &ticks {
        let tx = x(t);
        grid.push_str(&format!(
            "M{},{}V{}",
            num(tx),
            num(plot_top - grid_lead),
            num(plot_bottom)
        ));
        let label = tick_label(t);
        let width = measurer.width(&label, tick_size, 0);
        // 前のラベルにぶつかるか、それを繰り返すだけのラベルは飛ばす。格子の線は残す。
        if tx - width / 2.0 < last_right + 8.0 || label == last_label {
            continue;
        }
        last_right = tx + width / 2.0;
        extent = js::max(extent, last_right);
        let body = format!("text-anchor=\"middle\">{}</text>", esc(&label));
        last_label = label;
        axis_bottom.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" {body}",
            num(tx),
            num(bottom_y + tick_size * 0.35)
        ));
        if top_axis {
            axis_top.push_str(&format!(
                "<text x=\"{}\" y=\"{}\" {body}",
                num(tx),
                num(top_y + tick_size * 0.35)
            ));
        }
    }

    // 縦の印は行を取らない。ラベルは軸の下に、離して置けるだけの行を使う。
    let mut vert_lines = String::new();
    let mut vert_labels = String::new();
    let mut vert_ends: Vec<f64> = Vec::new();
    let vert_top = plot_bottom + tick_height + 4.0;
    let vert_line = js::round(label_size * 1.5);
    let mut sorted_verts = verts.clone();
    sorted_verts.sort_by(|&a, &b| js::order(start_of(&tasks[a]) - start_of(&tasks[b])));
    for &k in &sorted_verts {
        let task = &tasks[k];
        let vx = x(start_of(task));
        let label = one_line(&task.task, measurer, label_size);
        let cx = js::max(pad + label.width / 2.0, vx);
        let mut line = 0;
        while line < vert_ends.len() && cx - label.width / 2.0 < vert_ends[line] + 8.0 {
            line += 1;
        }
        let end = cx + label.width / 2.0;
        if line == vert_ends.len() {
            vert_ends.push(end);
        } else {
            vert_ends[line] = end;
        }
        extent = js::max(extent, end);
        vert_lines.push_str(&format!(
            "<path class=\"pele-vert\" data-id=\"{}\" d=\"M{},{}V{}\"/>",
            esc_text(&task.id),
            num(vx),
            num(plot_top - grid_lead),
            num(plot_bottom)
        ));
        vert_labels.push_str(&label_svg(
            &label,
            cx,
            vert_top + line as f64 * vert_line + vert_line / 2.0,
            &format!(
                " class=\"pele-vert-label\" data-id=\"{}\"",
                esc_text(&task.id)
            ),
            None,
        ));
    }

    let width = (js::max(extent, pad + title.width) + pad).ceil();
    let height = (plot_bottom
        + tick_height
        + if vert_ends.is_empty() {
            0.0
        } else {
            4.0 + vert_ends.len() as f64 * vert_line
        }
        + pad)
        .ceil();

    // 節の帯。同じ節の行の並びごとに 1 つ、1 つおきの節に色をつける。
    let mut bands = String::new();
    let mut section_out = String::new();
    let mut row = 0;
    while row < row_count {
        let section = &row_section[row];
        let mut end = row + 1;
        while end < row_count && row_section[end] == *section {
            end += 1;
        }
        let index = (category(section) as f64) % section_styles;
        let y = plot_top + row as f64 * row_height;
        let h = (end - row) as f64 * row_height;
        if categories.len() > 1 || !section.is_empty() {
            bands.push_str(&format!(
                "<rect class=\"pele-section pele-section-{}\" x=\"0\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>",
                js::fmt(index),
                num(y),
                js::fmt(width),
                num(h),
                if index % 2.0 == 0.0 { "var(--_s)" } else { "none" }
            ));
        }
        let label = &section_labels[category(section)];
        section_out.push_str(&label_svg(
            label,
            pad + 8.0 + label.width / 2.0,
            y + h / 2.0,
            &format!(
                " class=\"pele-section-label\" data-id=\"{}\"",
                esc_text(section)
            ),
            options.icons.as_ref(),
        ));
        row = end;
    }

    // 除いた日。Mermaid と同じく、最初のタスクの始まりから 1 日ずつ、図が 5 年を超えるならまったく描かない。
    let mut excluded = String::new();
    if (!model.excludes.is_empty() || !model.includes.is_empty())
        && add(d::time_clip(start), 6.0, Unit::Y) > stop
    {
        let mut from: Option<f64> = None;
        let mut to: Option<f64> = None;
        let flush = |from: &mut Option<f64>, to: &mut Option<f64>, excluded: &mut String| {
            let (Some(f), Some(t)) = (*from, *to) else {
                return;
            };
            let day_start = d::set_hours(f, 0.0, 0.0, 0.0, 0.0);
            let day_end = d::set_hours(t, 23.0, 59.0, 59.0, 999.0);
            let ex0 = js::max(left, x(day_start));
            let ex1 = js::min(right, x(day_end));
            if ex1 > ex0 {
                excluded.push_str(&format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>",
                    num(ex0),
                    num(plot_top),
                    num(ex1 - ex0),
                    num(plot_height)
                ));
            }
            *from = None;
            *to = None;
        };
        let mut day = d::time_clip(start);
        while day <= stop {
            if model.is_invalid_date(day)? {
                if from.is_none() {
                    from = Some(day);
                }
                to = Some(day);
            } else {
                flush(&mut from, &mut to, &mut excluded);
            }
            day = add_days(day, 1.0);
        }
        flush(&mut from, &mut to, &mut excluded);
    }

    let mut links: Vec<LinkInfo> = Vec::new();
    let mut tasks_out = String::new();
    for bar in &bars {
        let task = &tasks[bar.task];
        let cy = plot_top + bar.row as f64 * row_height + row_height / 2.0;
        let state = if task.active {
            "active"
        } else if task.done {
            "done"
        } else {
            ""
        };
        let mut classes = String::from("pele-task");
        if !state.is_empty() {
            classes.push_str(" pele-");
            classes.push_str(state);
        }
        if task.crit {
            classes.push_str(" pele-crit");
        }
        if task.milestone {
            classes.push_str(" pele-milestone");
        }
        if task.classes.iter().any(|c| c == "clickable") {
            classes.push_str(" pele-clickable");
        }

        let mut shape = String::new();
        let mut text_fill = "";
        if bar.x0.is_nan() {
            classes.push_str(" pele-unscheduled");
            text_fill = " fill=\"var(--_m)\"";
        } else {
            // 重要なタスクは強調の色の代わりに重要の色をとる。済んだものは済んだ仕事の塗りのまま、縁だけその色に。
            let color = if task.crit { CRITICAL } else { "var(--_c)" };
            let paint = match state {
                "done" => "fill=\"var(--_a)\"".to_string(),
                "active" => format!("fill=\"{color}\" fill-opacity=\"0.25\""),
                _ => format!("fill=\"{color}\""),
            };
            let stroke = format!(
                "stroke=\"{}\"",
                if state == "done" && !task.crit {
                    "var(--_b)"
                } else {
                    color
                }
            );
            if task.milestone {
                let cx = (bar.x0 + bar.x1) / 2.0;
                let r = bar_height / 2.0;
                shape = format!(
                    "<path d=\"M{},{}L{},{}L{},{}L{},{}Z\" {paint} {stroke} stroke-linejoin=\"round\"/>",
                    num(cx),
                    num(cy - r),
                    num(cx + r),
                    num(cy),
                    num(cx),
                    num(cy + r),
                    num(cx - r),
                    num(cy)
                );
            } else {
                shape = format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" {paint} {stroke}{ROUNDED}/>",
                    num(bar.x0),
                    num(cy - bar_height / 2.0),
                    num(bar.x1 - bar.x0),
                    num(bar_height)
                );
            }
            if bar.inside {
                text_fill = if state.is_empty() {
                    " fill=\"var(--_bg)\""
                } else {
                    ""
                };
            } else if state == "done" {
                text_fill = " fill=\"var(--_m)\"";
            }
        }

        let mut body = shape
            + &label_svg(
                &bar.label,
                bar.label_x,
                cy,
                &format!(" class=\"pele-label\"{text_fill}"),
                None,
            );
        if let Some((_, url)) = model.links.iter().find(|(id, _)| *id == task.id) {
            body = linked(&body, url, &task.id, &mut links, options, "");
        }
        tasks_out.push_str(&format!(
            "<g class=\"{classes}\" data-id=\"{}\">{body}</g>",
            esc_text(&task.id)
        ));
    }

    let mut today = String::new();
    if model.today_marker != "off" && now >= start && now <= stop && stop > start {
        let tx = x(now);
        let top = plot_top - grid_lead;
        let style = if model.today_marker.is_empty() {
            String::new()
        } else {
            let parts: Vec<String> = model.today_marker.split(',').map(str::to_string).collect();
            resolve_style(&parts).line
        };
        today = format!(
            "<g class=\"pele-today\"><path d=\"M{},{}V{}\" fill=\"none\" stroke=\"var(--_c)\" stroke-width=\"2\"{style}/>",
            num(tx),
            num(top),
            num(plot_bottom)
        );
        if style.is_empty() {
            today.push_str(&format!(
                "<path d=\"M{},{}H{}L{},{}Z\" fill=\"var(--_c)\"/>",
                num(tx - 4.0),
                num(top - 5.0),
                num(tx + 4.0),
                num(tx),
                num(top + 1.0)
            ));
        }
        today.push_str("</g>");
    }

    let mut axis_line = format!("M{},{}H{}", num(left), num(plot_bottom), num(right));
    if top_axis {
        axis_line.push_str(&format!(
            "M{},{}H{}",
            num(left),
            num(plot_top - grid_lead),
            num(right)
        ));
    }
    let mut content = label_svg(
        &title,
        width / 2.0,
        pad + title.height / 2.0,
        " class=\"pele-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    if !bands.is_empty() {
        content.push_str(&format!("<g class=\"pele-sections\">{bands}</g>"));
    }
    if !excluded.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-excluded\" fill=\"var(--_a)\">{excluded}</g>"
        ));
    }
    content.push_str(&format!(
        "<g class=\"pele-grid\" fill=\"none\"><path d=\"{grid}\" stroke=\"var(--_b)\" stroke-dasharray=\"1 3\"/><path d=\"{axis_line}\" stroke=\"var(--_b)\"/></g>"
    ));
    if !vert_lines.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-verts\" fill=\"none\" stroke=\"var(--_l)\" stroke-width=\"1.5\" stroke-dasharray=\"4 3\">{vert_lines}</g>"
        ));
    }
    content.push_str(&today);
    content.push_str(&format!(
        "<g class=\"pele-tasks\" font-size=\"{}\">{tasks_out}</g>",
        num(label_size)
    ));
    content.push_str(&format!(
        "<g class=\"pele-section-labels\" font-size=\"{}\">{section_out}</g>",
        num(section_size)
    ));
    content.push_str(&format!(
        "<g class=\"pele-axis\" font-size=\"{}\" fill=\"var(--_m)\">{axis_bottom}{axis_top}</g>",
        js::fmt(tick_size)
    ));
    if !vert_labels.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-vert-labels\" font-size=\"{}\">{vert_labels}</g>",
            num(label_size)
        ));
    }

    Ok(Rendered {
        svg: svg_document("gantt", width, height, size, options, &acc, &content),
        width,
        height,
        links,
    })
}
