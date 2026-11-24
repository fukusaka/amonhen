//! モデルを配置して SVG を書く: ノード（形・ラベル・record・HTML）、辺（道筋・矢じり・ラベル）、クラスタ、
//! グラフの題と地。色は文が決めたものを使い、決めないものはテーマの変数（`var(--_l)` など）にする。

use std::collections::{BTreeSet, HashMap};

use amonhen_pele::svg::builder::{esc, num};
use amonhen_pele::svg::root::{Accessible, svg_document};

use crate::Note;
use crate::arrows;
use crate::color::{self, Color, Rgba};
use crate::curve::{self, Cubic, P};
use crate::graph::{Attrs, Model};
use crate::layout::{self, LabelSide, Route};
use crate::record::{self, Field};
use crate::route;
use crate::shapes::{self, Geom, Kind};
use crate::text::{self, Font, Names, Text, color_attr, text_svg};

/// 点（Graphviz の 1 インチ = 72）。
const INCH: f64 = 72.0;
const PAD: f64 = 8.0;
const FONT_SIZE: f64 = 14.0;
/// クラスタの中の余白。
const CLUSTER_MARGIN: f64 = 8.0;

const LINE: &str = "var(--_l)";
const BORDER: &str = "var(--_b)";
const SURFACE: &str = "var(--_s)";
const SURFACE_ALT: &str = "var(--_a)";
const MUTED: &str = "var(--_m)";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dir {
    TB,
    BT,
    LR,
    RL,
}

impl Dir {
    fn sideways(self) -> bool {
        matches!(self, Dir::LR | Dir::RL)
    }
}

/// 配置の上限。
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    pub rank_iter: usize,
    pub x_iter: usize,
    pub sweeps: usize,
}

struct Ctx<'a> {
    model: &'a Model,
    notes: &'a mut BTreeSet<Note>,
    scheme: Option<String>,
    defs: String,
    next_id: usize,
}

impl Ctx<'_> {
    fn color(&mut self, text: &str, scheme: Option<&str>) -> Option<Color> {
        let scheme = scheme.or(self.scheme.as_deref());
        match color::parse(text, scheme) {
            Some(c) => Some(c),
            None => {
                if !text.trim().is_empty() {
                    // `/scheme/name` の配色が先
                    let s = match text.trim().strip_prefix('/') {
                        Some(rest) => rest.split_once('/').map_or("", |(s, _)| s),
                        None => scheme.unwrap_or(""),
                    };
                    self.notes.insert(if color::known_scheme(s) {
                        Note::UnknownColors
                    } else {
                        Note::ColorSchemes
                    });
                }
                None
            }
        }
    }

    /// 色の並びの最初の読める色。
    fn first_color(&mut self, text: &str, scheme: Option<&str>) -> Option<Color> {
        let stops = color::parse_list(text, scheme.or(self.scheme.as_deref()));
        if stops.iter().any(|s| s.color.is_none()) {
            // 読めない名前を注記（最初の読めないもので）
            for part in text.split(':') {
                let name = part.split(';').next().unwrap_or("");
                if !name.trim().is_empty() {
                    let _ = self.color(name, scheme);
                }
            }
        }
        stops.into_iter().find_map(|s| s.color)
    }

    fn id(&mut self) -> String {
        self.next_id += 1;
        format!("dot-{}", self.next_id)
    }
}

/// 色の属性の値（`#rrggbb` と透明度）。
fn paint(name: &str, c: Option<Color>, fallback: &str) -> String {
    match c {
        Some(c) => color_attr(name, &c),
        None => format!(" {name}=\"{fallback}\""),
    }
}

fn styles(s: Option<&str>) -> Vec<String> {
    let Some(s) = s else { return vec![] };
    let mut out = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    for c in s.chars() {
        match c {
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth -= 1;
                cur.push(c);
            }
            ',' if depth <= 0 => {
                out.push(cur.trim().to_ascii_lowercase());
                cur.clear();
            }
            c => cur.push(c),
        }
    }
    out.push(cur.trim().to_ascii_lowercase());
    out.retain(|s| !s.is_empty());
    out
}

fn has(st: &[String], name: &str) -> bool {
    st.iter().any(|s| s == name)
}

/// 線の太さ（penwidth と style の bold・setlinewidth）。
fn pen_width(attrs: &Attrs, st: &[String]) -> f64 {
    let mut w = attrs.num("penwidth").unwrap_or(1.0).clamp(0.0, 50.0);
    if has(st, "bold") {
        w = w.max(2.0);
    }
    for s in st {
        if let Some(v) = s
            .strip_prefix("setlinewidth(")
            .and_then(|r| r.strip_suffix(')'))
            .and_then(|v| v.trim().parse::<f64>().ok())
            .filter(|v| v.is_finite())
        {
            w = v.clamp(0.0, 50.0);
        }
    }
    w
}

fn line_attrs(st: &[String], pen: f64) -> String {
    let mut s = String::new();
    if (pen - 1.0).abs() > 1e-9 {
        s.push_str(&format!(" stroke-width=\"{}\"", num(pen)));
    }
    if has(st, "dashed") {
        s.push_str(" stroke-dasharray=\"5,2\"");
    } else if has(st, "dotted") {
        s.push_str(" stroke-dasharray=\"1,5\"");
    }
    s
}

fn font_of(attrs: &Attrs, prefix: &str, ctx: &mut Ctx, base: Option<&Font>) -> Font {
    let size_key = format!("{prefix}fontsize");
    let name_key = format!("{prefix}fontname");
    let color_key = format!("{prefix}fontcolor");
    let mut f = base.cloned().unwrap_or_else(|| Font::new(FONT_SIZE));
    if let Some(s) = attrs.num(&size_key).filter(|v| *v > 0.0) {
        f.size = s.clamp(1.0, 400.0);
    }
    if let Some(n) = attrs.get(&name_key) {
        f = f.with_name(n);
    }
    if let Some(c) = attrs.get(&color_key) {
        let scheme = attrs.get("colorscheme").map(str::to_owned);
        f.color = ctx.color(c, scheme.as_deref());
    }
    f
}

/// ラベル（HTML かふつう）。
fn label_text(
    raw: &crate::parse::Value,
    names: &Names,
    font: &Font,
    ctx: &mut Ctx,
    scheme: Option<&str>,
) -> Text {
    if raw.html {
        let t = text::html_label(&raw.text, font, scheme.or(ctx.scheme.as_deref()));
        if t.table {
            ctx.notes.insert(Note::HtmlTables);
        }
        if t.image {
            ctx.notes.insert(Note::Images);
        }
        t
    } else {
        text::plain_label(&raw.text, names, font)
    }
}

struct NodeView {
    geom: Geom,
    label: Option<Text>,
    record: Option<Field>,
    xlabel: Option<Text>,
    font: Font,
    invis: bool,
    st: Vec<String>,
    stroke: String,
    fill: String,
    rounded: bool,
    diagonals: bool,
    special: Option<&'static str>,
    /// 塗りが濃いときの文字の色（文が決めていなければ）。
    ink: Option<&'static str>,
    /// ラベルを寄せる幅。
    inner_w: f64,
    pos: P,
}

struct Port {
    /// ノードの中心からの点。
    at: P,
    /// 線を止める箱（ノードの中心から。None なら止めない）。
    clip: Option<[f64; 4]>,
    /// 向きのあるポート（`n` など）の出る向き。
    dir: Option<P>,
}

fn unit(d: P) -> P {
    let l = d.0.hypot(d.1).max(1e-9);
    (d.0 / l, d.1 / l)
}

fn compass(name: &str) -> Option<P> {
    Some(match name.trim() {
        "n" => (0.0, -1.0),
        "ne" => (1.0, -1.0),
        "e" => (1.0, 0.0),
        "se" => (1.0, 1.0),
        "s" => (0.0, 1.0),
        "sw" => (-1.0, 1.0),
        "w" => (-1.0, 0.0),
        "nw" => (-1.0, -1.0),
        "c" | "_" => (0.0, 0.0),
        _ => return None,
    })
}

fn port_of(view: &NodeView, port: Option<&str>) -> Option<Port> {
    let port = port?.trim();
    if port.is_empty() {
        return None;
    }
    let (name, dir) = match port.split_once(':') {
        Some((a, b)) => (a, Some(b)),
        None => (port, None),
    };
    let (w, h) = view.geom.outer();
    let on_box = |b: [f64; 4], d: P| -> P {
        let (cx, cy) = ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
        (
            cx + d.0 * (b[2] - b[0]) / 2.0,
            cy + d.1 * (b[3] - b[1]) / 2.0,
        )
    };
    if let Some(field) = view.record.as_ref().and_then(|r| record::port(r, name)) {
        let b = [field.x, field.y, field.x + field.w, field.y + field.h];
        return Some(match dir.and_then(compass) {
            Some(d) if d != (0.0, 0.0) => Port {
                at: on_box(b, d),
                clip: None,
                dir: Some(unit(d)),
            },
            _ => Port {
                at: ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0),
                clip: Some(b),
                dir: None,
            },
        });
    }
    let d = compass(dir.unwrap_or(name))?;
    if d == (0.0, 0.0) {
        return None;
    }
    // 箱の角に向けた線が外形と交わる所
    let target = (d.0 * w / 2.0, d.1 * h / 2.0);
    Some(Port {
        at: view.geom.boundary(target.0, target.1),
        clip: None,
        dir: Some(unit(d)),
    })
}

/// 描く。`Budget` は配置の上限。
pub fn render(model: &Model, budget: Budget, notes: &mut BTreeSet<Note>) -> String {
    let g = &model.attrs;
    let mut ctx = Ctx {
        model,
        notes,
        scheme: g.get("colorscheme").map(str::to_owned),
        defs: String::new(),
        next_id: 0,
    };
    let dir = match g.get("rankdir").map(|s| s.trim().to_ascii_uppercase()) {
        Some(ref s) if s == "LR" => Dir::LR,
        Some(ref s) if s == "RL" => Dir::RL,
        Some(ref s) if s == "BT" => Dir::BT,
        _ => Dir::TB,
    };
    if let Some(l) = g.get("layout").map(|s| s.trim().to_ascii_lowercase())
        && !l.is_empty()
        && l != "dot"
    {
        ctx.notes.insert(Note::OtherLayouts);
    }
    let splines = g
        .get("splines")
        .map(|s| s.trim().to_ascii_lowercase())
        .unwrap_or_default();
    if splines == "ortho" {
        ctx.notes.insert(Note::Ortho);
    }
    let draw_edges =
        !matches!(splines.as_str(), "none" | "\"\"") && !(g.get("splines") == Some(""));
    let straight = matches!(splines.as_str(), "line" | "false" | "no");
    let polyline = matches!(splines.as_str(), "polyline" | "ortho");
    let curved = splines == "curved";
    let nodesep = g
        .num("nodesep")
        .map_or(0.25 * INCH, |v| v.max(0.02) * INCH)
        .min(10.0 * INCH);
    let ranksep = g
        .get("ranksep")
        .and_then(|s| {
            s.split_whitespace()
                .next()
                .and_then(|v| v.parse::<f64>().ok())
        })
        .filter(|v| v.is_finite())
        .map_or(0.5 * INCH, |v| v.max(0.02) * INCH)
        .min(20.0 * INCH);
    let graph_font = font_of(g, "", &mut ctx, None);

    // ---- ノード ----
    let mut views: Vec<NodeView> = Vec::with_capacity(model.nodes.len());
    for node in &model.nodes {
        let a = &node.attrs;
        let scheme = a.get("colorscheme").map(str::to_owned);
        let font = font_of(a, "", &mut ctx, None);
        let st = styles(a.get("style"));
        let shape_name = a.get("shape").unwrap_or("ellipse");
        let mut spec = shapes::spec(
            shape_name,
            a.num("sides"),
            a.num("orientation").unwrap_or(0.0),
            a.num("distortion").unwrap_or(0.0).clamp(-10.0, 10.0),
            a.num("skew").unwrap_or(0.0).clamp(-10.0, 10.0),
            a.flag("regular").unwrap_or(false),
        );
        if spec.unknown {
            ctx.notes.insert(Note::UnknownShapes);
        }
        if a.get("image").is_some_and(|s| !s.is_empty()) || shape_name.eq_ignore_ascii_case("epsf")
        {
            ctx.notes.insert(Note::Images);
        }
        if let Some(p) = a.num("peripheries") {
            spec.peripheries = p.clamp(0.0, 10.0) as usize;
        }
        let names = Names {
            graph: &model.name,
            node: &node.name,
            ..Default::default()
        };
        let default_label = crate::parse::Value::plain("\\N");
        let raw = a.value("label").unwrap_or(&default_label);
        let mut label = None;
        let mut rec = None;
        let point = spec.kind == Kind::Point;
        match &spec.kind {
            Kind::Record { .. } if !raw.html => {
                let names_l = Names {
                    graph: &model.name,
                    node: &node.name,
                    ..Default::default()
                };
                match record::parse(&raw.text, !dir.sideways(), &names_l, &font) {
                    Some(mut f) => {
                        record::size(&mut f, font.size);
                        rec = Some(f);
                    }
                    None => label = Some(text::plain_label(&raw.text, &names, &font)),
                }
            }
            _ if point => {}
            _ => label = Some(label_text(raw, &names, &font, &mut ctx, scheme.as_deref())),
        }
        // HTML の表は枠で囲む
        if label.as_ref().is_some_and(|t| t.table) && spec.kind == Kind::Plain {
            spec.kind = shapes::spec("box", None, 0.0, 0.0, 0.0, false).kind;
            spec.peripheries = 1;
        }
        let (mx, my) = if spec.no_margin {
            (0.0, 0.0)
        } else {
            match a.get("margin") {
                Some(m) => {
                    let mut it = m.split(',').filter_map(|v| v.trim().parse::<f64>().ok());
                    let x = it.next().unwrap_or(0.11).clamp(0.0, 10.0) * INCH;
                    let y = it.next().map_or(x, |y| y.clamp(0.0, 10.0) * INCH);
                    (x, y)
                }
                None => (0.11 * INCH, 0.055 * INCH),
            }
        };
        let (lw, lh) = match (&label, &rec) {
            (Some(t), _) if !t.is_empty() => (t.width + 2.0 * mx, t.height + 2.0 * my),
            (_, Some(f)) => (f.w, f.h),
            _ => (0.0, 0.0),
        };
        let min_w = a
            .num("width")
            .map_or(0.75 * INCH, |v| v.clamp(0.0, 100.0) * INCH);
        let min_h = a
            .num("height")
            .map_or(0.5 * INCH, |v| v.clamp(0.0, 100.0) * INCH);
        let fixed = a
            .get("fixedsize")
            .is_some_and(|v| v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("shape"));
        let (mut w, mut h);
        if point {
            let d = match (a.num("width"), a.num("height")) {
                (Some(x), Some(y)) => x.min(y),
                (Some(x), None) | (None, Some(x)) => x,
                (None, None) => 0.05,
            }
            .clamp(0.0, 100.0)
                * INCH;
            w = d;
            h = d;
        } else if fixed {
            w = min_w;
            h = min_h;
        } else {
            let (fw, fh) = shapes::fit(&spec.kind, lw, lh);
            let plain = spec.no_margin;
            w = if plain { fw } else { fw.max(min_w) };
            h = if plain { fh } else { fh.max(min_h) };
            if spec.regular {
                let m = w.max(h);
                w = m;
                h = m;
            }
        }
        if !w.is_finite() || !h.is_finite() {
            w = min_w;
            h = min_h;
        }
        if let Some(f) = &mut rec {
            record::place(f, -w / 2.0, -h / 2.0, w, h);
        }
        let xlabel = a
            .value("xlabel")
            .filter(|v| !v.text.is_empty())
            .map(|v| label_text(v, &names, &font, &mut ctx, scheme.as_deref()));
        let invis = has(&st, "invis");
        let pen = pen_width(a, &st);
        // 線と塗り
        let color_text = a.get("color").map(str::to_owned);
        let line_color = color_text
            .as_deref()
            .and_then(|c| ctx.first_color(c, scheme.as_deref()));
        let filled =
            has(&st, "filled") || has(&st, "radial") || has(&st, "striped") || has(&st, "wedged");
        let fill_text = a
            .get("fillcolor")
            .or(color_text.as_deref())
            .map(str::to_owned);
        let mut fill = if point {
            paint("fill", line_color, LINE)
        } else if filled {
            match fill_text.as_deref() {
                Some(t) if t.contains(':') && !t.trim().ends_with(':') => gradient(
                    &mut ctx,
                    t,
                    scheme.as_deref(),
                    has(&st, "radial"),
                    a.num("gradientangle").unwrap_or(0.0),
                ),
                Some(t) => paint("fill", ctx.first_color(t, scheme.as_deref()), SURFACE_ALT),
                None => format!(" fill=\"{SURFACE_ALT}\""),
            }
        } else if matches!(spec.kind, Kind::Plain) {
            " fill=\"none\"".to_owned()
        } else {
            format!(" fill=\"{SURFACE}\"")
        };
        if invis {
            fill = " fill=\"none\"".to_owned();
        }
        let stroke = format!(
            "{}{}",
            paint("stroke", line_color, if point { LINE } else { BORDER }),
            line_attrs(&st, pen)
        );
        // 濃い塗りの上の既定の文字は白に
        let ink = if font.color.is_none() && filled {
            fill_text
                .as_deref()
                .and_then(|t| {
                    color::parse_list(t, scheme.as_deref().or(ctx.scheme.as_deref()))
                        .into_iter()
                        .find_map(|s| s.color)
                })
                .and_then(|c| match c {
                    Color::Rgba(rgba) if rgba.luminance() < 0.45 => Some("#ffffff"),
                    Color::Rgba(_) => Some("#1f1f1f"),
                    Color::None => None,
                })
        } else {
            None
        };
        let special = match spec.kind {
            Kind::Special(s) => Some(s),
            _ => None,
        };
        let inner_w = (w - 2.0 * mx).max(0.0);
        views.push(NodeView {
            geom: Geom {
                kind: spec.kind.clone(),
                w,
                h,
                peripheries: spec.peripheries,
            },
            label,
            record: rec,
            xlabel,
            font,
            invis,
            rounded: has(&st, "rounded") || matches!(spec.kind, Kind::Record { rounded: true }),
            diagonals: spec.diagonals || has(&st, "diagonals"),
            st,
            stroke,
            fill,
            special,
            ink,
            inner_w,
            pos: (0.0, 0.0),
        });
    }

    // ---- 辺 ----
    struct EdgeView {
        label: Option<Text>,
        head_label: Option<Text>,
        tail_label: Option<Text>,
        xlabel: Option<Text>,
        font: Font,
    }
    let mut eviews: Vec<EdgeView> = Vec::with_capacity(model.edges.len());
    let mut ledges: Vec<layout::Edge> = Vec::with_capacity(model.edges.len());
    let edge_name_sep = if model.directed { "->" } else { "--" };
    for e in &model.edges {
        let a = &e.attrs;
        let scheme = a.get("colorscheme").map(str::to_owned);
        let font = font_of(a, "", &mut ctx, None);
        let tail = &model.nodes[e.tail].name;
        let head = &model.nodes[e.head].name;
        let ename = format!("{tail}{edge_name_sep}{head}");
        let names = Names {
            graph: &model.name,
            edge: &ename,
            tail,
            head,
            ..Default::default()
        };
        let mk = |key: &str, font: &Font, ctx: &mut Ctx| {
            a.value(key)
                .filter(|v| !v.text.is_empty())
                .map(|v| label_text(v, &names, font, ctx, scheme.as_deref()))
        };
        let label = mk("label", &font, &mut ctx);
        let end_font = {
            let mut f = font.clone();
            if let Some(s) = a.num("labelfontsize").filter(|v| *v > 0.0) {
                f.size = s.clamp(1.0, 400.0);
            }
            if let Some(n) = a.get("labelfontname") {
                f = f.with_name(n);
            }
            if let Some(c) = a.get("labelfontcolor") {
                f.color = ctx.color(c, scheme.as_deref());
            }
            f
        };
        let head_label = mk("headlabel", &end_font, &mut ctx);
        let tail_label = mk("taillabel", &end_font, &mut ctx);
        let xlabel = mk("xlabel", &font, &mut ctx);
        let size = label.as_ref().map(|t| {
            let (w, h) = (t.width + 4.0, t.height + 2.0);
            if dir.sideways() { (h, w) } else { (w, h) }
        });
        let off = |node: usize, port: Option<&str>| -> f64 {
            match port_of(&views[node], port) {
                Some(p) => {
                    if dir.sideways() {
                        p.at.1
                    } else {
                        p.at.0
                    }
                }
                None => 0.0,
            }
        };
        let tail_port = a.get("tailport").or(e.tail_port.as_deref());
        let head_port = a.get("headport").or(e.head_port.as_deref());
        ledges.push(layout::Edge {
            tail: e.tail,
            head: e.head,
            minlen: a.num("minlen").unwrap_or(1.0).clamp(0.0, 100.0),
            weight: a.num("weight").unwrap_or(1.0).clamp(0.0, 1000.0),
            constraint: a.flag("constraint").unwrap_or(true),
            label: size,
            tail_off: off(e.tail, tail_port),
            head_off: off(e.head, head_port),
        });
        eviews.push(EdgeView {
            label,
            head_label,
            tail_label,
            xlabel,
            font,
        });
    }

    // 自分への辺の輪の分を右に空ける
    let mut loop_room = vec![0f64; views.len()];
    for (i, e) in model.edges.iter().enumerate() {
        if e.tail == e.head {
            let lw = eviews[i]
                .label
                .as_ref()
                .map_or(0.0, |t| if dir.sideways() { t.height } else { t.width });
            loop_room[e.tail] += 18.0 + lw + 4.0;
        }
    }

    // ---- クラスタ ----
    struct ClusterView {
        label: Option<Text>,
        font: Font,
        loc_bottom: bool,
        just: char,
    }
    let mut cviews: Vec<ClusterView> = Vec::new();
    let mut lclusters: Vec<layout::Cluster> = Vec::new();
    for c in &model.clusters {
        let a = &c.attrs;
        let font = font_of(a, "", &mut ctx, None);
        let names = Names {
            graph: &c.name,
            ..Default::default()
        };
        let scheme = a.get("colorscheme").map(str::to_owned);
        let label = a
            .value("label")
            .filter(|v| !v.text.is_empty())
            .map(|v| label_text(v, &names, &font, &mut ctx, scheme.as_deref()));
        let (lw, lh) = label
            .as_ref()
            .map_or((0.0, 0.0), |t| (t.width + 8.0, t.height + 4.0));
        let margin = a
            .num("margin")
            .map_or(CLUSTER_MARGIN, |m| m.clamp(0.0, 200.0));
        let loc_bottom = a
            .get("labelloc")
            .is_some_and(|s| s.trim().to_ascii_lowercase().starts_with('b'));
        // 題の側（配置の座標で。LR・RL は配置の x が絵の y）。BT は Graphviz と同じく、`t` が絵の下になる
        let side = match (dir, loc_bottom) {
            (Dir::TB | Dir::BT, false) => LabelSide::Top,
            (Dir::TB | Dir::BT, true) => LabelSide::Bottom,
            (Dir::LR | Dir::RL, false) => LabelSide::Left,
            (Dir::LR | Dir::RL, true) => LabelSide::Right,
        };
        lclusters.push(layout::Cluster {
            parent: c.parent,
            margin,
            label: if dir.sideways() { (lh, lw) } else { (lw, lh) },
            side,
        });
        cviews.push(ClusterView {
            label,
            font,
            loc_bottom: loc_bottom != (dir == Dir::BT),
            just: a
                .get("labeljust")
                .and_then(|s| s.trim().chars().next())
                .map_or('c', |c| c.to_ascii_lowercase()),
        });
    }

    // ---- 配置 ----
    let lnodes: Vec<layout::Node> = views
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let (w, h) = v.geom.outer();
            let (w, h) = if dir.sideways() { (h, w) } else { (w, h) };
            layout::Node {
                w,
                h,
                cluster: model.nodes[i].cluster,
                extra_right: loop_room[i],
            }
        })
        .collect();
    let sets: Vec<(crate::graph::RankKind, Vec<usize>)> = model
        .rank_sets
        .iter()
        .map(|s| (s.kind, s.nodes.clone()))
        .collect();
    let lopt = layout::Options {
        nodesep,
        ranksep,
        label_left: dir.sideways(),
        ordering_out: g
            .get("ordering")
            .is_some_and(|s| s.trim().eq_ignore_ascii_case("out")),
        rank_iter: budget.rank_iter,
        x_iter: budget.x_iter,
        sweeps: budget.sweeps,
        cluster_rank: !g.flag("newrank").unwrap_or(false)
            && !g.get("clusterrank").is_some_and(|s| {
                matches!(s.trim().to_ascii_lowercase().as_str(), "global" | "none")
            }),
    };
    let lay = layout::layout(&lnodes, &ledges, &lclusters, &sets, &lopt);
    let lh = lay.height;
    let to_final = |p: P| -> P {
        match dir {
            Dir::TB => p,
            Dir::BT => (p.0, lh - p.1),
            Dir::LR => (p.1, p.0),
            Dir::RL => (lh - p.1, p.0),
        }
    };
    for (i, v) in views.iter_mut().enumerate() {
        v.pos = to_final(lay.nodes[i]);
    }

    // ---- 書く ----
    let mut clusters_out = String::new();
    let mut edges_out = String::new();
    let mut labels_out = String::new();
    let mut nodes_out = String::new();
    let mut bbox = BBox::new();
    for v in &views {
        let (w, h) = v.geom.outer();
        bbox.add_rect(
            v.pos.0 - w / 2.0,
            v.pos.1 - h / 2.0,
            v.pos.0 + w / 2.0,
            v.pos.1 + h / 2.0,
        );
    }

    // クラスタ（外から）
    let cluster_index: HashMap<&str, usize> = model
        .clusters
        .iter()
        .enumerate()
        .map(|(i, c)| (c.name.as_str(), i))
        .collect();
    let mut cluster_boxes: Vec<Option<[f64; 4]>> = vec![None; model.clusters.len()];
    for (ci, b) in lay.clusters.iter().enumerate() {
        let Some(b) = b else { continue };
        let (p, q) = (to_final((b[0], b[1])), to_final((b[2], b[3])));
        let fb = [p.0.min(q.0), p.1.min(q.1), p.0.max(q.0), p.1.max(q.1)];
        cluster_boxes[ci] = Some(fb);
    }
    // クラスタの題の箱（絵の座標。辺が避ける）
    let mut title_rects: Vec<[f64; 4]> = Vec::new();
    let mut corder: Vec<usize> = (0..model.clusters.len()).collect();
    let cdepth = |mut c: usize| {
        let mut d = 0;
        while let Some(p) = model.clusters[c].parent {
            d += 1;
            c = p;
            if d > 10_000 {
                break;
            }
        }
        d
    };
    corder.sort_by_key(|&c| cdepth(c));
    for ci in corder {
        let Some(b) = cluster_boxes[ci] else { continue };
        bbox.add_rect(b[0], b[1], b[2], b[3]);
        let c = &model.clusters[ci];
        let a = &c.attrs;
        let st = styles(a.get("style"));
        if has(&st, "invis") {
            continue;
        }
        let scheme = a.get("colorscheme").map(str::to_owned);
        let pen = pen_width(a, &st);
        let color = a.get("color").map(str::to_owned);
        let border = a
            .get("pencolor")
            .or(color.as_deref())
            .map(str::to_owned)
            .and_then(|t| ctx.first_color(&t, scheme.as_deref()));
        let filled = has(&st, "filled") || has(&st, "radial") || has(&st, "striped");
        let fill_text = a
            .get("fillcolor")
            .or(color.as_deref())
            .or(a.get("bgcolor"))
            .map(str::to_owned);
        let fill = if filled {
            match fill_text.as_deref() {
                Some(t) if t.contains(':') => gradient(
                    &mut ctx,
                    t,
                    scheme.as_deref(),
                    has(&st, "radial"),
                    a.num("gradientangle").unwrap_or(0.0),
                ),
                Some(t) => paint("fill", ctx.first_color(t, scheme.as_deref()), SURFACE_ALT),
                None => format!(" fill=\"{SURFACE_ALT}\""),
            }
        } else if let Some(bg) = a.get("bgcolor").map(str::to_owned) {
            paint("fill", ctx.first_color(&bg, scheme.as_deref()), SURFACE_ALT)
        } else {
            format!(" fill=\"{SURFACE_ALT}\" fill-opacity=\"0.5\"")
        };
        let peripheries = a.num("peripheries").unwrap_or(1.0);
        let stroke = if peripheries < 1.0 {
            " stroke=\"none\"".to_owned()
        } else {
            format!(
                "{}{}",
                paint("stroke", border, BORDER),
                line_attrs(&st, pen)
            )
        };
        let r = if has(&st, "rounded") { " rx=\"8\"" } else { "" };
        clusters_out.push_str(&format!(
            "<g class=\"dot-cluster\" data-id=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{r}{fill}{stroke}/>",
            esc(&c.name),
            num(b[0]),
            num(b[1]),
            num(b[2] - b[0]),
            num(b[3] - b[1])
        ));
        let cv = &cviews[ci];
        if let Some(t) = &cv.label {
            let margin = lclusters[ci].margin;
            let cx = match cv.just {
                'l' => b[0] + margin + t.width / 2.0,
                'r' => b[2] - margin - t.width / 2.0,
                _ => (b[0] + b[2]) / 2.0,
            };
            let cy = if cv.loc_bottom {
                b[3] - margin / 2.0 - t.height / 2.0
            } else {
                b[1] + margin / 2.0 + 2.0 + t.height / 2.0
            };
            title_rects.push([
                cx - t.width / 2.0 - 4.0,
                cy - t.height / 2.0 - 1.0,
                cx + t.width / 2.0 + 4.0,
                cy + t.height / 2.0 + 1.0,
            ]);
            let fill = if cv.font.color.is_some() {
                String::new()
            } else {
                format!(" fill=\"{MUTED}\"")
            };
            clusters_out.push_str(&text_svg(
                t,
                cx,
                cy,
                0.0,
                &cv.font,
                &format!(
                    " class=\"dot-cluster-label\"{}{}",
                    font_attrs(&cv.font),
                    fill
                ),
            ));
        }
        clusters_out.push_str("</g>");
    }

    // 辺が避ける箱（配置の座標）: 端でないノードとクラスタの題
    let from_final = |f: P| -> P {
        match dir {
            Dir::TB => f,
            Dir::BT => (f.0, lh - f.1),
            Dir::LR => (f.1, f.0),
            Dir::RL => (f.1, lh - f.0),
        }
    };
    let mut obstacles = {
        const NODE_PAD: f64 = 4.0;
        let mut rects: Vec<(route::Rect, Option<usize>)> = Vec::new();
        for (i, v) in views.iter().enumerate() {
            if v.invis {
                continue;
            }
            let (x, y) = lay.nodes[i];
            let (w, h) = (lnodes[i].w / 2.0 + NODE_PAD, lnodes[i].h / 2.0 + NODE_PAD);
            rects.push(([x - w, y - h, x + w, y + h], Some(i)));
        }
        for r in &title_rects {
            let (p, q) = (from_final((r[0], r[1])), from_final((r[2], r[3])));
            rects.push((
                [p.0.min(q.0), p.1.min(q.1), p.0.max(q.0), p.1.max(q.1)],
                None,
            ));
        }
        route::Obstacles::new(rects)
    };

    // 辺
    let compound = g.flag("compound").unwrap_or(false);
    let vertical_flow = !dir.sideways();
    for (ei, e) in model.edges.iter().enumerate() {
        let a = &e.attrs;
        let scheme = a.get("colorscheme").map(str::to_owned);
        let st = styles(a.get("style"));
        let ev = &eviews[ei];
        let invis = has(&st, "invis") || !draw_edges;
        let pen = pen_width(a, &st);
        let arrowsize = a.num("arrowsize").unwrap_or(1.0).clamp(0.0, 10.0);
        let dir_attr = a
            .get("dir")
            .map(|s| s.trim().to_ascii_lowercase())
            .unwrap_or_else(|| {
                if model.directed {
                    "forward".into()
                } else {
                    "none".into()
                }
            });
        let head_arrow = if matches!(dir_attr.as_str(), "forward" | "both") {
            arrows::parse(a.get("arrowhead").unwrap_or("normal"))
        } else {
            vec![]
        };
        let tail_arrow = if matches!(dir_attr.as_str(), "back" | "both") {
            arrows::parse(a.get("arrowtail").unwrap_or("normal"))
        } else {
            vec![]
        };
        let tv = &views[e.tail];
        let hv = &views[e.head];
        let tail_port = port_of(tv, a.get("tailport").or(e.tail_port.as_deref()));
        let head_port = port_of(hv, a.get("headport").or(e.head_port.as_deref()));
        let head_clip = a.flag("headclip").unwrap_or(true);
        let tail_clip = a.flag("tailclip").unwrap_or(true);
        let mut path: Vec<Cubic>;
        let mut label_at: Option<P>;
        match &lay.routes[ei] {
            Route::Path { points, label } => {
                let mut pts: Vec<P> = points.clone();
                if !(straight || curved) {
                    obstacles.avoid(&mut pts, &[e.tail, e.head], !polyline);
                }
                let mut lp: Vec<P> = pts.clone();
                if straight || curved {
                    lp = vec![pts[0], pts[pts.len() - 1]];
                }
                path = if curved {
                    let (a0, b0) = (lp[0], lp[1]);
                    let (mx, my) = ((a0.0 + b0.0) / 2.0, (a0.1 + b0.1) / 2.0);
                    let (dx, dy) = (b0.0 - a0.0, b0.1 - a0.1);
                    let bend = (mx - dy * 0.2, my + dx * 0.2);
                    vec![Cubic(a0, bend, bend, b0)]
                } else {
                    curve::through(&lp, straight || polyline, true)
                };
                path = path.iter().map(|c| c.map(to_final)).collect();
                label_at = label.map(to_final);
            }
            Route::Flat { lane, adjacent } => {
                let (pa, pb) = (lay.nodes[e.tail], lay.nodes[e.head]);
                let ta = pa.0
                    + if dir.sideways() {
                        tail_port.as_ref().map_or(0.0, |p| p.at.1)
                    } else {
                        tail_port.as_ref().map_or(0.0, |p| p.at.0)
                    };
                let hb = pb.0
                    + if dir.sideways() {
                        head_port.as_ref().map_or(0.0, |p| p.at.1)
                    } else {
                        head_port.as_ref().map_or(0.0, |p| p.at.0)
                    };
                let (pa, pb) = ((ta, pa.1), (hb, pb.1));
                let h_a = lnodes[e.tail].h / 2.0;
                if *adjacent && *lane == 0 {
                    path = vec![Cubic::line(pa, pb)];
                    label_at = ev.label.as_ref().map(|t| {
                        let lh = if dir.sideways() { t.width } else { t.height };
                        ((pa.0 + pb.0) / 2.0, pa.1 - lh / 2.0 - 2.0)
                    });
                } else {
                    // 間のノード・題の上を越える（弧の頂きは制御点の 3/4）
                    let between = obstacles.overlapping(
                        [pa.0.min(pb.0), pa.1 - 10_000.0, pa.0.max(pb.0), pa.1 + 1.0],
                        &[e.tail, e.head],
                    );
                    let clear = between
                        .iter()
                        .map(|r| (pa.1 - r[1]) / 0.75 + 6.0)
                        .fold(0.0, f64::max);
                    let lift = (h_a + 14.0 + (pb.0 - pa.0).abs().min(400.0) * 0.05).max(clear)
                        + 12.0 * *lane as f64;
                    let top = pa.1.min(pb.1) - lift;
                    path = vec![Cubic(pa, (pa.0, top), (pb.0, top), pb)];
                    label_at = ev.label.as_ref().map(|t| {
                        let lh = if dir.sideways() { t.width } else { t.height };
                        ((pa.0 + pb.0) / 2.0, top + lift * 0.25 - lh / 2.0 - 2.0)
                    });
                }
                path = path.iter().map(|c| c.map(to_final)).collect();
                label_at = label_at.map(to_final);
            }
            Route::Loop { index } => {
                let c = lay.nodes[e.tail];
                let ln = &lnodes[e.tail];
                let half_w = ln.w / 2.0;
                let half_h = ln.h / 2.0;
                let size = 18.0 + 8.0 * *index as f64;
                let start = (c.0 + half_w * 0.6, c.1 - half_h * 0.6);
                let end = (c.0 + half_w * 0.6, c.1 + half_h * 0.6);
                let reach = c.0 + half_w + size;
                path = vec![Cubic(
                    start,
                    (reach, c.1 - half_h - size * 0.4),
                    (reach, c.1 + half_h + size * 0.4),
                    end,
                )];
                label_at = ev.label.as_ref().map(|t| {
                    let lw = if dir.sideways() { t.height } else { t.width };
                    (reach + lw / 2.0 + 2.0, c.1)
                });
                path = path.iter().map(|c| c.map(to_final)).collect();
                label_at = label_at.map(to_final);
            }
        }
        let _ = vertical_flow;
        if path.is_empty() {
            continue;
        }
        // 端の点をポートに
        let reach = {
            let (a0, b0) = (path[0].0, path[path.len() - 1].3);
            ((b0.0 - a0.0).hypot(b0.1 - a0.1) / 3.0).clamp(20.0, 60.0)
        };
        if let Some(p) = &tail_port {
            let at = (tv.pos.0 + p.at.0, tv.pos.1 + p.at.1);
            path[0].0 = at;
            if let Some(d) = p.dir {
                path[0].1 = (at.0 + d.0 * reach, at.1 + d.1 * reach);
                detour(&mut path, tv, d, reach);
            }
        }
        if let Some(p) = &head_port {
            let last = path.len() - 1;
            let at = (hv.pos.0 + p.at.0, hv.pos.1 + p.at.1);
            path[last].3 = at;
            if let Some(d) = p.dir {
                path[last].2 = (at.0 + d.0 * reach, at.1 + d.1 * reach);
                let mut rev = curve::reverse(&path);
                detour(&mut rev, hv, d, reach);
                path = curve::reverse(&rev);
            }
        }
        // 外形で切る
        let cluster_clip = |key: &str| -> Option<[f64; 4]> {
            if !compound {
                return None;
            }
            let name = a.get(key)?;
            let ci = *cluster_index.get(name)?;
            cluster_boxes[ci]
        };
        if e.tail != e.head {
            if tail_clip && let Some(f) = clip_fn(tv, &tail_port) {
                curve::clip_start(&mut path, &*f);
            }
            if head_clip && let Some(f) = clip_fn(hv, &head_port) {
                curve::clip_end(&mut path, &*f);
            }
            if let Some(b) = cluster_clip("ltail") {
                let inside_tail =
                    tv.pos.0 >= b[0] && tv.pos.0 <= b[2] && tv.pos.1 >= b[1] && tv.pos.1 <= b[3];
                let inside_head =
                    hv.pos.0 >= b[0] && hv.pos.0 <= b[2] && hv.pos.1 >= b[1] && hv.pos.1 <= b[3];
                if inside_tail && !inside_head {
                    curve::clip_start(&mut path, &|p: P| {
                        p.0 >= b[0] && p.0 <= b[2] && p.1 >= b[1] && p.1 <= b[3]
                    });
                }
            }
            if let Some(b) = cluster_clip("lhead") {
                let inside_tail =
                    tv.pos.0 >= b[0] && tv.pos.0 <= b[2] && tv.pos.1 >= b[1] && tv.pos.1 <= b[3];
                let inside_head =
                    hv.pos.0 >= b[0] && hv.pos.0 <= b[2] && hv.pos.1 >= b[1] && hv.pos.1 <= b[3];
                if inside_head && !inside_tail {
                    curve::clip_end(&mut path, &|p: P| {
                        p.0 >= b[0] && p.0 <= b[2] && p.1 >= b[1] && p.1 <= b[3]
                    });
                }
            }
        }
        if path.is_empty() {
            continue;
        }
        let start = path[0].0;
        let end = path[path.len() - 1].3;
        // 矢じりの分を縮める
        let head_len = arrows::length(&head_arrow, arrowsize);
        let tail_len = arrows::length(&tail_arrow, arrowsize);
        let head_tip = if head_arrow.is_empty() {
            None
        } else {
            curve::shorten_end(&mut path, head_len)
        };
        let tail_tip = if tail_arrow.is_empty() || path.is_empty() {
            None
        } else {
            let mut rev = curve::reverse(&path);
            let t = curve::shorten_end(&mut rev, tail_len);
            path = curve::reverse(&rev);
            t
        };
        for p in &path {
            bbox.add(p.0);
            bbox.add(p.1);
            bbox.add(p.2);
            bbox.add(p.3);
        }
        // 色
        let color_text = a.get("color").map(str::to_owned);
        let stops = color_text
            .as_deref()
            .map(|t| color::parse_list(t, scheme.as_deref().or(ctx.scheme.as_deref())))
            .unwrap_or_default();
        if color_text.is_some() && stops.iter().any(|s| s.color.is_none()) {
            let _ = ctx.first_color(color_text.as_deref().unwrap_or(""), scheme.as_deref());
        }
        let colors: Vec<(Option<Color>, Option<f64>)> =
            stops.iter().map(|s| (s.color, s.fraction)).collect();
        let main = colors.iter().find_map(|c| c.0);
        let main_attr = |name: &str| paint(name, main, LINE);
        if !invis {
            edges_out.push_str(&format!(
                "<g class=\"dot-edge\" data-id=\"{}\">",
                esc(&format!(
                    "{}{}{}",
                    model.nodes[e.tail].name, edge_name_sep, model.nodes[e.head].name
                ))
            ));
            let extra = line_attrs(&st, pen);
            let known: Vec<(Option<Color>, Option<f64>)> =
                colors.iter().filter(|c| c.0.is_some()).cloned().collect();
            if !path.is_empty() {
                if known.len() > 1 && known.iter().any(|c| c.1.is_some()) {
                    // 割合つき: 線に沿った区切り
                    let total = curve::total_length(&path);
                    let given: f64 = known.iter().filter_map(|c| c.1).sum();
                    let unspecified = known.iter().filter(|c| c.1.is_none()).count();
                    let rest = ((1.0 - given).max(0.0)) / unspecified.max(1) as f64;
                    let mut at = 0.0;
                    let d = curve::svg_d(&path);
                    for (c, f) in &known {
                        let frac = f.unwrap_or(rest);
                        let len = total * frac;
                        if len <= 0.0 {
                            continue;
                        }
                        edges_out.push_str(&format!(
                            "<path d=\"{d}\" fill=\"none\"{}{extra} stroke-dasharray=\"0,{},{},{}\"/>",
                            paint("stroke", *c, LINE),
                            num(at),
                            num(len),
                            num(total + 1.0)
                        ));
                        at += len;
                    }
                } else if known.len() > 1 {
                    // 平行な線
                    let n = known.len() as f64;
                    for (i, (c, _)) in known.iter().enumerate() {
                        let d = (i as f64 - (n - 1.0) / 2.0) * pen.max(1.0) * 2.0;
                        let off = curve::offset(&path, d);
                        edges_out.push_str(&format!(
                            "<path d=\"{}\" fill=\"none\"{}{extra}/>",
                            curve::svg_d(&off),
                            paint("stroke", *c, LINE)
                        ));
                    }
                } else {
                    edges_out.push_str(&format!(
                        "<path d=\"{}\" fill=\"none\"{}{extra}/>",
                        curve::svg_d(&path),
                        main_attr("stroke")
                    ));
                }
            }
            let arrow_color = match main {
                Some(Color::Rgba(c)) => c.hex(),
                Some(Color::None) => "none".into(),
                None => LINE.into(),
            };
            if let Some((tip, dir)) = head_tip {
                edges_out.push_str(&arrows::draw(
                    &head_arrow,
                    tip.0,
                    tip.1,
                    dir.0,
                    dir.1,
                    arrowsize,
                    &arrow_color,
                    pen,
                ));
                bbox.add(tip);
            }
            if let Some((tip, dir)) = tail_tip {
                edges_out.push_str(&arrows::draw(
                    &tail_arrow,
                    tip.0,
                    tip.1,
                    dir.0,
                    dir.1,
                    arrowsize,
                    &arrow_color,
                    pen,
                ));
                bbox.add(tip);
            }
            edges_out.push_str("</g>");
        }
        // ラベル
        let label_attrs = |f: &Font| format!(" class=\"dot-edge-label\"{}", font_attrs(f));
        if let (Some(t), Some(at)) = (&ev.label, label_at.or_else(|| curve::midpoint(&path))) {
            if !invis {
                labels_out.push_str(&text_svg(
                    t,
                    at.0,
                    at.1,
                    0.0,
                    &ev.font,
                    &label_attrs(&ev.font),
                ));
            }
            bbox.add_rect(
                at.0 - t.width / 2.0,
                at.1 - t.height / 2.0,
                at.0 + t.width / 2.0,
                at.1 + t.height / 2.0,
            );
        }
        if let (Some(t), Some(m)) = (&ev.xlabel, curve::midpoint(&path)) {
            let at = (m.0 + t.width / 2.0 + 4.0, m.1 - t.height / 2.0);
            if !invis {
                labels_out.push_str(&text_svg(
                    t,
                    at.0,
                    at.1,
                    0.0,
                    &ev.font,
                    &label_attrs(&ev.font),
                ));
            }
            bbox.add_rect(
                at.0 - t.width / 2.0,
                at.1 - t.height / 2.0,
                at.0 + t.width / 2.0,
                at.1 + t.height / 2.0,
            );
        }
        let distance = a.num("labeldistance").unwrap_or(1.0).clamp(0.0, 20.0) * 10.0;
        let angle = a
            .num("labelangle")
            .unwrap_or(-25.0)
            .clamp(-360.0, 360.0)
            .to_radians();
        for (t, at, other) in [(&ev.head_label, end, start), (&ev.tail_label, start, end)] {
            let Some(t) = t else { continue };
            let (dx, dy) = (other.0 - at.0, other.1 - at.1);
            let l = dx.hypot(dy).max(1e-9);
            let (ux, uy) = (dx / l, dy / l);
            // 辺の向きから angle だけ回す（画面で反時計回り）
            let (s, c) = (-angle).sin_cos();
            let (rx, ry) = (ux * c - uy * s, ux * s + uy * c);
            let reach = distance + t.width.max(t.height) / 2.0;
            let center = (at.0 + rx * reach, at.1 + ry * reach);
            if !invis {
                labels_out.push_str(&text_svg(
                    t,
                    center.0,
                    center.1,
                    0.0,
                    &ev.font,
                    &label_attrs(
                        &t.lines
                            .first()
                            .and_then(|l| l.spans.first())
                            .map_or(ev.font.clone(), |s| s.font.clone()),
                    ),
                ));
            }
            bbox.add_rect(
                center.0 - t.width / 2.0,
                center.1 - t.height / 2.0,
                center.0 + t.width / 2.0,
                center.1 + t.height / 2.0,
            );
        }
    }

    // ノード
    for (i, v) in views.iter().enumerate() {
        let node = &model.nodes[i];
        let (x, y) = v.pos;
        if let Some(t) = &v.xlabel {
            let (w, h) = v.geom.outer();
            let at = (x - w / 2.0 - t.width / 2.0, y - h / 2.0 - t.height / 2.0);
            if !v.invis {
                labels_out.push_str(&text_svg(
                    t,
                    at.0,
                    at.1,
                    0.0,
                    &v.font,
                    &format!(" class=\"dot-xlabel\"{}", font_attrs(&v.font)),
                ));
            }
            bbox.add_rect(
                at.0 - t.width / 2.0,
                at.1 - t.height / 2.0,
                at.0 + t.width / 2.0,
                at.1 + t.height / 2.0,
            );
        }
        if v.invis {
            continue;
        }
        let mut body = String::new();
        let fill = v.fill.clone();
        let striped = has(&v.st, "striped");
        let wedged = has(&v.st, "wedged");
        match (&v.geom.kind, v.special) {
            (_, Some(name)) => body.push_str(&shapes::special(
                name, v.geom.w, v.geom.h, x, y, &fill, &v.stroke,
            )),
            _ => body.push_str(&shapes::outline(
                &v.geom,
                x,
                y,
                &fill,
                &v.stroke,
                v.rounded,
                v.diagonals,
            )),
        }
        if striped || wedged {
            body.push_str(&stripes(&mut ctx, &node.attrs, v, striped));
        }
        if let Some(rec) = &v.record {
            let mut seps = Vec::new();
            record::separators(rec, &mut seps);
            let mut d = String::new();
            for s in seps {
                d.push_str(&format!(
                    "M{},{}L{},{}",
                    num(x + s[0]),
                    num(y + s[1]),
                    num(x + s[2]),
                    num(y + s[3])
                ));
            }
            if !d.is_empty() {
                body.push_str(&format!("<path d=\"{d}\" fill=\"none\"{}/>", v.stroke));
            }
            let mut leaves = Vec::new();
            record::leaves(rec, &mut leaves);
            for f in leaves {
                if let Some(t) = &f.text {
                    body.push_str(&text_svg(
                        t,
                        x + f.x + f.w / 2.0,
                        y + f.y + f.h / 2.0,
                        f.w - 16.0,
                        &v.font,
                        &format!(" class=\"dot-label\"{}{}", font_attrs(&v.font), ink_attr(v)),
                    ));
                }
            }
        }
        if let Some(t) = &v.label {
            body.push_str(&text_svg(
                t,
                x,
                y,
                v.inner_w,
                &v.font,
                &format!(" class=\"dot-label\"{}{}", font_attrs(&v.font), ink_attr(v)),
            ));
        }
        nodes_out.push_str(&format!(
            "<g class=\"dot-node\" data-id=\"{}\">{body}</g>",
            esc(&node.name)
        ));
    }

    // グラフの題
    let mut title = String::new();
    let names = Names {
        graph: &model.name,
        ..Default::default()
    };
    let graph_label = g
        .value("label")
        .filter(|v| !v.text.is_empty())
        .map(|v| label_text(v, &names, &graph_font, &mut ctx, None));
    if bbox.empty() {
        bbox.add((0.0, 0.0));
    }
    if let Some(t) = &graph_label {
        let top = g
            .get("labelloc")
            .is_some_and(|s| s.trim().to_ascii_lowercase().starts_with('t'));
        let just = g
            .get("labeljust")
            .and_then(|s| s.trim().chars().next())
            .map_or('c', |c| c.to_ascii_lowercase());
        let (x0, y0, x1, y1) = (bbox.x0, bbox.y0, bbox.x1, bbox.y1);
        let cy = if top {
            y0 - 6.0 - t.height / 2.0
        } else {
            y1 + 6.0 + t.height / 2.0
        };
        let cx = match just {
            'l' => x0 + t.width / 2.0,
            'r' => x1 - t.width / 2.0,
            _ => (x0 + x1) / 2.0,
        };
        title = text_svg(
            t,
            cx,
            cy,
            0.0,
            &graph_font,
            &format!(" class=\"dot-title\"{}", font_attrs(&graph_font)),
        );
        bbox.add_rect(
            cx - t.width / 2.0,
            cy - t.height / 2.0,
            cx + t.width / 2.0,
            cy + t.height / 2.0,
        );
    }
    let dx = PAD - bbox.x0;
    let dy = PAD - bbox.y0;
    let width = (bbox.x1 - bbox.x0 + 2.0 * PAD).ceil().max(1.0);
    let height = (bbox.y1 - bbox.y0 + 2.0 * PAD).ceil().max(1.0);
    let mut content = String::new();
    if !ctx.defs.is_empty() {
        content.push_str(&format!("<defs>{}</defs>", ctx.defs));
    }
    if let Some(bg) = g.get("bgcolor").map(str::to_owned)
        && let Some(c) = ctx.first_color(&bg, None)
    {
        content.push_str(&format!(
            "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\"{}/>",
            num(width),
            num(height),
            color_attr("fill", &c)
        ));
    }
    content.push_str(&format!(
        "<g transform=\"translate({},{})\">",
        num(dx),
        num(dy)
    ));
    if !clusters_out.is_empty() {
        content.push_str(&format!("<g class=\"dot-clusters\">{clusters_out}</g>"));
    }
    content.push_str(&format!("<g class=\"dot-nodes\">{nodes_out}</g>"));
    if !edges_out.is_empty() {
        content.push_str(&format!(
            "<g class=\"dot-edges\" stroke-linecap=\"round\">{edges_out}</g>"
        ));
    }
    if !labels_out.is_empty() {
        content.push_str(&format!("<g class=\"dot-labels\">{labels_out}</g>"));
    }
    content.push_str(&title);
    content.push_str("</g>");
    let options = amonhen_pele::RenderOptions::default();
    let acc = Accessible::default();
    let _ = ctx.model;
    svg_document("dot", width, height, FONT_SIZE, &options, &acc, &content)
}

/// 線を止める内外の判定（ポートの箱か、ノードの外形）。止めないポートなら None。
fn clip_fn<'a>(view: &'a NodeView, port: &Option<Port>) -> Option<Box<dyn Fn(P) -> bool + 'a>> {
    let (x, y) = view.pos;
    match port {
        Some(Port { clip: None, .. }) => None,
        Some(Port { clip: Some(b), .. }) => {
            let b = *b;
            Some(Box::new(move |p: P| {
                p.0 >= x + b[0] && p.0 <= x + b[2] && p.1 >= y + b[1] && p.1 <= y + b[3]
            }))
        }
        None => Some(Box::new(move |p: P| view.geom.inside(p.0 - x, p.1 - y))),
    }
}

/// ポートの向きが行き先と逆なら、ノードの脇を回る（ノードを横切らないように）。`path` は始めがそのポート。
fn detour(path: &mut Vec<Cubic>, view: &NodeView, d: P, reach: f64) {
    let Some(&first) = path.first() else { return };
    let target = path[path.len() - 1].3;
    let at = first.0;
    let (cx, cy) = view.pos;
    let chord = (target.0 - cx, target.1 - cy);
    if d.0 * chord.0 + d.1 * chord.1 >= 0.0 {
        return;
    }
    // 行き先の側の脇
    let mut perp = (-d.1, d.0);
    if perp.0 * chord.0 + perp.1 * chord.1 < 0.0 {
        perp = (-perp.0, -perp.1);
    }
    let (w, h) = view.geom.outer();
    let extent = (perp.0 * w).abs().max((perp.1 * h).abs()) / 2.0 + reach * 0.6;
    let out = (at.0 + d.0 * reach, at.1 + d.1 * reach);
    let side = (cx + perp.0 * extent, cy + perp.1 * extent);
    let first_leg = Cubic(at, out, (side.0 + d.0 * reach, side.1 + d.1 * reach), side);
    let rest_end = first.3;
    let second = Cubic(
        side,
        (side.0 - d.0 * reach, side.1 - d.1 * reach),
        first.2,
        rest_end,
    );
    path[0] = second;
    path.insert(0, first_leg);
}

fn ink_attr(v: &NodeView) -> String {
    match v.ink {
        Some(c) => format!(" fill=\"{c}\""),
        None => String::new(),
    }
}

/// `<text>` の書体の属性（大きさ・太さ・色）。
fn font_attrs(f: &Font) -> String {
    let mut s = String::new();
    if (f.size - FONT_SIZE).abs() > 0.01 {
        s.push_str(&format!(" font-size=\"{}\"", num(f.size)));
    }
    if f.bold {
        s.push_str(" font-weight=\"bold\"");
    }
    if f.italic {
        s.push_str(" font-style=\"italic\"");
    }
    if f.mono {
        s.push_str(" font-family=\"var(--_fm)\"");
    }
    if let Some(c) = &f.color {
        s.push_str(&color_attr("fill", c));
    }
    s
}

/// 色の並びのグラデーション（`defs` に足し、塗りの属性を返す）。
fn gradient(ctx: &mut Ctx, text: &str, scheme: Option<&str>, radial: bool, angle: f64) -> String {
    let stops = color::parse_list(text, scheme.or(ctx.scheme.as_deref()));
    if stops.iter().any(|s| s.color.is_none()) {
        let _ = ctx.first_color(text, scheme);
    }
    let known: Vec<(Rgba, Option<f64>)> = stops
        .iter()
        .filter_map(|s| match s.color {
            Some(Color::Rgba(c)) => Some((c, s.fraction)),
            Some(Color::None) => Some((
                Rgba {
                    r: 255,
                    g: 255,
                    b: 255,
                    a: 0,
                },
                s.fraction,
            )),
            None => None,
        })
        .collect();
    if known.is_empty() {
        return format!(" fill=\"{SURFACE_ALT}\"");
    }
    if known.len() == 1 {
        return color_attr("fill", &Color::Rgba(known[0].0));
    }
    let id = ctx.id();
    let mut body = String::new();
    let n = known.len();
    let mut at = 0.0;
    for (i, (c, f)) in known.iter().enumerate() {
        let offset = match f {
            Some(f) => {
                let o = at + f;
                at = o;
                (o - f).clamp(0.0, 1.0)
            }
            None => i as f64 / (n - 1) as f64,
        };
        body.push_str(&format!(
            "<stop offset=\"{}\" stop-color=\"{}\"{}/>",
            num(offset),
            c.hex(),
            c.opacity()
                .map_or(String::new(), |o| format!(" stop-opacity=\"{o}\""))
        ));
    }
    if radial {
        ctx.defs.push_str(&format!(
            "<radialGradient id=\"{id}\">{body}</radialGradient>"
        ));
    } else {
        let r = angle.to_radians();
        let (dx, dy) = (r.cos() / 2.0, -r.sin() / 2.0);
        ctx.defs.push_str(&format!(
            "<linearGradient id=\"{id}\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\">{body}</linearGradient>",
            num(0.5 - dx),
            num(0.5 - dy),
            num(0.5 + dx),
            num(0.5 + dy)
        ));
    }
    format!(" fill=\"url(#{id})\"")
}

/// striped（箱の縦の帯）・wedged（楕円の扇）。
fn stripes(ctx: &mut Ctx, a: &Attrs, v: &NodeView, striped: bool) -> String {
    let scheme = a.get("colorscheme").map(str::to_owned);
    let text = a
        .get("fillcolor")
        .or(a.get("color"))
        .unwrap_or("")
        .to_owned();
    let stops = color::parse_list(&text, scheme.as_deref().or(ctx.scheme.as_deref()));
    let known: Vec<(Color, Option<f64>)> = stops
        .iter()
        .filter_map(|s| s.color.map(|c| (c, s.fraction)))
        .collect();
    if known.len() < 2 {
        return String::new();
    }
    let given: f64 = known.iter().filter_map(|k| k.1).sum();
    let unspecified = known.iter().filter(|k| k.1.is_none()).count();
    let rest = (1.0 - given).max(0.0) / unspecified.max(1) as f64;
    let (x, y) = v.pos;
    let (w, h) = (v.geom.w, v.geom.h);
    let mut out = String::new();
    let mut at = 0.0;
    for (c, f) in known {
        let frac = f.unwrap_or(rest);
        if frac <= 0.0 {
            continue;
        }
        if striped {
            out.push_str(&format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{} stroke=\"none\"/>",
                num(x - w / 2.0 + at * w),
                num(y - h / 2.0),
                num(frac * w),
                num(h),
                color_attr("fill", &c)
            ));
        } else {
            let a0 = at * std::f64::consts::TAU;
            let a1 = (at + frac) * std::f64::consts::TAU;
            let (rx, ry) = (w / 2.0, h / 2.0);
            out.push_str(&format!(
                "<path d=\"M{},{}L{},{}A{} {} 0 {} 0 {},{}Z\"{} stroke=\"none\"/>",
                num(x),
                num(y),
                num(x + rx * a0.cos()),
                num(y - ry * a0.sin()),
                num(rx),
                num(ry),
                if frac > 0.5 { 1 } else { 0 },
                num(x + rx * a1.cos()),
                num(y - ry * a1.sin()),
                color_attr("fill", &c)
            ));
        }
        at += frac;
    }
    // 外形の線を上に描き直す
    out.push_str(&shapes::outline(
        &v.geom,
        x,
        y,
        " fill=\"none\"",
        &v.stroke,
        v.rounded,
        false,
    ));
    out
}

struct BBox {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

impl BBox {
    fn new() -> Self {
        BBox {
            x0: f64::MAX,
            y0: f64::MAX,
            x1: f64::MIN,
            y1: f64::MIN,
        }
    }

    fn empty(&self) -> bool {
        self.x0 > self.x1
    }

    fn add(&mut self, p: P) {
        if !p.0.is_finite() || !p.1.is_finite() {
            return;
        }
        self.x0 = self.x0.min(p.0);
        self.y0 = self.y0.min(p.1);
        self.x1 = self.x1.max(p.0);
        self.y1 = self.y1.max(p.1);
    }

    fn add_rect(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) {
        self.add((x0, y0));
        self.add((x1, y1));
    }
}
