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
        // 字の幅はおおよそ（10 pt の字で 1 文字 6 px）。
        let width = (name.chars().count() as f64 * 6.0 + 10.0).min(max.max(20.0));
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
                  note: Option<&(String, Option<Color>)>| {
        let r = if highlighted { 5.0 } else { 4.0 };
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
        c.dot((lane_x(lane), center(y)), r, dot, hollow);
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
    let rows = move_rows(d);
    c.text(
        "→",
        COLUMN + 6.0,
        TOP + rows.saturating_sub(1) as f64 * ROW / 2.0,
        alpha(FG, 0.4),
        false,
        20.0,
        15.0,
    );
    let height = rows as f64 * ROW + 26.0;
    Some(
        Canvas::new()
            .width(COLUMN * 2.0 + 24.0)
            .height(height)
            .children((View::keyed_fragment(c.items),)),
    )
}
