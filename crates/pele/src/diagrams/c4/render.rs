//! C4 を描く（pele の `src/diagrams/c4/render.ts`）。

use std::collections::{HashMap, HashSet};

use super::db::C4Db;
use super::layout::{HEAD_LEFT, HEAD_TOP, layout_c4};
use super::route::{Box, HEAD, LABEL_PAD, LANE, Lanes, crosses, cyl_ry, queue_rx, route};
use crate::diagrams::common::fit_width::{tighten, title_room};
use crate::diagrams::common::literal_label::left_label_svg;
use crate::errors::{ErrorCode, PeleError, Result};
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num};
use crate::svg::edges::EdgePath;
use crate::svg::edges::{ARROW, Fill, casings_svg, edge_casing, marker, marker_trim, over_fills};
use crate::svg::ink::{ink_fill, paints};
use crate::svg::root::{Accessible, svg_document};
use crate::svg::shapes::draw_shape;
use crate::svg::theme::{ResolvedStyle, resolve_style, rounded};
use crate::text::label::layout_label;
use crate::text::measurer::{TextMeasurer, default_measurer, style as st};
use crate::types::{LinkInfo, RenderOptions, Rendered};
use crate::util::url::{linked, sanitize_url};

const PAIR_SHIFT: f64 = 7.0;
const NARROW_GAP: f64 = 36.0;
const WIDEN: f64 = 24.0;
const MAX_OFFSET: f64 = 2000.0;
/// ラベルが形の縁の、文字のない所を覆ってよい幅。
const EDGE: f64 = 8.0;
/// 関係のラベルを試す線の上の場所。真ん中に近い順。
const SLIDE: [f64; 7] = [0.5, 0.4, 0.6, 0.3, 0.7, 0.2, 0.8];
/// 箱の比べ合いがこれを超えると、ラベルは真ん中に留まり、線は形をよけない。
const CARE_BUDGET: f64 = 2000000.0;

fn user_style(fill: Option<&str>, stroke: Option<&str>, color: Option<&str>) -> ResolvedStyle {
    let mut declarations: Vec<String> = Vec::new();
    if let Some(f) = fill.filter(|f| !f.is_empty()) {
        declarations.push(format!("fill:{f}"));
    }
    if let Some(s) = stroke.filter(|s| !s.is_empty()) {
        declarations.push(format!("stroke:{s}"));
    }
    if let Some(c) = color.filter(|c| !c.is_empty()) {
        declarations.push(format!("color:{c}"));
    }
    resolve_style(&declarations)
}

/// `url.replace(/ﬂ°°?/g, '#').replace(/¶ß/g, ';')`
fn unhide(url: &str) -> String {
    let mut out = String::new();
    let mut chars = url.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{fb02}' && chars.peek() == Some(&'\u{b0}') {
            chars.next();
            if chars.peek() == Some(&'\u{b0}') {
                chars.next();
            }
            out.push('#');
        } else {
            out.push(c);
        }
    }
    out.replace("\u{b6}\u{df}", ";")
}

fn link(
    body: &str,
    url: Option<&str>,
    id: &str,
    links: &mut Vec<LinkInfo>,
    options: &RenderOptions,
) -> String {
    let Some(url) = url.filter(|u| !u.is_empty()) else {
        return body.to_string();
    };
    // 符号の隠しが、住所の中の `#name;` を隠した。ブラウザが見るものを確かめられるよう戻す。
    let url = if url.contains("\u{b6}\u{df}") {
        unhide(url)
    } else {
        url.to_string()
    };
    linked(body, &sanitize_url(&url), id, links, options, "")
}

pub fn render_c4(db: &C4Db, config: &Config, options: &RenderOptions) -> Result<Rendered> {
    tighten(options, |tight| {
        if !tight {
            return Ok(draw(db, config, options, false, NARROW_GAP)?.0);
        }
        let (mut best, mut squeezed) = draw(db, config, options, true, NARROW_GAP)?;
        let mut gap = NARROW_GAP + WIDEN;
        while squeezed > 0 && gap <= NARROW_GAP + 3.0 * WIDEN {
            let (wider, left) = draw(db, config, options, true, gap)?;
            if left < squeezed {
                best = wider;
                squeezed = left;
            }
            gap += WIDEN;
        }
        Ok(best)
    })
}

fn draw(
    db: &C4Db,
    _config: &Config,
    options: &RenderOptions,
    tight: bool,
    gap: f64,
) -> Result<(Rendered, usize)> {
    let size = options.font_size.unwrap_or(16.0);
    let small = js::round(size * 0.8125);
    let measurer: &dyn TextMeasurer = match &options.measurer {
        Some(m) => m.as_ref(),
        None => default_measurer(),
    };
    let pad = options.padding.unwrap_or(8.0);
    let mut links: Vec<LinkInfo> = Vec::new();
    let mut layout = layout_c4(
        db,
        measurer,
        size,
        small,
        if tight {
            Some(js::max(
                80.0,
                options.max_width.unwrap_or(320.0) - 2.0 * pad,
            ))
        } else {
            None
        },
        gap,
    );
    let root = layout.order[0];

    let mut min_x: f64 = 0.0;
    let mut min_y: f64 = 0.0;
    let mut max_x = layout.groups[root].b.w;
    let mut max_y = layout.groups[root].b.h;
    let mut grow = |x0: f64, y0: f64, x1: f64, y1: f64| {
        min_x = js::min(min_x, x0);
        min_y = js::min(min_y, y0);
        max_x = js::max(max_x, x1);
        max_y = js::max(max_y, y1);
    };

    let mut ends: Vec<Box> = Vec::new();
    let mut pairs: HashMap<usize, HashSet<usize>> = HashMap::new();
    for rel in &db.rels {
        let from = if rel.from_attr.is_some() {
            None
        } else {
            db.get_c4_shape(&rel.from)
        };
        let to = if rel.to_attr.is_some() {
            None
        } else {
            db.get_c4_shape(&rel.to)
        };
        let a = from.and_then(|f| layout.boxes.get(&f));
        let b = to.and_then(|t| layout.boxes.get(&t));
        let (Some(a), Some(b)) = (a, b) else {
            return Err(PeleError::typed(
                format!(
                    "C4 rel \"{}\" -> \"{}\" references an unknown shape or boundary",
                    rel.from, rel.to
                ),
                ErrorCode::Semantic,
                "c4",
            ));
        };
        pairs.entry(a.id).or_default().insert(b.id);
        ends.push(a.clone());
        ends.push(b.clone());
    }

    // 関係のラベルが覆わないもの: 形、矢じり、境界の見出し、先に置いたラベル。
    let mut nodes: Vec<Box> = Vec::new();
    let mut taken: Vec<f64> = Vec::new();
    for &g in &layout.order {
        for node in &layout.groups[g].nodes {
            let b = &node.b;
            nodes.push(b.clone());
            taken.extend([b.x + EDGE, b.y + EDGE, b.x + b.w - EDGE, b.y + b.h - EDGE]);
        }
    }
    let n_rels = db.rels.len() as f64;
    let careful =
        n_rels * (nodes.len() as f64 + layout.order.len() as f64 + 3.0 * n_rels) <= CARE_BUDGET;
    let covered = |taken: &[f64], x0: f64, y0: f64, x1: f64, y1: f64| -> f64 {
        let mut area = 0.0;
        let mut i = 0;
        while i < taken.len() {
            let w = js::min(x1, taken[i + 2]) - js::max(x0, taken[i]);
            if w > 0.0 {
                let h = js::min(y1, taken[i + 3]) - js::max(y0, taken[i + 1]);
                if h > 0.0 {
                    area += w * h;
                }
            }
            i += 4;
        }
        area
    };
    let offset = |value: Option<f64>| -> f64 {
        match value {
            Some(v) if js::truthy(v) => js::max(-MAX_OFFSET, js::min(MAX_OFFSET, v)),
            _ => 0.0,
        }
    };

    // ラベルを置く前にすべての線を引く。ラベルが矢じりを避けられるように。
    let mut casings = String::new();
    let mut edges_out = String::new();
    let trim = marker_trim("arrow_point");
    let mut fills: Vec<Fill> = Vec::new();
    for &g in &layout.order {
        if g == root {
            continue;
        }
        let group = &layout.groups[g];
        let boundary = &db.boundaries[group.boundary];
        let shape = user_style(boundary.bg_color.as_deref(), None, None).shape;
        if paints(&shape) {
            let b = &group.b;
            fills.push(Fill {
                box_: [b.x, b.y, b.x + b.w, b.y + b.h],
                shape,
            });
        }
    }
    let mut lanes = Lanes::new();
    for &g in &layout.order {
        if g != root {
            lanes.frame(&layout.groups[g].b);
        }
    }
    let mut routes = Vec::new();
    for (index, rel) in db.rels.iter().enumerate() {
        let a = &ends[index * 2];
        let b = &ends[index * 2 + 1];
        let style = user_style(None, rel.line_color.as_deref(), None);
        let start_arrow = rel.type_ == "birel" || rel.type_ == "rel_b";
        let end_arrow = rel.type_ != "rel_b";
        let reverse = a.id != b.id && pairs.get(&b.id).is_some_and(|s| s.contains(&a.id));
        let r = route(
            a,
            b,
            if reverse { PAIR_SHIFT } else { 0.0 },
            if careful { Some(&nodes) } else { None },
            &mut lanes,
            if start_arrow { trim } else { 0.0 },
            if end_arrow { trim } else { 0.0 },
        );
        grow(
            js::min(r.x0, r.x1),
            js::min(r.y0, r.y1),
            js::max(r.x0, r.x1),
            js::max(r.y0, r.y1),
        );
        if start_arrow && careful {
            taken.extend([r.sx - ARROW, r.sy - ARROW, r.sx + ARROW, r.sy + ARROW]);
        }
        if end_arrow && careful {
            taken.extend([r.ex - ARROW, r.ey - ARROW, r.ex + ARROW, r.ey + ARROW]);
        }
        let over = over_fills(&r.d, &[r.sx, r.sy, r.ex, r.ey], &fills, &style.line, None);
        let color = match over.as_deref() {
            Some(o) if !o.is_empty() => o.to_string(),
            _ => match &style.stroke {
                Some(s) if !s.is_empty() => esc(s),
                _ => "var(--_l)".to_string(),
            },
        };
        if over.as_deref() == Some("") {
            let path = EdgePath {
                d: r.d.clone(),
                tokens: None,
                sx: r.sx,
                sy: r.sy,
                sdx: r.sdx,
                sdy: r.sdy,
                ex: r.ex,
                ey: r.ey,
                edx: r.edx,
                edy: r.edy,
            };
            casings.push_str(&edge_casing(
                &path,
                if start_arrow { "arrow_point" } else { "none" },
                if end_arrow { "arrow_point" } else { "none" },
                &style.line,
                false,
            ));
        }
        edges_out.push_str(&format!(
            "<g class=\"pele-edge\" data-id=\"{}\"{}{}><path d=\"{}\"/>{}{}</g>",
            esc_text(&format!("{}-{}", rel.from, rel.to)),
            match over.as_deref() {
                Some(o) if !o.is_empty() => format!(" stroke=\"{o}\""),
                _ => String::new(),
            },
            style.line,
            r.d,
            if start_arrow {
                marker("arrow_point", r.sx, r.sy, r.sdx, r.sdy, &color, "none")
            } else {
                String::new()
            },
            if end_arrow {
                marker("arrow_point", r.ex, r.ey, r.edx, r.edy, &color, "none")
            } else {
                String::new()
            }
        ));
        routes.push(r);
    }

    // 境界の見出しは上の辺の左、真ん中、右のうち、線の横切らない最初の所に置く。
    for k in 0..layout.order.len() {
        let g = layout.order[k];
        let group = &mut layout.groups[g];
        if !careful || group.head_w == 0.0 {
            continue;
        }
        let top = group.b.y + HEAD_TOP;
        let bottom = group.b.y + group.head - LANE;
        let mut fewest = f64::INFINITY;
        for x in [
            HEAD_LEFT,
            (group.b.w - group.head_w) / 2.0,
            group.b.w - HEAD_LEFT - group.head_w,
        ] {
            let mut count = 0.0;
            for r in &routes {
                if crosses(
                    &r.points,
                    group.b.x + x - 4.0,
                    top,
                    group.b.x + x + group.head_w + 4.0,
                    bottom,
                ) {
                    count += 1.0;
                }
            }
            if count < fewest {
                fewest = count;
                group.head_x = x;
            }
            if count == 0.0 {
                break;
            }
        }
        taken.extend([
            group.b.x + group.head_x - 4.0,
            top,
            group.b.x + group.head_x + group.head_w + 4.0,
            bottom,
        ]);
    }

    let mut labels_out = String::new();
    for (index, rel) in db.rels.iter().enumerate() {
        let r = &routes[index];
        let id = format!("{}-{}", rel.from, rel.to);
        let style = user_style(None, None, rel.text_color.as_deref());
        let text = if db.c4_type.as_deref() == Some("C4Dynamic") {
            format!("{}: {}", index + 1, rel.label)
        } else {
            rel.label.clone()
        };
        let label = layout_label(Some(&text), false, measurer, small, r.room, 0);
        let techn_text = rel
            .techn
            .as_deref()
            .filter(|t| !t.is_empty())
            .map(|t| format!("[{t}]"));
        let techn = layout_label(
            techn_text.as_deref(),
            false,
            measurer,
            small,
            r.room,
            st::ITALIC,
        );
        let lw = js::max(label.width, techn.width) + LABEL_PAD;
        let lh = label.height + techn.height;
        if lh == 0.0 {
            continue;
        }

        // ラベルは線の上に、逆向きの関係があるときはそのすぐ横に置く。
        let beside = r.nx.abs() * (lw / 2.0) + r.ny.abs() * (lh / 2.0) + 2.0;
        let aside = if js::truthy(r.lean) {
            js::max(0.0, js::min(r.lean, beside - 14.0))
        } else {
            beside
        };
        // 手で決めたずれは元の間隔に合わせてある。詰めたラベルは引いた矢印の横に置き直し、ずれでラベルが境界の外に
        // 浮かないように。
        let mut dx = r.nx * aside + if tight { 0.0 } else { offset(rel.offset_x) };
        let mut dy = r.ny * aside + if tight { 0.0 } else { offset(rel.offset_y) };
        let mut at = 0.5;
        if careful {
            // 線に沿って覆う所の最も少ない所へ滑らせる。図のずれは、線そのものの上の所がそれが導くどこよりも少なく
            // 覆うのでなければ保つ: Mermaid の間隔に合わせてあることが多い。
            let mut least = f64::INFINITY;
            let moved = dx != r.nx * aside || dy != r.ny * aside;
            let passes = if moved { 2 } else { 1 };
            let mut pass = 0;
            while pass < passes && least > 0.0 {
                let px = if pass > 0 { r.nx * aside } else { dx };
                let py = if pass > 0 { r.ny * aside } else { dy };
                for t in SLIDE {
                    let cx = r.x0 + (r.x1 - r.x0) * t + px;
                    let cy = r.y0 + (r.y1 - r.y0) * t + py;
                    let area = covered(
                        &taken,
                        cx - lw / 2.0,
                        cy - lh / 2.0,
                        cx + lw / 2.0,
                        cy + lh / 2.0,
                    );
                    if area < least {
                        least = area;
                        at = t;
                        dx = px;
                        dy = py;
                    }
                    if area == 0.0 {
                        break;
                    }
                }
                pass += 1;
            }
        }
        let cx = r.x0 + (r.x1 - r.x0) * at + dx;
        let top = r.y0 + (r.y1 - r.y0) * at + dy - lh / 2.0;
        if careful {
            taken.extend([cx - lw / 2.0, top, cx + lw / 2.0, top + lh]);
        }
        grow(cx - lw / 2.0, top, cx + lw / 2.0, top + lh);
        let body = label_svg(&label, cx, top + label.height / 2.0, &style.text, None)
            + &label_svg(
                &techn,
                cx,
                top + label.height + techn.height / 2.0,
                &format!(" class=\"pele-c4-techn\" fill=\"var(--_m)\"{}", style.text),
                None,
            );
        labels_out.push_str(&format!(
            "<g class=\"pele-edge-label\" data-id=\"{}\">{}<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"3\" fill=\"var(--_bg)\"/>{}</g>",
            esc_text(&id),
            match rel.descr.as_deref() {
                Some(d) if !d.is_empty() => format!("<title>{}</title>", esc_text(d)),
                _ => String::new(),
            },
            num(cx - lw / 2.0),
            num(top),
            num(lw),
            num(lh),
            link(&body, rel.link.as_deref(), &id, &mut links, options)
        ));
    }

    let mut clusters = String::new();
    let mut nodes_out = String::new();
    for &g in &layout.order {
        let group = &layout.groups[g];
        if g != root {
            let boundary = &db.boundaries[group.boundary];
            let style = user_style(
                boundary.bg_color.as_deref(),
                boundary.border_color.as_deref(),
                boundary.font_color.as_deref(),
            );
            let x = group.b.x + group.head_x;
            let mut y = group.b.y + HEAD_TOP;
            let color = ink_fill(&style.shape, &style.text, None) + &style.text;
            let muted = ink_fill(&style.shape, &style.text, Some("var(--_m)")) + &style.text;
            let mut text = left_label_svg(
                &group.labels[0],
                x,
                y,
                &format!(" class=\"pele-cluster-label\" font-weight=\"var(--_hw)\"{color}"),
            );
            y += group.labels[0].height;
            text.push_str(&left_label_svg(
                &group.labels[1],
                x,
                y,
                &format!(
                    " class=\"pele-c4-type\" font-size=\"{}\"{muted}",
                    js::fmt(small)
                ),
            ));
            y += group.labels[1].height;
            text.push_str(&left_label_svg(
                &group.labels[2],
                x,
                y,
                &format!(
                    " class=\"pele-c4-descr\" font-size=\"{}\"{muted}",
                    js::fmt(small)
                ),
            ));
            clusters.push_str(&format!(
                "<g class=\"pele-cluster pele-c4-{}\" data-id=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"var(--_b)\"{}{}/>{}</g>",
                if boundary.node_type.as_deref().is_some_and(|t| !t.is_empty()) {
                    "node"
                } else {
                    "boundary"
                },
                esc_text(&boundary.alias),
                num(group.b.x),
                num(group.b.y),
                num(group.b.w),
                num(group.b.h),
                if boundary.node_type.as_deref().is_some_and(|t| !t.is_empty()) {
                    ""
                } else {
                    " stroke-dasharray=\"6 4\""
                },
                rounded(&style.shape),
                link(&text, boundary.link.as_deref(), &boundary.alias, &mut links, options)
            ));
        }
        for node in &group.nodes {
            let shape = &db.shapes[node.shape];
            let external = shape.kind.starts_with("external");
            let style = user_style(
                shape.bg_color.as_deref(),
                shape.border_color.as_deref(),
                shape.font_color.as_deref(),
            );
            let stroke = format!(
                " stroke=\"var(--_b)\"{}",
                if external {
                    " stroke-dasharray=\"4 3\""
                } else {
                    ""
                }
            );
            let (w, h, form, labels) = (node.b.w, node.b.h, node.b.form, &node.labels);
            let dx = if form == "h-cyl" {
                -queue_rx(h) / 2.0
            } else {
                0.0
            };
            let dy = if form == "person" {
                HEAD / 2.0
            } else if form == "cyl" {
                cyl_ry(w) / 2.0
            } else {
                0.0
            };
            let color = ink_fill(&style.shape, &style.text, None) + &style.text;
            let muted = ink_fill(&style.shape, &style.text, Some("var(--_m)")) + &style.text;
            let mut y = dy - node.text_h / 2.0;
            let mut text = label_svg(
                &labels[0],
                dx,
                y + labels[0].height / 2.0,
                &format!(
                    " class=\"pele-c4-type\" font-size=\"{}\"{muted}",
                    js::fmt(small - 1.0)
                ),
                None,
            );
            y += labels[0].height;
            text.push_str(&label_svg(
                &labels[1],
                dx,
                y + labels[1].height / 2.0,
                &format!(" class=\"pele-label\" font-weight=\"var(--_hw)\"{color}"),
                None,
            ));
            y += labels[1].height;
            text.push_str(&label_svg(
                &labels[2],
                dx,
                y + labels[2].height / 2.0,
                &format!(
                    " class=\"pele-c4-techn\" font-size=\"{}\"{muted}",
                    js::fmt(small)
                ),
                None,
            ));
            y += labels[2].height + 4.0;
            text.push_str(&label_svg(
                &labels[3],
                dx,
                y + labels[3].height / 2.0,
                &format!(
                    " class=\"pele-c4-descr\" font-size=\"{}\"{color}",
                    js::fmt(small)
                ),
                None,
            ));
            let body = draw_shape(
                form,
                w,
                h,
                &format!(
                    " fill=\"var({})\"{stroke}{}",
                    if external { "--_bg" } else { "--_s" },
                    style.shape
                ),
                &(stroke.clone() + &style.line),
            ) + &text;
            nodes_out.push_str(&format!(
                "<g class=\"pele-node pele-shape-{form} pele-c4-{}{}\" data-id=\"{}\" transform=\"translate({},{})\">{}</g>",
                shape.kind,
                if external { " pele-c4-external" } else { "" },
                esc_text(&shape.alias),
                num(node.b.x + w / 2.0),
                num(node.b.y + h / 2.0),
                link(&body, shape.link.as_deref(), &shape.alias, &mut links, options)
            ));
        }
    }

    let title = layout_label(
        db.title.as_deref(),
        false,
        measurer,
        size,
        if tight { title_room(options) } else { 4000.0 },
        st::BOLD,
    );
    let title_height = if title.height > 0.0 {
        title.height + 12.0
    } else {
        0.0
    };
    let inner = js::max(max_x - min_x, title.width);
    let width = (inner + 2.0 * pad).ceil();
    let height = (max_y - min_y + title_height + 2.0 * pad).ceil();
    let ox = pad - min_x + (inner - (max_x - min_x)) / 2.0;
    let oy = pad + title_height - min_y;

    let mut content = label_svg(
        &title,
        width / 2.0,
        pad + title.height / 2.0,
        " class=\"pele-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    content.push_str(&format!(
        "<g transform=\"translate({},{})\">",
        num(ox),
        num(oy)
    ));
    if !clusters.is_empty() {
        content.push_str(&format!("<g class=\"pele-clusters\">{clusters}</g>"));
    }
    content.push_str(&casings_svg(&casings));
    if !edges_out.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-edges\" fill=\"none\" stroke=\"var(--_l)\" stroke-linecap=\"round\">{edges_out}</g>"
        ));
    }
    if !nodes_out.is_empty() {
        content.push_str(&format!("<g class=\"pele-nodes\">{nodes_out}</g>"));
    }
    if !labels_out.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-edge-labels\" font-size=\"{}\">{labels_out}</g>",
            js::fmt(small)
        ));
    }
    content.push_str("</g>");
    let acc = Accessible {
        acc_title: db.acc_title.as_deref(),
        acc_descr: db.acc_descr.as_deref(),
    };
    let svg = svg_document("c4", width, height, size, options, &acc, &content);
    Ok((
        Rendered {
            svg,
            width,
            height,
            links,
        },
        lanes.squeezed,
    ))
}
