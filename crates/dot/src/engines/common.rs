//! 配置に共通の道具: 決まった種の乱数、隣接、最短路、つながった成分、成分の詰め、重なりの除き、クラスタの箱。

use std::cmp::Reverse;
use std::collections::{BTreeSet, BinaryHeap};

use crate::Note;
use crate::graph::Attrs;

use super::{Group, Item, Link};

pub type P = (f64, f64);

pub const INCH: f64 = 72.0;

/// 決まった種の乱数（splitmix64）。実行のたびに変わらない。
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed ^ 0x9E37_79B9_7F4A_7C15)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// [0, 1) の数。
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// `start` の種（`random5`・`5` などの数。なければ 1）。
pub fn seed(g: &Attrs) -> u64 {
    let s = g.get("start").unwrap_or("").trim().to_ascii_lowercase();
    let digits: String = s.chars().filter(char::is_ascii_digit).collect();
    digits.parse::<u64>().unwrap_or(1)
}

/// 向きのない隣接（自分への辺は除き、同じ組は短いほうの長さ 1 つに）。
pub fn adjacency(n: usize, links: &[Link], default_len: f64) -> Vec<Vec<(usize, f64)>> {
    let mut adj: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
    for l in links {
        if l.tail == l.head || l.tail >= n || l.head >= n {
            continue;
        }
        let len = l.len.unwrap_or(default_len).max(1.0);
        adj[l.tail].push((l.head, len));
        adj[l.head].push((l.tail, len));
    }
    for list in &mut adj {
        list.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        list.dedup_by_key(|e| e.0);
    }
    adj
}

/// つながった成分（ノードの番号の順）。
pub fn components(adj: &[Vec<(usize, f64)>]) -> Vec<Vec<usize>> {
    let n = adj.len();
    let mut seen = vec![false; n];
    let mut out = Vec::new();
    for s in 0..n {
        if seen[s] {
            continue;
        }
        seen[s] = true;
        let mut comp = vec![s];
        let mut k = 0;
        while k < comp.len() {
            let v = comp[k];
            k += 1;
            for &(w, _) in &adj[v] {
                if !seen[w] {
                    seen[w] = true;
                    comp.push(w);
                }
            }
        }
        comp.sort_unstable();
        out.push(comp);
    }
    out
}

/// `src` からの最短路の長さ（Dijkstra）。届かなければ無限大。
pub fn shortest(adj: &[Vec<(usize, f64)>], src: usize, out: &mut [f64]) {
    out.iter_mut().for_each(|d| *d = f64::INFINITY);
    out[src] = 0.0;
    // 負でない f64 のビットは大小の順と同じ
    let mut heap = BinaryHeap::new();
    heap.push(Reverse((0u64, src)));
    while let Some(Reverse((bits, v))) = heap.pop() {
        let d = f64::from_bits(bits);
        if d > out[v] {
            continue;
        }
        for &(w, len) in &adj[v] {
            let nd = d + len;
            if nd < out[w] {
                out[w] = nd;
                heap.push(Reverse((nd.to_bits(), w)));
            }
        }
    }
}

/// 半分の大きさ。
pub fn halves(items: &[Item]) -> Vec<P> {
    items
        .iter()
        .map(|it| (it.w.max(0.0) / 2.0, it.h.max(0.0) / 2.0))
        .collect()
}

/// 成分を左上から行に詰める（大きいものから）。`gap` は間。`movable` が偽の成分は動かさない。
pub fn pack(parts: &[Vec<usize>], movable: &[bool], pos: &mut [P], half: &[P], gap: f64) {
    struct Part {
        idx: usize,
        b: [f64; 4],
    }
    let mut boxes: Vec<Part> = Vec::new();
    let mut fixed: Option<[f64; 4]> = None;
    for (k, part) in parts.iter().enumerate() {
        let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
        for &v in part {
            b[0] = b[0].min(pos[v].0 - half[v].0);
            b[1] = b[1].min(pos[v].1 - half[v].1);
            b[2] = b[2].max(pos[v].0 + half[v].0);
            b[3] = b[3].max(pos[v].1 + half[v].1);
        }
        if part.is_empty() || !b.iter().all(|v| v.is_finite()) {
            continue;
        }
        if movable.get(k).copied().unwrap_or(true) {
            boxes.push(Part { idx: k, b });
        } else {
            let f = fixed.get_or_insert(b);
            *f = [
                f[0].min(b[0]),
                f[1].min(b[1]),
                f[2].max(b[2]),
                f[3].max(b[3]),
            ];
        }
    }
    if boxes.is_empty() {
        return;
    }
    let area: f64 = boxes
        .iter()
        .map(|p| (p.b[2] - p.b[0] + gap) * (p.b[3] - p.b[1] + gap))
        .sum();
    let widest = boxes.iter().map(|p| p.b[2] - p.b[0]).fold(0.0, f64::max);
    let limit = widest.max(area.sqrt() * 1.2);
    boxes.sort_by(|a, b| {
        (b.b[3] - b.b[1])
            .total_cmp(&(a.b[3] - a.b[1]))
            .then(a.idx.cmp(&b.idx))
    });
    // 動かさない成分があれば、その右から
    let (x0, y0) = match fixed {
        Some(f) => (f[2] + gap, f[1]),
        None => (0.0, 0.0),
    };
    let (mut x, mut y, mut row_h) = (x0, y0, 0.0f64);
    for p in &boxes {
        let (w, h) = (p.b[2] - p.b[0], p.b[3] - p.b[1]);
        if x > x0 && x + w > x0 + limit {
            x = x0;
            y += row_h + gap;
            row_h = 0.0;
        }
        let (dx, dy) = (x - p.b[0], y - p.b[1]);
        for &v in &parts[p.idx] {
            pos[v].0 += dx;
            pos[v].1 += dy;
        }
        x += w + gap;
        row_h = row_h.max(h);
    }
}

/// `pack` の間（pt）。`pack=true` か数がなければ `default`。
pub fn pack_gap(g: &Attrs, default: f64) -> f64 {
    g.num("pack")
        .filter(|v| *v >= 0.0)
        .unwrap_or(default)
        .min(500.0)
}

/// 重なりの扱い。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overlap {
    /// そのまま（`overlap=true`）。
    Keep,
    /// 押し離す。
    Remove,
    /// 全体を同じ倍率で広げる。
    Scale,
}

/// `overlap` を読む。近いもので済ませる値は注記をつけて押し離す。
pub fn overlap(g: &Attrs, default: Overlap, notes: &mut BTreeSet<Note>) -> Overlap {
    let Some(v) = g.get("overlap") else {
        return default;
    };
    let v = v.trim().to_ascii_lowercase();
    // `9:prism` のような段の指定は最後の値だけ
    let v = v.rsplit(':').next().unwrap_or("").to_owned();
    match v.as_str() {
        "" => default,
        "true" | "yes" => Overlap::Keep,
        "scale" => Overlap::Scale,
        "false" | "no" | "voronoi" | "vpsc" => Overlap::Remove,
        s if s.starts_with("prism") => Overlap::Remove,
        s if s.parse::<f64>().is_ok() => {
            if s.parse::<f64>().unwrap_or(0.0) != 0.0 {
                Overlap::Keep
            } else {
                Overlap::Remove
            }
        }
        _ => {
            // scalexy・compress・ortho・portho・ipsep など
            notes.insert(Note::LayoutOptions);
            Overlap::Remove
        }
    }
}

/// `sep`（重なりを除くときのノードのまわりの余白）。`+x,y` は足す点、`x,y` は大きさの倍率。
pub fn sep(g: &Attrs, half: &[P]) -> Vec<P> {
    let raw = g.get("sep").unwrap_or("+4").trim();
    let (add, body) = match raw.strip_prefix('+') {
        Some(rest) => (true, rest),
        None => (false, raw),
    };
    let mut it = body
        .split(',')
        .filter_map(|s| s.trim().parse::<f64>().ok())
        .filter(|v| v.is_finite());
    let x = it.next().unwrap_or(4.0).clamp(0.0, 500.0);
    let y = it.next().unwrap_or(x).clamp(0.0, 500.0);
    half.iter()
        .map(|&(w, h)| {
            if add {
                (w + x, h + y)
            } else {
                (w * (1.0 + x.min(10.0)), h * (1.0 + y.min(10.0)))
            }
        })
        .collect()
}

/// 押し離しの回数と、対を見る数の上限。
const PUSH_ROUNDS: usize = 300;
const PUSH_WORK: usize = 20_000_000;

/// 重なる対を押し離す（x の左端で並べて掃き、重なりの浅い向きに半分ずつ。Misue ほか 1995 の force-scan と
/// 同じ考え）。`pinned` は動かさない。
pub fn remove_overlaps(pos: &mut [P], half: &[P], pinned: &[bool]) {
    let n = pos.len();
    if n < 2 {
        return;
    }
    let mut order: Vec<usize> = (0..n).collect();
    let mut work = PUSH_WORK;
    for _ in 0..PUSH_ROUNDS {
        order.sort_by(|&a, &b| {
            (pos[a].0 - half[a].0)
                .total_cmp(&(pos[b].0 - half[b].0))
                .then(a.cmp(&b))
        });
        let mut moved = false;
        for a in 0..n {
            let i = order[a];
            for &j in &order[a + 1..] {
                if work == 0 {
                    return;
                }
                work -= 1;
                if pos[j].0 - half[j].0 >= pos[i].0 + half[i].0 {
                    break;
                }
                let (dx, dy) = (pos[j].0 - pos[i].0, pos[j].1 - pos[i].1);
                let ox = half[i].0 + half[j].0 - dx.abs();
                let oy = half[i].1 + half[j].1 - dy.abs();
                if ox <= 0.0 || oy <= 0.0 {
                    continue;
                }
                let (pi, pj) = (
                    pinned.get(i).copied().unwrap_or(false),
                    pinned.get(j).copied().unwrap_or(false),
                );
                if pi && pj {
                    continue;
                }
                let (si, sj) = match (pi, pj) {
                    (true, _) => (0.0, 1.0),
                    (_, true) => (1.0, 0.0),
                    _ => (0.5, 0.5),
                };
                let along_x = if dx == 0.0 && dy == 0.0 {
                    (i + j).is_multiple_of(2)
                } else {
                    ox < oy
                };
                if along_x {
                    let s = if dx < 0.0 { -1.0 } else { 1.0 };
                    let d = ox + 0.01;
                    pos[i].0 -= s * d * si;
                    pos[j].0 += s * d * sj;
                } else {
                    let s = if dy < 0.0 { -1.0 } else { 1.0 };
                    let d = oy + 0.01;
                    pos[i].1 -= s * d * si;
                    pos[j].1 += s * d * sj;
                }
                moved = true;
            }
        }
        if !moved {
            return;
        }
    }
}

/// 全体を同じ倍率で広げて重なりをなくす（`overlap=scale`）。重なる中心どうしは広げても離れないので、残れば押し離す。
pub fn scale_apart(pos: &mut [P], half: &[P], pinned: &[bool]) {
    let n = pos.len();
    let mut s = 1.0f64;
    let mut stuck = false;
    if n <= 3000 {
        for i in 0..n {
            for j in i + 1..n {
                let (dx, dy) = ((pos[j].0 - pos[i].0).abs(), (pos[j].1 - pos[i].1).abs());
                let (nx, ny) = (half[i].0 + half[j].0, half[i].1 + half[j].1);
                if dx >= nx || dy >= ny {
                    continue;
                }
                let need = match (dx > 1e-9, dy > 1e-9) {
                    (true, true) => (nx / dx).min(ny / dy),
                    (true, false) => nx / dx,
                    (false, true) => ny / dy,
                    (false, false) => {
                        stuck = true;
                        continue;
                    }
                };
                s = s.max(need.min(1e4));
            }
        }
    }
    if s > 1.0 && !pinned.iter().any(|&p| p) {
        for p in pos.iter_mut() {
            p.0 *= s;
            p.1 *= s;
        }
    }
    if stuck || s >= 1e4 || pinned.iter().any(|&p| p) || n > 3000 {
        remove_overlaps(pos, half, pinned);
    }
}

/// 重なりを扱う。
pub fn apply_overlap(mode: Overlap, pos: &mut [P], half_sep: &[P], pinned: &[bool]) {
    match mode {
        Overlap::Keep => {}
        Overlap::Remove => remove_overlaps(pos, half_sep, pinned),
        Overlap::Scale => scale_apart(pos, half_sep, pinned),
    }
}

/// クラスタの深さ（根の直下が 0）。
pub fn depth(groups: &[Group], mut c: usize) -> usize {
    let mut d = 0;
    while let Some(p) = groups[c].parent {
        d += 1;
        c = p;
        if d > groups.len() {
            break;
        }
    }
    d
}

/// クラスタの箱を、中のノードと内のクラスタを囲む箱にする（余白と題の分を足す）。配置がクラスタを考えない
/// ときの箱（Graphviz の neato と同じ）。
pub fn cluster_bounds(items: &[Item], groups: &[Group], pos: &[P]) -> Vec<Option<[f64; 4]>> {
    let mut boxes: Vec<Option<[f64; 4]>> = vec![None; groups.len()];
    let mut order: Vec<usize> = (0..groups.len()).collect();
    order.sort_by_key(|&c| Reverse(depth(groups, c)));
    let grow = |b: &mut Option<[f64; 4]>, r: [f64; 4]| {
        *b = Some(match *b {
            None => r,
            Some(o) => [
                o[0].min(r[0]),
                o[1].min(r[1]),
                o[2].max(r[2]),
                o[3].max(r[3]),
            ],
        });
    };
    let mut inner: Vec<Option<[f64; 4]>> = vec![None; groups.len()];
    for (i, it) in items.iter().enumerate() {
        if let Some(c) = it.cluster.filter(|&c| c < groups.len()) {
            let (x, y) = pos[i];
            grow(
                &mut inner[c],
                [
                    x - it.w / 2.0,
                    y - it.h / 2.0,
                    x + it.w / 2.0,
                    y + it.h / 2.0,
                ],
            );
        }
    }
    for c in order {
        let Some(b) = inner[c] else { continue };
        let gr = &groups[c];
        let m = gr.margin;
        let (lw, lh) = gr.label;
        let mut r = [b[0] - m, b[1] - m - lh, b[2] + m, b[3] + m];
        if r[2] - r[0] < lw + 2.0 * m {
            let extra = (lw + 2.0 * m - (r[2] - r[0])) / 2.0;
            r[0] -= extra;
            r[2] += extra;
        }
        boxes[c] = Some(r);
        if let Some(p) = gr.parent.filter(|&p| p < groups.len()) {
            grow(&mut inner[p], r);
        }
    }
    boxes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_is_fixed() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        for _ in 0..10 {
            let x = a.unit();
            assert!((0.0..1.0).contains(&x));
            assert_eq!(x, b.unit());
        }
    }

    #[test]
    fn pushes_overlapping_boxes_apart() {
        let mut pos = vec![(0.0, 0.0), (5.0, 1.0), (5.0, 1.0), (200.0, 0.0)];
        let half = vec![(20.0, 10.0); 4];
        let pinned = vec![true, false, false, false];
        remove_overlaps(&mut pos, &half, &pinned);
        assert_eq!(pos[0], (0.0, 0.0));
        for i in 0..4 {
            for j in i + 1..4 {
                let ox = 40.0 - (pos[i].0 - pos[j].0).abs();
                let oy = 20.0 - (pos[i].1 - pos[j].1).abs();
                assert!(ox <= 0.0 || oy <= 0.0, "{i} {j} {pos:?}");
            }
        }
    }

    #[test]
    fn shortest_paths_and_components() {
        let links = [(0, 1, 2.0), (1, 2, 3.0), (0, 2, 10.0), (3, 4, 1.0)].map(|(t, h, l)| Link {
            tail: t,
            head: h,
            len: Some(l),
            weight: 1.0,
        });
        let adj = adjacency(5, &links, 1.0);
        let mut d = vec![0.0; 5];
        shortest(&adj, 0, &mut d);
        assert_eq!(&d[..3], &[0.0, 2.0, 5.0]);
        assert!(d[3].is_infinite());
        assert_eq!(components(&adj), vec![vec![0, 1, 2], vec![3, 4]]);
    }
}
