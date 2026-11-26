//! dot の配置の曲線: 辺の通り道（ランクの行ごとの x の区間）の中の最短の折れ線を求め、なめらかな曲線を当てはめる
//! （Gansner ほか 1993 の §5 の考え。通り道の中の最短路を引いてから、通り道に収まる曲線にする）。
//!
//! 通り道の関は上から下（または下から上）へ y の順に並ぶので、最短路は「見える向きの幅」を関ごとに狭めていけば
//! 求まる（幅が閉じたら、閉じた側の関の端を新しい頂点にする）。

use crate::curve::{Cubic, P};
use crate::layout::Portal;

/// 関の端に触れる許し。
const SLACK: f64 = 3.0;
/// 切れ端の接線の長さ（切れ端の長さに対して）。向きが弦からそれる端は、`BOW` でさらに短くする。
const TANGENT: f64 = 1.0 / 3.0;
/// 切れ端の曲線が弦からふくらんでよい幅（片方の端の分）。ランクの間でははみ出しを見ないので、大きいと曲線が隣の
/// 線やラベルへふくらむ（16 ではラベルの多い図の交差が 3 割増え、4 では曲がりが折れ点に集まって角ばる）。
const BOW: f64 = 8.0;
/// 大きく曲げた版の、折れ線から離れてよい距離と、切れ端のふくらみの上限。
const ROOMY_ERROR: f64 = 24.0;
const ROOMY_BOW: f64 = 24.0;

/// `start` から `end` へ、関 `gates`（y の順）をすべて通る最短の折れ線（両端を含む）。
pub fn shortest(start: P, end: P, gates: &[Portal]) -> Vec<P> {
    let mut out = vec![start];
    let down = end.1 >= start.1;
    // 進む向きに y が並ぶ関だけ（両端の間）
    let gates: Vec<Portal> = gates
        .iter()
        .copied()
        .filter(|g| g.lo <= g.hi && g.y.is_finite())
        .filter(|g| {
            if down {
                g.y > start.1 && g.y < end.1
            } else {
                g.y < start.1 && g.y > end.1
            }
        })
        .collect();
    let mut apex = start;
    let mut i = 0;
    // 頂点の数の上限（関ごとに 1 つまで）
    let mut guard = 0;
    while guard <= gates.len() + 1 {
        guard += 1;
        // 見える向き（y あたりの x の傾き）の幅と、それを決めた関
        let (mut lo_s, mut hi_s) = (f64::NEG_INFINITY, f64::INFINITY);
        let (mut lo_at, mut hi_at): (Option<usize>, Option<usize>) = (None, None);
        let mut turned = false;
        let mut k = i;
        while k < gates.len() {
            let g = gates[k];
            let dy = g.y - apex.1;
            if dy.abs() < 1e-9 {
                k += 1;
                continue;
            }
            // dy の符号で左右が入れ替わらないように、x = apex.x + s * |dy| で考える
            let d = dy.abs();
            let (a, b) = ((g.lo - apex.0) / d, (g.hi - apex.0) / d);
            if a > lo_s {
                if a > hi_s {
                    // 左の端が右の幅を越えた: 右の幅を決めた関の右端を頂点に
                    let h = hi_at.unwrap_or(k);
                    apex = (gates[h].hi, gates[h].y);
                    out.push(apex);
                    i = h + 1;
                    turned = true;
                    break;
                }
                lo_s = a;
                lo_at = Some(k);
            }
            if b < hi_s {
                if b < lo_s {
                    let l = lo_at.unwrap_or(k);
                    apex = (gates[l].lo, gates[l].y);
                    out.push(apex);
                    i = l + 1;
                    turned = true;
                    break;
                }
                hi_s = b;
                hi_at = Some(k);
            }
            k += 1;
        }
        if turned {
            continue;
        }
        // 終わりの点へ
        let d = (end.1 - apex.1).abs();
        if d < 1e-9 {
            break;
        }
        let s = (end.0 - apex.0) / d;
        match (lo_at, hi_at) {
            (Some(l), _) if s < lo_s - 1e-12 => {
                apex = (gates[l].lo, gates[l].y);
                out.push(apex);
                i = l + 1;
            }
            (_, Some(h)) if s > hi_s + 1e-12 => {
                apex = (gates[h].hi, gates[h].y);
                out.push(apex);
                i = h + 1;
            }
            _ => break,
        }
    }
    out.push(end);
    out
}

/// 曲線が通り道に収まるか（`SAMPLES` 点の間の線で見る）。関の高さではその区間、続く 2 つの関の間（行の中と、
/// ランクの間）では 2 つの区間を合わせた幅（Gansner ほか 1993 §5 の、行の箱とランクの間の箱の並び）。
fn fits(c: &Cubic, gates: &[Portal]) -> bool {
    const SAMPLES: usize = 24;
    // 関は y の順（上りか下り）に並ぶので、y の入る間を二分探索で探す
    let ascending = gates.first().map(|g| g.y) <= gates.last().map(|g| g.y);
    let inside = |x: f64, y: f64| -> bool {
        let k = if ascending {
            gates.partition_point(|g| g.y < y)
        } else {
            gates.partition_point(|g| g.y > y)
        };
        // gates[k - 1] と gates[k] の間（端の外は見ない）
        if k == 0 || k >= gates.len() {
            return match gates.get(k) {
                Some(g) if (y - g.y).abs() < 1e-9 => x >= g.lo - SLACK && x <= g.hi + SLACK,
                _ => true,
            };
        }
        let (g, h) = (gates[k - 1], gates[k]);
        if (y - h.y).abs() < 1e-9 {
            return x >= h.lo - SLACK && x <= h.hi + SLACK;
        }
        x >= g.lo.min(h.lo) - SLACK && x <= g.hi.max(h.hi) + SLACK
    };
    let mut prev = c.0;
    for i in 1..=SAMPLES {
        let p = c.at(i as f64 / SAMPLES as f64);
        if !inside(p.0, p.1) {
            return false;
        }
        let (y0, y1) = (prev.1.min(p.1), prev.1.max(p.1));
        // y0..=y1 に入る関だけ（y の順なので二分探索で範囲を絞る）
        let (from, to) = if ascending {
            (
                gates.partition_point(|g| g.y < y0),
                gates.partition_point(|g| g.y <= y1),
            )
        } else {
            (
                gates.partition_point(|g| g.y > y1),
                gates.partition_point(|g| g.y >= y0),
            )
        };
        for g in &gates[from..to.max(from)] {
            if g.y < y0 || g.y > y1 {
                continue;
            }
            let x = if (p.1 - prev.1).abs() < 1e-9 {
                prev.0
            } else {
                prev.0 + (p.0 - prev.0) * (g.y - prev.1) / (p.1 - prev.1)
            };
            if x < g.lo - SLACK || x > g.hi + SLACK {
                return false;
            }
        }
        prev = p;
    }
    true
}

/// 折れ線 `points` を近似するなめらかな曲線。両端の間を 1 本の 3 次曲線で近似し（端の向きは隣の点への向き、制御点
/// の長さは折れ線の上の点への最小二乗。Schneider 1990）、通り道からはみ出すか、折れ線から `FIT_ERROR` より離れれば、
/// ずれのいちばん大きい所に近い折れ点で分けて（そこでの向きは前後の点を結ぶ向き）それぞれやり直す（Dobkin ほか
/// 1997 の当てはめ）。隣どうしの点まで分けてもだめなら、接線を縮め、さらにだめなら直線。
pub fn fit(points: &[P], gates: &[Portal]) -> Vec<Cubic> {
    fit_shaped(points, gates, (FIT_ERROR, BOW))
}

/// `fit` の大きく曲げた版: 折れ線から `ROOMY_ERROR` まで離れ、切れ端は `ROOMY_BOW` までふくらむ。通り道には収めるが、
/// ランクの間のほかの線・ノード・ラベルは見ないので、使う側が重なりを確かめる。
pub fn fit_roomy(points: &[P], gates: &[Portal]) -> Vec<Cubic> {
    fit_shaped(points, gates, (ROOMY_ERROR, ROOMY_BOW))
}

/// 折れ線から離れてよい距離と、切れ端のふくらみの上限 `shape` を決めて当てはめる。
fn fit_shaped(points: &[P], gates: &[Portal], shape: (f64, f64)) -> Vec<Cubic> {
    let n = points.len();
    if n < 2 {
        return Vec::new();
    }
    // つなぎ目の向き: 前後の区間の向き（単位の向き）の二等分。前の点から次の点への向きにすると、長い区間と短い
    // 区間が隣り合うとき長い区間の向きにそろい、長い区間が直線になって、曲がりが短い区間だけに集まる
    let joint = |i: usize| -> P {
        let u = unit((points[i].0 - points[i - 1].0, points[i].1 - points[i - 1].1));
        let v = unit((points[i + 1].0 - points[i].0, points[i + 1].1 - points[i].1));
        let t = unit((u.0 + v.0, u.1 + v.1));
        if t != (0.0, 0.0) {
            t
        } else {
            unit((
                points[i + 1].0 - points[i - 1].0,
                points[i + 1].1 - points[i - 1].1,
            ))
        }
    };
    // 両端の向き: 隣のつなぎ目の向きを、端の区間の弦をはさんで鏡写しにしたもの。端の区間が左右対称の弧になり、
    // ノードから出るところから曲がり始める（隣の点への向きにすると、端の制御点が弦の上に乗って、端の側だけ直線になる）
    let mirror = |t: P, a: P, b: P| -> P {
        let c = unit((b.0 - a.0, b.1 - a.1));
        let d = t.0 * c.0 + t.1 * c.1;
        unit((2.0 * d * c.0 - t.0, 2.0 * d * c.1 - t.1))
    };
    let tangent = |i: usize| -> P {
        if n == 2 {
            unit((points[1].0 - points[0].0, points[1].1 - points[0].1))
        } else if i == 0 {
            mirror(joint(1), points[0], points[1])
        } else if i == n - 1 {
            mirror(joint(n - 2), points[n - 2], points[n - 1])
        } else {
            joint(i)
        }
    };
    let mut out = Vec::with_capacity(n - 1);
    span(
        points,
        0,
        n - 1,
        tangent(0),
        tangent(n - 1),
        &tangent,
        gates,
        shape,
        &mut out,
    );
    out
}

/// 曲線の列を折れ線にする（1 本あたり `PER` 区間）。
pub fn sample(path: &[Cubic]) -> Vec<P> {
    const PER: usize = 8;
    let mut out = Vec::with_capacity(path.len() * PER + 1);
    for (k, c) in path.iter().enumerate() {
        if k == 0 {
            out.push(c.0);
        }
        for i in 1..=PER {
            out.push(c.at(i as f64 / PER as f64));
        }
    }
    out
}

/// 折れ線が箱（x0, y0, x1, y1）にかかるか。
pub fn hits_box(poly: &[P], b: &[f64; 4]) -> bool {
    poly.windows(2).any(|w| {
        let (p, q) = (w[0], w[1]);
        // 区間を箱で切る（Liang–Barsky）
        let (dx, dy) = (q.0 - p.0, q.1 - p.1);
        let (mut t0, mut t1) = (0.0f64, 1.0f64);
        for (d, lo, hi, o) in [(dx, b[0], b[2], p.0), (dy, b[1], b[3], p.1)] {
            if d.abs() < 1e-12 {
                if o < lo || o > hi {
                    return false;
                }
            } else {
                let (a, c) = ((lo - o) / d, (hi - o) / d);
                t0 = t0.max(a.min(c));
                t1 = t1.min(a.max(c));
                if t0 > t1 {
                    return false;
                }
            }
        }
        true
    })
}

/// 2 本の折れ線の交わる回数。
pub fn crossings(a: &[P], b: &[P]) -> usize {
    let side = |p: P, q: P, r: P| (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0);
    let mut n = 0;
    for u in a.windows(2) {
        let (ux0, ux1) = (u[0].0.min(u[1].0), u[0].0.max(u[1].0));
        let (uy0, uy1) = (u[0].1.min(u[1].1), u[0].1.max(u[1].1));
        for v in b.windows(2) {
            if v[0].0.max(v[1].0) < ux0
                || v[0].0.min(v[1].0) > ux1
                || v[0].1.max(v[1].1) < uy0
                || v[0].1.min(v[1].1) > uy1
            {
                continue;
            }
            let (d1, d2) = (side(v[0], v[1], u[0]), side(v[0], v[1], u[1]));
            let (d3, d4) = (side(u[0], u[1], v[0]), side(u[0], u[1], v[1]));
            if d1 * d2 < 0.0 && d3 * d4 < 0.0 {
                n += 1;
            }
        }
    }
    n
}

/// 近似の曲線が折れ線から離れてよい距離。
const FIT_ERROR: f64 = 8.0;
/// 1 本の曲線で近似を試す区間の折れ点の数の上限。
const SPAN_POINTS: usize = 16;

fn unit(d: P) -> P {
    let l = d.0.hypot(d.1);
    if l < 1e-9 {
        (0.0, 0.0)
    } else {
        (d.0 / l, d.1 / l)
    }
}

/// 折れ線 `poly` の上に、間がおよそ `step` の点を取る（両端と折れ点を含む）。
fn samples(poly: &[P], step: f64) -> Vec<P> {
    let mut out = vec![poly[0]];
    for w in poly.windows(2) {
        let len = (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1);
        let k = (len / step).ceil().max(1.0) as usize;
        for i in 1..=k {
            let t = i as f64 / k as f64;
            out.push((
                w[0].0 + (w[1].0 - w[0].0) * t,
                w[0].1 + (w[1].1 - w[0].1) * t,
            ));
        }
    }
    out
}

/// 点の列 `d` を、`t1` で出て `t2` で入る 3 次曲線で最小二乗に近似する（媒介変数は弦の長さの割合）。曲線と、点の
/// うちいちばん離れたものの距離と番号。
fn least_squares(d: &[P], t1: P, t2: P) -> (Cubic, f64, usize) {
    let n = d.len();
    let (p0, p3) = (d[0], d[n - 1]);
    let mut u = vec![0.0; n];
    for i in 1..n {
        u[i] = u[i - 1] + (d[i].0 - d[i - 1].0).hypot(d[i].1 - d[i - 1].1);
    }
    let total = u[n - 1].max(1e-9);
    for v in &mut u {
        *v /= total;
    }
    let chord = (p3.0 - p0.0).hypot(p3.1 - p0.1);
    let solve = |u: &[f64]| -> (f64, f64) {
        let (mut c00, mut c01, mut c11, mut x0, mut x1) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for (i, &t) in u.iter().enumerate() {
            let s = 1.0 - t;
            let (b0, b1, b2, b3) = (s * s * s, 3.0 * t * s * s, 3.0 * t * t * s, t * t * t);
            let a0 = (t1.0 * b1, t1.1 * b1);
            let a1 = (-t2.0 * b2, -t2.1 * b2);
            c00 += a0.0 * a0.0 + a0.1 * a0.1;
            c01 += a0.0 * a1.0 + a0.1 * a1.1;
            c11 += a1.0 * a1.0 + a1.1 * a1.1;
            let base = (
                p0.0 * (b0 + b1) + p3.0 * (b2 + b3),
                p0.1 * (b0 + b1) + p3.1 * (b2 + b3),
            );
            let r = (d[i].0 - base.0, d[i].1 - base.1);
            x0 += a0.0 * r.0 + a0.1 * r.1;
            x1 += a1.0 * r.0 + a1.1 * r.1;
        }
        let det = c00 * c11 - c01 * c01;
        let (mut al, mut ar) = if det.abs() < 1e-12 {
            (0.0, 0.0)
        } else {
            ((x0 * c11 - x1 * c01) / det, (c00 * x1 - c01 * x0) / det)
        };
        // 小さすぎる・負なら、弦の 1/3（Schneider の取り決め）
        if al < chord * 1e-3 || ar < chord * 1e-3 {
            al = chord / 3.0;
            ar = chord / 3.0;
        }
        (al, ar)
    };
    let build = |al: f64, ar: f64| {
        Cubic(
            p0,
            (p0.0 + t1.0 * al, p0.1 + t1.1 * al),
            (p3.0 - t2.0 * ar, p3.1 - t2.1 * ar),
            p3,
        )
    };
    let err = |c: &Cubic, u: &[f64]| -> (f64, usize) {
        let mut worst = (0.0, n / 2);
        for (i, &t) in u.iter().enumerate() {
            let q = c.at(t);
            let e = (q.0 - d[i].0).hypot(q.1 - d[i].1);
            if e > worst.0 {
                worst = (e, i);
            }
        }
        worst
    };
    let (al, ar) = solve(&u);
    let mut c = build(al, ar);
    let (mut e, mut at) = err(&c, &u);
    // 媒介変数を曲線のいちばん近い点に寄せて解き直す（ニュートン法を 1 回ずつ、2 回まで）
    for _ in 0..2 {
        if e <= FIT_ERROR {
            break;
        }
        for (i, t) in u.iter_mut().enumerate() {
            *t = newton(&c, d[i], *t);
        }
        let (al, ar) = solve(&u);
        let c2 = build(al, ar);
        let (e2, at2) = err(&c2, &u);
        if e2 >= e {
            break;
        }
        (c, e, at) = (c2, e2, at2);
    }
    (c, e, at)
}

/// 媒介変数 `t` を、点 `p` にいちばん近い曲線の点へ寄せる（ニュートン法 1 回）。
fn newton(c: &Cubic, p: P, t: f64) -> f64 {
    let q = c.at(t);
    let d1 = c.derivative(t);
    let d2 = c.second_derivative(t);
    let num = (q.0 - p.0) * d1.0 + (q.1 - p.1) * d1.1;
    let den = d1.0 * d1.0 + d1.1 * d1.1 + (q.0 - p.0) * d2.0 + (q.1 - p.1) * d2.1;
    if den.abs() < 1e-12 {
        t
    } else {
        (t - num / den).clamp(0.0, 1.0)
    }
}

/// `points[i..=j]` を、`tp` で出て `tq` で入る曲線で近似する。
#[allow(clippy::too_many_arguments)]
fn span(
    points: &[P],
    i: usize,
    j: usize,
    tp: P,
    tq: P,
    tangent: &dyn Fn(usize) -> P,
    gates: &[Portal],
    shape: (f64, f64),
    out: &mut Vec<Cubic>,
) {
    let (tolerance, bow) = shape;
    if j - i == 1 {
        piece(points[i], points[j], tp, tq, gates, bow, 3, out);
        return;
    }
    // 折れ点が多すぎれば、近似を試さずに真ん中で分ける（込んだ長い線は 1 本に収まらず、試すだけ遅い）
    if j - i > SPAN_POINTS {
        let m = (i + j) / 2;
        let tm = tangent(m);
        span(points, i, m, tp, tm, tangent, gates, shape, out);
        span(points, m, j, tm, tq, tangent, gates, shape, out);
        return;
    }
    let d = samples(&points[i..=j], 16.0);
    let (c, e, at) = least_squares(&d, tp, tq);
    if e <= tolerance && fits(&c, gates) {
        out.push(c);
        return;
    }
    // いちばん離れた点に近い折れ点（両端を除く）で分ける
    let far = d[at];
    let m = (i + 1..j)
        .min_by(|&a, &b| {
            let dist = |k: usize| (points[k].0 - far.0).hypot(points[k].1 - far.1);
            dist(a)
                .partial_cmp(&dist(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or((i + j) / 2);
    let tm = tangent(m);
    span(points, i, m, tp, tm, tangent, gates, shape, out);
    span(points, m, j, tm, tq, tangent, gates, shape, out);
}

/// p から q への切れ端（接線 tp・tq）。はみ出せば接線を縮め、それでもだめなら直線の真ん中で 2 つに分けて（向きは
/// 直線の向き）やり直す。`depth` が尽きたら直線。
#[allow(clippy::too_many_arguments)]
fn piece(p: P, q: P, tp: P, tq: P, gates: &[Portal], bow: f64, depth: u32, out: &mut Vec<Cubic>) {
    let len = (q.0 - p.0).hypot(q.1 - p.1);
    let mut scale = len * TANGENT;
    // 制御点が弦から離れる分を抑えて、曲線のふくらみを `BOW` までにする（片方の制御点が弦から h 離れると、曲線は
    // 4h/9 までふくらむ）
    let chord = if len > 1e-9 {
        ((q.0 - p.0) / len, (q.1 - p.1) / len)
    } else {
        (0.0, 0.0)
    };
    let sin = |t: P| (t.0 * chord.1 - t.1 * chord.0).abs();
    let cap = |t: P, l: f64| {
        if sin(t) > 1e-6 {
            l.min(bow * 9.0 / 8.0 / sin(t))
        } else {
            l
        }
    };
    for _ in 0..3 {
        let (lp, lq) = (cap(tp, scale), cap(tq, scale));
        let c = Cubic(
            p,
            (p.0 + tp.0 * lp, p.1 + tp.1 * lp),
            (q.0 - tq.0 * lq, q.1 - tq.1 * lq),
            q,
        );
        if fits(&c, gates) {
            out.push(c);
            return;
        }
        scale /= 2.0;
    }
    if depth == 0 || len < 4.0 {
        out.push(Cubic::line(p, q));
        return;
    }
    let m = ((p.0 + q.0) / 2.0, (p.1 + q.1) / 2.0);
    let dir = ((q.0 - p.0) / len, (q.1 - p.1) / len);
    piece(p, m, tp, dir, gates, bow, depth - 1, out);
    piece(m, q, dir, tq, gates, bow, depth - 1, out);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate(y: f64, lo: f64, hi: f64) -> Portal {
        Portal { y, lo, hi }
    }

    #[test]
    fn fits_one_long_curve_through_a_gentle_polyline() {
        // 広い通り道のゆるい折れ線は、折れ点ごとに分けず 1 本の曲線にする
        let points = [
            (0.0, 0.0),
            (10.0, 50.0),
            (16.0, 100.0),
            (19.0, 150.0),
            (20.0, 200.0),
        ];
        let gates: Vec<Portal> = (1..8)
            .map(|k| gate(k as f64 * 25.0, -100.0, 100.0))
            .collect();
        let c = fit(&points, &gates);
        assert_eq!(c.len(), 1, "{c:?}");
        // 大きなジグザグは 1 本では折れ線から離れすぎるので、折れ点で分ける
        let zig = [
            (0.0, 0.0),
            (40.0, 50.0),
            (0.0, 100.0),
            (40.0, 150.0),
            (0.0, 200.0),
        ];
        let c = fit(&zig, &gates);
        assert!(c.len() > 1, "{c:?}");
        assert_eq!(c[0].0, zig[0]);
        assert_eq!(c[c.len() - 1].3, zig[4]);
    }

    #[test]
    fn long_pieces_bow_into_the_turn_but_not_far() {
        // 長い区間の先に短い区間が直角に続く: つなぎ目の向きは二等分なので、長い区間もふくらんで曲がる（弦の上に
        // 制御点が並んだ直線にならない）。ふくらみは BOW まで
        let points = [(0.0, 0.0), (300.0, 0.0), (300.0, 20.0), (0.0, 40.0)];
        let c = fit(&points, &[]);
        let end = c.iter().rfind(|q| q.3.0 == 300.0 && q.3.1 == 0.0).unwrap();
        // つなぎ目へ入る制御点は弦（y = 0）から外れる
        assert!(end.2.1.abs() > 1.0, "{c:?}");
        let far = c
            .iter()
            .filter(|q| q.3.1 <= 0.0 + 1e-9 && q.0.1 <= 1e-9)
            .flat_map(|q| (0..=32).map(move |i| q.at(i as f64 / 32.0).1.abs()))
            .fold(0.0, f64::max);
        assert!(far > 0.5 && far <= BOW + 1e-6, "{far} {c:?}");
    }

    #[test]
    fn end_pieces_are_symmetric_arcs() {
        // 1 か所で折れる線: 端の向きはつなぎ目の向きの鏡写しなので、端の制御点も弦から同じだけ外れる
        let points = [(0.0, 0.0), (60.0, 40.0), (0.0, 80.0)];
        let c = fit(&points, &[]);
        let first = c[0];
        let off = |p: P, a: P, b: P| {
            let l = (b.0 - a.0).hypot(b.1 - a.1);
            ((b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)) / l
        };
        let (h1, h2) = (
            off(first.1, first.0, first.3),
            off(first.2, first.0, first.3),
        );
        assert!(h1.abs() > 1.0, "{c:?}");
        assert!((h1 - h2).abs() < 1e-6, "{h1} {h2}");
        // 大きく曲げた版は、同じ向きでもっとふくらむ
        let r = fit_roomy(&points, &[]);
        assert!(off(r[0].1, r[0].0, r[0].3).abs() >= h1.abs(), "{r:?}");
    }

    #[test]
    fn counts_hits_and_crossings_of_polylines() {
        let a = [(0.0, 0.0), (10.0, 10.0)];
        assert!(hits_box(&a, &[4.0, 4.0, 6.0, 6.0]));
        assert!(!hits_box(&a, &[6.0, 0.0, 9.0, 3.0]));
        assert_eq!(crossings(&a, &[(0.0, 10.0), (10.0, 0.0)]), 1);
        assert_eq!(crossings(&a, &[(20.0, 0.0), (30.0, 10.0)]), 0);
        assert_eq!(sample(&[Cubic::line((0.0, 0.0), (8.0, 0.0))]).len(), 9);
    }

    #[test]
    fn goes_straight_when_the_corridor_allows() {
        let gates = [gate(40.0, -10.0, 100.0), gate(60.0, -10.0, 100.0)];
        assert_eq!(
            shortest((0.0, 0.0), (90.0, 100.0), &gates),
            vec![(0.0, 0.0), (90.0, 100.0)]
        );
    }

    #[test]
    fn bends_at_the_corner_that_closes_the_view() {
        // 右へ行く道を、真ん中の関の右端（20）が止める
        let gates = [gate(40.0, -10.0, 20.0), gate(60.0, -10.0, 20.0)];
        let p = shortest((0.0, 0.0), (100.0, 100.0), &gates);
        assert_eq!(p, vec![(0.0, 0.0), (20.0, 60.0), (100.0, 100.0)]);
        // 上へ向かう道（頭が上）でも同じ
        let p = shortest((100.0, 100.0), (0.0, 0.0), &[gates[1], gates[0]]);
        assert_eq!(p, vec![(100.0, 100.0), (20.0, 60.0), (0.0, 0.0)]);
    }

    #[test]
    fn zigzags_through_offset_gates() {
        let gates = [
            gate(30.0, 50.0, 60.0),
            gate(40.0, 50.0, 60.0),
            gate(70.0, 0.0, 10.0),
            gate(80.0, 0.0, 10.0),
        ];
        let p = shortest((0.0, 0.0), (60.0, 110.0), &gates);
        for g in &gates {
            // 折れ線は関の高さでその区間の中
            let x = p
                .windows(2)
                .find(|w| w[0].1 <= g.y && w[1].1 >= g.y)
                .map(|w| w[0].0 + (w[1].0 - w[0].0) * (g.y - w[0].1) / (w[1].1 - w[0].1))
                .unwrap();
            assert!(x >= g.lo - 1e-9 && x <= g.hi + 1e-9, "{p:?} {g:?}");
        }
        assert!(p.len() >= 4, "{p:?}");
    }

    #[test]
    fn fitted_curves_stay_in_the_corridor() {
        let gates = [
            gate(30.0, 50.0, 60.0),
            gate(40.0, 50.0, 60.0),
            gate(70.0, 0.0, 10.0),
            gate(80.0, 0.0, 10.0),
        ];
        let p = shortest((0.0, 0.0), (60.0, 110.0), &gates);
        for c in fit(&p, &gates) {
            assert!(fits(&c, &gates), "{c:?}");
        }
    }
}
