//! rebase の確認に埋め込む、前と後の絵（macOS 版の `RewriteDrawingView`。docs/design.md の「Rewriting operations
//! and stacked branches」）。左が今、右が rebase の後。History と同じく新しいものが上で、線・点・ブランチの札・
//! 件名で描く。載せ直すコミットは左で薄く、右で強調（A → A′）。一緒に動くブランチの札は右で新しいコミットへ
//! 移り、残るブランチと `origin/…` は左の元のコミットに残る。材料は `rewritePreview` の答えの `drawing`。

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gdk, pango};
use serde_json::Value;

const ROW: f64 = 22.0;
const LANE: f64 = 14.0;
const COLUMN: f64 = 330.0;
const TOP: f64 = 22.0;

/// 1 行（コミット、線、薄くするか、y）。
struct Row {
    oid: String,
    subject: String,
    lane: u8,
    dim: bool,
    y: f64,
}

/// 札（名前、種類 `local`・`remote`・`moved`、今のブランチか）。
type Label = (String, String, bool);

/// 答え（`rewritePreview` の `info`）を絵に渡す。
pub type Setter = Rc<dyn Fn(&Value)>;

/// 絵の部品。`set` で答えを渡す。`drawing` がなければ隠す。
pub fn new() -> (gtk::DrawingArea, Setter) {
    let area = gtk::DrawingArea::new();
    area.set_content_width((COLUMN * 2.0 + 24.0) as i32);
    area.set_visible(false);
    let info: Rc<RefCell<Value>> = Rc::default();
    let i = info.clone();
    area.set_draw_func(move |area, cr, _, _| draw(area, cr, &i.borrow()));
    let a = area.downgrade();
    let set: Setter = Rc::new(move |value: &Value| {
        let Some(area) = a.upgrade() else { return };
        info.replace(value.clone());
        let d = &value["drawing"];
        if d.is_null() {
            area.set_visible(false);
            return;
        }
        let picks = d["picks"].as_array().map_or(0, Vec::len);
        let onto = d["onto"].as_array().map_or(0, Vec::len);
        let more = usize::from(d["ontoMore"].as_u64().unwrap_or(0) > 0);
        let rows = picks + onto + more + 1;
        area.set_content_height((rows as f64 * ROW + 26.0) as i32);
        area.set_visible(true);
        area.queue_draw();
    });
    (area, set)
}

fn commits(v: &Value) -> Vec<(String, String)> {
    v.as_array()
        .into_iter()
        .flatten()
        .map(|c| {
            (
                c["oid"].as_str().unwrap_or_default().to_owned(),
                c["subject"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect()
}

fn draw(area: &gtk::DrawingArea, cr: &gtk::cairo::Context, info: &Value) {
    let d = &info["drawing"];
    if d.is_null() {
        return;
    }
    let fg = area.color();
    let current = info["current"].as_str().map(str::to_owned);
    let moving: Vec<String> = info["moving"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|b| b["name"].as_str().map(str::to_owned))
        .collect();
    let mut labels: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for l in d["labels"].as_array().into_iter().flatten() {
        labels
            .entry(l["oid"].as_str().unwrap_or_default().to_owned())
            .or_default()
            .push((
                l["name"].as_str().unwrap_or_default().to_owned(),
                l["kind"].as_str().unwrap_or_default().to_owned(),
            ));
    }
    let mut picks = commits(&d["picks"]);
    picks.reverse();
    let onto = commits(&d["onto"]);
    let more = d["ontoMore"].as_u64().unwrap_or(0);
    let base = (!d["base"].is_null()).then(|| {
        (
            d["base"]["oid"].as_str().unwrap_or_default().to_owned(),
            d["base"]["subject"].as_str().unwrap_or_default().to_owned(),
        )
    });

    let heading = |text: &str, x: f64| {
        text_at(area, cr, text, x, 2.0, &dimmed(&fg, 0.6), true, f64::MAX);
    };
    heading(&tr!("Now"), 4.0);
    heading(&tr!("After the rebase"), COLUMN + 28.0);

    // 左: 今。上に載せ直すコミット（右の線）、その下に載せ先で進んだコミット（左の線）、いちばん下で合流。
    let mut y = TOP;
    let mut rows = Vec::new();
    for (oid, subject) in &picks {
        rows.push(Row {
            oid: oid.clone(),
            subject: subject.clone(),
            lane: 1,
            dim: true,
            y,
        });
        y += ROW;
    }
    for (oid, subject) in &onto {
        rows.push(Row {
            oid: oid.clone(),
            subject: subject.clone(),
            lane: 0,
            dim: false,
            y,
        });
        y += ROW;
    }
    let more_y = (more > 0).then_some(y);
    if more > 0 {
        y += ROW;
    }
    let base_y = y;
    let current_c = current.clone();
    let labels_c = labels.clone();
    column(
        area,
        cr,
        0.0,
        &rows,
        base_y,
        base.as_ref(),
        more_y,
        more,
        0,
        &|oid| {
            // 左には今のブランチの札をすべて（動くものも、まだ元のところにある）。
            labels_c
                .get(oid)
                .into_iter()
                .flatten()
                .map(|(n, k)| (n.clone(), k.clone(), Some(n) == current_c.as_ref()))
                .collect()
        },
    );

    // 右: rebase の後。載せ直したコミット（A′）が載せ先の上に 1 本で続く。
    let mut after = Vec::new();
    y = TOP;
    for (oid, subject) in &picks {
        after.push(Row {
            oid: oid.clone(),
            subject: format!("{subject}′"),
            lane: 0,
            dim: false,
            y,
        });
        y += ROW;
    }
    for (oid, subject) in &onto {
        after.push(Row {
            oid: oid.clone(),
            subject: subject.clone(),
            lane: 0,
            dim: true,
            y,
        });
        y += ROW;
    }
    let after_more_y = (more > 0).then_some(y);
    if more > 0 {
        y += ROW;
    }
    let first_pick = picks.first().map(|p| p.0.clone());
    let onto_ids: Vec<String> = onto.iter().map(|c| c.0.clone()).collect();
    let base_id = base.as_ref().map(|b| b.0.clone());
    column(
        area,
        cr,
        COLUMN + 24.0,
        &after,
        y,
        base.as_ref(),
        after_more_y,
        more,
        picks.len(),
        &|oid| {
            let mut out: Vec<Label> = Vec::new();
            // 今のブランチは先頭の A′ へ、一緒に動くブランチは、元の先頭に対応する A′ へ。
            if Some(oid) == first_pick.as_deref()
                && let Some(c) = &current
            {
                out.push((c.clone(), "local".into(), true));
            }
            for (n, k) in labels.get(oid).into_iter().flatten() {
                if k == "local" && moving.contains(n) {
                    out.push((n.clone(), "moved".into(), false));
                }
            }
            // 載せ先のコミットに付いている札は、そのまま。
            if onto_ids.iter().any(|o| o == oid) || base_id.as_deref() == Some(oid) {
                for (n, k) in labels.get(oid).into_iter().flatten() {
                    if Some(n) != current.as_ref() && !moving.contains(n) {
                        out.push((n.clone(), k.clone(), false));
                    }
                }
            }
            out
        },
    );
    // 真ん中の矢印。
    text_at(
        area,
        cr,
        "→",
        COLUMN + 6.0,
        TOP + (picks.len().saturating_sub(1)) as f64 * ROW / 2.0,
        &dimmed(&fg, 0.4),
        false,
        f64::MAX,
    );
}

fn dimmed(c: &gdk::RGBA, alpha: f32) -> gdk::RGBA {
    let mut c = *c;
    c.set_alpha(c.alpha() * alpha);
    c
}

/// 1 列を描く。lane 1 は右の線（いちばん下で lane 0 へ合流）。`highlight` は強調する上からの行数。
#[allow(clippy::too_many_arguments)]
fn column(
    area: &gtk::DrawingArea,
    cr: &gtk::cairo::Context,
    x: f64,
    rows: &[Row],
    base_y: f64,
    base: Option<&(String, String)>,
    more_y: Option<f64>,
    more: u64,
    highlight: usize,
    labels: &dyn Fn(&str) -> Vec<Label>,
) {
    let fg = area.color();
    let lane_x = |lane: u8| x + 10.0 + f64::from(lane) * LANE;
    let center = |y: f64| y + ROW / 2.0;
    // 線。
    cr.set_line_width(2.0);
    let line = dimmed(&fg, 0.4);
    cr.set_source_rgba(
        line.red().into(),
        line.green().into(),
        line.blue().into(),
        line.alpha().into(),
    );
    let lane1: Vec<&Row> = rows.iter().filter(|r| r.lane == 1).collect();
    if let (Some(first), Some(last)) = (lane1.first(), lane1.last()) {
        cr.move_to(lane_x(1), center(first.y));
        cr.line_to(lane_x(1), center(last.y));
        cr.curve_to(
            lane_x(1),
            center(last.y) + 8.0,
            lane_x(0),
            center(base_y) - 6.0,
            lane_x(0),
            center(base_y),
        );
        let _ = cr.stroke();
    }
    if let Some(first) = rows.iter().find(|r| r.lane == 0) {
        cr.move_to(lane_x(0), center(first.y));
        cr.line_to(lane_x(0), center(base_y));
        let _ = cr.stroke();
    }
    // 点と件名、札。
    let text_x = lane_x(1) + 14.0;
    for (i, row) in rows.iter().enumerate() {
        row_at(
            area,
            cr,
            lane_x(row.lane),
            row.y,
            text_x,
            x + COLUMN,
            &row.subject,
            row.dim,
            i < highlight,
            &labels(&row.oid),
        );
    }
    if let Some(y) = more_y {
        text_at(
            area,
            cr,
            &tr!("and {count} more", count = more),
            text_x,
            y + 3.0,
            &dimmed(&fg, 0.5),
            false,
            x + COLUMN - text_x,
        );
    }
    if let Some((oid, subject)) = base {
        row_at(
            area,
            cr,
            lane_x(0),
            base_y,
            text_x,
            x + COLUMN,
            subject,
            true,
            false,
            &labels(oid),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn row_at(
    area: &gtk::DrawingArea,
    cr: &gtk::cairo::Context,
    dot_x: f64,
    y: f64,
    text_x: f64,
    right: f64,
    subject: &str,
    dim: bool,
    highlighted: bool,
    labels: &[Label],
) {
    let fg = area.color();
    let r = if highlighted { 5.0 } else { 4.0 };
    let dot = if highlighted {
        accent(area)
    } else if dim {
        dimmed(&fg, 0.35)
    } else {
        dimmed(&fg, 0.65)
    };
    cr.set_source_rgba(
        dot.red().into(),
        dot.green().into(),
        dot.blue().into(),
        dot.alpha().into(),
    );
    cr.arc(dot_x, y + ROW / 2.0, r, 0.0, std::f64::consts::TAU);
    let _ = cr.fill();
    let mut x = text_x;
    for (name, kind, current) in labels {
        x = pill(area, cr, name, kind, *current, x, y + 3.0) + 4.0;
    }
    let color = if dim { dimmed(&fg, 0.45) } else { fg };
    text_at(
        area,
        cr,
        subject,
        x,
        y + 3.0,
        &color,
        highlighted,
        (right - x).max(0.0),
    );
}

fn accent(area: &gtk::DrawingArea) -> gdk::RGBA {
    let _ = area;
    gdk::RGBA::new(0.21, 0.52, 0.89, 1.0)
}

/// History と同じ色の札（ローカルは青、リモートは紫、一緒に動いたものは緑）。右端を返す。
#[allow(clippy::too_many_arguments)]
fn pill(
    area: &gtk::DrawingArea,
    cr: &gtk::cairo::Context,
    name: &str,
    kind: &str,
    current: bool,
    x: f64,
    y: f64,
) -> f64 {
    let color = match kind {
        "remote" => gdk::RGBA::new(0.57, 0.25, 0.67, 1.0),
        "moved" => gdk::RGBA::new(0.18, 0.62, 0.32, 1.0),
        _ => accent(area),
    };
    let layout = area.create_pango_layout(Some(name));
    let mut font = area.pango_context().font_description().unwrap_or_default();
    font.set_size(font.size() * 85 / 100);
    if current {
        font.set_weight(pango::Weight::Bold);
    }
    layout.set_font_description(Some(&font));
    let (w, h) = layout.pixel_size();
    let (w, h) = (f64::from(w) + 10.0, f64::from(h) + 2.0);
    let fill = if current { color } else { dimmed(&color, 0.16) };
    cr.set_source_rgba(
        fill.red().into(),
        fill.green().into(),
        fill.blue().into(),
        fill.alpha().into(),
    );
    rounded(cr, x, y, w, h, 4.0);
    let _ = cr.fill();
    let text = if current { gdk::RGBA::WHITE } else { color };
    cr.set_source_rgba(
        text.red().into(),
        text.green().into(),
        text.blue().into(),
        text.alpha().into(),
    );
    cr.move_to(x + 5.0, y + 1.0);
    pangocairo::functions::show_layout(cr, &layout);
    x + w
}

fn rounded(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    use std::f64::consts::{FRAC_PI_2, PI};
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, FRAC_PI_2);
    cr.arc(x + r, y + h - r, r, FRAC_PI_2, PI);
    cr.arc(x + r, y + r, r, PI, 3.0 * FRAC_PI_2);
    cr.close_path();
}

/// 小さな字で `text` を描く（`width` を超えたら末尾を … にする）。
#[allow(clippy::too_many_arguments)]
fn text_at(
    area: &gtk::DrawingArea,
    cr: &gtk::cairo::Context,
    text: &str,
    x: f64,
    y: f64,
    color: &gdk::RGBA,
    bold: bool,
    width: f64,
) {
    let layout = area.create_pango_layout(Some(text));
    let mut font = area.pango_context().font_description().unwrap_or_default();
    font.set_size(font.size() * 9 / 10);
    if bold {
        font.set_weight(pango::Weight::Bold);
    }
    layout.set_font_description(Some(&font));
    if width.is_finite() {
        layout.set_width((width * f64::from(pango::SCALE)) as i32);
        layout.set_ellipsize(pango::EllipsizeMode::End);
    }
    cr.set_source_rgba(
        color.red().into(),
        color.green().into(),
        color.blue().into(),
        color.alpha().into(),
    );
    cr.move_to(x, y);
    pangocairo::functions::show_layout(cr, &layout);
}
