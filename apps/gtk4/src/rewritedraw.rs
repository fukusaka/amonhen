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
/// 列の幅（macOS 版の 330 pt。ふつうの字 13 pt に対する幅なので、字の大きさに合わせて広げる）。
const COLUMN: f64 = 330.0;
const TOP: f64 = 22.0;

/// 字の大きさに合わせた列の幅（件名が macOS 版と同じだけ入る）。
fn column_width(area: &gtk::DrawingArea) -> f64 {
    let font = area.pango_context().font_description().unwrap_or_default();
    // 字の大きさ（pt）を 96 dpi の px に。
    let px = f64::from(font.size()) / f64::from(pango::SCALE) * 96.0 / 72.0;
    if px > 0.0 {
        (COLUMN * px / 13.0).max(COLUMN).round()
    } else {
        COLUMN
    }
}

/// 1 行（コミット、線、薄くするか、y）。`hollow` なら中を抜いた点（外れる・消えるコミット）、`note` は件名の
/// 後ろに添える文と色（なければ薄く）。
#[derive(Default)]
struct Row {
    oid: String,
    subject: String,
    lane: u8,
    dim: bool,
    y: f64,
    hollow: bool,
    note: Option<(String, Option<gdk::RGBA>)>,
    /// これから作るコミット（点線の丸）。
    is_new: bool,
}

/// 札（名前、種類 `local`・`remote`・`moved`、今のブランチか）。
type Label = (String, String, bool);

/// 答え（`rewritePreview` の `info`）を絵に渡す。
pub type Setter = Rc<dyn Fn(&Value)>;

/// 絵の部品。`set` で答えを渡す。`drawing` がなければ隠す。
pub fn new() -> (gtk::DrawingArea, Setter) {
    let area = gtk::DrawingArea::new();
    area.set_content_width((column_width(&area) * 2.0 + 24.0) as i32);
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
        let more = |k: &str| usize::from(d[k].as_u64().unwrap_or(0) > 0);
        let rows = picks + more("picksMore") + onto + more("ontoMore") + 1;
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
    heading(&tr!("After the rebase"), column_width(area) + 28.0);

    // 左: 今。上に載せ直すコミット（右の線）、その下に載せ先で進んだコミット（左の線）、いちばん下で合流。
    let mut y = TOP;
    let mut rows = Vec::new();
    let mut gaps = Vec::new();
    // 載せ直すコミットも載せ先で進んだコミットも、長ければ真ん中を省く（そこだけ点線）。
    let picks_more = d["picksMore"].as_u64().unwrap_or(0);
    let picks_at = d["picksMoreAt"].as_u64();
    with_gap(
        &picks,
        picks_more,
        picks_at,
        1,
        &mut y,
        &mut gaps,
        |(oid, subject), y| {
            rows.push(Row {
                oid: oid.clone(),
                subject: subject.clone(),
                lane: 1,
                dim: true,
                y,
                ..Default::default()
            });
        },
    );
    let more_at = d["ontoMoreAt"].as_u64();
    with_gap(
        &onto,
        more,
        more_at,
        0,
        &mut y,
        &mut gaps,
        |(oid, subject), y| {
            rows.push(Row {
                oid: oid.clone(),
                subject: subject.clone(),
                lane: 0,
                dim: false,
                y,
                ..Default::default()
            });
        },
    );
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
        &gaps,
        0,
        split_note(&d["base"]),
        None,
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
    let mut after_gaps = Vec::new();
    y = TOP;
    with_gap(
        &picks,
        picks_more,
        picks_at,
        0,
        &mut y,
        &mut after_gaps,
        |(oid, subject), y| {
            after.push(Row {
                oid: oid.clone(),
                subject: format!("{subject}′"),
                lane: 0,
                dim: false,
                y,
                ..Default::default()
            });
        },
    );
    with_gap(
        &onto,
        more,
        more_at,
        0,
        &mut y,
        &mut after_gaps,
        |(oid, subject), y| {
            after.push(Row {
                oid: oid.clone(),
                subject: subject.clone(),
                lane: 0,
                dim: true,
                y,
                ..Default::default()
            });
        },
    );
    let first_pick = picks.first().map(|p| p.0.clone());
    let onto_ids: Vec<String> = onto.iter().map(|c| c.0.clone()).collect();
    let base_id = base.as_ref().map(|b| b.0.clone());
    column(
        area,
        cr,
        column_width(area) + 24.0,
        &after,
        y,
        base.as_ref(),
        &after_gaps,
        picks.len(),
        None,
        None,
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
    arrow(area, cr, &fg, picks.len());
}

fn dimmed(c: &gdk::RGBA, alpha: f32) -> gdk::RGBA {
    let mut c = *c;
    c.set_alpha(c.alpha() * alpha);
    c
}

/// 省いたコミットの行（線、y、数）。そこだけ線を点線にして「ほか n コミット」と書く。
type Gap = (u8, f64, u64);

/// 1 列を描く。lane 1 から先は右の線（それぞれ、いちばん下で lane 0 へ合流。octopus merge ではいくつも）。
/// `highlight` は強調する上からの行数。`gaps` は省いたところ、`base_note` は分かれたところに添える文（いつ
/// 分かれたか）。`lane1_join` は merge コミットの行で、そこから右の線それぞれの先頭へ下りる。
#[allow(clippy::too_many_arguments)]
fn column(
    area: &gtk::DrawingArea,
    cr: &gtk::cairo::Context,
    x: f64,
    rows: &[Row],
    base_y: f64,
    base: Option<&(String, String)>,
    gaps: &[Gap],
    highlight: usize,
    base_note: Option<String>,
    lane1_join: Option<f64>,
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
    // まっすぐな線を、省いたところ（前の点から次の点まで）だけ点線にして引く。
    let vertical = |lane: u8, top: f64, bottom: f64| {
        let mut sorted: Vec<&Gap> = gaps.iter().filter(|g| g.0 == lane).collect();
        sorted.sort_by(|a, b| a.1.total_cmp(&b.1));
        let mut y = top;
        for g in sorted {
            let (a, b) = (center(g.1) - ROW, center(g.1) + ROW);
            if a < top || b > bottom {
                continue;
            }
            cr.move_to(lane_x(lane), y);
            cr.line_to(lane_x(lane), a);
            let _ = cr.stroke();
            cr.set_dash(&[3.0, 3.0], 0.0);
            cr.move_to(lane_x(lane), a);
            cr.line_to(lane_x(lane), b);
            let _ = cr.stroke();
            cr.set_dash(&[], 0.0);
            y = b;
        }
        cr.move_to(lane_x(lane), y);
        cr.line_to(lane_x(lane), bottom);
        let _ = cr.stroke();
    };
    // 右の線（octopus merge ではいくつも）は、それぞれいちばん下で左の線へ合流する。
    let lanes = rows.iter().map(|r| r.lane).max().unwrap_or(1).max(1);
    for lane in 1..=lanes {
        let on: Vec<&Row> = rows.iter().filter(|r| r.lane == lane).collect();
        let (Some(first), Some(last)) = (on.first(), on.last()) else {
            continue;
        };
        let last_y = gaps
            .iter()
            .filter(|g| g.0 == lane)
            .map(|g| g.1)
            .fold(last.y, f64::max);
        vertical(lane, center(first.y), center(last_y));
        cr.move_to(lane_x(lane), center(last_y));
        // 制御点は macOS 版と同じ並び。
        cr.curve_to(
            lane_x(lane),
            center(base_y) - 6.0,
            lane_x(0),
            center(last_y) + 8.0,
            lane_x(0),
            center(base_y),
        );
        let _ = cr.stroke();
        // merge コミットから、その線へ曲がり、取り込む側の先端まで下りる（macOS 版と同じ）。
        if let Some(join) = lane1_join {
            cr.move_to(lane_x(0), center(join));
            cr.curve_to(
                lane_x(lane),
                center(join),
                lane_x(lane),
                center(join),
                lane_x(lane),
                center(join) + ROW * 0.8,
            );
            cr.line_to(lane_x(lane), center(first.y));
            let _ = cr.stroke();
        }
    }
    if let Some(first) = rows.iter().find(|r| r.lane == 0) {
        vertical(0, center(first.y), center(base_y));
    }
    // 点と件名、札。
    let text_x = lane_x(lanes) + 14.0;
    for (i, row) in rows.iter().enumerate() {
        row_at(
            area,
            cr,
            lane_x(row.lane),
            row.y,
            text_x,
            x + column_width(area),
            &row.subject,
            row.dim,
            i < highlight,
            &labels(&row.oid),
            row.hollow,
            row.note.as_ref(),
            row.is_new,
        );
    }
    for &(_, y, count) in gaps {
        text_at(
            area,
            cr,
            &tr!("and {count} more", count = count),
            text_x,
            y + 3.0,
            &dimmed(&fg, 0.5),
            false,
            x + column_width(area) - text_x,
        );
    }
    if let Some((oid, subject)) = base {
        let note = base_note.map(|n| (n, None));
        row_at(
            area,
            cr,
            lane_x(0),
            base_y,
            text_x,
            x + column_width(area),
            subject,
            true,
            false,
            &labels(oid),
            false,
            note.as_ref(),
            false,
        );
    }
}

/// 分かれたところに添える文（「3 か月前に分かれた」）。日時がなければ None。
fn split_note(base: &Value) -> Option<String> {
    let time = base["time"].as_i64().filter(|t| *t > 0)?;
    Some(tr!("split off {when}", when = crate::stashes::ago(time)))
}

/// 省いたところを挟んで行を並べる（`at` の前に省いた行。macOS 版と同じ）。`y` を進める。
fn with_gap(
    items: &[(String, String)],
    more: u64,
    at: Option<u64>,
    lane: u8,
    y: &mut f64,
    gaps: &mut Vec<Gap>,
    mut push: impl FnMut(&(String, String), f64),
) {
    let at = if more > 0 {
        at.map_or(items.len(), |a| a as usize)
    } else {
        usize::MAX
    };
    for (i, c) in items.iter().enumerate() {
        if i == at {
            gaps.push((lane, *y, more));
            *y += ROW;
        }
        push(c, *y);
        *y += ROW;
    }
    if at == items.len() {
        gaps.push((lane, *y, more));
        *y += ROW;
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
    hollow: bool,
    note: Option<&(String, Option<gdk::RGBA>)>,
    is_new: bool,
) {
    let fg = area.color();
    let r = if highlighted || is_new { 5.0 } else { 4.0 };
    let dot = if highlighted {
        accent(area)
    } else if dim {
        dimmed(&fg, 0.35)
    } else {
        dimmed(&fg, 0.65)
    };
    let dot = if hollow {
        note.and_then(|n| n.1).unwrap_or(dot)
    } else {
        dot
    };
    cr.set_source_rgba(
        dot.red().into(),
        dot.green().into(),
        dot.blue().into(),
        dot.alpha().into(),
    );
    // 前の文字の位置から線がつながらないよう、経路を空にしてから。
    cr.new_path();
    cr.arc(dot_x, y + ROW / 2.0, r, 0.0, std::f64::consts::TAU);
    if is_new {
        // これから作るコミットは、点線の丸（まだない）。
        let a = accent(area);
        cr.set_source_rgba(a.red().into(), a.green().into(), a.blue().into(), 1.0);
        cr.set_line_width(1.5);
        cr.set_dash(&[2.0, 2.0], 0.0);
        let _ = cr.stroke();
        cr.set_dash(&[], 0.0);
    } else if hollow {
        cr.set_line_width(1.5);
        let _ = cr.stroke();
    } else {
        let _ = cr.fill();
    }
    let mut x = text_x;
    for (name, kind, current) in labels {
        x = pill(area, cr, name, kind, *current, x, y + 3.0) + 4.0;
    }
    let color = if dim { dimmed(&fg, 0.45) } else { fg };
    let Some((note, note_color)) = note else {
        text_at(
            area,
            cr,
            subject,
            x,
            y + 3.0,
            &color,
            highlighted || is_new,
            (right - x).max(0.0),
        );
        return;
    };
    // 件名の後ろに、添える文を別の色で。
    let text = format!("{subject} — {note}");
    let layout = area.create_pango_layout(Some(&text));
    let mut font = area.pango_context().font_description().unwrap_or_default();
    font.set_size(font.size() * 11 / 13);
    layout.set_font_description(Some(&font));
    layout.set_width(((right - x).max(0.0) * f64::from(pango::SCALE)) as i32);
    layout.set_ellipsize(pango::EllipsizeMode::End);
    let attrs = pango::AttrList::new();
    // 強調する行は、件名だけ太字で（添える文はふつうの太さ）。
    if highlighted || is_new {
        let mut bold = pango::AttrInt::new_weight(pango::Weight::Bold);
        bold.set_end_index(subject.len() as u32);
        attrs.insert(bold);
    }
    let c = note_color.unwrap_or_else(|| dimmed(&fg, 0.5));
    let to16 = |v: f32| (v.clamp(0.0, 1.0) * 65535.0) as u16;
    let mut fgattr =
        pango::AttrColor::new_foreground(to16(c.red()), to16(c.green()), to16(c.blue()));
    fgattr.set_start_index(subject.len() as u32);
    attrs.insert(fgattr);
    layout.set_attributes(Some(&attrs));
    cr.set_source_rgba(
        color.red().into(),
        color.green().into(),
        color.blue().into(),
        color.alpha().into(),
    );
    cr.move_to(x, y + 3.0);
    pangocairo::functions::show_layout(cr, &layout);
}

/// ローカルの札の青（macOS 版の `systemBlue`。アクセントカラーによらない）。
const BLUE: gdk::RGBA = gdk::RGBA::new(0.21, 0.52, 0.89, 1.0);

/// 強調する点と、これから作るコミットの丸の色（テーマのアクセントカラー。macOS 版の `controlAccentColor`）。
/// 名前はテーマによって違う（libadwaita は `accent_bg_color`、1.6 からシステムのアクセントカラーに合う）。
#[allow(deprecated)]
fn accent(area: &gtk::DrawingArea) -> gdk::RGBA {
    let style = area.style_context();
    ["accent_bg_color", "theme_selected_bg_color"]
        .iter()
        .find_map(|name| style.lookup_color(name))
        .unwrap_or(BLUE)
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
        _ => BLUE,
    };
    let layout = area.create_pango_layout(Some(name));
    let mut font = area.pango_context().font_description().unwrap_or_default();
    // macOS 版と同じ割合（ふつうの字 13 pt に対して 10 pt）。
    font.set_size(font.size() * 10 / 13);
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
    // macOS 版と同じ割合（ふつうの字 13 pt に対して小さな字 11 pt）。件名が列に収まるように。
    font.set_size(font.size() * 11 / 13);
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

/// ref を動かす確認（Force Push・Push・Reset）の絵の見せ方（macOS 版の `MoveDrawingView` の引数）。
pub struct Move {
    /// 動かす ref（`main`・`origin/main`）と、その札の種類（`local`・`remote`）。
    pub moving: String,
    pub kind: &'static str,
    /// 右の見出し（「Push の後」など）。
    pub after_title: String,
    /// 外れるコミットに添える文と色（Force Push は赤、Reset は薄く）。
    pub lost_note: String,
    pub lost_red: bool,
    /// ref が消える（ブランチの削除）。右には札を描かない。
    pub removes: bool,
}

/// ref を動かす確認に埋め込む、前と後の絵（docs/design.md の「Previewing graph changes」）。左が今、右が動かした
/// 後。行き先だけにあるコミットは左の線、外れるコミットは右の線で、分かれたところで合流する（片方しかなければ
/// 1 本）。動かす ref の札は、右では行き先へ移る。外れるコミットは右で中を抜いた点にして、文を添える。材料は
/// `movePreview` の答え。`set` で答えを渡す。`drawing` がなければ隠す。
pub fn new_move(how: Move) -> (gtk::DrawingArea, Setter) {
    let area = gtk::DrawingArea::new();
    area.set_content_width((column_width(&area) * 2.0 + 24.0) as i32);
    area.set_visible(false);
    let info: Rc<RefCell<Value>> = Rc::default();
    let i = info.clone();
    let how = Rc::new(how);
    area.set_draw_func(move |area, cr, _, _| draw_move(area, cr, &i.borrow(), &how));
    let a = area.downgrade();
    let set: Setter = Rc::new(move |value: &Value| {
        let Some(area) = a.upgrade() else { return };
        info.replace(value.clone());
        let d = &value["drawing"];
        if d.is_null() {
            area.set_visible(false);
            return;
        }
        area.set_content_height((move_rows(d) as f64 * ROW + 26.0) as i32);
        area.set_visible(true);
        area.queue_draw();
    });
    (area, set)
}

fn move_rows(d: &Value) -> usize {
    let len = |k: &str| d[k].as_array().map_or(0, Vec::len);
    let more = |k: &str| usize::from(d[k].as_u64().unwrap_or(0) > 0);
    len("gained")
        + more("gainedMore")
        + len("lost")
        + more("lostMore")
        + usize::from(!d["base"].is_null())
}

fn draw_move(area: &gtk::DrawingArea, cr: &gtk::cairo::Context, info: &Value, how: &Move) {
    let d = &info["drawing"];
    if d.is_null() {
        return;
    }
    let fg = area.color();
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
    let gained = commits(&d["gained"]);
    let lost = commits(&d["lost"]);
    let hidden: Vec<String> = d["gained"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| c["hidden"].as_bool() == Some(true))
        .filter_map(|c| c["oid"].as_str().map(str::to_owned))
        .collect();
    let gained_more = d["gainedMore"].as_u64().unwrap_or(0);
    let lost_more = d["lostMore"].as_u64().unwrap_or(0);
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
    heading(&how.after_title, column_width(area) + 28.0);
    // 両方にあれば 2 本（行き先の側が左）、片方だけなら 1 本。
    let lost_lane = u8::from(!gained.is_empty());
    // 動かす ref の行き先（行き先だけのコミットがなければ、分かれたところ）。
    let destination = gained
        .first()
        .map(|c| c.0.clone())
        .or_else(|| base.as_ref().map(|b| b.0.clone()));
    let red = gdk::RGBA::new(0.89, 0.29, 0.29, 1.0);
    for after in [false, true] {
        let x = if after {
            column_width(area) + 24.0
        } else {
            0.0
        };
        let mut y = TOP;
        let mut rows = Vec::new();
        let mut gaps = Vec::new();
        // 長ければ真ん中を省く（そこだけ点線にして「ほか n コミット」）。
        with_gap(
            &gained,
            gained_more,
            d["gainedMoreAt"].as_u64(),
            0,
            &mut y,
            &mut gaps,
            |(oid, subject), y| {
                // どこからも辿れないコミットは、今は History にない（Reflog にだけある。macOS 版と同じ）。
                let gone = !after && hidden.contains(oid);
                rows.push(Row {
                    oid: oid.clone(),
                    subject: subject.clone(),
                    dim: gone,
                    hollow: gone,
                    note: gone.then(|| (tr!("only in Reflog"), None)),
                    y,
                    ..Default::default()
                });
            },
        );
        with_gap(
            &lost,
            lost_more,
            d["lostMoreAt"].as_u64(),
            lost_lane,
            &mut y,
            &mut gaps,
            |(oid, subject), y| {
                rows.push(Row {
                    oid: oid.clone(),
                    subject: subject.clone(),
                    lane: lost_lane,
                    dim: after,
                    y,
                    hollow: after,
                    note: after.then(|| (how.lost_note.clone(), how.lost_red.then_some(red))),
                    ..Default::default()
                });
            },
        );
        // 両方の側があれば、左の分かれたところに、いつ分かれたか。
        let split = (!after && !gained.is_empty() && !lost.is_empty())
            .then(|| split_note(&d["base"]))
            .flatten();
        column(
            area,
            cr,
            x,
            &rows,
            y,
            base.as_ref(),
            &gaps,
            if after { gained.len() } else { 0 },
            split,
            None,
            &|oid| {
                let mut out: Vec<Label> = labels
                    .get(oid)
                    .into_iter()
                    .flatten()
                    .filter(|(n, _)| *n != how.moving)
                    .map(|(n, k)| (n.clone(), k.clone(), false))
                    .collect();
                // 動かす ref は、左では今のところ、右では行き先に、塗った札で。
                let here = if after {
                    destination.as_deref() == Some(oid) && !how.removes
                } else {
                    labels
                        .get(oid)
                        .is_some_and(|l| l.iter().any(|(n, _)| *n == how.moving))
                };
                if here {
                    out.insert(0, (how.moving.clone(), how.kind.to_owned(), true));
                }
                out
            },
        );
    }
    // 真ん中の矢印。
    arrow(area, cr, &fg, move_rows(d));
}

/// merge の見込み（`mergePreview` の答え）の絵。fast-forward なら ref が動く絵（[`new_move`]）、merge コミットを作るなら
/// [`draw_merge`]。描くものがなければ None。`current` は今のブランチ、`after_title` は右の見出し（macOS 版の
/// `mergeDrawing`）。
pub fn for_merge(info: &Value, current: &str, after_title: String) -> Option<gtk::DrawingArea> {
    if info["fastForward"].as_bool() == Some(true) {
        let mv = &info["move"];
        if mv["drawing"].is_null() {
            return None;
        }
        let (area, set) = new_move(Move {
            moving: current.to_owned(),
            kind: "local",
            after_title,
            lost_note: String::new(),
            lost_red: false,
            removes: false,
        });
        set(mv);
        return Some(area);
    }
    merge_area(info, current, after_title, false)
}

/// Pull で取り込めないとき（Fast-Forward Only など）の、左の今だけの絵と、右の列（後の絵の場所）の左端。右には呼ぶ側が
/// 取り込めないことを重ねる（macOS 版の `mergeDrawing(nowOnly:)`）。merge コミットを作る見込みでなければ None。
pub fn for_merge_now(
    info: &Value,
    current: &str,
    after_title: String,
) -> Option<(gtk::DrawingArea, f64)> {
    if info["fastForward"].as_bool() == Some(true) {
        return None;
    }
    let area = merge_area(info, current, after_title, true)?;
    let x = column_width(&area) + 28.0;
    Some((area, x))
}

fn merge_area(
    info: &Value,
    current: &str,
    after_title: String,
    now_only: bool,
) -> Option<gtk::DrawingArea> {
    let d = &info["drawing"];
    if d.is_null() {
        return None;
    }
    let area = gtk::DrawingArea::new();
    area.set_content_width((column_width(&area) * 2.0 + 24.0) as i32);
    // 右には merge コミットの 1 行が増える。
    area.set_content_height(((merge_rows(d) + 1) as f64 * ROW + 26.0) as i32);
    let info = info.clone();
    let current = current.to_owned();
    area.set_draw_func(move |area, cr, _, _| {
        draw_merge(area, cr, &info, &current, &after_title, now_only)
    });
    Some(area)
}

/// merge の絵の左の行数（今のブランチ、取り込む側、分かれたところ）。
fn merge_rows(d: &Value) -> usize {
    let rows = |k: &str, more: &str| {
        d[k].as_array().map_or(0, Vec::len) + usize::from(d[more].as_u64().unwrap_or(0) > 0)
    };
    rows("ours", "oursMore") + rows("theirs", "theirsMore") + usize::from(!d["base"].is_null())
}

/// merge の確認の、前と後の絵（docs/design.md の「Previewing graph changes」。macOS 版の `MergeDrawingView`）。左が今:
/// 今のブランチ（左の線）と取り込む側（右の線）が、分かれたところで合流する。右が後: 一番上に、これから作る merge
/// コミット（点線の丸）が両方の先端を親に持ち、今のブランチの札がそこへ移る。
fn draw_merge(
    area: &gtk::DrawingArea,
    cr: &gtk::cairo::Context,
    info: &Value,
    current: &str,
    after_title: &str,
    now_only: bool,
) {
    let d = &info["drawing"];
    let fg = area.color();
    let labels = labels_of(d);
    let ours = commits(&d["ours"]);
    let theirs = commits(&d["theirs"]);
    let base = (!d["base"].is_null()).then(|| {
        (
            d["base"]["oid"].as_str().unwrap_or_default().to_owned(),
            d["base"]["subject"].as_str().unwrap_or_default().to_owned(),
        )
    });
    let ours_tip = ours
        .first()
        .map(|c| c.0.clone())
        .or_else(|| base.as_ref().map(|b| b.0.clone()));
    let heading = |text: &str, x: f64| {
        text_at(area, cr, text, x, 2.0, &dimmed(&fg, 0.6), true, f64::MAX);
    };
    heading(&tr!("Now"), 4.0);
    heading(after_title, column_width(area) + 28.0);
    let columns: &[bool] = if now_only { &[false] } else { &[false, true] };
    for &after in columns {
        let x = if after {
            column_width(area) + 24.0
        } else {
            0.0
        };
        let mut y = TOP;
        let mut rows = Vec::new();
        let mut gaps = Vec::new();
        // 右の一番上に、これから作る merge コミット。
        let merge_y = y;
        if after {
            rows.push(Row {
                subject: d["subject"].as_str().unwrap_or_default().to_owned(),
                y,
                is_new: true,
                ..Default::default()
            });
            y += ROW;
        }
        with_gap(
            &ours,
            d["oursMore"].as_u64().unwrap_or(0),
            d["oursMoreAt"].as_u64(),
            0,
            &mut y,
            &mut gaps,
            |(oid, subject), y| {
                rows.push(Row {
                    oid: oid.clone(),
                    subject: subject.clone(),
                    y,
                    ..Default::default()
                });
            },
        );
        with_gap(
            &theirs,
            d["theirsMore"].as_u64().unwrap_or(0),
            d["theirsMoreAt"].as_u64(),
            1,
            &mut y,
            &mut gaps,
            |(oid, subject), y| {
                rows.push(Row {
                    oid: oid.clone(),
                    subject: subject.clone(),
                    lane: 1,
                    y,
                    ..Default::default()
                });
            },
        );
        column(
            area,
            cr,
            x,
            &rows,
            y,
            base.as_ref(),
            &gaps,
            0,
            if after { None } else { split_note(&d["base"]) },
            after.then_some(merge_y),
            &|oid| {
                let mut out: Vec<Label> = labels
                    .get(oid)
                    .into_iter()
                    .flatten()
                    .filter(|(n, _)| n != current)
                    .map(|(n, k)| (n.clone(), k.clone(), false))
                    .collect();
                // 今のブランチの札は、左では今の先端、右では merge コミットに、塗った札で。
                let here = if after {
                    oid.is_empty()
                } else {
                    ours_tip.as_deref() == Some(oid)
                };
                if here {
                    out.insert(0, (current.to_owned(), "local".to_owned(), true));
                }
                out
            },
        );
    }
    arrow(area, cr, &fg, merge_rows(d));
}

/// cherry-pick・revert の見込み（`pickPreview` の答え）の絵（macOS 版の `PickDrawingView`）。左が今の先端、右が後:
/// その上に、これから作るコミット（点線の丸。cherry-pick は件名に ′、revert は「Revert "…"」）が並び、今のブランチの
/// 札が一番上へ移る。描くものがなければ None。
/// 何で作るか（見出しと、件名の書き方が変わる。macOS 版の `PickDrawingView.Kind`）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PickKind {
    CherryPick,
    Revert,
    Patches,
}

pub fn for_pick(info: &Value, current: &str, kind: PickKind) -> Option<gtk::DrawingArea> {
    let d = &info["drawing"];
    if d.is_null() {
        return None;
    }
    let area = gtk::DrawingArea::new();
    area.set_content_width((column_width(&area) * 2.0 + 24.0) as i32);
    let picks = d["picks"].as_array().map_or(0, Vec::len)
        + usize::from(d["picksMore"].as_u64().unwrap_or(0) > 0);
    let head = d["head"].as_array().map_or(0, Vec::len);
    area.set_content_height(((picks + head) as f64 * ROW + 26.0) as i32);
    let info = info.clone();
    let current = current.to_owned();
    area.set_draw_func(move |area, cr, _, _| draw_pick(area, cr, &info, &current, kind));
    Some(area)
}

fn draw_pick(
    area: &gtk::DrawingArea,
    cr: &gtk::cairo::Context,
    info: &Value,
    current: &str,
    kind: PickKind,
) {
    let d = &info["drawing"];
    let fg = area.color();
    let labels = labels_of(d);
    let head = commits(&d["head"]);
    let picks = commits(&d["picks"]);
    let tip = head.first().map(|c| c.0.clone());
    let first_new = picks.first().map(|c| format!("new:{}", c.0));
    let heading = |text: &str, x: f64| {
        text_at(area, cr, text, x, 2.0, &dimmed(&fg, 0.6), true, f64::MAX);
    };
    heading(&tr!("Now"), 4.0);
    heading(
        &match kind {
            PickKind::CherryPick => tr!("After the cherry-pick"),
            PickKind::Revert => tr!("After the revert"),
            PickKind::Patches => tr!("After applying the patches"),
        },
        column_width(area) + 28.0,
    );
    for after in [false, true] {
        let x = if after {
            column_width(area) + 24.0
        } else {
            0.0
        };
        let mut y = TOP;
        let mut rows = Vec::new();
        let mut gaps = Vec::new();
        if after {
            with_gap(
                &picks,
                d["picksMore"].as_u64().unwrap_or(0),
                d["picksMoreAt"].as_u64(),
                0,
                &mut y,
                &mut gaps,
                |(oid, subject), y| {
                    rows.push(Row {
                        oid: format!("new:{oid}"),
                        subject: if kind == PickKind::CherryPick {
                            format!("{subject}′")
                        } else {
                            subject.clone()
                        },
                        y,
                        is_new: true,
                        ..Default::default()
                    });
                },
            );
        }
        for (i, (oid, subject)) in head.iter().enumerate() {
            rows.push(Row {
                oid: oid.clone(),
                subject: subject.clone(),
                dim: after || i > 0,
                y,
                ..Default::default()
            });
            y += ROW;
        }
        column(
            area,
            cr,
            x,
            &rows,
            y - ROW,
            None,
            &gaps,
            0,
            None,
            None,
            &|oid| {
                let mut out: Vec<Label> = labels
                    .get(oid)
                    .into_iter()
                    .flatten()
                    .filter(|(n, _)| n != current)
                    .map(|(n, k)| (n.clone(), k.clone(), false))
                    .collect();
                let here = if after {
                    first_new.as_deref() == Some(oid)
                } else {
                    tip.as_deref() == Some(oid)
                };
                if here {
                    out.insert(0, (current.to_owned(), "local".to_owned(), true));
                }
                out
            },
        );
    }
    arrow(
        area,
        cr,
        &fg,
        picks.len() + usize::from(d["picksMore"].as_u64().unwrap_or(0) > 0),
    );
}

/// octopus merge の絵に渡す、取り込む順と merge コミットの件名（シートで並べ替えるたびに）。
pub type OctopusSetter = Rc<dyn Fn(Vec<String>, String)>;

/// 線は 4 本まで、残りは「ほか n 本」（macOS 版の `OctopusDrawingView.maxLanes`）。
const MAX_LANES: usize = 4;

/// 描く取り込む側（並べた順、もう取り込んであるものは除く）と、描かないものの数。
fn octopus_sides<'a>(d: &'a Value, order: &[String]) -> (Vec<&'a Value>, usize) {
    let sides: Vec<&Value> = d["sides"].as_array().into_iter().flatten().collect();
    let all: Vec<&Value> = order
        .iter()
        .filter_map(|rev| {
            sides
                .iter()
                .find(|s| s["rev"].as_str() == Some(rev))
                .copied()
        })
        .filter(|s| !s["tip"].is_null())
        .collect();
    let rest = all.len().saturating_sub(MAX_LANES);
    (all.into_iter().take(MAX_LANES).collect(), rest)
}

fn octopus_rows(d: &Value, order: &[String]) -> usize {
    let (shown, rest) = octopus_sides(d, order);
    d["ours"].as_array().map_or(0, Vec::len)
        + usize::from(d["oursMore"].as_u64().unwrap_or(0) > 0)
        + shown
            .iter()
            .map(|s| 1 + usize::from(s["more"].as_u64().unwrap_or(0) > 0))
            .sum::<usize>()
        + usize::from(rest > 0)
        + usize::from(!d["base"].is_null())
}

/// octopus merge の見込み（`octopusPreview` の答え）の絵（macOS 版の `OctopusDrawingView`）。描くものがなければ
/// None。左が今: 今のブランチ（左の線）と、取り込むもの（並べた順に右の線。先端と、ほかにいくつあるか）が、
/// すべてが分かれたところで合流する。右が後: 一番上の merge コミット（点線の丸）がすべての先端を親に持ち、今の
/// ブランチの札がそこへ移る。
pub fn for_octopus(info: &Value, current: &str) -> Option<(gtk::DrawingArea, OctopusSetter)> {
    let d = &info["drawing"];
    if d.is_null() {
        return None;
    }
    let area = gtk::DrawingArea::new();
    area.set_content_width((column_width(&area) * 2.0 + 24.0) as i32);
    let state: Rc<RefCell<(Vec<String>, String)>> = Rc::default();
    let s = state.clone();
    let info = info.clone();
    let current = current.to_owned();
    let d = info["drawing"].clone();
    area.set_draw_func(move |area, cr, _, _| {
        let (order, subject) = &*s.borrow();
        draw_octopus(area, cr, &info["drawing"], &current, order, subject);
    });
    let a = area.downgrade();
    let set: OctopusSetter = Rc::new(move |order, subject| {
        let Some(area) = a.upgrade() else { return };
        // 右には merge コミットの 1 行が増える。
        area.set_content_height(((octopus_rows(&d, &order) + 1) as f64 * ROW + 26.0) as i32);
        state.replace((order, subject));
        area.queue_draw();
    });
    Some((area, set))
}

fn draw_octopus(
    area: &gtk::DrawingArea,
    cr: &gtk::cairo::Context,
    d: &Value,
    current: &str,
    order: &[String],
    subject: &str,
) {
    let fg = area.color();
    let labels = labels_of(d);
    let ours = commits(&d["ours"]);
    let base = (!d["base"].is_null()).then(|| {
        (
            d["base"]["oid"].as_str().unwrap_or_default().to_owned(),
            d["base"]["subject"].as_str().unwrap_or_default().to_owned(),
        )
    });
    let ours_tip = ours
        .first()
        .map(|c| c.0.clone())
        .or_else(|| base.as_ref().map(|b| b.0.clone()));
    let (shown, rest) = octopus_sides(d, order);
    let tips: HashMap<String, String> = shown
        .iter()
        .filter_map(|s| {
            Some((
                s["tip"]["oid"].as_str()?.to_owned(),
                s["rev"].as_str()?.to_owned(),
            ))
        })
        .collect();
    let heading = |text: &str, x: f64| {
        text_at(area, cr, text, x, 2.0, &dimmed(&fg, 0.6), true, f64::MAX);
    };
    heading(&tr!("Now"), 4.0);
    heading(&tr!("After the merge"), column_width(area) + 28.0);
    for after in [false, true] {
        let x = if after {
            column_width(area) + 24.0
        } else {
            0.0
        };
        let mut y = TOP;
        let mut rows = Vec::new();
        let mut gaps = Vec::new();
        let merge_y = y;
        if after {
            rows.push(Row {
                subject: subject.to_owned(),
                y,
                is_new: true,
                ..Default::default()
            });
            y += ROW;
        }
        with_gap(
            &ours,
            d["oursMore"].as_u64().unwrap_or(0),
            d["oursMoreAt"].as_u64(),
            0,
            &mut y,
            &mut gaps,
            |(oid, subject), y| {
                rows.push(Row {
                    oid: oid.clone(),
                    subject: subject.clone(),
                    y,
                    ..Default::default()
                });
            },
        );
        for (i, side) in shown.iter().enumerate() {
            let lane = i as u8 + 1;
            rows.push(Row {
                oid: side["tip"]["oid"].as_str().unwrap_or_default().to_owned(),
                subject: side["tip"]["subject"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                lane,
                y,
                ..Default::default()
            });
            y += ROW;
            let more = side["more"].as_u64().unwrap_or(0);
            if more > 0 {
                gaps.push((lane, y, more));
                y += ROW;
            }
        }
        if rest > 0 {
            let text_x = x + 10.0 + shown.len() as f64 * LANE + 14.0;
            text_at(
                area,
                cr,
                &tr!("and {count} more branches", count = rest),
                text_x,
                y + 3.0,
                &dimmed(&fg, 0.5),
                false,
                x + column_width(area) - text_x,
            );
            y += ROW;
        }
        column(
            area,
            cr,
            x,
            &rows,
            y,
            base.as_ref(),
            &gaps,
            0,
            if after { None } else { split_note(&d["base"]) },
            after.then_some(merge_y),
            &|oid| {
                let mut out: Vec<Label> = labels
                    .get(oid)
                    .into_iter()
                    .flatten()
                    .filter(|(n, _)| n != current)
                    .map(|(n, k)| (n.clone(), k.clone(), false))
                    .collect();
                // タグ・コミットを取り込むときは、その名前を札に。
                if let Some(rev) = tips.get(oid)
                    && !out.iter().any(|l| &l.0 == rev)
                {
                    out.push((rev.clone(), "local".to_owned(), false));
                }
                let here = if after {
                    oid.is_empty()
                } else {
                    ours_tip.as_deref() == Some(oid)
                };
                if here {
                    out.insert(0, (current.to_owned(), "local".to_owned(), true));
                }
                out
            },
        );
    }
    arrow(area, cr, &fg, octopus_rows(d, order));
}

/// History の編集の前と後（`rewritePreview` の答えの `editDrawing`）の絵（macOS 版の `EditDrawingView`）。描く
/// ものがなければ None。左が今: 書き換わるコミットで、drop するものは中を抜いた点、まとめる・メッセージを変える・
/// 止まるものには、そうと添える。右が後: できるコミット（′ 付き。まとめたものは 1 行、メッセージを変えたものは
/// 新しい件名）。どちらも 1 本の線で、下に書き換えないコミット。
pub fn for_edit(info: &Value) -> Option<gtk::DrawingArea> {
    let d = &info["editDrawing"];
    if d.is_null() {
        return None;
    }
    let area = gtk::DrawingArea::new();
    area.set_content_width((column_width(&area) * 2.0 + 24.0) as i32);
    let count =
        edit_rows(d, "before").max(edit_rows(d, "after")) + usize::from(!d["base"].is_null());
    area.set_content_height((count as f64 * ROW + 26.0) as i32);
    let d = d.clone();
    let current = info["current"].as_str().map(str::to_owned);
    area.set_draw_func(move |area, cr, _, _| draw_edit(area, cr, &d, current.as_deref()));
    Some(area)
}

fn edit_rows(d: &Value, side: &str) -> usize {
    d[side].as_array().map_or(0, Vec::len)
        + usize::from(d[format!("{side}More")].as_u64().unwrap_or(0) > 0)
}

fn draw_edit(area: &gtk::DrawingArea, cr: &gtk::cairo::Context, d: &Value, current: Option<&str>) {
    let fg = area.color();
    let labels = labels_of(d);
    let red = gdk::RGBA::new(0.89, 0.29, 0.29, 1.0);
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
    heading(&tr!("After the edit"), column_width(area) + 28.0);
    // 下の書き換えないコミットは、左右で同じ高さに。
    let base_y = TOP + edit_rows(d, "before").max(edit_rows(d, "after")) as f64 * ROW;
    for after in [false, true] {
        let x = if after {
            column_width(area) + 24.0
        } else {
            0.0
        };
        let side = if after { "after" } else { "before" };
        let steps: Vec<&Value> = d[side].as_array().into_iter().flatten().collect();
        let items: Vec<(String, String)> = steps
            .iter()
            .map(|s| {
                (
                    s["oid"].as_str().unwrap_or_default().to_owned(),
                    s["subject"].as_str().unwrap_or_default().to_owned(),
                )
            })
            .collect();
        let mut y = TOP;
        let mut rows = Vec::new();
        let mut gaps = Vec::new();
        let mut i = 0;
        with_gap(
            &items,
            d[format!("{side}More")].as_u64().unwrap_or(0),
            d[format!("{side}MoreAt")].as_u64(),
            0,
            &mut y,
            &mut gaps,
            |(oid, subject), y| {
                let step = steps[i];
                i += 1;
                let mut row = Row {
                    oid: oid.clone(),
                    subject: if after {
                        format!("{subject}′")
                    } else {
                        subject.clone()
                    },
                    dim: !after,
                    y,
                    ..Default::default()
                };
                if after {
                    let n = step["combined"].as_u64().unwrap_or(0);
                    if n > 0 {
                        row.note = Some((tr!("{count} commits in one", count = n + 1), None));
                    }
                } else {
                    match step["action"].as_str().unwrap_or_default() {
                        "drop" => {
                            row.hollow = true;
                            row.note = Some((tr!("dropped"), Some(red)));
                        }
                        "squash" | "fixup" => {
                            row.note = Some((tr!("combined with the one below"), None));
                        }
                        "reword" => row.note = Some((tr!("new message"), None)),
                        "edit" => row.note = Some((tr!("stops here"), None)),
                        _ => {}
                    }
                }
                rows.push(row);
            },
        );
        let top = items.first().map(|c| c.0.clone());
        let highlight = if after { items.len() } else { 0 };
        column(
            area,
            cr,
            x,
            &rows,
            base_y,
            base.as_ref(),
            &gaps,
            highlight,
            None,
            None,
            &|oid| {
                // 今のブランチの札は、左右とも一番上に。ほかの札は左（書き換える前）と、下の書き換えないコミットに。
                let mut out: Vec<Label> = Vec::new();
                if top.as_deref() == Some(oid)
                    && let Some(current) = current
                {
                    out.push((current.to_owned(), "local".to_owned(), true));
                }
                if !after || base.as_ref().is_some_and(|b| b.0 == oid) {
                    out.extend(
                        labels
                            .get(oid)
                            .into_iter()
                            .flatten()
                            .filter(|(n, _)| Some(n.as_str()) != current)
                            .map(|(n, k)| (n.clone(), k.clone(), false)),
                    );
                }
                out
            },
        );
    }
    arrow(area, cr, &fg, edit_rows(d, "before"));
}

/// 絵の材料の `labels` を、コミットごとの（名前、種類）に。
fn labels_of(d: &Value) -> HashMap<String, Vec<(String, String)>> {
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
    labels
}

/// 真ん中の矢印。左の `rows` 行の真ん中に、ふつうより大きな字で（macOS 版と同じ 15 pt）。
fn arrow(area: &gtk::DrawingArea, cr: &gtk::cairo::Context, fg: &gdk::RGBA, rows: usize) {
    let layout = area.create_pango_layout(Some("→"));
    let mut font = area.pango_context().font_description().unwrap_or_default();
    font.set_size(font.size() * 15 / 13);
    layout.set_font_description(Some(&font));
    let c = dimmed(fg, 0.4);
    cr.set_source_rgba(
        c.red().into(),
        c.green().into(),
        c.blue().into(),
        c.alpha().into(),
    );
    cr.move_to(
        column_width(area) + 6.0,
        TOP + rows.saturating_sub(1) as f64 * ROW / 2.0,
    );
    pangocairo::functions::show_layout(cr, &layout);
}
