//! モデルを配置して SVG を書く: ノード（形・ラベル・record・HTML）、辺（道筋・矢じり・ラベル）、クラスタ、
//! グラフの題と地。色は文が決めたものを使い、決めないものはテーマの変数（`var(--_l)` など）にする。

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use amonhen_pele::svg::builder::{esc, num};
use amonhen_pele::svg::root::{Accessible, svg_document};

use crate::Note;
use crate::arrows;
use crate::color::{self, Color, Rgba};
use crate::curve::{self, Cubic, P};
use crate::engines::{self, Engine};
use crate::graph::{Attrs, Model};
use crate::image::{self, Images, Picture, Scale};
use crate::layout::{self, LabelSide, Route};
use crate::record::{self, Field};
use crate::route;
use crate::shapes::{self, Geom, Kind};
use crate::spline;
use crate::text::{self, Font, Names, Text, color_attr, text_svg};

/// patchwork の面積 1 の升の辺（Graphviz の絵に合わせた大きさ）。
const PATCH: f64 = 40.0;
/// 点（Graphviz の 1 インチ = 72）。
const INCH: f64 = 72.0;
/// 絵の余白の既定（Graphviz の `pad` の既定 0.0555 インチ）。
const PAD: f64 = 4.0;
const FONT_SIZE: f64 = 14.0;
/// クラスタの題の左右の余白（アプリの書体のゆとりとは別に）。
const TITLE_PAD: f64 = 8.0;
/// クラスタの中の余白。
const CLUSTER_MARGIN: f64 = 8.0;

const LINE: &str = "var(--_l)";
const BORDER: &str = "var(--_b)";
const SURFACE_ALT: &str = "var(--_a)";

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
    images: Images<'a>,
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
        let images = &mut ctx.images;
        let t = text::html_label_with_images(
            &raw.text,
            font,
            scheme.or(ctx.scheme.as_deref()),
            &mut |src| images.get(src),
        );
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
    /// `image` の画像と、合わせ方・置き場所（`imagepos` の縦と横: -1・0・1）。
    image: Option<(Arc<Picture>, Scale, (i8, i8))>,
    /// ラベルの縦のずれ（`labelloc` の t・b。ノードがラベルより高いとき）。
    label_dy: f64,
    /// ノードの外の地（文が決めたクラスタの塗り・グラフの bgcolor）の上で読める文字の色。
    ground_ink: Option<&'static str>,
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
    let html_port = view.label.as_ref().and_then(|t| {
        let table = t.html.as_ref()?;
        let b = table.port(name)?;
        let (ox, oy) = (-table.w / 2.0, -table.h / 2.0);
        Some([b[0] + ox, b[1] + oy, b[2] + ox, b[3] + oy])
    });
    let record_port = view
        .record
        .as_ref()
        .and_then(|r| record::port(r, name))
        .map(|f| [f.x, f.y, f.x + f.w, f.y + f.h]);
    if let Some(b) = record_port.or(html_port) {
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
pub fn render(
    model: &Model,
    budget: Budget,
    default_engine: Engine,
    notes: &mut BTreeSet<Note>,
    images: Option<&image::Loader>,
) -> String {
    let g = &model.attrs;
    // 手間の上限で打ち切ったかは、配置・道筋の中で記し、最後に注記にする
    crate::take_cut_short();
    let mut ctx = Ctx {
        model,
        notes,
        scheme: g.get("colorscheme").map(str::to_owned),
        defs: String::new(),
        next_id: 0,
        images: Images::new(images),
    };
    // 配置: `layout` が決め、なければフェンスの言葉の既定。知らない名前は dot で描いて注記
    let engine = match g.get("layout").map(str::trim).filter(|l| !l.is_empty()) {
        Some(l) => Engine::from_name(l).unwrap_or_else(|| {
            ctx.notes.insert(Note::OtherLayouts);
            Engine::Dot
        }),
        None => default_engine,
    };
    // dot 以外の配置（rankdir は使わない）
    let free = engine != Engine::Dot;
    let dir = match g.get("rankdir").map(|s| s.trim().to_ascii_uppercase()) {
        _ if free => Dir::TB,
        Some(ref s) if s == "LR" => Dir::LR,
        Some(ref s) if s == "RL" => Dir::RL,
        Some(ref s) if s == "BT" => Dir::BT,
        _ => Dir::TB,
    };
    let splines = g
        .get("splines")
        .map(|s| s.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let ortho = splines == "ortho";
    let draw_edges = !matches!(splines.as_str(), "none" | "\"\"")
        && !(g.get("splines") == Some(""))
        && engine.draws_edges();
    // dot 以外の既定（splines なし）は直線（Graphviz と同じ）
    let straight =
        matches!(splines.as_str(), "line" | "false" | "no") || (free && g.get("splines").is_none());
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
    // 文が決めた地（クラスタの塗り・グラフの bgcolor）。その上の既定の色の文字・線は読める色にする
    let grounds = Grounds::new(model, ctx.scheme.as_deref());

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
        // 画像（読めなければ注記して描かない）
        let picture = match a.get("image").filter(|s| !s.trim().is_empty()) {
            Some(src) => {
                let p = ctx.images.get(src);
                if p.is_none() {
                    ctx.notes.insert(Note::Images);
                }
                p
            }
            None => None,
        };
        if shape_name.eq_ignore_ascii_case("epsf") {
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
        let (mut lw, mut lh) = match (&label, &rec) {
            // 枠のある形は、アプリの書体が広くても文字が線に触れないように
            (Some(t), _) if !t.is_empty() => {
                let tw = if matches!(spec.kind, Kind::Plain) {
                    t.width
                } else {
                    t.fit_width()
                };
                (tw + 2.0 * mx, t.height + 2.0 * my)
            }
            (_, Some(f)) => (f.w, f.h),
            _ => (0.0, 0.0),
        };
        // 画像の入る大きさ（1 画素を 1 点。ラベルは画像の上に重ねる）
        if let Some(p) = &picture {
            let (iw, ih) = p.size(1.0);
            lw = lw.max(iw);
            lh = lh.max(ih);
        }
        // plain は幅と高さが 0（属性は読まない。fixedsize なら大きさも 0。Graphviz の絵と同じ）
        let min_w = if spec.no_margin {
            0.0
        } else {
            a.num("width")
                .map_or(0.75 * INCH, |v| v.clamp(0.0, 100.0) * INCH)
        };
        let min_h = if spec.no_margin {
            0.0
        } else {
            a.num("height")
                .map_or(0.5 * INCH, |v| v.clamp(0.0, 100.0) * INCH)
        };
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
            let plain = spec.no_margin;
            let base = spec.base.as_ref().unwrap_or(&spec.kind);
            let (fw, fh) = shapes::fit(base, lw, lh, if plain { 0.0 } else { min_h });
            if spec.regular {
                // 同じ幅と高さ: 下限は幅と高さの下限の小さいほう（Graphviz の絵と同じ）
                let floor = if plain { 0.0 } else { min_w.min(min_h) };
                let m = fw.max(fh).max(floor);
                w = m;
                h = m;
            } else {
                w = if plain { fw } else { fw.max(min_w) };
                h = if plain { fh } else { fh.max(min_h) };
            }
            // ゆがみ・傾きの分（横向きの配置でも、形の向きで）
            w *= spec.grow.0;
            h *= spec.grow.1;
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
        // plain は塗らない（Graphviz の絵と同じ）
        let filled = !spec.no_margin
            && (has(&st, "filled")
                || has(&st, "radial")
                || has(&st, "striped")
                || has(&st, "wedged"));
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
        } else {
            // 塗りの指定がなければ塗らない（Graphviz と同じく透明）
            " fill=\"none\"".to_owned()
        };
        if invis {
            fill = " fill=\"none\"".to_owned();
        }
        // 自分の塗りのないノードは、外の地（文が決めたクラスタの塗り・グラフの bgcolor）の上で読める色で描く
        let ground_ink = grounds.at(node.cluster).1.and_then(Color::ink);
        let bare = !filled && !point;
        let stroke = format!(
            "{}{}",
            paint(
                "stroke",
                line_color,
                match ground_ink {
                    Some(c) if bare => c,
                    _ if point => LINE,
                    _ => BORDER,
                }
            ),
            line_attrs(&st, pen)
        );
        // 濃い塗りの上の既定の文字は白に
        let ink = if font.color.is_none() && bare {
            ground_ink
        } else if font.color.is_none() && filled {
            fill_text
                .as_deref()
                .and_then(|t| {
                    color::parse_list(t, scheme.as_deref().or(ctx.scheme.as_deref()))
                        .into_iter()
                        .find_map(|s| s.color)
                })
                .and_then(Color::ink)
        } else {
            None
        };
        let special = match spec.kind {
            Kind::Special(s) => Some(s),
            _ => None,
        };
        let inner_w = (w - 2.0 * mx).max(0.0);
        // labelloc: 形の中のラベルの入る高さの上・下に寄せる
        let label_dy = match a
            .get("labelloc")
            .and_then(|s| s.trim().chars().next())
            .map(|c| c.to_ascii_lowercase())
        {
            Some(side @ ('t' | 'b')) if rec.is_none() && !point => {
                let (_, ky) = shapes::fit(&spec.kind, 1.0, 1.0, 0.0);
                let room = (h / ky.max(1e-9) - lh).max(0.0) / 2.0;
                if side == 't' { -room } else { room }
            }
            _ => 0.0,
        };
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
            image: picture.map(|p| {
                (
                    p,
                    Scale::parse(a.get("imagescale")),
                    image_pos(a.get("imagepos")),
                )
            }),
            label_dy,
            ground_ink,
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
            // 線の脇に置くので、アプリの書体が広くても線に重ならないように
            let (w, h) = (t.fit_width() + 4.0, t.height + 2.0);
            if dir.sideways() { (h, w) } else { (w, h) }
        });
        // ortho の線はポートを見ないので、並びと位置もポートを見ない（Graphviz と同じ）
        let off = |node: usize, port: Option<&str>| -> f64 {
            match port_of(&views[node], port).filter(|_| !ortho) {
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
            invisible: has(&styles(a.get("style")), "invis"),
            bundle: !(free || ortho)
                && e.tail != e.head
                && size.is_none()
                && !has(&styles(a.get("style")), "invis")
                && [tail_port, head_port]
                    .into_iter()
                    .flatten()
                    .all(|p| p.trim().is_empty()),
        });
        eviews.push(EdgeView {
            label,
            head_label,
            tail_label,
            xlabel,
            font,
        });
    }

    // 自分への辺の輪の分を脇に空ける。i 本目の輪は、ノードの脇から `nodesep` の i 倍まで出る（Graphviz の絵と同じ）。
    // 内側の輪にラベルがあれば、その分だけ外の輪を外へ出す（Graphviz はラベルと外の輪を重ねる）
    let mut loop_room = vec![0f64; views.len()];
    let mut loop_count = vec![0usize; views.len()];
    let mut loop_reach = vec![0f64; model.edges.len()];
    for (i, e) in model.edges.iter().enumerate() {
        if e.tail == e.head {
            let lw = eviews[i]
                .label
                .as_ref()
                .map_or(0.0, |t| if dir.sideways() { t.height } else { t.width });
            let reach = loop_room[e.tail] + nodesep;
            loop_reach[i] = reach;
            loop_count[e.tail] += 1;
            loop_room[e.tail] = reach + if lw > 0.0 { lw + 6.0 } else { 0.0 };
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
        let (lw, lh) = label.as_ref().map_or((0.0, 0.0), |t| {
            (t.fit_width() + 2.0 * TITLE_PAD, t.height + 4.0)
        });
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
                // 輪は配置の右に出す。横向きの配置（LR・RL）では絵の上になる側（配置の左）
                extra_left: if dir.sideways() { loop_room[i] } else { 0.0 },
                extra_right: if dir.sideways() { 0.0 } else { loop_room[i] },
                key: model.nodes[i]
                    .name
                    .bytes()
                    .fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
                        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
                    }),
            }
        })
        .collect();
    let cluster_rank = !g.flag("newrank").unwrap_or(false)
        && !g
            .get("clusterrank")
            .is_some_and(|s| matches!(s.trim().to_ascii_lowercase().as_str(), "global" | "none"));
    // クラスタごとにランクを決めるとき、クラスタの部分グラフそのものの `rank` は読まない（Graphviz の絵と同じ。
    // 中の普通の部分グラフの `rank` は効く）
    let sets: Vec<(crate::graph::RankKind, Vec<usize>)> = model
        .rank_sets
        .iter()
        .filter(|s| !(cluster_rank && s.on_cluster))
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
        cluster_rank,
    };
    let lay = if free {
        let placed = place_free(engine, model, &lnodes, &lclusters, &mut ctx);
        if let Some(sizes) = &placed.sizes {
            // patchwork: ノードは升いっぱいの箱
            let boxed = shapes::spec("box", None, 0.0, 0.0, 0.0, false).kind;
            for (v, &(w, h)) in views.iter_mut().zip(sizes) {
                v.geom.kind = boxed.clone();
                v.geom.w = w;
                v.geom.h = h;
                v.geom.peripheries = v.geom.peripheries.min(1);
                v.inner_w = w;
            }
        }
        free_layout(model, &placed)
    } else {
        let mut lay = layout::layout(&lnodes, &ledges, &lclusters, &sets, &lopt);
        // 仮のノードの上限を超えて直線にした長い辺（ortho では直角にできなかった線の注記になる）
        if lay.straightened && !ortho {
            ctx.notes.insert(Note::LongEdges);
        }
        if ortho {
            // 直角の線の段の場所をランクの間に空ける（縁から 4 まで使い、1 つの間で足すのは ranksep まで）
            let heights: Vec<f64> = lnodes.iter().map(|n| n.h).collect();
            layout::make_room_for_tracks(&mut lay, &heights, route::TRACK, 4.0, ranksep);
        }
        // ratio: 位置だけを伸ばして縦横の比を合わせる（仕上がりの向きで比べる）
        let (span_x, span_y) = layout::center_span(&lay);
        let (w, h, span) = if dir.sideways() {
            (lay.height, lay.width, (span_y, span_x))
        } else {
            (lay.width, lay.height, (span_x, span_y))
        };
        let (fx, fy) = ratio_scale(g.get("ratio"), g.get("size"), w, h, span);
        let (sx, sy) = if dir.sideways() { (fy, fx) } else { (fx, fy) };
        layout::stretch(&mut lay, sx, sy);
        lay
    };
    let lnodes: Vec<layout::Node> = if free {
        lnodes
            .iter()
            .zip(&views)
            .map(|(n, v)| {
                let (w, h) = v.geom.outer();
                layout::Node { w, h, ..n.clone() }
            })
            .collect()
    } else {
        lnodes
    };
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
    // クラスタの題の箱（絵の座標）とクラスタ。辺が避ける（そのクラスタの中のノードへの辺は避けない）
    let mut title_rects: Vec<([f64; 4], usize)> = Vec::new();
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
        // 題の下の地（題の文字の色を決める。グラデーションは色の平均、なければ外の地）
        let ground = grounds.at(Some(ci)).1;
        let fill = if filled {
            match fill_text.as_deref() {
                Some(t) if t.contains(':') && !has(&st, "striped") => gradient(
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
            gradient(
                &mut ctx,
                &bg,
                scheme.as_deref(),
                has(&st, "radial"),
                a.num("gradientangle").unwrap_or(0.0),
            )
        } else {
            // 塗りの指定がなければ塗らない（Graphviz と同じ）
            " fill=\"none\"".to_owned()
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
        // striped: 色の並びの縦の帯（枠の線は帯の上に描き直す）
        let bands = if has(&st, "striped") {
            let text = fill_text.clone().unwrap_or_default();
            bands(
                &mut ctx,
                &text,
                scheme.as_deref(),
                b[0],
                b[1],
                b[2] - b[0],
                b[3] - b[1],
            )
        } else {
            String::new()
        };
        let rect = |fill: &str, stroke: &str| {
            format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{r}{fill}{stroke}/>",
                num(b[0]),
                num(b[1]),
                num(b[2] - b[0]),
                num(b[3] - b[1])
            )
        };
        clusters_out.push_str(&format!(
            "<g class=\"dot-cluster\" data-id=\"{}\">",
            esc(&c.name)
        ));
        if bands.is_empty() {
            clusters_out.push_str(&rect(&fill, &stroke));
        } else {
            clusters_out.push_str(&bands);
            clusters_out.push_str(&rect(" fill=\"none\"", &stroke));
        }
        let cv = &cviews[ci];
        if let Some(t) = &cv.label {
            let margin = lclusters[ci].margin;
            let cx = match cv.just {
                'l' => b[0] + margin + t.fit_width() / 2.0,
                'r' => b[2] - margin - t.fit_width() / 2.0,
                _ => (b[0] + b[2]) / 2.0,
            };
            let cy = if cv.loc_bottom {
                b[3] - margin / 2.0 - t.height / 2.0
            } else {
                b[1] + margin / 2.0 + 2.0 + t.height / 2.0
            };
            title_rects.push((
                [
                    cx - t.fit_width() / 2.0 - 4.0,
                    cy - t.height / 2.0 - 1.0,
                    cx + t.fit_width() / 2.0 + 4.0,
                    cy + t.height / 2.0 + 1.0,
                ],
                ci,
            ));
            clusters_out.push_str(&text_svg(
                t,
                cx,
                cy,
                0.0,
                &cv.font,
                &format!(" class=\"dot-cluster-label\"{}", font_attrs(&cv.font)),
                // 地がなければ文字の色（Graphviz と同じくほかのラベルと同じ色）
                ground.and_then(Color::ink),
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
    // 題の箱の持ち主の番号はノードの後ろ（ノードの数 + クラスタ）
    let title_owner = |ci: usize| views.len() + ci;
    let titles: Vec<(route::Rect, usize)> = title_rects
        .iter()
        .map(|(r, ci)| {
            let (p, q) = (from_final((r[0], r[1])), from_final((r[2], r[3])));
            (
                [p.0.min(q.0), p.1.min(q.1), p.0.max(q.0), p.1.max(q.1)],
                title_owner(*ci),
            )
        })
        .collect();
    // 辺が避けない箱: 両端のノードと、両端のノードのあるクラスタ（外のクラスタも）の題。クラスタの中のノードへの
    // 線は題を横切る（題の真下のノードへの線を脇に回さない。Graphviz と同じ）
    let edge_skip = |ei: usize| -> Vec<usize> {
        let e = &model.edges[ei];
        let mut skip = vec![e.tail, e.head];
        for v in [e.tail, e.head] {
            let mut c = model.nodes[v].cluster;
            let mut depth = 0;
            while let Some(ci) = c {
                if !skip.contains(&title_owner(ci)) {
                    skip.push(title_owner(ci));
                }
                c = model.clusters[ci].parent;
                depth += 1;
                if depth > 10_000 {
                    break;
                }
            }
        }
        skip
    };
    // 間を越える同じランクの弧（隣でない組と、隣でも 2 本目から）が水平に進む高さ（配置の座標）。ランクのすぐ上の
    // 空きを通り、同じランクの間のノードの上を越える（上のランクのノードは越えない。Graphviz と同じ）。同じランクで
    // 横の範囲が重なる弧は、短いものから置き、中に収まる弧より 1 段（8）上、一部だけ重なる弧とは別の高さにする
    let flat_top: Vec<Option<f64>> = {
        // (辺, 行の y, 左, 右, 間のノードを越える高さ)
        let mut arcs: Vec<(usize, f64, f64, f64, f64)> = Vec::new();
        for (ei, e) in model.edges.iter().enumerate() {
            let Route::Flat { lane, adjacent } = lay.routes[ei] else {
                continue;
            };
            if (adjacent && lane == 0) || has(&styles(e.attrs.get("style")), "invis") {
                continue;
            }
            let (pa, pb) = (lay.nodes[e.tail], lay.nodes[e.head]);
            let (lo, hi) = (pa.0.min(pb.0), pa.0.max(pb.0));
            let mut top = (pa.1 - lnodes[e.tail].h / 2.0).min(pb.1 - lnodes[e.head].h / 2.0) - 10.0;
            for (v, &(x, y)) in lay.nodes.iter().enumerate() {
                if v == e.tail || v == e.head || views[v].invis || (y - pa.1).abs() > 1.0 {
                    continue;
                }
                let half = lnodes[v].w / 2.0;
                if x + half > lo && x - half < hi {
                    top = top.min(y - lnodes[v].h / 2.0 - 8.0);
                }
            }
            // 同じ行の隣どうしの辺のラベル（その線の上に置く）も越える
            for (fi, f) in model.edges.iter().enumerate() {
                let (
                    Route::Flat {
                        adjacent: true,
                        lane: 0,
                    },
                    Some(t),
                ) = (&lay.routes[fi], eviews[fi].label.as_ref())
                else {
                    continue;
                };
                let (fa, fb) = (lay.nodes[f.tail], lay.nodes[f.head]);
                let mid = (fa.0 + fb.0) / 2.0;
                let (lw, lh) = if dir.sideways() {
                    (t.height, t.width)
                } else {
                    (t.width, t.height)
                };
                if (fa.1 - pa.1).abs() < 1.0 && mid + lw / 2.0 > lo && mid - lw / 2.0 < hi {
                    top = top.min(fa.1 - lh - 2.0 - 8.0);
                }
            }
            arcs.push((ei, pa.1, lo, hi, top));
        }
        arcs.sort_by(|a, b| {
            a.1.total_cmp(&b.1)
                .then((a.3 - a.2).total_cmp(&(b.3 - b.2)))
                .then(a.0.cmp(&b.0))
        });
        let mut out: Vec<Option<f64>> = vec![None; model.edges.len()];
        let mut placed: Vec<(f64, f64, f64, f64)> = Vec::new(); // (行の y, 左, 右, 高さ)
        for (ei, row, lo, hi, base) in arcs {
            let same: Vec<(f64, f64, f64)> = placed
                .iter()
                .filter(|p| (p.0 - row).abs() < 1.0 && p.1 < hi + 1.0 && p.2 > lo - 1.0)
                .map(|p| (p.1, p.2, p.3))
                .collect();
            // 中に収まる弧より上
            let mut top = base;
            for &(l, h, t) in &same {
                if l >= lo - 1.0 && h <= hi + 1.0 {
                    top = top.min(t - 8.0);
                }
            }
            // 一部だけ重なる弧とは、同じ高さを避ける
            for _ in 0..same.len() {
                match same.iter().find(|s| (s.2 - top).abs() < 7.5) {
                    Some(s) => top = top.min(s.2 - 8.0),
                    None => break,
                }
            }
            placed.push((row, lo, hi, top));
            out[ei] = Some(top);
        }
        out
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
        for (r, owner) in &titles {
            rects.push((*r, Some(*owner)));
        }
        // 配置が場所を空けた辺のラベル（仮のノードの脇と、隣どうしの同じランクの線の上）。ほかの辺はここを通らない
        if !free {
            for (ei, e) in model.edges.iter().enumerate() {
                let Some(t) = eviews[ei].label.as_ref() else {
                    continue;
                };
                let (lw, lh) = if dir.sideways() {
                    (t.height, t.width)
                } else {
                    (t.width, t.height)
                };
                let at = match &lay.routes[ei] {
                    Route::Path {
                        points,
                        label: Some(l),
                    } if points.len() > 2 => Some(*l),
                    Route::Flat {
                        adjacent: true,
                        lane: 0,
                    } => {
                        let (pa, pb) = (lay.nodes[e.tail], lay.nodes[e.head]);
                        Some(((pa.0 + pb.0) / 2.0, pa.1 - lh / 2.0 - 2.0))
                    }
                    Route::Flat { .. } => flat_top[ei].map(|top| {
                        let (pa, pb) = (lay.nodes[e.tail], lay.nodes[e.head]);
                        ((pa.0 + pb.0) / 2.0, top - lh / 2.0 - 2.0)
                    }),
                    _ => None,
                };
                if let Some((x, y)) = at {
                    let (w, h) = (lw / 2.0 + NODE_PAD, lh / 2.0 + NODE_PAD);
                    rects.push(([x - w, y - h, x + w, y + h], None));
                }
            }
        }
        route::Obstacles::new(rects)
    };
    // splines=ortho の道筋（配置の座標）
    let ortho_paths: Vec<Option<Vec<P>>> = if ortho && free {
        // 引ける辺だけまとめて引き、辺の番号に戻す
        let mut which: Vec<usize> = Vec::new();
        let mut input: Vec<route::FreeEdge> = Vec::new();
        for (ei, e) in model.edges.iter().enumerate() {
            if let Route::Path { points, .. } = &lay.routes[ei]
                && points.len() >= 2
            {
                which.push(ei);
                input.push(route::FreeEdge {
                    a: points[0],
                    b: points[points.len() - 1],
                    ha: (lnodes[e.tail].w / 2.0, lnodes[e.tail].h / 2.0),
                    hb: (lnodes[e.head].w / 2.0, lnodes[e.head].h / 2.0),
                    tail: e.tail,
                    head: e.head,
                    skip: edge_skip(ei),
                });
            }
        }
        let mut out: Vec<Option<Vec<P>>> = vec![None; model.edges.len()];
        for (ei, p) in which
            .into_iter()
            .zip(route::ortho_free_all(&input, &mut obstacles))
        {
            out[ei] = Some(p);
        }
        out
    } else if ortho {
        let input: Vec<route::OrthoEdge> = model
            .edges
            .iter()
            .enumerate()
            .map(|(ei, e)| {
                let a = &e.attrs;
                // ortho の線はポートを見ない（Graphviz と同じ）
                let end = |same: &str| route::OrthoEnd {
                    port: route::OrthoPort::Free,
                    same: a.get(same).filter(|v| !v.is_empty()).map(str::to_owned),
                };
                let (points, label) = match &lay.routes[ei] {
                    Route::Path { points, label } => (points.clone(), label.is_some()),
                    _ => (vec![], false),
                };
                route::OrthoEdge {
                    points,
                    tail: e.tail,
                    head: e.head,
                    tail_end: end("sametail"),
                    head_end: end("samehead"),
                    keep: label,
                    skip: edge_skip(ei),
                }
            })
            .collect();
        let boxes: Vec<(P, f64, f64)> = (0..views.len())
            .map(|i| (lay.nodes[i], lnodes[i].w, lnodes[i].h))
            .collect();
        route::ortho(&input, &boxes, &titles, &mut obstacles)
    } else {
        vec![]
    };

    // samehead・sametail: 同じ値の辺がノードの外形の 1 点で会う（向こうの点の平均の向き）
    // 会う点と外向きの向き（矢じりの向きをそろえる）
    let mut meet: Vec<[Option<(P, P)>; 2]> = vec![[None, None]; model.edges.len()];
    if !ortho {
        let mut groups: HashMap<(usize, bool, &str), Vec<usize>> = HashMap::new();
        for (ei, e) in model.edges.iter().enumerate() {
            if e.tail == e.head {
                continue;
            }
            for (head, key) in [(true, "samehead"), (false, "sametail")] {
                if let Some(v) = e.attrs.get(key).filter(|v| !v.is_empty()) {
                    let node = if head { e.head } else { e.tail };
                    groups.entry((node, head, v)).or_default().push(ei);
                }
            }
        }
        for ((node, head, _), list) in groups {
            if list.len() < 2 {
                continue;
            }
            let c = views[node].pos;
            let (mut sx, mut sy) = (0.0, 0.0);
            for &ei in &list {
                let e = &model.edges[ei];
                let other = if head { e.tail } else { e.head };
                let toward = match &lay.routes[ei] {
                    Route::Path { points, .. } if points.len() >= 2 => {
                        let k = if head { points.len() - 2 } else { 1 };
                        to_final(points[k])
                    }
                    _ => views[other].pos,
                };
                let (dx, dy) = (toward.0 - c.0, toward.1 - c.1);
                let l = dx.hypot(dy).max(1e-9);
                sx += dx / l;
                sy += dy / l;
            }
            if sx.hypot(sy) < 1e-6 {
                continue;
            }
            let b = views[node].geom.boundary(sx, sy);
            let l = sx.hypot(sy);
            for ei in list {
                meet[ei][usize::from(!head)] = Some(((c.0 + b.0, c.1 + b.1), (sx / l, sy / l)));
            }
        }
    }

    // 辺
    let compound = g.flag("compound").unwrap_or(false);
    let vertical_flow = !dir.sideways();
    // 辺の少ない図では、線を大きく曲げた形も試す（ノード・ラベル・先に引いた線に新しくかからないときだけ使う。
    // Gansner ほか 1993 §5 の「使える場所はすべて使い、先に引いた線には近づきすぎない」）。配置の座標
    let roomy = !(free || ortho) && model.edges.len() <= ROOMY_EDGES;
    let mut node_boxes: Vec<[f64; 4]> = Vec::new();
    let mut label_boxes: Vec<[f64; 4]> = Vec::new();
    if roomy {
        for (p, n) in lay.nodes.iter().zip(&lnodes) {
            node_boxes.push([
                p.0 - n.w / 2.0 - 2.0,
                p.1 - n.h / 2.0 - 2.0,
                p.0 + n.w / 2.0 + 2.0,
                p.1 + n.h / 2.0 + 2.0,
            ]);
        }
        for (r, le) in lay.routes.iter().zip(&ledges) {
            if let (Route::Path { label: Some(l), .. }, Some((w, h))) = (r, le.label) {
                label_boxes.push([l.0 - w / 2.0, l.1 - h / 2.0, l.0 + w / 2.0, l.1 + h / 2.0]);
            }
        }
    }
    // 引いた線（折れ線と外接の箱）
    let mut drawn: Vec<(Vec<P>, [f64; 4])> = Vec::new();
    // 同じ 2 つのノードを結ぶ辺の束（向きは問わない。ラベルもポートもない見える辺）: 1 本の線に重ならないように
    // 離す（Gansner ほか 1993 §5.1.1 の多重の辺）。隣り合うランクの辺は、書いた順に左から `nodesep` ずつ制御点を
    // ずらす（線の真ん中の間は nodesep の 3/4。横向きの配置では絵の下から）。仮のノードのある辺は、それぞれの仮の
    // ノードを通す（通り道の中を真っすぐ張ると、隣の仮のノードとの境に寄って重なる）
    let mut bundle: Vec<Option<f64>> = vec![None; model.edges.len()];
    if !(free || ortho) {
        let mut groups: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
        for (ei, e) in model.edges.iter().enumerate() {
            let a = &e.attrs;
            let plain = e.tail != e.head
                && matches!(&lay.routes[ei], Route::Path { label: None, .. })
                && !ledges[ei].invisible
                && [
                    a.get("tailport").or(e.tail_port.as_deref()),
                    a.get("headport").or(e.head_port.as_deref()),
                ]
                .into_iter()
                .flatten()
                .all(|p| p.trim().is_empty());
            if plain {
                groups
                    .entry((e.tail.min(e.head), e.tail.max(e.head)))
                    .or_default()
                    .push(ei);
            }
        }
        let side = if dir.sideways() { -1.0 } else { 1.0 };
        for list in groups.values().filter(|l| l.len() > 1) {
            let mid = (list.len() - 1) as f64 / 2.0;
            for (k, &ei) in list.iter().enumerate() {
                bundle[ei] = Some(side * (k as f64 - mid) * nodesep);
            }
        }
    }
    // 同じノードに同じ向きから着く線（または出る線）は、ノードの中心を目指すと外形の手前で合わさり、矢印も重なる。
    // 向き（角度）の近い線どうしは、目指す点を向きと直角に `ARRIVAL_SPREAD` ずつ分ける（来る側の順に）。込んだ大きな
    // 図ではずらした線がほかのノードにかかるので、線を選び直す図（`ROOMY_EDGES` まで）だけ
    let mut end_shift: Vec<[P; 2]> = vec![[(0.0, 0.0); 2]; model.edges.len()];
    if roomy {
        // (ノード, 頭の端か) → (向きの角度, 辺)
        let mut arrivals: HashMap<(usize, bool), Vec<(f64, usize)>> = HashMap::new();
        for (ei, e) in model.edges.iter().enumerate() {
            let Route::Path { points, .. } = &lay.routes[ei] else {
                continue;
            };
            let a = &e.attrs;
            let ported = [
                a.get("tailport").or(e.tail_port.as_deref()),
                a.get("headport").or(e.head_port.as_deref()),
            ]
            .into_iter()
            .flatten()
            .any(|p| !p.trim().is_empty());
            if points.len() < 2 || e.tail == e.head || ported || bundle[ei].is_some() {
                continue;
            }
            let last = points.len() - 1;
            for (head, p, q) in [
                (false, points[0], points[1]),
                (true, points[last], points[last - 1]),
            ] {
                if meet[ei][usize::from(!head)].is_some() {
                    continue;
                }
                let node = if head { e.head } else { e.tail };
                arrivals
                    .entry((node, head))
                    .or_default()
                    .push(((q.1 - p.1).atan2(q.0 - p.0), ei));
            }
        }
        for ((_, head), mut list) in arrivals {
            list.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            let mut i = 0;
            while i < list.len() {
                let mut j = i + 1;
                while j < list.len() && list[j].0 - list[j - 1].0 < ARRIVAL_ANGLE {
                    j += 1;
                }
                if j - i > 1 {
                    let mid = (j - i - 1) as f64 / 2.0;
                    let mean = list[i..j].iter().map(|x| x.0).sum::<f64>() / (j - i) as f64;
                    // 向きと直角（角度の大きい側へ）
                    let normal = (-mean.sin(), mean.cos());
                    for (n, &(_, ei)) in list[i..j].iter().enumerate() {
                        let d = (n as f64 - mid) * ARRIVAL_SPREAD;
                        end_shift[ei][usize::from(head)] = (normal.0 * d, normal.1 * d);
                    }
                }
                i = j;
            }
        }
    }
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
        // ortho の線はポートを見ない（Graphviz と同じ）
        let port = |v: &NodeView, key: &str, own: Option<&str>| {
            port_of(v, a.get(key).or(own)).filter(|_| !ortho)
        };
        let mut tail_port = port(tv, "tailport", e.tail_port.as_deref());
        let mut head_port = port(hv, "headport", e.head_port.as_deref());
        let head_clip = a.flag("headclip").unwrap_or(true);
        let tail_clip = a.flag("tailclip").unwrap_or(true);
        let mut path: Vec<Cubic>;
        let mut label_at: Option<P>;
        // 通り道の中の曲線で引いた（端の向きは道筋が決めた）
        let mut routed = false;
        match &lay.routes[ei] {
            Route::Path { points, label } => {
                let mut pts: Vec<P> = points.clone();
                let right_angles = ortho_paths.get(ei).cloned().flatten();
                if ortho && right_angles.is_none() {
                    // 直角にできなかった線（仮のノードの上限を超えた長い線）
                    ctx.notes.insert(Note::Ortho);
                }
                // dot の曲線は、通り道の中の最短の折れ線を引いてから避け、通り道に収まる曲線にする
                // ポートのある端: レコード・表の内側の欄は、先に外形に触れる辺（向こうの端の側）へ付け替える。欄が
                // 外形に触れていれば欄の幅の中から、向きのあるポートは向きが進む向きと合うか横ならその点から出入り
                // する。向きが逆なら前のまま（仮のノードを縦の向きで通る曲線）
                let mut gates: Vec<layout::Portal> =
                    lay.corridors.get(ei).cloned().unwrap_or_default();
                let down = lay.nodes[e.head].1 >= lay.nodes[e.tail].1;
                if !(free || ortho) {
                    outer_side(tv, &mut tail_port, hv.pos);
                    outer_side(hv, &mut head_port, tv.pos);
                }
                let mut port_end =
                    |port: &Option<Port>, view: &NodeView, tail: bool| -> Option<P> {
                        let Some(p) = port else {
                            return Some((f64::NAN, f64::NAN));
                        };
                        let at = from_final((view.pos.0 + p.at.0, view.pos.1 + p.at.1));
                        let gate = if tail {
                            gates.first_mut()?
                        } else {
                            gates.last_mut()?
                        };
                        // 出る向き（尾）・入る向き（頭）が配置の下向きか
                        let outward_down = if tail { down } else { !down };
                        if let Some(d) = p.dir {
                            let o = from_final(view.pos);
                            let q = from_final((view.pos.0 + d.0, view.pos.1 + d.1));
                            let dy = q.1 - o.1;
                            if dy.abs() > 1e-6 && (dy > 0.0) != outward_down {
                                return None;
                            }
                            if dy.abs() > 1e-6 {
                                gate.lo = at.0;
                                gate.hi = at.0;
                            }
                        } else if let Some(b) = p.clip {
                            let (c0, c1) = (
                                from_final((view.pos.0 + b[0], view.pos.1 + b[1])),
                                from_final((view.pos.0 + b[2], view.pos.1 + b[3])),
                            );
                            gate.lo = c0.0.min(c1.0);
                            gate.hi = c0.0.max(c1.0);
                        }
                        Some(at)
                    };
                let tail_at = port_end(&tail_port, tv, true);
                let head_at = port_end(&head_port, hv, false);
                let fitted = !(free || straight || curved || polyline || right_angles.is_some())
                    && !gates.is_empty()
                    && tail_at.is_some()
                    && head_at.is_some();
                let gates = &gates[..];
                routed = fitted;
                if fitted {
                    // samehead・sametail の辺は、会う点から引く。ポートのある端はポートの点から
                    let [meet_head, meet_tail] = meet[ei];
                    let start = match (meet_tail, tail_at) {
                        (_, Some(p)) if !p.0.is_nan() => p,
                        (Some((m, _)), _) => from_final(m),
                        _ => {
                            let (p, d) = (pts[0], end_shift[ei][0]);
                            (p.0 + d.0, p.1 + d.1)
                        }
                    };
                    let end = match (meet_head, head_at) {
                        (_, Some(p)) if !p.0.is_nan() => p,
                        (Some((m, _)), _) => from_final(m),
                        _ => {
                            let (p, d) = (pts[pts.len() - 1], end_shift[ei][1]);
                            (p.0 + d.0, p.1 + d.1)
                        }
                    };
                    pts = spline::shortest(start, end, gates);
                    obstacles.avoid(&mut pts, &edge_skip(ei), false);
                } else if !(straight || curved || right_angles.is_some()) {
                    // dot 以外は点の間を直線として避け、あとでなめらかにつなぐ
                    obstacles.avoid(&mut pts, &edge_skip(ei), !polyline && !free);
                }
                let angled = right_angles.is_some();
                let mut lp: Vec<P> = right_angles.unwrap_or_else(|| pts.clone());
                if (straight && !free) || curved {
                    lp = vec![pts[0], pts[pts.len() - 1]];
                }
                path = if free && !curved && !polyline && !angled {
                    // 多重の辺の中の点も通る
                    curve::smooth(&lp)
                } else if curved {
                    let (a0, b0) = (lp[0], lp[1]);
                    let (mx, my) = ((a0.0 + b0.0) / 2.0, (a0.1 + b0.1) / 2.0);
                    let (dx, dy) = (b0.0 - a0.0, b0.1 - a0.1);
                    let bend = (mx - dy * 0.2, my + dx * 0.2);
                    vec![Cubic(a0, bend, bend, b0)]
                } else if let Some(d) = bundle[ei].filter(|_| fitted && points.len() == 2) {
                    // 束の中の隣り合うランクの線: 両端はそのまま、間を横にずらす（避ける点が入っていれば、その点ごと）
                    if lp.len() == 2 {
                        let (p, q) = (lp[0], lp[1]);
                        let third = ((q.0 - p.0) / 3.0, (q.1 - p.1) / 3.0);
                        vec![Cubic(
                            p,
                            (p.0 + third.0 + d, p.1 + third.1),
                            (q.0 - third.0 + d, q.1 - third.1),
                            q,
                        )]
                    } else {
                        let last = lp.len() - 1;
                        for p in &mut lp[1..last] {
                            p.0 += d * 0.75;
                        }
                        curve::smooth(&lp)
                    }
                } else if let Some(d) = bundle[ei].filter(|_| fitted) {
                    // 束ねた長い辺（仮のノードの列を共にする）: 真ん中の線を引き、点を横にずらす。両端は半分だけずらして
                    // ノードの外形で切る（Graphviz と同じく、束の矢印が 1 か所に重ならない）
                    let mut path = spline::fit(&lp, gates);
                    let last = path.len() - 1;
                    for (i, seg) in path.iter_mut().enumerate() {
                        let Cubic(p, c1, c2, q) = seg;
                        c1.0 += d;
                        c2.0 += d;
                        p.0 += if i > 0 { d } else { d / 2.0 };
                        q.0 += if i < last { d } else { d / 2.0 };
                    }
                    path
                } else if fitted {
                    spline::fit(&lp, gates)
                } else {
                    curve::through(&lp, straight || polyline, true)
                };
                if fitted && roomy && bundle[ei].is_none() {
                    let bbox = |poly: &[P]| -> [f64; 4] {
                        let mut b = [
                            f64::INFINITY,
                            f64::INFINITY,
                            f64::NEG_INFINITY,
                            f64::NEG_INFINITY,
                        ];
                        for p in poly {
                            b = [b[0].min(p.0), b[1].min(p.1), b[2].max(p.0), b[3].max(p.1)];
                        }
                        b
                    };
                    // かかるノード（両端を除く）の数、ラベルの数、先に引いた線との交差の数
                    let cost = |poly: &[P]| -> [usize; 3] {
                        let bb = bbox(poly);
                        let over = |b: &[f64; 4]| {
                            b[0] <= bb[2] && b[2] >= bb[0] && b[1] <= bb[3] && b[3] >= bb[1]
                        };
                        let nodes = node_boxes
                            .iter()
                            .enumerate()
                            .filter(|&(v, b)| {
                                v != e.tail && v != e.head && over(b) && spline::hits_box(poly, b)
                            })
                            .count();
                        let labels = label_boxes
                            .iter()
                            .filter(|b| over(b) && spline::hits_box(poly, b))
                            .count();
                        let cross = drawn
                            .iter()
                            .filter(|(_, b)| over(b))
                            .map(|(q, _)| spline::crossings(poly, q))
                            .sum();
                        [nodes, labels, cross]
                    };
                    let mut poly = spline::sample(&path);
                    let cand = spline::fit_roomy(&lp, gates);
                    if cand != path {
                        let base = cost(&poly);
                        let q = spline::sample(&cand);
                        let c = cost(&q);
                        if c.iter().zip(&base).all(|(a, b)| a <= b) {
                            path = cand;
                            poly = q;
                        }
                    }
                    let b = bbox(&poly);
                    drawn.push((poly, b));
                }
                path = path.iter().map(|c| c.map(to_final)).collect();
                // 向きのあるポート（e など）の端は、その向きに出入りする（Graphviz の絵と同じ。e から出る線はいったん
                // 右へ出てから曲がる。向きのない欄はランクの向き）。端の切れ端の制御点を、その向きに弦の 1/3（4〜36）
                // だけ出す
                if fitted && !path.is_empty() {
                    let reach =
                        |c: &Cubic| ((c.3.0 - c.0.0).hypot(c.3.1 - c.0.1) / 3.0).clamp(4.0, 36.0);
                    // 向きのない欄（レコード・表）は、ランクの向きに出入りする
                    let flow = |out_down: bool| -> P {
                        let o = to_final((0.0, 0.0));
                        let q = to_final((0.0, if out_down { 1.0 } else { -1.0 }));
                        (q.0 - o.0, q.1 - o.1)
                    };
                    let along = |p: &Port, out_down: bool| -> Option<P> {
                        p.dir.or(p.clip.map(|_| flow(out_down)))
                    };
                    if let Some(d) = tail_port.as_ref().and_then(|p| along(p, down)) {
                        let c = &mut path[0];
                        let l = reach(c);
                        c.1 = (c.0.0 + d.0 * l, c.0.1 + d.1 * l);
                    }
                    if let Some(d) = head_port.as_ref().and_then(|p| along(p, !down)) {
                        let last = path.len() - 1;
                        let c = &mut path[last];
                        let l = reach(c);
                        c.2 = (c.3.0 + d.0 * l, c.3.1 + d.1 * l);
                    }
                }
                // ラベルの仮のノードの脇のラベルは、文字を線の側に寄せる（箱は書体の違いの分だけ文字より広い。
                // 文字の左端は仮のノードから `LABEL_GAP`）。横向きの配置では箱の余りは線と平行なので寄せない
                let hug = match (&ev.label, label, dir.sideways()) {
                    (Some(t), Some(l), false) => {
                        let box_w = t.fit_width() + 4.0;
                        // 仮のノード（同じ行の点）から間 + 箱の半分の右にあれば、その脇のラベル
                        let gap = layout::LABEL_GAP;
                        let beside = points
                            .iter()
                            .any(|p| p.1 == l.1 && (l.0 - p.0 - (gap + box_w / 2.0)).abs() < 0.01);
                        if beside { (box_w - t.width) / 2.0 } else { 0.0 }
                    }
                    _ => 0.0,
                };
                label_at = label.map(|(x, y)| to_final((x - hug, y)));
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
                    // ランクのすぐ上を、上がる・水平に進む・下りる（角は丸める）
                    let top = flat_top[ei].unwrap_or(pa.1 - h_a - 10.0);
                    path = if ortho {
                        // 上から出て、上を越える（ポートは見ない）
                        let (ca, cb) = (lay.nodes[e.tail], lay.nodes[e.head]);
                        let half = |v: usize| (lnodes[v].w / 2.0, lnodes[v].h / 2.0);
                        let pts = route::ortho_flat(
                            ca,
                            half(e.tail),
                            &route::OrthoPort::Free,
                            cb,
                            half(e.head),
                            &route::OrthoPort::Free,
                            top,
                            route::STUB,
                        );
                        pts.windows(2).map(|w| Cubic::line(w[0], w[1])).collect()
                    } else {
                        // 角の丸みの横の幅: 両端のノードの幅の半分（間が狭ければ真ん中まで）
                        let span = (pb.0 - pa.0).abs();
                        let sg = if pb.0 >= pa.0 { 1.0 } else { -1.0 };
                        let ra = (lnodes[e.tail].w / 2.0).max(10.0).min(span / 2.0);
                        let rb = (lnodes[e.head].w / 2.0).max(10.0).min(span / 2.0);
                        let (qa, qb) = ((pa.0 + sg * ra, top), (pb.0 - sg * rb, top));
                        let mut out = vec![Cubic(pa, (pa.0, top), (pa.0, top), qa)];
                        if (qb.0 - qa.0).abs() > 0.5 {
                            out.push(Cubic::line(qa, qb));
                        }
                        out.push(Cubic(qb, (pb.0, top), (pb.0, top), pb));
                        out
                    };
                    label_at = ev.label.as_ref().map(|t| {
                        let lh = if dir.sideways() { t.width } else { t.height };
                        ((pa.0 + pb.0) / 2.0, top - lh / 2.0 - 2.0)
                    });
                }
                path = path.iter().map(|c| c.map(to_final)).collect();
                label_at = label_at.map(to_final);
            }
            Route::Loop { index } => {
                let c = lay.nodes[e.tail];
                let ln = &lnodes[e.tail];
                let size = 18.0 + 8.0 * *index as f64;
                let has_port = [
                    a.get("tailport").or(e.tail_port.as_deref()),
                    a.get("headport").or(e.head_port.as_deref()),
                ]
                .into_iter()
                .flatten()
                .any(|p| !p.trim().is_empty());
                if !ortho && !has_port {
                    // ポートのない輪（Graphviz の絵に合わせた形）: ノードの中心から出て中心へ戻る 1 本の曲線を、外形で
                    // 切る。脇（配置の右。横向きの配置では絵の上になる左）へ `reach` まで出て、ランクの向きに始まりから
                    // 終わりへ回る。いちばん外の輪の高さはノードの高さ（ランクの向き）の 0.385 倍で、内側の輪ほど低い
                    let n = loop_count[e.tail].max(1) as f64;
                    let i = (*index + 1).min(loop_count[e.tail].max(1)) as f64;
                    let (hw, hh) = (ln.w / 2.0, ln.h / 2.0);
                    let out = if dir.sideways() { -1.0 } else { 1.0 };
                    let reach = loop_reach[ei].max(1.0);
                    // 隣のランクがあれば、輪はその中心までの距離の半分まで高くなる（Graphviz の絵で、ノードの高さと
                    // ranksep を変えて測った）
                    let r = lay.ranks[e.tail];
                    let gap = (!free)
                        .then(|| {
                            lay.ranks
                                .iter()
                                .zip(&lay.nodes)
                                .filter(|(q, _)| (**q - r).abs() == 1)
                                .map(|(_, p)| (p.1 - c.1).abs())
                                .fold(f64::INFINITY, f64::min)
                        })
                        .filter(|g| g.is_finite())
                        .unwrap_or(0.0);
                    let most = hh.max(gap / 2.0) * 0.385 * i / n;
                    // 両端が同じ点の 3 次曲線: 横は制御点までの 3/4、縦は制御点までの 0.2887 倍（3·t(1−t)(1−2t) の最大）
                    let x = out * (hw + reach) * 4.0 / 3.0;
                    let y = most / 0.2887;
                    let whole = Cubic(c, (c.0 + x, c.1 - y), (c.0 + x, c.1 + y), c);
                    // 外形を出る所から入る所まで（いちばん外の点がまだ中なら切らない）
                    let inside = |t: f64| {
                        let f = to_final(whole.at(t));
                        tv.geom.inside(f.0 - tv.pos.0, f.1 - tv.pos.1)
                    };
                    let edge = |from: f64| {
                        let (mut lo, mut hi) = (from, 0.5);
                        for _ in 0..30 {
                            let mid = (lo + hi) / 2.0;
                            if inside(mid) {
                                lo = mid;
                            } else {
                                hi = mid;
                            }
                        }
                        lo
                    };
                    let (t0, t1) = if inside(0.5) {
                        (0.0, 1.0)
                    } else {
                        (edge(0.0), edge(1.0))
                    };
                    let (head, _) = whole.split(t1);
                    let (_, cut) = head.split(if t1 > 0.0 { t0 / t1 } else { 0.0 });
                    path = vec![cut.map(to_final)];
                    label_at = ev.label.as_ref().map(|t| {
                        let l = if dir.sideways() { t.height } else { t.width };
                        to_final((c.0 + out * (hw + reach + l / 2.0 + 4.0), c.1))
                    });
                } else {
                    // 輪の側: 向きのあるポートがあればその側（尾が先）、なければ配置の右（Graphviz と同じ）。
                    // ortho はポートを見ない
                    let port_side = [
                        a.get("tailport").or(e.tail_port.as_deref()),
                        a.get("headport").or(e.head_port.as_deref()),
                    ]
                    .into_iter()
                    .filter(|_| !ortho)
                    .find_map(|p| match ortho_port(tv, p, dir) {
                        route::OrthoPort::Point { dir: d, .. } => {
                            Some(if d.0.abs() > d.1.abs() + 1e-6 {
                                (d.0.signum(), 0.0)
                            } else {
                                (0.0, d.1.signum())
                            })
                        }
                        _ => None,
                    });
                    let u = port_side.unwrap_or((1.0, 0.0));
                    let v = (-u.1, u.0);
                    // 外へ（u）の半分の大きさと、脇へ（v）の半分の大きさ
                    let ext = (u.0 * ln.w).abs().max((u.1 * ln.h).abs()) / 2.0;
                    let mut span = (v.0 * ln.w).abs().max((v.1 * ln.h).abs()) / 2.0;
                    if port_side.is_some() {
                        span = span.min(ext * 1.5);
                    }
                    let at = |p: f64, q: f64| (c.0 + u.0 * p + v.0 * q, c.1 + u.1 * p + v.1 * q);
                    // 輪の両端（脇への位置 q0・q1）と、ノードの中の始まり（外への位置 inner）。上下の輪は、ランクの
                    // 間の線が出入りする真ん中を空けて、その側の右の肩（中心から 0.3〜0.85）に置く
                    let (q0, q1, inner) = if port_side.is_some() && u.0 == 0.0 {
                        let sg = -u.1;
                        (span * 0.3 * sg, span * 0.85 * sg, ext * 0.2)
                    } else {
                        (-span * 0.6, span * 0.6, if ortho { ext } else { ext * 0.6 })
                    };
                    let bulge = if q1 >= q0 { 1.0 } else { -1.0 };
                    path = if ortho {
                        let r = ext + size;
                        vec![
                            Cubic::line(at(inner, q0), at(r, q0)),
                            Cubic::line(at(r, q0), at(r, q1)),
                            Cubic::line(at(r, q1), at(inner, q1)),
                        ]
                    } else {
                        // 既定の右の輪は、両端の外へ少しふくらむ（上下の肩の輪は、真ん中の側へはふくらませない）
                        let (b0, b1) = if port_side.is_some() && u.0 == 0.0 {
                            (q0 - bulge * size * 0.1, q1 + bulge * size * 0.4)
                        } else {
                            (-span - size * 0.4, span + size * 0.4)
                        };
                        vec![Cubic(
                            at(inner, q0),
                            at(ext + size, b0),
                            at(ext + size, b1),
                            at(inner, q1),
                        )]
                    };
                    label_at = ev.label.as_ref().map(|t| {
                        let (lw, lh) = if dir.sideways() {
                            (t.height, t.width)
                        } else {
                            (t.width, t.height)
                        };
                        let l = (u.0 * lw).abs().max((u.1 * lh).abs());
                        at(ext + size + l / 2.0 + 2.0, (q0 + q1) / 2.0)
                    });
                    path = path.iter().map(|c| c.map(to_final)).collect();
                    label_at = label_at.map(to_final);
                    if port_side.is_some() && u.0 == 0.0 && !ortho {
                        // 肩の輪の両端は、その位置から外へ（u の向きに）進んで外形に出る点（輪は外形で切らない）
                        let on = |q: f64| {
                            let inside = |t: f64| {
                                let f = to_final(at(t, q));
                                tv.geom.inside(f.0 - tv.pos.0, f.1 - tv.pos.1)
                            };
                            if !inside(0.0) {
                                return to_final(at(inner, q));
                            }
                            let (mut lo, mut hi) = (0.0, ext * 2.0 + 1.0);
                            for _ in 0..30 {
                                let mid = (lo + hi) / 2.0;
                                if inside(mid) {
                                    lo = mid;
                                } else {
                                    hi = mid;
                                }
                            }
                            to_final(at(lo, q))
                        };
                        let last = path.len() - 1;
                        path[0].0 = on(q0);
                        path[last].3 = on(q1);
                    }
                }
            }
        }
        let _ = vertical_flow;
        if path.is_empty() {
            continue;
        }
        // 内側の升・欄のポートは、外形に触れる辺から出入りする
        if !ortho && e.tail != e.head {
            let toward = path[0].3;
            outer_side(tv, &mut tail_port, toward);
            let toward = path[path.len() - 1].0;
            outer_side(hv, &mut head_port, toward);
        }
        // 端の点をポートに
        let reach = {
            let (a0, b0) = (path[0].0, path[path.len() - 1].3);
            ((b0.0 - a0.0).hypot(b0.1 - a0.1) / 3.0).clamp(20.0, 60.0)
        };
        // 直角の道と自分への輪は、端の側と位置を道筋で決めた（ノードの中から始まり、外形で切る）
        if ortho || e.tail == e.head {
            tail_port = None;
            head_port = None;
        }
        if let Some(p) = &tail_port {
            let at = (tv.pos.0 + p.at.0, tv.pos.1 + p.at.1);
            path[0].0 = at;
            if let Some(d) = p.dir.filter(|_| !ortho && !routed) {
                path[0].1 = (at.0 + d.0 * reach, at.1 + d.1 * reach);
                detour(&mut path, tv, d, reach);
            }
        }
        if let Some(p) = &head_port {
            let last = path.len() - 1;
            let at = (hv.pos.0 + p.at.0, hv.pos.1 + p.at.1);
            path[last].3 = at;
            if let Some(d) = p.dir.filter(|_| !ortho && !routed) {
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
        let [meet_head, meet_tail] = meet[ei];
        if let Some((m, u)) = meet_tail
            && tail_port.is_none()
        {
            path[0].0 = m;
            path[0].1 = (m.0 + u.0 * reach, m.1 + u.1 * reach);
        }
        if let Some((m, u)) = meet_head
            && head_port.is_none()
        {
            let last = path.len() - 1;
            path[last].3 = m;
            path[last].2 = (m.0 + u.0 * reach, m.1 + u.1 * reach);
        }
        if e.tail != e.head {
            if tail_clip
                && (meet_tail.is_none() || tail_port.is_some())
                && let Some(f) = clip_fn(tv, &tail_port)
            {
                curve::clip_start(&mut path, &*f);
            }
            if head_clip
                && (meet_head.is_none() || head_port.is_some())
                && let Some(f) = clip_fn(hv, &head_port)
            {
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
        // 両端が同じ地（文が決めたクラスタの塗り・グラフの bgcolor）の中なら、既定の色はその上で読める色
        let edge_ink = {
            let (ka, ca) = grounds.at(model.nodes[e.tail].cluster);
            let (kb, _) = grounds.at(model.nodes[e.head].cluster);
            if ka == kb {
                ca.and_then(Color::ink)
            } else {
                None
            }
        };
        let line_default = edge_ink.unwrap_or(LINE);
        let main_attr = |name: &str| paint(name, main, line_default);
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
            if !path.is_empty() && has(&st, "tapered") {
                // 太さ penwidth から細く: dir の forward は頭へ、back は尾へ、both は両端へ、none は同じ太さ
                let taper = |s: f64| match dir_attr.as_str() {
                    "back" => pen * s,
                    "both" => pen * (1.0 - (2.0 * s - 1.0).abs()),
                    "none" => pen,
                    _ => pen * (1.0 - s),
                };
                edges_out.push_str(&format!(
                    "<path d=\"{}\"{} stroke=\"none\"/>",
                    curve::tapered(&path, taper),
                    main_attr("fill")
                ));
            } else if !path.is_empty() {
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
                None => line_default.into(),
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
                    edge_ink,
                ));
                // decorate: ラベルに下線を引き、近い端から線の中ほどへつなぐ
                if a.flag("decorate").unwrap_or(false)
                    && let Some(m) = curve::midpoint(&path)
                {
                    let y = at.1 + t.height / 2.0;
                    let (l, r) = (at.0 - t.width / 2.0, at.0 + t.width / 2.0);
                    let (far, near) = if (m.0 - l).abs() <= (m.0 - r).abs() {
                        (r, l)
                    } else {
                        (l, r)
                    };
                    labels_out.push_str(&format!(
                        "<path d=\"M{},{}L{},{}L{},{}\" fill=\"none\"{}/>",
                        num(far),
                        num(y),
                        num(near),
                        num(y),
                        num(m.0),
                        num(m.1),
                        main_attr("stroke")
                    ));
                }
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
                    edge_ink,
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
                    edge_ink,
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
                    v.ground_ink,
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
        if let Some((p, scale, (px, py))) = &v.image {
            // 置き場所はノードの外接の箱の中
            let (aw, ah) = (v.geom.w, v.geom.h);
            let (w, h) = image::fit(p.size(1.0), (aw, ah), *scale);
            let ix = x - w / 2.0 + f64::from(*px) * (aw - w) / 2.0;
            let iy = y - h / 2.0 + f64::from(*py) * (ah - h) / 2.0;
            body.push_str(&image::element(p, ix, iy, w, h));
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
                        &format!(" class=\"dot-label\"{}", font_attrs(&v.font)),
                        v.ink,
                    ));
                }
            }
        }
        if let Some(t) = &v.label {
            body.push_str(&text_svg(
                t,
                x,
                y + v.label_dy,
                v.inner_w,
                &v.font,
                &format!(" class=\"dot-label\"{}", font_attrs(&v.font)),
                v.ink,
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
            grounds.graph.and_then(Color::ink),
        );
        bbox.add_rect(
            cx - t.width / 2.0,
            cy - t.height / 2.0,
            cx + t.width / 2.0,
            cy + t.height / 2.0,
        );
    }
    let (pad_x, pad_y) = pad(g.get("pad"));
    let dx = pad_x - bbox.x0;
    let dy = pad_y - bbox.y0;
    let width = (bbox.x1 - bbox.x0 + 2.0 * pad_x).ceil().max(1.0);
    let height = (bbox.y1 - bbox.y0 + 2.0 * pad_y).ceil().max(1.0);
    let mut content = String::new();
    // グラフの地（色の並びはグラデーション）
    let ground = g
        .get("bgcolor")
        .map(str::to_owned)
        .filter(|bg| ctx.first_color(bg, None).is_some())
        .map(|bg| {
            let radial = has(&styles(g.get("style")), "radial");
            let angle = g.num("gradientangle").unwrap_or(0.0);
            gradient(&mut ctx, &bg, None, radial, angle)
        });
    if !ctx.defs.is_empty() {
        content.push_str(&format!("<defs>{}</defs>", ctx.defs));
    }
    if let Some(fill) = ground {
        content.push_str(&format!(
            "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\"{fill}/>",
            num(width),
            num(height),
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
    if crate::take_cut_short() {
        ctx.notes.insert(Note::WorkLimit);
    }
    // rotate=90・orientation=landscape: 絵を左に 90° 回す（Graphviz の絵と同じ。上が左、右が上になる）
    let landscape = g.num("rotate").is_some_and(|r| r == 90.0)
        || g.get("orientation")
            .is_some_and(|o| o.trim().starts_with(['l', 'L']));
    if landscape {
        let content = format!(
            "<g transform=\"translate(0,{}) rotate(-90)\">{content}</g>",
            num(width)
        );
        return svg_document("dot", height, width, FONT_SIZE, &options, &acc, &content);
    }
    svg_document("dot", width, height, FONT_SIZE, &options, &acc, &content)
}

/// 絵の余白（点。`pad` はインチで、`x,y` か 1 つの値。`!` は読まない）。既定は Graphviz と同じ 4 点。
fn pad(s: Option<&str>) -> (f64, f64) {
    let read = |v: &str| {
        v.trim()
            .trim_end_matches('!')
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .map(|v| v.clamp(0.0, 10.0) * INCH)
    };
    let Some(s) = s else {
        return (PAD, PAD);
    };
    let mut it = s.trim().trim_end_matches('!').split(',');
    match (it.next().and_then(read), it.next().and_then(read)) {
        (Some(x), Some(y)) => (x, y),
        (Some(x), None) => (x, x),
        _ => (PAD, PAD),
    }
}

/// `imagepos` の置き場所（横と縦: -1 は左・上、0 は中、1 は右・下）。既定は中央。
fn image_pos(s: Option<&str>) -> (i8, i8) {
    let s = s.map(|s| s.trim().to_ascii_lowercase()).unwrap_or_default();
    let mut c = s.chars();
    let (v, h) = (c.next(), c.next());
    if s.len() != 2 {
        return (0, 0);
    }
    let y = match v {
        Some('t') => -1,
        Some('b') => 1,
        _ => 0,
    };
    let x = match h {
        Some('l') => -1,
        Some('r') => 1,
        _ => 0,
    };
    (x, y)
}

/// `pos` の値（`x,y` と、動かさない印の `!`）。
fn parse_pos(s: &str) -> Option<(f64, f64, bool)> {
    let s = s.trim();
    let (body, bang) = match s.strip_suffix('!') {
        Some(b) => (b, true),
        None => (s, false),
    };
    let mut it = body.split(',').map(|v| v.trim().parse::<f64>().ok());
    let x = it.next()??;
    let y = it.next()??;
    (x.is_finite() && y.is_finite() && x.abs() < 1e6 && y.abs() < 1e6).then_some((x, y, bang))
}

/// dot 以外の配置で置く。`pos` は neato などではインチ、`nop`（`neato -n`）では点。Graphviz の y は上向きなので
/// 逆にする。
fn place_free(
    engine: Engine,
    model: &Model,
    lnodes: &[layout::Node],
    lclusters: &[layout::Cluster],
    ctx: &mut Ctx,
) -> engines::Placement {
    let g = &model.attrs;
    let unit = if engine == Engine::Nop { 1.0 } else { INCH };
    let root_name = g.get("root").map(str::trim).filter(|s| !s.is_empty());
    let items: Vec<engines::Item> = model
        .nodes
        .iter()
        .zip(lnodes)
        .map(|(node, ln)| {
            let a = &node.attrs;
            let (pos, bang) = match a.get("pos").and_then(parse_pos) {
                Some((x, y, b)) => (Some((x * unit, -y * unit)), b),
                None => (None, false),
            };
            engines::Item {
                w: ln.w,
                h: ln.h,
                cluster: node.cluster,
                pos,
                pinned: bang || a.flag("pin").unwrap_or(false),
                area: a
                    .num("area")
                    .filter(|v| *v > 0.0)
                    .unwrap_or(1.0)
                    .min(10_000.0)
                    * PATCH
                    * PATCH,
                root: a.flag("root").unwrap_or(false) || root_name == Some(node.name.as_str()),
                sortv: a.num("sortv"),
            }
        })
        .collect();
    let links: Vec<engines::Link> = model
        .edges
        .iter()
        .map(|e| engines::Link {
            tail: e.tail,
            head: e.head,
            len: e
                .attrs
                .num("len")
                .filter(|v| *v > 0.0)
                .map(|v| v.min(100.0) * INCH),
            weight: e.attrs.num("weight").unwrap_or(1.0).clamp(0.0, 1000.0),
        })
        .collect();
    let groups: Vec<engines::Group> = model
        .clusters
        .iter()
        .zip(lclusters)
        .map(|(c, lc)| engines::Group {
            parent: c.parent,
            margin: lc.margin,
            label: lc.label,
            sortv: c.attrs.num("sortv"),
        })
        .collect();
    engines::place(engine, &items, &links, &groups, g, ctx.notes)
}

/// dot 以外の配置の結果を、描く側の形に（辺は中心どうしの直線、同じ組の多重の辺は左右にふくらませる）。
fn free_layout(model: &Model, placed: &engines::Placement) -> layout::Layout {
    let p = &placed.nodes;
    let mut total: HashMap<(usize, usize), usize> = HashMap::new();
    for e in &model.edges {
        if e.tail != e.head {
            *total
                .entry((e.tail.min(e.head), e.tail.max(e.head)))
                .or_default() += 1;
        }
    }
    let mut seen: HashMap<(usize, usize), usize> = HashMap::new();
    let mut loops = vec![0usize; model.nodes.len()];
    let routes = model
        .edges
        .iter()
        .map(|e| {
            if e.tail == e.head {
                let index = loops[e.tail];
                loops[e.tail] += 1;
                return Route::Loop { index };
            }
            let key = (e.tail.min(e.head), e.tail.max(e.head));
            let m = total.get(&key).copied().unwrap_or(1);
            let k = seen.entry(key).or_default();
            let off = (*k as f64 - (m as f64 - 1.0) / 2.0) * 18.0;
            *k += 1;
            let (a, b) = (p[e.tail], p[e.head]);
            if off == 0.0 {
                return Route::Path {
                    points: vec![a, b],
                    label: None,
                };
            }
            // 組の向き（小さい番号から）の左
            let (lo, hi) = (p[key.0], p[key.1]);
            let (dx, dy) = (hi.0 - lo.0, hi.1 - lo.1);
            let l = dx.hypot(dy).max(1e-9);
            let mid = (
                (a.0 + b.0) / 2.0 - dy / l * off,
                (a.1 + b.1) / 2.0 + dx / l * off,
            );
            Route::Path {
                points: vec![a, mid, b],
                label: None,
            }
        })
        .collect();
    let (w, h) = p
        .iter()
        .fold((0.0f64, 0.0f64), |(w, h), q| (w.max(q.0), h.max(q.1)));
    layout::Layout {
        nodes: p.clone(),
        ranks: vec![0; p.len()],
        routes,
        clusters: placed.clusters.clone(),
        width: w,
        height: h,
        crossings: 0,
        corridors: vec![Vec::new(); model.edges.len()],
        straightened: false,
    }
}

/// 直角の道の端のポート（配置の座標、ノードの中心から）。
fn ortho_port(view: &NodeView, port: Option<&str>, dir: Dir) -> route::OrthoPort {
    let to_layout = |v: P| -> P {
        match dir {
            Dir::TB => v,
            Dir::BT => (v.0, -v.1),
            Dir::LR => (v.1, v.0),
            Dir::RL => (v.1, -v.0),
        }
    };
    match port_of(view, port) {
        Some(Port {
            at, dir: Some(d), ..
        }) => route::OrthoPort::Point {
            at: to_layout(at),
            dir: to_layout(d),
        },
        Some(Port { clip: Some(b), .. }) => {
            let (p, q) = (to_layout((b[0], b[1])), to_layout((b[2], b[3])));
            route::OrthoPort::Cell {
                b: [p.0.min(q.0), p.1.min(q.1), p.0.max(q.0), p.1.max(q.1)],
            }
        }
        _ => route::OrthoPort::Free,
    }
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

/// 升（record の欄・HTML の表の升）のポートから `toward`（絵の座標）へまっすぐ出ると、同じノードのほかの升の
/// 上を通るなら、外形に触れる升の辺のうち `toward` に向くものの中ほどから、その向きに出すポートに替える。
fn outer_side(view: &NodeView, port: &mut Option<Port>, toward: P) {
    let Some(Port {
        clip: Some(b),
        dir: None,
        ..
    }) = port
    else {
        return;
    };
    let b = *b;
    let (w, h) = view.geom.outer();
    let o = [-w / 2.0, -h / 2.0, w / 2.0, h / 2.0];
    let c = ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
    let t = (toward.0 - view.pos.0 - c.0, toward.1 - view.pos.1 - c.1);
    if t.0.hypot(t.1) < 1e-9 {
        return;
    }
    // 升の中心から t の向きに出る升の縁の点
    let (hw, hh) = ((b[2] - b[0]) / 2.0, (b[3] - b[1]) / 2.0);
    let s = (hw / t.0.abs().max(1e-12)).min(hh / t.1.abs().max(1e-12));
    let exit = (c.0 + t.0 * s, c.1 + t.1 * s);
    let near = |a: f64, b: f64| (a - b).abs() < 1.0;
    if near(exit.0, o[0]) || near(exit.0, o[2]) || near(exit.1, o[1]) || near(exit.1, o[3]) {
        return;
    }
    let u = unit(t);
    let sides = [
        (near(b[0], o[0]), (-1.0, 0.0), (b[0], c.1)),
        (near(b[2], o[2]), (1.0, 0.0), (b[2], c.1)),
        (near(b[1], o[1]), (0.0, -1.0), (c.0, b[1])),
        (near(b[3], o[3]), (0.0, 1.0), (c.0, b[3])),
    ];
    let best = sides.iter().filter(|s| s.0).max_by(|x, y| {
        let dx = x.1.0 * u.0 + x.1.1 * u.1;
        let dy = y.1.0 * u.0 + y.1.1 * u.1;
        dx.total_cmp(&dy)
    });
    if let Some(&(_, d, at)) = best {
        *port = Some(Port {
            at,
            clip: None,
            dir: Some(d),
        });
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
    if let Some(css) = f.family_css() {
        s.push_str(&format!(" font-family=\"{}\"", esc(css)));
    }
    if let Some(c) = &f.color {
        s.push_str(&color_attr("fill", c));
    }
    s
}

/// 色の並びの地の色（グラデーションは読める色の平均。透明・読めない色は除く）。
fn ground_color(text: &str, scheme: Option<&str>) -> Option<Color> {
    let list: Vec<Rgba> = color::parse_list(text, scheme)
        .into_iter()
        .filter_map(|s| match s.color {
            Some(Color::Rgba(c)) => Some(c),
            _ => None,
        })
        .collect();
    if list.is_empty() {
        return None;
    }
    let n = list.len() as u32;
    let avg = |f: fn(&Rgba) -> u8| (list.iter().map(|c| u32::from(f(c))).sum::<u32>() / n) as u8;
    Some(Color::Rgba(Rgba {
        r: avg(|c| c.r),
        g: avg(|c| c.g),
        b: avg(|c| c.b),
        a: 255,
    }))
}

/// 文が決めた地（クラスタの塗り・グラフの bgcolor）。`clusters[c]` はそのクラスタの中の地（自分になければ
/// 外のクラスタ、なければグラフの地）と、その地を決めたもの（クラスタの番号、グラフは None）。
struct Grounds {
    graph: Option<Color>,
    clusters: Vec<(Option<usize>, Option<Color>)>,
}

impl Grounds {
    fn new(model: &Model, scheme: Option<&str>) -> Grounds {
        let g = &model.attrs;
        let graph = g.get("bgcolor").and_then(|t| ground_color(t, scheme));
        let own: Vec<Option<Color>> = model
            .clusters
            .iter()
            .map(|c| {
                let a = &c.attrs;
                let sc = a.get("colorscheme").or(scheme);
                let st = styles(a.get("style"));
                if has(&st, "invis") {
                    return None;
                }
                let filled = has(&st, "filled") || has(&st, "radial") || has(&st, "striped");
                let text = if filled {
                    a.get("fillcolor").or(a.get("color")).or(a.get("bgcolor"))
                } else {
                    a.get("bgcolor")
                };
                text.and_then(|t| ground_color(t, sc))
            })
            .collect();
        let clusters = (0..model.clusters.len())
            .map(|c| {
                let mut at = Some(c);
                let mut steps = 0;
                while let Some(ci) = at {
                    if let Some(col) = own[ci] {
                        return (Some(ci), Some(col));
                    }
                    at = model.clusters[ci].parent;
                    steps += 1;
                    if steps > 10_000 {
                        break;
                    }
                }
                (None, graph)
            })
            .collect();
        Grounds { graph, clusters }
    }

    /// クラスタ `c`（None は根）の中の地。
    fn at(&self, c: Option<usize>) -> (Option<usize>, Option<Color>) {
        match c {
            Some(c) => self.clusters.get(c).copied().unwrap_or((None, self.graph)),
            None => (None, self.graph),
        }
    }
}

/// 色の並びのグラデーション（`defs` に足し、塗りの属性を返す）。
fn gradient(ctx: &mut Ctx, text: &str, scheme: Option<&str>, radial: bool, angle: f64) -> String {
    let stops = color::parse_list(text, scheme.or(ctx.scheme.as_deref()));
    if stops.iter().any(|s| s.color.is_none()) {
        let _ = ctx.first_color(text, scheme);
    }
    match color::Fill::parse(text, scheme.or(ctx.scheme.as_deref()), radial, angle) {
        None => format!(" fill=\"{SURFACE_ALT}\""),
        Some(color::Fill::Solid(c)) => color_attr("fill", &c),
        Some(color::Fill::Gradient(g)) => {
            let id = ctx.id();
            ctx.defs.push_str(&g.element(&id));
            format!(" fill=\"url(#{id})\"")
        }
    }
}

/// 箱 (x, y, w, h) を色の並び `text` の縦の帯で塗る（クラスタの striped）。読める色が 2 つ未満なら空。
#[allow(clippy::too_many_arguments)]
fn bands(
    ctx: &mut Ctx,
    text: &str,
    scheme: Option<&str>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> String {
    let stops = color::parse_list(text, scheme.or(ctx.scheme.as_deref()));
    if stops.iter().any(|s| s.color.is_none()) {
        let _ = ctx.first_color(text, scheme);
    }
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
    let mut out = String::new();
    let mut at = 0.0;
    for (c, f) in known {
        let frac = f.unwrap_or(rest);
        if frac <= 0.0 {
            continue;
        }
        out.push_str(&format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{} stroke=\"none\"/>",
            num(x + at * w),
            num(y),
            num(frac.min(1.0 - at).max(0.0) * w),
            num(h),
            color_attr("fill", &c)
        ));
        at += frac;
        if at >= 1.0 {
            break;
        }
    }
    out
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

/// `ratio` と `size`（インチ）から、仕上がりの向きの横と縦の位置の伸ばし（どちらも 1 以上）。`w`・`h` は図の
/// 大きさ、`span` はノードの中心の広がりで、伸びるのはこの分だけ。数なら縦横の比（高さ / 幅）に足りない側を伸ばす。
/// `fill` は `size` の比に合わせ、両方とも `size` より小さければ両方を `size` まで伸ばす。`expand` は両方とも
/// 小さければ同じ倍率で `size` に届くまで。`compress`・`auto` は読まない。
fn ratio_scale(
    ratio: Option<&str>,
    size: Option<&str>,
    w: f64,
    h: f64,
    span: (f64, f64),
) -> (f64, f64) {
    let none = (1.0, 1.0);
    let Some(ratio) = ratio.map(|r| r.trim().to_ascii_lowercase()) else {
        return none;
    };
    if w <= 0.0 || h <= 0.0 {
        return none;
    }
    let size = size.and_then(|s| {
        let s = s.trim().trim_end_matches('!');
        let mut it = s.split(',').map(|v| v.trim().parse::<f64>().ok());
        let a = it.next().flatten()?;
        let b = it.next().flatten().unwrap_or(a);
        (a.is_finite() && b.is_finite() && a > 0.0 && b > 0.0).then_some((a * INCH, b * INCH))
    });
    // 横（縦）を `to` の大きさにする位置の倍率
    let grow_x = |to: f64| {
        if span.0 > 0.0 {
            1.0 + (to - w) / span.0
        } else {
            1.0
        }
    };
    let grow_y = |to: f64| {
        if span.1 > 0.0 {
            1.0 + (to - h) / span.1
        } else {
            1.0
        }
    };
    let to_aspect = |r: f64| -> (f64, f64) {
        if h / w < r {
            (1.0, grow_y(r * w))
        } else {
            (grow_x(h / r), 1.0)
        }
    };
    let (fx, fy) = match ratio.as_str() {
        "fill" => match size {
            Some((sw, sh)) if sw > w && sh > h => (grow_x(sw), grow_y(sh)),
            Some((sw, sh)) => to_aspect(sh / sw),
            None => none,
        },
        "expand" => match size {
            Some((sw, sh)) if sw > w && sh > h => {
                let k = grow_x(sw).min(grow_y(sh));
                (k, k)
            }
            _ => none,
        },
        _ => match ratio.parse::<f64>() {
            Ok(r) if r.is_finite() && r > 0.0 => to_aspect(r),
            _ => none,
        },
    };
    // 図が大きくなりすぎないように
    (fx.clamp(1.0, MAX_STRETCH), fy.clamp(1.0, MAX_STRETCH))
}

/// 線を大きく曲げた形を試す図の、辺の数の上限（線ごとにほかの線との交わりを調べるので）。
/// 同じノードに着く線を同じ向きとみなす角度の差（ラジアン）と、そのときに目指す点を分ける間。
const ARRIVAL_ANGLE: f64 = 0.35;
const ARRIVAL_SPREAD: f64 = 8.0;

const ROOMY_EDGES: usize = 120;

/// `ratio` で伸ばす倍率の上限。
const MAX_STRETCH: f64 = 20.0;
