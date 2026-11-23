//! mindmap を描く（pele の `src/diagrams/mindmap/render.ts`）。

use super::db::{MindmapDb, MindmapNode};
use crate::diagrams::common::fit_width::{title_room, turn_to_fit};
use crate::diagrams::common::icon::icon_svg;
use crate::errors::Result;
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc_text, label_svg, num};
use crate::svg::root::{Accessible, svg_document};
use crate::svg::shapes::{draw_shape, shape_inset, shape_size};
use crate::svg::theme::{class_names, series_color};
use crate::svg::title::title_label;
use crate::text::icons::icon_markup;
use crate::text::label::{Label, label_line, layout_label, line_middle};
use crate::text::measurer::{TextMeasurer, default_measurer};
use crate::types::{IconResolver, RenderOptions, Rendered};
use crate::util::yaml::Value;

/// Mermaid のノードの種類の番号ごと。区切りのないノードは線の上の文字（根のほか）。
const SHAPES: [&str; 7] = ["", "rounded", "rect", "circle", "cloud", "bang", "hex"];
const ROOT_SHAPE: &str = "stadium";

const GAP_X: f64 = 36.0;
const GAP_Y: f64 = 8.0;
/// 2 つの部分木の間。少なくとも一方に自分の子があるとき。
const GROUP_GAP: f64 = 18.0;
const BRANCH_GAP_X: f64 = 52.0;
const BRANCH_GAP_Y: f64 = 28.0;
const TEXT_PAD: f64 = 6.0;
const PILL_PAD: f64 = 12.0;
const TINT: f64 = 0.22;
const ICON_GAP: f64 = 6.0;
// 縦に流すときは、各段をこれだけ下げ、線はノードの左の端からこれだけの所を出る。
const INDENT: f64 = 28.0;
const STEM: f64 = 12.0;
const BEND: f64 = 8.0;
const ROW_GAP: f64 = 10.0;

/// 各行を `left` から始めるラベル。見出しの並びでは、中央に揃えた行はがたつく。
fn left_label(label: &Label, left: f64, cy: f64, icons: Option<&IconResolver>) -> String {
    if label.lines.len() < 2 {
        return label_svg(
            label,
            left + label.width / 2.0,
            cy,
            " class=\"pele-label\"",
            icons,
        );
    }
    let mut out = String::new();
    let top = cy - label.height / 2.0;
    for k in 0..label.lines.len() {
        out.push_str(&label_svg(
            &label_line(label, k),
            left + label.widths[k] / 2.0,
            top + line_middle(label, k),
            " class=\"pele-label\"",
            icons,
        ));
    }
    out
}

pub fn render_mindmap(
    model: &MindmapDb,
    config: &Config,
    options: &RenderOptions,
) -> Result<Rendered> {
    turn_to_fit(options, |down| Ok(draw(model, config, options, down)))
}

/// 部分木の高さの帯の計算。`rel` は兄弟の中での部分木のずれ、`own` は帯の中でのノードのずれ、`kids` は子の帯の
/// ずれ。
struct Bands {
    extent: Vec<f64>,
    rel: Vec<f64>,
    own: Vec<f64>,
    kids: Vec<f64>,
    stacked: f64,
}

impl Bands {
    /// 部分木を上から下へ積み、親の線の高さを返す。
    fn stack(
        &mut self,
        nodes: &[MindmapNode],
        list: &[usize],
        from: usize,
        to: usize,
        gap: f64,
        anchor: &[f64],
    ) -> f64 {
        let mut y = 0.0;
        let mut last = 0.0;
        for k in from..to {
            let c = list[k];
            self.rel[c] = y;
            let grouped = !nodes[c].children.is_empty()
                || (k + 1 < to && !nodes[list[k + 1]].children.is_empty());
            last = if grouped {
                js::max(gap, GROUP_GAP)
            } else {
                gap
            };
            y += self.extent[c] + last;
        }
        self.stacked = y - last;
        let a = list[from];
        let b = list[to - 1];
        (self.rel[a] + self.own[a] + anchor[a] + self.rel[b] + self.own[b] + anchor[b]) / 2.0
    }
}

/// `down` は図を見出しの並びとして描く: 根を上に、各段を親の下に下げて。両側に広がる枝よりずっと狭く、要る
/// だけ高い。
fn draw(model: &MindmapDb, config: &Config, options: &RenderOptions, down: bool) -> Rendered {
    let size = options.font_size.unwrap_or(16.0);
    let measurer: &dyn TextMeasurer = match &options.measurer {
        Some(m) => m.as_ref(),
        None => default_measurer(),
    };
    let pad = options.padding.unwrap_or(8.0);
    let icons = options.icons.as_ref();
    let wrap = match config
        .get("mindmap")
        .and_then(Value::as_obj)
        .and_then(|m| m.get("maxNodeWidth"))
    {
        Some(Value::Num(w)) if *w > 0.0 => js::min(*w, 2000.0),
        _ => 200.0,
    };
    let glyph = size * 1.25;

    let nodes = &model.nodes;
    let n = nodes.len();
    let mut parent = vec![0usize; n];
    let mut depth = vec![0usize; n];
    for i in 0..n {
        for &child in &nodes[i].children {
            parent[child] = i;
            depth[child] = depth[i] + 1;
        }
    }
    // 見出しの並びはホストの幅から、段の下げを引いた幅を持つ。
    let room = match options.max_width {
        Some(m) if down && m > 0.0 => m - 2.0 * pad,
        _ => f64::INFINITY,
    };
    let mut labels: Vec<Label> = Vec::with_capacity(n);
    let mut shapes: Vec<&str> = Vec::with_capacity(n);
    let mut w = vec![0.0; n];
    let mut h = vec![0.0; n];
    let mut dy = vec![0.0; n];
    // 線がノードに会う所。上から測って、真ん中。
    let mut anchor = vec![0.0; n];
    // ホストがノードのアイコンを持つか。持たないものは場所を取らない。
    let mut drawn = vec![false; n];

    for i in 0..n {
        let node = &nodes[i];
        drawn[i] = node
            .icon
            .as_deref()
            .is_some_and(|icon| !icon_markup(icon).is_empty());
        let fit = js::min(
            wrap,
            js::max(
                72.0,
                room - depth[i] as f64 * INDENT
                    - 2.0 * TEXT_PAD
                    - if drawn[i] { glyph + ICON_GAP } else { 0.0 },
            ),
        );
        let label = layout_label(Some(&node.descr), true, measurer, size, fit, 0);
        let tw = label.width
            + if drawn[i] {
                glyph + if label.width > 0.0 { ICON_GAP } else { 0.0 }
            } else {
                0.0
            };
        let th = js::max(label.height, js::round(size * 1.5));
        let mut shape = match SHAPES.get(node.type_ as usize).copied() {
            Some(s) if !s.is_empty() => s,
            _ if i == 0 => ROOT_SHAPE,
            _ => "",
        };
        // 見出しの並びでは根が列の頭になり、幹はその左の端の下から下りる。丸や雲はそこから外れて立つので、箱で
        // なければ根は丸めた棒にする。
        if down && i == 0 && shape != "rect" && shape != "rounded" {
            shape = ROOT_SHAPE;
        }
        shapes.push(shape);
        if shape.is_empty() {
            // 自分の形のないノードは、枝の終わりなら文字だけ、枝が続くなら色を付けた丸い棒の中の文字にし、段を
            // 見分けやすくする。
            let pill = !node.children.is_empty();
            w[i] = tw + 2.0 * if pill { PILL_PAD } else { TEXT_PAD };
            h[i] = th + if pill { 6.0 } else { 0.0 };
            anchor[i] = h[i] / 2.0;
        } else {
            let s = shape_size(shape, tw, th);
            w[i] = s.w;
            h[i] = s.h;
            dy[i] = s.dy;
            anchor[i] = s.h / 2.0;
        }
        labels.push(label);
    }

    let mut bands = Bands {
        extent: vec![0.0; n],
        rel: vec![0.0; n],
        own: vec![0.0; n],
        kids: vec![0.0; n],
        stacked: 0.0,
    };

    // ノードは深さ優先の順なので、後ろから回れば親より先に子を訪ねる。
    for i in (1..n).rev() {
        let children = &nodes[i].children;
        if children.is_empty() {
            bands.extent[i] = h[i];
            continue;
        }
        let top = bands.stack(nodes, children, 0, children.len(), GAP_Y, &anchor) - anchor[i];
        let low = js::min(0.0, top);
        bands.extent[i] = js::max(bands.stacked, top + h[i]) - low;
        bands.own[i] = top - low;
        bands.kids[i] = -low;
    }

    let mut x = vec![0.0; n];
    let mut y = vec![0.0; n];
    let mut band = vec![0.0; n];
    let mut side = vec![0.0f64; n];
    // 縦に流すとき: 各ノードの下の幹をどこまで描いたか。
    let mut trunk = vec![0.0; n];

    if down {
        // ノードは深さ優先の順で、見出しの並びの行の順。
        let mut row = 0.0;
        for i in 0..n {
            if i > 0 && parent[i] == 0 {
                row += ROW_GAP;
            }
            x[i] = depth[i] as f64 * INDENT + w[i] / 2.0;
            y[i] = row;
            row += h[i] + ROW_GAP;
        }
    } else if n > 0 {
        // 最初の枝は右へ、残りは左へ。両側の高さがいちばん近い所で分ける。
        let branches = &nodes[0].children;
        let count = branches.len();
        let mut total = 0.0;
        for &branch in branches {
            total += bands.extent[branch];
        }
        let mut split = count;
        let mut best = f64::INFINITY;
        let mut right = 0.0;
        for k in 1..count {
            right += bands.extent[branches[k - 1]];
            let diff = (2.0 * right - total + (2.0 * k as f64 - count as f64) * BRANCH_GAP_Y).abs();
            if diff < best {
                best = diff;
                split = k;
            }
        }
        let (mut from, mut to, mut dir) = (0, split, 1.0);
        while from < count {
            let middle = bands.stack(nodes, branches, from, to, BRANCH_GAP_Y, &anchor);
            for &c in &branches[from..to] {
                band[c] = bands.rel[c] - middle;
                side[c] = dir;
            }
            from = to;
            to = count;
            dir = -1.0;
        }
        y[0] = -h[0] / 2.0;
        for i in 0..n {
            if i > 0 {
                y[i] = band[i] + bands.own[i];
            }
            for &c in &nodes[i].children {
                if i > 0 {
                    side[c] = side[i];
                    band[c] = band[i] + bands.kids[i] + bands.rel[c];
                }
                x[c] = x[i]
                    + side[c]
                        * (w[i] / 2.0 + if i > 0 { GAP_X } else { BRANCH_GAP_X } + w[c] / 2.0);
            }
        }
    }

    let mut min_x: f64 = 0.0;
    let mut max_x: f64 = 0.0;
    let mut min_y: f64 = 0.0;
    let mut max_y: f64 = 0.0;
    let mut out = String::new();
    let mut edges = String::new();
    let mut body = String::new();
    let mut color = String::new();
    let flush = |section: Option<usize>,
                 out: &mut String,
                 edges: &mut String,
                 body: &mut String,
                 color: &str| {
        if body.is_empty() {
            return;
        }
        out.push_str(&format!(
            "<g class=\"pele-branch pele-section-{}\"><g class=\"pele-edges\" fill=\"none\" stroke=\"{color}\" stroke-width=\"1.5\">{edges}</g>{body}</g>",
            section.unwrap_or(0)
        ));
        edges.clear();
        body.clear();
    };

    // 根は最後に、そこを出る線の上に描く。
    for k in 1..=n {
        let i = k % n;
        let node = &nodes[i];
        let shape = shapes[i];
        let label = &labels[i];
        let cx = x[i];
        let top = y[i];
        min_x = js::min(min_x, cx - w[i] / 2.0);
        max_x = js::max(max_x, cx + w[i] / 2.0);
        min_y = js::min(min_y, top);
        max_y = js::max(max_y, top + h[i]);

        if i == 0 || parent[i] == 0 {
            flush(
                nodes[k - 1].section,
                &mut out,
                &mut edges,
                &mut body,
                &color,
            );
        }
        if i > 0 && down {
            if parent[i] == 0 {
                color = series_color(node.section.unwrap_or(0) as i64);
            }
            let p = parent[i];
            // 線は親の下から下り、ノードの左へ曲がる。上の兄弟への線が曲がった所から始まるので、共有の幹の
            // 各区間は 1 つの色。根の丸い端の下では、その端が下まで下りた所から幹が始まる。
            let stem = if p == 0 {
                js::min(js::max(STEM, h[0] / 2.0), INDENT - BEND)
            } else {
                STEM
            };
            let x1 = x[p] - w[p] / 2.0 + js::min(stem, w[p] / 2.0);
            let y1 = if js::truthy(trunk[p]) {
                trunk[p]
            } else {
                y[p] + h[p]
            };
            let left = cx - w[i] / 2.0;
            let x2 = left
                + if shape.is_empty() {
                    0.0
                } else {
                    shape_inset(shape, w[i], h[i], 3, 0.0)
                };
            let y2 = top + anchor[i];
            let bend = js::max(0.0, js::min(BEND, js::min(x2 - x1, y2 - y1)));
            // 次の線はこの線が幹を出る所から続くので、幹に切れ目がない。
            trunk[p] = y2 - bend;
            edges.push_str(&format!(
                "<path class=\"pele-edge\" d=\"M{},{}V{}Q{},{} {},{}H{}\"/>",
                num(x1),
                num(y1),
                num(y2 - bend),
                num(x1),
                num(y2),
                num(x1 + bend),
                num(y2),
                num(x2)
            ));
        } else if i > 0 {
            if parent[i] == 0 {
                color = series_color(node.section.unwrap_or(0) as i64);
            }
            let p = parent[i];
            let dir = side[i];
            let from = if dir > 0.0 { 1 } else { 3 };
            let x1 = x[p]
                + dir
                    * (w[p] / 2.0
                        - if shapes[p].is_empty() {
                            0.0
                        } else {
                            shape_inset(shapes[p], w[p], h[p], from, 0.0)
                        });
            let y1 = y[p] + anchor[p];
            let near = cx - (dir * w[i]) / 2.0;
            let x2 = near
                + if shape.is_empty() {
                    0.0
                } else {
                    dir * shape_inset(shape, w[i], h[i], 4 - from, 0.0)
                };
            let y2 = top + anchor[i];
            let mid = num((x1 + x2) / 2.0);
            edges.push_str(&format!(
                "<path class=\"pele-edge\" d=\"M{},{}C{mid},{} {mid},{} {},{}\"/>",
                num(x1),
                num(y1),
                num(y1),
                num(y2),
                num(x2),
                num(y2)
            ));
        }

        let pill = shape.is_empty() && !node.children.is_empty();
        // 根から外れたノードはどれも、枝の色の淡い色で塗り、その色で縁取る（書いた形でも、枝の続くノードの丸い
        // 棒でも）。根は中立の色。
        let tint = format!(
            " fill=\"{color}\" fill-opacity=\"{}\" stroke=\"{color}\"",
            js::fmt(TINT)
        );
        let mut inner = String::new();
        if !shape.is_empty() {
            inner = if i > 0 {
                draw_shape(shape, w[i], h[i], &tint, &format!(" stroke=\"{color}\""))
            } else {
                draw_shape(
                    shape,
                    w[i],
                    h[i],
                    " fill=\"var(--_s)\" stroke=\"var(--_b)\"",
                    " stroke=\"var(--_b)\"",
                )
            };
        } else if pill {
            inner = format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\"{tint}/>",
                num(-w[i] / 2.0),
                num(-h[i] / 2.0),
                num(w[i]),
                num(h[i]),
                num(h[i] / 2.0)
            );
        }
        let mut shift = 0.0;
        if let Some(icon) = node.icon.as_deref().filter(|_| drawn[i]) {
            shift = (glyph + if label.width > 0.0 { ICON_GAP } else { 0.0 }) / 2.0;
            inner.push_str(&icon_svg(
                icon,
                -shift - label.width / 2.0,
                dy[i] - glyph / 2.0,
                glyph,
                icons,
            ));
        }
        let kind = if !shape.is_empty() {
            shape
        } else if pill {
            "branch"
        } else {
            "text"
        };
        let group = format!(
            "<g class=\"pele-node pele-shape-{kind}{}{}\" data-id=\"{}\" transform=\"translate({},{})\">{inner}{}</g>",
            if i > 0 { "" } else { " pele-root" },
            class_names(node.class.as_deref().unwrap_or_default()),
            esc_text(&node.node_id),
            num(cx),
            num(top + h[i] / 2.0),
            if down && shape.is_empty() {
                left_label(label, shift - label.width / 2.0, dy[i], icons)
            } else {
                label_svg(label, shift, dy[i], " class=\"pele-label\"", icons)
            }
        );
        if i > 0 {
            body.push_str(&group);
        } else {
            out.push_str(&group);
        }
    }

    let title = title_label(model.title.as_deref(), measurer, size, title_room(options));
    let title_height = if title.height > 0.0 {
        title.height + 16.0
    } else {
        0.0
    };
    let width = js::max(max_x - min_x, title.width);
    let total_width = (width + 2.0 * pad).ceil();
    let total_height = (max_y - min_y + title_height + 2.0 * pad).ceil();
    let content = label_svg(
        &title,
        total_width / 2.0,
        pad + title.height / 2.0,
        " class=\"pele-title\" font-weight=\"var(--_tw)\"",
        None,
    ) + &format!(
        "<g transform=\"translate({},{})\">{out}</g>",
        num(pad - min_x + (width - max_x + min_x) / 2.0),
        num(pad + title_height - min_y)
    );

    Rendered {
        svg: svg_document(
            "mindmap",
            total_width,
            total_height,
            size,
            options,
            &Accessible::default(),
            &content,
        ),
        width: total_width,
        height: total_height,
        links: Vec::new(),
    }
}
