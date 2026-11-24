//! 辺の道筋の後処理: 端でないノードとクラスタの題を避ける。座標は配置の向き（ランクが y、上から下）。
//!
//! 道筋は点の列（仮のノードを通る）で、点の間は縦の向きに出入りする曲線（`curve::through`）か直線。点の間の線が
//! 箱（ノード・題を少し広げたもの）に入るなら、その箱の角を 1 つか 2 つ通る回り道に替え、短いほうを選ぶ。

use crate::curve::{Cubic, P};

/// 箱（x0, y0, x1, y1）。
pub type Rect = [f64; 4];

/// 避ける箱と、引くための格子。
pub struct Obstacles {
    rects: Vec<Rect>,
    /// 箱の持ち主のノード（題なら None）。辺の端のノードは避けない。
    owner: Vec<Option<usize>>,
    x0: f64,
    y0: f64,
    cell: f64,
    cols: usize,
    rows: usize,
    grid: Vec<Vec<u32>>,
    seen: Vec<u32>,
    stamp: u32,
    /// 残りの手間（線分と箱の判定の数）。尽きたらもう避けない。
    work: usize,
}

/// 格子の目の数の上限（片側）。
const MAX_CELLS: usize = 256;
/// 1 つの線で引く目の上限（これを超える長い線は避けない）。
const MAX_SCAN: usize = 4096;
/// 1 本の辺に足す回り道の点の上限。
const MAX_DETOURS: usize = 12;
/// 図ぜんぶで、線分と箱を比べる回数の上限（込んだ大きなグラフの時間を抑える）。
const WORK: usize = 4_000_000;

fn contains(r: &Rect, p: P) -> bool {
    p.0 > r[0] && p.0 < r[2] && p.1 > r[1] && p.1 < r[3]
}

/// 線分が箱の内側を通るか（縁に触れるだけなら通らない）。
fn segment_hits(a: P, b: P, r: &Rect) -> bool {
    let r = [r[0] + 0.25, r[1] + 0.25, r[2] - 0.25, r[3] - 0.25];
    if r[0] >= r[2] || r[1] >= r[3] {
        return false;
    }
    // Liang–Barsky
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let mut t0 = 0.0f64;
    let mut t1 = 1.0f64;
    for (p, q) in [
        (-dx, a.0 - r[0]),
        (dx, r[2] - a.0),
        (-dy, a.1 - r[1]),
        (dy, r[3] - a.1),
    ] {
        if p.abs() < 1e-12 {
            if q < 0.0 {
                return false;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

/// 点の間の線（`curved` なら縦に出入りする曲線）。
pub fn leg(a: P, b: P, curved: bool) -> Cubic {
    if curved {
        let dy = (b.1 - a.1) * 0.45;
        Cubic(a, (a.0, a.1 + dy), (b.0, b.1 - dy), b)
    } else {
        Cubic::line(a, b)
    }
}

const SAMPLES: usize = 20;

/// 線が箱に入る最初の t（入らなければ None）。
fn first_hit(c: &Cubic, r: &Rect) -> Option<f64> {
    let mut prev = c.0;
    for i in 1..=SAMPLES {
        let t = i as f64 / SAMPLES as f64;
        let p = c.at(t);
        if segment_hits(prev, p, r) {
            return Some(t);
        }
        prev = p;
    }
    None
}

fn dist(a: P, b: P) -> f64 {
    (b.0 - a.0).hypot(b.1 - a.1)
}

impl Obstacles {
    /// `rects` と持ち主から作る。
    pub fn new(rects: Vec<(Rect, Option<usize>)>) -> Self {
        let mut x0 = f64::MAX;
        let mut y0 = f64::MAX;
        let mut x1 = f64::MIN;
        let mut y1 = f64::MIN;
        let mut size = 0.0;
        let rects: Vec<(Rect, Option<usize>)> = rects
            .into_iter()
            .filter(|(r, _)| r.iter().all(|v| v.is_finite()) && r[0] < r[2] && r[1] < r[3])
            .collect();
        for (r, _) in &rects {
            x0 = x0.min(r[0]);
            y0 = y0.min(r[1]);
            x1 = x1.max(r[2]);
            y1 = y1.max(r[3]);
            size += (r[2] - r[0]).max(r[3] - r[1]);
        }
        let n = rects.len();
        if n == 0 {
            return Obstacles {
                rects: vec![],
                owner: vec![],
                x0: 0.0,
                y0: 0.0,
                cell: 1.0,
                cols: 0,
                rows: 0,
                grid: vec![],
                seen: vec![],
                stamp: 0,
                work: 0,
            };
        }
        let span = (x1 - x0).max(y1 - y0).max(1.0);
        let cell = (2.0 * size / n as f64)
            .max(span / MAX_CELLS as f64)
            .max(1.0);
        let cols = (((x1 - x0) / cell).floor() as usize + 1).min(MAX_CELLS);
        let rows = (((y1 - y0) / cell).floor() as usize + 1).min(MAX_CELLS);
        let mut grid = vec![Vec::new(); cols * rows];
        let at = |v: f64, lo: f64, count: usize| {
            ((((v - lo) / cell).floor()).max(0.0) as usize).min(count - 1)
        };
        for (i, (r, _)) in rects.iter().enumerate() {
            for gy in at(r[1], y0, rows)..=at(r[3], y0, rows) {
                for gx in at(r[0], x0, cols)..=at(r[2], x0, cols) {
                    grid[gy * cols + gx].push(i as u32);
                }
            }
        }
        Obstacles {
            owner: rects.iter().map(|(_, o)| *o).collect(),
            rects: rects.into_iter().map(|(r, _)| r).collect(),
            x0,
            y0,
            cell,
            cols,
            rows,
            grid,
            seen: vec![0; n],
            stamp: 0,
            work: WORK,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }

    /// 箱 `b` に重なるかもしれない箱の番号。目が多すぎれば None。
    fn query(&mut self, b: Rect) -> Option<Vec<usize>> {
        if self.rects.is_empty() {
            return Some(vec![]);
        }
        let cell = self.cell;
        let at = |v: f64, lo: f64, count: usize| -> usize {
            (((v - lo) / cell).floor().clamp(0.0, (count - 1) as f64)) as usize
        };
        let (gx0, gx1) = (at(b[0], self.x0, self.cols), at(b[2], self.x0, self.cols));
        let (gy0, gy1) = (at(b[1], self.y0, self.rows), at(b[3], self.y0, self.rows));
        if (gx1 - gx0 + 1) * (gy1 - gy0 + 1) > MAX_SCAN {
            return None;
        }
        self.stamp = self.stamp.wrapping_add(1);
        if self.stamp == 0 {
            self.seen.iter_mut().for_each(|s| *s = 0);
            self.stamp = 1;
        }
        let mut out = Vec::new();
        for gy in gy0..=gy1 {
            for gx in gx0..=gx1 {
                for &i in &self.grid[gy * self.cols + gx] {
                    let i = i as usize;
                    if self.seen[i] != self.stamp {
                        self.seen[i] = self.stamp;
                        let r = &self.rects[i];
                        if r[0] <= b[2] && r[2] >= b[0] && r[1] <= b[3] && r[3] >= b[1] {
                            out.push(i);
                        }
                    }
                }
            }
        }
        Some(out)
    }

    /// 箱 `b` に重なる箱（端のノード `skip` のものを除く）。
    pub fn overlapping(&mut self, b: Rect, skip: &[usize]) -> Vec<Rect> {
        let ids = self.query(b).unwrap_or_default();
        ids.into_iter()
            .filter(|&i| !self.owner[i].is_some_and(|o| skip.contains(&o)))
            .map(|i| self.rects[i])
            .collect()
    }

    /// 線 `c` が最初に入る箱（端の点を含む箱と、`skip` のノードの箱を除く）。
    fn first_obstacle(&mut self, c: &Cubic, skip: &[usize]) -> Option<usize> {
        let xs = [c.0.0, c.1.0, c.2.0, c.3.0];
        let ys = [c.0.1, c.1.1, c.2.1, c.3.1];
        let b = [
            xs.iter().copied().fold(f64::MAX, f64::min),
            ys.iter().copied().fold(f64::MAX, f64::min),
            xs.iter().copied().fold(f64::MIN, f64::max),
            ys.iter().copied().fold(f64::MIN, f64::max),
        ];
        let ids = self.query(b)?;
        let mut best: Option<(f64, usize)> = None;
        for i in ids {
            if self.owner[i].is_some_and(|o| skip.contains(&o)) {
                continue;
            }
            let r = &self.rects[i];
            if contains(r, c.0) || contains(r, c.3) {
                continue;
            }
            self.work = self.work.saturating_sub(SAMPLES);
            if let Some(t) = first_hit(c, r)
                && best.is_none_or(|(bt, _)| t < bt)
            {
                best = Some((t, i));
            }
        }
        best.map(|(_, i)| i)
    }

    /// 点の列 `points` を、箱を避ける回り道に替える。`skip` は辺の端のノード、`curved` は点の間が曲線か。
    pub fn avoid(&mut self, points: &mut Vec<P>, skip: &[usize], curved: bool) {
        if self.rects.is_empty() || points.len() < 2 {
            return;
        }
        let mut i = 0;
        let mut added = 0;
        let mut tries = 0;
        while i + 1 < points.len() && tries < 4 * MAX_DETOURS && self.work > 0 {
            tries += 1;
            let (a, b) = (points[i], points[i + 1]);
            let Some(id) = self.first_obstacle(&leg(a, b, curved), skip) else {
                i += 1;
                continue;
            };
            let r = self.rects[id];
            let corners = [(r[0], r[1]), (r[2], r[1]), (r[2], r[3]), (r[0], r[3])];
            let clear = |p: P, q: P| first_hit(&leg(p, q, curved), &r).is_none();
            self.work = self.work.saturating_sub(12 * 3 * SAMPLES);
            let mut best: Option<(f64, Vec<P>)> = None;
            let mut consider = |via: Vec<P>| {
                let mut prev = a;
                let mut len = 0.0;
                for &p in via.iter().chain(std::iter::once(&b)) {
                    if !clear(prev, p) {
                        return;
                    }
                    len += dist(prev, p);
                    prev = p;
                }
                if best.as_ref().is_none_or(|(l, _)| len < *l) {
                    best = Some((len, via));
                }
            };
            for &c in &corners {
                consider(vec![c]);
            }
            for k in 0..4 {
                consider(vec![corners[k], corners[(k + 1) % 4]]);
                consider(vec![corners[k], corners[(k + 3) % 4]]);
            }
            match best {
                Some((_, via)) if added + via.len() <= MAX_DETOURS => {
                    added += via.len();
                    for (j, p) in via.into_iter().enumerate() {
                        points.insert(i + 1 + j, p);
                    }
                }
                _ => i += 1,
            }
        }
    }
}

/// dot 以外の配置の直角の道: 横・縦・横か縦・横・縦の Z、または L の形のうち、ほかのノードと題の箱に当たる数の
/// いちばん少ないもの（同じなら Z を先に、長いほうの向きから出る）。両端の箱が縦か横に重なれば、その重なりの
/// 中ほどを通る 1 本の直線（端はノードの中から始まり、外形で切られる）。`ha`・`hb` は両端の箱の大きさの半分。
pub fn ortho_free(a: P, b: P, ha: P, hb: P, obstacles: &mut Obstacles, skip: &[usize]) -> Vec<P> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let (mx, my) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    let (lo_x, hi_x) = ((a.0 - ha.0).max(b.0 - hb.0), (a.0 + ha.0).min(b.0 + hb.0));
    let (lo_y, hi_y) = ((a.1 - ha.1).max(b.1 - hb.1), (a.1 + ha.1).min(b.1 + hb.1));
    if dx.abs() < 0.5 || dy.abs() < 0.5 {
        return vec![a, b];
    }
    if dy.abs() >= dx.abs() && hi_x - lo_x > 2.0 {
        let x = (lo_x + hi_x) / 2.0;
        return vec![(x, a.1), (x, b.1)];
    }
    if hi_y - lo_y > 2.0 {
        let y = (lo_y + hi_y) / 2.0;
        return vec![(a.0, y), (b.0, y)];
    }
    let hvh = vec![a, (mx, a.1), (mx, b.1), b];
    let vhv = vec![a, (a.0, my), (b.0, my), b];
    let l1 = vec![a, (b.0, a.1), b];
    let l2 = vec![a, (a.0, b.1), b];
    let candidates = if dx.abs() >= dy.abs() {
        [hvh, vhv, l1, l2]
    } else {
        [vhv, hvh, l2, l1]
    };
    let mut best: Option<(usize, Vec<P>)> = None;
    for c in candidates {
        let hits: usize = c
            .windows(2)
            .map(|w| {
                let r = [
                    w[0].0.min(w[1].0),
                    w[0].1.min(w[1].1),
                    w[0].0.max(w[1].0),
                    w[0].1.max(w[1].1),
                ];
                obstacles.overlapping(r, skip).len()
            })
            .sum();
        if best.as_ref().is_none_or(|(h, _)| hits < *h) {
            best = Some((hits, c));
        }
    }
    best.map(|(_, c)| c).unwrap_or_else(|| vec![a, b])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hits_any(points: &[P], rects: &[Rect], curved: bool) -> bool {
        points.windows(2).any(|w| {
            rects
                .iter()
                .any(|r| first_hit(&leg(w[0], w[1], curved), r).is_some())
        })
    }

    #[test]
    fn goes_around_boxes() {
        let rects = vec![[-40.0, 30.0, 40.0, 50.0], [60.0, 120.0, 120.0, 150.0]];
        for curved in [true, false] {
            let mut obs = Obstacles::new(rects.iter().map(|r| (*r, None)).collect());
            let mut pts = vec![(0.0, 0.0), (0.0, 100.0), (90.0, 200.0)];
            assert!(hits_any(&pts, &rects, curved));
            obs.avoid(&mut pts, &[], curved);
            assert!(!hits_any(&pts, &rects, curved), "{pts:?}");
            assert_eq!(pts.first(), Some(&(0.0, 0.0)));
            assert_eq!(pts.last(), Some(&(90.0, 200.0)));
        }
    }

    #[test]
    fn keeps_the_ends_and_skips_their_nodes() {
        let rects = vec![
            ([-10.0, -10.0, 10.0, 10.0], Some(0)),
            ([-10.0, 90.0, 10.0, 110.0], Some(1)),
        ];
        let mut obs = Obstacles::new(rects);
        let mut pts = vec![(0.0, 0.0), (0.0, 100.0)];
        obs.avoid(&mut pts, &[0, 1], true);
        assert_eq!(pts.len(), 2);
        // 端の点を含む箱は避けられないので、そのまま
        let mut pts = vec![(0.0, 0.0), (0.0, 100.0)];
        obs.avoid(&mut pts, &[], true);
        assert_eq!(pts.len(), 2);
    }

    #[test]
    fn segments_and_boxes() {
        let r = [0.0, 0.0, 10.0, 10.0];
        assert!(segment_hits((-5.0, 5.0), (15.0, 5.0), &r));
        assert!(!segment_hits((-5.0, 0.0), (15.0, 0.0), &r));
        assert!(!segment_hits((-5.0, -5.0), (-1.0, 20.0), &r));
        assert!(segment_hits((5.0, 5.0), (5.0, 6.0), &r));
    }
}

// ---- splines=ortho ----

/// 直角の道筋の入力の 1 本（配置の座標、尾から頭への点）。
pub struct OrthoEdge {
    pub points: Vec<P>,
    pub tail: usize,
    pub head: usize,
    /// 端の点を広げてよいか（ポート・samehead などがなければ）。
    pub spread_tail: bool,
    pub spread_head: bool,
}

/// 直角の道筋（`splines=ortho`）。点の列はランクを 1 つずつ下る（上る）ので、ランクの間の空き（ノードのない帯）
/// で横に動き、ランクの行は仮のノードの位置（ノードのない列）を縦に通る。同じ空きで横の線が重なるときは高さを
/// 分け、クラスタの題を避ける高さを選ぶ。同じノードの同じ側に出入りする線は、ノードの幅の中で端の点を散らす。
/// `nodes` はノードの中心と幅・高さ、`titles` は題の箱。2 つ以上のランクを一度にまたぐ線（仮のノードの上限を
/// 超えたもの）は直角にしない（None）。
pub fn ortho(edges: &[OrthoEdge], nodes: &[(P, f64, f64)], titles: &[Rect]) -> Vec<Option<Vec<P>>> {
    use std::collections::HashMap;
    let key = |y: f64| (y * 64.0).round() as i64;
    // ランクの行の上下（ノードのある行）
    let mut rows: HashMap<i64, (f64, f64)> = HashMap::new();
    for &((_, y), _, h) in nodes {
        let e = rows.entry(key(y)).or_insert((y, y));
        e.0 = e.0.min(y - h / 2.0);
        e.1 = e.1.max(y + h / 2.0);
    }
    let row = |y: f64| rows.get(&key(y)).copied().unwrap_or((y, y));
    // ランクの行の y（ノードの中心）を順に。間に別の行があれば一度にまたぐ線
    let mut centers: Vec<f64> = nodes.iter().map(|n| n.0.1).collect();
    centers.sort_by(f64::total_cmp);
    centers.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    let skips_rank = |a: f64, b: f64| {
        let (lo, hi) = (a.min(b), a.max(b));
        let i = centers.partition_point(|&c| c <= lo + 1e-6);
        centers.get(i).is_some_and(|&c| c < hi - 1e-6)
    };

    let mut pts: Vec<Vec<P>> = edges.iter().map(|e| e.points.clone()).collect();
    // 端の点を散らす: (ノード, 下へ出るか) ごとに、向こうの x の順に
    // (ノード, 下側か) → (向こうの x, 辺, 尾か)
    type Ends = Vec<(f64, usize, bool)>;
    let mut groups: HashMap<(usize, bool), Ends> = HashMap::new();
    for (i, e) in edges.iter().enumerate() {
        let p = &pts[i];
        if p.len() < 2 || e.tail == e.head {
            continue;
        }
        if e.spread_tail {
            groups
                .entry((e.tail, p[1].1 > p[0].1))
                .or_default()
                .push((p[1].0, i, true));
        }
        let n = p.len();
        if e.spread_head {
            groups
                .entry((e.head, p[n - 2].1 > p[n - 1].1))
                .or_default()
                .push((p[n - 2].0, i, false));
        }
    }
    for ((node, _), mut list) in groups {
        if list.len() < 2 {
            continue;
        }
        list.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let w = nodes.get(node).map_or(0.0, |n| n.1);
        let k = list.len();
        let spread = (w * 0.6).min(10.0 * (k - 1) as f64);
        for (j, (_, i, tail)) in list.into_iter().enumerate() {
            let off = -spread / 2.0 + spread * j as f64 / (k - 1) as f64;
            let p = &mut pts[i];
            let at = if tail { 0 } else { p.len() - 1 };
            p[at].0 += off;
        }
    }

    // 横の線（空きの上の行の y、x の範囲、辺、点の番号）
    struct Run {
        gap: i64,
        x0: f64,
        x1: f64,
        edge: usize,
        at: usize,
    }
    let mut runs: Vec<Run> = Vec::new();
    let mut usable = vec![true; edges.len()];
    for (i, p) in pts.iter().enumerate() {
        for (k, w) in p.windows(2).enumerate() {
            let (a, b) = (w[0], w[1]);
            if (a.1 - b.1).abs() < 1e-6 || skips_rank(a.1, b.1) {
                usable[i] = false;
                break;
            }
            if (a.0 - b.0).abs() > 0.5 {
                runs.push(Run {
                    gap: key(a.1.min(b.1)),
                    x0: a.0.min(b.0),
                    x1: a.0.max(b.0),
                    edge: i,
                    at: k,
                });
            }
        }
    }
    runs.retain(|r| usable[r.edge]);
    // 空きごとに、重なる横の線を別の高さ（段）に
    runs.sort_by(|a, b| a.gap.cmp(&b.gap).then(a.x0.total_cmp(&b.x0)));
    let mut track = vec![0usize; runs.len()];
    let mut tracks_in: HashMap<i64, usize> = HashMap::new();
    {
        let mut start = 0;
        while start < runs.len() {
            let gap = runs[start].gap;
            let mut end = start;
            while end < runs.len() && runs[end].gap == gap {
                end += 1;
            }
            let mut ends: Vec<f64> = Vec::new();
            for r in start..end {
                let t = match ends.iter().position(|&e| e < runs[r].x0 - 2.0) {
                    Some(t) => t,
                    None => {
                        ends.push(f64::MIN);
                        ends.len() - 1
                    }
                };
                ends[t] = runs[r].x1;
                track[r] = t;
            }
            tracks_in.insert(gap, ends.len());
            start = end;
        }
    }
    let mut level: HashMap<(usize, usize), f64> = HashMap::new();
    for (r, run) in runs.iter().enumerate() {
        let p = &pts[run.edge];
        let (a, b) = (p[run.at], p[run.at + 1]);
        let (upper, lower) = if a.1 < b.1 { (a, b) } else { (b, a) };
        let g0 = row(upper.1).1;
        let g1 = row(lower.1).0;
        // 端の縦の線に矢じりの分（12）を残す
        let (g0, g1) = if g1 - g0 < 4.0 {
            let m = (upper.1 + lower.1) / 2.0;
            (m - 2.0, m + 2.0)
        } else if g1 - g0 > 30.0 {
            (g0 + 12.0, g1 - 12.0)
        } else {
            let m = (g0 + g1) / 2.0;
            (m - 3.0, m + 3.0)
        };
        let count = tracks_in.get(&run.gap).copied().unwrap_or(1);
        let want = g0 + (g1 - g0) * (track[r] + 1) as f64 / (count + 1) as f64;
        // 題を避ける高さ（望む高さから近い順に）。調べるのはこの線の範囲に重なる題だけ
        let (lo, hi) = (a.0.min(b.0), a.0.max(b.0));
        let (top, bottom) = (upper.1, lower.1);
        let near: Vec<&Rect> = titles
            .iter()
            .filter(|r| r[0] <= hi && r[2] >= lo && r[1] <= bottom && r[3] >= top)
            .collect();
        let hits_title = |p: P, q: P| near.iter().any(|r| segment_hits(p, q, r));
        let clear = |y: f64| {
            !hits_title(a, (a.0, y)) && !hits_title((a.0, y), (b.0, y)) && !hits_title((b.0, y), b)
        };
        let mut y = want;
        if !clear(want) {
            let steps = ((g1 - g0) / 2.0).ceil().clamp(1.0, 400.0) as usize;
            let mut found = None;
            for s in 1..=steps {
                for sign in [-1.0, 1.0] {
                    let c = want + sign * 2.0 * s as f64;
                    if c > g0 && c < g1 && clear(c) {
                        found = Some(c);
                        break;
                    }
                }
                if found.is_some() {
                    break;
                }
            }
            y = found.unwrap_or(want);
        }
        level.insert((run.edge, run.at), y);
    }
    pts.iter()
        .enumerate()
        .map(|(i, p)| {
            if !usable[i] || p.len() < 2 {
                return None;
            }
            let mut out = vec![p[0]];
            for (k, w) in p.windows(2).enumerate() {
                let (a, b) = (w[0], w[1]);
                if let Some(&y) = level.get(&(i, k)) {
                    out.push((a.0, y));
                    out.push((b.0, y));
                }
                out.push(b);
            }
            // 一直線に並ぶ中の点を除く
            let mut clean: Vec<P> = Vec::with_capacity(out.len());
            for q in out {
                if clean
                    .last()
                    .is_some_and(|l: &P| (l.0 - q.0).abs() < 1e-6 && (l.1 - q.1).abs() < 1e-6)
                {
                    continue;
                }
                if clean.len() >= 2 {
                    let (s, t) = (clean[clean.len() - 2], clean[clean.len() - 1]);
                    let vertical = (s.0 - t.0).abs() < 1e-6 && (t.0 - q.0).abs() < 1e-6;
                    let horizontal = (s.1 - t.1).abs() < 1e-6 && (t.1 - q.1).abs() < 1e-6;
                    if vertical || horizontal {
                        clean.pop();
                    }
                }
                clean.push(q);
            }
            Some(clean)
        })
        .collect()
}

#[cfg(test)]
mod ortho_tests {
    use super::*;

    fn axis_aligned(p: &[P]) -> bool {
        p.windows(2)
            .all(|w| (w[0].0 - w[1].0).abs() < 1e-6 || (w[0].1 - w[1].1).abs() < 1e-6)
    }

    #[test]
    fn routes_with_right_angles_and_separate_tracks() {
        // 0 が上、1・2 が下。0→1、0→2、それに 3→2（3 は上の行の右）
        let nodes = vec![
            ((0.0, 0.0), 54.0, 36.0),
            ((-60.0, 100.0), 54.0, 36.0),
            ((60.0, 100.0), 54.0, 36.0),
            ((120.0, 0.0), 54.0, 36.0),
        ];
        let e = |t: usize, h: usize| OrthoEdge {
            points: vec![nodes[t].0, nodes[h].0],
            tail: t,
            head: h,
            spread_tail: true,
            spread_head: true,
        };
        let edges = vec![e(0, 1), e(0, 2), e(3, 2)];
        let out = ortho(&edges, &nodes, &[]);
        let mut levels = Vec::new();
        for p in &out {
            let p = p.as_ref().unwrap();
            assert!(axis_aligned(p), "{p:?}");
            // 横の線は行の間の空き（18..82）
            for w in p.windows(2) {
                if (w[0].1 - w[1].1).abs() < 1e-6 {
                    assert!(w[0].1 > 18.0 && w[0].1 < 82.0);
                    levels.push((w[0].1, w[0].0.min(w[1].0), w[0].0.max(w[1].0)));
                }
            }
        }
        // 重なる横の線は高さが違う
        for (i, a) in levels.iter().enumerate() {
            for b in &levels[i + 1..] {
                if a.1 < b.2 && b.1 < a.2 {
                    assert!((a.0 - b.0).abs() > 1.0, "{levels:?}");
                }
            }
        }
        // 同じノードから出る 2 本は出る点が違う
        let (a, b) = (out[0].as_ref().unwrap()[0], out[1].as_ref().unwrap()[0]);
        assert!(a.0 < b.0);
        // 題を避ける
        let title = [[-100.0, 40.0, -30.0, 60.0]];
        let one = vec![OrthoEdge {
            points: vec![(0.0, 0.0), (-60.0, 100.0)],
            tail: 0,
            head: 1,
            spread_tail: false,
            spread_head: false,
        }];
        let p = ortho(&one, &nodes, &title)[0].clone().unwrap();
        assert!(
            !p.windows(2).any(|w| segment_hits(w[0], w[1], &title[0])),
            "{p:?}"
        );
    }
}
