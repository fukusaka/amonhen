//! ishikawa を描く（pele の `src/diagrams/ishikawa/render.ts`）。

use super::db::IshikawaDb;
use crate::diagrams::common::fit_width::{title_room, turn_to_fit};
use crate::diagrams::common::literal_label::break_label;
use crate::errors::Result;
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc_text, label_svg, num};
use crate::svg::edges::{ARROW, marker};
use crate::svg::root::{Accessible, svg_document};
use crate::svg::theme::ROUNDED;
use crate::text::label::{Label, layout_label};
use crate::text::measurer::{TextMeasurer, default_measurer, style};
use crate::types::{RenderOptions, Rendered};

/// 骨が背骨から 1 だけ離れるごとに尾へ寄る量。
const SLANT: f64 = 0.5;
const FIRST_ROW: f64 = 14.0;
const ROW_GAP: f64 = 6.0;
const BONE_END: f64 = 8.0;
const TWIG_STEP: f64 = 22.0;
const STUB: f64 = 18.0;
const TEXT_GAP: f64 = 5.0;
const PAIR_GAP: f64 = 26.0;
const HEAD_GAP: f64 = 34.0;
const TAIL: f64 = 28.0;
const BOX_PAD_X: f64 = 10.0;
const BOX_PAD_Y: f64 = 5.0;

fn length() -> f64 {
    js::hypot(SLANT, 1.0)
}

/// 分類の下の原因。偶数の深さの骨は上の斜めの骨から尾へ水平に、奇数の深さの骨は上の水平の骨から背骨の外へ
/// 斜めに走る。
struct Bone {
    label: Label,
    depth: usize,
    parent: Option<usize>,
    /// 兄弟の中の位置（背骨から外へ数える）。
    slot: usize,
    kids: usize,
    /// その行の背骨からの距離。
    rise: f64,
    /// 親と会う所。分類の骨から水平に測り、骨の上で 0、尾の方へ負。
    at: f64,
    /// 文字の終わる所。
    text_end: f64,
}

struct Category {
    node: usize,
    label: Label,
    box_w: f64,
    box_h: f64,
    bones: Vec<Bone>,
    /// 骨の長さ（背骨からの距離）。
    rise: f64,
    /// 原因が尾の方へ届く所（骨から水平に測る）。
    reach: f64,
}

struct Frame {
    node: usize,
    depth: usize,
    parent: Option<usize>,
    slot: usize,
    /// 骨を作り、行だけが残るときに立つ。
    bone: Option<usize>,
}

/// 分類のどの原因にも自分の行を与え、2 つの文字が出会わないようにし、全体がどこまで届くかを測る。水平の骨は
/// 背骨にいちばん近い行を、その小枝はその先の行を取る。斜めの小枝は、そこから下がるものすべての先の行を取る。
/// 字下げの続く限り深く入れ子にできるので、自分の積みで歩く。
fn arrange(
    db: &IshikawaDb,
    node: usize,
    up: bool,
    measurer: &dyn TextMeasurer,
    size: f64,
    small: f64,
) -> Category {
    let nodes = &db.nodes;
    let label = layout_label(Some(&nodes[node].text), false, measurer, size, 180.0, 0);
    let line_height = js::round(small * 1.5);
    let mut bones: Vec<Bone> = Vec::new();
    let mut rows: Vec<usize> = Vec::new();
    let mut stack: Vec<Frame> = Vec::new();
    // 背骨の上では最初の原因がいちばん外へ行き、原因が上から読めるようにする。
    let push = |stack: &mut Vec<Frame>, children: &[usize], depth: usize, parent: Option<usize>| {
        let n = children.len();
        for slot in (0..n).rev() {
            stack.push(Frame {
                node: children[if up { n - 1 - slot } else { slot }],
                depth,
                parent,
                slot,
                bone: None,
            });
        }
    };
    push(&mut stack, &nodes[node].children, 2, None);
    while let Some(mut frame) = stack.pop() {
        if let Some(bone) = frame.bone {
            rows.push(bone);
            continue;
        }
        let index = bones.len();
        let n = &nodes[frame.node];
        bones.push(Bone {
            label: layout_label(Some(&n.text), false, measurer, small, 200.0, 0),
            depth: frame.depth,
            parent: frame.parent,
            slot: frame.slot,
            kids: n.children.len(),
            rise: 0.0,
            at: 0.0,
            text_end: 0.0,
        });
        let (children, depth) = (&n.children, frame.depth);
        if frame.depth.is_multiple_of(2) {
            rows.push(index);
        } else {
            frame.bone = Some(index);
            stack.push(frame);
        }
        push(&mut stack, children, depth + 1, Some(index));
    }

    let mut rise = FIRST_ROW;
    for &index in &rows {
        let height = js::max(bones[index].label.height, line_height) + ROW_GAP;
        bones[index].rise = rise + height / 2.0;
        rise += height;
    }
    let box_w = label.width + 2.0 * BOX_PAD_X;
    let mut reach = box_w / 2.0;
    for k in 0..bones.len() {
        let parent = bones[k].parent.map(|p| (bones[p].at, bones[p].kids));
        let bone = &mut bones[k];
        if bone.depth.is_multiple_of(2) {
            bone.at = parent.map_or(0.0, |p| p.0);
            bone.text_end = bone.at
                - (if bone.kids > 0 {
                    (bone.kids + 1) as f64 * TWIG_STEP
                } else {
                    STUB
                })
                - TEXT_GAP;
        } else {
            // 小枝は水平の骨に沿って立ち、背骨にいちばん近いものが尾にいちばん近い。
            let (pat, pkids) = parent.unwrap();
            bone.at = pat - (pkids as f64 - bone.slot as f64) * TWIG_STEP;
            bone.text_end = bone.at - TEXT_GAP;
        }
        reach = js::max(reach, bone.label.width - bone.text_end);
    }
    Category {
        node,
        box_h: label.height + 2.0 * BOX_PAD_Y,
        label,
        box_w,
        bones,
        rise: rise + BONE_END,
        reach,
    }
}

pub fn render_ishikawa(
    model: &IshikawaDb,
    config: &Config,
    options: &RenderOptions,
) -> Result<Rendered> {
    turn_to_fit(options, |down| Ok(draw(model, config, options, down)))
}

fn draw(model: &IshikawaDb, _config: &Config, options: &RenderOptions, down: bool) -> Rendered {
    let size = options.font_size.unwrap_or(16.0);
    let small = js::round(size * 0.875);
    let measurer: &dyn TextMeasurer = match &options.measurer {
        Some(m) => m.as_ref(),
        None => default_measurer(),
    };
    let pad = options.padding.unwrap_or(8.0);
    if down {
        return vertical_causes(model, options);
    }
    let title = layout_label(
        model.title.as_deref(),
        false,
        measurer,
        size,
        4000.0,
        style::BOLD,
    );
    let title_height = if title.height > 0.0 {
        title.height + 12.0
    } else {
        0.0
    };
    let len = length();

    let mut body = String::new();
    let mut min_x = 0.0;
    let mut max_x = 0.0;
    let mut min_y = 0.0;
    let mut max_y = 0.0;
    if let Some(root) = model.root() {
        let root = &model.nodes[root];
        let head = layout_label(Some(&root.text), false, measurer, size, 200.0, style::BOLD);
        let head_h = js::max(head.height + 24.0, 48.0);
        let head_w = head.width + 24.0 + head_h / 2.0;
        let mut categories: Vec<Category> = root
            .children
            .iter()
            .enumerate()
            .map(|(i, &node)| arrange(model, node, i % 2 == 0, measurer, size, small))
            .collect();
        // 原因のない分類にも、頭から離して名前を載せるだけの長さの骨を与える。
        for category in &mut categories {
            category.rise = js::max(category.rise, head_h / 2.0 + 14.0);
        }

        // 分類は 2 つ組で背骨に会う（上と下に 1 つずつ）。組は前の組から、2 つの広い方が要るだけ尾へ寄る。
        let mut xs: Vec<f64> = Vec::new();
        let mut x = -HEAD_GAP;
        let mut i = 0;
        while i < categories.len() {
            if i > 0 {
                let mut room: f64 = 0.0;
                for side in 0..2 {
                    let before = categories.get(i - 2 + side);
                    let next = categories.get(i + side);
                    if let Some(before) = before {
                        room = js::max(room, before.reach + next.map_or(0.0, |n| n.box_w / 2.0));
                    }
                }
                x -= room + PAIR_GAP;
            }
            xs.push(x);
            xs.push(x);
            i += 2;
        }
        let tail = if categories.is_empty() {
            -HEAD_GAP
        } else {
            x - TAIL
        };

        max_x = head_w;
        min_x = tail;
        min_y = -head_h / 2.0;
        max_y = head_h / 2.0;
        let mut groups = String::new();
        for (i, category) in categories.iter().enumerate() {
            let sign = if i % 2 == 0 { -1.0 } else { 1.0 };
            let ax = xs[i];
            let px = |at: f64, rise: f64| -> f64 { ax + at - rise * SLANT };
            let end_x = px(0.0, category.rise);
            let end_y = sign * category.rise;
            // 骨は矢じりの先をきれいに残すよう、背骨の手前で止まる。
            let trim = ARROW - 1.0;
            let mut d = format!(
                "M{},{}L{},{}",
                num(ax - (SLANT / len) * trim),
                num((sign * trim) / len),
                num(end_x),
                num(end_y)
            );
            let mut texts = String::new();
            for bone in &category.bones {
                let y = sign * bone.rise;
                if bone.depth.is_multiple_of(2) {
                    d.push_str(&format!(
                        "M{},{}H{}",
                        num(px(bone.at, bone.rise)),
                        num(y),
                        num(px(bone.text_end + TEXT_GAP, bone.rise))
                    ));
                } else {
                    let from = category.bones[bone.parent.unwrap()].rise;
                    d.push_str(&format!(
                        "M{},{}L{},{}",
                        num(px(bone.at, from)),
                        num(sign * from),
                        num(px(bone.at, bone.rise)),
                        num(y)
                    ));
                }
                let right = px(bone.text_end, bone.rise);
                texts.push_str(&label_svg(
                    &bone.label,
                    right - bone.label.width / 2.0,
                    y,
                    if bone.depth > 2 {
                        " fill=\"var(--_m)\""
                    } else {
                        ""
                    },
                    None,
                ));
                min_x = js::min(min_x, right - bone.label.width);
            }
            let box_y = end_y + sign * (category.box_h / 2.0);
            min_x = js::min(min_x, end_x - category.box_w / 2.0);
            max_x = js::max(max_x, end_x + category.box_w / 2.0);
            min_y = js::min(min_y, box_y - category.box_h / 2.0);
            max_y = js::max(max_y, box_y + category.box_h / 2.0);
            groups.push_str(&format!(
                "<g class=\"pele-ishikawa-category\" data-id=\"{}\"><path class=\"pele-ishikawa-bones\" d=\"{d}\" fill=\"none\" stroke=\"var(--_l)\"/>{}<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_s)\" stroke=\"var(--_b)\"{ROUNDED}/>{}{}</g>",
                esc_text(&model.nodes[category.node].text),
                marker(
                    "arrow_point",
                    ax,
                    0.0,
                    SLANT / len,
                    -sign / len,
                    "var(--_l)",
                    "none"
                ),
                num(end_x - category.box_w / 2.0),
                num(box_y - category.box_h / 2.0),
                num(category.box_w),
                num(category.box_h),
                label_svg(&category.label, end_x, box_y, " class=\"pele-label\"", None),
                if texts.is_empty() {
                    String::new()
                } else {
                    format!(
                        "<g class=\"pele-ishikawa-causes\" font-size=\"{}\">{texts}</g>",
                        js::fmt(small)
                    )
                }
            ));
        }

        let r = head_h / 2.0;
        body = format!(
            "<path class=\"pele-ishikawa-spine\" d=\"M{},0H{}\" fill=\"none\" stroke=\"var(--_l)\"/>{}{groups}<g class=\"pele-ishikawa-head\" data-id=\"{}\"><path d=\"M0,{}H{}A{},{} 0 0 1 {},{}H0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\" stroke-linejoin=\"round\"/>{}</g>",
            num(tail),
            num(-(ARROW - 1.0)),
            marker("arrow_point", 0.0, 0.0, 1.0, 0.0, "var(--_l)", "none"),
            esc_text(&root.text),
            num(-r),
            num(head_w - r),
            num(r),
            num(r),
            num(head_w - r),
            num(r),
            label_svg(
                &head,
                12.0 + head.width / 2.0,
                0.0,
                " class=\"pele-label\" font-weight=\"var(--_hw)\"",
                None
            )
        );
    }

    let inner = js::max(max_x - min_x, title.width);
    let width = (inner + 2.0 * pad).ceil();
    let height = (max_y - min_y + title_height + 2.0 * pad).ceil();
    let ox = pad + (inner - (max_x - min_x)) / 2.0 - min_x;
    let oy = pad + title_height - min_y;
    let mut content = label_svg(
        &title,
        width / 2.0,
        pad + title.height / 2.0,
        " class=\"pele-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    if !body.is_empty() {
        content.push_str(&format!(
            "<g transform=\"translate({},{})\">{body}</g>",
            num(ox),
            num(oy)
        ));
    }
    let svg = svg_document(
        "ishikawa",
        width,
        height,
        size,
        options,
        &Accessible::default(),
        &content,
    );
    Rendered {
        svg,
        width,
        height,
        links: Vec::new(),
    }
}

struct Entry {
    node: usize,
    depth: usize,
    parent: Option<usize>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

fn vertical_causes(model: &IshikawaDb, options: &RenderOptions) -> Rendered {
    let size = options.font_size.unwrap_or(16.0);
    let small = js::round(size * 0.875);
    let measurer: &dyn TextMeasurer = match &options.measurer {
        Some(m) => m.as_ref(),
        None => default_measurer(),
    };
    let pad = options.padding.unwrap_or(8.0);
    let room = js::max(140.0, options.max_width.unwrap_or(320.0) - 2.0 * pad);
    let title = layout_label(
        model.title.as_deref(),
        false,
        measurer,
        size,
        title_room(options),
        style::BOLD,
    );
    let mut order: Vec<Entry> = Vec::new();
    let mut pending: Vec<(usize, usize, Option<usize>)> =
        model.root().map(|r| (r, 0, None)).into_iter().collect();
    let mut depth = 0;
    while let Some((node, d, parent)) = pending.pop() {
        let index = order.len();
        order.push(Entry {
            node,
            depth: d,
            parent,
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
        });
        depth = depth.max(d);
        for &child in model.nodes[node].children.iter().rev() {
            pending.push((child, d + 1, Some(index)));
        }
    }
    let indent = js::min(
        20.0,
        js::max(2.0, (room - 120.0) / js::max(1.0, depth as f64)),
    );
    let mut y = pad
        + if title.height != 0.0 {
            title.height + 12.0
        } else {
            0.0
        };
    let mut width = title.width;
    let mut nodes = String::new();
    let mut edges = String::new();
    for k in 0..order.len() {
        let entry = &mut order[k];
        let d = entry.depth;
        entry.x = pad + d as f64 * indent;
        let label_room = js::max(60.0, room - d as f64 * indent - 20.0);
        let text = &model.nodes[entry.node].text;
        let label = break_label(
            &layout_label(
                Some(text),
                false,
                measurer,
                if d < 2 { size } else { small },
                label_room,
                if d < 2 { style::BOLD } else { 0 },
            ),
            measurer,
            label_room,
        );
        entry.w = label.width + 20.0;
        entry.h = label.height + 12.0;
        entry.y = y;
        y += entry.h + 12.0;
        width = js::max(width, entry.x - pad + entry.w);
        let framed = d < 2;
        let (ex, ey, ew, eh) = (entry.x, entry.y, entry.w, entry.h);
        nodes.push_str(&format!(
            "<g class=\"pele-node {}\" data-id=\"{}\">{}{}</g>",
            match d {
                0 => "pele-ishikawa-head",
                1 => "pele-ishikawa-category",
                _ => "pele-ishikawa-cause",
            },
            esc_text(text),
            if framed {
                format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>",
                    num(ex),
                    num(ey),
                    num(ew),
                    num(eh),
                    if d == 0 { 12 } else { 4 }
                )
            } else {
                String::new()
            },
            label_svg(
                &label,
                ex + ew / 2.0,
                ey + eh / 2.0,
                &format!(
                    " class=\"pele-label\"{}{}",
                    if d < 2 {
                        " font-weight=\"var(--_hw)\""
                    } else {
                        ""
                    },
                    if d > 1 {
                        format!(" font-size=\"{}\"", js::fmt(small))
                    } else {
                        String::new()
                    }
                ),
                None
            )
        ));
        if let Some(p) = order[k].parent {
            let parent = &order[p];
            let gx = parent.x + indent / 2.0;
            edges.push_str(&format!(
                "<path class=\"pele-edge\" d=\"M{},{}V{}H{}\" fill=\"none\" stroke=\"var(--_b)\"/>",
                num(gx),
                num(parent.y + parent.h),
                num(ey + eh / 2.0),
                num(ex)
            ));
        }
    }
    let total_width = (width + 2.0 * pad).ceil();
    let height = (js::max(pad + title.height, y - 12.0) + pad).ceil();
    let mut content = label_svg(
        &title,
        total_width / 2.0,
        pad + title.height / 2.0,
        " class=\"pele-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    content.push_str(&edges);
    content.push_str(&nodes);
    Rendered {
        svg: svg_document(
            "ishikawa",
            total_width,
            height,
            size,
            options,
            &Accessible::default(),
            &content,
        ),
        width: total_width,
        height,
        links: Vec::new(),
    }
}
