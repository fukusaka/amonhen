//! swimlane の配置（pele の `src/layout/lanes.ts`）。いちばん外のグループはどれもレーン: 流れに沿って図の端から
//! 端まで走る帯。ランクはすべてのレーンで共通なので、ある段は、ほかのレーンで同じときに起きる段と並び、どの
//! ノードも自分のレーンの中にとどまる。

#![allow(clippy::needless_range_loop, clippy::too_many_arguments)]

use std::collections::HashSet;

use super::compound::{CEdge, CNode, CompoundResult, Dir};
use super::layered::{Kind, LEdge, LNode, LayeredOptions, ledge, lnode, orient, rank, sort_by};
use crate::js;

const LANE_HEAD: f64 = 34.0;
/// レーンがその中身のまわりに取る既定の余白（TS の `laneLayout` の `lanePad` の既定）。
pub const LANE_PAD: f64 = 24.0;
const SWEEPS: usize = 8;
const MAX_DUMMIES: i64 = 20000;
const MAX_HOPS: usize = 10000;
const ROUTE_BUDGET: f64 = 2000000.0;
/// これより近い平行な線は 1 本に見える。
const NEAR: f64 = 4.0;
const ROUNDS: usize = 3;
const RELAY: f64 = 8.0;
const TURN: f64 = 20.0;
/// レーンが縁の受け渡しのそばに取る余白と、そのうち受け渡しとレーンの中身の間の分。
const MARGIN: f64 = 20.0;
const LEG: f64 = 14.0;
const CLOSE: f64 = 3.0;

/// `a || b`（数）。
fn or(a: f64, b: f64) -> f64 {
    if js::truthy(a) { a } else { b }
}

struct Ctx<'a> {
    n: usize,
    sideways: bool,
    flipped: bool,
    opt: &'a LayeredOptions,
    lanes: &'a [usize],
    lane_count: usize,
    lane_slot: &'a [i32],
    item_of: &'a [i32],
    real: &'a [usize],
    lnodes: &'a [LNode],
    ledges: &'a [LEdge],
    edge_of: &'a [usize],
    step: i32,
    item_lane: &'a [i32],
    lane_pads: &'a [f64],
    end_pad: f64,
}

struct Placed {
    width: f64,
    height: f64,
    routed: Vec<u8>,
    margins: Vec<u8>,
    work: f64,
}

/// レーンの配置。`lane_pad` はレーンが中身のまわりに取る余白（既定は `LANE_PAD`）。
pub fn lane_layout(
    nodes: &mut [CNode],
    edges: &mut [CEdge],
    dir: Dir,
    opt: &LayeredOptions,
    lane_pad: f64,
) -> CompoundResult {
    let n = nodes.len();
    if n == 0 {
        return CompoundResult {
            width: 0.0,
            height: 0.0,
        };
    }
    let sideways = matches!(dir, Dir::LR | Dir::RL);
    let flipped = matches!(dir, Dir::BT | Dir::RL);

    // 書いた順のレーン。グラフはグループを、後に宣言したものから並べる。
    let lanes: Vec<usize> = (0..n)
        .rev()
        .filter(|&i| nodes[i].parent < 0 && nodes[i].is_group)
        .collect();
    let lane_count = lanes.len().max(1);
    let mut lane_slot = vec![-1i32; n];
    for (k, &id) in lanes.iter().enumerate() {
        lane_slot[id] = k as i32;
    }
    let mut has_child = vec![false; n];
    for i in 0..n {
        if nodes[i].parent >= 0 {
            has_child[nodes[i].parent as usize] = true;
        }
    }
    // レーンが中身のまわりに取る余白。中に描くグループがレーンの中に収まるように。
    let mut lane_pads = vec![lane_pad; lane_count];
    let mut end_pad = lane_pad;

    // 順位を付けて置くもの: ノードと、空なのでノードとして振る舞うグループ。
    let mut item_of = vec![-1i32; n];
    let mut real: Vec<usize> = Vec::new();
    for i in 0..n {
        if lane_slot[i] >= 0 {
            continue;
        }
        if !nodes[i].is_group || !has_child[i] {
            item_of[i] = real.len() as i32;
            real.push(i);
        }
    }
    let along = |c: &CNode| if sideways { c.w } else { c.h };
    let cross = |c: &CNode| if sideways { c.h } else { c.w };

    let mut lnodes: Vec<LNode> = real
        .iter()
        .map(|&i| lnode(cross(&nodes[i]), along(&nodes[i]), Kind::Node))
        .collect();
    let mut ledges: Vec<LEdge> = Vec::new();
    let mut edge_of: Vec<usize> = Vec::new();
    let mut step = 1;
    {
        let mut seen: HashSet<usize> = HashSet::new();
        for (ei, e) in edges.iter().enumerate() {
            let a = item_of[e.src];
            let b = item_of[e.dst];
            if a < 0 || b < 0 || a == b {
                continue;
            }
            let (a, b) = (a as usize, b as usize);
            let key = a.min(b) * real.len() + a.max(b);
            if e.label_w > 0.0 || seen.contains(&key) {
                step = 2;
            }
            seen.insert(key);
            ledges.push(ledge(
                a,
                b,
                e.minlen,
                if sideways { e.label_h } else { e.label_w },
                if sideways { e.label_w } else { e.label_h },
            ));
            edge_of.push(ei);
        }
    }
    orient(&lnodes, &mut ledges);
    rank(&mut lnodes, &ledges, step);

    let m = ledges.len();
    let item_lane: Vec<i32> = real
        .iter()
        .map(|&i| {
            let mut t = i;
            let mut room = lane_pad;
            let mut hops = 0;
            while nodes[t].parent >= 0 && hops < MAX_HOPS {
                t = nodes[t].parent as usize;
                if lane_slot[t] < 0 {
                    room += js::max(
                        js::max(nodes[t].pad_x, nodes[t].pad_top),
                        nodes[t].pad_bottom,
                    );
                }
                hops += 1;
            }
            let lane = lane_slot[t].max(0) as usize;
            if room > lane_pads[lane] {
                lane_pads[lane] = room;
            }
            if room > end_pad {
                end_pad = room;
            }
            lane as i32
        })
        .collect();

    let ctx = Ctx {
        n,
        sideways,
        flipped,
        opt,
        lanes: &lanes,
        lane_count,
        lane_slot: &lane_slot,
        item_of: &item_of,
        real: &real,
        lnodes: &lnodes,
        ledges: &ledges,
        edge_of: &edge_of,
        step,
        item_lane: &item_lane,
        lane_pads: &lane_pads,
        end_pad,
    };

    let mut plain = vec![0u8; m];
    let mut margins = vec![0u8; lane_count];
    let mut placed = place(&ctx, &plain, &margins, nodes, edges);
    // 単純な道を得た受け渡しは、通るランクに場所を取らなくてよいので、それを除いてレーンを配置し直す。そのとき
    // 道を失った受け渡しは、ランクの道に戻る。道を引くのが仕事の大半なので、大きなグラフは 1 度だけ配置する。
    let mut round = 0;
    while round < ROUNDS && placed.work <= ROUTE_BUDGET / RELAY {
        let next: Vec<u8> = if round == 0 {
            placed.routed.clone()
        } else {
            plain
                .iter()
                .zip(&placed.routed)
                .map(|(&on, &r)| on & r)
                .collect()
        };
        if next == plain && placed.margins == margins {
            break;
        }
        plain = next;
        margins = std::mem::take(&mut placed.margins);
        placed = place(&ctx, &plain, &margins, nodes, edges);
        round += 1;
    }
    if plain
        .iter()
        .zip(&placed.routed)
        .any(|(&on, &r)| on != 0 && r == 0)
    {
        placed = place(&ctx, &vec![0u8; m], &vec![0u8; lane_count], nodes, edges);
    }
    CompoundResult {
        width: placed.width,
        height: placed.height,
    }
}

/// 順位を付けたものを置き、つながりの道を引く。単純なつながりは、自分の単純な道を通る受け渡しで、通るランクに
/// 場所を取らない。
fn place(
    ctx: &Ctx,
    plain: &[u8],
    margins: &[u8],
    nodes: &mut [CNode],
    edges: &mut [CEdge],
) -> Placed {
    let Ctx {
        n,
        sideways,
        flipped,
        opt,
        lanes,
        lane_count,
        lane_slot,
        item_of,
        real,
        lnodes,
        ledges,
        edge_of,
        step,
        item_lane,
        lane_pads,
        end_pad,
    } = *ctx;
    let m = ledges.len();
    let pad: Vec<f64> = lane_pads
        .iter()
        .enumerate()
        .map(|(lane, &room)| {
            if margins[lane] != 0 {
                js::max(room, MARGIN)
            } else {
                room
            }
        })
        .collect();
    let mut used = vec![0u8; lane_count];
    // 置くもの: 本物のもの、それから辺が通るランクごとの仮のもの。
    let mut dummies: i64 = 0;
    let mut span = vec![0i32; m];
    for k in 0..m {
        span[k] = (lnodes[ledges[k].head].rank - lnodes[ledges[k].tail].rank).abs();
        if plain[k] == 0 {
            dummies += span[k] as i64 - 1;
        }
    }
    let direct = dummies > MAX_DUMMIES;
    // 辺の長さが 0 のランクはない（辺はどれも 1 ランク以上離れる）ので、仮のものの数は負にならない。
    let total = real.len() + if direct { 0 } else { dummies.max(0) as usize };
    let mut rank_ = vec![0i32; total];
    let mut lane_ = vec![0i32; total];
    let mut size = vec![0f64; total];
    let mut depth = vec![0f64; total];
    let mut dummy = vec![0u8; total];
    let mut max_rank = 0i32;
    for k in 0..real.len() {
        rank_[k] = lnodes[k].rank;
        lane_[k] = item_lane[k];
        size[k] = lnodes[k].w;
        depth[k] = lnodes[k].h;
        if rank_[k] > max_rank {
            max_rank = rank_[k];
        }
    }
    let mut first_dummy = vec![-1i64; m];
    let mut label_item = vec![-1i64; m];
    let mut links: Vec<usize> = Vec::new();
    {
        let mut next = real.len();
        for k in 0..m {
            let e = &ledges[k];
            let a = if e.reversed { e.head } else { e.tail };
            let b = if e.reversed { e.tail } else { e.head };
            if direct || span[k] < 2 || plain[k] != 0 {
                links.push(a);
                links.push(b);
                continue;
            }
            let ra = rank_[a];
            let rb = rank_[b];
            let mut label_rank = -1;
            if e.label_w > 0.0 {
                label_rank = ra + ((rb - ra) >> 1);
                if step == 2 && (label_rank & 1) == 0 {
                    label_rank -= 1;
                }
            }
            first_dummy[k] = next as i64;
            let mut prev = a;
            for r in ra + 1..rb {
                let d = next;
                next += 1;
                rank_[d] = r;
                // 辺は最初のレーンから最後のレーンへ、道のりに沿って均等に移る。
                lane_[d] = lane_[a]
                    + js::round(((lane_[b] - lane_[a]) as f64 * (r - ra) as f64) / (rb - ra) as f64)
                        as i32;
                dummy[d] = 1;
                if r == label_rank {
                    size[d] = e.label_w;
                    depth[d] = e.label_h;
                    label_item[k] = d as i64;
                }
                links.push(prev);
                links.push(d);
                prev = d;
            }
            links.push(prev);
            links.push(b);
        }
    }

    let mut up_start = vec![0usize; total + 1];
    let mut down_start = vec![0usize; total + 1];
    for k in (0..links.len()).step_by(2) {
        down_start[links[k] + 1] += 1;
        up_start[links[k + 1] + 1] += 1;
    }
    for i in 0..total {
        up_start[i + 1] += up_start[i];
        down_start[i + 1] += down_start[i];
    }
    let mut up = vec![0usize; links.len() / 2];
    let mut down = vec![0usize; links.len() / 2];
    {
        let mut uf = up_start[..total].to_vec();
        let mut df = down_start[..total].to_vec();
        for k in (0..links.len()).step_by(2) {
            down[df[links[k]]] = links[k + 1];
            df[links[k]] += 1;
            up[uf[links[k + 1]]] = links[k];
            uf[links[k + 1]] += 1;
        }
    }

    // ランクの中の順: まずレーン、次に隣が並ぶ位置。
    let mut rows: Vec<Vec<usize>> = (0..=max_rank).map(|_| Vec::new()).collect();
    for i in 0..total {
        rows[rank_[i] as usize].push(i);
    }
    let mut pos = vec![0f64; total];
    let mut key = vec![0f64; total];
    let settle = |row: &mut Vec<usize>, key: &[f64], pos: &mut [f64]| {
        sort_by(row, |a, b| {
            or(
                or((lane_[a] - lane_[b]) as f64, key[a] - key[b]),
                a as f64 - b as f64,
            )
        });
        for (k, &i) in row.iter().enumerate() {
            pos[i] = k as f64;
        }
    };
    for row in rows.iter_mut() {
        for (k, &i) in row.iter().enumerate() {
            key[i] = k as f64;
        }
        settle(row, &key, &mut pos);
    }
    let mr = max_rank as usize;
    let mut sweep = 0;
    while sweep < SWEEPS && max_rank > 0 {
        let downward = sweep & 1 == 0;
        for s in 1..=mr {
            let r = if downward { s } else { mr - s };
            let (start, list) = if downward {
                (&up_start, &up)
            } else {
                (&down_start, &down)
            };
            for &i in &rows[r] {
                let mut sum = 0.0;
                let count = start[i + 1] - start[i];
                for k in start[i]..start[i + 1] {
                    sum += pos[list[k]];
                }
                key[i] = if count > 0 {
                    sum / count as f64
                } else {
                    pos[i]
                };
            }
            settle(&mut rows[r], &key, &mut pos);
        }
        sweep += 1;
    }

    // あるランクの、同じレーンのものは横に並んで 1 つの並び（run）になる。
    let gap = |a: usize, b: usize| {
        if dummy[a] != 0 || dummy[b] != 0 {
            opt.edge_sep
        } else {
            opt.node_sep
        }
    };
    let runs = |row: &[usize]| -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut from = 0;
        while from < row.len() {
            let mut to = from;
            while to + 1 < row.len() && lane_[row[to + 1]] == lane_[row[from]] {
                to += 1;
            }
            out.push((from, to));
            from = to + 1;
        }
        out
    };
    let run_size = |row: &[usize], from: usize, to: usize| {
        let mut s = size[row[from]];
        for k in from..to {
            s += gap(row[k], row[k + 1]) + size[row[k + 1]];
        }
        s
    };

    // レーンの幅: いちばん詰まったランクと題が収まるだけ。
    let mut breadth = vec![0f64; lane_count];
    let mut head = if lanes.is_empty() { 0.0 } else { LANE_HEAD };
    for k in 0..lanes.len() {
        let lane = &nodes[lanes[k]];
        breadth[k] = js::max(0.0, lane.min_w - 2.0 * pad[k]);
        if lane.pad_top > head {
            head = lane.pad_top;
        }
    }
    for row in &rows {
        for (from, to) in runs(row) {
            let s = run_size(row, from, to);
            let l = lane_[row[from]] as usize;
            if s > breadth[l] {
                breadth[l] = s;
            }
        }
    }
    let mut lane_start = vec![0f64; lane_count + 1];
    for k in 0..lane_count {
        lane_start[k + 1] = lane_start[k] + breadth[k] + 2.0 * pad[k];
    }

    // 流れを横切る向き: 各レーンのものは真ん中から始め、レーンと隣のものが許すかぎり、同じレーンの隣にそろえる。
    let mut c_ = vec![0f64; total];
    let mut want = vec![0f64; total];
    for row in &rows {
        for (from, to) in runs(row) {
            let lane = lane_[row[from]] as usize;
            let mut at = (lane_start[lane] + pad[lane] + (lane_start[lane + 1] - pad[lane])
                - run_size(row, from, to))
                / 2.0;
            for k in from..=to {
                c_[row[k]] = at + size[row[k]] / 2.0;
                if k < to {
                    at += size[row[k]] + gap(row[k], row[k + 1]);
                }
            }
        }
    }
    let mut pass = 0;
    while pass < 4 && max_rank > 0 {
        let downward = pass & 1 == 0;
        for s in 1..=mr {
            let row = &rows[if downward { s } else { mr - s }];
            let (start, list) = if downward {
                (&up_start, &up)
            } else {
                (&down_start, &down)
            };
            for &i in row {
                let mut sum = 0.0;
                let mut count = 0;
                for k in start[i]..start[i + 1] {
                    if lane_[list[k]] != lane_[i] {
                        continue;
                    }
                    sum += c_[list[k]];
                    count += 1;
                }
                want[i] = if count > 0 { sum / count as f64 } else { c_[i] };
            }
            for (from, to) in runs(row) {
                let lane = lane_[row[from]] as usize;
                let mut floor = lane_start[lane] + pad[lane];
                for k in from..=to {
                    let i = row[k];
                    c_[i] = js::max(want[i], floor + size[i] / 2.0);
                    if k < to {
                        floor = c_[i] + size[i] / 2.0 + gap(i, row[k + 1]);
                    }
                }
                let mut ceil = lane_start[lane + 1] - pad[lane];
                for k in (from..=to).rev() {
                    let i = row[k];
                    c_[i] = js::min(c_[i], ceil - size[i] / 2.0);
                    if k > from {
                        ceil = c_[i] - size[i] / 2.0 - gap(row[k - 1], i);
                    }
                }
            }
        }
        pass += 1;
    }

    // 流れに沿う向き: レーンの題の後に、ランクごとの帯。
    let mut band_start = vec![0f64; mr + 1];
    let mut band_end = vec![0f64; mr + 1];
    // ランクの後の隙間は、そこで曲がるつながりや、求められた長さしかないつながりのため。単純な受け渡しとまっすぐの
    // 線だけが通る所では、間の受け渡しのラベルが載る線を残すかぎり、ランクを詰める。
    let step_f = step as f64;
    let mut sep = vec![opt.rank_sep / step_f; mr + 1];
    if plain.contains(&1) {
        let full = opt.rank_sep / step_f;
        sep.fill(full / CLOSE);
        for k in 0..m {
            let e = &ledges[k];
            let a = if e.reversed { e.head } else { e.tail };
            let b = if e.reversed { e.tail } else { e.head };
            if plain[k] != 0 {
                if e.label_h <= 0.0 || span[k] > step {
                    continue;
                }
                let room = js::min(
                    full,
                    (e.label_h + 2.0 * TURN - js::min(depth[a], depth[b]) / 2.0) / span[k] as f64,
                );
                for r in rank_[a]..rank_[b] {
                    if room > sep[r as usize] {
                        sep[r as usize] = room;
                    }
                }
                continue;
            }
            let d0 = first_dummy[k];
            if d0 < 0 || span[k] as f64 <= step_f * e.minlen {
                for r in rank_[a]..rank_[b] {
                    sep[r as usize] = full;
                }
                continue;
            }
            let last = d0 + span[k] as i64 - 1;
            for d in d0 - 1..last {
                let u = if d < d0 { a } else { d as usize };
                let v = if d + 1 < last { (d + 1) as usize } else { b };
                if lane_[u] != lane_[v] || (c_[u] - c_[v]).abs() >= 0.5 {
                    sep[rank_[u] as usize] = full;
                }
            }
        }
    }
    let mut at = head + end_pad;
    for r in 0..=mr {
        let mut d = 0.0;
        for &i in &rows[r] {
            if depth[i] > d {
                d = depth[i];
            }
        }
        band_start[r] = at;
        band_end[r] = at + d;
        at += d + if r == mr { 0.0 } else { sep[r] };
    }
    let length = at + end_pad;
    let breadth_total = lane_start[lane_count];
    let flip = |a: f64| if flipped { head + length - a } else { a };
    let px = |c: f64, a: f64| if sideways { flip(a) } else { c };
    let py = |c: f64, a: f64| if sideways { c } else { flip(a) };
    let axis = if sideways { 1.0 } else { 0.0 };

    for k in 0..real.len() {
        let node = &mut nodes[real[k]];
        let a = (band_start[rank_[k] as usize] + band_end[rank_[k] as usize]) / 2.0;
        node.x = px(c_[k], a);
        node.y = py(c_[k], a);
    }
    for k in 0..lanes.len() {
        let lane = &mut nodes[lanes[k]];
        let c = (lane_start[k] + lane_start[k + 1]) / 2.0;
        let s = lane_start[k + 1] - lane_start[k];
        lane.x = if sideways { length / 2.0 } else { c };
        lane.y = if sideways { c } else { length / 2.0 };
        lane.w = if sideways { length } else { s };
        lane.h = if sideways { s } else { length };
        lane.pad_top = head;
    }

    // レーンの中のグループは、深いものから、その中身のまわりに描く。
    let mut depth_of = vec![0i32; n];
    let mut inner: Vec<usize> = Vec::new();
    for i in 0..n {
        if !nodes[i].is_group || lane_slot[i] >= 0 || item_of[i] >= 0 {
            continue;
        }
        let mut d = 0;
        let mut p = nodes[i].parent;
        while p >= 0 && (d as usize) < MAX_HOPS {
            d += 1;
            p = nodes[p as usize].parent;
        }
        depth_of[i] = d;
        inner.push(i);
    }
    sort_by(&mut inner, |a, b| {
        or((depth_of[b] - depth_of[a]) as f64, a as f64 - b as f64)
    });
    let mut bx = vec![0f64; n * 4];
    for i in 0..n {
        bx[i * 4] = f64::INFINITY;
        bx[i * 4 + 1] = f64::INFINITY;
        bx[i * 4 + 2] = f64::NEG_INFINITY;
        bx[i * 4 + 3] = f64::NEG_INFINITY;
    }
    let grow = |bx: &mut [f64], g: usize, c: &CNode| {
        if c.x - c.w / 2.0 < bx[g * 4] {
            bx[g * 4] = c.x - c.w / 2.0;
        }
        if c.y - c.h / 2.0 < bx[g * 4 + 1] {
            bx[g * 4 + 1] = c.y - c.h / 2.0;
        }
        if c.x + c.w / 2.0 > bx[g * 4 + 2] {
            bx[g * 4 + 2] = c.x + c.w / 2.0;
        }
        if c.y + c.h / 2.0 > bx[g * 4 + 3] {
            bx[g * 4 + 3] = c.y + c.h / 2.0;
        }
    };
    for &i in real {
        let p = nodes[i].parent;
        if p >= 0 && lane_slot[p as usize] < 0 {
            grow(&mut bx, p as usize, &nodes[i]);
        }
    }
    for &g in &inner {
        if bx[g * 4] == f64::INFINITY {
            continue;
        }
        let c = &mut nodes[g];
        let w = js::max(bx[g * 4 + 2] - bx[g * 4] + 2.0 * c.pad_x, c.min_w);
        c.x = (bx[g * 4] + bx[g * 4 + 2]) / 2.0;
        c.w = w;
        c.y = (bx[g * 4 + 1] - c.pad_top + bx[g * 4 + 3] + c.pad_bottom) / 2.0;
        c.h = bx[g * 4 + 3] - bx[g * 4 + 1] + c.pad_top + c.pad_bottom;
        let p = c.parent;
        if p >= 0 && lane_slot[p as usize] < 0 {
            let c = nodes[g].clone();
            grow(&mut bx, p as usize, &c);
        }
    }

    for e in edges.iter_mut() {
        e.route = Vec::new();
    }
    for k in 0..m {
        let e = &ledges[k];
        let out = &mut edges[edge_of[k]];
        let a = if e.reversed { e.head } else { e.tail };
        let b = if e.reversed { e.tail } else { e.head };
        let (ra, rb) = (rank_[a] as usize, rank_[b] as usize);
        let mut pts: Vec<f64> = vec![
            c_[a],
            band_end[ra] - (band_end[ra] - band_start[ra] - depth[a]) / 2.0,
        ];
        let d0 = first_dummy[k];
        if d0 >= 0 {
            for d in d0 as usize..(d0 + span[k] as i64 - 1) as usize {
                let rd = rank_[d] as usize;
                pts.push(c_[d]);
                pts.push(band_start[rd]);
                if band_end[rd] > band_start[rd] {
                    pts.push(c_[d]);
                    pts.push(band_end[rd]);
                }
            }
        }
        pts.push(c_[b]);
        pts.push(band_start[rb] + (band_end[rb] - band_start[rb] - depth[b]) / 2.0);
        let mut route: Vec<f64> = Vec::with_capacity(pts.len() / 2 * 3);
        for p in (0..pts.len()).step_by(2) {
            route.push(px(pts[p], pts[p + 1]));
            route.push(py(pts[p], pts[p + 1]));
            route.push(axis);
        }
        if e.reversed {
            let (mut i, mut j) = (0, route.len() - 3);
            while i < j {
                for t in 0..2 {
                    route.swap(i + t, j + t);
                }
                i += 3;
                j -= 3;
            }
        }
        let label = label_item[k];
        if label >= 0 {
            let lr = rank_[label as usize] as usize;
            let mid = (band_start[lr] + band_end[lr]) / 2.0;
            out.label_x = px(c_[label as usize], mid);
            out.label_y = py(c_[label as usize], mid);
        } else {
            out.label_x = (route[0] + route[route.len() - 3]) / 2.0;
            out.label_y = (route[1] + route[route.len() - 2]) / 2.0;
        }
        out.route = route;
    }

    // レーンやグループに始まるか終わる辺は、その縁までまっすぐ走る。
    for ei in 0..edges.len() {
        let e = &edges[ei];
        if !e.route.is_empty() || e.src == e.dst {
            continue;
        }
        let (ax, ay) = border(&nodes[e.src], &nodes[e.dst]);
        let (bx_, by) = border(&nodes[e.dst], &nodes[e.src]);
        let flat = if (bx_ - ax).abs() > (by - ay).abs() {
            1.0
        } else {
            0.0
        };
        let e = &mut edges[ei];
        e.route = vec![ax, ay, flat, bx_, by, flat];
        e.label_x = (ax + bx_) / 2.0;
        e.label_y = (ay + by) / 2.0;
    }

    // 受け渡しは、ほかのレーンへ向かって出てから流れに沿って着く（または逆）ことができる。とくに、分かれ目から
    // 戻る枝は、入ってきた辺から出て前の段の脇をなぞってはならない。角が 1 つの 2 つの道を試し、ノードやほかの
    // ラベルがふさぐなら、ランクの道のままにする。
    let mut routed = vec![0u8; m];
    let route_points: usize = edges.iter().map(|e| e.route.len() / 3).sum();
    let work = m as f64 * (real.len() + edges.len() + route_points) as f64;
    if work <= ROUTE_BUDGET {
        let amid = |i: usize| (band_start[rank_[i] as usize] + band_end[rank_[i] as usize]) / 2.0;
        let mut labels: Vec<[f64; 2]> = edges
            .iter()
            .map(|e| {
                [
                    if sideways { e.label_y } else { e.label_x },
                    flip(if sideways { e.label_x } else { e.label_y }),
                ]
            })
            .collect();
        let overlaps = |c0: f64, a0: f64, c1: f64, a1: f64, lc: f64, la: f64, cw: f64, ah: f64| {
            c1 > lc - cw / 2.0 && c0 < lc + cw / 2.0 && a1 > la - ah / 2.0 && a0 < la + ah / 2.0
        };
        let clear = |edges: &[CEdge],
                     labels: &[[f64; 2]],
                     c0: f64,
                     a0: f64,
                     c1: f64,
                     a1: f64,
                     src: i64,
                     dst: i64,
                     ei: usize|
         -> bool {
            for i in 0..real.len() {
                if i as i64 != src
                    && i as i64 != dst
                    && overlaps(
                        c0,
                        a0,
                        c1,
                        a1,
                        c_[i],
                        amid(i),
                        size[i] + 8.0,
                        depth[i] + 8.0,
                    )
                {
                    return false;
                }
            }
            for (i, e) in edges.iter().enumerate() {
                // 段が自分に戻る輪は、ラベルを段の箱の中に置くので、ここには関わらない。
                if i != ei
                    && e.label_w > 0.0
                    && e.src != e.dst
                    && overlaps(
                        c0,
                        a0,
                        c1,
                        a1,
                        labels[i][0],
                        labels[i][1],
                        if sideways { e.label_h } else { e.label_w },
                        if sideways { e.label_w } else { e.label_h },
                    )
                {
                    return false;
                }
            }
            true
        };
        // 箱の中にひと区切りを持つ、ほかのつながりの数。区切りの箱全体（曲がりを含む）を数える。
        let crossings = |edges: &[CEdge], x0: f64, y0: f64, x1: f64, y1: f64, ei: usize| -> usize {
            let mut count = 0;
            for (i, e) in edges.iter().enumerate() {
                if i == ei {
                    continue;
                }
                let route = &e.route;
                let mut j = 3;
                while j < route.len() {
                    if js::max(route[j - 3], route[j]) > x0
                        && js::min(route[j - 3], route[j]) < x1
                        && js::max(route[j - 2], route[j + 1]) > y0
                        && js::min(route[j - 2], route[j + 1]) < y1
                    {
                        count += 1;
                        break;
                    }
                    j += 3;
                }
            }
            count
        };
        let meets = |edges: &[CEdge], x: f64, y: f64, ei: usize| -> usize {
            let mut count = 0;
            for (i, e) in edges.iter().enumerate() {
                let route = &e.route;
                if i == ei || route.is_empty() {
                    continue;
                }
                let last = route.len() - 3;
                if ((route[0] - x).abs() < 1.0 && (route[1] - y).abs() < 1.0)
                    || ((route[last] - x).abs() < 1.0 && (route[last + 1] - y).abs() < 1.0)
                {
                    count += 1;
                }
            }
            count
        };
        // 1 本の線に沿って引いた 2 つのつながりは、1 本の道に読める。同じ段から出るものどうしと、同じ段に着く
        // ものどうしは幹を分け合ってよいが、ほかの組はいけない。
        let retraces = |edges: &[CEdge], x0: f64, y0: f64, x1: f64, y1: f64, ei: usize| -> bool {
            let upright = (x1 - x0).abs() < 0.5;
            let lo = if upright {
                js::min(y0, y1)
            } else {
                js::min(x0, x1)
            };
            let hi = if upright {
                js::max(y0, y1)
            } else {
                js::max(x0, x1)
            };
            let line = if upright { x0 } else { y0 };
            let (s, t) = if upright { (0, 1) } else { (1, 0) };
            for (i, other) in edges.iter().enumerate() {
                if i == ei || other.src == edges[ei].src || other.dst == edges[ei].dst {
                    continue;
                }
                let route = &other.route;
                let mut j = 3;
                while j < route.len() {
                    let u0 = route[j - 3 + s];
                    let u1 = route[j + s];
                    if (u0 - line).abs() >= NEAR || (u1 - line).abs() >= NEAR {
                        j += 3;
                        continue;
                    }
                    let v0 = route[j - 3 + t];
                    let v1 = route[j + t];
                    if js::min(hi, js::max(v0, v1)) - js::max(lo, js::min(v0, v1)) > 1.0 {
                        return true;
                    }
                    j += 3;
                }
            }
            false
        };
        for k in 0..m {
            let e = &ledges[k];
            let ei = edge_of[k];
            let a = e.tail;
            let b = e.head;
            if lane_[a] == lane_[b] || (c_[b] - c_[a]).abs() < 1.0 {
                continue;
            }
            let (ac, bc, aa, ba) = (c_[a], c_[b], amid(a), amid(b));
            let dc = js::sign(bc - ac);
            let da = js::sign(ba - aa);
            let attempt =
                |edges: &mut [CEdge], labels: &mut [[f64; 2]], pts: &[f64], margin: f64| -> bool {
                    for p in (2..pts.len()).step_by(2) {
                        if !clear(
                            edges,
                            labels,
                            js::min(pts[p - 2], pts[p]) - margin,
                            js::min(pts[p - 1], pts[p + 1]) - margin,
                            js::max(pts[p - 2], pts[p]) + margin,
                            js::max(pts[p - 1], pts[p + 1]) + margin,
                            a as i64,
                            b as i64,
                            ei,
                        ) {
                            return false;
                        }
                        if retraces(
                            edges,
                            px(pts[p - 2], pts[p - 1]),
                            py(pts[p - 2], pts[p - 1]),
                            px(pts[p], pts[p + 1]),
                            py(pts[p], pts[p + 1]),
                            ei,
                        ) {
                            return false;
                        }
                    }
                    let (mut lc, mut la, mut longest) = (0.0, 0.0, -1.0);
                    let (out_w, out_h) = (edges[ei].label_w, edges[ei].label_h);
                    if out_w > 0.0 {
                        let (cw, ah) = (e.label_w, e.label_h);
                        for p in (2..pts.len()).step_by(2) {
                            let cc = (pts[p - 2] + pts[p]) / 2.0;
                            let ca = (pts[p - 1] + pts[p + 1]) / 2.0;
                            let across = (pts[p] - pts[p - 2]).abs();
                            let along = (pts[p + 1] - pts[p - 1]).abs();
                            let run = across + along;
                            if run <= longest
                                || (if across > 0.0 {
                                    across < cw + 8.0
                                } else {
                                    along < ah + 8.0
                                })
                            {
                                continue;
                            }
                            if cc - cw / 2.0 < 0.0
                                || cc + cw / 2.0 > breadth_total
                                || ca - ah / 2.0 < 0.0
                                || ca + ah / 2.0 > length
                            {
                                continue;
                            }
                            // ここでは両端も数える: ラベルはつなぐ 2 つの形のどちらにもかからない。
                            if !clear(
                                edges,
                                labels,
                                cc - cw / 2.0,
                                ca - ah / 2.0,
                                cc + cw / 2.0,
                                ca + ah / 2.0,
                                -1,
                                -1,
                                ei,
                            ) {
                                continue;
                            }
                            // 受け渡しに移したラベルは、ランクの道を保つ戻りの道を覆ってはならない。区切りの箱全体
                            // （曲がりを含む）を空けておく。
                            let lx = px(cc, ca);
                            let ly = py(cc, ca);
                            if crossings(
                                edges,
                                lx - out_w / 2.0,
                                ly - out_h / 2.0,
                                lx + out_w / 2.0,
                                ly + out_h / 2.0,
                                ei,
                            ) > 0
                            {
                                continue;
                            }
                            lc = cc;
                            la = ca;
                            longest = run;
                        }
                        if longest < 0.0 {
                            return false;
                        }
                    }
                    // 角の手前の点と過ぎた点で、曲がりを角の近くに保つ。線はラベルの下にとどまり、受け渡しは場所を
                    // あまり取らない。
                    let mut way = vec![pts[0], pts[1]];
                    for p in (2..pts.len()).step_by(2) {
                        let dx = pts[p] - pts[p - 2];
                        let dy = pts[p + 1] - pts[p - 1];
                        let run = dx.abs() + dy.abs();
                        let first = if p > 2 { TURN } else { 0.0 };
                        let last = if p + 2 < pts.len() { TURN } else { 0.0 };
                        if run > first + last + 1.0 {
                            if first != 0.0 {
                                way.push(pts[p - 2] + (dx / run) * first);
                                way.push(pts[p - 1] + (dy / run) * first);
                            }
                            if last != 0.0 {
                                way.push(pts[p] - (dx / run) * last);
                                way.push(pts[p + 1] - (dy / run) * last);
                            }
                        }
                        way.push(pts[p]);
                        way.push(pts[p + 1]);
                    }
                    let mut route = Vec::with_capacity(way.len() / 2 * 3);
                    for p in (0..way.len()).step_by(2) {
                        let crosswise = if p == 0 {
                            way[0] != way[2]
                        } else {
                            way[p] != way[p - 2]
                        };
                        route.push(px(way[p], way[p + 1]));
                        route.push(py(way[p], way[p + 1]));
                        route.push(if crosswise { 1.0 - axis } else { axis });
                    }
                    let out = &mut edges[ei];
                    out.route = route;
                    if out.label_w > 0.0 {
                        out.label_x = px(lc, la);
                        out.label_y = py(lc, la);
                        labels[ei] = [lc, la];
                    }
                    true
                };
            if attempt(
                edges,
                &mut labels,
                &[
                    ac + dc * size[a] / 2.0,
                    aa,
                    bc,
                    aa,
                    bc,
                    ba - da * depth[b] / 2.0,
                ],
                2.0,
            ) || attempt(
                edges,
                &mut labels,
                &[
                    ac,
                    aa + da * depth[a] / 2.0,
                    ac,
                    ba,
                    bc - dc * size[b] / 2.0,
                    ba,
                ],
                2.0,
            ) {
                routed[k] = 1;
                continue;
            }
            // 2 つの角がふさがれていれば（間の段を越えて戻るときなど）、受け渡しはまわり道をする: レーンの縁へ
            // 出て、それに沿い、着く所の高さで戻る。ほかのつながりと交わるのが最も少なく、端を分け合うのが最も
            // 少ない縁を取り、次に短い道を取る。
            if work > ROUTE_BUDGET / RELAY {
                continue;
            }
            let mut around: Vec<(Vec<f64>, f64, usize)> = Vec::new();
            let order = if lane_[a] < lane_[b] {
                [lane_[b], lane_[a]]
            } else {
                [lane_[a], lane_[b]]
            };
            for lane in order {
                let lane = lane as usize;
                // まだ縁を取っていないレーンはそこの場所が狭いので、受け渡しはその中にとどまる。
                let inset = js::max(pad[lane] - LEG, js::min(pad[lane] / 2.0, MARGIN - LEG));
                for ch in [lane_start[lane] + inset, lane_start[lane + 1] - inset] {
                    if (ch - ac).abs() < size[a] / 2.0 + 4.0
                        || (ch - bc).abs() < size[b] / 2.0 + 4.0
                    {
                        continue;
                    }
                    let pts = vec![
                        ac + js::sign(ch - ac) * size[a] / 2.0,
                        aa,
                        ch,
                        aa,
                        ch,
                        ba,
                        bc + js::sign(ch - bc) * size[b] / 2.0,
                        ba,
                    ];
                    let mut cost = (meets(edges, px(pts[0], pts[1]), py(pts[0], pts[1]), ei)
                        + meets(edges, px(pts[6], pts[7]), py(pts[6], pts[7]), ei))
                        as f64
                        * 1e6;
                    for p in (2..pts.len()).step_by(2) {
                        let x0 = px(pts[p - 2], pts[p - 1]);
                        let y0 = py(pts[p - 2], pts[p - 1]);
                        let x1 = px(pts[p], pts[p + 1]);
                        let y1 = py(pts[p], pts[p + 1]);
                        cost += crossings(
                            edges,
                            js::min(x0, x1),
                            js::min(y0, y1),
                            js::max(x0, x1),
                            js::max(y0, y1),
                            ei,
                        ) as f64
                            * 1e6
                            + (x1 - x0).abs()
                            + (y1 - y0).abs();
                    }
                    around.push((pts, cost, lane));
                }
            }
            let mut order: Vec<usize> = (0..around.len()).collect();
            sort_by(&mut order, |p, q| around[p].1 - around[q].1);
            for w in order {
                if !attempt(edges, &mut labels, &around[w].0, 1.0) {
                    continue;
                }
                routed[k] = 1;
                used[around[w].2] = 1;
                break;
            }
        }
    }

    Placed {
        width: if sideways { length } else { breadth_total },
        height: if sideways { breadth_total } else { length },
        routed,
        margins: used,
        work,
    }
}

/// ある箱の真ん中からほかの箱の真ん中への線が、はじめの箱を出る点。ほかの真ん中が箱の中にあれば、いちばん近い
/// 辺の上の点。
fn border(from: &CNode, to: &CNode) -> (f64, f64) {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let hw = js::max(from.w / 2.0, 0.5);
    let hh = js::max(from.h / 2.0, 0.5);
    if dx.abs() < hw && dy.abs() < hh {
        let to_side = hw - dx.abs();
        let to_end = hh - dy.abs();
        return if to_side < to_end {
            (from.x + if dx < 0.0 { -hw } else { hw }, to.y)
        } else {
            (to.x, from.y + if dy < 0.0 { -hh } else { hh })
        };
    }
    let scale = js::min(
        if dx != 0.0 {
            hw / dx.abs()
        } else {
            f64::INFINITY
        },
        if dy != 0.0 {
            hh / dy.abs()
        } else {
            f64::INFINITY
        },
    );
    (from.x + dx * scale, from.y + dy * scale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::compound::cnode;

    fn options() -> LayeredOptions {
        LayeredOptions {
            node_sep: 40.0,
            edge_sep: 16.0,
            rank_sep: 48.0,
            port_sep: 20.0,
            ..Default::default()
        }
    }

    fn edge(src: usize, dst: usize, label_w: f64, label_h: f64) -> CEdge {
        CEdge {
            src,
            dst,
            minlen: 1.0,
            label_w,
            label_h,
            route: Vec::new(),
            label_x: 0.0,
            label_y: 0.0,
        }
    }

    fn group(parent: i64) -> CNode {
        let mut node = cnode(0.0, 0.0, parent);
        node.is_group = true;
        node.pad_x = 20.0;
        node.pad_top = 36.0;
        node.pad_bottom = 20.0;
        node.min_w = 60.0;
        node
    }

    #[test]
    fn lays_out_nothing() {
        let r = lane_layout(&mut [], &mut [], Dir::TB, &options(), LANE_PAD);
        assert_eq!((r.width, r.height), (0.0, 0.0));
    }

    #[test]
    fn puts_lanes_side_by_side_in_the_order_written() {
        for dir in [Dir::TB, Dir::BT, Dir::LR, Dir::RL] {
            let mut nodes = vec![
                cnode(60.0, 30.0, 3),
                cnode(60.0, 30.0, 2),
                group(-1),
                group(-1),
            ];
            let result = lane_layout(
                &mut nodes,
                &mut [edge(0, 1, 0.0, 0.0)],
                dir,
                &options(),
                LANE_PAD,
            );
            let (first, second) = (&nodes[3], &nodes[2]);
            if matches!(dir, Dir::TB | Dir::BT) {
                assert!(first.x < second.x);
                assert_eq!(first.h, result.height);
                assert!((first.w + second.w - result.width).abs() < 1e-6);
            } else {
                assert!(first.y < second.y);
                assert_eq!(first.w, result.width);
                assert!((first.h + second.h - result.height).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn keeps_a_chain_in_one_lane_on_a_straight_line() {
        let mut nodes = vec![
            cnode(50.0, 30.0, 3),
            cnode(120.0, 30.0, 3),
            cnode(70.0, 30.0, 3),
            group(-1),
        ];
        let mut edges = [edge(0, 1, 0.0, 0.0), edge(1, 2, 0.0, 0.0)];
        lane_layout(&mut nodes, &mut edges, Dir::TB, &options(), LANE_PAD);
        assert!((nodes[0].x - nodes[1].x).abs() < 1e-6);
        assert!((nodes[1].x - nodes[2].x).abs() < 1e-6);
    }
}
