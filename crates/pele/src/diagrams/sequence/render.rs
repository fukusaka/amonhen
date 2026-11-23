//! sequence を描く（pele の `src/diagrams/sequence/render.ts`）。

use std::collections::HashMap;

use super::actors::{ACTOR_PAD_X, ActorSize, actor_kind, actor_size, draw_actor, is_figure};
use super::db::SeqDb;
use super::types::{SeqActor, SeqMessage, line, placement};
use crate::diagrams::common::fit_width::{tighten, title_room};
use crate::diagrams::common::literal_label::break_label;
use crate::errors::Result;
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num};
use crate::svg::edges::{ARROW, marker};
use crate::svg::ink::ink;
use crate::svg::root::{Accessible, svg_document};
use crate::svg::theme::{FONT_MONO, ROUNDED, class_names, resolve_style, rounded};
use crate::svg::title::title_label;
use crate::text::label::{Label, layout_label};
use crate::text::measurer::{TextMeasurer, default_measurer, style};
use crate::types::{IconResolver, RenderOptions, Rendered};
use crate::util::yaml::Value;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Head {
    None,
    Arrow,
    Cross,
    Open,
    HalfSolid,
    HalfStick,
}

#[derive(Clone, Copy)]
struct LineStyle {
    dotted: bool,
    head: Head,
    /// 矢じりの場所: 受け手に、送り手だけに（逆向きの矢）、両方に。
    at_start: bool,
    at_end: bool,
    bottom: bool,
}

fn line_style(type_: i32) -> Option<LineStyle> {
    let make = |dotted: bool, head: Head, ends: i32, bottom: bool| LineStyle {
        dotted,
        head,
        at_start: ends != 1,
        at_end: ends != 0,
        bottom,
    };
    Some(match type_ {
        line::SOLID => make(false, Head::Arrow, 1, false),
        line::DOTTED => make(true, Head::Arrow, 1, false),
        line::SOLID_CROSS => make(false, Head::Cross, 1, false),
        line::DOTTED_CROSS => make(true, Head::Cross, 1, false),
        line::SOLID_OPEN => make(false, Head::None, 1, false),
        line::DOTTED_OPEN => make(true, Head::None, 1, false),
        line::SOLID_POINT => make(false, Head::Open, 1, false),
        line::DOTTED_POINT => make(true, Head::Open, 1, false),
        line::BIDIRECTIONAL_SOLID => make(false, Head::Arrow, 2, false),
        line::BIDIRECTIONAL_DOTTED => make(true, Head::Arrow, 2, false),
        _ => {
            let (dotted, base) = if (51..=58).contains(&type_) {
                (true, type_ - 10)
            } else {
                (false, type_)
            };
            match base {
                line::SOLID_TOP => make(dotted, Head::HalfSolid, 1, false),
                line::SOLID_BOTTOM => make(dotted, Head::HalfSolid, 1, true),
                line::STICK_TOP => make(dotted, Head::HalfStick, 1, false),
                line::STICK_BOTTOM => make(dotted, Head::HalfStick, 1, true),
                line::SOLID_ARROW_TOP_REVERSE => make(dotted, Head::HalfSolid, 0, false),
                line::SOLID_ARROW_BOTTOM_REVERSE => make(dotted, Head::HalfSolid, 0, true),
                line::STICK_ARROW_TOP_REVERSE => make(dotted, Head::HalfStick, 0, false),
                line::STICK_ARROW_BOTTOM_REVERSE => make(dotted, Head::HalfStick, 0, true),
                _ => return None,
            }
        }
    })
}

fn frame_kind(type_: i32) -> Option<&'static str> {
    Some(match type_ {
        line::LOOP_START => "loop",
        line::ALT_START => "alt",
        line::OPT_START => "opt",
        line::PAR_START | line::PAR_OVER_START => "par",
        line::CRITICAL_START => "critical",
        line::BREAK_START => "break",
        line::RECT_START => "rect",
        _ => return None,
    })
}

const MSG_PAD: f64 = 12.0;
const SELF_W: f64 = 28.0;
const SELF_H: f64 = 22.0;
const ACT_W: f64 = 10.0;
const ACT_STEP: f64 = 4.0;
const ACT_MIN: f64 = 14.0;
const NOTE_GAP: f64 = 10.0;
const NOTE_OVER: f64 = 16.0;
const FRAME_PAD: f64 = 20.0;
const FRAME_NEST: f64 = 10.0;
const TAB_H: f64 = 20.0;
const FRAME_TEXT: f64 = 240.0;
const BADGE_H: f64 = 16.0;
const CENTRAL_R: f64 = 4.0;
const DASH: &str = " stroke-dasharray=\"4 4\"";

pub struct Numbering {
    /// 線のメッセージそれぞれの番号（見せるかどうかにかかわらず）。ほかは NaN。
    pub index: Vec<f64>,
    pub shown: Vec<bool>,
    /// 最後の文の後に番号が見えるか。
    pub visible: bool,
}

pub fn is_line_message(type_: Option<i32>) -> bool {
    type_.is_some_and(|t| line_style(t).is_some())
}

/// Mermaid の描き手が描きながら数えるとおりに、線のメッセージを数える。
pub fn number_messages(messages: &[SeqMessage], always: bool) -> Numbering {
    let mut index = vec![f64::NAN; messages.len()];
    let mut shown = vec![false; messages.len()];
    let mut current = 1.0;
    let mut step = 1.0;
    let mut visible = false;
    for (i, m) in messages.iter().enumerate() {
        if m.type_ == Some(line::AUTONUMBER) {
            let spec = m.numbering.expect("autonumber");
            current = spec.start.filter(|s| js::truthy(*s)).unwrap_or(current);
            step = spec.step.filter(|s| js::truthy(*s)).unwrap_or(step);
            visible = spec.visible;
        } else if is_line_message(m.type_) {
            index[i] = current;
            shown[i] = visible || always;
            current = js::round((current + step) * 100.0) / 100.0;
        }
    }
    Numbering {
        index,
        shown,
        visible,
    }
}

struct Column<'a> {
    id: &'a str,
    actor: &'a SeqActor,
    kind: &'static str,
    label: Label,
    size: ActorSize,
    x: f64,
    create_at: i64,
    destroy_at: i64,
    top: f64,
    life_start: f64,
    life_end: f64,
    activations: Vec<f64>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum V {
    Line,
    Self_,
    Note,
}

struct View {
    kind: V,
    a: usize,
    b: usize,
    label: Label,
    x: f64,
    w: f64,
}

struct Frame {
    kind: &'static str,
    x0: f64,
    x1: f64,
    top: f64,
    title: Label,
    tab_w: f64,
    fill: String,
    dividers: Vec<usize>,
    divider_y: Vec<f64>,
    divider_labels: Vec<Label>,
}

struct BoxRun {
    box_: usize,
    first: usize,
    last: usize,
    title: Label,
    grow: f64,
}

fn option(conf: Option<&crate::util::yaml::Map>, key: &str, fallback: f64) -> f64 {
    match conf.and_then(|c| c.get(key)) {
        Some(Value::Num(v)) if *v >= 0.0 && *v <= 2000.0 => *v,
        _ => fallback,
    }
}

fn half(x: f64, y: f64, dir: f64, up: f64, stick: bool, color: &str) -> String {
    let bx = num(x - dir * ARROW);
    if stick {
        return format!(
            "<path class=\"pele-marker\" d=\"M{},{}L{bx},{}\"/>",
            num(x),
            num(y),
            num(y + up * 5.0)
        );
    }
    format!(
        "<path class=\"pele-marker\" d=\"M{},{}L{bx},{}V{}Z\" fill=\"{color}\" stroke=\"none\"/>",
        num(x),
        num(y),
        num(y + up * 4.5),
        num(y)
    )
}

/// 先が (x, y) で dir を向く矢じり。`up` は半分の矢じりの乗る側。
fn head(type_: Head, x: f64, y: f64, dir: f64, up: f64, color: &str) -> String {
    match type_ {
        Head::Arrow => marker("arrow_point", x, y, dir, 0.0, color, "none"),
        Head::Cross => marker("arrow_cross", x, y, dir, 0.0, color, "none"),
        Head::Open => format!(
            "<path class=\"pele-marker\" d=\"M{},{}L{},{}L{},{}\" stroke-linejoin=\"round\"/>",
            num(x - dir * ARROW),
            num(y - 4.5),
            num(x),
            num(y),
            num(x - dir * ARROW),
            num(y + 4.5)
        ),
        Head::HalfSolid => half(x, y, dir, up, false, color),
        Head::HalfStick => half(x, y, dir, up, true, color),
        Head::None => String::new(),
    }
}

fn trim(type_: Head) -> f64 {
    match type_ {
        Head::Arrow => ARROW - 1.0,
        Head::Cross => 4.0,
        _ => 0.0,
    }
}

/// `messages` の文字を等幅で測る。
struct Mono<'a>(&'a dyn TextMeasurer);

impl TextMeasurer for Mono<'_> {
    fn width(&self, text: &str, size: f64, st: u32) -> f64 {
        self.0.width(text, size, st | style::MONO)
    }
}

pub fn render_sequence(db: &SeqDb, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    tighten(options, |tight| {
        Ok(draw(db, config, options, tight, 0.0, true))
    })
}

/// 文字を組む決まり（TS の `text` と `messageText`）。
struct Texts<'a> {
    measurer: &'a dyn TextMeasurer,
    messages: &'a dyn TextMeasurer,
    tight: bool,
    small: f64,
}

impl Texts<'_> {
    fn text_by(
        &self,
        raw: Option<&str>,
        font_size: f64,
        wrap: bool,
        width: f64,
        by: &dyn TextMeasurer,
    ) -> Label {
        let label = layout_label(
            raw,
            false,
            by,
            font_size,
            if self.tight || wrap {
                width
            } else {
                f64::INFINITY
            },
            0,
        );
        if self.tight {
            break_label(&label, by, js::max(120.0, width))
        } else {
            label
        }
    }

    fn text(&self, raw: Option<&str>, font_size: f64, wrap: bool, width: f64) -> Label {
        self.text_by(raw, font_size, wrap, width, self.measurer)
    }

    fn message(&self, raw: &str, wrap: bool, width: f64) -> Label {
        self.text_by(Some(raw), self.small, wrap, width, self.messages)
    }
}

/// 横の配置（TS の `layout` とそれが使うもの）。
struct Board<'a> {
    texts: &'a Texts<'a>,
    messages: &'a [SeqMessage],
    numbering: &'a Numbering,
    cols: Vec<Column<'a>>,
    gaps: Vec<f64>,
    runs: Vec<BoxRun>,
    views: Vec<Option<View>>,
    frames: Vec<Frame>,
    frame_at: Vec<Option<usize>>,
    min_x: f64,
    max_x: f64,
    sized: HashMap<(usize, u64), Label>,
    tight: bool,
    wrap_w: f64,
    note_pad: f64,
    note_over: f64,
    msg_pad: f64,
    frame_pad: f64,
    frame_nest: f64,
    box_pad: f64,
    small: f64,
    tiny: f64,
}

impl Board<'_> {
    fn badge_width(&self, i: usize) -> f64 {
        if self.numbering.shown[i] {
            js::max(
                BADGE_H,
                self.texts
                    .measurer
                    .width(&js::fmt(self.numbering.index[i]), self.tiny, 0)
                    + 8.0,
            )
        } else {
            0.0
        }
    }

    fn sized_text(&mut self, i: usize, room: f64) -> Label {
        if let Some(label) = self.sized.get(&(i, room.to_bits())) {
            return label.clone();
        }
        let label = self.texts.message(&self.messages[i].message, true, room);
        self.sized.insert((i, room.to_bits()), label.clone());
        label
    }

    fn place(&mut self) {
        for i in 1..self.cols.len() {
            self.cols[i].x = self.cols[i - 1].x + self.gaps[i - 1];
        }
    }

    fn extend(&mut self, x0: f64, x1: f64) {
        if x0 < self.min_x {
            self.min_x = x0;
        }
        if x1 > self.max_x {
            self.max_x = x1;
        }
    }

    fn box_edges(&self, run: &BoxRun) -> (f64, f64) {
        let first = &self.cols[run.first];
        let last = &self.cols[run.last];
        (
            first.x - first.size.w / 2.0 - self.box_pad - run.grow,
            last.x + last.size.w / 2.0 + self.box_pad + run.grow,
        )
    }

    fn span_all(&self) -> (f64, f64) {
        match self.cols.last() {
            None => (0.0, 120.0),
            Some(last) => (self.cols[0].x - self.frame_pad, last.x + self.frame_pad),
        }
    }

    fn layout(&mut self) {
        self.place();
        self.min_x = f64::INFINITY;
        self.max_x = f64::NEG_INFINITY;
        for k in 0..self.cols.len() {
            let col = &self.cols[k];
            let (x0, x1) = (col.x - col.size.w / 2.0, col.x + col.size.w / 2.0);
            self.extend(x0, x1);
        }
        for k in 0..self.runs.len() {
            let (x0, x1) = self.box_edges(&self.runs[k]);
            self.extend(x0, x1);
        }

        // 囲みそれぞれの横の広がりを、中身から。
        self.frames.clear();
        self.frame_at = vec![None; self.messages.len()];
        let mut open: Vec<usize> = Vec::new();
        let tiny = self.tiny;
        let small = self.small;
        for i in 0..self.messages.len() {
            let m = &self.messages[i];
            let type_ = m.type_;
            let kind = type_.and_then(frame_kind);
            if let Some(kind) = kind {
                self.frames.push(Frame {
                    kind,
                    x0: f64::INFINITY,
                    x1: f64::NEG_INFINITY,
                    top: 0.0,
                    title: Label::empty(),
                    tab_w: 0.0,
                    fill: if kind == "rect" && m.numbering.is_none() {
                        m.message.clone()
                    } else {
                        String::new()
                    },
                    dividers: vec![i],
                    divider_y: Vec::new(),
                    divider_labels: Vec::new(),
                });
                let f = self.frames.len() - 1;
                self.frame_at[i] = Some(f);
                open.push(f);
            } else if matches!(
                type_,
                Some(line::ALT_ELSE | line::PAR_AND | line::CRITICAL_OPTION)
            ) {
                if let Some(&f) = open.last() {
                    self.frames[f].dividers.push(i);
                }
            } else if matches!(
                type_,
                Some(
                    line::LOOP_END
                        | line::ALT_END
                        | line::OPT_END
                        | line::PAR_END
                        | line::CRITICAL_END
                        | line::BREAK_END
                        | line::RECT_END
                )
            ) {
                let Some(f) = open.pop() else {
                    continue;
                };
                self.frame_at[i] = Some(f);
                if self.frames[f].x0 > self.frames[f].x1 {
                    let (x0, x1) = self.span_all();
                    self.frames[f].x0 = x0;
                    self.frames[f].x1 = x1;
                }
                if self.frames[f].kind != "rect" {
                    let tab_w = self
                        .texts
                        .measurer
                        .width(self.frames[f].kind, tiny, style::BOLD)
                        + 14.0;
                    self.frames[f].tab_w = tab_w;
                    // 囲みは右へ広がり、文が折り返す前に FRAME_TEXT までの幅を与える。
                    for k in 0..self.frames[f].dividers.len() {
                        let raw = &self.messages[self.frames[f].dividers[k]].message;
                        let tab = if k == 0 { tab_w + 8.0 } else { 0.0 };
                        let frame = &self.frames[f];
                        let room = frame.x1 - frame.x0 - 16.0 - tab;
                        let label = if raw.is_empty() {
                            Label::empty()
                        } else {
                            self.texts.text(
                                Some(&format!("[{raw}]")),
                                small,
                                true,
                                js::max(
                                    room,
                                    if self.tight {
                                        js::min(FRAME_TEXT, self.wrap_w)
                                    } else {
                                        FRAME_TEXT
                                    },
                                ),
                            )
                        };
                        let frame = &mut self.frames[f];
                        if label.width > room {
                            frame.x1 += label.width - room;
                        }
                        if k == 0 {
                            frame.title = label;
                        } else {
                            frame.divider_labels.push(label);
                        }
                    }
                }
                let (x0, x1) = (self.frames[f].x0, self.frames[f].x1);
                self.extend(x0, x1);
                if let Some(&parent) = open.last() {
                    let nest = self.frame_nest;
                    let parent = &mut self.frames[parent];
                    parent.x0 = js::min(parent.x0, x0 - nest);
                    parent.x1 = js::max(parent.x1, x1 + nest);
                }
            } else if let Some(view) = &self.views[i] {
                let (kind, va, vb) = (view.kind, view.a, view.b);
                let (ax, bx) = (self.cols[va].x, self.cols[vb].x);
                let x0;
                let mut x1;
                if kind == V::Note {
                    let vw = view.w;
                    let mut new_label = None;
                    let (vx, vw) = if m.placement == Some(placement::LEFTOF) {
                        (ax - NOTE_GAP - vw, vw)
                    } else if m.placement == Some(placement::RIGHTOF) {
                        (ax + NOTE_GAP, vw)
                    } else if va == vb {
                        (ax - vw / 2.0, vw)
                    } else {
                        let lo = js::min(ax, bx) - self.note_over;
                        let hi = js::max(ax, bx) + self.note_over;
                        let mut width = view.label.width;
                        if self.tight || m.wrap {
                            let label = self.texts.text(
                                Some(&m.message),
                                small,
                                true,
                                js::max(self.wrap_w, hi - lo - 2.0 * self.note_pad),
                            );
                            width = label.width;
                            new_label = Some(label);
                        }
                        let vw = js::max(hi - lo, width + 2.0 * self.note_pad);
                        ((lo + hi - vw) / 2.0, vw)
                    };
                    let view = self.views[i].as_mut().expect("view");
                    view.x = vx;
                    view.w = vw;
                    if let Some(label) = new_label {
                        view.label = label;
                    }
                    x0 = vx;
                    x1 = vx + vw;
                } else if kind == V::Self_ {
                    x0 = ax;
                    x1 = ax + ACT_W + SELF_W + 8.0 + view.label.width + self.badge_width(i);
                } else {
                    x0 = js::min(ax, bx);
                    x1 = js::max(ax, bx);
                    let mut lo = x0;
                    if self.tight || m.wrap {
                        let label = self.sized_text(
                            i,
                            js::max(self.wrap_w, x1 - x0 - 2.0 * self.msg_pad - ACT_W),
                        );
                        self.views[i].as_mut().expect("view").label = label;
                    }
                    let b = &self.cols[vb];
                    if b.create_at == i as i64 {
                        if bx > ax {
                            x1 += b.size.w / 2.0;
                        } else {
                            lo -= b.size.w / 2.0;
                        }
                    }
                    self.extend_frame(&open, lo, x1);
                    continue;
                }
                self.extend_frame(&open, x0, x1);
            }
        }
    }

    /// 広がりを図と、いちばん内の囲みに足す。
    fn extend_frame(&mut self, open: &[usize], x0: f64, x1: f64) {
        self.extend(x0, x1);
        if let Some(&f) = open.last() {
            let pad = self.frame_pad;
            let frame = &mut self.frames[f];
            frame.x0 = js::min(frame.x0, x0 - pad);
            frame.x1 = js::max(frame.x1, x1 + pad);
        }
    }

    fn view_lines(&self, wrapped: &[usize]) -> Vec<usize> {
        wrapped
            .iter()
            .map(|&i| self.views[i].as_ref().expect("view").label.lines.len())
            .collect()
    }
}

/// 縦に配置しながら描いたもの。
#[derive(Default)]
struct Out {
    rects: Vec<String>,
    activations: Vec<String>,
    frames: String,
    edges: String,
    labels: String,
    notes: String,
    actors: String,
    crosses: String,
}

fn draw(
    db: &SeqDb,
    config: &Config,
    options: &RenderOptions,
    tight: bool,
    hold: f64,
    by_need: bool,
) -> Rendered {
    let conf = config.get("sequence").and_then(Value::as_obj);
    let get = |key: &str| conf.and_then(|c| c.get(key));
    let size = options.font_size.unwrap_or(16.0);
    let small = js::round(size * 0.875);
    let tiny = js::round(size * 0.75);
    let measurer: &dyn TextMeasurer = match &options.measurer {
        Some(m) => m.as_ref(),
        None => default_measurer(),
    };
    let monospaced = options
        .monospace
        .as_ref()
        .is_some_and(|m| m.iter().any(|p| p == "messages"));
    let mono = Mono(measurer);
    let message_measurer: &dyn TextMeasurer = if monospaced { &mono } else { measurer };
    let icons = options.icons.as_ref();
    let pad = options.padding.unwrap_or(8.0);
    let mirror = get("mirrorActors") != Some(&Value::Bool(false));
    let right_angles = get("rightAngles") == Some(&Value::Bool(true));
    let actor_margin = js::min(
        option(conf, "actorMargin", 40.0),
        if tight { 8.0 } else { f64::INFINITY },
    );
    let min_w = js::min(
        option(conf, "width", 64.0),
        if tight { 48.0 } else { f64::INFINITY },
    );
    let min_h = option(conf, "height", js::round(size * 1.5) + 20.0);
    let wrap_w = js::min(
        option(conf, "width", 160.0),
        if tight {
            js::max(
                32.0,
                (options.max_width.unwrap_or(320.0) - 2.0 * pad)
                    / js::max(1.0, db.actors.len() as f64)
                    - actor_margin
                    - 24.0,
            )
        } else {
            f64::INFINITY
        },
    );
    // Mermaid の既定の 35 が pele の既定の間に当たる。ほかの値はその差だけ動かす。
    let msg_gap = js::max(4.0, 14.0 + option(conf, "messageMargin", 35.0) - 35.0);
    let note_pad = js::min(
        option(conf, "noteMargin", 10.0),
        if tight { 6.0 } else { f64::INFINITY },
    );
    let box_pad = js::min(
        option(conf, "boxMargin", 10.0),
        if tight { 6.0 } else { f64::INFINITY },
    );
    let align = match get("messageAlign").and_then(Value::as_str) {
        Some("left") => -1,
        Some("right") => 1,
        _ => 0,
    };

    let msg_pad = if tight { 6.0 } else { MSG_PAD };
    let note_over = if tight { 8.0 } else { NOTE_OVER };
    let frame_pad = if tight { 12.0 } else { FRAME_PAD };
    let frame_nest = if tight { 6.0 } else { FRAME_NEST };
    let messages = &db.messages;
    let count = messages.len();
    let numbering = number_messages(
        messages,
        get("showSequenceNumbers") == Some(&Value::Bool(true)),
    );
    let texts = Texts {
        measurer,
        messages: message_measurer,
        tight,
        small,
    };

    let used: Option<Vec<Option<&str>>> =
        (get("hideUnusedParticipants") == Some(&Value::Bool(true))).then(|| {
            let mut used = Vec::new();
            for m in messages {
                used.push(m.from.as_deref());
                used.push(m.to.as_deref());
            }
            used
        });
    let used: Option<std::collections::HashSet<Option<&str>>> =
        used.map(|u| u.into_iter().collect());

    let mut next_line = vec![-1i64; count + 1];
    for i in (0..count).rev() {
        next_line[i] = if is_line_message(messages[i].type_) {
            i as i64
        } else {
            next_line[i + 1]
        };
    }

    let mut cols: Vec<Column> = Vec::new();
    let mut col_of: HashMap<&str, usize> = HashMap::new();
    for (id, actor) in &db.actors {
        if used
            .as_ref()
            .is_some_and(|u| !u.contains(&Some(id.as_str())))
        {
            continue;
        }
        let kind = actor_kind(&actor.type_);
        let label = texts.text(Some(&actor.description), size, actor.wrap, wrap_w);
        let created = db.created_actors.get(id);
        let destroyed = db.destroyed_actors.get(id);
        let mut create_at = created.map_or(-1, |&c| next_line[c.min(count)]);
        if create_at >= 0 && messages[create_at as usize].to.as_deref() != Some(id) {
            create_at = -1;
        }
        let mut destroy_at = destroyed.map_or(-1, |&d| next_line[d.min(count)]);
        if destroy_at >= 0 {
            let m = &messages[destroy_at as usize];
            if m.to.as_deref() != Some(id) && m.from.as_deref() != Some(id) {
                destroy_at = -1;
            }
        }
        col_of.insert(id, cols.len());
        cols.push(Column {
            id,
            actor,
            kind,
            size: actor_size(
                kind,
                &label,
                min_w,
                min_h,
                if tight { 8.0 } else { ACTOR_PAD_X },
            ),
            label,
            x: 0.0,
            create_at,
            destroy_at,
            top: 0.0,
            life_start: 0.0,
            life_end: -1.0,
            activations: Vec::new(),
        });
    }
    let n_cols = cols.len();
    let last = n_cols as i64 - 1;

    // 生命線の間の幅。メッセージと注はどれも、触れる生命線がどれだけ離れていなければならないかを言う。
    let mut needs: HashMap<i64, f64> = HashMap::new();
    let mut need = |lo: i64, hi: i64, width: f64| {
        if lo < 0 || hi > last {
            return;
        }
        let key = lo * n_cols as i64 + hi;
        let entry = needs.entry(key).or_insert(0.0);
        if *entry < width {
            *entry = width;
        }
    };
    let badge_width = |i: usize| -> f64 {
        if numbering.shown[i] {
            js::max(
                BADGE_H,
                measurer.width(&js::fmt(numbering.index[i]), tiny, 0) + 8.0,
            )
        } else {
            0.0
        }
    };

    let mut views: Vec<Option<View>> = (0..count).map(|_| None).collect();
    for i in 0..count {
        let m = &messages[i];
        let a = m.from.as_deref().and_then(|f| col_of.get(f)).copied();
        let b = m.to.as_deref().and_then(|t| col_of.get(t)).copied();
        let (Some(a), Some(b)) = (a, b) else {
            continue;
        };
        if m.numbering.is_some() {
            continue;
        }
        let (ai, bi) = (a as i64, b as i64);
        if m.type_ == Some(line::NOTE) {
            let label = texts.text(Some(&m.message), small, m.wrap, wrap_w);
            let w = label.width + 2.0 * note_pad;
            if m.placement == Some(placement::LEFTOF) {
                need(ai - 1, ai, w + 2.0 * NOTE_GAP + ACT_W);
            } else if m.placement == Some(placement::RIGHTOF) {
                need(ai, ai + 1, w + 2.0 * NOTE_GAP + ACT_W);
            } else if a == b {
                need(ai - 1, ai, w / 2.0 + NOTE_GAP);
                need(ai, ai + 1, w / 2.0 + NOTE_GAP);
            } else {
                need(ai.min(bi), ai.max(bi), w - 2.0 * note_over);
            }
            views[i] = Some(View {
                kind: V::Note,
                a,
                b,
                label,
                x: 0.0,
                w,
            });
        } else if is_line_message(m.type_) {
            let label = texts.message(&m.message, m.wrap, wrap_w);
            let badge = badge_width(i);
            if a == b {
                need(
                    ai,
                    ai + 1,
                    SELF_W + 8.0 + label.width + 16.0 + ACT_W + badge,
                );
                views[i] = Some(View {
                    kind: V::Self_,
                    a,
                    b,
                    label,
                    x: 0.0,
                    w: 0.0,
                });
            } else {
                let created = if cols[b].create_at == i as i64 {
                    cols[b].size.w / 2.0
                } else {
                    0.0
                };
                let central = if m.central_connection != 0 {
                    4.0 * CENTRAL_R
                } else {
                    0.0
                };
                need(
                    ai.min(bi),
                    ai.max(bi),
                    (if tight {
                        js::min(label.width, js::max(16.0, wrap_w - 16.0))
                    } else {
                        label.width
                    }) + 2.0 * msg_pad
                        + ACT_W
                        + badge
                        + created
                        + central,
                );
                views[i] = Some(View {
                    kind: V::Line,
                    a,
                    b,
                    label,
                    x: 0.0,
                    w: 0.0,
                });
            }
        }
    }

    let mut runs: Vec<BoxRun> = Vec::new();
    for (i, col) in cols.iter().enumerate() {
        let Some(box_) = col.actor.box_ else {
            continue;
        };
        if let Some(run) = runs.last_mut()
            && run.box_ == box_
            && run.last + 1 == i
        {
            run.last = i;
            continue;
        }
        let b = &db.boxes[box_];
        runs.push(BoxRun {
            box_,
            first: i,
            last: i,
            title: texts.text(b.name.as_deref(), small, b.wrap, wrap_w),
            grow: 0.0,
        });
    }

    let mut gaps = vec![0.0; n_cols.saturating_sub(1)];
    for i in 0..gaps.len() {
        let left = &cols[i];
        let right = &cols[i + 1];
        let boxed = if left.actor.box_ != right.actor.box_ {
            (if left.actor.box_.is_some() {
                box_pad
            } else {
                0.0
            }) + if right.actor.box_.is_some() {
                box_pad
            } else {
                0.0
            }
        } else {
            0.0
        };
        gaps[i] = js::max(
            left.size.w / 2.0 + right.size.w / 2.0 + actor_margin + boxed,
            needs
                .get(&(i as i64 * n_cols as i64 + i as i64 + 1))
                .copied()
                .unwrap_or(0.0),
        );
    }
    // いくつもの生命線をまたぐメッセージは、足りない分を等しく分けて、またぐ間それぞれを広げる。
    let mut spans: Vec<(usize, usize, f64)> = Vec::new();
    for (&key, &width) in &needs {
        let lo = (key / n_cols as i64) as usize;
        let hi = (key % n_cols as i64) as usize;
        if hi - lo > 1 {
            spans.push((lo, hi, width));
        }
    }
    spans.sort_by_key(|p| (p.1 - p.0, p.0));
    for (lo, hi, width) in spans {
        let mut total = 0.0;
        for gap in &gaps[lo..hi] {
            total += gap;
        }
        if total >= width {
            continue;
        }
        let share = (width - total) / (hi - lo) as f64;
        for gap in &mut gaps[lo..hi] {
            *gap += share;
        }
    }

    let mut board = Board {
        texts: &texts,
        messages,
        numbering: &numbering,
        cols,
        gaps,
        runs,
        views,
        frames: Vec::new(),
        frame_at: Vec::new(),
        min_x: f64::INFINITY,
        max_x: f64::NEG_INFINITY,
        sized: HashMap::new(),
        tight,
        wrap_w,
        note_pad,
        note_over,
        msg_pad,
        frame_pad,
        frame_nest,
        box_pad,
        small,
        tiny,
    };
    board.place();
    for k in 0..board.runs.len() {
        let run = &board.runs[k];
        let (first, lastc) = (&board.cols[run.first], &board.cols[run.last]);
        let inner = lastc.x + lastc.size.w / 2.0 - (first.x - first.size.w / 2.0);
        let grow = js::max(0.0, (run.title.width - inner) / 2.0);
        let (rf, rl) = (run.first, run.last);
        board.runs[k].grow = grow;
        if grow == 0.0 {
            continue;
        }
        if rf > 0 {
            board.gaps[rf - 1] += grow;
        }
        if (rl as i64) < last {
            board.gaps[rl] += grow;
        }
    }
    board.layout();

    // 間を広げるのは、メッセージの折り返しが減るときだけ。
    let mut grown = false;
    let mut uneven = false;
    if tight && let Some(max_width) = options.max_width {
        let spare = max_width.floor() - (board.max_x - board.min_x + 2.0 * pad).ceil() - hold;
        let mut stretch = vec![0i64; n_cols];
        let mut want = vec![0.0; board.gaps.len()];
        let mut wrapped: Vec<usize> = Vec::new();
        let mut lines = 0usize;
        let mut i = 0;
        while spare > 0.0 && i < board.views.len() {
            let Some(view) = board.views[i].as_ref().filter(|v| v.kind == V::Line) else {
                i += 1;
                continue;
            };
            let natural = texts.message(&messages[i].message, false, f64::INFINITY);
            if view.label.lines.len() <= natural.lines.len() {
                i += 1;
                continue;
            }
            wrapped.push(i);
            lines += view.label.lines.len();
            let lo = view.a.min(view.b);
            let hi = view.a.max(view.b);
            stretch[lo] += 1;
            stretch[hi] -= 1;
            let short = (natural.width
                - (board.cols[hi].x - board.cols[lo].x - 2.0 * msg_pad - ACT_W))
                / (hi - lo) as f64;
            for w in &mut want[lo..hi] {
                *w = js::max(*w, short);
            }
            i += 1;
        }
        let mut active = 0;
        let mut crossed = 0i64;
        for s in stretch.iter_mut().take(board.gaps.len()) {
            active += *s;
            *s = if active > 0 { 1 } else { 0 };
            crossed += *s;
        }
        if crossed > 0 {
            let before = board.gaps.clone();
            let spread = |board: &mut Board, share: &dyn Fn(usize) -> f64| -> Vec<usize> {
                for (g, gap) in board.gaps.iter_mut().enumerate() {
                    *gap = before[g] + share(g);
                }
                board.layout();
                board.view_lines(&wrapped)
            };
            let sum = |counts: &[usize]| counts.iter().sum::<usize>();
            let even = |g: usize| stretch[g] as f64 * spare / crossed as f64;
            let mut counts = spread(&mut board, &even);
            let mut missing = 0.0;
            for w in &want {
                missing += w;
            }
            // 丸めの誤りを見込む。
            let needs = |g: usize| want[g] * (spare - 0.01) / missing;
            // 折り返しを減らすには、少なくとも 1 つの間が等しく分けたより広くなければならない。
            if by_need
                && crossed > 1
                && missing > 0.0
                && (0..board.gaps.len()).any(|g| needs(g) > even(g))
            {
                let first = counts;
                counts = spread(&mut board, &needs);
                uneven =
                    sum(&counts) < sum(&first) && counts.iter().zip(&first).all(|(n, f)| n <= f);
                if !uneven {
                    counts = spread(&mut board, &even);
                }
            }
            grown = sum(&counts) < lines;
            if !grown {
                board.gaps = before;
                board.layout();
            }
        }
    }

    // 縦の配置。上から下へ、描きながら。
    let mut box_title_h: f64 = 0.0;
    for run in &board.runs {
        box_title_h = js::max(box_title_h, run.title.height);
    }
    let box_top = if board.runs.is_empty() {
        0.0
    } else {
        box_pad
            + if box_title_h > 0.0 {
                box_title_h + 4.0
            } else {
                0.0
            }
    };
    let mut row_h: f64 = 0.0;
    for col in &board.cols {
        if col.create_at < 0 {
            row_h = js::max(row_h, col.size.h);
        }
    }
    let head_bottom = box_top + row_h;
    for col in &mut board.cols {
        col.top = head_bottom - col.size.h;
        col.life_start = head_bottom;
    }

    let xs: Vec<f64> = board.cols.iter().map(|c| c.x).collect();
    // 2 つの x の間を生命線が通るか（文字に線が重なる）。
    let crosses = |x0: f64, x1: f64| -> bool {
        let mut lo = 0;
        let mut hi = xs.len();
        while lo < hi {
            let mid = (lo + hi) >> 1;
            if xs[mid] <= x0 {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo < xs.len() && xs[lo] < x1
    };
    let mut stack: Vec<usize> = Vec::new();
    let mut inks: HashMap<String, Option<&'static str>> = HashMap::new();
    let mut ink_of = |fill: Option<&str>| -> Option<&'static str> {
        let fill = fill.filter(|f| !f.is_empty())?;
        *inks
            .entry(fill.to_string())
            .or_insert_with(|| ink(&resolve_style(&[format!("fill:{fill}")]).shape, ""))
    };
    let ink_on = |color: Option<&'static str>| -> String {
        match color {
            Some(c) => format!(" fill=\"{c}\""),
            None => String::new(),
        }
    };
    let mut out = Out::default();

    let mut y = head_bottom + 6.0;
    for i in 0..count {
        let m = &messages[i];
        let type_ = m.type_;
        let frame = board.frame_at.get(i).copied().flatten();

        // 内のものから: いちばん内の rect の塗り、次にそれを含む箱の塗り。
        let fill_under = |board: &Board, stack: &[usize], l: f64, r: f64| -> Option<String> {
            for &f in stack.iter().rev() {
                if board.frames[f].kind == "rect" {
                    return Some(board.frames[f].fill.clone());
                }
            }
            for run in &board.runs {
                let (x0, x1) = board.box_edges(run);
                if l >= x0 && r <= x1 {
                    return Some(db.boxes[run.box_].fill.clone());
                }
            }
            None
        };

        if type_ == Some(line::ACTIVE_START) {
            if let Some(&c) = m.from.as_deref().and_then(|f| col_of.get(f)) {
                board.cols[c].activations.push(y);
            }
        } else if type_ == Some(line::ACTIVE_END) {
            if let Some(&c) = m.from.as_deref().and_then(|f| col_of.get(f))
                && !board.cols[c].activations.is_empty()
            {
                y = js::max(y, end_activation(&mut board, c, y, &mut out));
            }
        } else if let Some(f) = frame.filter(|&f| board.frames[f].dividers[0] == i) {
            stack.push(f);
            let frame = &mut board.frames[f];
            if frame.kind == "rect" {
                frame.top = y + 8.0;
                y = frame.top;
            } else {
                frame.top = y + 12.0;
                y = frame.top + js::max(TAB_H, frame.title.height + 2.0);
            }
        } else if let Some(f) = frame {
            stack.pop();
            let bottom = y + 12.0;
            let fr = &board.frames[f];
            let x = num(fr.x0);
            let w = num(fr.x1 - fr.x0);
            if fr.kind == "rect" {
                let fill = if fr.fill.is_empty() {
                    String::new()
                } else {
                    resolve_style(&[format!("fill:{}", fr.fill)]).shape
                };
                out.rects.push(format!(
                    "<rect class=\"pele-rect\" x=\"{x}\" y=\"{}\" width=\"{w}\" height=\"{}\"{}/>",
                    num(fr.top),
                    num(bottom - fr.top),
                    rounded(&format!(
                        "{}{fill}",
                        if fill.is_empty() {
                            " fill-opacity=\"0.5\""
                        } else {
                            ""
                        }
                    ))
                ));
            } else {
                let mut inner = format!(
                    "<rect x=\"{x}\" y=\"{}\" width=\"{w}\" height=\"{}\" fill=\"none\"{ROUNDED}/>",
                    num(fr.top),
                    num(bottom - fr.top)
                );
                for k in 0..fr.divider_y.len() {
                    let dy = fr.divider_y[k];
                    let label = &fr.divider_labels[k];
                    inner.push_str(&format!(
                        "<path d=\"M{x},{}H{}\"{DASH}/>",
                        num(dy),
                        num(fr.x1)
                    ));
                    inner.push_str(&backed(
                        label,
                        fr.x0 + 8.0 + label.width / 2.0,
                        dy + 2.0 + label.height / 2.0,
                        true,
                        &crosses,
                        &mut |l, r| ink_on(ink_of(fill_under(&board, &stack, l, r).as_deref())),
                        icons,
                    ));
                }
                inner.push_str(&format!(
                    "<rect x=\"{x}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_s)\"{ROUNDED}/><text class=\"pele-frame-kind\" x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-size=\"{}\" font-weight=\"var(--_hw)\" fill=\"var(--_m)\" stroke=\"none\">{}</text>",
                    num(fr.top),
                    num(fr.tab_w),
                    js::fmt(TAB_H),
                    num(fr.x0 + fr.tab_w / 2.0),
                    num(fr.top + TAB_H / 2.0 + tiny * 0.35),
                    js::fmt(tiny),
                    fr.kind
                ));
                inner.push_str(&backed(
                    &fr.title,
                    fr.x0 + fr.tab_w + 8.0 + fr.title.width / 2.0,
                    fr.top + js::max(TAB_H, fr.title.height) / 2.0,
                    true,
                    &crosses,
                    &mut |l, r| ink_on(ink_of(fill_under(&board, &stack, l, r).as_deref())),
                    icons,
                ));
                out.frames.push_str(&format!(
                    "<g class=\"pele-frame pele-frame-{}\">{inner}</g>",
                    fr.kind
                ));
            }
            y = bottom;
        } else if matches!(
            type_,
            Some(line::ALT_ELSE | line::PAR_AND | line::CRITICAL_OPTION)
        ) {
            if let Some(&f) = stack.last() {
                let current = &mut board.frames[f];
                let height = current.divider_labels[current.divider_y.len()].height;
                current.divider_y.push(y + 12.0);
                y += 12.0 + if height > 0.0 { height } else { 0.0 };
            }
        } else if let Some(view) = board.views[i].as_ref().filter(|v| v.kind == V::Note) {
            let a = &board.cols[view.a];
            let h = view.label.height + 2.0 * js::max(4.0, note_pad - 4.0);
            let top = y + 10.0;
            let mut x = view.x;
            if m.placement == Some(placement::RIGHTOF) {
                x += right_edge(a, 0) - a.x;
            } else if m.placement == Some(placement::LEFTOF) {
                x += left_edge(a, 0) - a.x;
            }
            let w = view.w;
            out.notes.push_str(&format!(
                "<g class=\"pele-note\" data-id=\"i{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{ROUNDED}/>{}</g>",
                esc_text(&m.id),
                num(x),
                num(top),
                num(w),
                num(h),
                label_svg(
                    &view.label,
                    x + w / 2.0,
                    top + h / 2.0,
                    " class=\"pele-label\" stroke=\"none\" fill=\"var(--_fg)\"",
                    icons
                )
            ));
            board.extend(x, x + w);
            y = top + h;
        } else if let Some(view) = board.views[i].as_ref() {
            let st = line_style(type_.expect("line")).expect("line style");
            let (va, vb) = (view.a, view.b);
            let label = view.label.clone();
            let is_self = view.kind == V::Self_;
            let bw = badge_width(i);
            // `+` の始める活性化はこのメッセージのものなので、矢はその縁で止まる。
            let mut starting = 0usize;
            for next in &messages[i + 1..] {
                let t = next.type_;
                if t == Some(line::ACTIVE_START) {
                    if next.from == m.to {
                        starting += 1;
                    }
                } else if !matches!(
                    t,
                    Some(
                        line::ACTIVE_END
                            | line::CENTRAL_CONNECTION
                            | line::CENTRAL_CONNECTION_REVERSE
                    )
                ) {
                    break;
                }
            }
            let up = if st.bottom { 1.0 } else { -1.0 };
            let attrs = if st.dotted { DASH } else { "" };
            let cc = m.central_connection;
            let circle_to = cc == line::CENTRAL_CONNECTION || cc == line::CENTRAL_CONNECTION_DUAL;
            let circle_from =
                cc == line::CENTRAL_CONNECTION_REVERSE || cc == line::CENTRAL_CONNECTION_DUAL;
            let mut color = "var(--_l)".to_string();
            let circle = |cx: f64, cy: f64, color: &str| {
                format!(
                    "<circle class=\"pele-central\" cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{color}\" stroke=\"none\"/>",
                    num(cx),
                    num(cy),
                    js::fmt(CENTRAL_R)
                )
            };
            let badge = |cx: f64, cy: f64, w: f64| {
                format!(
                    "<g class=\"pele-sequence-number\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\" fill=\"var(--_l)\" stroke=\"none\"/><text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-size=\"{}\" fill=\"var(--_bg)\" stroke=\"none\">{}</text></g>",
                    num(cx - w / 2.0),
                    num(cy - BADGE_H / 2.0),
                    num(w),
                    js::fmt(BADGE_H),
                    js::fmt(BADGE_H / 2.0),
                    num(cx),
                    num(cy + tiny * 0.35),
                    js::fmt(tiny),
                    esc(&js::fmt(numbering.index[i]))
                )
            };
            let tail_at_end = st.at_start && !st.at_end;
            let head_trim = trim(st.head);
            let mut inner = String::new();
            let line_y: f64;
            let line: Option<&'static str>;

            if is_self {
                let a = &board.cols[va];
                let top = y + msg_gap + js::max(0.0, (label.height - SELF_H) / 2.0);
                let bottom = top + SELF_H;
                let mut x0 = if circle_from {
                    a.x + CENTRAL_R
                } else {
                    right_edge(a, 0)
                };
                let mut x1 = if circle_to {
                    a.x + CENTRAL_R
                } else {
                    right_edge(a, starting)
                };
                let ax = a.x;
                let out_x = js::max(x0, x1) + SELF_W + bw / 2.0;
                line = ink_of(fill_under(&board, &stack, ax, out_x).as_deref());
                if let Some(l) = line {
                    color = l.to_string();
                }
                if circle_from {
                    inner.push_str(&circle(ax, top, &color));
                }
                if circle_to {
                    inner.push_str(&circle(ax, bottom, &color));
                }
                if bw > 0.0 {
                    inner.push_str(&badge(
                        if tail_at_end { x1 } else { x0 },
                        if tail_at_end { bottom } else { top },
                        bw,
                    ));
                    if tail_at_end {
                        x1 += bw / 2.0;
                    } else {
                        x0 += bw / 2.0;
                    }
                }
                // 輪は右へ出て左へ戻るので、半分の矢じりは裏返る。
                let heads = (if st.at_start {
                    head(st.head, x0, top, -1.0, up, &color)
                } else {
                    String::new()
                }) + &if st.at_end {
                    head(st.head, x1, bottom, -1.0, -up, &color)
                } else {
                    String::new()
                };
                let from = num(x0 + if st.at_start { head_trim } else { 0.0 });
                let to = num(x1 + if st.at_end { head_trim } else { 0.0 });
                let d = if right_angles {
                    format!("M{from},{}H{}V{}H{to}", num(top), num(out_x), num(bottom))
                } else {
                    format!(
                        "M{from},{}H{}A6,6 0 0 1 {},{}V{}A6,6 0 0 1 {},{}H{to}",
                        num(top),
                        num(out_x - 6.0),
                        num(out_x),
                        num(top + 6.0),
                        num(bottom - 6.0),
                        num(out_x - 6.0),
                        num(bottom)
                    )
                };
                inner = format!("<path d=\"{d}\"{attrs}/>{heads}{inner}");
                board.extend(ax, out_x + 8.0 + label.width);
                if label.width > 0.0 {
                    let fill = ink_on(ink_of(
                        fill_under(&board, &stack, out_x + 8.0, out_x + 8.0 + label.width)
                            .as_deref(),
                    ));
                    out.labels.push_str(&format!(
                        "<g class=\"pele-edge-label\" data-id=\"i{}\">{}</g>",
                        esc_text(&m.id),
                        label_svg(
                            &label,
                            out_x + 8.0 + label.width / 2.0,
                            (top + bottom) / 2.0,
                            &fill,
                            icons
                        )
                    ));
                }
                // ラベルが輪より高いと下にはみ出るので、次の行はラベルの下から。
                line_y = js::max(bottom, (top + bottom) / 2.0 + label.height / 2.0);
            } else {
                let (a, b) = (&board.cols[va], &board.cols[vb]);
                let (ax, bx) = (a.x, b.x);
                let dir = if bx > ax { 1.0 } else { -1.0 };
                let creating = b.create_at == i as i64;
                line =
                    ink_of(fill_under(&board, &stack, js::min(ax, bx), js::max(ax, bx)).as_deref());
                if let Some(l) = line {
                    color = l.to_string();
                }
                let b_size = b.size;
                let b_figure = is_figure(b.kind);
                line_y = y
                    + msg_gap
                    + js::max(
                        if label.height > 0.0 {
                            label.height + 2.0
                        } else {
                            6.0
                        },
                        if creating {
                            b_size.h / 2.0 - msg_gap + 6.0
                        } else {
                            0.0
                        },
                    );
                let mut x0 = if dir > 0.0 {
                    right_edge(a, 0)
                } else {
                    left_edge(a, 0)
                };
                let mut x1 = if dir > 0.0 {
                    left_edge(b, starting)
                } else {
                    right_edge(b, starting)
                };
                if creating {
                    let b = &mut board.cols[vb];
                    b.top = line_y - b_size.h / 2.0;
                    b.life_start = b.top + b_size.h;
                    x1 = bx - dir * if b_figure { 18.0 } else { b_size.w / 2.0 + 1.0 };
                }
                if circle_from {
                    inner.push_str(&circle(ax, line_y, &color));
                    x0 = ax + dir * CENTRAL_R;
                }
                if circle_to {
                    inner.push_str(&circle(bx, line_y, &color));
                    x1 = bx - dir * CENTRAL_R;
                }
                if bw > 0.0 {
                    // 番号は矢の尾の生命線の上、つなぎの丸があればその先に置く。
                    if tail_at_end {
                        let cx = if circle_to {
                            x1 - dir * (bw / 2.0 + 2.0)
                        } else {
                            x1
                        };
                        inner.push_str(&badge(cx, line_y, bw));
                        x1 = cx - (dir * bw) / 2.0;
                    } else {
                        let cx = if circle_from {
                            x0 + dir * (bw / 2.0 + 2.0)
                        } else {
                            x0
                        };
                        inner.push_str(&badge(cx, line_y, bw));
                        x0 = cx + (dir * bw) / 2.0;
                    }
                }
                let heads = (if st.at_start {
                    head(st.head, x0, line_y, -dir, up * dir, &color)
                } else {
                    String::new()
                }) + &if st.at_end {
                    head(st.head, x1, line_y, dir, up * dir, &color)
                } else {
                    String::new()
                };
                let from = x0 + if st.at_start { dir * head_trim } else { 0.0 };
                let to = x1 - if st.at_end { dir * head_trim } else { 0.0 };
                inner = format!(
                    "<path d=\"M{},{}H{}\"{attrs}/>{heads}{inner}",
                    num(from),
                    num(line_y),
                    num(to)
                );
                if label.width > 0.0 {
                    let l = js::min(x0, x1);
                    let r = js::max(x0, x1);
                    let cx = if align < 0 {
                        l + msg_pad + label.width / 2.0
                    } else if align > 0 {
                        r - msg_pad - label.width / 2.0
                    } else {
                        (l + r) / 2.0
                    };
                    board.extend(cx - label.width / 2.0, cx + label.width / 2.0);
                    let text = backed(
                        &label,
                        cx,
                        line_y - 4.0 - label.height / 2.0,
                        false,
                        &crosses,
                        &mut |l, r| ink_on(ink_of(fill_under(&board, &stack, l, r).as_deref())),
                        icons,
                    );
                    out.labels.push_str(&format!(
                        "<g class=\"pele-edge-label\" data-id=\"i{}\">{text}</g>",
                        esc_text(&m.id)
                    ));
                }
                if creating {
                    y = line_y + b_size.h / 2.0 - 6.0;
                }
            }
            out.edges.push_str(&format!(
                "<g class=\"pele-edge pele-message\" data-id=\"i{}\"{}>{inner}</g>",
                esc_text(&m.id),
                match line {
                    Some(l) => format!(" stroke=\"{l}\""),
                    None => String::new(),
                }
            ));
            y = js::max(y, line_y);
            if board.cols[va].destroy_at == i as i64 {
                y = js::max(y, destroy(&mut board.cols[va], line_y, &mut out));
            }
            if board.cols[vb].destroy_at == i as i64 && vb != va {
                y = js::max(y, destroy(&mut board.cols[vb], line_y, &mut out));
            }
        }
    }

    y += 20.0;
    for c in 0..board.cols.len() {
        while !board.cols[c].activations.is_empty() {
            end_activation(&mut board, c, y, &mut out);
        }
    }
    let mut bottom = y;
    if mirror {
        let mut foot_h: f64 = 0.0;
        for col in &board.cols {
            if col.life_end >= 0.0 {
                continue;
            }
            foot_h = js::max(foot_h, col.size.h);
            out.actors.push_str(&actor_svg(
                col,
                y,
                true,
                icons,
                &ink_on(ink_of(col.actor.box_.map(|b| db.boxes[b].fill.as_str()))),
            ));
        }
        bottom = y + foot_h;
    }
    let mut lifelines = String::new();
    for col in &board.cols {
        out.actors.push_str(&actor_svg(
            col,
            col.top,
            false,
            icons,
            &ink_on(ink_of(col.actor.box_.map(|b| db.boxes[b].fill.as_str()))),
        ));
        let end = if col.life_end >= 0.0 { col.life_end } else { y };
        if end > col.life_start {
            lifelines.push_str(&format!(
                "<path class=\"pele-lifeline\" data-id=\"{}\" d=\"M{},{}V{}\"/>",
                esc_text(col.id),
                num(col.x),
                num(col.life_start),
                num(end)
            ));
        }
    }

    let mut boxes_out = String::new();
    for run in &board.runs {
        let (x0, x1) = board.box_edges(run);
        let b = &db.boxes[run.box_];
        let fill = if b.fill == "transparent" {
            String::new()
        } else {
            resolve_style(&[format!("fill:{}", b.fill)]).shape
        };
        let ink = ink_on(ink_of(Some(&b.fill)));
        boxes_out.push_str(&format!(
            "<g class=\"pele-cluster pele-box\"{}><rect x=\"{}\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"var(--_a)\" stroke=\"var(--_b)\"{}/>{}</g>",
            match b.name.as_deref().filter(|n| !n.is_empty()) {
                Some(name) => format!(" data-id=\"{}\"", esc_text(name)),
                None => String::new(),
            },
            num(x0),
            num(x1 - x0),
            num(bottom + box_pad),
            rounded(&format!(
                "{}{fill}",
                if fill.is_empty() {
                    " fill-opacity=\"0.5\""
                } else {
                    ""
                }
            )),
            label_svg(
                &run.title,
                (x0 + x1) / 2.0,
                box_pad / 2.0 + 2.0 + run.title.height / 2.0,
                &format!(
                    " class=\"pele-cluster-label\"{}",
                    if ink.is_empty() {
                        " fill=\"var(--_m)\""
                    } else {
                        &ink
                    }
                ),
                icons
            )
        ));
    }
    if !board.runs.is_empty() {
        bottom += box_pad;
    }

    let mut min_y = 0.0;
    let mut title_out = String::new();
    let room = title_room(options);
    let plain_title = title_label(
        db.title.as_deref(),
        measurer,
        size,
        if tight { room } else { f64::INFINITY },
    );
    let title = if tight {
        break_label(&plain_title, measurer, js::max(120.0, room))
    } else {
        plain_title
    };
    if board.min_x > board.max_x {
        board.extend(0.0, js::max(title.width, 40.0));
    }
    if title.width > 0.0 {
        let cx = (board.min_x + board.max_x) / 2.0;
        board.extend(cx - title.width / 2.0, cx + title.width / 2.0);
        min_y = -title.height - 12.0;
        title_out = label_svg(
            &title,
            cx,
            min_y + title.height / 2.0,
            " class=\"pele-title\" font-weight=\"var(--_tw)\"",
            icons,
        );
    }

    let width = (board.max_x - board.min_x + 2.0 * pad).ceil();
    // 等しく分けて、または活性化の注と丸めの場所を空けて、やり直す。
    if let Some(max_width) = options.max_width
        && grown
        && width > max_width
    {
        let hold = if uneven {
            hold
        } else if hold > 0.0 {
            f64::INFINITY
        } else {
            width - max_width.floor()
        };
        return draw(db, config, options, tight, hold, false);
    }
    let height = (bottom - min_y + 2.0 * pad).ceil();
    let mut content = format!(
        "<g transform=\"translate({},{})\">{title_out}",
        num(pad - board.min_x),
        num(pad - min_y)
    );
    if !boxes_out.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-boxes\" font-size=\"{}\">{boxes_out}</g>",
            js::fmt(small)
        ));
    }
    if !out.rects.is_empty() {
        out.rects.reverse();
        content.push_str(&format!(
            "<g class=\"pele-rects\" fill=\"var(--_a)\">{}</g>",
            out.rects.concat()
        ));
    }
    if !lifelines.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-lifelines\" fill=\"none\" stroke=\"var(--_b)\">{lifelines}</g>"
        ));
    }
    if !out.activations.is_empty() {
        out.activations.reverse();
        content.push_str(&format!(
            "<g class=\"pele-activations\" fill=\"var(--_s)\" stroke=\"var(--_b)\">{}</g>",
            out.activations.concat()
        ));
    }
    if !out.frames.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-frames\" font-size=\"{}\" stroke=\"var(--_b)\">{}</g>",
            js::fmt(small),
            out.frames
        ));
    }
    if !out.edges.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-edges\" fill=\"none\" stroke=\"var(--_l)\" stroke-linecap=\"round\">{}</g>",
            out.edges
        ));
    }
    if !out.crosses.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-destroys\" fill=\"none\" stroke=\"var(--_l)\" stroke-width=\"1.5\" stroke-linecap=\"round\">{}</g>",
            out.crosses
        ));
    }
    if !out.labels.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-edge-labels\" font-size=\"{}\"{}>{}</g>",
            js::fmt(small),
            if monospaced {
                format!(" font-family=\"{FONT_MONO}\"")
            } else {
                String::new()
            },
            out.labels
        ));
    }
    if !out.notes.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-notes\" font-size=\"{}\" fill=\"var(--_a)\" stroke=\"var(--_b)\">{}</g>",
            js::fmt(small),
            out.notes
        ));
    }
    if !out.actors.is_empty() {
        content.push_str(&format!("<g class=\"pele-nodes\">{}</g>", out.actors));
    }
    content.push_str("</g>");
    let acc = Accessible {
        acc_title: db.acc_title.as_deref(),
        acc_descr: db.acc_descr.as_deref(),
    };
    Rendered {
        svg: svg_document("sequence", width, height, size, options, &acc, &content),
        width,
        height,
        links: Vec::new(),
    }
}

fn left_edge(col: &Column, extra: usize) -> f64 {
    if col.activations.len() + extra > 0 {
        col.x - ACT_W / 2.0
    } else {
        col.x
    }
}

fn right_edge(col: &Column, extra: usize) -> f64 {
    let depth = col.activations.len() + extra;
    if depth > 0 {
        col.x + ACT_W / 2.0 + (depth - 1) as f64 * ACT_STEP
    } else {
        col.x
    }
}

/// 生命線の上の文字。線が通るなら下地を敷く。
fn backed(
    label: &Label,
    cx: f64,
    cy: f64,
    frame: bool,
    crosses: &dyn Fn(f64, f64) -> bool,
    under: &mut dyn FnMut(f64, f64) -> String,
    icons: Option<&IconResolver>,
) -> String {
    if label.width == 0.0 {
        return String::new();
    }
    let l = cx - label.width / 2.0;
    let plate = crosses(l, l + label.width);
    let back = if plate {
        format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"3\" fill=\"var(--_bg)\" stroke=\"none\"/>",
            num(l - 3.0),
            num(cy - label.height / 2.0 + 2.0),
            num(label.width + 6.0),
            num(label.height - 4.0)
        )
    } else {
        String::new()
    };
    let fill = if plate {
        String::new()
    } else {
        under(l, l + label.width)
    };
    let attrs = if frame {
        format!(
            " class=\"pele-frame-label\"{} stroke=\"none\"",
            if fill.is_empty() {
                " fill=\"var(--_m)\""
            } else {
                &fill
            }
        )
    } else {
        fill
    };
    back + &label_svg(label, cx, cy, &attrs, icons)
}

/// 活性化と影の囲みは内から閉じる。逆に書いて、内のものが上に来るようにする。
fn end_activation(board: &mut Board, c: usize, end: f64, out: &mut Out) -> f64 {
    let col = &mut board.cols[c];
    let start = col.activations.pop().expect("activation");
    let bottom = js::max(end, start + ACT_MIN);
    let x = col.x - ACT_W / 2.0 + col.activations.len() as f64 * ACT_STEP;
    out.activations.push(format!(
        "<rect class=\"pele-activation\" data-id=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"2\"/>",
        esc_text(col.id),
        num(x),
        num(start),
        js::fmt(ACT_W),
        num(bottom - start)
    ));
    board.extend(x, x + ACT_W);
    bottom
}

/// 消える参加者の生命線は、それを終えるメッセージの少し下の×で終わる。
fn destroy(col: &mut Column, line_y: f64, out: &mut Out) -> f64 {
    let cx = col.x;
    let cy = line_y + 16.0;
    col.life_end = cy;
    out.crosses.push_str(&format!(
        "<path class=\"pele-destroy\" data-id=\"{}\" d=\"M{},{}L{},{}M{},{}L{},{}\"/>",
        esc_text(col.id),
        num(cx - 6.0),
        num(cy - 6.0),
        num(cx + 6.0),
        num(cy + 6.0),
        num(cx + 6.0),
        num(cy - 6.0),
        num(cx - 6.0),
        num(cy + 6.0)
    ));
    cy + 6.0
}

fn actor_svg(
    col: &Column,
    top: f64,
    footer: bool,
    icons: Option<&IconResolver>,
    box_ink: &str,
) -> String {
    let cls = match &col.actor.class {
        Some(Value::Str(s)) => class_names(s),
        _ => String::new(),
    };
    format!(
        "<g class=\"pele-node pele-actor pele-actor-{}{}{cls}\" data-id=\"{}\">{}</g>",
        col.kind,
        if footer { " pele-actor-footer" } else { "" },
        esc_text(col.id),
        draw_actor(
            col.kind,
            col.x,
            top,
            col.size,
            &col.label,
            icons,
            if is_figure(col.kind) { box_ink } else { "" }
        )
    )
}
