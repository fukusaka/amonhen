//! dot の配置の曲線: 辺の通り道（ランクの行ごとの x の区間）の中の最短の折れ線を求め、なめらかな曲線を当てはめる
//! （Gansner ほか 1993 の §5 の考え。通り道の中の最短路を引いてから、通り道に収まる曲線にする）。
//!
//! 通り道の関は上から下（または下から上）へ y の順に並ぶので、最短路は「見える向きの幅」を関ごとに狭めていけば
//! 求まる（幅が閉じたら、閉じた側の関の端を新しい頂点にする）。

use crate::curve::{Cubic, P};
use crate::layout::Portal;

/// 関の端に触れる許し。
const SLACK: f64 = 3.0;
/// 切れ端の接線の長さ（切れ端の長さに対して）。関の高さの間でははみ出しを見ないので、長いと曲線が隣の線の通り道へ
/// ふくらんで交わる（1/3 では 1/5 より交差が 1.6% 多く、ほかのノードを通る線が 1.7 倍）。
const TANGENT: f64 = 0.2;

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
    let n = points.len();
    if n < 2 {
        return Vec::new();
    }
    let tangent = |i: usize| -> P {
        let (a, b) = if i == 0 {
            (points[0], points[1])
        } else if i == n - 1 {
            (points[n - 2], points[n - 1])
        } else {
            (points[i - 1], points[i + 1])
        };
        unit((b.0 - a.0, b.1 - a.1))
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
        &mut out,
    );
    out
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
    out: &mut Vec<Cubic>,
) {
    if j - i == 1 {
        piece(points[i], points[j], tp, tq, gates, 3, out);
        return;
    }
    // 折れ点が多すぎれば、近似を試さずに真ん中で分ける（込んだ長い線は 1 本に収まらず、試すだけ遅い）
    if j - i > SPAN_POINTS {
        let m = (i + j) / 2;
        let tm = tangent(m);
        span(points, i, m, tp, tm, tangent, gates, out);
        span(points, m, j, tm, tq, tangent, gates, out);
        return;
    }
    let d = samples(&points[i..=j], 16.0);
    let (c, e, at) = least_squares(&d, tp, tq);
    if e <= FIT_ERROR && fits(&c, gates) {
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
    span(points, i, m, tp, tm, tangent, gates, out);
    span(points, m, j, tm, tq, tangent, gates, out);
}

/// p から q への切れ端（接線 tp・tq）。はみ出せば接線を縮め、それでもだめなら直線の真ん中で 2 つに分けて（向きは
/// 直線の向き）やり直す。`depth` が尽きたら直線。
fn piece(p: P, q: P, tp: P, tq: P, gates: &[Portal], depth: u32, out: &mut Vec<Cubic>) {
    let len = (q.0 - p.0).hypot(q.1 - p.1);
    let mut scale = len * TANGENT;
    for _ in 0..3 {
        let c = Cubic(
            p,
            (p.0 + tp.0 * scale, p.1 + tp.1 * scale),
            (q.0 - tq.0 * scale, q.1 - tq.1 * scale),
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
    piece(p, m, tp, dir, gates, depth - 1, out);
    piece(m, q, dir, tq, gates, depth - 1, out);
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
