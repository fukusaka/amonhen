//! eventmodeling を描く（pele の `src/diagrams/eventmodeling/render.ts`）。

use std::collections::{HashMap, HashSet};

use super::model::{EmBox, EntityKind, EventModelingModel};
use crate::diagrams::common::fit_width::turn_to_fit;
use crate::diagrams::common::literal_label::{break_label, left_label_svg, literal_label};
use crate::errors::Result;
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num};
use crate::svg::edges::{marker, marker_trim};
use crate::svg::root::{Accessible, svg_document};
use crate::svg::theme::{FONT_MONO, ROUNDED, series_color};
use crate::text::entities::decode_entities;
use crate::text::label::{Label, layout_label};
use crate::text::measurer::{TextMeasurer, default_measurer, style as st};
use crate::types::{RenderOptions, Rendered};

/// 実体の種類ごとの系列の色。UI にはなく、無地に描く。
fn series(kind: EntityKind) -> Option<i64> {
    match kind {
        EntityKind::Command => Some(0),
        EntityKind::Event => Some(1),
        EntityKind::Readmodel => Some(2),
        EntityKind::Processor => Some(6),
        EntityKind::Ui => None,
    }
}

const BOX_PAD_X: f64 = 12.0;
const BOX_PAD_Y: f64 = 10.0;
const BOX_MIN_WIDTH: f64 = 96.0;
const BOX_GAP: f64 = 16.0;
const STAGGER: f64 = 80.0;
const LANE_PAD: f64 = 12.0;
const LANE_GAP: f64 = 8.0;
const DATA_COLUMNS: usize = 48;
const NOTE_COLUMNS: usize = 80;
const CARD_GAP: f64 = 8.0;

struct Placed {
    name: Label,
    data: Vec<String>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

struct Band {
    label: Label,
    right: f64,
    y: f64,
    h: f64,
}

/// 文字を `columns` 文字までの行に分ける。自分の改行は保つ。
fn hard_wrap(text: &str, columns: usize) -> Vec<String> {
    let mut out = Vec::new();
    for line in decode_entities(text).split('\n') {
        if js::len16(line) <= columns {
            out.push(line.to_string());
            continue;
        }
        let chars: Vec<char> = line.chars().collect();
        if chars.len() <= columns {
            out.push(line.to_string());
            continue;
        }
        for chunk in chars.chunks(columns) {
            out.push(chunk.iter().collect());
        }
    }
    out
}

fn lines(rows: &[String], x: f64, y: f64, step: f64, attrs: &str) -> String {
    let mut body = String::new();
    for (i, row) in rows.iter().enumerate() {
        if !row.is_empty() {
            body.push_str(&format!(
                "<tspan x=\"{}\" y=\"{}\">{}</tspan>",
                num(x),
                num(y + i as f64 * step),
                esc(row)
            ));
        }
    }
    if body.is_empty() {
        String::new()
    } else {
        format!("<text{attrs} xml:space=\"preserve\">{body}</text>")
    }
}

fn accessible(model: &EventModelingModel) -> Accessible<'_> {
    Accessible {
        acc_title: model.acc_title.as_deref(),
        acc_descr: model.acc_descr.as_deref(),
    }
}

fn measurer_of(options: &RenderOptions) -> &dyn TextMeasurer {
    match &options.measurer {
        Some(m) => m.as_ref(),
        None => default_measurer(),
    }
}

fn node_class(b: &EmBox) -> String {
    format!(
        "pele-node pele-em-{}{}",
        b.kind.name(),
        if b.reset { " pele-em-reset" } else { "" }
    )
}

pub fn render_event_model(
    model: &EventModelingModel,
    _config: &Config,
    options: &RenderOptions,
) -> Result<Rendered> {
    turn_to_fit(options, |down| Ok(draw(model, options, down)))
}

fn draw(model: &EventModelingModel, options: &RenderOptions, down: bool) -> Rendered {
    if down {
        return vertical_event_model(model, options);
    }
    let size = options.font_size.unwrap_or(16.0);
    let name_size = js::round(size * 0.875);
    let small = js::round(size * 0.8125);
    let mono = js::round(size * 0.75);
    let mono_step = js::round(mono * 1.4);
    let small_step = js::round(small * 1.5);
    let measurer = measurer_of(options);
    let pad = options.padding.unwrap_or(8.0);
    let widest = |rows: &[String], font_size: f64, style: u32| -> f64 {
        let mut width = 0.0;
        for row in rows {
            width = js::max(width, measurer.width(row, font_size, style));
        }
        width
    };

    // レーン（`lanes_made` の番号）ごとの帯。上から下の順。
    let mut bands: HashMap<usize, Band> = HashMap::new();
    let mut label_column = 0.0;
    for &lane in &model.lanes {
        let label = layout_label(
            Some(&model.lanes_made[lane].label),
            false,
            measurer,
            small,
            4000.0,
            st::BOLD,
        );
        label_column = js::max(label_column, label.width);
        bands.insert(
            lane,
            Band {
                label,
                right: f64::NEG_INFINITY,
                y: 0.0,
                h: 0.0,
            },
        );
    }
    label_column += 2.0 * LANE_PAD;

    // 時は左から右へ。新しいレーンの枠は前の枠の終わりの下から始め、その間の矢印がまっすぐ下りられるように
    // する。同じレーンの枠は続けて置く。
    let mut placed: Vec<Placed> = Vec::new();
    for (k, b) in model.boxes.iter().enumerate() {
        let name = layout_label(Some(&b.name), false, measurer, name_size, 4000.0, st::BOLD);
        let data = match &b.data {
            Some(d) if !d.is_empty() => hard_wrap(d, DATA_COLUMNS),
            _ => Vec::new(),
        };
        let w = js::max(
            BOX_MIN_WIDTH,
            js::max(name.width, widest(&data, mono, st::MONO)) + 2.0 * BOX_PAD_X,
        );
        let h = 2.0 * BOX_PAD_Y
            + name.height
            + if data.is_empty() {
                0.0
            } else {
                6.0 + data.len() as f64 * mono_step
            };
        let band_right = bands[&b.lane].right;
        let mut x = 0.0;
        if k > 0 {
            let previous = &placed[k - 1];
            let stagger = previous.x + js::max(previous.w / 2.0, previous.w - STAGGER);
            x = if model.boxes[k - 1].lane == b.lane {
                band_right + BOX_GAP
            } else {
                js::max(stagger, band_right + BOX_GAP)
            };
        }
        let band = bands.get_mut(&b.lane).unwrap();
        band.right = x + w;
        band.h = js::max(band.h, h);
        placed.push(Placed {
            name,
            data,
            x,
            y: 0.0,
            w,
            h,
        });
    }

    let mut content_right = 0.0;
    for item in &placed {
        content_right = js::max(content_right, item.x + item.w);
    }
    let lane_width = label_column + content_right + LANE_PAD;

    let title = layout_label(
        model.title.as_deref(),
        false,
        measurer,
        size,
        4000.0,
        st::BOLD,
    );
    let title_height = if title.height > 0.0 {
        title.height + 12.0
    } else {
        0.0
    };
    let left = pad;
    let top = pad + title_height;

    let mut y = top;
    let mut lanes = String::new();
    for &lane in &model.lanes {
        let band = bands.get_mut(&lane).unwrap();
        band.y = y;
        band.h += 2.0 * LANE_PAD;
        lanes.push_str(&format!(
            "<g class=\"pele-cluster pele-em-lane\" data-id=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_s)\"{ROUNDED}/>{}</g>",
            esc_text(&model.lanes_made[lane].label),
            num(left),
            num(y),
            num(lane_width),
            num(band.h),
            label_svg(
                &band.label,
                left + LANE_PAD + band.label.width / 2.0,
                y + band.h / 2.0,
                &format!(
                    " class=\"pele-cluster-label\" font-size=\"{}\" font-weight=\"var(--_hw)\" fill=\"var(--_m)\"",
                    js::fmt(small)
                ),
                None
            )
        ));
        y += band.h + LANE_GAP;
    }
    let mut bottom = if bands.is_empty() { top } else { y - LANE_GAP };

    let mut nodes = String::new();
    for (k, item) in placed.iter_mut().enumerate() {
        let b = &model.boxes[k];
        let band = &bands[&b.lane];
        item.x += left + label_column;
        item.y = band.y + (band.h - item.h) / 2.0;
        let frame = format!(
            "x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"",
            num(item.x),
            num(item.y),
            num(item.w),
            num(item.h)
        );
        let shape = match series(b.kind) {
            None => format!("<rect {frame} fill=\"var(--_bg)\" stroke=\"var(--_b)\"{ROUNDED}/>"),
            Some(s) => {
                let color = series_color(s);
                format!(
                    "<rect {frame} fill=\"var(--_bg)\"{ROUNDED}/><rect {frame} fill=\"{color}\" fill-opacity=\"0.22\" stroke=\"{color}\"{ROUNDED}/>"
                )
            }
        };
        let name_y = item.y + BOX_PAD_Y + item.name.height / 2.0;
        let text = label_svg(
            &item.name,
            item.x + item.w / 2.0,
            name_y,
            &format!(
                " class=\"pele-label\" font-size=\"{}\" font-weight=\"var(--_hw)\"",
                js::fmt(name_size)
            ),
            None,
        ) + &lines(
            &item.data,
            item.x + BOX_PAD_X,
            item.y + BOX_PAD_Y + item.name.height + 6.0 + mono_step / 2.0 + mono * 0.35,
            mono_step,
            &format!(
                " class=\"pele-em-data\" font-family=\"{FONT_MONO}\" font-size=\"{}\"",
                js::fmt(mono)
            ),
        );
        nodes.push_str(&format!(
            "<g class=\"{}\" data-id=\"{}\">{shape}{text}</g>",
            node_class(b),
            esc_text(&b.id)
        ));
    }

    let mut edges = String::new();
    let trim = marker_trim("arrow_point");
    for relation in &model.relations {
        if relation.source == relation.target {
            continue;
        }
        let s = &placed[relation.source];
        let t = &placed[relation.target];
        let (d, head);
        if model.boxes[relation.source].lane == model.boxes[relation.target].lane {
            // レーンに沿って、箱の横から箱の横へ。
            let forward = t.x >= s.x;
            let sx = if forward { s.x + s.w } else { s.x };
            let tx = if forward { t.x } else { t.x + t.w };
            let sy = s.y + s.h / 2.0;
            let ty = t.y + t.h / 2.0;
            let dir = if tx >= sx { 1.0 } else { -1.0 };
            d = format!(
                "M{},{}L{},{}",
                num(sx),
                num(sy),
                num(tx - dir * trim),
                num(ty)
            );
            head = marker("arrow_point", tx, ty, dir, 0.0, "var(--_l)", "none");
        } else {
            // レーンの間は、箱が重なる所はまっすぐ上下に、でなければ S の字に。
            let down = t.y > s.y;
            let from = js::max(s.x, t.x);
            let to = js::min(s.x + s.w, t.x + t.w);
            let inset = |w: f64| js::min(24.0, w / 4.0);
            let sx = if to - from >= 16.0 {
                (from + to) / 2.0
            } else if t.x > s.x {
                s.x + s.w - inset(s.w)
            } else {
                s.x + inset(s.w)
            };
            let tx = if to - from >= 16.0 {
                sx
            } else if t.x > s.x {
                t.x + inset(t.w)
            } else {
                t.x + t.w - inset(t.w)
            };
            let sy = if down { s.y + s.h } else { s.y };
            let ty = if down { t.y } else { t.y + t.h };
            let dir = if down { 1.0 } else { -1.0 };
            let end = ty - dir * trim;
            let mid = (sy + end) / 2.0;
            d = if sx == tx {
                format!("M{},{}V{}", num(sx), num(sy), num(end))
            } else {
                format!(
                    "M{},{}C{},{} {},{} {},{}",
                    num(sx),
                    num(sy),
                    num(sx),
                    num(mid),
                    num(tx),
                    num(mid),
                    num(tx),
                    num(end)
                )
            };
            head = marker("arrow_point", tx, ty, 0.0, dir, "var(--_l)", "none");
        }
        edges.push_str(&format!(
            "<g class=\"pele-edge\" data-id=\"{}-{}\"><path d=\"{d}\" fill=\"none\" stroke=\"var(--_l)\"/>{head}</g>",
            esc_text(&model.boxes[relation.source].id),
            esc_text(&model.boxes[relation.target].id)
        ));
    }

    // 注記と given/when/then の仕様。レーンの下に、属する枠とともに並べる。
    let mut names: HashMap<&str, &str> = HashMap::new();
    for b in &model.boxes {
        names.insert(&b.id, &b.name);
    }
    let mut cards = String::new();
    let mut cards_right = 0.0;
    let mut card = |kind: &str, heading: &str, frame: &str, rows: &[String]| {
        let head_text = match names.get(frame) {
            Some(n) => format!("{heading} · {frame} {n}"),
            None => format!("{heading} · {frame}"),
        };
        let head = layout_label(Some(&head_text), false, measurer, small, 4000.0, st::BOLD);
        let w = js::max(head.width, widest(rows, small, 0)) + 2.0 * BOX_PAD_X;
        let h = 2.0 * BOX_PAD_Y + head.height + rows.len() as f64 * small_step;
        let cy = bottom
            + if cards.is_empty() && bands.is_empty() {
                0.0
            } else {
                CARD_GAP
            };
        cards.push_str(&format!(
            "<g class=\"pele-em-{kind}\" data-id=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_bg)\" stroke=\"var(--_b)\"{ROUNDED}/>{}{}</g>",
            esc_text(frame),
            num(left),
            num(cy),
            num(w),
            num(h),
            label_svg(
                &head,
                left + BOX_PAD_X + head.width / 2.0,
                cy + BOX_PAD_Y + head.height / 2.0,
                &format!(" font-size=\"{}\" font-weight=\"var(--_hw)\"", js::fmt(small)),
                None
            ),
            lines(
                rows,
                left + BOX_PAD_X,
                cy + BOX_PAD_Y + head.height + small_step / 2.0 + small * 0.35,
                small_step,
                &format!(" font-size=\"{}\"", js::fmt(small))
            )
        ));
        bottom = cy + h;
        cards_right = js::max(cards_right, w);
    };
    for note in &model.notes {
        card(
            "note",
            "Note",
            &note.frame,
            &hard_wrap(&note.text, NOTE_COLUMNS),
        );
    }
    for spec in &model.specifications {
        let mut rows = vec![format!("Given  {}", spec.given.join(", "))];
        if !spec.when.is_empty() {
            rows.push(format!("When  {}", spec.when.join(", ")));
        }
        rows.push(format!("Then  {}", spec.then.join(", ")));
        let rows: Vec<String> = rows
            .iter()
            .flat_map(|row| hard_wrap(row, NOTE_COLUMNS))
            .collect();
        card("specification", "Given / When / Then", &spec.frame, &rows);
    }

    let width = js::max(
        js::max(if bands.is_empty() { 0.0 } else { lane_width }, cards_right),
        title.width,
    );
    let total_width = (width + 2.0 * pad).ceil();
    let total_height = (bottom + pad).ceil();
    let content = label_svg(
        &title,
        total_width / 2.0,
        pad + title.height / 2.0,
        " class=\"pele-title\" font-weight=\"var(--_tw)\"",
        None,
    ) + &lanes
        + &edges
        + &nodes
        + &cards;

    Rendered {
        svg: svg_document(
            "eventmodeling",
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

const RAIL_GAP: f64 = 8.0;
const CARD_MIN: f64 = 120.0;
const ARROW_ROOM: f64 = 20.0;

struct Card {
    y: f64,
    h: f64,
    w: f64,
    index: usize,
    lane: f64,
}

fn vertical_event_model(model: &EventModelingModel, options: &RenderOptions) -> Rendered {
    let size = options.font_size.unwrap_or(16.0);
    let small = js::round(size * 0.875);
    let mono = js::round(size * 0.75);
    let pad = options.padding.unwrap_or(8.0);
    let measurer = measurer_of(options);
    let room = js::max(160.0, options.max_width.unwrap_or(320.0) - 2.0 * pad);
    // 札を飛ばすつながりは、札の横の線路を下る。同じ札へのつながりは線路を分け合い、ほかで走る所が重なるものは
    // 次の外の線路を取る。2 つが同じ線をたどらないように。
    let mut runs: Vec<(usize, [usize; 2])> = Vec::new();
    // 隣の札の間のつながりは、下の札のレーンの見出しの横を通る。
    let mut passed: HashSet<usize> = HashSet::new();
    for relation in &model.relations {
        let (s, t) = (relation.source, relation.target);
        if s == t {
            continue;
        }
        if s.abs_diff(t) < 2 {
            passed.insert(s.max(t));
            continue;
        }
        match runs.iter_mut().find(|(target, _)| *target == t) {
            Some((_, run)) => {
                run[0] = run[0].min(s);
                run[1] = run[1].max(s);
            }
            None => runs.push((t, [s.min(t), s.max(t)])),
        }
    }
    runs.sort_by_key(|(_, run)| run[0]);
    let mut rail_of: HashMap<usize, usize> = HashMap::new();
    let mut rail_ends: Vec<usize> = Vec::new();
    for (target, [from, to]) in &runs {
        let rail = match rail_ends.iter().position(|&end| end < *from) {
            Some(r) => r,
            None => rail_ends.len(),
        };
        if rail == rail_ends.len() {
            rail_ends.push(*to);
        } else {
            rail_ends[rail] = *to;
        }
        rail_of.insert(*target, rail);
    }
    let gutter = 24.0 + RAIL_GAP * (rail_ends.len().max(1) - 1) as f64;
    // 線路は場所のために分け合わない: 幅が許すより多ければ、図が広くなる。
    let card_width = js::max(CARD_MIN, room - gutter);
    let text_room = card_width - 2.0 * BOX_PAD_X;
    let title = layout_label(
        model.title.as_deref(),
        false,
        measurer,
        size,
        room,
        st::BOLD,
    );
    let mut y = pad
        + if title.height != 0.0 {
            title.height + 12.0
        } else {
            0.0
        };
    let mut bottom = pad + title.height;
    let mut width = js::max(title.width, card_width + gutter);
    let mut nodes = String::new();
    let mut edges = String::new();
    let mut cards = String::new();
    let mut placed: Vec<Card> = Vec::new();
    let text = |raw: &str, font_size: f64, style: u32| -> Label {
        literal_label(&decode_entities(raw), measurer, font_size, text_room, style)
    };
    let row_text = |label: &Label| -> Vec<String> {
        label
            .lines
            .iter()
            .map(|line| line.iter().map(|s| s.text.as_str()).collect::<String>())
            .collect()
    };
    for (index, b) in model.boxes.iter().enumerate() {
        let lane_room = if passed.contains(&index) {
            text_room - ARROW_ROOM
        } else {
            text_room
        };
        let lane = break_label(
            &layout_label(
                Some(&model.lanes_made[b.lane].label),
                false,
                measurer,
                small,
                lane_room,
                st::BOLD,
            ),
            measurer,
            lane_room,
        );
        let name = layout_label(
            Some(&b.name),
            false,
            measurer,
            small,
            text_room - 36.0,
            st::BOLD,
        );
        let data = match &b.data {
            Some(d) if !d.is_empty() => Some(text(d, mono, st::MONO)),
            _ => None,
        };
        let w = js::max_of(&[
            card_width,
            lane.width + 2.0 * BOX_PAD_X,
            name.width + 2.0 * BOX_PAD_X + 36.0,
            data.as_ref().map_or(0.0, |d| d.width) + 2.0 * BOX_PAD_X,
        ]);
        let head_h = lane.height + 12.0;
        let h = 2.0 * BOX_PAD_Y + name.height + data.as_ref().map_or(0.0, |d| d.height + 6.0);
        let cy = y + head_h;
        let frame = format!(
            "x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"",
            num(pad),
            num(cy),
            num(w),
            num(h)
        );
        nodes.push_str(&format!(
            "<g class=\"pele-cluster pele-em-lane\" data-id=\"{}\">{}<g class=\"{}\" data-id=\"{}\"><rect {frame} fill=\"var(--_bg)\" stroke=\"var(--_b)\"{ROUNDED}/>",
            esc_text(&model.lanes_made[b.lane].label),
            left_label_svg(
                &lane,
                pad + BOX_PAD_X,
                y,
                &format!(
                    " class=\"pele-cluster-label\" font-size=\"{}\" font-weight=\"var(--_hw)\" fill=\"var(--_m)\"",
                    js::fmt(small)
                )
            ),
            node_class(b),
            esc_text(&b.id)
        ));
        if let Some(s) = series(b.kind) {
            let color = series_color(s);
            nodes.push_str(&format!(
                "<rect {frame} fill=\"{color}\" fill-opacity=\"0.22\" stroke=\"{color}\"{ROUNDED}/>"
            ));
        }
        nodes.push_str(&label_svg(
            &name,
            pad + BOX_PAD_X + name.width / 2.0,
            cy + BOX_PAD_Y + name.height / 2.0,
            &format!(
                " class=\"pele-label\" font-size=\"{}\" font-weight=\"var(--_hw)\"",
                js::fmt(small)
            ),
            None,
        ));
        nodes.push_str(&format!(
            "<text class=\"pele-em-frame\" x=\"{}\" y=\"{}\" text-anchor=\"end\" font-size=\"{}\" fill=\"var(--_m)\">{}</text>",
            num(pad + w - BOX_PAD_X),
            num(cy + BOX_PAD_Y + small),
            js::fmt(mono),
            esc(&b.id)
        ));
        if let Some(data) = &data {
            nodes.push_str(&lines(
                &row_text(data),
                pad + BOX_PAD_X,
                cy + BOX_PAD_Y + name.height + 6.0 + data.line_height / 2.0 + mono * 0.35,
                data.line_height,
                &format!(
                    " class=\"pele-em-data\" font-family=\"{FONT_MONO}\" font-size=\"{}\"",
                    js::fmt(mono)
                ),
            ));
        }
        nodes.push_str("</g></g>");
        placed.push(Card {
            y: cy,
            h,
            w,
            index,
            lane: lane.width,
        });
        width = js::max(width, w + gutter);
        bottom = cy + h;
        y = bottom + 28.0;
    }
    for relation in &model.relations {
        if relation.source == relation.target {
            continue;
        }
        let s = &placed[relation.source];
        let t = &placed[relation.target];
        let down = t.y > s.y;
        let dir = if down { 1.0 } else { -1.0 };
        let sy = if down { s.y + s.h } else { s.y };
        let ty = if down { t.y } else { t.y + t.h };
        let (d, head);
        if s.index.abs_diff(t.index) == 1 {
            // 下の札のレーンの見出しは間にあり、横に矢印の場所を残して折り返す。
            let lower = if down { t } else { s };
            let cx = pad + js::max(js::min(s.w, t.w) / 2.0, BOX_PAD_X + lower.lane + 16.0);
            d = format!(
                "M{},{}V{}",
                num(cx),
                num(sy),
                num(ty - dir * marker_trim("arrow_point"))
            );
            head = marker("arrow_point", cx, ty, 0.0, dir, "var(--_l)", "none");
        } else {
            let rail = pad + width - gutter
                + 12.0
                + RAIL_GAP * rail_of.get(&relation.target).copied().unwrap_or(0) as f64;
            // 札は 1 つのつながりを送り別のものを受けうる: 真ん中の上から出て、下に着く。
            let from = s.y + s.h / 2.0 - 5.0;
            let to = t.y + t.h / 2.0 + 5.0;
            d = format!(
                "M{},{}H{}V{}H{}",
                num(pad + s.w),
                num(from),
                num(rail),
                num(to),
                num(pad + t.w + marker_trim("arrow_point"))
            );
            head = marker("arrow_point", pad + t.w, to, -1.0, 0.0, "var(--_l)", "none");
        }
        edges.push_str(&format!(
            "<g class=\"pele-edge\" data-id=\"{}-{}\"><path d=\"{d}\" fill=\"none\" stroke=\"var(--_l)\"/>{head}</g>",
            esc_text(&model.boxes[relation.source].id),
            esc_text(&model.boxes[relation.target].id)
        ));
    }
    let mut names: HashMap<&str, &str> = HashMap::new();
    for b in &model.boxes {
        names.insert(&b.id, &b.name);
    }
    let mut card = |kind: &str, heading: &str, frame: &str, raw: &str| {
        let head_text = match names.get(frame) {
            Some(n) => format!("{heading} · {frame} {n}"),
            None => format!("{heading} · {frame}"),
        };
        let head = layout_label(
            Some(&head_text),
            false,
            measurer,
            small,
            text_room,
            st::BOLD,
        );
        let body = text(raw, small, 0);
        let w = js::max_of(&[
            card_width,
            head.width + 2.0 * BOX_PAD_X,
            body.width + 2.0 * BOX_PAD_X,
        ]);
        let h = 2.0 * BOX_PAD_Y + head.height + body.height;
        cards.push_str(&format!(
            "<g class=\"pele-em-{kind}\" data-id=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_bg)\" stroke=\"var(--_b)\"{ROUNDED}/>{}{}</g>",
            esc_text(frame),
            num(pad),
            num(y),
            num(w),
            num(h),
            label_svg(
                &head,
                pad + BOX_PAD_X + head.width / 2.0,
                y + BOX_PAD_Y + head.height / 2.0,
                &format!(" font-size=\"{}\" font-weight=\"var(--_hw)\"", js::fmt(small)),
                None
            ),
            lines(
                &row_text(&body),
                pad + BOX_PAD_X,
                y + BOX_PAD_Y + head.height + body.line_height / 2.0 + small * 0.35,
                body.line_height,
                &format!(" font-size=\"{}\"", js::fmt(small))
            )
        ));
        bottom = y + h;
        y = bottom + CARD_GAP;
        width = js::max(width, w);
    };
    for note in &model.notes {
        card("note", "Note", &note.frame, &note.text);
    }
    for spec in &model.specifications {
        let mut rows = vec![format!("Given  {}", spec.given.join(", "))];
        if !spec.when.is_empty() {
            rows.push(format!("When  {}", spec.when.join(", ")));
        }
        rows.push(format!("Then  {}", spec.then.join(", ")));
        card(
            "specification",
            "Given / When / Then",
            &spec.frame,
            &rows.join("\n"),
        );
    }
    let total_width = (width + 2.0 * pad).ceil();
    let height = (bottom + pad).ceil();
    let content = label_svg(
        &title,
        total_width / 2.0,
        pad + title.height / 2.0,
        " class=\"pele-title\" font-weight=\"var(--_tw)\"",
        None,
    ) + &edges
        + &nodes
        + &cards;
    Rendered {
        svg: svg_document(
            "eventmodeling",
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
