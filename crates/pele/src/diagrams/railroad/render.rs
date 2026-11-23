//! railroad を描く（pele の `src/diagrams/railroad/render.ts`）。

use std::collections::HashMap;

use super::common::fold;
use super::types::{RailroadModel, RailroadNode};
use crate::diagrams::common::fit_width::{tighten, title_room};
use crate::errors::Result;
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num};
use crate::svg::root::{Accessible, svg_document};
use crate::svg::theme::{FONT_MONO, ROUNDED};
use crate::text::entities::decode_entities;
use crate::text::label::layout_label;
use crate::text::measurer::{TextMeasurer, default_measurer, style as st};
use crate::types::{RenderOptions, Rendered};
use crate::util::yaml::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Terminal,
    NonTerminal,
    Special,
    Sequence,
    Choice,
    Bypass,
    Loop,
    Folded,
}

/// 並べた要素。線は左の端から入り、1 つの基線の上で右の端から出る。`up` はその上、`down` はその下。
/// 子は `Boxes` の番号。
struct LBox {
    kind: Kind,
    width: f64,
    up: f64,
    down: f64,
    kids: Vec<usize>,
    /// 葉の文字か、輪の回数。
    text: String,
    /// 非終端が指す名前。
    id: String,
    /// 迂回が基線の上にどれだけ上がるか、輪がその下にどれだけ下がるか。
    reach: f64,
}

struct Metrics<'a> {
    measurer: &'a dyn TextMeasurer,
    size: f64,
    padding: f64,
    height: f64,
    /// 並びの要素の間と、積んだ枝の間の隙間。
    gap: f64,
    sep: f64,
    arc: f64,
}

const LEAD: f64 = 12.0;
const HEAD_GAP: f64 = 6.0;
const RULE_GAP: f64 = 18.0;
const COUNT_SIZE: f64 = 12.0;
const ARROW: f64 = 7.0;

fn section(config: &Config) -> Option<&crate::util::yaml::Map> {
    match config.get("railroad") {
        Some(Value::Obj(m)) => Some(m),
        _ => None,
    }
}

fn option(config: &Config, key: &str, fallback: f64, max: f64) -> f64 {
    match section(config).and_then(|s| s.get(key)) {
        Some(Value::Num(v)) if *v >= 0.0 => js::min(*v, max),
        _ => fallback,
    }
}

/// ここの文字は書いたとおり: "<br>" や "*" のような終端はそれ自身で、書式ではない。Mermaid の `#35;` の符号は
/// 文字を表し、見えない逃がしは書き出す。
fn shown(text: &str) -> String {
    let s = if text.contains("¶ß") {
        decode_entities(&text.replace('&', "&amp;"))
    } else {
        text.to_string()
    };
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

fn reverse<T>(list: &mut [T], from: usize) {
    if from < list.len() {
        list[from..].reverse();
    }
}

fn even(value: f64) -> f64 {
    (value / 2.0).ceil() * 2.0
}

fn leaf(kind: Kind, text: String, id: String, style: u32, m: &Metrics) -> LBox {
    let half = m.height / 2.0;
    let width = js::max(
        even(m.measurer.width(&text, m.size, style) + 2.0 * m.padding),
        m.height,
    );
    LBox {
        kind,
        width,
        up: half,
        down: half,
        kids: Vec::new(),
        text,
        id,
        reach: 0.0,
    }
}

fn count(min: f64, max: f64) -> String {
    if max == f64::INFINITY && min <= 1.0 {
        return String::new();
    }
    let text = |v: f64| {
        if v.is_finite() {
            js::fmt(v)
        } else {
            "∞".to_string()
        }
    };
    if min == max {
        return text(min) + "×";
    }
    if max == f64::INFINITY {
        return format!("≥{}×", text(min));
    }
    if min == 0.0 {
        format!("≤{}×", text(max))
    } else {
        format!("{}–{}×", text(min), text(max))
    }
}

/// 選びの次の枝が基線の下のどこに来るか（その上の枝から）。
fn below(offset: f64, above: &LBox, branch: &LBox, m: &Metrics) -> f64 {
    js::max(offset + above.down + m.sep + branch.up, 2.0 * m.arc)
}

fn bypass(kid: usize, boxes: &[LBox], m: &Metrics) -> LBox {
    let k = &boxes[kid];
    let reach = js::max(k.up + m.sep, 2.0 * m.arc);
    LBox {
        kind: Kind::Bypass,
        width: k.width + 4.0 * m.arc,
        up: reach,
        down: k.down,
        kids: vec![kid],
        text: String::new(),
        id: String::new(),
        reach,
    }
}

fn plain(kind: Kind, width: f64, up: f64, down: f64, kids: Vec<usize>) -> LBox {
    LBox {
        kind,
        width,
        up,
        down,
        kids,
        text: String::new(),
        id: String::new(),
        reach: 0.0,
    }
}

/// 規則の定義の箱を `boxes` に組み、根の番号を返す。
fn measure(
    model: &RailroadModel,
    root: usize,
    m: &Metrics,
    room: f64,
    boxes: &mut Vec<LBox>,
) -> usize {
    let nodes = &model.nodes;
    let mut budgets: HashMap<usize, f64> = HashMap::new();
    if room != f64::INFINITY {
        let mut pending: Vec<(usize, f64)> = vec![(root, room)];
        let mut i = 0;
        while i < pending.len() {
            let (node, width) = pending[i];
            budgets.insert(node, width);
            // 折れる並びは行を線路の中に置くので、部分はその行に合わせて測る。
            let around = match &nodes[node] {
                RailroadNode::Choice(_) | RailroadNode::Optional(_) => 4.0 * m.arc,
                RailroadNode::Repetition(_, min, _) => {
                    (if *min == 0.0 { 6.0 } else { 2.0 }) * m.arc
                }
                RailroadNode::Sequence(_) => 6.0 * m.arc,
                _ => 0.0,
            };
            for &child in nodes[node].parts() {
                pending.push((child, js::max(40.0, width - around)));
            }
            i += 1;
        }
    }
    let add = |b: LBox, boxes: &mut Vec<LBox>| {
        boxes.push(b);
        boxes.len() - 1
    };
    fold(
        root,
        |k| nodes[k].parts().to_vec(),
        |k, kids: Vec<usize>| {
            let room = budgets.get(&k).copied().unwrap_or(f64::INFINITY);
            let b = match &nodes[k] {
                RailroadNode::Terminal(value) => {
                    let text = shown(value);
                    let text = if !text.is_empty() && text.bytes().all(|b| b == b' ') {
                        "␣".repeat(text.len())
                    } else {
                        text
                    };
                    leaf(Kind::Terminal, text, String::new(), st::MONO, m)
                }
                RailroadNode::Nonterminal(name) => {
                    leaf(Kind::NonTerminal, shown(name), name.clone(), 0, m)
                }
                RailroadNode::Special(text) => leaf(
                    Kind::Special,
                    format!("? {} ?", shown(text)),
                    String::new(),
                    st::ITALIC,
                    m,
                ),
                RailroadNode::Sequence(_) => {
                    let mut width = kids.len().saturating_sub(1) as f64 * m.gap;
                    let mut up = 0.0;
                    let mut down = 0.0;
                    for &kid in &kids {
                        let b = &boxes[kid];
                        width += b.width;
                        up = js::max(up, b.up);
                        down = js::max(down, b.down);
                    }
                    if width <= room || kids.len() < 2 {
                        plain(Kind::Sequence, width, up, down, kids)
                    } else {
                        let mut rows: Vec<(f64, f64, f64, Vec<usize>)> = Vec::new();
                        let mut row: Vec<usize> = Vec::new();
                        let mut row_w = 0.0;
                        let flush = |row: &mut Vec<usize>,
                                     row_w: &mut f64,
                                     rows: &mut Vec<(f64, f64, f64, Vec<usize>)>,
                                     boxes: &Vec<LBox>| {
                            let mut up = 0.0;
                            let mut down = 0.0;
                            for &kid in row.iter() {
                                up = js::max(up, boxes[kid].up);
                                down = js::max(down, boxes[kid].down);
                            }
                            rows.push((*row_w, up, down, std::mem::take(row)));
                            *row_w = 0.0;
                        };
                        for &kid in &kids {
                            if !row.is_empty()
                                && row_w + m.gap + boxes[kid].width
                                    > js::max(40.0, room - 6.0 * m.arc)
                            {
                                flush(&mut row, &mut row_w, &mut rows, boxes);
                            }
                            row_w += (if row.is_empty() { 0.0 } else { m.gap }) + boxes[kid].width;
                            row.push(kid);
                        }
                        flush(&mut row, &mut row_w, &mut rows, boxes);
                        if rows.len() < 2 {
                            plain(Kind::Sequence, width, up, down, kids)
                        } else {
                            let mut folded_width = 0.0;
                            let mut offset = 0.0;
                            for i in 0..rows.len() {
                                folded_width = js::max(folded_width, rows[i].0);
                                if i > 0 {
                                    offset +=
                                        rows[i - 1].2 + js::max(m.sep, 2.0 * m.arc) + rows[i].1;
                                }
                            }
                            let first_up = rows[0].1;
                            let last_down = rows[rows.len() - 1].2;
                            let row_boxes: Vec<usize> = rows
                                .into_iter()
                                .map(|(w, up, down, kids)| {
                                    add(plain(Kind::Sequence, w, up, down, kids), boxes)
                                })
                                .collect();
                            plain(
                                Kind::Folded,
                                folded_width + 6.0 * m.arc,
                                first_up,
                                offset + last_down,
                                row_boxes,
                            )
                        }
                    }
                }
                RailroadNode::Choice(_) => {
                    let mut inner = 0.0;
                    let mut offset = 0.0;
                    for i in 0..kids.len() {
                        inner = js::max(inner, boxes[kids[i]].width);
                        if i > 0 {
                            offset = below(offset, &boxes[kids[i - 1]], &boxes[kids[i]], m);
                        }
                    }
                    let up = kids.first().map_or(0.0, |&k| boxes[k].up);
                    let down = kids.last().map_or(0.0, |&k| offset + boxes[k].down);
                    plain(Kind::Choice, inner + 4.0 * m.arc, up, down, kids)
                }
                RailroadNode::Optional(_) => bypass(kids[0], boxes, m),
                RailroadNode::Repetition(_, min, max) => {
                    let (min, max) = (*min, *max);
                    let kid = kids[0];
                    if min == 1.0 && max == 1.0 {
                        return kid;
                    }
                    let text = count(min, max);
                    let k = &boxes[kid];
                    let reach = js::max(k.down + m.sep, 2.0 * m.arc);
                    let label = if text.is_empty() {
                        0.0
                    } else {
                        even(m.measurer.width(&text, COUNT_SIZE, 0))
                    };
                    let looped = LBox {
                        kind: Kind::Loop,
                        width: js::max(k.width, label) + 2.0 * m.arc,
                        up: k.up,
                        down: reach
                            + if text.is_empty() {
                                ARROW / 2.0 + 1.0
                            } else {
                                COUNT_SIZE + 6.0
                            },
                        kids: vec![kid],
                        text,
                        id: String::new(),
                        reach,
                    };
                    if min == 0.0 {
                        let l = add(looped, boxes);
                        bypass(l, boxes, m)
                    } else {
                        looped
                    }
                }
            };
            add(b, boxes)
        },
    )
}

pub fn render_railroad(
    model: &RailroadModel,
    config: &Config,
    options: &RenderOptions,
) -> Result<Rendered> {
    tighten(options, |tight| Ok(draw(model, config, options, tight)))
}

struct Out {
    track: String,
    arrows: String,
    nodes: String,
}

impl Out {
    fn line(&mut self, x1: f64, y: f64, x2: f64) {
        if x2 > x1 {
            self.track
                .push_str(&format!("M{},{}H{}", num(x1), num(y), num(x2)));
        }
    }
}

fn draw(model: &RailroadModel, config: &Config, options: &RenderOptions, tight: bool) -> Rendered {
    let base = options.font_size.unwrap_or(16.0);
    let font_size = option(config, "fontSize", 0.0, 200.0);
    let size = if js::truthy(font_size) {
        font_size
    } else {
        js::round(base * 0.875)
    };
    let head_size = if js::truthy(font_size) {
        font_size
    } else {
        base
    };
    let padding = option(config, "padding", -1.0, 200.0);
    let line_height = js::round(size * 1.5);
    let measurer: &dyn TextMeasurer = match &options.measurer {
        Some(m) => m.as_ref(),
        None => default_measurer(),
    };
    let m = Metrics {
        measurer,
        size,
        padding: if padding < 0.0 { 12.0 } else { padding },
        height: even(line_height + if padding < 0.0 { 9.0 } else { 2.0 * padding }),
        gap: js::min(
            option(config, "horizontalSeparation", 16.0, 500.0),
            if tight { 12.0 } else { f64::INFINITY },
        ),
        sep: option(config, "verticalSeparation", 10.0, 500.0),
        arc: js::min(
            option(config, "arcRadius", 10.0, 200.0),
            if tight { 6.0 } else { f64::INFINITY },
        ),
    };
    let radius = if section(config).and_then(|s| s.get("showMarkers")) == Some(&Value::Bool(false))
    {
        0.0
    } else {
        option(config, "markerRadius", 4.0, 50.0)
    };
    let stroke_width = option(config, "strokeWidth", 0.0, 20.0);
    let stroke = if stroke_width > 0.0 {
        format!(" stroke-width=\"{}\"", num(stroke_width))
    } else {
        String::new()
    };
    let pad = options.padding.unwrap_or(8.0);
    let arc = m.arc;

    let a = num(arc);
    let turn = |sweep: u8, dx: &str, dy: &str| format!("a{a},{a} 0 0 {sweep} {dx}{a},{dy}{a}");
    let east_to_south = turn(1, "", "");
    let south_to_east = turn(0, "", "");
    let east_to_north = turn(0, "", "-");
    let north_to_east = turn(1, "", "-");
    let loop_down = turn(0, "-", "");
    let loop_up = turn(0, "-", "-");

    let title = layout_label(
        model.title.as_deref(),
        false,
        measurer,
        base,
        if tight { title_room(options) } else { 4000.0 },
        st::BOLD,
    );
    let title_height = if title.height > 0.0 {
        title.height + 16.0
    } else {
        0.0
    };
    let head_height = js::round(head_size * 1.5);
    let text_shift = size * 0.35;
    let box_top = m.height / 2.0;

    let place = |out: &mut Out, boxes: &[LBox], root: usize, left: f64, baseline: f64| {
        let mut stack: Vec<usize> = vec![root];
        let mut at: Vec<f64> = vec![left, baseline];
        while let Some(bi) = stack.pop() {
            let y = at.pop().unwrap();
            let x = at.pop().unwrap();
            let b = &boxes[bi];
            let kids = &b.kids;
            let right = x + b.width;
            match b.kind {
                Kind::Sequence => {
                    let mut kx = right;
                    for i in (0..kids.len()).rev() {
                        kx -= boxes[kids[i]].width;
                        stack.push(kids[i]);
                        at.push(kx);
                        at.push(y);
                        if i > 0 {
                            out.line(kx - m.gap, y, kx);
                        }
                        kx -= m.gap;
                    }
                }
                Kind::Folded => {
                    let start = x + 2.0 * arc;
                    let from = stack.len();
                    let mut offset = 0.0;
                    out.line(x, y, start);
                    let step = js::max(m.sep, 2.0 * arc);
                    for i in 0..kids.len() {
                        let kid = &boxes[kids[i]];
                        let ky = y + offset;
                        stack.push(kids[i]);
                        at.push(ky);
                        at.push(start);
                        if i + 1 < kids.len() {
                            let next = &boxes[kids[i + 1]];
                            let next_offset = offset + kid.down + step + next.up;
                            let mid = y + offset + kid.down + step / 2.0;
                            out.track.push_str(&format!(
                                "M{},{}H{}V{}H{}V{}H{}",
                                num(start + kid.width),
                                num(ky),
                                num(right - 2.0 * arc),
                                num(mid),
                                num(x + arc),
                                num(y + next_offset),
                                num(start)
                            ));
                            out.arrows.push_str(&format!(
                                "M{},{}l{},{}l{},{}z",
                                num(start - ARROW),
                                num(y + next_offset - ARROW / 2.0),
                                js::fmt(ARROW),
                                js::fmt(ARROW / 2.0),
                                js::fmt(-ARROW),
                                js::fmt(ARROW / 2.0)
                            ));
                            offset = next_offset;
                        } else {
                            out.track.push_str(&format!(
                                "M{},{}H{}V{}H{}",
                                num(start + kid.width),
                                num(ky),
                                num(right - arc),
                                num(y),
                                num(right)
                            ));
                        }
                    }
                    reverse(&mut stack, from);
                    reverse(&mut at, 2 * from);
                }
                Kind::Choice => {
                    let inner = b.width - 4.0 * arc;
                    let rail_in = x + arc;
                    let rail_out = right - arc;
                    let from = stack.len();
                    let mut offset = 0.0;
                    for i in 0..kids.len() {
                        let kid = &boxes[kids[i]];
                        if i > 0 {
                            offset = below(offset, &boxes[kids[i - 1]], kid, &m);
                        }
                        let ky = y + offset;
                        let kx = x + 2.0 * arc + (inner - kid.width) / 2.0;
                        let last = i == kids.len() - 1;
                        if i == 0 {
                            out.line(x, y, kx);
                            out.line(kx + kid.width, y, right);
                        } else {
                            let mut s = if last {
                                format!("M{},{}{east_to_south}V{}", num(x), num(y), num(ky - arc))
                            } else {
                                format!("M{},{}", num(rail_in), num(ky - arc))
                            };
                            s.push_str(&south_to_east);
                            if kx > rail_in + arc {
                                s.push_str(&format!("H{}", num(kx)));
                            }
                            s.push_str(&format!("M{},{}", num(kx + kid.width), num(ky)));
                            if kx > rail_in + arc {
                                s.push_str(&format!("H{}", num(rail_out - arc)));
                            }
                            s.push_str(&east_to_north);
                            if last {
                                s.push_str(&format!("V{}{north_to_east}", num(y + arc)));
                            }
                            out.track.push_str(&s);
                        }
                        stack.push(kids[i]);
                        at.push(ky);
                        at.push(kx);
                    }
                    // 逆にして、枝が上から下の順に積みから出るように。
                    reverse(&mut stack, from);
                    reverse(&mut at, 2 * from);
                }
                Kind::Bypass => {
                    let kid = &boxes[kids[0]];
                    let kx = x + 2.0 * arc;
                    out.line(x, y, kx);
                    out.line(kx + kid.width, y, right);
                    out.track.push_str(&format!(
                        "M{},{}{east_to_north}V{}{north_to_east}H{}{east_to_south}V{}{south_to_east}",
                        num(x),
                        num(y),
                        num(y - b.reach + arc),
                        num(right - 2.0 * arc),
                        num(y - arc)
                    ));
                    stack.push(kids[0]);
                    at.push(kx);
                    at.push(y);
                }
                Kind::Loop => {
                    let kid = &boxes[kids[0]];
                    let kx = x + (b.width - kid.width) / 2.0;
                    let loop_y = y + b.reach;
                    let middle = x + b.width / 2.0;
                    out.line(x, y, kx);
                    out.line(kx + kid.width, y, right);
                    out.track.push_str(&format!(
                        "M{},{}{loop_down}V{}{south_to_east}H{}{east_to_north}V{}{loop_up}",
                        num(x + arc),
                        num(y),
                        num(loop_y - arc),
                        num(right - arc),
                        num(y + arc)
                    ));
                    out.arrows.push_str(&format!(
                        "M{},{}l{},{}v{}z",
                        num(middle - ARROW / 2.0),
                        num(loop_y),
                        js::fmt(ARROW),
                        js::fmt(-ARROW / 2.0),
                        js::fmt(ARROW)
                    ));
                    if !b.text.is_empty() {
                        out.nodes.push_str(&format!(
                            "<text class=\"pele-edge-label pele-count\" x=\"{}\" y=\"{}\" font-size=\"{}\" fill=\"var(--_m)\">{}</text>",
                            num(middle),
                            num(loop_y + COUNT_SIZE + 3.0),
                            js::fmt(COUNT_SIZE),
                            esc(&b.text)
                        ));
                    }
                    stack.push(kids[0]);
                    at.push(kx);
                    at.push(y);
                }
                _ => {
                    let text = &b.text;
                    let space =
                        if text.starts_with(' ') || text.ends_with(' ') || text.contains("  ") {
                            " xml:space=\"preserve\""
                        } else {
                            ""
                        };
                    let shape = format!(
                        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"",
                        num(x),
                        num(y - box_top),
                        num(b.width),
                        num(m.height)
                    );
                    let label = format!(
                        "<text x=\"{}\" y=\"{}\"{space}",
                        num(x + b.width / 2.0),
                        num(y + text_shift)
                    );
                    if b.kind == Kind::Terminal {
                        out.nodes.push_str(&format!(
                            "<g class=\"pele-node pele-terminal\">{shape} rx=\"{}\" fill=\"var(--_s)\" stroke=\"var(--_b)\"{stroke}/>{label} font-family=\"{FONT_MONO}\">{}</text></g>",
                            num(box_top),
                            esc(text)
                        ));
                    } else if b.kind == Kind::NonTerminal {
                        out.nodes.push_str(&format!(
                            "<g class=\"pele-node pele-nonterminal\" data-id=\"{}\">{shape} fill=\"var(--_bg)\" stroke=\"var(--_b)\"{stroke}{ROUNDED}/>{label}>{}</text></g>",
                            esc_text(&b.id),
                            esc(text)
                        ));
                    } else {
                        // "? " と " ?" を除いた中身。
                        let inner = &text[2..text.len() - 2];
                        out.nodes.push_str(&format!(
                            "<g class=\"pele-node pele-special\">{shape} fill=\"var(--_bg)\" stroke=\"var(--_b)\" stroke-dasharray=\"4 3\"{stroke}{ROUNDED}/>{label} font-style=\"italic\"><tspan fill=\"var(--_m)\">? </tspan>{}<tspan fill=\"var(--_m)\"> ?</tspan></text></g>",
                            esc(inner)
                        ));
                    }
                }
            }
        }
    };

    let mut rules = String::new();
    let mut width = title.width;
    let mut top = pad + title_height;
    let mut bottom = pad + title.height;
    for rule in &model.rules {
        let mut boxes: Vec<LBox> = Vec::new();
        let room = if tight {
            js::max(
                80.0,
                options.max_width.unwrap_or(320.0) - 2.0 * pad - 4.0 * radius - 2.0 * LEAD - 1.0,
            )
        } else {
            f64::INFINITY
        };
        let root = measure(model, rule.definition, &m, room, &mut boxes);
        let b = &boxes[root];
        let name = shown(&rule.name);
        let baseline = top + head_height + HEAD_GAP + b.up;
        let start = pad + 2.0 * radius;
        let left = start + LEAD;
        let end = left + b.width + LEAD;
        let mut out = Out {
            track: String::new(),
            arrows: String::new(),
            nodes: String::new(),
        };
        out.line(start, baseline, left);
        out.line(left + b.width, baseline, end);
        place(&mut out, &boxes, root, left, baseline);
        let markers = if radius > 0.0 {
            format!(
                "<circle class=\"pele-marker pele-start\" cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"var(--_l)\"/><circle class=\"pele-marker pele-end\" cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"var(--_l)\"/>",
                num(pad + radius),
                num(baseline),
                num(radius),
                num(end + radius),
                num(baseline),
                num(radius)
            )
        } else {
            String::new()
        };
        rules.push_str(&format!(
            "<g class=\"pele-rule\" data-id=\"{}\"><text class=\"pele-rule-name\" x=\"{}\" y=\"{}\" font-size=\"{}\" font-weight=\"var(--_hw)\" text-anchor=\"start\">{}</text><path class=\"pele-edge pele-track\" d=\"{}\" fill=\"none\" stroke=\"var(--_l)\"{stroke}/>{}{markers}{}</g>",
            esc_text(&rule.name),
            num(pad),
            num(top + head_height / 2.0 + head_size * 0.35),
            num(head_size),
            esc(&name),
            out.track,
            if out.arrows.is_empty() {
                String::new()
            } else {
                format!(
                    "<path class=\"pele-marker\" d=\"{}\" fill=\"var(--_l)\"/>",
                    out.arrows
                )
            },
            out.nodes
        ));
        width = js::max_of(&[
            width,
            end + 2.0 * radius - pad,
            measurer.width(&name, head_size, st::BOLD),
        ]);
        bottom = baseline + b.down;
        top = bottom + m.sep + RULE_GAP;
    }

    let total_width = (width + 2.0 * pad).ceil();
    let total_height = (bottom + pad).ceil();
    let mut content = label_svg(
        &title,
        total_width / 2.0,
        pad + title.height / 2.0,
        " class=\"pele-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    if !rules.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-rules\" transform=\"translate(0.5,0.5)\" font-size=\"{}\" text-anchor=\"middle\">{rules}</g>",
            num(size)
        ));
    }
    Rendered {
        svg: svg_document(
            "railroad",
            total_width,
            total_height,
            base,
            options,
            &Accessible {
                acc_title: model.acc_title.as_deref(),
                acc_descr: model.acc_descr.as_deref(),
            },
            &content,
        ),
        width: total_width,
        height: total_height,
        links: Vec::new(),
    }
}
