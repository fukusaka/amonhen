//! class を描く（pele の `src/diagrams/class/render.ts`）。

use std::collections::HashMap;

use super::db::ClassDb;
use super::graph::{GraphEdge, GraphNode, NodeShape, Pattern, build_class_graph};
use super::markers::{class_marker, class_marker_trim};
use super::members::ClassMember;
use super::types::ClassNode;
use crate::diagrams::common::fit_width::{runs_across, tighten, turn_to_fit};
use crate::diagrams::common::generics::parse_generic_types;
use crate::errors::Result;
use crate::js;
use crate::layout::components::wrap_components;
use crate::layout::compound::{CEdge, CNode, Dir, cnode, compound_layout, direction};
use crate::layout::layered::LayeredOptions;
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num};
use crate::svg::cluster::{
    Crossings, cluster_title_x, mark_crossings, struck_title, title_crossed,
};
use crate::svg::edges::{EdgePath, edge_label_svg, route_path};
use crate::svg::ink::ink_fill;
use crate::svg::root::{Accessible, svg_document};
use crate::svg::shapes::end_side;
use crate::svg::theme::{FONT_MONO, ROUNDED, ResolvedStyle, class_names, resolve_style, rounded};
use crate::svg::title::with_title;
use crate::text::entities::decode_entities;
use crate::text::label::{Label, has_math, layout_label};
use crate::text::measurer::{TextMeasurer, default_measurer, style};
use crate::types::{LinkInfo, RenderOptions, Rendered};
use crate::util::url::linked;
use crate::util::yaml::Value;

const NODE_SEP: f64 = 40.0;
const EDGE_SEP: f64 = 16.0;
const RANK_SEP: f64 = 56.0;
const GROUP_PAD: f64 = 20.0;
const LOOP: f64 = 26.0;
const PAD_X: f64 = 12.0;
const TITLE_PAD: f64 = 8.0;
const SECTION_PAD: f64 = 6.0;
const EMPTY_SECTION: f64 = 10.0;
const MIN_WIDTH: f64 = 64.0;
const FOLD: f64 = 8.0;
const LINE: &str = "var(--_l)";
const TARGETS: [&str; 4] = ["_self", "_blank", "_parent", "_top"];

fn number_option(config: &Config, key: &str, fallback: f64) -> f64 {
    match config.get(key) {
        Some(Value::Num(v)) if *v > 0.0 => js::min(*v, 2000.0),
        _ => fallback,
    }
}

struct Line {
    text: String,
    width: f64,
    classifier: String,
}

/// 注釈: そのままの文字か、組んだ数式。
enum Annotation {
    Text(String),
    Math(Label),
}

struct View {
    node: usize,
    style: ResolvedStyle,
    label: Label,
    w: f64,
    h: f64,
    loops: usize,
    annotations: Vec<Annotation>,
    attributes: Vec<Line>,
    methods: Vec<Line>,
    title_h: f64,
    attributes_h: f64,
    font_size: f64,
}

#[derive(Clone, Copy)]
struct End {
    /// 辺（`cedges` の添字）。
    edge: usize,
    at: usize,
    toward: f64,
    order: usize,
    room: f64,
    side: i32,
    /// この端の文字が線の右か下に来るか。
    after: bool,
}

struct Card {
    text: String,
    math: Option<Label>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    parent: i64,
}

fn member_line(member: &ClassMember, measurer: &dyn TextMeasurer, size: f64, mono: bool) -> Line {
    let text = js::trim(&decode_entities(&member.get_display_details().display_text)).to_string();
    let st = (if member.classifier == "*" {
        style::ITALIC
    } else {
        0
    }) | (if mono { style::MONO } else { 0 });
    Line {
        width: measurer.width(&text, size, st),
        text,
        classifier: member.classifier.clone(),
    }
}

pub fn render_class(db: &ClassDb, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    let fitted = |down: bool| tighten(options, |tight| Ok(draw(db, config, options, down, tight)));
    if runs_across(Some(&db.direction)) {
        turn_to_fit(options, fitted)
    } else {
        fitted(false)
    }
}

/// `raw.replace(/^default\s?/, '')`
fn strip_default(raw: &str) -> &str {
    match raw.strip_prefix("default") {
        Some(rest) => match rest.chars().next() {
            Some(c) if crate::util::chars::is_space_char(c) => &rest[c.len_utf8()..],
            _ => rest,
        },
        None => raw,
    }
}

/// 入れた順を保ち、同じ鍵に入れ直しても位置の変わらない表（JS の `Map`）。
#[derive(Default)]
struct Kept(Vec<(String, String)>);

impl Kept {
    fn set(&mut self, k: &str, v: &str) {
        match self.0.iter_mut().find(|(kk, _)| kk == k) {
            Some(slot) => slot.1 = v.to_string(),
            None => self.0.push((k.to_string(), v.to_string())),
        }
    }
}

/// クラスは多くの classDef の宣言を何度も持ちうる。違う宣言はそれぞれ 1 度だけ確かめ、効くプロパティの最後の値
/// だけを解く。
#[derive(Default)]
struct Styles {
    effect: HashMap<String, String>,
    from_defs: HashMap<String, Vec<(String, String)>>,
    kept: Kept,
}

impl Styles {
    fn keep(&mut self, decl: &str) {
        let prop = match self.effect.get(decl) {
            Some(p) => p.clone(),
            None => {
                let one = resolve_style(&[decl.to_string()]);
                let p = if one.shape.is_empty() && one.text.is_empty() {
                    String::new()
                } else {
                    let at = decl.find(':').unwrap_or(decl.len());
                    js::trim(&decl[..at]).to_lowercase()
                };
                self.effect.insert(decl.to_string(), p.clone());
                p
            }
        };
        if !prop.is_empty() {
            self.kept.set(&prop, decl);
        }
    }

    fn class_style(&mut self, db: &ClassDb, cls: &ClassNode) -> ResolvedStyle {
        self.kept.0.clear();
        match self.from_defs.get(&cls.css_classes) {
            None => {
                for name in cls.css_classes.split(' ') {
                    if let Some(def) = db.style_class(name) {
                        for s in &def.styles {
                            for part in s.split(',') {
                                self.keep(part);
                            }
                        }
                    }
                }
                self.from_defs
                    .insert(cls.css_classes.clone(), self.kept.0.clone());
            }
            Some(base) => {
                for (prop, decl) in base.clone() {
                    self.kept.set(&prop, &decl);
                }
            }
        }
        for s in &cls.styles {
            self.keep(s);
        }
        let values: Vec<String> = self.kept.0.iter().map(|(_, v)| v.clone()).collect();
        resolve_style(&values)
    }
}

/// `turned` は横に流れる図を縦に流して描く。`tight` は、名前空間の名前を、その横に入る辺から離しておく余地を
/// あきらめる。
fn draw(
    db: &ClassDb,
    config: &Config,
    options: &RenderOptions,
    turned: bool,
    tight: bool,
) -> Rendered {
    let empty = Config::new();
    let conf = match config.get("class") {
        Some(Value::Obj(m)) => m,
        _ => &empty,
    };
    let graph = build_class_graph(
        db,
        conf.get("hierarchicalNamespaces") != Some(&Value::Bool(false)),
    );
    let hide_empty = conf.get("hideEmptyMembersBox") == Some(&Value::Bool(true));
    let monospaced = options
        .monospace
        .as_ref()
        .is_some_and(|m| m.iter().any(|s| s == "members"));
    // クラスの書体の指定は等幅より強い。
    let mono = |st: &ResolvedStyle| -> bool {
        monospaced && !(st.text.contains("\"font-family:") || st.text.contains(";font-family:"))
    };
    let size = options.font_size.unwrap_or(16.0);
    let small = js::round(size * 0.875);
    let tiny = js::round(size * 0.8);
    let tiny_height = js::round(tiny * 1.4);
    let measurer: &dyn TextMeasurer = match &options.measurer {
        Some(m) => m.as_ref(),
        None => default_measurer(),
    };
    let pad = options.padding.unwrap_or(8.0);
    let icons = options.icons.as_ref();
    // 数式を組むときのほか、注釈と多重度は書いたとおりに保つ。
    let math_text = |raw: &str, font_size: f64| -> Option<Label> {
        if !raw.contains("$$") {
            return None;
        }
        let label = layout_label(Some(raw), false, measurer, font_size, 4000.0, 0);
        label.lines.iter().any(|l| has_math(l)).then_some(label)
    };
    let card_width = |raw: &str| -> f64 {
        if raw.is_empty() {
            return 0.0;
        }
        match math_text(raw, tiny) {
            Some(l) => l.width,
            None => measurer.width(&decode_entities(raw), tiny, 0),
        }
    };
    let root_dir = if turned {
        Dir::TB
    } else {
        direction(&db.direction)
    };

    let mut styles = Styles::default();

    let n = graph.nodes.len();
    let mut children = vec![0i32; n];
    for node in &graph.nodes {
        if node.parent >= 0 {
            children[node.parent as usize] += 1;
        }
    }

    // 辺が入りうる名前空間は、題がその横に収まる幅にする。
    let mut crossed = vec![false; n];
    for edge in &graph.edges {
        if edge.source < 0 || edge.target < 0 {
            continue;
        }
        let a = graph.nodes[edge.source as usize].parent;
        let b = graph.nodes[edge.target as usize].parent;
        if a == b {
            continue;
        }
        for start in [a, b] {
            let mut g = start;
            while g >= 0 && !crossed[g as usize] {
                crossed[g as usize] = true;
                g = graph.nodes[g as usize].parent;
            }
        }
    }

    let base = |node: usize, label: Label, w: f64, h: f64, font_size: f64| View {
        node,
        style: ResolvedStyle::default(),
        label,
        w,
        h,
        loops: 0,
        annotations: Vec::new(),
        attributes: Vec::new(),
        methods: Vec::new(),
        title_h: 0.0,
        attributes_h: 0.0,
        font_size,
    };

    let mut views: Vec<View> = Vec::new();
    let mut cnodes: Vec<CNode> = Vec::new();
    for (i, node) in graph.nodes.iter().enumerate() {
        let mut view;
        let mut c;
        if node.is_group {
            let label = layout_label(Some(&node.label), false, measurer, size, 4000.0, 0);
            view = base(i, label, 0.0, 0.0, size);
            if children[i] > 0 {
                c = cnode(0.0, 0.0, -1);
                c.is_group = true;
                c.pad_x = GROUP_PAD;
                c.pad_top = if view.label.height > 0.0 {
                    view.label.height + 16.0
                } else {
                    GROUP_PAD
                };
                c.pad_bottom = GROUP_PAD;
                c.min_w = view.label.width + 2.0 * GROUP_PAD - 12.0
                    + if crossed[i] && !tight {
                        js::min(view.label.width + 24.0, 240.0)
                    } else {
                        0.0
                    };
            } else {
                view.w = js::max(view.label.width + 2.0 * GROUP_PAD, 80.0);
                view.h = view.label.height + 2.0 * GROUP_PAD;
                c = cnode(view.w, view.h, -1);
            }
        } else if let Some(k) = node.class_node {
            view = class_view(
                i,
                &db.classes[k],
                db,
                &mut styles,
                &ClassViewEnv {
                    size,
                    tiny,
                    tiny_height,
                    hide_empty,
                    measurer,
                },
                &mono,
                &math_text,
            );
            c = cnode(view.w, view.h, -1);
        } else if node.shape == NodeShape::Note {
            let label = layout_label(Some(&node.label), true, measurer, small, 260.0, 0);
            let w = js::max(label.width + 2.0 * PAD_X, 40.0);
            let h = label.height + 2.0 * TITLE_PAD;
            view = base(i, label, w, h, small);
            c = cnode(view.w, view.h, -1);
        } else {
            let label = layout_label(Some(&node.label), false, measurer, small, 200.0, 0);
            let w = js::max(label.width, 12.0) + 4.0;
            let h = js::max(label.height, 12.0);
            view = base(i, label, w, h, small);
            c = cnode(view.w, view.h, -1);
        }
        c.parent = node.parent;
        c.seq = i as f64;
        views.push(view);
        cnodes.push(c);
    }

    let mut cedges: Vec<CEdge> = Vec::new();
    let mut drawn: Vec<&GraphEdge> = Vec::new();
    let mut edge_labels: Vec<Label> = Vec::new();
    let mut start_room: Vec<f64> = Vec::new();
    let mut end_room: Vec<f64> = Vec::new();
    // 辺はクラスの 1 辺から出て、反対の辺に着く。辺の多いクラスは、辺とその端の文字が並んで収まるまで広げる。
    let mut crowd = vec![0f64; views.len() * 4];
    let meet = |crowd: &mut [f64], item: usize, slot: usize, room: f64| {
        crowd[item * 4 + slot] += 1.0;
        if room > crowd[item * 4 + slot + 1] {
            crowd[item * 4 + slot + 1] = room;
        }
    };
    for edge in &graph.edges {
        if edge.source < 0 || edge.target < 0 {
            continue;
        }
        let (source, target) = (edge.source as usize, edge.target as usize);
        let label = layout_label(edge.label.as_deref(), true, measurer, small, 200.0, 0);
        let start_w = card_width(&edge.start_label_right);
        let end_w = card_width(&edge.end_label_left);
        if source == target {
            views[source].loops += 1;
            cnodes[source].w += 2.0
                * (LOOP
                    + if label.width > 0.0 {
                        label.width + 12.0
                    } else {
                        0.0
                    });
        } else {
            meet(&mut crowd, source, 0, start_w);
            meet(&mut crowd, target, 2, end_w);
        }
        start_room.push(start_w);
        end_room.push(end_w);
        drawn.push(edge);
        cedges.push(CEdge {
            src: source,
            dst: target,
            minlen: 1.0,
            label_w: if label.width > 0.0 {
                label.width + 12.0
            } else {
                0.0
            },
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

    let vertical = root_dir == Dir::TB || root_dir == Dir::BT;
    if vertical {
        for i in 0..views.len() {
            if graph.nodes[views[i].node].class_node.is_none() {
                continue;
            }
            let mut need: f64 = 0.0;
            for slot in [0, 2] {
                let count = crowd[i * 4 + slot];
                let room = crowd[i * 4 + slot + 1];
                if count > 1.0 {
                    need = js::max(
                        need,
                        (count - 1.0) * (if room > 0.0 { room + 24.0 } else { 14.0 }) + 24.0,
                    );
                }
            }
            need = js::min(need, 480.0);
            if need > views[i].w {
                cnodes[i].w += need - views[i].w;
                views[i].w = need;
            }
        }
    }

    let lettered = (0..drawn.len()).any(|i| start_room[i] > 0.0 || end_room[i] > 0.0);
    // 縦の辺の両端の多重度には、ランクの間にその高さの余地が要る。
    let mut tall_cards: f64 = 0.0;
    if vertical {
        let card_height = |raw: &str| -> f64 {
            if raw.is_empty() {
                0.0
            } else {
                js::max(tiny_height, math_text(raw, tiny).map_or(0.0, |l| l.height))
            }
        };
        for edge in &drawn {
            tall_cards = js::max(
                tall_cards,
                card_height(&edge.start_label_right) + card_height(&edge.end_label_left)
                    - 2.0 * tiny_height,
            );
        }
    }
    let node_sep = number_option(conf, "nodeSpacing", NODE_SEP);
    let layout = compound_layout(
        &mut cnodes,
        &mut cedges,
        root_dir,
        &LayeredOptions {
            node_sep,
            edge_sep: EDGE_SEP,
            rank_sep: number_option(
                conf,
                "rankSpacing",
                if lettered { RANK_SEP + 16.0 } else { RANK_SEP },
            ) + tall_cards,
            port_sep: 20.0,
            apart: true,
            ..Default::default()
        },
    );

    // どの辺も辺の真ん中に着く。いくつかが 1 辺を分け合うと、来る所の順にその辺に沿って広げる。文字のある端は、
    // 文字の長さだけまっすぐ走り、文字は線がその後に曲がる側、反対に曲がる隣から離れた側に置く。隣どうしは間の
    // 文字が収まるだけ離す。
    let mut sides: Vec<(usize, Vec<End>)> = Vec::new();
    let mut side_of: HashMap<usize, usize> = HashMap::new();
    let mut attach = |cedges: &mut [CEdge],
                      e: usize,
                      at: usize,
                      item: usize,
                      room: f64,
                      order: usize|
     -> Option<End> {
        let c = &cnodes[item];
        let route = &mut cedges[e].route;
        let side = end_side(route, at, c);
        if side < 0 {
            return None;
        }
        let slack = (c.w - views[item].w) / 2.0;
        if side == 1 {
            route[at] -= slack;
        } else if side == 3 {
            route[at] += slack;
        }
        let across = (side & 1) as usize;
        let toward = route[(if at == 0 { 3 } else { at - 3 }) + across];
        let bend = toward - route[at + across];
        let end = End {
            edge: e,
            at,
            toward,
            order,
            room,
            side,
            after: if across == 1 { bend > 0.0 } else { bend >= 0.0 },
        };
        let key = item * 4 + side as usize;
        match side_of.get(&key) {
            Some(&k) => sides[k].1.push(end),
            None => {
                side_of.insert(key, sides.len());
                sides.push((key, vec![end]));
            }
        }
        Some(end)
    };
    let mut starts: Vec<Option<End>> = Vec::new();
    let mut ends: Vec<Option<End>> = Vec::new();
    for i in 0..cedges.len() {
        let (src, dst) = (cedges[i].src, cedges[i].dst);
        let routed = src != dst && cedges[i].route.len() >= 6;
        starts.push(if routed {
            attach(&mut cedges, i, 0, src, start_room[i], i)
        } else {
            None
        });
        let last = cedges[i].route.len().wrapping_sub(3);
        ends.push(if routed {
            attach(&mut cedges, i, last, dst, end_room[i], i)
        } else {
            None
        });
    }
    // 点が、クラスのまわりの名前空間のどれかの縁にあるか。
    let on_border = |item: usize, x: f64, y: f64| -> bool {
        let mut g = cnodes[item].parent;
        let mut hops = 0;
        while g >= 0 && hops < 10000 {
            let c = &cnodes[g as usize];
            let dx = (x - c.x).abs();
            let dy = (y - c.y).abs();
            if ((dy - c.h / 2.0).abs() < 0.5 && dx <= c.w / 2.0 + 0.5)
                || ((dx - c.w / 2.0).abs() < 0.5 && dy <= c.h / 2.0 + 0.5)
            {
                return true;
            }
            g = c.parent;
            hops += 1;
        }
        false
    };
    let mut gaps: Vec<f64> = Vec::new();
    for (key, list) in &mut sides {
        if list.len() < 2 {
            continue;
        }
        let item = *key >> 2;
        let across = *key & 1;
        let length = if across == 1 {
            views[item].h
        } else {
            views[item].w
        };
        list.sort_by(|a, b| {
            let d = a.toward - b.toward;
            if d != 0.0 && !d.is_nan() {
                js::order(d)
            } else {
                a.order.cmp(&b.order)
            }
        });
        let mut total = 0.0;
        for j in 0..list.len() - 1 {
            let (a, b) = (&list[j], &list[j + 1]);
            let mut gap = if across == 1 { 20.0 } else { 24.0 };
            if a.room > 0.0 && a.after {
                gap += if across == 1 {
                    tiny_height + 6.0
                } else {
                    a.room
                };
            }
            if b.room > 0.0 && !b.after {
                gap += if across == 1 {
                    tiny_height + 6.0
                } else {
                    b.room
                };
            }
            if gaps.len() <= j {
                gaps.resize(j + 1, f64::NAN);
            }
            gaps[j] = gap;
            total += gap;
        }
        let scale = js::min(1.0, (length - 24.0) / total);
        if scale <= 0.0 {
            continue;
        }
        let mut at = (if across == 1 {
            cnodes[item].y
        } else {
            cnodes[item].x
        }) - (total * scale) / 2.0;
        for j in 0..list.len() {
            let end = list[j].at as isize;
            let route = &mut cedges[list[j].edge].route;
            // クラスの名前空間の縁を近くで越える辺は、そこまでまっすぐ走る: 越える点を端にそろえる。
            let step: isize = if end == 0 { 3 } else { -3 };
            let first = end + step;
            let ac = across as isize;
            let len = route.len() as isize;
            let mut border: isize = -1;
            let mut k = first;
            while k >= 0
                && k < len
                && (route[(k + ac) as usize] - route[(first + ac) as usize]).abs() < 0.5
            {
                if on_border(item, route[k as usize], route[k as usize + 1]) {
                    border = k;
                    break;
                }
                k += step;
            }
            if border >= 0 && (route[(first + ac) as usize] - at).abs() <= 24.0 {
                let mut k = first;
                while k != border + step {
                    route[(k + ac) as usize] = at;
                    k += step;
                }
            }
            route[(end + ac) as usize] = at;
            at += gaps.get(j).copied().unwrap_or(f64::NAN) * scale;
        }
    }

    let straighten = |route: &mut Vec<f64>, end: Option<End>, room: f64, first: bool| {
        let Some(end) = end else {
            return;
        };
        if room <= 0.0 {
            return;
        }
        let across = (end.side & 1) as usize;
        let at = if first { 0 } else { route.len() - 3 };
        let along = at + 1 - across;
        let reach = (route[(if first { 3 } else { at - 3 }) + 1 - across] - route[along]).abs();
        let length = js::min(
            if across == 1 {
                room + 10.0
            } else {
                tiny_height + 6.0
            },
            reach * 0.35,
        );
        if length < 4.0 {
            return;
        }
        let mut point = [route[at], route[at + 1], route[at + 2]];
        point[1 - across] += (if end.side == 0 || end.side == 3 {
            -1.0
        } else {
            1.0
        }) * length;
        let place = if first { 3 } else { at };
        route.splice(place..place, point);
    };
    for i in 0..cedges.len() {
        straighten(&mut cedges[i].route, starts[i], start_room[i], true);
        straighten(&mut cedges[i].route, ends[i], end_room[i], false);
    }

    let mut min_x: f64 = 0.0;
    let mut min_y: f64 = 0.0;
    let mut max_x = layout.width;
    let mut max_y = layout.height;
    let mut cards: Vec<Card> = Vec::new();
    let mut card = |parent: usize,
                    raw: &str,
                    width: f64,
                    x: f64,
                    y: f64,
                    side: i32,
                    after: bool,
                    bounds: &mut [f64; 4]| {
        if raw.is_empty() {
            return;
        }
        let math = math_text(raw, tiny);
        let h = match &math {
            Some(m) => js::max(tiny_height, m.height),
            None => tiny_height,
        };
        let mut cx = x;
        let mut cy = y;
        if side & 1 == 1 {
            cx += (if side == 3 { -1.0 } else { 1.0 }) * (5.0 + width / 2.0);
            cy += (if after { 1.0 } else { -1.0 }) * (7.0 + h / 2.0);
        } else {
            cx += (if after { 1.0 } else { -1.0 }) * (9.0 + width / 2.0);
            cy += (if side == 0 { -1.0 } else { 1.0 }) * (3.0 + h / 2.0);
        }
        cards.push(Card {
            text: decode_entities(raw),
            math,
            x: cx,
            y: cy,
            w: width,
            h,
            parent: parent as i64,
        });
        bounds[0] = js::min(bounds[0], cx - width / 2.0);
        bounds[2] = js::max(bounds[2], cx + width / 2.0);
        bounds[1] = js::min(bounds[1], cy - h / 2.0);
        bounds[3] = js::max(bounds[3], cy + h / 2.0);
    };
    let mut bounds = [min_x, min_y, max_x, max_y];
    let mut loop_seen: HashMap<usize, usize> = HashMap::new();
    let mut loop_at = vec![0usize; cedges.len()];
    for i in 0..cedges.len() {
        let edge = drawn[i];
        let (src, dst) = (cedges[i].src, cedges[i].dst);
        if src == dst {
            let k = loop_seen.get(&src).copied().unwrap_or(0);
            loop_seen.insert(src, k + 1);
            loop_at[i] = k;
            let (cx, cy) = (cnodes[src].x, cnodes[src].y);
            let view = &views[src];
            let x = cx + view.w / 2.0;
            let spread = js::min(view.h / 2.0 - 4.0, 8.0 + k as f64 * 6.0);
            card(
                src,
                &edge.start_label_right,
                start_room[i],
                x,
                cy - spread,
                1,
                false,
                &mut bounds,
            );
            card(
                dst,
                &edge.end_label_left,
                end_room[i],
                x,
                cy + spread,
                1,
                true,
                &mut bounds,
            );
            cedges[i].label_x =
                cx + view.w / 2.0 + LOOP + k as f64 * 8.0 + 4.0 + cedges[i].label_w / 2.0;
            cedges[i].label_y = cy;
        } else if cedges[i].route.len() >= 6 {
            let route = &cedges[i].route;
            let last = route.len() - 3;
            let (from, to) = (starts[i], ends[i]);
            card(
                src,
                &edge.start_label_right,
                start_room[i],
                route[0],
                route[1],
                from.map_or(2, |f| f.side),
                from.is_none_or(|f| f.after),
                &mut bounds,
            );
            card(
                dst,
                &edge.end_label_left,
                end_room[i],
                route[last],
                route[last + 1],
                to.map_or(0, |t| t.side),
                to.is_none_or(|t| t.after),
                &mut bounds,
            );
        }
    }
    [min_x, min_y, max_x, max_y] = bounds;

    if let Some(available) = options.max_width.filter(|&a| a > 0.0)
        && max_x - min_x + 2.0 * pad > available
    {
        let mut all = cnodes.clone();
        for item in &cards {
            let mut c = cnode(item.w, item.h, item.parent);
            c.x = item.x;
            c.y = item.y;
            all.push(c);
        }
        let wrapped = wrap_components(
            &mut all,
            &mut cedges,
            js::max(1.0, available - 2.0 * pad),
            node_sep,
        );
        if let Some(wrapped) = wrapped {
            let count = cnodes.len();
            for (i, c) in cnodes.iter_mut().enumerate() {
                c.x = all[i].x;
                c.y = all[i].y;
            }
            for (i, item) in cards.iter_mut().enumerate() {
                item.x = all[count + i].x;
                item.y = all[count + i].y;
            }
            min_x = 0.0;
            min_y = 0.0;
            max_x = wrapped.width;
            max_y = wrapped.height;
        }
    }

    let ox = pad - min_x;
    let oy = pad - min_y;
    let width = (max_x - min_x + 2.0 * pad).ceil();
    let height = (max_y - min_y + 2.0 * pad).ceil();
    let mut links: Vec<LinkInfo> = Vec::new();

    let mut crossings: Crossings = HashMap::new();
    if graph.nodes.first().is_some_and(|n| n.is_group) {
        for e in &cedges {
            mark_crossings(&mut crossings, &e.route);
        }
    }

    let env = SvgEnv {
        size,
        tiny,
        tiny_height,
        options,
    };
    let mut clusters = String::new();
    let mut struck_titles = String::new();
    let mut nodes_out = String::new();
    for i in 0..views.len() {
        let view = &views[i];
        let c = &cnodes[i];
        let node: &GraphNode = &graph.nodes[view.node];
        let x = c.x + ox;
        let y = c.y + oy;
        let id = esc_text(&node.id);
        let w = if c.is_group { c.w } else { view.w };
        let h = if c.is_group { c.h } else { view.h };
        if node.is_group {
            let title_x = if c.is_group {
                cluster_title_x(c, view.label.width, &crossings, GROUP_PAD)
            } else {
                c.x
            };
            let title_y = y - h / 2.0 + 8.0 + view.label.height / 2.0;
            let title_attrs = " class=\"pele-cluster-label\" fill=\"var(--_m)\"";
            let struck = c.is_group && title_crossed(c, view.label.width, title_x, &crossings);
            if struck {
                struck_titles.push_str(&struck_title(
                    &view.label,
                    title_x + ox,
                    title_y,
                    title_attrs,
                    &id,
                    &cnodes,
                    i,
                    None,
                ));
            }
            clusters.push_str(&format!(
                "<g class=\"pele-cluster pele-namespace\" data-id=\"{id}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_a)\" fill-opacity=\"0.5\" stroke=\"var(--_b)\"{ROUNDED}/>{}</g>",
                num(x - w / 2.0),
                num(y - h / 2.0),
                num(w),
                num(h),
                if struck {
                    String::new()
                } else {
                    label_svg(&view.label, title_x + ox, title_y, title_attrs, None)
                }
            ));
            continue;
        }
        let at = format!(" transform=\"translate({},{})\"", num(x), num(y));
        if let Some(k) = node.class_node {
            nodes_out.push_str(&class_svg(
                view,
                &db.classes[k],
                &id,
                &at,
                &env,
                &mono,
                &mut links,
            ));
        } else if node.shape == NodeShape::Note {
            let r = w / 2.0;
            let t = -h / 2.0;
            nodes_out.push_str(&format!(
                "<g class=\"pele-node pele-note\" data-id=\"{id}\"{at}><path d=\"M{},{}H{}L{},{}V{}H{}Z\" fill=\"var(--_a)\" stroke=\"var(--_b)\" stroke-linejoin=\"round\"/><path d=\"M{},{}V{}H{}\" fill=\"none\" stroke=\"var(--_b)\" stroke-linejoin=\"round\"/>{}</g>",
                num(-r),
                num(t),
                num(r - FOLD),
                num(r),
                num(t + FOLD),
                num(-t),
                num(-r),
                num(r - FOLD),
                num(t),
                num(t + FOLD),
                num(r),
                label_svg(
                    &view.label,
                    0.0,
                    0.0,
                    &format!(" class=\"pele-label\" font-size=\"{}\"", js::fmt(small)),
                    icons
                )
            ));
        } else {
            nodes_out.push_str(&format!(
                "<g class=\"pele-node pele-interface\" data-id=\"{id}\"{at}>{}</g>",
                label_svg(
                    &view.label,
                    0.0,
                    0.0,
                    &format!(" class=\"pele-label\" font-size=\"{}\"", js::fmt(small)),
                    icons
                )
            ));
        }
    }

    let mut edges_out = String::new();
    let mut labels_out = String::new();
    for i in 0..cedges.len() {
        let edge = drawn[i];
        let label = &edge_labels[i];
        let start_type = edge.arrow_type_start;
        let end_type = edge.arrow_type_end;
        let start_trim = class_marker_trim(start_type);
        let end_trim = class_marker_trim(end_type);
        let (src, dst) = (cedges[i].src, cedges[i].dst);
        let path: EdgePath = if src == dst {
            let c = &cnodes[src];
            let view = &views[src];
            let k = loop_at[i] as f64;
            let x = c.x + ox + view.w / 2.0;
            let y = c.y + oy;
            let spread = js::min(view.h / 2.0 - 4.0, 8.0 + k * 6.0);
            let reach = LOOP + k * 8.0;
            EdgePath {
                d: format!(
                    "M{},{}C{},{} {},{} {},{}",
                    num(x + start_trim),
                    num(y - spread),
                    num(x + reach + start_trim),
                    num(y - spread),
                    num(x + reach + end_trim),
                    num(y + spread),
                    num(x + end_trim),
                    num(y + spread)
                ),
                tokens: None,
                sx: x,
                sy: y - spread,
                sdx: -1.0,
                sdy: 0.0,
                ex: x,
                ey: y + spread,
                edx: -1.0,
                edy: 0.0,
            }
        } else {
            let route = &mut cedges[i].route;
            let mut k = 0;
            while k + 1 < route.len() {
                route[k] += ox;
                route[k + 1] += oy;
                k += 3;
            }
            route_path(route, None, start_trim, end_trim, None, None)
        };
        let attrs = match edge.pattern {
            Pattern::Dashed => " stroke-dasharray=\"6 4\"",
            Pattern::Dotted => " stroke-dasharray=\"2 4\"",
            Pattern::Solid => "",
        };
        edges_out.push_str(&format!(
            "<g class=\"pele-edge{}\" data-id=\"{}\"><path d=\"{}\"{attrs}/>{}{}</g>",
            if edge.pattern == Pattern::Dotted {
                " pele-note-edge"
            } else {
                " pele-relation"
            },
            esc_text(&edge.id),
            path.d,
            class_marker(start_type, path.sx, path.sy, path.sdx, path.sdy, LINE),
            class_marker(end_type, path.ex, path.ey, path.edx, path.edy, LINE)
        ));
        if label.width > 0.0 {
            let x = cedges[i].label_x + ox;
            let y = cedges[i].label_y + oy;
            labels_out.push_str(&edge_label_svg(&edge.id, label, x, y, "", icons));
        }
    }

    let mut cards_out = String::new();
    for item in &cards {
        match &item.math {
            Some(math) => cards_out.push_str(&label_svg(
                math,
                item.x + ox,
                item.y + oy,
                " class=\"pele-cardinality\"",
                icons,
            )),
            None => cards_out.push_str(&format!(
                "<text class=\"pele-cardinality\" x=\"{}\" y=\"{}\" text-anchor=\"middle\">{}</text>",
                num(item.x + ox),
                num(item.y + oy + tiny * 0.35),
                esc(&item.text)
            )),
        }
    }

    let mut content = String::new();
    if !clusters.is_empty() {
        content.push_str(&format!("<g class=\"pele-clusters\">{clusters}</g>"));
    }
    if !edges_out.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-edges\" fill=\"none\" stroke=\"{LINE}\" stroke-linecap=\"round\">{edges_out}</g>"
        ));
    }
    if !struck_titles.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-cluster-titles\">{struck_titles}</g>"
        ));
    }
    if !labels_out.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-edge-labels\" font-size=\"{}\">{labels_out}</g>",
            js::fmt(small)
        ));
    }
    if !cards_out.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-cardinalities\" font-size=\"{}\" fill=\"var(--_m)\">{cards_out}</g>",
            js::fmt(tiny)
        ));
    }
    content.push_str(&format!("<g class=\"pele-nodes\">{nodes_out}</g>"));
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
        "class",
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

struct ClassViewEnv<'a> {
    size: f64,
    tiny: f64,
    tiny_height: f64,
    hide_empty: bool,
    measurer: &'a dyn TextMeasurer,
}

#[allow(clippy::too_many_arguments)]
fn class_view(
    node: usize,
    cls: &ClassNode,
    db: &ClassDb,
    styles: &mut Styles,
    env: &ClassViewEnv,
    mono: &dyn Fn(&ResolvedStyle) -> bool,
    math_text: &dyn Fn(&str, f64) -> Option<Label>,
) -> View {
    let measurer = env.measurer;
    let st = styles.class_style(db, cls);
    let font_size = st.font_size.unwrap_or(env.size);
    let member_size = js::round(font_size * 0.875);
    let line_height = js::round(member_size * 1.5);
    // `<T>` をタグと読まないよう、総称の型は符号にする。
    let generic = if cls.type_.is_empty() {
        String::new()
    } else {
        format!(
            "&lt;{}&gt;",
            parse_generic_types(&cls.type_)
                .replace('<', "&lt;")
                .replace('>', "&gt;")
        )
    };
    let label = layout_label(
        Some(&format!("{}{generic}", cls.label)),
        false,
        measurer,
        font_size,
        4000.0,
        style::BOLD | if st.italic { style::ITALIC } else { 0 },
    );
    let annotations: Vec<Annotation> = cls
        .annotations
        .iter()
        .map(
            |a| match math_text(&format!("«{}»", js::trim(a)), env.tiny) {
                Some(l) => Annotation::Math(l),
                None => Annotation::Text(format!("«{}»", js::trim(&decode_entities(a)))),
            },
        )
        .collect();
    let m = mono(&st);
    let attributes: Vec<Line> = cls
        .members
        .iter()
        .map(|x| member_line(x, measurer, member_size, m))
        .collect();
    let methods: Vec<Line> = cls
        .methods
        .iter()
        .map(|x| member_line(x, measurer, member_size, m))
        .collect();
    let mut inner = label.width;
    let mut annotations_h = 0.0;
    for a in &annotations {
        match a {
            Annotation::Text(s) => {
                inner = js::max(inner, measurer.width(s, env.tiny, 0));
                annotations_h += env.tiny_height;
            }
            Annotation::Math(l) => {
                inner = js::max(inner, l.width);
                annotations_h += js::max(env.tiny_height, l.height);
            }
        }
    }
    for line in &attributes {
        inner = js::max(inner, line.width);
    }
    for line in &methods {
        inner = js::max(inner, line.width);
    }
    let sections = !(env.hide_empty && attributes.is_empty() && methods.is_empty());
    let section = |count: usize| -> f64 {
        if !sections {
            0.0
        } else if count > 0 {
            2.0 * SECTION_PAD + count as f64 * line_height
        } else {
            EMPTY_SECTION
        }
    };
    let title_h =
        2.0 * TITLE_PAD + annotations_h + js::max(label.height, js::round(font_size * 1.5));
    let attributes_h = section(attributes.len());
    View {
        node,
        w: js::max(inner + 2.0 * PAD_X, MIN_WIDTH),
        h: title_h + attributes_h + section(methods.len()),
        style: st,
        label,
        loops: 0,
        annotations,
        attributes,
        methods,
        title_h,
        attributes_h,
        font_size,
    }
}

struct SvgEnv<'a> {
    size: f64,
    tiny: f64,
    tiny_height: f64,
    options: &'a RenderOptions,
}

fn class_svg(
    view: &View,
    cls: &ClassNode,
    id: &str,
    at: &str,
    env: &SvgEnv,
    mono: &dyn Fn(&ResolvedStyle) -> bool,
    links: &mut Vec<LinkInfo>,
) -> String {
    let (w, h, st) = (view.w, view.h, &view.style);
    let icons = env.options.icons.as_ref();
    let left = -w / 2.0;
    let top = -h / 2.0;
    let member_size = js::round(view.font_size * 0.875);
    let line_height = js::round(member_size * 1.5);
    let mut body = format!(
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_s)\" stroke=\"var(--_b)\"{}/>",
        num(left),
        num(top),
        num(w),
        num(h),
        rounded(&st.shape)
    );
    let ink = ink_fill(&st.shape, &st.text, None);
    let first = top + view.title_h;
    let second = first + view.attributes_h;
    if h > view.title_h {
        let stroke = match &st.stroke {
            Some(s) if !s.is_empty() => format!(" style=\"stroke:{}\"", esc(s)),
            _ => String::new(),
        };
        body.push_str(&format!(
            "<path class=\"pele-divider\" d=\"M{},{}H{}M{},{}H{}\" fill=\"none\" stroke=\"var(--_b)\"{stroke}/>",
            num(left),
            num(first),
            num(-left),
            num(left),
            num(second),
            num(-left)
        ));
    }
    let ink_or_muted = if ink.is_empty() {
        " fill=\"var(--_m)\"".to_string()
    } else {
        ink.clone()
    };
    let mut y = top + TITLE_PAD;
    for annotation in &view.annotations {
        match annotation {
            Annotation::Text(s) => {
                body.push_str(&format!(
                    "<text class=\"pele-annotation\" x=\"0\" y=\"{}\" text-anchor=\"middle\" font-size=\"{}\"{ink_or_muted}>{}</text>",
                    num(y + env.tiny_height / 2.0 + env.tiny * 0.35),
                    js::fmt(env.tiny),
                    esc(s)
                ));
                y += env.tiny_height;
            }
            Annotation::Math(l) => {
                let h = js::max(env.tiny_height, l.height);
                body.push_str(&label_svg(
                    l,
                    0.0,
                    y + h / 2.0,
                    &format!(
                        " class=\"pele-annotation\" font-size=\"{}\"{ink_or_muted}",
                        js::fmt(env.tiny)
                    ),
                    icons,
                ));
                y += h;
            }
        }
    }
    let title_size = if view.font_size == env.size {
        String::new()
    } else {
        format!(" font-size=\"{}\"", num(view.font_size))
    };
    body.push_str(&label_svg(
        &view.label,
        0.0,
        (y + first - TITLE_PAD) / 2.0,
        &format!(
            " class=\"pele-label\"{title_size} font-weight=\"var(--_hw)\"{ink}{}",
            st.text
        ),
        icons,
    ));
    let lines = |list: &[Line], kind: &str, from: f64| -> String {
        let mut out = String::new();
        let mut baseline = from + SECTION_PAD + line_height / 2.0 + member_size * 0.35;
        for line in list {
            let mark = match line.classifier.as_str() {
                "$" => format!(
                    " pele-static\"{}",
                    if st.text.is_empty() {
                        " style=\"text-decoration:underline\"".to_string()
                    } else {
                        format!(
                            "{}text-decoration:underline\"",
                            &st.text[..st.text.len() - 1]
                        )
                    }
                ),
                "*" => format!(" pele-abstract\" font-style=\"italic\"{}", st.text),
                _ => format!("\"{}", st.text),
            };
            out.push_str(&format!(
                "<text class=\"pele-member pele-{kind}{mark} x=\"{}\" y=\"{}\">{}</text>",
                num(left + PAD_X),
                num(baseline),
                esc(&line.text)
            ));
            baseline += line_height;
        }
        out
    };
    if !view.attributes.is_empty() || !view.methods.is_empty() {
        body.push_str(&format!(
            "<g class=\"pele-members\" font-size=\"{}\"{ink}{}>{}{}</g>",
            js::fmt(member_size),
            if mono(st) {
                format!(" font-family=\"{FONT_MONO}\"")
            } else {
                String::new()
            },
            lines(&view.attributes, "attribute", first),
            lines(&view.methods, "method", second)
        ));
    }
    if let Some(tip) = cls.tooltip.as_deref().filter(|t| !t.is_empty()) {
        body = format!("<title>{}</title>{body}", esc_text(tip));
    }
    if let Some(link) = cls.link.as_deref().filter(|l| !l.is_empty()) {
        let target = match cls.link_target.as_deref() {
            Some(t) if TARGETS.contains(&t) => format!(" target=\"{t}\""),
            _ => String::new(),
        };
        body = linked(&body, link, &cls.id, links, env.options, &target);
    }
    format!(
        "<g class=\"pele-node pele-class{}\" data-id=\"{id}\"{at}>{body}</g>",
        class_names(strip_default(&cls.css_classes))
    )
}
