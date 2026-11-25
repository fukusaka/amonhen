//! circo: 二重連結成分（ブロック）ごとに円に置き、ブロックと切断点の木で外へつなぐ（Six, Tollis, "A Framework
//! for Circular Drawings of Networks", 1999）。ブロックの中の順は、深さ優先の順から始め、隣どうしの入れ替えで弦の
//! 交差を減らす。子のブロックは、切断点で親の円の外へ、切断点が子の円の上に来るように置く。木（ブロックがみな
//! 辺 1 本）のときは、次数のいちばん大きいノードを中心にまわりへ広げる。

use super::common::{self, INCH, P};
use super::{Item, Link};
use crate::graph::Attrs;

use std::f64::consts::{PI, TAU};

/// 交差を減らす入れ替えの手間の上限（図ぜんぶ）。
const SWAP_WORK: usize = 20_000_000;

pub fn layout(items: &[Item], links: &[Link], g: &Attrs) -> Vec<P> {
    let n = items.len();
    let mut pos = vec![(0.0, 0.0); n];
    if n == 0 {
        return pos;
    }
    let adj = common::adjacency(n, links, 1.0);
    let half = common::halves(items);
    let mindist = g
        .num("mindist")
        .filter(|v| *v >= 0.0)
        .unwrap_or(1.0)
        .min(20.0)
        * INCH;
    let diam: Vec<f64> = items.iter().map(|it| it.w.max(it.h)).collect();
    let comps = common::components(&adj);
    let blocks = biconnected(&adj);
    let mut of_node: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (b, list) in blocks.iter().enumerate() {
        for &v in list {
            of_node[v].push(b);
        }
    }
    let mut ctx = Ctx {
        adj: &adj,
        blocks: &blocks,
        of_node: &of_node,
        diam: &diam,
        mindist,
        done: vec![false; blocks.len()],
        pos: &mut pos,
        work: SWAP_WORK,
    };
    for comp in &comps {
        if comp.len() < 2 {
            continue;
        }
        let mut in_comp: Vec<usize> = comp.iter().flat_map(|&v| of_node[v].clone()).collect();
        in_comp.sort_unstable();
        in_comp.dedup();
        let largest = in_comp
            .iter()
            .copied()
            .max_by_key(|&b| (blocks[b].len(), std::cmp::Reverse(b)));
        let Some(largest) = largest else { continue };
        if blocks[largest].len() >= 3 {
            let r = ctx.radius(largest);
            ctx.place_block(largest, (0.0, 0.0), None, r);
        } else {
            // 木: 次数のいちばん大きいノードを中心に
            let hub = comp
                .iter()
                .copied()
                .max_by_key(|&v| (adj[v].len(), std::cmp::Reverse(v)))
                .unwrap_or(comp[0]);
            ctx.pos[hub] = (0.0, 0.0);
            ctx.children_at(hub, 0.0, TAU);
        }
    }
    if comps.len() > 1 {
        let movable = vec![true; comps.len()];
        common::pack(&comps, &movable, &mut pos, &half, common::pack_gap(g, 16.0));
    }
    pos
}

/// 置くブロック・中心・親とつながる切断点とその角・半径。
type Placing = (usize, P, Option<(usize, f64)>, f64);

struct Ctx<'a> {
    adj: &'a [Vec<(usize, f64)>],
    blocks: &'a [Vec<usize>],
    of_node: &'a [Vec<usize>],
    diam: &'a [f64],
    mindist: f64,
    done: Vec<bool>,
    pos: &'a mut [P],
    work: usize,
}

impl Ctx<'_> {
    /// ブロックの円の半径。
    fn radius(&self, b: usize) -> f64 {
        let list = &self.blocks[b];
        let m = list.len();
        let big = list.iter().map(|&v| self.diam[v]).fold(0.0, f64::max);
        if m <= 2 {
            return (big + self.mindist) / 2.0;
        }
        let around: f64 = list.iter().map(|&v| self.diam[v] + self.mindist).sum();
        (around / TAU).max((big + self.mindist) / (2.0 * (PI / m as f64).sin()))
    }

    /// ブロック `b` を中心 `c` の円に置く。`attach` は親とつながる切断点と、その点の円の上の角。
    fn place_block(&mut self, b: usize, c: P, attach: Option<(usize, f64)>, r: f64) {
        // 子から親へ戻らないよう、ブロックは 1 回だけ置く（深い木でもスタックを使わない）
        let mut todo: Vec<Placing> = vec![(b, c, attach, r)];
        while let Some((b, c, attach, r)) = todo.pop() {
            if self.done[b] {
                continue;
            }
            self.done[b] = true;
            let start = attach.map_or_else(
                || self.blocks[b].iter().copied().min().unwrap_or(0),
                |a| a.0,
            );
            let order = self.order(b, start);
            let a0 = attach.map_or(-PI / 2.0, |a| a.1);
            let m = order.len();
            let mut angles = Vec::with_capacity(m);
            for (k, &v) in order.iter().enumerate() {
                let a = a0 + TAU * k as f64 / m as f64;
                if attach.is_none_or(|x| x.0 != v) {
                    self.pos[v] = (c.0 + r * a.cos(), c.1 + r * a.sin());
                }
                angles.push((v, a));
            }
            // 切断点の子のブロック（外へ）
            for (v, a) in angles {
                if attach.is_some_and(|x| x.0 == v) {
                    continue;
                }
                let kids: Vec<usize> = self.of_node[v]
                    .iter()
                    .copied()
                    .filter(|&k| k != b && !self.done[k])
                    .collect();
                if kids.is_empty() {
                    continue;
                }
                let span = (PI * 0.9).min(kids.len() as f64 * PI / 3.0);
                for (kb, kc, ka, kr) in self.spread(v, &kids, a, span) {
                    todo.push((kb, kc, Some((v, ka)), kr));
                }
            }
        }
    }

    /// 切断点 `v`（位置は決まっている）のまわりに子のブロックを置く: 向き `dir` を中心に幅 `span` に広げる。
    /// 辺 1 本のブロックは、並んだ兄弟の先のノードが重ならない長さまで伸ばす。中心・角・半径を返す。
    fn spread(&self, v: usize, kids: &[usize], dir: f64, span: f64) -> Vec<(usize, P, f64, f64)> {
        let m = kids.len();
        let p = self.pos[v];
        let mut out = Vec::with_capacity(m);
        let reach = if m < 2 {
            0.0
        } else if span >= TAU - 1e-9 {
            kids.iter().map(|&k| 2.0 * self.radius(k)).sum::<f64>() / TAU
        } else {
            let widest = kids
                .iter()
                .map(|&k| 2.0 * self.radius(k))
                .fold(0.0, f64::max);
            widest / (span / (m - 1) as f64)
        };
        for (j, &kb) in kids.iter().enumerate() {
            let t = if m == 1 {
                dir
            } else if span >= TAU - 1e-9 {
                dir + TAU * j as f64 / m as f64
            } else {
                dir - span / 2.0 + span * j as f64 / (m - 1) as f64
            };
            let mut r = self.radius(kb);
            if self.blocks[kb].len() == 2 {
                r = r.max(reach / 2.0);
            }
            let c = (p.0 + r * t.cos(), p.1 + r * t.sin());
            out.push((kb, c, t + PI, r));
        }
        out
    }

    /// 木の中心 `hub` のまわりにすべてのブロックを置く。
    fn children_at(&mut self, hub: usize, dir: f64, span: f64) {
        let kids: Vec<usize> = self.of_node[hub]
            .iter()
            .copied()
            .filter(|&k| !self.done[k])
            .collect();
        for (kb, kc, ka, kr) in self.spread(hub, &kids, dir, span) {
            self.place_block(kb, kc, Some((hub, ka)), kr);
        }
    }

    /// ブロックの中の順: `start` からの深さ優先の順を、隣どうしの入れ替えで弦の交差が減るかぎり直す。
    fn order(&mut self, b: usize, start: usize) -> Vec<usize> {
        let list = &self.blocks[b];
        let m = list.len();
        let member = |v: usize| list.binary_search(&v).is_ok();
        let mut seen = vec![false; m];
        let idx = |v: usize| list.binary_search(&v).unwrap_or(0);
        let mut order = Vec::with_capacity(m);
        let mut stack = vec![start];
        while let Some(v) = stack.pop() {
            if !member(v) || seen[idx(v)] {
                continue;
            }
            seen[idx(v)] = true;
            order.push(v);
            for &(w, _) in self.adj[v].iter().rev() {
                if member(w) && !seen[idx(w)] {
                    stack.push(w);
                }
            }
        }
        for (k, &v) in list.iter().enumerate() {
            if !seen[k] {
                order.push(v);
            }
        }
        if m < 4 {
            return order;
        }
        // 弦（ブロックの中の辺）
        let mut chords: Vec<(usize, usize)> = Vec::new();
        for (k, &v) in list.iter().enumerate() {
            for &(w, _) in &self.adj[v] {
                if w > v && member(w) {
                    chords.push((k, idx(w)));
                }
            }
        }
        let e = chords.len();
        let cost = e * e;
        let mut at = vec![0usize; m];
        let set = |order: &[usize], at: &mut [usize]| {
            for (p, &v) in order.iter().enumerate() {
                at[list.binary_search(&v).unwrap_or(0)] = p;
            }
        };
        set(&order, &mut at);
        let mut best = crossings(&chords, &at);
        // 始めの点（親とつながる）は動かさない
        'passes: for _ in 0..20 {
            let mut better = false;
            for p in 1..m {
                if best == 0 {
                    break 'passes;
                }
                if self.work < cost {
                    crate::cut_short();
                    break 'passes;
                }
                self.work -= cost;
                let q = (p + 1) % m;
                if q == 0 {
                    continue;
                }
                order.swap(p, q);
                set(&order, &mut at);
                let c = crossings(&chords, &at);
                if c < best {
                    best = c;
                    better = true;
                } else {
                    order.swap(p, q);
                    set(&order, &mut at);
                }
            }
            if !better {
                break;
            }
        }
        order
    }
}

/// 円の上の順 `at` での弦の交差の数。
fn crossings(chords: &[(usize, usize)], at: &[usize]) -> usize {
    let mut count = 0;
    for (i, &(a, b)) in chords.iter().enumerate() {
        let (lo, hi) = (at[a].min(at[b]), at[a].max(at[b]));
        for &(c, d) in &chords[i + 1..] {
            let (pc, pd) = (at[c], at[d]);
            if pc == lo || pc == hi || pd == lo || pd == hi {
                continue;
            }
            let inside = |p: usize| p > lo && p < hi;
            if inside(pc) != inside(pd) {
                count += 1;
            }
        }
    }
    count
}

/// 二重連結成分（Hopcroft, Tarjan 1973 の深さ優先と辺のスタック。再帰しない）。各成分はノードの番号の昇順。
fn biconnected(adj: &[Vec<(usize, f64)>]) -> Vec<Vec<usize>> {
    let n = adj.len();
    const UNSET: usize = usize::MAX;
    let mut disc = vec![UNSET; n];
    let mut low = vec![0usize; n];
    let mut parent = vec![UNSET; n];
    let mut time = 0;
    let mut blocks = Vec::new();
    let mut edges: Vec<(usize, usize)> = Vec::new();
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for s in 0..n {
        if disc[s] != UNSET {
            continue;
        }
        disc[s] = time;
        low[s] = time;
        time += 1;
        stack.push((s, 0));
        while let Some(&(v, i)) = stack.last() {
            if i < adj[v].len() {
                if let Some(top) = stack.last_mut() {
                    top.1 += 1;
                }
                let w = adj[v][i].0;
                if disc[w] == UNSET {
                    parent[w] = v;
                    disc[w] = time;
                    low[w] = time;
                    time += 1;
                    edges.push((v, w));
                    stack.push((w, 0));
                } else if w != parent[v] && disc[w] < disc[v] {
                    low[v] = low[v].min(disc[w]);
                    edges.push((v, w));
                }
            } else {
                stack.pop();
                if let Some(&(u, _)) = stack.last() {
                    low[u] = low[u].min(low[v]);
                    if low[v] >= disc[u] {
                        let mut block = Vec::new();
                        while let Some((a, b)) = edges.pop() {
                            block.push(a);
                            block.push(b);
                            if (a, b) == (u, v) {
                                break;
                            }
                        }
                        block.sort_unstable();
                        block.dedup();
                        blocks.push(block);
                    }
                }
            }
        }
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn links(pairs: &[(usize, usize)]) -> Vec<Link> {
        pairs
            .iter()
            .map(|&(t, h)| Link {
                tail: t,
                head: h,
                len: None,
                weight: 1.0,
            })
            .collect()
    }

    #[test]
    fn finds_blocks() {
        // 三角 0-1-2、橋 2-3、四角 3-4-5-6
        let l = links(&[
            (0, 1),
            (1, 2),
            (2, 0),
            (2, 3),
            (3, 4),
            (4, 5),
            (5, 6),
            (6, 3),
        ]);
        let adj = common::adjacency(7, &l, 1.0);
        let mut b = biconnected(&adj);
        b.sort();
        assert_eq!(b, vec![vec![0, 1, 2], vec![2, 3], vec![3, 4, 5, 6]]);
    }

    #[test]
    fn cycle_on_a_circle() {
        let items = vec![
            Item {
                w: 54.0,
                h: 36.0,
                cluster: None,
                pos: None,
                pinned: false,
                area: 1.0,
                root: false,
                sortv: None,
            };
            7
        ];
        let l = links(&[(0, 1), (1, 2), (2, 3), (3, 4), (4, 5), (5, 0), (2, 6)]);
        let p = layout(&items, &l, &Attrs::default());
        let c = (0..6).fold((0.0, 0.0), |a, v| (a.0 + p[v].0 / 6.0, a.1 + p[v].1 / 6.0));
        let r: Vec<f64> = (0..6).map(|v| (p[v].0 - c.0).hypot(p[v].1 - c.1)).collect();
        for v in 1..6 {
            assert!((r[v] - r[0]).abs() < 1e-6, "{r:?}");
        }
        // 6 は 2 の外
        let d6 = (p[6].0 - c.0).hypot(p[6].1 - c.1);
        assert!(d6 > r[0] + 50.0);
    }
}
