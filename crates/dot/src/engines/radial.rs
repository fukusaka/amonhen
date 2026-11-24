//! twopi: 根からの幅優先の木を同心円に置く放射状の配置（Eades, "Drawing Free Trees", 1992）。根は `root` の
//! ノードか、離心数のいちばん小さいノード（グラフの中心）。各ノードは親の角の幅のうち、葉の数に比べた分をもらい、
//! その中央に置く。円の間は `ranksep`（インチ、`:` で円ごと。既定 1 インチ）。

use super::common::{self, INCH, P};
use super::{Item, Link};
use crate::graph::Attrs;

/// 離心数をすべてのノードで数える手間の上限（超えたら 2 回の幅優先で直径の中点）。
const CENTER_WORK: usize = 4_000_000;

pub fn layout(items: &[Item], links: &[Link], g: &Attrs) -> Vec<P> {
    let n = items.len();
    let mut pos = vec![(0.0, 0.0); n];
    if n == 0 {
        return pos;
    }
    let adj = common::adjacency(n, links, 1.0);
    let half = common::halves(items);
    let seps: Vec<f64> = g
        .get("ranksep")
        .map(|s| {
            s.split(':')
                .filter_map(|v| v.trim().parse::<f64>().ok())
                .filter(|v| v.is_finite())
                .map(|v| v.clamp(0.02, 20.0) * INCH)
                .take(64)
                .collect()
        })
        .unwrap_or_default();
    let ring = |k: usize| -> f64 {
        // k 番目の円の半径（0 は根）
        let mut r = 0.0;
        for i in 0..k {
            r += seps.get(i).or(seps.last()).copied().unwrap_or(INCH);
        }
        r
    };
    let comps = common::components(&adj);
    for comp in &comps {
        if comp.len() < 2 {
            continue;
        }
        let root = comp
            .iter()
            .copied()
            .find(|&v| items[v].root)
            .unwrap_or_else(|| center(comp, &adj));
        // 幅優先の木（隣の順で子を並べる）
        let mut parent = vec![usize::MAX; n];
        let mut depth = vec![0usize; n];
        let mut order = vec![root];
        parent[root] = root;
        let mut k = 0;
        while k < order.len() {
            let v = order[k];
            k += 1;
            for &(w, _) in &adj[v] {
                if parent[w] == usize::MAX {
                    parent[w] = v;
                    depth[w] = depth[v] + 1;
                    order.push(w);
                }
            }
        }
        let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
        for &v in &order[1..] {
            children[parent[v]].push(v);
        }
        // 葉の数（下から）
        let mut leaves = vec![0usize; n];
        for &v in order.iter().rev() {
            leaves[v] = if children[v].is_empty() {
                1
            } else {
                children[v].iter().map(|&c| leaves[c]).sum()
            };
        }
        // 角の幅（上から）。根は円ぜんぶ
        let mut wedge = vec![(0.0f64, 0.0f64); n];
        wedge[root] = (0.0, std::f64::consts::TAU);
        pos[root] = (0.0, 0.0);
        for &v in &order {
            let (a0, span) = wedge[v];
            let total = leaves[v].max(1) as f64;
            let mut a = a0;
            for &c in &children[v] {
                let s = span * leaves[c] as f64 / total;
                wedge[c] = (a, s);
                let mid = a + s / 2.0;
                let r = ring(depth[c]);
                pos[c] = (r * mid.cos(), r * mid.sin());
                a += s;
            }
        }
    }
    if comps.len() > 1 {
        let movable = vec![true; comps.len()];
        common::pack(&comps, &movable, &mut pos, &half, common::pack_gap(g, 16.0));
    }
    pos
}

/// 離心数のいちばん小さいノード。大きければ、2 回の幅優先で見つけた最も長い道の中ほど。
fn center(comp: &[usize], adj: &[Vec<(usize, f64)>]) -> usize {
    let edges: usize = comp.iter().map(|&v| adj[v].len()).sum();
    if comp.len().saturating_mul(comp.len() + edges) <= CENTER_WORK {
        let mut best = (usize::MAX, comp[0]);
        for &v in comp {
            let d = common::hops(adj, v);
            let ecc = comp.iter().map(|&w| d[w]).max().unwrap_or(0);
            if ecc < best.0 {
                best = (ecc, v);
            }
        }
        return best.1;
    }
    let far = |d: &[usize]| {
        comp.iter()
            .copied()
            .max_by_key(|&w| (d[w], std::cmp::Reverse(w)))
            .unwrap_or(comp[0])
    };
    let a = far(&common::hops(adj, comp[0]));
    let da = common::hops(adj, a);
    let b = far(&da);
    let db = common::hops(adj, b);
    let len = da[b];
    // 道の上で a から len/2 のノード
    comp.iter()
        .copied()
        .filter(|&w| da[w] == len / 2 && db[w] == len - len / 2)
        .min()
        .unwrap_or(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rings_by_depth_around_the_center() {
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
            6
        ];
        // 0-1-2-3-4 の道と 2-5: 中心は 2
        let links: Vec<Link> = [(0, 1), (1, 2), (2, 3), (3, 4), (2, 5)]
            .iter()
            .map(|&(t, h)| Link {
                tail: t,
                head: h,
                len: None,
                weight: 1.0,
            })
            .collect();
        let p = layout(&items, &links, &Attrs::default());
        let r = |v: usize| p[v].0.hypot(p[v].1);
        assert!(r(2) < 1e-9);
        for v in [1, 3, 5] {
            assert!((r(v) - 72.0).abs() < 1e-6);
        }
        for v in [0, 4] {
            assert!((r(v) - 144.0).abs() < 1e-6);
        }
    }
}
