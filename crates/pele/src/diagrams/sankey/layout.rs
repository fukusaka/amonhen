//! sankey の配置（pele の `src/diagrams/sankey/layout.ts`）。

use crate::errors::{ErrorCode, PeleError, Result};
use crate::js;

use super::db::SankeyDb;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alignment {
    Justify,
    Left,
    Right,
    Center,
}

pub struct LayoutOptions {
    pub width: f64,
    pub height: f64,
    pub node_width: f64,
    pub node_padding: f64,
    /// ラベルの 1 行の高さ。細い節も、場所があればこれだけ離す。
    pub label_height: f64,
    pub alignment: Alignment,
}

#[derive(Clone, Debug)]
pub struct PlacedNode {
    pub layer: usize,
    /// 節を通る流れの合計（図の単位で）。
    pub value: f64,
    pub x: f64,
    pub y: f64,
    pub height: f64,
}

#[derive(Clone, Debug)]
pub struct PlacedLink {
    pub source: usize,
    pub target: usize,
    pub value: f64,
    pub thickness: f64,
    /// 帯が元を出る所と先に入る所の、縦の中心。
    pub source_y: f64,
    pub target_y: f64,
}

pub struct SankeyLayout {
    pub nodes: Vec<PlacedNode>,
    pub links: Vec<PlacedLink>,
}

const PASSES: i32 = 6;

/// JS の比べる関数の答えを順に（`> 0` だけを見る挿入の並べ替えと同じく、NaN は等しい）。
fn ordering(v: f64) -> std::cmp::Ordering {
    js::order(v)
}

/// たいていの節は帯が少ない。組み込みの並べ替えは短い列では立ち上がりが遅い。
fn sort(list: &mut [usize], compare: impl Fn(usize, usize) -> f64) {
    if list.len() > 8 {
        list.sort_by(|&a, &b| ordering(compare(a, b)));
        return;
    }
    for i in 1..list.len() {
        let item = list[i];
        let mut j = i as isize - 1;
        while j >= 0 && compare(list[j as usize], item) > 0.0 {
            list[(j + 1) as usize] = list[j as usize];
            j -= 1;
        }
        list[(j + 1) as usize] = item;
    }
}

/// `a || b`（数の）。
fn or(a: f64, b: f64) -> f64 {
    if js::truthy(a) { a } else { b }
}

/// 節を元からの距離で列に置き、流れで大きさを決め、帯の交わりが減るよう列ごとに隣に合わせて落ち着かせる。
pub fn layout_sankey(model: &SankeyDb, options: &LayoutOptions) -> Result<SankeyLayout> {
    let n = model.nodes.len();
    let mut links: Vec<PlacedLink> = model
        .links
        .iter()
        .map(|link| PlacedLink {
            source: link.source,
            target: link.target,
            value: if link.value.is_finite() && link.value > 0.0 {
                link.value
            } else {
                0.0
            },
            thickness: 0.0,
            source_y: 0.0,
            target_y: 0.0,
        })
        .collect();
    let mut outgoing: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut incoming: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (k, link) in links.iter().enumerate() {
        outgoing[link.source].push(k);
        incoming[link.target].push(k);
    }

    // 元からの最も長い距離を、トポロジカルの順に。残るものは輪の上にある。
    let mut depth = vec![0i32; n];
    let mut pending = vec![0i32; n];
    let mut order: Vec<usize> = Vec::new();
    for i in 0..n {
        pending[i] = incoming[i].len() as i32;
        if pending[i] == 0 {
            order.push(i);
        }
    }
    let mut q = 0;
    while q < order.len() {
        let i = order[q];
        for &k in &outgoing[i] {
            let t = links[k].target;
            if depth[t] < depth[i] + 1 {
                depth[t] = depth[i] + 1;
            }
            pending[t] -= 1;
            if pending[t] == 0 {
                order.push(t);
            }
        }
        q += 1;
    }
    if order.len() < n {
        // 残った節にはどれも、残った前の節がある。後ろへたどれば輪が見つかる。
        let mut stuck = 0;
        while pending[stuck] == 0 {
            stuck += 1;
        }
        let mut seen = vec![false; n];
        while !seen[stuck] {
            seen[stuck] = true;
            for &k in &incoming[stuck] {
                if pending[links[k].source] > 0 {
                    stuck = links[k].source;
                    break;
                }
            }
        }
        return Err(PeleError::typed(
            format!(
                "Sankey diagrams cannot have circular links, and \"{}\" is part of one.",
                model.nodes[stuck].id
            ),
            ErrorCode::Semantic,
            "sankey",
        ));
    }
    let mut layers: i32 = 1;
    for &d in &depth {
        if d + 1 > layers {
            layers = d + 1;
        }
    }
    let mut reach = vec![0i32; n];
    for q in (0..n).rev() {
        let i = order[q];
        for &k in &outgoing[i] {
            if reach[i] < reach[links[k].target] + 1 {
                reach[i] = reach[links[k].target] + 1;
            }
        }
    }

    // 流れは最も大きいもので割る。大きな値の和も有限に保つ。
    let mut largest = 0.0;
    for link in &links {
        if link.value > largest {
            largest = link.value;
        }
    }
    let unit = if largest > 0.0 { 1.0 / largest } else { 0.0 };

    let mut nodes: Vec<PlacedNode> = Vec::with_capacity(n);
    let mut flow = vec![0.0f64; n];
    let mut columns: Vec<Vec<usize>> = vec![Vec::new(); layers as usize];
    let step = if layers > 1 {
        (options.width - options.node_width) / (layers - 1) as f64
    } else {
        0.0
    };
    for i in 0..n {
        let layer: i32 = match options.alignment {
            Alignment::Left => depth[i],
            Alignment::Right => layers - 1 - reach[i],
            Alignment::Center => {
                let mut layer = depth[i];
                if incoming[i].is_empty() && !outgoing[i].is_empty() {
                    layer = layers;
                    for &k in &outgoing[i] {
                        if depth[links[k].target] - 1 < layer {
                            layer = depth[links[k].target] - 1;
                        }
                    }
                }
                layer
            }
            Alignment::Justify => {
                if !outgoing[i].is_empty() {
                    depth[i]
                } else {
                    layers - 1
                }
            }
        };
        let (mut into, mut out, mut value, mut value_out) = (0.0, 0.0, 0.0, 0.0);
        for &k in &incoming[i] {
            into += links[k].value * unit;
            value += links[k].value;
        }
        for &k in &outgoing[i] {
            out += links[k].value * unit;
            value_out += links[k].value;
        }
        flow[i] = js::max(into, out);
        let layer = layer as usize;
        nodes.push(PlacedNode {
            layer,
            value: js::max(value, value_out),
            x: layer as f64 * step,
            y: 0.0,
            height: 0.0,
        });
        columns[layer].push(i);
    }

    let mut tallest = 1;
    for column in &columns {
        if column.len() > tallest {
            tallest = column.len();
        }
    }
    let gap = if tallest > 1 {
        js::min(options.node_padding, options.height / (tallest - 1) as f64)
    } else {
        options.node_padding
    };
    let mut scale = f64::INFINITY;
    for column in &columns {
        let mut sum = 0.0;
        for &i in column {
            sum += flow[i];
        }
        if sum > 0.0 {
            scale = js::min(
                scale,
                (options.height - (column.len() as f64 - 1.0) * gap) / sum,
            );
        }
    }
    if !scale.is_finite() || scale < 0.0 {
        scale = 0.0;
    }

    // 細い節もラベルの場所が要る。どの列にも余裕があれば、どの節もラベルの高さは取り、流れをその分縮める。
    let mut claim = js::max(0.0, options.label_height - gap);
    let fits = |s: f64, claim: f64| -> bool {
        for column in &columns {
            let mut extent = (column.len() as f64 - 1.0) * gap;
            for &i in column {
                extent += js::max(flow[i] * s, claim);
            }
            if extent > options.height + 1e-6 {
                return false;
            }
        }
        true
    };
    if claim > 0.0 && !fits(scale, claim) {
        if fits(0.0, claim) {
            let mut low = 0.0;
            for _ in 0..40 {
                let mid = (low + scale) / 2.0;
                if fits(mid, claim) {
                    low = mid;
                } else {
                    scale = mid;
                }
            }
            scale = low;
        } else {
            claim = 0.0;
        }
    }

    // 節が上と下に空けておく場所。
    let mut margin = vec![0.0f64; n];
    for link in links.iter_mut() {
        link.thickness = link.value * unit * scale;
    }
    for column in &columns {
        let mut y = 0.0;
        for &i in column {
            nodes[i].height = flow[i] * scale;
            margin[i] = js::max(0.0, claim - nodes[i].height) / 2.0;
            nodes[i].y = y + margin[i];
            y += nodes[i].height + 2.0 * margin[i] + gap;
        }
        // 余った場所は列の節で分ける。
        let spare = (options.height - y + gap) / (column.len() as f64 + 1.0);
        for (k, &i) in column.iter().enumerate() {
            nodes[i].y += spare * (k as f64 + 1.0);
        }
    }

    // 帯は行き先の順に節を出入りし、節の所で交わらない。帯の中ほどの、両端の節の上からの位置。
    let mut source_offset = vec![0.0f64; links.len()];
    let mut target_offset = vec![0.0f64; links.len()];

    fn center(nodes: &[PlacedNode], i: usize) -> f64 {
        nodes[i].y + nodes[i].height / 2.0
    }
    fn stack_out(
        i: usize,
        nodes: &[PlacedNode],
        links: &[PlacedLink],
        outgoing: &mut [Vec<usize>],
        source_offset: &mut [f64],
    ) {
        let mut y = 0.0;
        sort(&mut outgoing[i], |a, b| {
            or(
                center(nodes, links[a].target) - center(nodes, links[b].target),
                a as f64 - b as f64,
            )
        });
        for &k in &outgoing[i] {
            source_offset[k] = y + links[k].thickness / 2.0;
            y += links[k].thickness;
        }
    }
    fn stack_in(
        i: usize,
        nodes: &[PlacedNode],
        links: &[PlacedLink],
        incoming: &mut [Vec<usize>],
        target_offset: &mut [f64],
    ) {
        let mut y = 0.0;
        sort(&mut incoming[i], |a, b| {
            or(
                center(nodes, links[a].source) - center(nodes, links[b].source),
                a as f64 - b as f64,
            )
        });
        for &k in &incoming[i] {
            target_offset[k] = y + links[k].thickness / 2.0;
            y += links[k].thickness;
        }
    }

    let stack_all = |nodes: &[PlacedNode],
                     outgoing: &mut [Vec<usize>],
                     incoming: &mut [Vec<usize>],
                     source_offset: &mut [f64],
                     target_offset: &mut [f64],
                     links: &[PlacedLink]| {
        for i in 0..n {
            stack_out(i, nodes, links, outgoing, source_offset);
            stack_in(i, nodes, links, incoming, target_offset);
        }
    };

    stack_all(
        &nodes,
        &mut outgoing,
        &mut incoming,
        &mut source_offset,
        &mut target_offset,
        &links,
    );
    for pass in 0..PASSES {
        let pull = 0.99f64.powf(pass as f64);
        for column in columns[..layers as usize - 1].iter_mut().rev() {
            settle(
                column,
                pull,
                false,
                &mut nodes,
                &links,
                &mut outgoing,
                &mut incoming,
                &mut source_offset,
                &mut target_offset,
                &margin,
                gap,
                options.height,
            );
        }
        stack_all(
            &nodes,
            &mut outgoing,
            &mut incoming,
            &mut source_offset,
            &mut target_offset,
            &links,
        );
        for column in columns.iter_mut().skip(1) {
            settle(
                column,
                pull,
                true,
                &mut nodes,
                &links,
                &mut outgoing,
                &mut incoming,
                &mut source_offset,
                &mut target_offset,
                &margin,
                gap,
                options.height,
            );
        }
        stack_all(
            &nodes,
            &mut outgoing,
            &mut incoming,
            &mut source_offset,
            &mut target_offset,
            &links,
        );
    }
    for k in 0..links.len() {
        links[k].source_y = nodes[links[k].source].y + source_offset[k];
        links[k].target_y = nodes[links[k].target].y + target_offset[k];
    }

    /// 列の節を、帯が水平に走る所へ寄せ、順と間を戻す。`forward` は入る帯を、でなければ出る帯を見る。
    #[allow(clippy::too_many_arguments)]
    fn settle(
        column: &mut [usize],
        pull: f64,
        forward: bool,
        nodes: &mut [PlacedNode],
        links: &[PlacedLink],
        outgoing: &mut [Vec<usize>],
        incoming: &mut [Vec<usize>],
        source_offset: &mut [f64],
        target_offset: &mut [f64],
        margin: &[f64],
        gap: f64,
        height: f64,
    ) {
        for &i in column.iter() {
            let empty = if forward {
                incoming[i].is_empty()
            } else {
                outgoing[i].is_empty()
            };
            if empty {
                continue;
            }
            if forward {
                stack_in(i, nodes, links, incoming, target_offset);
            } else {
                stack_out(i, nodes, links, outgoing, source_offset);
            }
            let list = if forward { &incoming[i] } else { &outgoing[i] };
            let (mut weighted, mut weight, mut plain) = (0.0, 0.0, 0.0);
            for &k in list {
                let level = if forward {
                    nodes[links[k].source].y + source_offset[k] - target_offset[k]
                } else {
                    nodes[links[k].target].y + target_offset[k] - source_offset[k]
                };
                weighted += level * links[k].thickness;
                weight += links[k].thickness;
                plain += center(
                    nodes,
                    if forward {
                        links[k].source
                    } else {
                        links[k].target
                    },
                ) - nodes[i].height / 2.0;
            }
            let wanted = if weight > 0.0 {
                weighted / weight
            } else {
                plain / list.len() as f64
            };
            nodes[i].y += (wanted - nodes[i].y) * pull;
        }
        sort(column, |a, b| {
            or(center(nodes, a) - center(nodes, b), a as f64 - b as f64)
        });
        let mut y = 0.0;
        for &i in column.iter() {
            if nodes[i].y - margin[i] < y {
                nodes[i].y = y + margin[i];
            }
            y = nodes[i].y + nodes[i].height + margin[i] + gap;
        }
        y = height;
        for &i in column.iter().rev() {
            if nodes[i].y + nodes[i].height + margin[i] > y {
                nodes[i].y = y - nodes[i].height - margin[i];
            }
            y = nodes[i].y - margin[i] - gap;
        }
    }

    Ok(SankeyLayout { nodes, links })
}
