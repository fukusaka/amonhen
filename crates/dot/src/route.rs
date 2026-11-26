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
    /// 箱の持ち主（ノードの番号、クラスタの題はノードの数 + クラスタの番号、辺のラベルは None）。辺は `skip` の
    /// 持ち主の箱（端のノードと、そのクラスタの題）を避けない。
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
    /// 直角の道で脇を回る形を試す残りの手間（格子の目と箱の数）。
    around: usize,
}

/// 格子の目の数の上限（片側）。
const MAX_CELLS: usize = 256;
/// 1 つの線で引く目の上限（これを超える長い線は避けない）。
const MAX_SCAN: usize = 4096;
/// 1 本の辺に足す回り道の点の上限。
const MAX_DETOURS: usize = 12;
/// 図ぜんぶで、線分と箱を比べる回数の上限（込んだ大きなグラフの時間を抑える）。
const WORK: usize = 4_000_000;
/// 図ぜんぶで、直角の道の脇を回る形を試すときの手間（引く格子の目と、目に入る箱の数）の上限。
const AROUND_WORK: usize = 150_000;

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
                around: 0,
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
            around: AROUND_WORK,
        }
    }

    /// 箱 `b` を引く手間（覆う格子の目と、目に入る箱の数）。
    fn cells(&self, b: Rect) -> usize {
        if self.rects.is_empty() {
            return 0;
        }
        let at = |v: f64, lo: f64, count: usize| -> usize {
            (((v - lo) / self.cell)
                .floor()
                .clamp(0.0, (count - 1) as f64)) as usize
        };
        let (gx0, gx1) = (at(b[0], self.x0, self.cols), at(b[2], self.x0, self.cols));
        let (gy0, gy1) = (at(b[1], self.y0, self.rows), at(b[3], self.y0, self.rows));
        let count = (gx1 - gx0 + 1) * (gy1 - gy0 + 1);
        if count > MAX_SCAN {
            return count;
        }
        (gy0..=gy1)
            .flat_map(|gy| (gx0..=gx1).map(move |gx| gy * self.cols + gx))
            .map(|i| 1 + self.grid[i].len())
            .sum()
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

    /// 箱 `b` にほかの箱（`skip` のノードのものを除く）が重なるか。調べきれないとき・手間が尽きたときも重なると
    /// 答える（変えずにおく側に倒す）。
    pub fn blocked(&mut self, b: Rect, skip: &[usize]) -> bool {
        if self.rects.is_empty() {
            return false;
        }
        if self.work == 0 {
            crate::cut_short();
            return true;
        }
        let Some(ids) = self.query(b) else {
            return true;
        };
        self.work = self.work.saturating_sub(ids.len() + 1);
        ids.into_iter()
            .any(|i| !self.owner[i].is_some_and(|o| skip.contains(&o)))
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
        if self.work == 0 && i + 1 < points.len() {
            crate::cut_short();
        }
    }
}

/// 脇を回る Z を試す、間の箱の数の上限と、箱から離す間。
const AROUND: usize = 6;
const AROUND_GAP: f64 = 6.0;

/// dot 以外の配置の直角の道: 横・縦・横か縦・横・縦の Z、または L の形、それに真ん中の線を間の箱の脇へずらした Z
/// のうち、ほかのノードと題の箱に当たる数のいちばん少ないもの（同じなら Z を先に、長いほうの向きから出る）。両端の箱が縦か横に重なれば、その重なりの
/// 中ほどを通る 1 本の直線（端はノードの中から始まり、外形で切られる）。`ha`・`hb` は両端の箱の大きさの半分。
pub fn ortho_free(a: P, b: P, ha: P, hb: P, obstacles: &mut Obstacles, skip: &[usize]) -> Vec<P> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let (mx, my) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    let (lo_x, hi_x) = ((a.0 - ha.0).max(b.0 - hb.0), (a.0 + ha.0).min(b.0 + hb.0));
    let (lo_y, hi_y) = ((a.1 - ha.1).max(b.1 - hb.1), (a.1 + ha.1).min(b.1 + hb.1));
    // ほぼ縦・横の線は、ずれの真ん中にそろえる（斜めの線を残さない）
    if dx.abs() < 0.5 {
        return vec![(mx, a.1), (mx, b.1)];
    }
    if dy.abs() < 0.5 {
        return vec![(a.0, my), (b.0, my)];
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
    let candidates: Vec<Vec<P>> = if dx.abs() >= dy.abs() {
        vec![hvh, vhv, l1, l2]
    } else {
        vec![vhv, hvh, l2, l1]
    };
    let hits_of = |c: &[P], obstacles: &mut Obstacles| -> usize {
        c.windows(2)
            .map(|w| {
                let r = [
                    w[0].0.min(w[1].0),
                    w[0].1.min(w[1].1),
                    w[0].0.max(w[1].0),
                    w[0].1.max(w[1].1),
                ];
                let n = obstacles.overlapping(r, skip).len();
                obstacles.work = obstacles.work.saturating_sub(n + 1);
                n
            })
            .sum()
    };
    let mut best: Option<(usize, Vec<P>)> = None;
    for c in candidates {
        let hits = hits_of(&c, obstacles);
        if best.as_ref().is_none_or(|(h, _)| hits < *h) {
            best = Some((hits, c));
        }
        if hits == 0 {
            break;
        }
    }
    // どれも当たるなら、間のノード・題の脇を通る Z（真ん中の線を箱の外へずらしたもの）。両端のノードの外にある所
    // だけ。手間の上限が尽きたら試さない
    let blocked = best.as_ref().is_some_and(|(h, _)| *h > 0);
    if blocked && (obstacles.work == 0 || obstacles.around == 0) {
        crate::cut_short();
    }
    if blocked && obstacles.work > 0 && obstacles.around > 0 {
        let bb = [a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1)];
        obstacles.around = obstacles.around.saturating_sub(obstacles.cells(bb));
        let between = obstacles.overlapping(bb, skip);
        obstacles.work = obstacles.work.saturating_sub(between.len() + 1);
        let (gx0, gx1) = if a.0 < b.0 {
            (a.0 + ha.0, b.0 - hb.0)
        } else {
            (b.0 + hb.0, a.0 - ha.0)
        };
        let (gy0, gy1) = if a.1 < b.1 {
            (a.1 + ha.1, b.1 - hb.1)
        } else {
            (b.1 + hb.1, a.1 - ha.1)
        };
        let mut more: Vec<Vec<P>> = Vec::new();
        for r in between.iter().take(AROUND) {
            for x in [r[0] - AROUND_GAP, r[2] + AROUND_GAP] {
                if x > gx0 && x < gx1 {
                    more.push(vec![a, (x, a.1), (x, b.1), b]);
                }
            }
            for y in [r[1] - AROUND_GAP, r[3] + AROUND_GAP] {
                if y > gy0 && y < gy1 {
                    more.push(vec![a, (a.0, y), (b.0, y), b]);
                }
            }
        }
        for c in more {
            if obstacles.around == 0 {
                crate::cut_short();
                break;
            }
            for w in c.windows(2) {
                let r = [
                    w[0].0.min(w[1].0),
                    w[0].1.min(w[1].1),
                    w[0].0.max(w[1].0),
                    w[0].1.max(w[1].1),
                ];
                obstacles.around = obstacles.around.saturating_sub(obstacles.cells(r));
            }
            let hits = hits_of(&c, obstacles);
            if best.as_ref().is_none_or(|(h, _)| hits < *h) {
                best = Some((hits, c));
            }
            if hits == 0 {
                break;
            }
        }
    }
    best.map(|(_, c)| c).unwrap_or_else(|| vec![a, b])
}

/// dot 以外の配置の直角の道の 1 本（両端のノードの中心と大きさの半分、ノードの番号）。
pub struct FreeEdge {
    pub a: P,
    pub b: P,
    pub ha: P,
    pub hb: P,
    pub tail: usize,
    pub head: usize,
    /// 避けない箱の持ち主（両端のノードと、そのクラスタの題）。
    pub skip: Vec<usize>,
}

/// 同じ溝（同じ向きで、この距離より近い平行な線）とみなす間。
const CHANNEL: f64 = 4.0;
/// 溝の中で分けた線の間。
pub const TRACK: f64 = 6.0;
/// 溝を分けるときに比べる組の数の上限（図ぜんぶ）。
const NUDGE_WORK: usize = 2_000_000;

/// dot 以外の配置の直角の道をまとめて引く。1 本ずつの形は `ortho_free` で選び、それから
///
/// - 端を散らす: 同じノードの同じ側から出入りする端を、その側の長さの 7 割に等しい間で並べる（向こうの順）。
/// - 溝を分ける: 中ほどの線（Z の真ん中）が別の辺の中ほどの線と同じ溝で重なれば、並べて間を空ける。
///
/// 端を散らしてから溝を分ける考えは、直角のコネクタの道を引いてから「nudging」で平行な線を離す
/// Wybrow・Marriott・Stuckey（2009）の手順にならう（線どうしの順は簡単な決まりで、最適化はしない）。
pub fn ortho_free_all(edges: &[FreeEdge], obstacles: &mut Obstacles) -> Vec<Vec<P>> {
    use std::collections::HashMap;
    // 直線は 4 点（端、真ん中に長さ 0 の線、端）にする（両端を別々に散らすと真ん中が段になる）。L は 3 点の
    // まま（曲がりの x と y を両端が 1 つずつ決める）
    let mut routes: Vec<Vec<P>> = edges
        .iter()
        .map(|e| {
            let p = ortho_free(e.a, e.b, e.ha, e.hb, obstacles, &e.skip);
            match p.len() {
                2 => {
                    let m = ((p[0].0 + p[1].0) / 2.0, (p[0].1 + p[1].1) / 2.0);
                    vec![p[0], m, m, p[1]]
                }
                _ => p,
            }
        })
        .collect();
    let horizontal = |a: P, b: P| (a.1 - b.1).abs() < 1e-9 && (a.0 - b.0).abs() > 1e-9;
    // 端の側: 最初（最後）の線の向きと、ノードの中心から見た向き
    // (ノード, 側) → (並べる値, 辺, 尾か)
    type Ends = Vec<(f64, usize, bool)>;
    let mut groups: HashMap<(usize, Side), Ends> = HashMap::new();
    for (i, (e, p)) in edges.iter().zip(&routes).enumerate() {
        let n = p.len();
        if n < 3 || e.tail == e.head {
            continue;
        }
        for (tail, v, c, near, far) in [
            (true, e.tail, e.a, p[1], e.b),
            (false, e.head, e.b, p[n - 2], e.a),
        ] {
            let end = if tail { p[0] } else { p[n - 1] };
            let side = if horizontal(end, near) {
                if near.0 > c.0 { Side::E } else { Side::W }
            } else if (end.0 - near.0).abs() < 1e-9 && (end.1 - near.1).abs() > 1e-9 {
                if near.1 > c.1 { Side::S } else { Side::N }
            } else {
                continue;
            };
            // E・W の側は向こうの y の順、N・S の側は x の順
            let key = if matches!(side, Side::E | Side::W) {
                far.1
            } else {
                far.0
            };
            groups.entry((v, side)).or_default().push((key, i, tail));
        }
    }
    for ((v, side), mut list) in groups {
        if list.len() < 2 {
            continue;
        }
        list.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        let e0 = &edges[list[0].1];
        let (c, half) = if e0.tail == v && list[0].2 {
            (e0.a, e0.ha)
        } else {
            (e0.b, e0.hb)
        };
        let vertical_side = matches!(side, Side::N | Side::S);
        let avail = if vertical_side { half.0 } else { half.1 } * 2.0 * 0.7;
        let k = list.len() as f64;
        for (j, (_, i, tail)) in list.into_iter().enumerate() {
            let at = c_along(c, vertical_side) - avail / 2.0 + avail * (j as f64 + 0.5) / k;
            let p = &mut routes[i];
            let n = p.len();
            let (x, y) = if tail { (0, 1) } else { (n - 1, n - 2) };
            if vertical_side {
                p[x].0 = at;
                p[y].0 = at;
            } else {
                p[x].1 = at;
                p[y].1 = at;
            }
        }
    }
    // 中ほどの線（p[1]→p[2]）を溝ごとに分ける
    struct Mid {
        vertical: bool,
        at: f64,
        lo: f64,
        hi: f64,
        edge: usize,
    }
    let mut mids: Vec<Mid> = routes
        .iter()
        .enumerate()
        .filter(|(_, p)| p.len() == 4)
        .filter_map(|(i, p)| {
            let (a, b) = (p[1], p[2]);
            if (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() > 1e-9 {
                Some(Mid {
                    vertical: true,
                    at: a.0,
                    lo: a.1.min(b.1),
                    hi: a.1.max(b.1),
                    edge: i,
                })
            } else if horizontal(a, b) {
                Some(Mid {
                    vertical: false,
                    at: a.1,
                    lo: a.0.min(b.0),
                    hi: a.0.max(b.0),
                    edge: i,
                })
            } else {
                None
            }
        })
        .collect();
    mids.sort_by(|a, b| {
        a.vertical
            .cmp(&b.vertical)
            .then(a.at.total_cmp(&b.at))
            .then(a.edge.cmp(&b.edge))
    });
    // 近くて重なる線を 1 つの溝に（union-find）
    let mut parent: Vec<usize> = (0..mids.len()).collect();
    fn find(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }
    let mut work = NUDGE_WORK;
    for i in 0..mids.len() {
        for j in i + 1..mids.len() {
            if mids[j].vertical != mids[i].vertical || mids[j].at - mids[i].at >= CHANNEL {
                break;
            }
            if work == 0 {
                crate::cut_short();
                break;
            }
            work -= 1;
            if mids[j].lo < mids[i].hi && mids[i].lo < mids[j].hi {
                let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                parent[a.max(b)] = a.min(b);
            }
        }
    }
    let mut channels: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..mids.len() {
        let r = find(&mut parent, i);
        channels.entry(r).or_default().push(i);
    }
    for (_, list) in channels {
        if list.len() < 2 {
            continue;
        }
        let mean = list.iter().map(|&i| mids[i].at).sum::<f64>() / list.len() as f64;
        // 線の順: 並びの始め（lo）の順。始めが同じなら辺の順
        let mut order = list;
        order.sort_by(|&a, &b| {
            mids[a]
                .lo
                .total_cmp(&mids[b].lo)
                .then(mids[a].edge.cmp(&mids[b].edge))
        });
        let k = order.len() as f64;
        for (j, i) in order.into_iter().enumerate() {
            let at = mean + (j as f64 - (k - 1.0) / 2.0) * TRACK;
            let m = &mids[i];
            let p = &mut routes[m.edge];
            if m.vertical {
                p[1].0 = at;
                p[2].0 = at;
            } else {
                p[1].1 = at;
                p[2].1 = at;
            }
        }
    }
    routes.into_iter().map(tidy).collect()
}

/// 中心の、側に沿った座標（N・S の側なら x、E・W なら y）。
fn c_along(c: P, vertical_side: bool) -> f64 {
    if vertical_side { c.0 } else { c.1 }
}

#[cfg(test)]
mod free_tests {
    use super::*;

    #[test]
    fn goes_beside_a_node_in_the_way() {
        // 0 から右下の 1 へ。真ん中に 2、左下に 3 があり、真ん中で曲がる Z も L も当たる
        let rects = vec![
            ([-27.0, -18.0, 27.0, 18.0], Some(0)),
            ([273.0, 182.0, 327.0, 218.0], Some(1)),
            ([110.0, -40.0, 190.0, 150.0], Some(2)),
            ([60.0, 185.0, 100.0, 215.0], Some(3)),
        ];
        let mut obs = Obstacles::new(rects);
        let p = ortho_free(
            (0.0, 0.0),
            (300.0, 200.0),
            (27.0, 18.0),
            (27.0, 18.0),
            &mut obs,
            &[0, 1],
        );
        for r in [[110.0, -40.0, 190.0, 150.0], [60.0, 185.0, 100.0, 215.0]] {
            assert!(!p.windows(2).any(|w| segment_hits(w[0], w[1], &r)), "{p:?}");
        }
    }

    fn axis_aligned(p: &[P]) -> bool {
        p.windows(2)
            .all(|w| (w[0].0 - w[1].0).abs() < 1e-6 || (w[0].1 - w[1].1).abs() < 1e-6)
    }

    #[test]
    fn spreads_ends_and_separates_channels() {
        // 中心の 0 から右上・右・右下の 1・2・3 へ、それに 0 から 2 へもう 1 本
        let half = (27.0, 18.0);
        let c = (0.0, 0.0);
        let others = [(200.0, -150.0), (200.0, 0.0), (200.0, 150.0), (200.0, 0.0)];
        let edges: Vec<FreeEdge> = others
            .iter()
            .enumerate()
            .map(|(i, &b)| FreeEdge {
                a: c,
                b,
                ha: half,
                hb: half,
                tail: 0,
                head: [1, 2, 3, 2][i],
                skip: vec![0, [1, 2, 3, 2][i]],
            })
            .collect();
        let out = ortho_free_all(&edges, &mut Obstacles::new(vec![]));
        for p in &out {
            assert!(axis_aligned(p), "{p:?}");
        }
        // 0 から出る端はすべて違う点
        let starts: Vec<P> = out.iter().map(|p| p[0]).collect();
        for (i, a) in starts.iter().enumerate() {
            for b in &starts[i + 1..] {
                assert!((a.0 - b.0).abs() + (a.1 - b.1).abs() > 1.0, "{starts:?}");
            }
        }
        // 縦の中ほどの線は、重なれば間が空く
        let mut verticals: Vec<(f64, f64, f64)> = Vec::new();
        for p in &out {
            for w in p.windows(2) {
                if (w[0].0 - w[1].0).abs() < 1e-6 && (w[0].1 - w[1].1).abs() > 1e-6 {
                    verticals.push((w[0].0, w[0].1.min(w[1].1), w[0].1.max(w[1].1)));
                }
            }
        }
        for (i, a) in verticals.iter().enumerate() {
            for b in &verticals[i + 1..] {
                if a.1 < b.2 && b.1 < a.2 {
                    assert!((a.0 - b.0).abs() > 1.0, "{verticals:?}");
                }
            }
        }
    }
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

/// 直角の道の端が出入りするノードの側（配置の座標。N が上）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Side {
    N,
    S,
    E,
    W,
}

/// 直角の道の端のポート（配置の座標。ノードの中心からの相対）。
#[derive(Clone, Debug, PartialEq)]
pub enum OrthoPort {
    /// ポートなし: 行き先の側（上か下）から、同じ側のほかの端と散らして出る。
    Free,
    /// 向きのあるポート（`n`・`se` など）: 外形の点と、その向き。
    Point { at: P, dir: P },
    /// 向きのない升・欄のポート: 升の箱。ノードの外形に触れる辺から出る。
    Cell { b: Rect },
}

/// 直角の道の端。
#[derive(Clone, Debug)]
pub struct OrthoEnd {
    pub port: OrthoPort,
    /// samehead・sametail の値。同じ値の端は 1 つの点で会う（散らすときも 1 つの席）。
    pub same: Option<String>,
}

impl OrthoEnd {
    pub fn free() -> Self {
        OrthoEnd {
            port: OrthoPort::Free,
            same: None,
        }
    }
}

/// 直角の道筋の入力の 1 本（配置の座標、尾から頭への点）。
pub struct OrthoEdge {
    pub points: Vec<P>,
    pub tail: usize,
    pub head: usize,
    pub tail_end: OrthoEnd,
    pub head_end: OrthoEnd,
    /// 仮のノードの位置を動かさない（ラベルが仮のノードの脇にある線）。
    pub keep: bool,
    /// 避けない箱の持ち主（両端のノードと、そのクラスタの題）。
    pub skip: Vec<usize>,
}

fn opposite(s: Side) -> Side {
    match s {
        Side::N => Side::S,
        Side::S => Side::N,
        Side::E => Side::W,
        Side::W => Side::E,
    }
}

/// 端の側と、側に沿ったずれ（N・S なら x、E・W なら y。ノードの中心から）。ポートなしは None。`natural` は
/// 行き先の側（N か S）、`toward` は行き先の x（ノードの中心から）。`half` はノードの大きさの半分。
fn resolve(port: &OrthoPort, half: P, natural: Side, toward: f64) -> Option<(Side, f64)> {
    match port {
        OrthoPort::Free => None,
        OrthoPort::Point { at, dir } => {
            let vertical = if dir.1 < 0.0 { Side::N } else { Side::S };
            let horizontal = if dir.0 < 0.0 { Side::W } else { Side::E };
            let (ax, ay) = (dir.0.abs(), dir.1.abs());
            // 斜め（`ne` など）は、縦の向きが行き先と合えば縦の側、合わなければ横の側
            let side = if ay > ax + 1e-6 || (ax <= ay + 1e-6 && vertical == natural) {
                vertical
            } else {
                horizontal
            };
            let along = if matches!(side, Side::N | Side::S) {
                at.0
            } else {
                at.1
            };
            Some((side, along))
        }
        OrthoPort::Cell { b } => {
            let touches = |s: Side| match s {
                Side::N => b[1] <= -half.1 + 1.0,
                Side::S => b[3] >= half.1 - 1.0,
                Side::W => b[0] <= -half.0 + 1.0,
                Side::E => b[2] >= half.0 - 1.0,
            };
            let lateral = if toward >= 0.0 {
                [Side::E, Side::W]
            } else {
                [Side::W, Side::E]
            };
            // 行き先の側、横の側（行き先に近いほうから）、反対の側の順に、外形に触れる辺。どれもなければ行き先の側
            let side = [natural, lateral[0], lateral[1], opposite(natural)]
                .into_iter()
                .find(|&s| touches(s))
                .unwrap_or(natural);
            let along = if matches!(side, Side::N | Side::S) {
                (b[0] + b[2]) / 2.0
            } else {
                (b[1] + b[3]) / 2.0
            };
            Some((side, along))
        }
    }
}

/// 端の短い道（ノードの中から外へ）。最後の点から、行き先の側（`natural`）へ縦に進めば、自分のノードを
/// 横切らない。`c` はノードの中心、`along` は側に沿ったずれ、`toward` は行き先の x（絵の座標）、`m` はノードから
/// 左・右へ離れる長さ（上下は `STUB`）。外形で切るのは描く側（中の点から始める）。
fn stub(c: P, half: P, side: Side, along: f64, natural: Side, toward: f64, m: P) -> Vec<P> {
    let ax = along.clamp(-half.0 * 0.95, half.0 * 0.95);
    let ay = along.clamp(-half.1 * 0.95, half.1 * 0.95);
    match side {
        s if s == natural => vec![(c.0 + ax, c.1)],
        Side::N | Side::S => {
            // 行き先と反対の側: いったん出てから、行き先の x の側の脇へ
            let y = if side == Side::N {
                c.1 - half.1 - STUB
            } else {
                c.1 + half.1 + STUB
            };
            let sx = if toward >= c.0 {
                c.0 + half.0 + m.1
            } else {
                c.0 - half.0 - m.0
            };
            vec![(c.0 + ax, c.1), (c.0 + ax, y), (sx, y)]
        }
        Side::E => vec![(c.0, c.1 + ay), (c.0 + half.0 + m.1, c.1 + ay)],
        Side::W => vec![(c.0, c.1 + ay), (c.0 - half.0 - m.0, c.1 + ay)],
    }
}

/// 端の短い道がノードから離れる長さの既定（矢じり 10 と少し）。
pub const STUB: f64 = 14.0;
/// 同じ側に並べる端の間の下限（これより込めば広げ、横の側へも回す）。
const END_GAP: f64 = 8.0;
/// 横の側から出入りするのに要る、隣のノードとの間（ノードから離れる長さ）。
const SIDE_ROOM: f64 = 8.0;

/// 一直線に並ぶ中の点と、重なる点を除く。
fn tidy(out: Vec<P>) -> Vec<P> {
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
    clean
}

/// 同じランクの辺の直角の道（上を越える）。`a`・`b` は両端の中心、`ha`・`hb` は大きさの半分、`top` は越える高さ、
/// `m` はノードから離れる長さ。ポートの側が上でなければ、その側から出て脇を上る。
#[allow(clippy::too_many_arguments)]
pub fn ortho_flat(
    a: P,
    ha: P,
    pa: &OrthoPort,
    b: P,
    hb: P,
    pb: &OrthoPort,
    top: f64,
    m: f64,
) -> Vec<P> {
    let end = |c: P, half: P, port: &OrthoPort, other: f64| {
        let (side, along) = resolve(port, half, Side::N, other - c.0).unwrap_or((Side::N, 0.0));
        stub(c, half, side, along, Side::N, other, (m, m))
    };
    let sa = end(a, ha, pa, b.0);
    let sb = end(b, hb, pb, a.0);
    let (la, lb) = (sa[sa.len() - 1], sb[sb.len() - 1]);
    let top = top.min(la.1 - m).min(lb.1 - m);
    let mut out = sa;
    out.push((la.0, top));
    out.push((lb.0, top));
    out.extend(sb.into_iter().rev());
    tidy(out)
}

/// 仮のノードの位置の都合の回り道を減らす: 点の列 `p`（尾から頭、ランクごとに 1 点）の中の点を、尾の x の縦の線と
/// 頭の x の縦の線に寄せ、曲がるのを 1 か所にする。曲がる所は、もとの道のいちばん大きな横の動きにいちばん近い
/// ランクの間で、縦の線がほかのノード・題の箱に当たらないもの。どこも当たるなら変えない。
fn straighten(p: &mut [P], obstacles: &mut Obstacles, skip: &[usize]) {
    let n = p.len();
    if n <= 2 {
        return;
    }
    let (xs, xe) = (p[0].0, p[n - 1].0);
    let mut clear = |x: f64, y0: f64, y1: f64| {
        !obstacles.blocked([x - 0.5, y0.min(y1), x + 0.5, y0.max(y1)], skip)
    };
    // ok_s[k]: 尾から k 番目の点まで xs の縦で通れる。ok_e[k]: k 番目から頭まで xe の縦で通れる
    let mut ok_s = vec![false; n];
    ok_s[0] = true;
    for k in 1..n - 1 {
        if !clear(xs, p[k - 1].1, p[k].1) {
            break;
        }
        ok_s[k] = true;
    }
    let mut ok_e = vec![false; n];
    ok_e[n - 1] = true;
    for k in (1..n - 1).rev() {
        if !clear(xe, p[k].1, p[k + 1].1) {
            break;
        }
        ok_e[k] = true;
    }
    let widest = (0..n - 1)
        .max_by(|&a, &b| {
            let da = (p[a + 1].0 - p[a].0).abs();
            let db = (p[b + 1].0 - p[b].0).abs();
            da.total_cmp(&db).then(b.cmp(&a))
        })
        .unwrap_or(0);
    let mut order: Vec<usize> = (0..n - 1).filter(|&g| ok_s[g] && ok_e[g + 1]).collect();
    order.sort_by_key(|&g| (g.abs_diff(widest), g));
    for g in order {
        let mid = (p[g].1 + p[g + 1].1) / 2.0;
        if clear(xs, p[g].1, mid) && clear(xe, mid, p[g + 1].1) {
            for q in &mut p[1..=g] {
                q.0 = xs;
            }
            for q in &mut p[g + 1..n - 1] {
                q.0 = xe;
            }
            return;
        }
    }
}

/// 直角の道筋（`splines=ortho`）。点の列はランクを 1 つずつ下る（上る）ので、ランクの間の空き（ノードのない帯）
/// で横に動き、ランクの行は仮のノードの位置（ノードのない列）を縦に通る。同じ空きで横の線が重なるときは高さを
/// 分け、クラスタの題を避ける高さを選ぶ。
///
/// 端: ポートがあればその側（向きのあるポートはその向き、升・欄はノードの外形に触れる辺）の、その位置から出る。
/// 行き先と別の側なら、ノードの脇を回ってから進む。ポートのない端は、行き先の側（上か下）に出入りする同じノードの
/// ほかの端と、ノードの幅の中に等しい間で並べる（向こうの x の順。samehead・sametail の組は 1 つの席を分け合う）。
/// 中の点は、ほかのノードに当たらなければ尾と頭の縦の線に寄せる（`straighten`）。
///
/// `nodes` はノードの中心と幅・高さ、`titles` は題の箱とその持ち主、`obstacles` はほかのノードと題の箱。2 つ以上のランクを
/// 一度にまたぐ線（仮のノードの上限を超えたもの）は直角にしない（None）。
pub fn ortho(
    edges: &[OrthoEdge],
    nodes: &[(P, f64, f64)],
    titles: &[(Rect, usize)],
    obstacles: &mut Obstacles,
) -> Vec<Option<Vec<P>>> {
    use std::collections::HashMap;
    let key = |y: f64| (y * 64.0).round() as i64;
    // ランクの行の上下（ノードのある行）
    let mut rows: HashMap<i64, (f64, f64)> = HashMap::new();
    let mut by_row: HashMap<i64, Vec<usize>> = HashMap::new();
    for (i, &((_, y), _, h)) in nodes.iter().enumerate() {
        let e = rows.entry(key(y)).or_insert((y, y));
        e.0 = e.0.min(y - h / 2.0);
        e.1 = e.1.max(y + h / 2.0);
        by_row.entry(key(y)).or_default().push(i);
    }
    let row = |y: f64| rows.get(&key(y)).copied().unwrap_or((y, y));
    // ノードの脇の空き（同じ行の隣のノードまで）から、端の短い道がノードから離れる長さ（矢じりの分を残す）
    let mut margin = vec![(STUB, STUB); nodes.len()];
    for list in by_row.values_mut() {
        list.sort_by(|&a, &b| nodes[a].0.0.total_cmp(&nodes[b].0.0).then(a.cmp(&b)));
        for w in list.windows(2) {
            let (a, b) = (nodes[w[0]], nodes[w[1]]);
            let gap = (b.0.0 - b.1 / 2.0) - (a.0.0 + a.1 / 2.0);
            let m = (gap * 0.45).clamp(2.0, STUB);
            margin[w[0]].1 = margin[w[0]].1.min(m);
            margin[w[1]].0 = margin[w[1]].0.min(m);
        }
    }
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
    let half = |v: usize| nodes.get(v).map_or((0.0, 0.0), |n| (n.1 / 2.0, n.2 / 2.0));
    let center = |v: usize| nodes.get(v).map_or((0.0, 0.0), |n| n.0);
    // 端ごとの (側, ずれ)。ポートのない端は散らしてから決める
    // (側, ずれ, 行き先の側, 行き先の x)
    type End = Option<(Side, f64, Side, f64)>;
    let mut ends: Vec<[End; 2]> = vec![[None, None]; edges.len()];
    // (ノード, 側) → (向こうの x, samehead などの値, 辺, 尾か)
    type Free<'a> = Vec<(f64, Option<&'a str>, usize, bool)>;
    let mut groups: HashMap<(usize, Side), Free> = HashMap::new();
    for (i, e) in edges.iter().enumerate() {
        let p = &pts[i];
        let n = p.len();
        if n < 2 || e.tail == e.head {
            continue;
        }
        for (tail, v, end, toward) in [
            (true, e.tail, &e.tail_end, p[1]),
            (false, e.head, &e.head_end, p[n - 2]),
        ] {
            let c = center(v);
            let natural = if toward.1 > c.1 { Side::S } else { Side::N };
            match resolve(&end.port, half(v), natural, toward.0 - c.0) {
                Some((side, along)) => {
                    ends[i][usize::from(!tail)] = Some((side, along, natural, toward.0));
                }
                None => {
                    ends[i][usize::from(!tail)] = Some((natural, 0.0, natural, toward.0));
                    groups.entry((v, natural)).or_default().push((
                        toward.0,
                        end.same.as_deref().filter(|s| !s.is_empty()),
                        i,
                        tail,
                    ));
                }
            }
        }
    }
    // ポートのない端を、側の幅の 7 割に等しい間で並べる
    for ((node, _), list) in groups {
        // samehead などの組は 1 つの席
        // (向こうの x の和, 数, 値, 端)
        type Slot<'a> = (f64, usize, Option<&'a str>, Vec<(usize, bool)>);
        let mut slots: Vec<Slot> = Vec::new();
        for (x, same, i, tail) in list {
            match same.and_then(|s| slots.iter().position(|t| t.2 == Some(s))) {
                Some(t) => {
                    slots[t].0 += x;
                    slots[t].1 += 1;
                    slots[t].3.push((i, tail));
                }
                None => slots.push((x, 1, same, vec![(i, tail)])),
            }
        }
        if slots.len() < 2 {
            continue;
        }
        slots.sort_by(|a, b| {
            (a.0 / a.1 as f64)
                .total_cmp(&(b.0 / b.1 as f64))
                .then(a.3[0].cmp(&b.3[0]))
        });
        let (width, cx) = nodes.get(node).map_or((0.0, 0.0), |n| (n.1, n.0.0));
        let half_w = width / 2.0;
        // 込んでいれば（7 割の幅で END_GAP 未満の間）幅の 9 割まで使い、それでも足りなければ、いちばん左・右の端を
        // 向こうがその側にあり、脇を通る間があれば、横の側へ移す（Graphviz も横の側から出入りする）
        let mut avail = width * 0.7;
        if avail / (slots.len() as f64) < END_GAP {
            avail = width * 0.9;
        }
        if avail / (slots.len() as f64) < END_GAP && slots.len() >= 3 {
            for (left, side) in [(true, Side::W), (false, Side::E)] {
                let at = if left { 0 } else { slots.len() - 1 };
                let x = slots[at].0 / slots[at].1 as f64;
                let room = if left { margin[node].0 } else { margin[node].1 };
                let beyond = if left {
                    x < cx - half_w
                } else {
                    x > cx + half_w
                };
                if beyond && room >= SIDE_ROOM {
                    let slot = slots.remove(at);
                    for (i, tail) in slot.3 {
                        if let Some(end) = &mut ends[i][usize::from(!tail)] {
                            end.0 = side;
                            end.1 = 0.0;
                        }
                    }
                }
            }
        }
        if slots.is_empty() {
            continue;
        }
        let k = slots.len() as f64;
        for (j, slot) in slots.into_iter().enumerate() {
            let off = -avail / 2.0 + avail * (j as f64 + 0.5) / k;
            for (i, tail) in slot.3 {
                if let Some(end) = &mut ends[i][usize::from(!tail)] {
                    end.1 = off;
                }
            }
        }
    }
    // 端の短い道。道の端の点は、短い道の最後の x のランクの行に
    let mut stubs: Vec<[Vec<P>; 2]> = vec![[vec![], vec![]]; edges.len()];
    for (i, e) in edges.iter().enumerate() {
        let n = pts[i].len();
        for (k, v) in [(0usize, e.tail), (1, e.head)] {
            let Some((side, along, natural, toward)) = ends[i][k] else {
                continue;
            };
            let c = center(v);
            let s = stub(c, half(v), side, along, natural, toward, margin[v]);
            let at = if k == 0 { 0 } else { n - 1 };
            pts[i][at] = (s[s.len() - 1].0, c.1);
            stubs[i][k] = s;
        }
        // 脇へ出る短い道は、向こうの端の縦の線がその脇の空きに入るなら、そこまでで止める（曲がりを減らす）。
        // ノードからは尾で 8、頭で 12（矢じりの分）は離す
        if n >= 2 {
            for (k, v) in [(0usize, e.tail), (1, e.head)] {
                let s = &stubs[i][k];
                if s.len() < 2 {
                    continue;
                }
                let (c, h) = (center(v), half(v));
                let target = if k == 0 { pts[i][n - 1].0 } else { pts[i][0].0 };
                let near = if k == 0 { 8.0 } else { 12.0 };
                let sx = s[s.len() - 1].0;
                let (lo, hi) = if sx > c.0 {
                    (c.0 + h.0 + near, sx)
                } else {
                    (sx, c.0 - h.0 - near)
                };
                if target >= lo && target <= hi && (target - sx).abs() > 1e-6 {
                    let last = s.len() - 1;
                    stubs[i][k][last].0 = target;
                    let at = if k == 0 { 0 } else { n - 1 };
                    pts[i][at].0 = target;
                }
            }
        }
        if !edges[i].keep {
            straighten(&mut pts[i], obstacles, &e.skip);
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
            if (a.0 - b.0).abs() > 1e-6 {
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
        // 段は空きの真ん中を中心に、TRACK 以上（12 まで）の間で並べる。端の縦の線に矢じりの分（12）を残せる
        // ならそうし、残せなければ空きの縁から 4 まで使う
        let gap = g1 - g0;
        let mid = if gap < 4.0 {
            (upper.1 + lower.1) / 2.0
        } else {
            (g0 + g1) / 2.0
        };
        let (g0, g1) = if gap < 4.0 {
            (mid - 2.0, mid + 2.0)
        } else {
            (g0 + 2.0, g1 - 2.0)
        };
        let count = tracks_in.get(&run.gap).copied().unwrap_or(1);
        let step = if count < 2 {
            0.0
        } else {
            let n = (count - 1) as f64;
            let roomy = ((gap - 24.0).max(0.0) / n).min(12.0);
            if roomy >= TRACK {
                roomy
            } else {
                ((gap - 8.0).max(0.0) / n).min(TRACK)
            }
        };
        let want = mid + (track[r] as f64 - (count - 1) as f64 / 2.0) * step;
        // 題を避ける高さ（望む高さから近い順に）。調べるのはこの線の範囲に重なる題だけ
        let (lo, hi) = (a.0.min(b.0), a.0.max(b.0));
        let (top, bottom) = (upper.1, lower.1);
        let skip = &edges[run.edge].skip;
        let near: Vec<&Rect> = titles
            .iter()
            .filter(|(r, owner)| {
                !skip.contains(owner) && r[0] <= hi && r[2] >= lo && r[1] <= bottom && r[3] >= top
            })
            .map(|(r, _)| r)
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
    let mut paths: Vec<Option<Vec<P>>> = pts
        .iter()
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
            // 端の短い道をつなぐ（道の端の点はランクの行の上で、短い道の最後の点と縦に並ぶ）
            let [tail_stub, head_stub] = &stubs[i];
            if !tail_stub.is_empty() {
                out.splice(0..1, tail_stub.iter().copied());
            }
            if !head_stub.is_empty() {
                let last = out.len() - 1;
                out.splice(last..last + 1, head_stub.iter().rev().copied());
            }
            Some(tidy(out))
        })
        .collect();
    separate(&mut paths, edges, nodes, obstacles);
    paths
}

/// 違う辺の、同じ向きで TRACK より近く並んで重なる線を離す（溝を分ける）。動かすのは両隣が直角の中ほどの線
/// だけで、両隣の線を伸び縮みさせて TRACK ずつずらし、ほかの線とも TRACK 以上離れ、ノード・題・ラベルの箱に
/// 当たらない位置を選ぶ。samehead・sametail の組（同じ点で会う線）は離さない。比べる組の数は NUDGE_WORK まで。
fn separate(
    paths: &mut [Option<Vec<P>>],
    edges: &[OrthoEdge],
    nodes: &[(P, f64, f64)],
    obstacles: &mut Obstacles,
) {
    #[derive(Clone, Copy)]
    struct Seg {
        edge: usize,
        at: usize,
        vertical: bool,
        pos: f64,
        lo: f64,
        hi: f64,
    }
    let joined = |a: usize, b: usize| {
        let (ea, eb) = (&edges[a], &edges[b]);
        let same = |x: &Option<String>, y: &Option<String>| {
            x.as_deref()
                .is_some_and(|x| !x.is_empty() && Some(x) == y.as_deref())
        };
        (ea.head == eb.head && same(&ea.head_end.same, &eb.head_end.same))
            || (ea.tail == eb.tail && same(&ea.tail_end.same, &eb.tail_end.same))
    };
    let mut work = NUDGE_WORK;
    for _ in 0..3 {
        let mut segs: Vec<Seg> = Vec::new();
        for (i, p) in paths.iter().enumerate() {
            let Some(p) = p else { continue };
            for (k, w) in p.windows(2).enumerate() {
                let (a, b) = (w[0], w[1]);
                if (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() > 1e-6 {
                    segs.push(Seg {
                        edge: i,
                        at: k,
                        vertical: true,
                        pos: a.0,
                        lo: a.1.min(b.1),
                        hi: a.1.max(b.1),
                    });
                } else if (a.1 - b.1).abs() < 1e-6 && (a.0 - b.0).abs() > 1e-6 {
                    segs.push(Seg {
                        edge: i,
                        at: k,
                        vertical: false,
                        pos: a.1,
                        lo: a.0.min(b.0),
                        hi: a.0.max(b.0),
                    });
                }
            }
        }
        segs.sort_by(|a, b| {
            a.vertical
                .cmp(&b.vertical)
                .then(a.pos.total_cmp(&b.pos))
                .then(a.edge.cmp(&b.edge))
                .then(a.at.cmp(&b.at))
        });
        let near = |a: &Seg, b: &Seg| {
            a.edge != b.edge
                && a.vertical == b.vertical
                && (a.pos - b.pos).abs() < TRACK - 0.01
                && a.hi.min(b.hi) - a.lo.max(b.lo) > 2.0
                && !joined(a.edge, b.edge)
        };
        // 並びの時の位置（動かしたものは `shifted` で別に見る）
        let keys: Vec<(bool, f64)> = segs.iter().map(|s| (s.vertical, s.pos)).collect();
        let mut shifted: Vec<usize> = Vec::new();
        let mut moved = false;
        let mut i = 0;
        while i < segs.len() && work > 0 {
            let mut j = i + 1;
            while j < segs.len() && segs[j].vertical == segs[i].vertical && work > 0 {
                if segs[j].pos - segs[i].pos >= TRACK - 0.01 {
                    break;
                }
                work -= 1;
                if near(&segs[i], &segs[j]) {
                    // 動かせるほう（後の辺を先に）: 中ほどの線と、ポートのない端の縦の線（ノードの幅の中で、
                    // 隣の横の線を伸び縮みさせる）
                    let movable = |s: &Seg, paths: &[Option<Vec<P>>]| {
                        let n = paths[s.edge].as_ref().map_or(0, Vec::len);
                        let e = &edges[s.edge];
                        let free_end = |end: &OrthoEnd| matches!(end.port, OrthoPort::Free);
                        (s.at > 0 && s.at + 2 < n)
                            || (n >= 3
                                && s.vertical
                                && ((s.at == 0 && free_end(&e.tail_end))
                                    || (s.at + 2 == n && free_end(&e.head_end))))
                    };
                    // 動かせるほうを順に試す（後の辺が動かせなければ、先の辺）
                    let mut pairs: Vec<(usize, usize)> = Vec::new();
                    if movable(&segs[j], paths) {
                        pairs.push((j, i));
                    }
                    if movable(&segs[i], paths) {
                        pairs.push((i, j));
                    }
                    for (m, other) in pairs {
                        let seg = segs[m];
                        let base = segs[other].pos;
                        let away = if seg.pos >= base { 1.0 } else { -1.0 };
                        let mut tries: Vec<f64> = (1..=3)
                            .flat_map(|k| [away * k as f64, -away * k as f64])
                            .map(|k| base + k * TRACK)
                            .collect();
                        tries.sort_by(|a, b| (a - seg.pos).abs().total_cmp(&(b - seg.pos).abs()));
                        let p = paths[seg.edge].clone().unwrap_or_default();
                        let e = &edges[seg.edge];
                        let skip = &e.skip;
                        let mut done = false;
                        // 端の線は、そのノードの幅の 9 割の中だけ
                        let n_pts = p.len();
                        let end_node = if seg.at == 0 {
                            Some(e.tail)
                        } else if seg.at + 2 == n_pts {
                            Some(e.head)
                        } else {
                            None
                        };
                        for to in tries {
                            if let Some(v) = end_node
                                && let Some(&((cx, _), w, _)) = nodes.get(v)
                                && (to - cx).abs() > w / 2.0 * 0.9
                            {
                                continue;
                            }
                            let probe = Seg { pos: to, ..seg };
                            // 近くの線だけ（並びの時の位置で二分探索）と、動かした線
                            let from =
                                keys.partition_point(|k| (k.0, k.1) < (seg.vertical, to - TRACK));
                            let mut window = 0;
                            let mut crowded = false;
                            for (k, s) in keys[from..].iter().zip(&segs[from..]) {
                                if k.0 != seg.vertical || k.1 > to + TRACK {
                                    break;
                                }
                                window += 1;
                                if near(&probe, s) {
                                    crowded = true;
                                    break;
                                }
                            }
                            work = work.saturating_sub(window + shifted.len() + 1);
                            if crowded || shifted.iter().any(|&x| near(&probe, &segs[x])) {
                                continue;
                            }
                            let mut q = p.clone();
                            for idx in [seg.at, seg.at + 1] {
                                if seg.vertical {
                                    q[idx].0 = to;
                                } else {
                                    q[idx].1 = to;
                                }
                            }
                            // 端の線（ノードに出入りする線）は短くしない（ノードの外の分が矢じりより短くなる）
                            let len = |p: &[P], k: usize| {
                                (p[k].0 - p[k + 1].0).abs() + (p[k].1 - p[k + 1].1).abs()
                            };
                            let last = q.len() - 2;
                            let ends_ok = [seg.at.checked_sub(1), Some(seg.at + 1)]
                                .into_iter()
                                .flatten()
                                .filter(|&k| k <= last && k != seg.at && (k == 0 || k == last))
                                .all(|k| len(&q, k) >= len(&p, k) - 1e-6);
                            if !ends_ok {
                                continue;
                            }
                            let clear =
                                (seg.at.saturating_sub(1)..=(seg.at + 1).min(last)).all(|k| {
                                    let (a, b) = (q[k], q[k + 1]);
                                    let r = [
                                        a.0.min(b.0) - 0.5,
                                        a.1.min(b.1) - 0.5,
                                        a.0.max(b.0) + 0.5,
                                        a.1.max(b.1) + 0.5,
                                    ];
                                    !obstacles.blocked(r, skip)
                                });
                            if clear {
                                paths[seg.edge] = Some(q);
                                segs[m].pos = to;
                                shifted.push(m);
                                moved = true;
                                done = true;
                                break;
                            }
                        }
                        if done {
                            break;
                        }
                    }
                }
                j += 1;
            }
            i += 1;
        }
        if !moved {
            break;
        }
    }
    if work == 0 {
        crate::cut_short();
    }
}

#[cfg(test)]
mod ortho_tests {
    use super::*;

    fn axis_aligned(p: &[P]) -> bool {
        p.windows(2)
            .all(|w| (w[0].0 - w[1].0).abs() < 1e-6 || (w[0].1 - w[1].1).abs() < 1e-6)
    }

    fn free(points: Vec<P>, tail: usize, head: usize) -> OrthoEdge {
        OrthoEdge {
            points,
            tail,
            head,
            tail_end: OrthoEnd::free(),
            head_end: OrthoEnd::free(),
            keep: false,
            skip: vec![tail, head],
        }
    }

    fn no_obstacles() -> Obstacles {
        Obstacles::new(vec![])
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
        let e = |t: usize, h: usize| free(vec![nodes[t].0, nodes[h].0], t, h);
        let edges = vec![e(0, 1), e(0, 2), e(3, 2)];
        let out = ortho(&edges, &nodes, &[], &mut no_obstacles());
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
                    assert!((a.0 - b.0).abs() >= TRACK - 0.01, "{levels:?}");
                }
            }
        }
        // 同じノードから出る 2 本は出る点が違う
        let (a, b) = (out[0].as_ref().unwrap()[0], out[1].as_ref().unwrap()[0]);
        assert!(a.0 < b.0);
        // 題を避ける
        let title = [([-100.0, 40.0, -30.0, 60.0], 99)];
        let one = vec![free(vec![(0.0, 0.0), (-60.0, 100.0)], 0, 1)];
        let p = ortho(&one, &nodes, &title, &mut no_obstacles())[0]
            .clone()
            .unwrap();
        assert!(
            !p.windows(2).any(|w| segment_hits(w[0], w[1], &title[0].0)),
            "{p:?}"
        );
    }

    #[test]
    fn spreads_free_ends_evenly_but_joins_samehead() {
        // 0 から下の 1・2・3 へ、4・5・6 から 7 へ（5 と 6 は samehead）
        let mut nodes = vec![((0.0, 0.0), 80.0, 36.0)];
        for x in [-100.0, 0.0, 100.0] {
            nodes.push(((x, 100.0), 54.0, 36.0));
        }
        for x in [-100.0, 0.0, 100.0] {
            nodes.push(((x, 200.0), 54.0, 36.0));
        }
        nodes.push(((0.0, 300.0), 80.0, 36.0));
        let mut edges: Vec<OrthoEdge> = (1..=3)
            .map(|h| free(vec![nodes[0].0, nodes[h].0], 0, h))
            .collect();
        for t in 4..=6 {
            let mut e = free(vec![nodes[t].0, nodes[7].0], t, 7);
            if t > 4 {
                e.head_end.same = Some("x".into());
            }
            edges.push(e);
        }
        let out: Vec<Vec<P>> = ortho(&edges, &nodes, &[], &mut no_obstacles())
            .into_iter()
            .map(Option::unwrap)
            .collect();
        for p in &out {
            assert!(axis_aligned(p), "{p:?}");
        }
        // 0 の下の 3 つの端は等しい間で、向こうの順
        let xs: Vec<f64> = out[..3].iter().map(|p| p[0].0).collect();
        assert!(xs[0] < xs[1] && xs[1] < xs[2], "{xs:?}");
        assert!(((xs[1] - xs[0]) - (xs[2] - xs[1])).abs() < 1e-6, "{xs:?}");
        // 7 の上: samehead の 2 本は同じ点、ほかの 1 本は別の点
        let heads: Vec<P> = out[3..].iter().map(|p| p[p.len() - 1]).collect();
        assert_eq!(heads[1], heads[2]);
        assert!((heads[0].0 - heads[1].0).abs() > 5.0, "{heads:?}");
    }

    #[test]
    fn leaves_from_the_port_side() {
        // 0 の東のポートから下の 1 へ、0 の北のポートから下の 2 へ
        let nodes = vec![
            ((0.0, 0.0), 60.0, 36.0),
            ((80.0, 100.0), 54.0, 36.0),
            ((-80.0, 100.0), 54.0, 36.0),
        ];
        let mut east = free(vec![(30.0, 0.0), nodes[1].0], 0, 1);
        east.tail_end.port = OrthoPort::Point {
            at: (30.0, 0.0),
            dir: (1.0, 0.0),
        };
        let mut north = free(vec![(0.0, 0.0), nodes[2].0], 0, 2);
        north.tail_end.port = OrthoPort::Point {
            at: (0.0, -18.0),
            dir: (0.0, -1.0),
        };
        let out = ortho(&[east, north], &nodes, &[], &mut no_obstacles());
        let e = out[0].as_ref().unwrap();
        assert!(axis_aligned(e), "{e:?}");
        // 中心の高さから右へ出て、ノードの外で下へ
        assert_eq!(e[0], (0.0, 0.0));
        assert!(e[1].1 == 0.0 && e[1].0 > 30.0, "{e:?}");
        let n = out[1].as_ref().unwrap();
        assert!(axis_aligned(n), "{n:?}");
        // 上へ出て、左の脇を回って下へ
        assert!(n[1].1 < -18.0 && n[1].0 == 0.0, "{n:?}");
        assert!(n[2].0 < -30.0, "{n:?}");
        assert_eq!(n[n.len() - 1], nodes[2].0);
    }

    #[test]
    fn straightens_past_virtual_nodes() {
        // 0 から 3 ランク下の 1 へ。仮のノードは左右に揺れるが、間にノードはない
        let nodes = vec![
            ((0.0, 0.0), 54.0, 36.0),
            ((0.0, 300.0), 54.0, 36.0),
            ((200.0, 100.0), 54.0, 36.0),
            ((200.0, 200.0), 54.0, 36.0),
        ];
        let e = free(
            vec![(0.0, 0.0), (40.0, 100.0), (-30.0, 200.0), (0.0, 300.0)],
            0,
            1,
        );
        let mut obstacles = Obstacles::new(
            nodes
                .iter()
                .enumerate()
                .map(|(i, n)| {
                    let (x, y) = n.0;
                    ([x - 31.0, y - 22.0, x + 31.0, y + 22.0], Some(i))
                })
                .collect(),
        );
        let p = ortho(&[e], &nodes, &[], &mut obstacles)[0].clone().unwrap();
        assert_eq!(p, vec![(0.0, 0.0), (0.0, 300.0)]);
        // 真ん中にノードがあれば回る（まっすぐにしない）
        let mut nodes2 = nodes.clone();
        nodes2[2].0 = (0.0, 100.0);
        let mut obstacles = Obstacles::new(vec![([-31.0, 78.0, 31.0, 122.0], Some(2))]);
        let e = free(
            vec![(0.0, 0.0), (60.0, 100.0), (60.0, 200.0), (0.0, 300.0)],
            0,
            1,
        );
        let p = ortho(&[e], &nodes2, &[], &mut obstacles)[0]
            .clone()
            .unwrap();
        assert!(p.iter().any(|q| q.0 > 40.0), "{p:?}");
        assert!(axis_aligned(&p));
    }
}
