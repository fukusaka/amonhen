//! neato: 最短路の距離を目標にした stress の最小化（Gansner, Koren, North, "Graph Drawing by Stress
//! Majorization", 2004）。重み w_ij = d_ij^-2 の stress を、ノードごとの majorization の更新（ほかを止めて 1 つずつ
//! 解く局所の版）で小さくする。初めの位置は pivot MDS（Brandes, Pich, "Eigensolver Methods for Progressive
//! Multidimensional Scaling of Large Data", 2006）。大きな成分（1,000 ノード超）は、全部の対の代わりに隣と
//! pivot との対だけを使う疎な stress（Ortmann, Klimenta, Brandes 2016 の考え）。

use std::collections::BTreeSet;

use super::common::{self, INCH, Overlap, P, Rng};
use super::{Item, Link};
use crate::Note;
use crate::graph::Attrs;

/// 全部の対の距離を持つ成分の大きさの上限。
const FULL: usize = 1000;
/// pivot の数。
const PIVOTS: usize = 50;
/// 更新で見る対の数の上限（図ぜんぶ）。
const WORK: usize = 60_000_000;
/// 辺の既定の長さ（neato の `len` の既定 1 インチ）。
const DEFAULT_LEN: f64 = INCH;

pub fn layout(
    items: &[Item],
    links: &[Link],
    g: &Attrs,
    nop: bool,
    notes: &mut BTreeSet<Note>,
) -> Vec<P> {
    let n = items.len();
    if n == 0 {
        return vec![];
    }
    if let Some(m) = g.get("mode") {
        // KK と SGD は同じ stress を別の解き方で小さくするので同じに描く
        if !matches!(
            m.trim().to_ascii_lowercase().as_str(),
            "" | "major" | "kk" | "sgd"
        ) {
            notes.insert(Note::LayoutOptions);
        }
    }
    if let Some(m) = g.get("model")
        && !matches!(
            m.trim().to_ascii_lowercase().as_str(),
            "" | "shortpath" | "mds"
        )
    {
        notes.insert(Note::LayoutOptions);
    }
    let pinned: Vec<bool> = items
        .iter()
        .map(|it| it.pos.is_some() && (it.pinned || nop))
        .collect();
    let mut pos: Vec<P> = items
        .iter()
        .map(|it| it.pos.unwrap_or((0.0, 0.0)))
        .collect();
    if nop && items.iter().all(|it| it.pos.is_some()) {
        // neato -n: 位置はそのまま
        return pos;
    }
    let adj = common::adjacency(n, links, DEFAULT_LEN);
    let half = common::halves(items);
    let comps = common::components(&adj);
    let opts = Opts {
        start: g.get("start").unwrap_or("").trim().to_ascii_lowercase(),
        maxiter: g
            .num("maxiter")
            .map_or(200, |v| v.clamp(0.0, 10_000.0) as usize),
        epsilon: g
            .num("epsilon")
            .filter(|v| *v > 0.0)
            .unwrap_or(1e-4)
            .min(1.0),
    };
    let mut rng = Rng::new(common::seed(g));
    let mut local = vec![usize::MAX; n];
    let mut budget = WORK;
    let mut movable = Vec::with_capacity(comps.len());
    for comp in &comps {
        movable.push(!comp.iter().any(|&v| items[v].pos.is_some()));
        component(
            comp,
            &adj,
            items,
            &pinned,
            &opts,
            &mut rng,
            &mut local,
            &mut budget,
            &mut pos,
        );
    }
    if comps.len() > 1 {
        common::pack(&comps, &movable, &mut pos, &half, common::pack_gap(g, 16.0));
    }
    let mode = common::overlap(g, Overlap::Keep, notes);
    let hs = common::sep(g, &half);
    common::apply_overlap(mode, &mut pos, &hs, &pinned);
    pos
}

struct Opts {
    start: String,
    maxiter: usize,
    epsilon: f64,
}

/// 距離の表（全部の対か、pivot からだけ）。
enum Dist {
    Full(Vec<f64>),
    Pivots { piv: Vec<usize>, d: Vec<f64> },
}

#[allow(clippy::too_many_arguments)]
fn component(
    comp: &[usize],
    adj: &[Vec<(usize, f64)>],
    items: &[Item],
    pinned: &[bool],
    opts: &Opts,
    rng: &mut Rng,
    local: &mut [usize],
    budget: &mut usize,
    pos: &mut [P],
) {
    let m = comp.len();
    if m == 1 {
        return;
    }
    for (k, &v) in comp.iter().enumerate() {
        local[v] = k;
    }
    let sub: Vec<Vec<(usize, f64)>> = comp
        .iter()
        .map(|&v| adj[v].iter().map(|&(w, l)| (local[w], l)).collect())
        .collect();
    let fixed: Vec<bool> = comp.iter().map(|&v| pinned[v]).collect();
    let avg_len = {
        let (s, c) = sub
            .iter()
            .flatten()
            .fold((0.0, 0usize), |(s, c), e| (s + e.1, c + 1));
        if c == 0 { DEFAULT_LEN } else { s / c as f64 }
    };
    // 距離
    let dist = if m <= FULL {
        let mut d = vec![0.0; m * m];
        for i in 0..m {
            common::shortest(&sub, i, &mut d[i * m..(i + 1) * m]);
        }
        Dist::Full(d)
    } else {
        let k = PIVOTS.min(m);
        let mut piv = Vec::with_capacity(k);
        let mut d = vec![0.0; k * m];
        let mut nearest = vec![f64::INFINITY; m];
        let mut next = (0..m).max_by_key(|&i| sub[i].len()).unwrap_or(0);
        for p in 0..k {
            piv.push(next);
            common::shortest(&sub, next, &mut d[p * m..(p + 1) * m]);
            let mut far = (f64::MIN, 0);
            for i in 0..m {
                nearest[i] = nearest[i].min(d[p * m + i]);
                if nearest[i] > far.0 {
                    far = (nearest[i], i);
                }
            }
            next = far.1;
        }
        Dist::Pivots { piv, d }
    };
    // 初めの位置
    let has_pos = comp.iter().any(|&v| items[v].pos.is_some());
    let side = (m as f64).sqrt() * avg_len;
    let mut x: Vec<P> = if has_pos {
        start_self(comp, items, &sub, rng, avg_len)
    } else if opts.start.starts_with("random") || opts.start.parse::<u64>().is_ok() {
        (0..m)
            .map(|_| (rng.unit() * side, rng.unit() * side))
            .collect()
    } else if opts.start == "regular" {
        let r = m as f64 * avg_len / (2.0 * std::f64::consts::PI);
        (0..m)
            .map(|i| {
                let a = 2.0 * std::f64::consts::PI * i as f64 / m as f64;
                (r * a.cos(), r * a.sin())
            })
            .collect()
    } else {
        let mds = match &dist {
            Dist::Full(d) => {
                let piv = full_pivots(d, m, PIVOTS.min(m));
                let mut dp = vec![0.0; piv.len() * m];
                for (p, &v) in piv.iter().enumerate() {
                    dp[p * m..(p + 1) * m].copy_from_slice(&d[v * m..(v + 1) * m]);
                }
                pivot_mds(m, &piv, &dp)
            }
            Dist::Pivots { piv, d } => pivot_mds(m, piv, d),
        };
        mds.unwrap_or_else(|| {
            (0..m)
                .map(|_| (rng.unit() * side, rng.unit() * side))
                .collect()
        })
    };
    // 少しずらして重なる点をなくす（決まった乱数）
    for p in &mut x {
        p.0 += (rng.unit() - 0.5) * 1e-3 * avg_len;
        p.1 += (rng.unit() - 0.5) * 1e-3 * avg_len;
    }
    for (k, &v) in comp.iter().enumerate() {
        if fixed[k] {
            x[k] = pos[v];
        }
    }
    // majorization
    let per_iter = match &dist {
        Dist::Full(_) => m * m,
        Dist::Pivots { piv, .. } => m * (piv.len() + 4),
    };
    let iters = opts.maxiter.min(*budget / per_iter.max(1)).max(3);
    let tol = opts.epsilon * avg_len;
    // 手間の上限で回数を減らし、収まる前に尽きたら打ち切ったと記す
    let mut settled = false;
    for _ in 0..iters {
        *budget = budget.saturating_sub(per_iter);
        let mut change = 0.0;
        for i in 0..m {
            if fixed[i] {
                continue;
            }
            let (mut nx, mut ny, mut den) = (0.0, 0.0, 0.0);
            let mut term = |j: usize, d: f64, x: &[P]| {
                if j == i || !d.is_finite() || d <= 0.0 {
                    return;
                }
                let w = 1.0 / (d * d);
                let (dx, dy) = (x[i].0 - x[j].0, x[i].1 - x[j].1);
                let l = (dx * dx + dy * dy).sqrt();
                let (ux, uy) = if l > 1e-9 {
                    (dx / l, dy / l)
                } else if i < j {
                    (1.0, 0.0)
                } else {
                    (-1.0, 0.0)
                };
                nx += w * (x[j].0 + d * ux);
                ny += w * (x[j].1 + d * uy);
                den += w;
            };
            match &dist {
                Dist::Full(d) => {
                    for j in 0..m {
                        term(j, d[i * m + j], &x);
                    }
                }
                Dist::Pivots { piv, d } => {
                    for &(j, l) in &sub[i] {
                        term(j, l, &x);
                    }
                    for (p, &j) in piv.iter().enumerate() {
                        if !sub[i].iter().any(|e| e.0 == j) {
                            term(j, d[p * m + i], &x);
                        }
                    }
                }
            }
            if den > 0.0 {
                let new = (nx / den, ny / den);
                change += (new.0 - x[i].0).abs() + (new.1 - x[i].1).abs();
                x[i] = new;
            }
        }
        if change < tol * m as f64 {
            settled = true;
            break;
        }
    }
    if !settled && iters < opts.maxiter {
        crate::cut_short();
    }
    for (k, &v) in comp.iter().enumerate() {
        pos[v] = x[k];
    }
}

/// 位置のあるノードから、ないノードを隣の平均の近くに置く（`start=self` と同じ考え）。
fn start_self(
    comp: &[usize],
    items: &[Item],
    sub: &[Vec<(usize, f64)>],
    rng: &mut Rng,
    len: f64,
) -> Vec<P> {
    let m = comp.len();
    let mut x: Vec<Option<P>> = comp.iter().map(|&v| items[v].pos).collect();
    let mut queue: Vec<usize> = (0..m).filter(|&i| x[i].is_some()).collect();
    let mut k = 0;
    while k < queue.len() {
        let v = queue[k];
        k += 1;
        for &(w, _) in &sub[v] {
            if x[w].is_some() {
                continue;
            }
            let placed: Vec<P> = sub[w].iter().filter_map(|&(u, _)| x[u]).collect();
            let c = placed.len().max(1) as f64;
            let (sx, sy) = placed
                .iter()
                .fold((0.0, 0.0), |(a, b), p| (a + p.0, b + p.1));
            x[w] = Some((
                sx / c + (rng.unit() - 0.5) * len,
                sy / c + (rng.unit() - 0.5) * len,
            ));
            queue.push(w);
        }
    }
    x.into_iter().map(|p| p.unwrap_or((0.0, 0.0))).collect()
}

/// 互いに遠い pivot（最大の最小距離で選ぶ）。
fn full_pivots(d: &[f64], m: usize, k: usize) -> Vec<usize> {
    let mut piv = Vec::with_capacity(k);
    let mut nearest = vec![f64::INFINITY; m];
    let mut next = 0;
    for _ in 0..k {
        piv.push(next);
        let mut far = (f64::MIN, 0);
        for i in 0..m {
            nearest[i] = nearest[i].min(d[next * m + i]);
            if nearest[i] > far.0 {
                far = (nearest[i], i);
            }
        }
        if far.0 <= 0.0 {
            break;
        }
        next = far.1;
    }
    piv
}

/// pivot MDS: pivot との距離の二乗を二重に中心化した行列 C（m×k）の、CᵀC の上の 2 つの固有ベクトルへの射影。
/// 固有ベクトルは決まった初めのベクトルからのべき乗法。
fn pivot_mds(m: usize, piv: &[usize], dp: &[f64]) -> Option<Vec<P>> {
    let k = piv.len();
    if k < 2 {
        return None;
    }
    let mut c = vec![0.0; m * k];
    for p in 0..k {
        for i in 0..m {
            let d = dp[p * m + i];
            if !d.is_finite() {
                return None;
            }
            c[i * k + p] = d * d;
        }
    }
    let mut row = vec![0.0; m];
    let mut col = vec![0.0; k];
    let mut all = 0.0;
    for i in 0..m {
        for p in 0..k {
            let v = c[i * k + p];
            row[i] += v / k as f64;
            col[p] += v / m as f64;
            all += v;
        }
    }
    all /= (m * k) as f64;
    for i in 0..m {
        for p in 0..k {
            c[i * k + p] = -0.5 * (c[i * k + p] - row[i] - col[p] + all);
        }
    }
    let mut mat = vec![0.0; k * k];
    for i in 0..m {
        let r = &c[i * k..(i + 1) * k];
        for p in 0..k {
            if r[p] == 0.0 {
                continue;
            }
            for q in 0..k {
                mat[p * k + q] += r[p] * r[q];
            }
        }
    }
    let power = |against: Option<&[f64]>| -> Vec<f64> {
        // 決まった乱数の初めのベクトル（規則的な値だと固有空間に直交することがある）
        let mut seed = Rng::new(0x5EED + against.map_or(0, |_| 1));
        let mut v: Vec<f64> = (0..k).map(|_| seed.unit() + 0.5).collect();
        for _ in 0..200 {
            if let Some(u) = against {
                let dot: f64 = v.iter().zip(u).map(|(a, b)| a * b).sum();
                v.iter_mut().zip(u).for_each(|(a, b)| *a -= dot * b);
            }
            let mut w = vec![0.0; k];
            for p in 0..k {
                let r = &mat[p * k..(p + 1) * k];
                w[p] = r.iter().zip(&v).map(|(a, b)| a * b).sum();
            }
            let norm = w.iter().map(|a| a * a).sum::<f64>().sqrt();
            if norm < 1e-12 {
                return vec![0.0; k];
            }
            let mut diff = 0.0;
            for p in 0..k {
                let nw = w[p] / norm;
                diff += (nw - v[p]).abs();
                v[p] = nw;
            }
            if diff < 1e-10 {
                break;
            }
        }
        v
    };
    let v1 = power(None);
    let v2 = power(Some(&v1));
    let mut x: Vec<P> = (0..m)
        .map(|i| {
            let r = &c[i * k..(i + 1) * k];
            (
                r.iter().zip(&v1).map(|(a, b)| a * b).sum(),
                r.iter().zip(&v2).map(|(a, b)| a * b).sum(),
            )
        })
        .collect();
    // 距離に合う倍率
    let (mut num, mut den) = (0.0, 0.0);
    for (p, &v) in piv.iter().enumerate() {
        for i in 0..m {
            let (dx, dy) = (x[i].0 - x[v].0, x[i].1 - x[v].1);
            let l = (dx * dx + dy * dy).sqrt();
            num += dp[p * m + i] * l;
            den += l * l;
        }
    }
    if den < 1e-12 || !num.is_finite() {
        return None;
    }
    let s = num / den;
    for p in &mut x {
        p.0 *= s;
        p.1 *= s;
    }
    Some(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item() -> Item {
        Item {
            w: 54.0,
            h: 36.0,
            cluster: None,
            pos: None,
            pinned: false,
            area: 1.0,
            root: false,
            sortv: None,
        }
    }

    fn link(t: usize, h: usize) -> Link {
        Link {
            tail: t,
            head: h,
            len: None,
            weight: 1.0,
        }
    }

    fn len(p: &[P], a: usize, b: usize) -> f64 {
        (p[a].0 - p[b].0).hypot(p[a].1 - p[b].1)
    }

    #[test]
    fn edges_get_their_length() {
        // 4 の輪: 辺は 1 インチ、対角は 2 インチ近く（輪なので正方形）
        let items = vec![item(); 4];
        let links = vec![link(0, 1), link(1, 2), link(2, 3), link(3, 0)];
        let mut notes = BTreeSet::new();
        let p = layout(&items, &links, &Attrs::default(), false, &mut notes);
        for (a, b) in [(0, 1), (1, 2), (2, 3), (3, 0)] {
            let l = len(&p, a, b);
            assert!((l - 72.0).abs() < 15.0, "{a}-{b}: {l} {p:?}");
        }
        assert!(len(&p, 0, 2) > 90.0);
        assert!(notes.is_empty());
        // いつも同じ
        assert_eq!(
            p,
            layout(&items, &links, &Attrs::default(), false, &mut notes)
        );
    }

    #[test]
    fn pinned_nodes_stay() {
        let mut items = vec![item(); 3];
        items[0].pos = Some((0.0, 0.0));
        items[0].pinned = true;
        items[2].pos = Some((300.0, 0.0));
        items[2].pinned = true;
        let links = vec![link(0, 1), link(1, 2)];
        let p = layout(
            &items,
            &links,
            &Attrs::default(),
            false,
            &mut BTreeSet::new(),
        );
        assert_eq!(p[0], (0.0, 0.0));
        assert_eq!(p[2], (300.0, 0.0));
        assert!((p[1].0 - 150.0).abs() < 30.0, "{p:?}");
    }
}
