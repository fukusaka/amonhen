//! 確認に埋め込む、前と後の絵（GTK4 版の `rewritedraw.rs`、macOS 版の `RewriteDrawingView`。docs/design.md の
//! 「Previewing graph changes」）。左が今、右が操作の後。History と同じく新しいものが上で、線・点・ブランチの札・
//! 件名で描く。XAML の Canvas に線・丸・文字を置いて描く（曲線は短い線をつないで）。

use std::collections::HashMap;

use amonhen_model::tr;
use serde_json::Value;
use windows_reactor::*;

const ROW: f64 = 22.0;
const LANE: f64 = 14.0;
/// 列の幅。ContentDialog の中身は 500 まで（548 から余白を引く）なので、2 列と矢印がそこに収まる幅。
const COLUMN: f64 = 236.0;
const TOP: f64 = 22.0;

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
    note: Option<(String, Option<Color>)>,
    /// これから作るコミット（点線の丸）。
    is_new: bool,
}

/// 札（名前、種類 `local`・`remote`・`moved`、今のブランチか）。
type Label = (String, String, bool);

/// 省いたコミットの行（線、y、数）。そこだけ線を点線にして「ほか n コミット」と書く。
type Gap = (u8, f64, u64);

/// ref を動かす確認（Fast-Forward・Force Push・Reset など）の絵の見せ方（GTK4 版の `rewritedraw::Move`）。
pub struct Move {
    /// 動かす ref と、その札の種類（`local`・`remote`）。
    pub moving: String,
    pub kind: &'static str,
    /// 右の見出し（「fast-forward の後」など）。
    pub after_title: String,
    /// 外れるコミットに添える文と色（Force Push は赤、Reset は薄く）。
    pub lost_note: String,
    pub lost_red: bool,
    /// ref が消える（ブランチの削除）。右には札を描かない。
    pub removes: bool,
}

const FG: Color = Color::rgb(0x1f, 0x1f, 0x1f);
/// ローカルの札・強調する点の青（GTK4 版・macOS 版と同じ）。
const BLUE: Color = Color::rgb(0x36, 0x85, 0xe3);
const RED: Color = Color::rgb(0xe3, 0x4a, 0x4a);

fn alpha(c: Color, a: f64) -> Color {
    Color::argb((a.clamp(0.0, 1.0) * 255.0) as u8, c.r, c.g, c.b)
}

/// 描いたものを並べる。
struct Canvas2 {
    items: Vec<(String, View)>,
}

impl Canvas2 {
    fn push(&mut self, view: View) {
        let key = format!("{}", self.items.len());
        self.items.push((key, view));
    }

    fn line(&mut self, (x1, y1): (f64, f64), (x2, y2): (f64, f64), color: Color, width: f64) {
        self.push(
            Line::new()
                .x1(x1)
                .y1(y1)
                .x2(x2)
                .y2(y2)
                .stroke(Brush::Solid(color))
                .stroke_thickness(width)
                .into(),
        );
    }

    /// 点線（短い線を並べて）。
    fn dashed(&mut self, x: f64, top: f64, bottom: f64, color: Color, width: f64) {
        let mut y = top;
        while y < bottom {
            self.line((x, y), (x, (y + 3.0).min(bottom)), color, width);
            y += 6.0;
        }
    }

    /// 3 次ベジェ曲線（短い線をつないで）。
    fn curve(&mut self, p: [(f64, f64); 4], color: Color, width: f64) {
        let at = |t: f64| {
            let u = 1.0 - t;
            let f = |i: usize| -> (f64, f64) { p[i] };
            let x = u * u * u * f(0).0
                + 3.0 * u * u * t * f(1).0
                + 3.0 * u * t * t * f(2).0
                + t * t * t * f(3).0;
            let y = u * u * u * f(0).1
                + 3.0 * u * u * t * f(1).1
                + 3.0 * u * t * t * f(2).1
                + t * t * t * f(3).1;
            (x, y)
        };
        let steps = 12;
        let mut last = at(0.0);
        for i in 1..=steps {
            let next = at(f64::from(i) / f64::from(steps));
            self.line(last, next, color, width);
            last = next;
        }
    }

    /// 点線の丸（これから作るコミット。短い線を円に沿って一つおきに）。
    fn dashed_ring(&mut self, (x, y): (f64, f64), r: f64, color: Color) {
        let steps = 12;
        let at = |i: i32| {
            let a = std::f64::consts::TAU * f64::from(i) / f64::from(steps);
            (x + r * a.cos(), y + r * a.sin())
        };
        for i in (0..steps).step_by(2) {
            self.line(at(i), at(i + 1), color, 1.5);
        }
    }

    fn dot(&mut self, (x, y): (f64, f64), r: f64, color: Color, hollow: bool) {
        let e = Ellipse::new().width(r * 2.0).height(r * 2.0);
        let e = if hollow {
            e.stroke(Brush::Solid(color)).stroke_thickness(1.5)
        } else {
            e.fill(Brush::Solid(color))
        };
        self.push(e.canvas_left(x - r).canvas_top(y - r).into());
    }

    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        text: &str,
        x: f64,
        y: f64,
        color: Color,
        bold: bool,
        width: f64,
        size: f64,
    ) {
        let t = TextBlock::new()
            .text(text.to_owned())
            .font_size(size)
            .foreground(Brush::Solid(color))
            .text_trimming(TextTrimming::CharacterEllipsis)
            .max_width(width.max(0.0));
        let t = if bold {
            t.font_weight(FontWeight::SEMI_BOLD)
        } else {
            t
        };
        self.push(t.canvas_left(x).canvas_top(y).into());
    }

    /// History と同じ色の札（ローカルは青、リモートは紫、一緒に動いたものは緑）。右端を返す。
    fn pill(&mut self, name: &str, kind: &str, current: bool, x: f64, y: f64, max: f64) -> f64 {
        let color = match kind {
            "remote" => Color::rgb(0x91, 0x40, 0xab),
            "moved" => Color::rgb(0x2e, 0x9e, 0x52),
            _ => BLUE,
        };
        // 字の幅はおおよそ（10 pt の字で 1 文字 6 px）に、左右の余白（5 px ずつ）と、字の幅の見込み違いの分。
        let width = (name.chars().count() as f64 * 6.0 + 18.0).min(max.max(20.0));
        let fill = if current { color } else { alpha(color, 0.16) };
        let text = if current {
            Color::rgb(0xff, 0xff, 0xff)
        } else {
            color
        };
        let t = TextBlock::new()
            .text(name.to_owned())
            .font_size(10.0)
            .foreground(Brush::Solid(text))
            .text_trimming(TextTrimming::CharacterEllipsis);
        let t = if current {
            t.font_weight(FontWeight::SEMI_BOLD)
        } else {
            t
        };
        self.push(
            Border::new()
                .background(Brush::Solid(fill))
                .corner_radius(4.0)
                .padding(Thickness::new(5.0, 0.0, 5.0, 1.0))
                .width(width)
                .canvas_left(x)
                .canvas_top(y)
                .content(t),
        );
        x + width
    }
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

/// 省いたところを挟んで行を並べる（`at` の前に省いた行。GTK4 版と同じ）。`y` を進める。
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

/// 1 列を描く（GTK4 版の `column`）。lane 1 から先は右の線で、いちばん下で lane 0 へ合流する。
#[allow(clippy::too_many_arguments)]
fn column(
    c: &mut Canvas2,
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
    let lane_x = |lane: u8| x + 10.0 + f64::from(lane) * LANE;
    let center = |y: f64| y + ROW / 2.0;
    let line = alpha(FG, 0.4);
    // まっすぐな線を、省いたところ（前の点から次の点まで）だけ点線にして引く。
    let vertical = |c: &mut Canvas2, lane: u8, top: f64, bottom: f64| {
        let mut sorted: Vec<&Gap> = gaps.iter().filter(|g| g.0 == lane).collect();
        sorted.sort_by(|a, b| a.1.total_cmp(&b.1));
        let mut y = top;
        for g in sorted {
            let (a, b) = (center(g.1) - ROW, center(g.1) + ROW);
            if a < top || b > bottom {
                continue;
            }
            c.line((lane_x(lane), y), (lane_x(lane), a), line, 2.0);
            c.dashed(lane_x(lane), a, b, line, 2.0);
            y = b;
        }
        if bottom > y {
            c.line((lane_x(lane), y), (lane_x(lane), bottom), line, 2.0);
        }
    };
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
        vertical(c, lane, center(first.y), center(last_y));
        c.curve(
            [
                (lane_x(lane), center(last_y)),
                (lane_x(lane), center(base_y) - 6.0),
                (lane_x(0), center(last_y) + 8.0),
                (lane_x(0), center(base_y)),
            ],
            line,
            2.0,
        );
        // merge コミットから、その線へ曲がり、取り込む側の先端まで下りる（GTK4 版・macOS 版と同じ）。
        if let Some(join) = lane1_join {
            let bend = center(join) + ROW * 0.8;
            c.curve(
                [
                    (lane_x(0), center(join)),
                    (lane_x(lane), center(join)),
                    (lane_x(lane), center(join)),
                    (lane_x(lane), bend),
                ],
                line,
                2.0,
            );
            if center(first.y) > bend {
                c.line(
                    (lane_x(lane), bend),
                    (lane_x(lane), center(first.y)),
                    line,
                    2.0,
                );
            }
        }
    }
    if let Some(first) = rows.iter().find(|r| r.lane == 0) {
        vertical(c, 0, center(first.y), center(base_y));
    }
    // 点と件名、札。
    let text_x = lane_x(lanes) + 14.0;
    let right = x + COLUMN;
    let row_at = |c: &mut Canvas2,
                  lane: u8,
                  y: f64,
                  subject: &str,
                  dim: bool,
                  highlighted: bool,
                  labels: &[Label],
                  hollow: bool,
                  note: Option<&(String, Option<Color>)>,
                  is_new: bool| {
        let r = if highlighted || is_new { 5.0 } else { 4.0 };
        let dot = if highlighted {
            BLUE
        } else if dim {
            alpha(FG, 0.35)
        } else {
            alpha(FG, 0.65)
        };
        let dot = if hollow {
            note.and_then(|n| n.1).unwrap_or(dot)
        } else {
            dot
        };
        if is_new {
            c.dashed_ring((lane_x(lane), center(y)), r, BLUE);
        } else {
            c.dot((lane_x(lane), center(y)), r, dot, hollow);
        }
        let highlighted = highlighted || is_new;
        let mut tx = text_x;
        // 札は列の中に収める（入りきらない札は描かない）。
        for (name, kind, current) in labels {
            if right - tx <= 40.0 {
                break;
            }
            tx = c.pill(name, kind, *current, tx, y + 3.0, right - tx - 24.0) + 4.0;
        }
        if right - tx <= 20.0 {
            return;
        }
        let color = if dim { alpha(FG, 0.45) } else { FG };
        match note {
            None => c.text(subject, tx, y + 2.0, color, highlighted, right - tx, 11.0),
            // 件名の後ろに、添える文を別の色で。
            Some((note, note_color)) => {
                let width = right - tx;
                let subject_width = (subject.chars().count() as f64 * 6.5).min(width * 0.6);
                c.text(
                    subject,
                    tx,
                    y + 2.0,
                    color,
                    highlighted,
                    subject_width,
                    11.0,
                );
                c.text(
                    &format!("— {note}"),
                    tx + subject_width + 4.0,
                    y + 2.0,
                    note_color.unwrap_or(alpha(FG, 0.5)),
                    false,
                    width - subject_width - 4.0,
                    11.0,
                );
            }
        }
    };
    for (i, row) in rows.iter().enumerate() {
        row_at(
            c,
            row.lane,
            row.y,
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
        c.text(
            &tr!("and {count} more", count = count),
            text_x,
            y + 2.0,
            alpha(FG, 0.5),
            false,
            right - text_x,
            11.0,
        );
    }
    if let Some((oid, subject)) = base {
        let note = base_note.map(|n| (n, None));
        row_at(
            c,
            0,
            base_y,
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

fn move_rows(d: &Value) -> usize {
    let len = |k: &str| d[k].as_array().map_or(0, Vec::len);
    let more = |k: &str| usize::from(d[k].as_u64().unwrap_or(0) > 0);
    len("gained")
        + more("gainedMore")
        + len("lost")
        + more("lostMore")
        + usize::from(!d["base"].is_null())
}

/// ref を動かす確認の絵（`movePreview` の答え。GTK4 版の `draw_move`）。左が今、右が動かした後。行き先だけに
/// あるコミットは左の線、外れるコミットは右の線で、分かれたところで合流する。動かす ref の札は、右では
/// 行き先へ移る。外れるコミットは右で中を抜いた点にして、文を添える。描くものがなければ None。
pub fn for_move(info: &Value, how: &Move) -> Option<View> {
    let d = &info["drawing"];
    if d.is_null() {
        return None;
    }
    let labels = labels_of(d);
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
    let mut c = Canvas2 { items: Vec::new() };
    c.text(&tr!("Now"), 4.0, 2.0, alpha(FG, 0.6), true, COLUMN, 11.0);
    c.text(
        &how.after_title,
        COLUMN + 28.0,
        2.0,
        alpha(FG, 0.6),
        true,
        COLUMN,
        11.0,
    );
    // 両方にあれば 2 本（行き先の側が左）、片方だけなら 1 本。
    let lost_lane = u8::from(!gained.is_empty());
    // 動かす ref の行き先（行き先だけのコミットがなければ、分かれたところ）。
    let destination = gained
        .first()
        .map(|c| c.0.clone())
        .or_else(|| base.as_ref().map(|b| b.0.clone()));
    for after in [false, true] {
        let x = if after { COLUMN + 24.0 } else { 0.0 };
        let mut y = TOP;
        let mut rows = Vec::new();
        let mut gaps = Vec::new();
        with_gap(
            &gained,
            gained_more,
            d["gainedMoreAt"].as_u64(),
            0,
            &mut y,
            &mut gaps,
            |(oid, subject), y| {
                // どこからも辿れないコミットは、今は History にない（Reflog にだけある）。
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
                    note: after.then(|| (how.lost_note.clone(), how.lost_red.then_some(RED))),
                    is_new: false,
                });
            },
        );
        let split = (!after && !gained.is_empty() && !lost.is_empty())
            .then(|| split_note(&d["base"]))
            .flatten();
        column(
            &mut c,
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
    Some(finish(c, move_rows(d), move_rows(d)))
}

/// 真ん中の矢印（`arrow_rows` 行の真ん中の高さ）を置き、`rows` 行の高さの Canvas にする。
fn finish(mut c: Canvas2, rows: usize, arrow_rows: usize) -> View {
    c.text(
        "→",
        COLUMN + 6.0,
        TOP + arrow_rows.saturating_sub(1) as f64 * ROW / 2.0,
        alpha(FG, 0.4),
        false,
        20.0,
        15.0,
    );
    Canvas::new()
        .width(COLUMN * 2.0 + 24.0)
        .height(rows as f64 * ROW + 26.0)
        .children((View::keyed_fragment(c.items),))
}

/// rebase の確認の絵（`rewritePreview` の答え。GTK4 版の `rewritedraw::draw`）。左が今: 載せ直すコミット（右の
/// 線）と載せ先で進んだコミット（左の線）が、分かれたところで合流する。右が rebase の後: 載せ直したコミット
/// （A′）が載せ先の上に 1 本で続く。今のブランチと一緒に動くブランチの札は A′ へ移る。描くものがなければ None。
pub fn for_rewrite(info: &Value) -> Option<View> {
    let d = &info["drawing"];
    if d.is_null() {
        return None;
    }
    let current = info["current"].as_str().map(str::to_owned);
    let moving: Vec<String> = info["moving"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|b| b["name"].as_str().map(str::to_owned))
        .collect();
    let labels = labels_of(d);
    let mut picks = commits(&d["picks"]);
    picks.reverse();
    let onto = commits(&d["onto"]);
    let more = d["ontoMore"].as_u64().unwrap_or(0);
    let more_at = d["ontoMoreAt"].as_u64();
    let picks_more = d["picksMore"].as_u64().unwrap_or(0);
    let picks_at = d["picksMoreAt"].as_u64();
    let base = (!d["base"].is_null()).then(|| {
        (
            d["base"]["oid"].as_str().unwrap_or_default().to_owned(),
            d["base"]["subject"].as_str().unwrap_or_default().to_owned(),
        )
    });
    let mut c = Canvas2 { items: Vec::new() };
    c.text(&tr!("Now"), 4.0, 2.0, alpha(FG, 0.6), true, COLUMN, 11.0);
    c.text(
        &tr!("After the rebase"),
        COLUMN + 28.0,
        2.0,
        alpha(FG, 0.6),
        true,
        COLUMN,
        11.0,
    );

    // 左: 今。上に載せ直すコミット（右の線）、その下に載せ先で進んだコミット（左の線）、いちばん下で合流。
    // どちらも長ければ真ん中を省く（そこだけ点線）。
    let mut y = TOP;
    let mut rows = Vec::new();
    let mut gaps = Vec::new();
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
                y,
                ..Default::default()
            });
        },
    );
    column(
        &mut c,
        0.0,
        &rows,
        y,
        base.as_ref(),
        &gaps,
        0,
        split_note(&d["base"]),
        None,
        &|oid| {
            // 左には今のブランチの札をすべて（動くものも、まだ元のところにある）。
            labels
                .get(oid)
                .into_iter()
                .flatten()
                .map(|(n, k)| (n.clone(), k.clone(), Some(n) == current.as_ref()))
                .collect()
        },
    );

    // 右: rebase の後。
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
        &mut c,
        COLUMN + 24.0,
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
    let more = |k: &str| usize::from(d[k].as_u64().unwrap_or(0) > 0);
    let rows = picks.len() + more("picksMore") + onto.len() + more("ontoMore") + 1;
    Some(finish(c, rows, picks.len()))
}

/// merge の見込み（`mergePreview` の答え）の絵（GTK4 版の `rewritedraw::for_merge`）。fast-forward なら ref が動く絵
/// （[`for_move`]）。merge コミットを作るなら、左が今: 今のブランチ（左の線）と取り込む側（右の線）が、分かれた
/// ところで合流する。右が後: 一番上に、これから作る merge コミット（点線の丸）が両方の先端を親に持ち、今の
/// ブランチの札がそこへ移る。描くものがなければ None。
pub fn for_merge(info: &Value, current: &str, after_title: String) -> Option<View> {
    if info["fastForward"].as_bool() == Some(true) {
        return for_move(
            &info["move"],
            &Move {
                moving: current.to_owned(),
                kind: "local",
                after_title,
                lost_note: String::new(),
                lost_red: false,
                removes: false,
            },
        );
    }
    let d = &info["drawing"];
    if d.is_null() {
        return None;
    }
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
    let mut c = Canvas2 { items: Vec::new() };
    c.text(&tr!("Now"), 4.0, 2.0, alpha(FG, 0.6), true, COLUMN, 11.0);
    c.text(
        &after_title,
        COLUMN + 28.0,
        2.0,
        alpha(FG, 0.6),
        true,
        COLUMN,
        11.0,
    );
    for after in [false, true] {
        let x = if after { COLUMN + 24.0 } else { 0.0 };
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
            &mut c,
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
    let rows = merge_rows(d);
    // 右には merge コミットの 1 行が増える。
    Some(finish(c, rows + 1, rows))
}

/// merge の絵の左の行数（今のブランチ、取り込む側、分かれたところ）。
fn merge_rows(d: &Value) -> usize {
    let rows = |k: &str, more: &str| {
        d[k].as_array().map_or(0, Vec::len) + usize::from(d[more].as_u64().unwrap_or(0) > 0)
    };
    rows("ours", "oursMore") + rows("theirs", "theirsMore") + usize::from(!d["base"].is_null())
}

/// 何で作るか（見出しと、件名の書き方が変わる。GTK4 版の `rewritedraw::PickKind`）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PickKind {
    CherryPick,
    Revert,
    Patches,
}

/// 確認に添える絵の見せ方（`ConfirmDrawn` に持たせる）。
pub enum How {
    /// ref を動かす（`movePreview` の答え）。
    Move(Move),
    /// 今のブランチの上にコミットを作る（`pickPreview` の答え）。`current` は今のブランチ。
    Pick { current: String, kind: PickKind },
}

/// 答えと見せ方から絵を作る。描くものがなければ None。
pub fn for_how(info: &Value, how: &How) -> Option<View> {
    match how {
        How::Move(how) => for_move(info, how),
        How::Pick { current, kind } => for_pick(info, current, *kind),
    }
}

/// cherry-pick・revert・パッチ（git am）の見込み（`pickPreview` の答え）の絵（GTK4 版の `draw_pick`）。左が今の
/// 先端、右が後: その上に、これから作るコミット（点線の丸。cherry-pick は件名に ′）が並び、今のブランチの札が
/// 一番上へ移る。描くものがなければ None。
pub fn for_pick(info: &Value, current: &str, kind: PickKind) -> Option<View> {
    let d = &info["drawing"];
    if d.is_null() {
        return None;
    }
    let labels = labels_of(d);
    let head = commits(&d["head"]);
    let picks = commits(&d["picks"]);
    let picks_more = d["picksMore"].as_u64().unwrap_or(0);
    let tip = head.first().map(|c| c.0.clone());
    let first_new = picks.first().map(|c| format!("new:{}", c.0));
    let mut c = Canvas2 { items: Vec::new() };
    c.text(&tr!("Now"), 4.0, 2.0, alpha(FG, 0.6), true, COLUMN, 11.0);
    c.text(
        &match kind {
            PickKind::CherryPick => tr!("After the cherry-pick"),
            PickKind::Revert => tr!("After the revert"),
            PickKind::Patches => tr!("After applying the patches"),
        },
        COLUMN + 28.0,
        2.0,
        alpha(FG, 0.6),
        true,
        COLUMN,
        11.0,
    );
    for after in [false, true] {
        let x = if after { COLUMN + 24.0 } else { 0.0 };
        let mut y = TOP;
        let mut rows = Vec::new();
        let mut gaps = Vec::new();
        if after {
            with_gap(
                &picks,
                picks_more,
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
            &mut c,
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
    let picks = picks.len() + usize::from(picks_more > 0);
    Some(finish(c, picks + head.len(), picks))
}

/// 線は 4 本まで、残りは「ほか n 本」（GTK4 版・macOS 版の `maxLanes`）。
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

/// octopus merge の見込み（`octopusPreview` の答え）の絵（GTK4 版の `draw_octopus`）。`order` は取り込む順、
/// `subject` は merge コミットの件名。左が今: 今のブランチ（左の線）と、取り込むもの（並べた順に右の線。先端と、
/// ほかにいくつあるか）が、すべてが分かれたところで合流する。右が後: 一番上の merge コミット（点線の丸）が
/// すべての先端を親に持ち、今のブランチの札がそこへ移る。描くものがなければ None。
pub fn for_octopus(info: &Value, current: &str, order: &[String], subject: &str) -> Option<View> {
    let d = &info["drawing"];
    if d.is_null() {
        return None;
    }
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
    let mut c = Canvas2 { items: Vec::new() };
    c.text(&tr!("Now"), 4.0, 2.0, alpha(FG, 0.6), true, COLUMN, 11.0);
    c.text(
        &tr!("After the merge"),
        COLUMN + 28.0,
        2.0,
        alpha(FG, 0.6),
        true,
        COLUMN,
        11.0,
    );
    for after in [false, true] {
        let x = if after { COLUMN + 24.0 } else { 0.0 };
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
            c.text(
                &tr!("and {count} more branches", count = rest),
                text_x,
                y + 2.0,
                alpha(FG, 0.5),
                false,
                x + COLUMN - text_x,
                11.0,
            );
            y += ROW;
        }
        column(
            &mut c,
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
    let rows = octopus_rows(d, order);
    // 右には merge コミットの 1 行が増える。
    Some(finish(c, rows + 1, rows))
}
