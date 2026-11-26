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

/// 曲線が関の区間に収まるか（`SAMPLES` 点の間の線で、y が関をまたぐ所の x を見る）。
fn fits(c: &Cubic, gates: &[Portal]) -> bool {
    const SAMPLES: usize = 16;
    let mut prev = c.0;
    for i in 1..=SAMPLES {
        let p = c.at(i as f64 / SAMPLES as f64);
        let (y0, y1) = (prev.1.min(p.1), prev.1.max(p.1));
        for g in gates {
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

/// 折れ線 `points` を通るなめらかな曲線（中の点では前後の点を結ぶ向き、両端では隣の点への向き）。関 `gates` から
/// はみ出す切れ端は、接線を縮めてやり直し、だめなら直線にする。
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
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let l = dx.hypot(dy);
        if l < 1e-9 {
            (0.0, 0.0)
        } else {
            (dx / l, dy / l)
        }
    };
    let mut out = Vec::with_capacity(n - 1);
    for i in 0..n - 1 {
        piece(
            points[i],
            points[i + 1],
            tangent(i),
            tangent(i + 1),
            gates,
            3,
            &mut out,
        );
    }
    out
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
