//! state を描く（pele の `src/diagrams/state/render.ts`）。

use std::collections::HashMap;

use super::db::StateDb;
use super::graph::{StateEdge, StateNode};
use crate::diagrams::common::fit_width::{runs_across, turn_to_fit};
use crate::errors::Result;
use crate::js;
use crate::layout::compound::{CEdge, CNode, Dir, cnode, compound_layout, direction};
use crate::layout::layered::LayeredOptions;
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num};
use crate::svg::edges::{
    EdgePath, Fill, casings_svg, edge_casing, edge_label_svg, marker, marker_trim, over_fills,
    route_path, side_reach,
};
use crate::svg::ink::{ink_fill, paints};
use crate::svg::root::{Accessible, svg_document};
use crate::svg::shapes::end_side;
use crate::svg::theme::{ResolvedStyle, class_names, resolve_style, sets_fill};
use crate::svg::title::with_title;
use crate::text::entities::decode_entities;
use crate::text::label::{Label, layout_label};
use crate::text::measurer::{TextMeasurer, default_measurer, style};
use crate::types::{LinkInfo, RenderOptions, Rendered};
use crate::util::url::{linked, safe_url};
use crate::util::yaml::Value;

const NODE_SEP: f64 = 40.0;
const EDGE_SEP: f64 = 16.0;
const RANK_SEP: f64 = 44.0;
const GROUP_PAD: f64 = 16.0;
const REGION_PAD: f64 = 10.0;
const PAD_X: f64 = 16.0;
const PAD_Y: f64 = 10.0;
const LOOP: f64 = 26.0;
const FOLD: f64 = 8.0;
const NOTE_WRAP: f64 = 320.0;
const CORNER: f64 = 10.0;
const BAR: f64 = 8.0;
const SURFACE: &str = " fill=\"var(--_s)\" stroke=\"var(--_b)\"";
const RULE: &str = " fill=\"none\" stroke=\"var(--_b)\"";

/// 描く形の種類。`REGION` から後ろは描かない。
mod k {
    pub const STATE: u8 = 0;
    pub const DESCRIBED: u8 = 1;
    pub const START: u8 = 2;
    pub const END: u8 = 3;
    pub const CHOICE: u8 = 4;
    pub const FORK: u8 = 5;
    pub const JOIN: u8 = 6;
    pub const NOTE: u8 = 7;
    pub const COMPOSITE: u8 = 8;
    pub const REGION: u8 = 9;
    pub const HIDDEN: u8 = 10;
}

fn kind_of(shape: Option<&str>) -> u8 {
    match shape {
        Some("rectWithTitle") => k::DESCRIBED,
        Some("stateStart") => k::START,
        Some("stateEnd") => k::END,
        Some("choice") => k::CHOICE,
        Some("fork") => k::FORK,
        Some("join") => k::JOIN,
        Some("note") => k::NOTE,
        Some("divider") => k::HIDDEN,
        _ => k::STATE,
    }
}

const CLASSES: [&str; 9] = [
    "pele-node pele-state",
    "pele-node pele-state",
    "pele-node pele-state-start",
    "pele-node pele-state-end",
    "pele-node pele-state-choice",
    "pele-node pele-state-fork",
    "pele-node pele-state-join",
    "pele-note",
    "pele-cluster pele-state-composite",
];

/// `raw.replace(/(?:^| )statediagram-(?:state|cluster-alt|cluster|note)(?= |$)/g, '')`。後で `class_names` が空白を
/// 詰めるので、空白で分けた語を除くのと同じ。
fn strip_mermaid_classes(raw: &str) -> String {
    raw.split(' ')
        .filter(|w| {
            !matches!(
                *w,
                "statediagram-state"
                    | "statediagram-cluster-alt"
                    | "statediagram-cluster"
                    | "statediagram-note"
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

struct View {
    /// 配置に要る見えない助けにはない。
    node: Option<usize>,
    kind: u8,
    style: Option<ResolvedStyle>,
    label: Label,
    lines: Vec<Label>,
    w: f64,
    h: f64,
    title_h: f64,
}

fn vertical(dir: Option<Dir>) -> bool {
    matches!(dir, Some(Dir::TB | Dir::BT))
}

fn number_option(config: &Config, key: &str, fallback: f64) -> f64 {
    match config.get("state") {
        Some(Value::Obj(s)) => match s.get(key) {
            Some(Value::Num(v)) if *v > 0.0 => js::min(*v, 2000.0),
            _ => fallback,
        },
        _ => fallback,
    }
}

/// Mermaid は click の文の文字の引用符を残し、描くときに取る。
fn unquote(text: &str) -> &str {
    text.trim_matches('"')
}

fn clamp(value: f64, min: f64, max: f64) -> f64 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

fn rect(x: f64, y: f64, w: f64, h: f64, rest: &str) -> String {
    format!(
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{rest}/>",
        num(x),
        num(y),
        num(w),
        num(h)
    )
}

fn group(dir: Dir, pad_x: f64, pad_top: f64, pad_bottom: f64) -> CNode {
    let mut c = cnode(0.0, 0.0, -1);
    c.is_group = true;
    c.dir = Some(dir);
    c.pad_x = pad_x;
    c.pad_top = pad_top;
    c.pad_bottom = pad_bottom;
    c
}

pub fn render_state(db: &StateDb, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let dir = db.direction();
    if !runs_across(Some(&dir)) {
        return Ok(draw(db, config, options, false));
    }
    turn_to_fit(options, |down| Ok(draw(db, config, options, down)))
}

/// `turned` は横に流れる図を縦に流して描く。
fn draw(db: &StateDb, config: &Config, options: &RenderOptions, turned: bool) -> Rendered {
    let graph = &db.graph;
    // state の図は flowchart の曲線に従う。
    let curve: Option<&str> = match config.get("flowchart") {
        Some(Value::Obj(f)) => match f.get("curve") {
            Some(Value::Str(s)) => Some(s.as_str()),
            _ => None,
        },
        _ => None,
    };
    let size = options.font_size.unwrap_or(16.0);
    let small = js::round(size * 0.875);
    let measurer: &dyn TextMeasurer = match &options.measurer {
        Some(m) => m.as_ref(),
        None => default_measurer(),
    };
    let wrap_width = number_option(config, "wrappingWidth", 200.0);
    let pad = options.padding.unwrap_or(8.0);
    let icons = options.icons.as_ref();
    let root_dir = if turned {
        Dir::TB
    } else {
        direction(&db.direction())
    };
    let text = |raw: Option<&str>, font_size: f64, wrap: f64, st: u32| -> Label {
        layout_label(raw, true, measurer, font_size, wrap, st)
    };
    let node_of = |v: &View| -> &StateNode { &graph.nodes[v.node.unwrap()] };

    let mut index: HashMap<&str, usize> = HashMap::new();
    let mut views: Vec<View> = Vec::new();
    let mut cnodes: Vec<CNode> = Vec::new();
    let add = |views: &mut Vec<View>,
               cnodes: &mut Vec<CNode>,
               node: Option<usize>,
               kind: u8,
               c: Option<CNode>|
     -> usize {
        views.push(View {
            node,
            kind,
            style: None,
            label: Label::empty(),
            lines: Vec::new(),
            w: 0.0,
            h: 0.0,
            title_h: 0.0,
        });
        if let Some(c) = c {
            cnodes.push(c);
        }
        views.len() - 1
    };
    for (n, node) in graph.nodes.iter().enumerate() {
        if node.shape.as_deref() == Some("noteGroup") {
            continue;
        }
        let kind = if node.is_group {
            if node.shape.as_deref() == Some("divider") {
                k::REGION
            } else {
                k::COMPOSITE
            }
        } else {
            kind_of(node.shape.as_deref())
        };
        let at = add(&mut views, &mut cnodes, Some(n), kind, None);
        index.insert(&node.id, at);
    }
    let states = views.len();

    let mut logical: Vec<i64> = Vec::with_capacity(states);
    for i in 0..states {
        let parent = node_of(&views[i])
            .parent_id
            .as_deref()
            .and_then(|p| index.get(p).copied());
        logical.push(match parent {
            Some(p) if p != i && node_of(&views[p]).is_group => p as i64,
            _ => -1,
        });
    }
    // 互いの中で宣言し直した合成の状態は、根から入れない輪になる。輪ごとに 1 つを根に移し、木にする。
    {
        let mut kids: Vec<Vec<usize>> = vec![Vec::new(); states];
        let mut queue: Vec<usize> = Vec::new();
        let mut placed = vec![false; states];
        for i in 0..states {
            if logical[i] == -1 {
                placed[i] = true;
                queue.push(i);
            } else {
                kids[logical[i] as usize].push(i);
            }
        }
        let mut done = 0;
        let settle = |queue: &mut Vec<usize>, placed: &mut Vec<bool>, done: &mut usize| {
            while *done < queue.len() {
                let at = queue[*done];
                for &kid in &kids[at] {
                    if placed[kid] {
                        continue;
                    }
                    placed[kid] = true;
                    queue.push(kid);
                }
                *done += 1;
            }
        };
        settle(&mut queue, &mut placed, &mut done);
        let mut seen = vec![-1i64; states];
        for i in 0..states {
            if placed[i] {
                continue;
            }
            let mut ring = i;
            while seen[ring] != i as i64 {
                seen[ring] = i as i64;
                ring = logical[ring] as usize;
            }
            logical[ring] = -1;
            placed[ring] = true;
            queue.push(ring);
            settle(&mut queue, &mut placed, &mut done);
        }
    }
    let mut owners = vec![-1i64; states];
    for i in 0..states {
        let owner = node_of(&views[i])
            .note_for
            .as_deref()
            .and_then(|id| index.get(id).copied());
        let Some(owner) = owner else {
            continue;
        };
        owners[i] = owner as i64;
        logical[i] = logical[owner];
    }

    // Mermaid と同じく、向きを決めない合成の状態は、まわりがどうでも上から下に流れる。
    let inner_dir = |views: &[View], composite: usize| -> Dir {
        direction(node_of(&views[composite]).dir.as_deref().unwrap_or(""))
    };
    // 状態のまわりの流れが縦か。
    let across = |views: &[View], logical: &[i64], i: usize| -> bool {
        vertical(Some(if logical[i] == -1 {
            root_dir
        } else {
            inner_dir(views, logical[i] as usize)
        }))
    };

    let mut degree = vec![0i64; states];
    let mut noted = vec![false; states];
    // 状態が初めて遷移に加わる順。配置は宣言の順ではなくこの順に従い、先に宣言した状態が流れの頭にならない。
    let mut appear: Vec<f64> = vec![f64::INFINITY; states];
    let mut appeared = 0.0;
    let mut counts = vec![0i64; 2 * states];
    for edge in &graph.edges {
        let (Some(&src), Some(&dst)) =
            (index.get(edge.start.as_str()), index.get(edge.end.as_str()))
        else {
            continue;
        };
        if edge.dashed {
            noted[if views[src].kind == k::NOTE { dst } else { src }] = true;
            continue;
        }
        counts[2 * src] += 1;
        degree[src] = degree[src].max(counts[2 * src]);
        counts[2 * dst + 1] += 1;
        degree[dst] = degree[dst].max(counts[2 * dst + 1]);
        if appear[src] == f64::INFINITY {
            appear[src] = appeared;
            appeared += 1.0;
        }
        if appear[dst] == f64::INFINITY {
            appear[dst] = appeared;
            appeared += 1.0;
        }
    }

    for i in 0..states {
        let node = node_of(&views[i]);
        let declarations: Vec<String> = if !node.css_styles.is_empty() {
            node.css_compiled_styles
                .iter()
                .chain(node.css_styles.iter())
                .cloned()
                .collect()
        } else {
            node.css_compiled_styles.clone()
        };
        let st = resolve_style(&declarations);
        let text_style =
            (if st.bold { style::BOLD } else { 0 }) | (if st.italic { style::ITALIC } else { 0 });
        let font_size = st.font_size.unwrap_or(size);
        let mut c: Option<CNode> = None;
        let kind = views[i].kind;
        let is_across = across(&views, &logical, i);
        match kind {
            k::COMPOSITE => {
                let label = text(node.label.as_deref(), font_size, 4000.0, text_style);
                let title_h = if label.height > 0.0 {
                    label.height + 12.0
                } else {
                    0.0
                };
                let mut g = group(
                    inner_dir(&views, i),
                    GROUP_PAD,
                    title_h + GROUP_PAD,
                    GROUP_PAD,
                );
                g.min_w = label.width + 2.0 * GROUP_PAD;
                views[i].label = label;
                views[i].title_h = title_h;
                c = Some(g);
            }
            k::REGION => {
                c = Some(group(
                    inner_dir(&views, i),
                    REGION_PAD,
                    REGION_PAD,
                    REGION_PAD,
                ));
            }
            k::START => {
                views[i].w = 14.0;
                views[i].h = 14.0;
            }
            k::END => {
                views[i].w = 18.0;
                views[i].h = 18.0;
            }
            k::CHOICE => {
                views[i].w = 30.0;
                views[i].h = 30.0;
            }
            k::FORK | k::JOIN => {
                let length = clamp(40.0 * degree[i] as f64 + 16.0, 72.0, 320.0);
                views[i].w = if is_across { length } else { BAR };
                views[i].h = if is_across { BAR } else { length };
            }
            k::HIDDEN => {}
            k::NOTE => {
                let label = text(
                    node.label.as_deref(),
                    small,
                    js::max(wrap_width, NOTE_WRAP),
                    0,
                );
                views[i].w = label.width + 24.0;
                views[i].h = label.height + 16.0;
                views[i].label = label;
            }
            _ => {
                let label = text(node.label.as_deref(), font_size, wrap_width, text_style);
                let mut width = label.width;
                let mut height = label.height + 2.0 * PAD_Y;
                if kind == k::DESCRIBED {
                    height += 14.0;
                    for line in &node.description {
                        let l = text(
                            Some(if line.is_empty() { " " } else { line }),
                            small,
                            wrap_width,
                            text_style,
                        );
                        if l.width > width {
                            width = l.width;
                        }
                        height += l.height;
                        views[i].lines.push(l);
                    }
                }
                views[i].w = js::max(width + 2.0 * PAD_X, 56.0);
                views[i].h = height;
                views[i].label = label;
            }
        }
        views[i].style = Some(st);
        let mut c = c.unwrap_or_else(|| cnode(views[i].w, views[i].h, -1));
        // 状態の箱の 1 辺の遷移は、角を避けてその辺に沿って広がる。
        if kind == k::STATE || kind == k::DESCRIBED {
            c.span = (if is_across { views[i].w } else { views[i].h }) - 24.0;
        }
        cnodes.push(c);
    }

    let mut cedges: Vec<CEdge> = Vec::new();
    let mut edge_labels: Vec<Label> = Vec::new();
    let mut drawn: Vec<&StateEdge> = Vec::new();
    // 各状態の自分への遷移: 数、いちばん広いラベル、ラベルの高さ。
    let mut loop_count = vec![0i64; states];
    let mut loop_width = vec![0f64; states];
    let mut loop_height = vec![0f64; states];
    // 合成の状態とその中の状態の間の遷移は、その状態と合成の状態のすぐ内側の点の間を走り、合成の状態の縁まで描き足す。
    let mut anchors: HashMap<usize, usize> = HashMap::new();
    let encloses = |cnodes: &[CNode], logical: &[i64], composite: usize, item: usize| -> bool {
        if !cnodes[composite].is_group {
            return false;
        }
        let mut p = logical[item];
        while p != -1 {
            if p as usize == composite {
                return true;
            }
            p = logical[p as usize];
        }
        false
    };
    for edge in &graph.edges {
        let (Some(&s0), Some(&d0)) = (index.get(edge.start.as_str()), index.get(edge.end.as_str()))
        else {
            continue;
        };
        let (mut src, mut dst) = (s0, d0);
        let label = text(Some(&edge.label), small, wrap_width, 0);
        let label_w = if label.width > 0.0 {
            label.width + 12.0
        } else {
            0.0
        };
        if src == dst {
            loop_count[src] += 1;
            loop_width[src] = js::max(loop_width[src], label_w);
            loop_height[src] += label.height;
        } else if encloses(&cnodes, &logical, src, dst) {
            anchors.insert(cedges.len(), src);
            appear.push(-1.0);
            src = add(
                &mut views,
                &mut cnodes,
                None,
                k::HIDDEN,
                Some(cnode(0.0, 0.0, src as i64)),
            );
        } else if encloses(&cnodes, &logical, dst, src) {
            anchors.insert(cedges.len(), dst);
            appear.push(f64::INFINITY);
            dst = add(
                &mut views,
                &mut cnodes,
                None,
                k::HIDDEN,
                Some(cnode(0.0, 0.0, dst as i64)),
            );
        }
        drawn.push(edge);
        cedges.push(CEdge {
            src,
            dst,
            minlen: 1.0,
            label_w,
            label_h: if label.height > 0.0 {
                label.height + 4.0
            } else {
                0.0
            },
            route: Vec::new(),
            label_x: 0.0,
            label_y: 0.0,
        });
        edge_labels.push(label);
    }

    // ノートのある状態や、自分への遷移のある合成の状態は、見えない自分のまとまりに入る: ノートは状態の横、流れを
    // 横切る向きに、遷移を乱さずに置く。
    for i in 0..states {
        if owners[i] != -1 {
            continue;
        }
        let is_across = across(&views, &logical, i);
        // 輪は右に、流れが横なら下に下がり、ラベルはその先に。
        let mut room = 0.0;
        if loop_count[i] > 0 {
            let reach = LOOP + (loop_count[i] - 1) as f64 * 8.0;
            room = if is_across {
                reach + loop_width[i]
            } else {
                reach * 0.75 + 4.0 + loop_height[i]
            };
            let item = &mut cnodes[i];
            if !item.is_group {
                if is_across {
                    item.w += 2.0 * room;
                    item.h = js::max(item.h, loop_height[i]);
                } else {
                    item.h += 2.0 * room;
                    item.w = js::max(item.w, loop_width[i]);
                }
                room = 0.0;
            }
        }
        if !noted[i] && room == 0.0 {
            cnodes[i].parent = logical[i];
            continue;
        }
        let mut c = if is_across {
            group(Dir::LR, room, 0.0, 0.0)
        } else {
            group(Dir::TB, 0.0, room, room)
        };
        c.parent = logical[i];
        appear.push(appear[i]);
        cnodes[i].parent = add(&mut views, &mut cnodes, None, k::HIDDEN, Some(c)) as i64;
    }
    for i in 0..states {
        if owners[i] != -1 {
            cnodes[i].parent = cnodes[owners[i] as usize].parent;
        }
    }

    {
        // 木に番号をつける: どの段でも、始めの状態が先で、残りは遷移が持ち込む順。
        let mut members: Vec<Vec<usize>> = vec![Vec::new(); cnodes.len()];
        let mut top: Vec<usize> = Vec::new();
        for (i, c) in cnodes.iter().enumerate() {
            if c.parent >= 0 {
                members[c.parent as usize].push(i);
            } else {
                top.push(i);
            }
        }
        let initial = |i: usize| -> i32 { if views[i].kind == k::START { 0 } else { 1 } };
        let by_flow = |a: &usize, b: &usize| -> std::cmp::Ordering {
            initial(*a)
                .cmp(&initial(*b))
                .then_with(|| {
                    let (x, y) = (appear[*a], appear[*b]);
                    if x == y {
                        std::cmp::Ordering::Equal
                    } else if x < y {
                        std::cmp::Ordering::Less
                    } else {
                        std::cmp::Ordering::Greater
                    }
                })
                .then_with(|| a.cmp(b))
        };
        let mut seq = 0.0;
        top.sort_by(by_flow);
        top.reverse();
        let mut pending = top;
        while let Some(i) = pending.pop() {
            cnodes[i].seq = seq;
            seq += 1.0;
            let mut inside = std::mem::take(&mut members[i]);
            inside.sort_by(by_flow);
            for &kk in inside.iter().rev() {
                pending.push(kk);
            }
        }
    }

    let layout = compound_layout(
        &mut cnodes,
        &mut cedges,
        root_dir,
        &LayeredOptions {
            node_sep: number_option(config, "nodeSpacing", NODE_SEP),
            edge_sep: EDGE_SEP,
            rank_sep: number_option(config, "rankSpacing", RANK_SEP),
            port_sep: 20.0,
            tracks: matches!(curve, Some("step" | "stepBefore" | "stepAfter")),
            straight: curve == Some("linear"),
            head_room: marker_trim("arrow_point"),
            ..Default::default()
        },
    );

    let width = (layout.width + 2.0 * pad).ceil();
    let height = (layout.height + 2.0 * pad).ceil();
    let mut links: Vec<LinkInfo> = Vec::new();

    // 丸めた y ごとの経路の点。合成の状態の題の帯を横切る辺を探す。
    let mut crossings: HashMap<i64, Vec<f64>> = HashMap::new();
    for e in &cedges {
        let route = &e.route;
        let mut kk = 3;
        while kk + 3 < route.len() {
            crossings
                .entry(js::round(route[kk + 1]) as i64)
                .or_default()
                .push(route[kk]);
            kk += 3;
        }
    }

    let mut children: Vec<Vec<usize>> = vec![Vec::new(); cnodes.len()];
    let mut order: Vec<usize> = Vec::new();
    for (i, c) in cnodes.iter().enumerate() {
        if c.parent >= 0 {
            children[c.parent as usize].push(i);
        } else {
            order.push(i);
        }
    }
    let roots = order.clone();
    let mut q = 0;
    while q < order.len() {
        let at = order[q];
        order.extend(children[at].iter().copied());
        q += 1;
    }

    let mut clusters = String::new();
    let mut fills: Vec<Fill> = Vec::new();
    let mut dividers = String::new();
    let mut nodes_out = String::new();
    let mut notes_out = String::new();
    // 合成の状態の中身のうち、領域の間に破線を引く。
    let divide =
        |dividers: &mut String, items: &[usize], left: f64, top: f64, right: f64, bottom: f64| {
            let regions: Vec<usize> = items
                .iter()
                .copied()
                .filter(|&i| views[i].kind == k::REGION)
                .collect();
            if regions.len() < 2 {
                return;
            }
            let lo = |i: usize, x: bool| -> f64 {
                if x {
                    cnodes[i].x - cnodes[i].w / 2.0
                } else {
                    cnodes[i].y - cnodes[i].h / 2.0
                }
            };
            let hi = |i: usize, x: bool| -> f64 {
                if x {
                    cnodes[i].x + cnodes[i].w / 2.0
                } else {
                    cnodes[i].y + cnodes[i].h / 2.0
                }
            };
            for x in [true, false] {
                let mut sorted = regions.clone();
                sorted.sort_by(|&a, &b| {
                    let d = lo(a, x) - lo(b, x);
                    if d != 0.0 && !d.is_nan() {
                        js::order(d)
                    } else {
                        a.cmp(&b)
                    }
                });
                let mut apart = true;
                for kk in 1..sorted.len() {
                    if hi(sorted[kk - 1], x) > lo(sorted[kk], x) + 0.5 {
                        apart = false;
                    }
                }
                if !apart {
                    continue;
                }
                for kk in 1..sorted.len() {
                    let at = num((hi(sorted[kk - 1], x) + lo(sorted[kk], x)) / 2.0 + pad);
                    let d = if x {
                        format!("{at},{}V{}", num(top), num(bottom))
                    } else {
                        format!("{},{at}H{}", num(left), num(right))
                    };
                    dividers.push_str(&format!("<path class=\"pele-divider\" d=\"M{d}\"/>"));
                }
                return;
            }
            // 横にも縦にも並ばない領域は、それぞれ縁取る。
            for &i in &regions {
                let c = &cnodes[i];
                dividers.push_str(&rect(
                    c.x - c.w / 2.0 + pad,
                    c.y - c.h / 2.0 + pad,
                    c.w,
                    c.h,
                    &format!(" class=\"pele-divider\" rx=\"{}\"", js::fmt(CORNER)),
                ));
            }
        };
    divide(
        &mut dividers,
        &roots,
        pad / 2.0,
        pad / 2.0,
        width - pad / 2.0,
        height - pad / 2.0,
    );

    for &i in &order {
        let view = &views[i];
        let (Some(n), Some(st)) = (view.node, view.style.as_ref()) else {
            continue;
        };
        if view.kind >= k::REGION {
            continue;
        }
        let node = &graph.nodes[n];
        let c = &cnodes[i];
        let x = c.x + pad;
        let y = c.y + pad;
        let w = view.w;
        let h = view.h;
        let ink = ink_fill(&st.shape, &st.text, None);
        let label_attrs = format!(" class=\"pele-label\"{ink}{}", st.text);
        let mut body;

        match view.kind {
            k::COMPOSITE => {
                let left = x - c.w / 2.0;
                let top = y - c.h / 2.0;
                if paints(&st.shape) {
                    fills.push(Fill {
                        box_: [left, top, left + c.w, top + c.h],
                        shape: st.shape.clone(),
                    });
                }
                body = rect(
                    left,
                    top,
                    c.w,
                    c.h,
                    &format!(
                        " rx=\"{}\" fill=\"var(--_a)\"{} stroke=\"var(--_b)\"{}",
                        js::fmt(CORNER),
                        if sets_fill(&st.shape) {
                            ""
                        } else {
                            " fill-opacity=\"0.5\""
                        },
                        st.shape
                    ),
                );
                if view.title_h > 0.0 {
                    body.push_str(&format!(
                        "<path d=\"M{},{}H{}\"{RULE}{}/>",
                        num(left),
                        num(top + view.title_h),
                        num(left + c.w),
                        st.line
                    ));
                    body.push_str(&label_svg(
                        &view.label,
                        title_x(c, view.label.width, &crossings) + pad,
                        top + view.title_h / 2.0,
                        &format!(" class=\"pele-cluster-label\"{ink}{}", st.text),
                        icons,
                    ));
                }
                divide(
                    &mut dividers,
                    &children[i],
                    left,
                    top + view.title_h,
                    left + c.w,
                    top + c.h,
                );
            }
            k::START => body = format!("<circle r=\"7\" fill=\"var(--_l)\"{}/>", st.shape),
            k::END => {
                body = format!(
                    "<circle r=\"8.5\" fill=\"var(--_bg)\" stroke=\"var(--_l)\"{}/><circle r=\"5\" fill=\"var(--_l)\"{}/>",
                    st.line, st.shape
                )
            }
            k::CHOICE => {
                body = format!(
                    "<polygon points=\"0,{} {},0 0,{} {},0\"{SURFACE}{}/>",
                    num(-h / 2.0),
                    num(w / 2.0),
                    num(h / 2.0),
                    num(-w / 2.0),
                    st.shape
                )
            }
            k::FORK | k::JOIN => {
                body = rect(
                    -w / 2.0,
                    -h / 2.0,
                    w,
                    h,
                    &format!(" rx=\"2\" fill=\"var(--_l)\"{}", st.shape),
                )
            }
            k::NOTE => {
                let r = num(w / 2.0);
                let b = num(h / 2.0);
                let fold = num(w / 2.0 - FOLD);
                let corner = num(FOLD - h / 2.0);
                body = format!(
                    "<path d=\"M-{r},-{b}H{fold}L{r},{corner}V{b}H-{r}Z\" fill=\"var(--_bg)\" stroke=\"var(--_b)\"/><path d=\"M{fold},-{b}V{corner}H{r}\"{RULE}/>{}",
                    label_svg(
                        &view.label,
                        0.0,
                        0.0,
                        &format!(
                            " class=\"pele-label\" font-size=\"{}\" fill=\"var(--_m)\"",
                            js::fmt(small)
                        ),
                        icons
                    )
                );
            }
            _ => {
                body = rect(
                    -w / 2.0,
                    -h / 2.0,
                    w,
                    h,
                    &format!(
                        " rx=\"{}\"{SURFACE}{}",
                        num(js::min(CORNER, h / 2.0)),
                        st.shape
                    ),
                );
                if view.kind == k::STATE {
                    body.push_str(&label_svg(&view.label, 0.0, 0.0, &label_attrs, icons));
                } else {
                    let mut at = PAD_Y - h / 2.0 + view.label.height;
                    body.push_str(&format!(
                        "<path d=\"M{},{}H{}\"{RULE}{}/>",
                        num(-w / 2.0),
                        num(at + 6.0),
                        num(w / 2.0),
                        st.line
                    ));
                    body.push_str(&label_svg(
                        &view.label,
                        0.0,
                        at - view.label.height / 2.0,
                        &label_attrs,
                        icons,
                    ));
                    at += 14.0;
                    for line in &view.lines {
                        body.push_str(&label_svg(
                            line,
                            0.0,
                            at + line.height / 2.0,
                            &format!(
                                " class=\"pele-state-description\" font-size=\"{}\"{ink}{}",
                                js::fmt(small),
                                st.text
                            ),
                            icons,
                        ));
                        at += line.height;
                    }
                }
            }
        }

        if let Some(link) = db.links.get(&node.id) {
            // 符号は先に文字に戻し、ブラウザがたどる形の URL を確かめる。
            let tooltip = decode_entities(unquote(&link.tooltip));
            if !tooltip.is_empty() {
                body = format!("<title>{}</title>{body}", esc(&tooltip));
            }
            body = linked(
                &body,
                &safe_url(unquote(&link.url)),
                &node.id,
                &mut links,
                options,
                " target=\"_blank\"",
            );
        }
        let open = format!(
            "<g class=\"{}{}\" data-id=\"{}\"",
            CLASSES[view.kind as usize],
            class_names(&strip_mermaid_classes(&node.css_classes)),
            esc_text(&node.id)
        );
        if view.kind == k::COMPOSITE {
            clusters.push_str(&format!("{open}>{body}</g>"));
        } else if view.kind == k::NOTE {
            notes_out.push_str(&format!(
                "{open} transform=\"translate({},{})\">{body}</g>",
                num(x),
                num(y)
            ));
        } else {
            nodes_out.push_str(&format!(
                "{open} transform=\"translate({},{})\">{body}</g>",
                num(x),
                num(y)
            ));
        }
    }

    // 経路の端を配置の箱から描いた形へ動かし、棒の上では端を広げる。
    let attach = |route: &mut [f64], at: usize, i: usize| {
        let c = &cnodes[i];
        if c.is_group {
            return;
        }
        let view = &views[i];
        let bar = view.kind == k::FORK || view.kind == k::JOIN;
        let other = if at == 0 { 3 } else { at - 3 };
        // 0 は x 軸、1 は y 軸。端は `axis` に向いた辺にある。
        for axis in [1usize, 0] {
            let extent = if axis == 1 { c.h } else { c.w };
            let drawn_extent = if axis == 1 { view.h } else { view.w };
            let breadth = if axis == 1 { view.w } else { view.h };
            let center = (if axis == 1 { c.y } else { c.x }) + pad;
            let cross = (if axis == 1 { c.x } else { c.y }) + pad;
            let delta = route[at + axis] - center;
            #[allow(clippy::neg_cmp_op_on_partial_ord)]
            if !((delta.abs() - extent / 2.0).abs() < 0.5) {
                continue;
            }
            route[at + axis] -= (js::sign(delta) * (extent - drawn_extent)) / 2.0;
            if bar && breadth > drawn_extent {
                route[at + 1 - axis] = clamp(
                    route[other + 1 - axis],
                    cross - breadth / 2.0 + 6.0,
                    cross + breadth / 2.0 - 6.0,
                );
            }
            return;
        }
    };

    let mut reach = side_reach();
    for (i, ce) in cedges.iter().enumerate() {
        if ce.src == ce.dst || ce.route.len() < 6 || drawn[i].dashed {
            continue;
        }
        reach.add(
            ce.dst,
            end_side(&ce.route, ce.route.len() - 3, &cnodes[ce.dst]),
            "arrow_point",
        );
    }

    let mut casings = String::new();
    let mut edges_out = String::new();
    let mut labels_out = String::new();
    let mut loop_seen = vec![0usize; states];
    let mut loop_text = vec![0f64; states];
    for i in 0..cedges.len() {
        let edge = drawn[i];
        let label = &edge_labels[i];
        let note = edge.dashed;
        let end_type = if note { "none" } else { "arrow_point" };
        let (src, dst) = (cedges[i].src, cedges[i].dst);
        let path: EdgePath = if src == dst {
            let a = &cnodes[src];
            let view = &views[src];
            let kk = loop_seen[src] as f64;
            loop_seen[src] += 1;
            // 輪は流れの使わない辺に下がる: 右、流れが横なら下。
            let side = across(&views, &logical, src);
            let w = if a.is_group { a.w } else { view.w };
            let h = if a.is_group { a.h } else { view.h };
            let out = (if side { w } else { h }) / 2.0;
            let spread = js::max(
                js::min((if side { h } else { w }) / 2.0 - 4.0, 8.0 + kk * 6.0),
                2.0,
            );
            let reach_ = LOOP + kk * 8.0;
            let hyp = js::hypot(reach_, spread);
            let trim = marker_trim(end_type) / if js::truthy(hyp) { hyp } else { 1.0 };
            // 右の輪のずれ。下の輪には 4 分の 1 回して使う。
            let (ax, ay) = (a.x, a.y);
            let px = |dx: f64, dy: f64| -> f64 { ax + pad + if side { out + dx } else { -dy } };
            let py = |dx: f64, dy: f64| -> f64 { ay + pad + if side { dy } else { out + dx } };
            let at =
                |dx: f64, dy: f64| -> String { format!("{},{}", num(px(dx, dy)), num(py(dx, dy))) };
            let mut path = EdgePath {
                d: format!(
                    "M{}C{} {} {}",
                    at(0.0, -spread),
                    at(reach_, -2.0 * spread),
                    at(reach_, 2.0 * spread),
                    at(reach_ * trim, spread + spread * trim)
                ),
                tokens: None,
                sx: 0.0,
                sy: 0.0,
                sdx: 0.0,
                sdy: 0.0,
                ex: px(0.0, spread),
                ey: py(0.0, spread),
                edx: px(-reach_, -spread) - px(0.0, 0.0),
                edy: py(-reach_, -spread) - py(0.0, 0.0),
            };
            let len = js::hypot(path.edx, path.edy);
            let len = if js::truthy(len) { len } else { 1.0 };
            path.edx /= len;
            path.edy /= len;
            // ラベルは遷移の順に、いちばん外の輪の先に積む。
            let furthest = LOOP + (loop_count[src] - 1) as f64 * 8.0;
            let below = loop_text[src] + label.height / 2.0;
            loop_text[src] += label.height;
            let ce = &mut cedges[i];
            ce.label_x = if side {
                ax + out + furthest + 4.0 + ce.label_w / 2.0
            } else {
                ax
            };
            ce.label_y = if side {
                ay - loop_height[src] / 2.0 + below
            } else {
                ay + out + furthest * 0.75 + 4.0 + below
            };
            path
        } else {
            let mut route = cedges[i].route.clone();
            let leaves = reach.of(src, end_side(&route, 0, &cnodes[src]));
            let arrives = reach.of(dst, end_side(&route, route.len() - 3, &cnodes[dst]));
            let mut kk = 0;
            while kk + 1 < route.len() {
                route[kk] += pad;
                route[kk + 1] += pad;
                kk += 3;
            }
            attach(&mut route, 0, src);
            let last = route.len() - 3;
            attach(&mut route, last, dst);
            if let Some(&composite) = anchors.get(&i)
                && route.len() >= 6
            {
                let g = &cnodes[composite];
                let inward = views[src].kind == k::HIDDEN;
                let at = if inward { 0 } else { route.len() - 3 };
                let near = matches!(g.dir, Some(Dir::TB | Dir::LR)) == inward;
                let point = if vertical(g.dir) {
                    [
                        route[at],
                        g.y + pad
                            + if near {
                                views[composite].title_h - g.h / 2.0
                            } else {
                                g.h / 2.0
                            },
                        0.0,
                    ]
                } else {
                    [
                        g.x + pad + if near { -g.w / 2.0 } else { g.w / 2.0 },
                        route[at + 1],
                        1.0,
                    ]
                };
                if inward {
                    route.splice(0..0, point);
                } else {
                    route.extend_from_slice(&point);
                }
            }
            route_path(
                &route,
                curve,
                0.0,
                marker_trim(end_type),
                Some(leaves),
                Some(arrives),
            )
        };
        let over = over_fills(
            &path.d,
            &[path.ex, path.ey],
            &fills,
            "",
            path.tokens.as_deref(),
        );
        if over.as_deref() == Some("") {
            casings.push_str(&edge_casing(&path, "none", end_type, "", false));
        }
        let over = over.filter(|o| !o.is_empty());
        let kind = if note {
            format!(
                "pele-note-edge\" stroke=\"{}\" stroke-dasharray=\"3 4",
                over.as_deref().unwrap_or("var(--_b)")
            )
        } else if let Some(o) = &over {
            format!("pele-transition\" stroke=\"{o}")
        } else {
            "pele-transition".to_string()
        };
        edges_out.push_str(&format!(
            "<g class=\"pele-edge {kind}\" data-id=\"{}\"><path d=\"{}\"/>{}</g>",
            esc_text(&edge.id),
            path.d,
            marker(
                end_type,
                path.ex,
                path.ey,
                path.edx,
                path.edy,
                over.as_deref().unwrap_or("var(--_l)"),
                "none"
            )
        ));
        if label.width > 0.0 {
            let x = cedges[i].label_x + pad;
            let y = cedges[i].label_y + pad;
            labels_out.push_str(&edge_label_svg(&edge.id, label, x, y, "", icons));
        }
    }

    let mut content = String::new();
    if !clusters.is_empty() {
        content.push_str(&format!("<g class=\"pele-clusters\">{clusters}</g>"));
    }
    if !dividers.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-dividers\" stroke-dasharray=\"4 4\"{RULE}>{dividers}</g>"
        ));
    }
    content.push_str(&casings_svg(&casings));
    if !edges_out.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-edges\" fill=\"none\" stroke=\"var(--_l)\" stroke-linecap=\"round\">{edges_out}</g>"
        ));
    }
    if !labels_out.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-edge-labels\" font-size=\"{}\">{labels_out}</g>",
            js::fmt(small)
        ));
    }
    content.push_str(&format!(
        "<g class=\"pele-nodes\">{nodes_out}{notes_out}</g>"
    ));
    let titled = with_title(
        db.title.as_deref(),
        measurer,
        size,
        pad,
        width,
        height,
        content,
    );
    let acc = Accessible {
        acc_title: db.acc_title.as_deref(),
        acc_descr: db.acc_descr.as_deref(),
    };
    let svg = svg_document(
        "state",
        titled.width,
        titled.height,
        size,
        options,
        &acc,
        &titled.content,
    );
    Rendered {
        svg,
        width: titled.width,
        height: titled.height,
        links,
    }
}

/// 合成の状態の題を、上から入る辺が横切らないよう、題の帯のどこに置くか選ぶ。
fn title_x(c: &CNode, width: f64, crossings: &HashMap<i64, Vec<f64>>) -> f64 {
    let Some(xs) = crossings.get(&(js::round(c.y - c.h / 2.0) as i64)) else {
        return c.x;
    };
    if width == 0.0 {
        return c.x;
    }
    let left = c.x - c.w / 2.0;
    let half = width / 2.0;
    let mut best = c.x;
    let mut best_gap = -1.0;
    for x in [c.x, left + GROUP_PAD + half, left + c.w - GROUP_PAD - half] {
        let mut gap = f64::INFINITY;
        for &cx in xs {
            if cx > left && cx < left + c.w {
                gap = js::min(gap, (cx - x).abs() - half);
            }
        }
        if gap >= 6.0 {
            return x;
        }
        if gap > best_gap + 0.5 {
            best_gap = gap;
            best = x;
        }
    }
    best
}
