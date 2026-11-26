//! 3 次ベジェの道筋: 点を通る曲線、ノードの外形での切り取り、矢じりの分の短縮、長さ、平行にずらした線。

pub type P = (f64, f64);

/// 3 次ベジェの切れ端（始点・制御点 2 つ・終点）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cubic(pub P, pub P, pub P, pub P);

fn lerp(a: P, b: P, t: f64) -> P {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
}

impl Cubic {
    pub fn line(a: P, b: P) -> Self {
        Cubic(a, lerp(a, b, 1.0 / 3.0), lerp(a, b, 2.0 / 3.0), b)
    }

    pub fn at(&self, t: f64) -> P {
        let u = 1.0 - t;
        let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
        (
            a * self.0.0 + b * self.1.0 + c * self.2.0 + d * self.3.0,
            a * self.0.1 + b * self.1.1 + c * self.2.1 + d * self.3.1,
        )
    }

    /// t での向き（1 階の微分）。
    pub fn derivative(&self, t: f64) -> P {
        let u = 1.0 - t;
        let (a, b, c) = (3.0 * u * u, 6.0 * u * t, 3.0 * t * t);
        (
            a * (self.1.0 - self.0.0) + b * (self.2.0 - self.1.0) + c * (self.3.0 - self.2.0),
            a * (self.1.1 - self.0.1) + b * (self.2.1 - self.1.1) + c * (self.3.1 - self.2.1),
        )
    }

    /// t での 2 階の微分。
    pub fn second_derivative(&self, t: f64) -> P {
        let u = 1.0 - t;
        (
            6.0 * u * (self.2.0 - 2.0 * self.1.0 + self.0.0)
                + 6.0 * t * (self.3.0 - 2.0 * self.2.0 + self.1.0),
            6.0 * u * (self.2.1 - 2.0 * self.1.1 + self.0.1)
                + 6.0 * t * (self.3.1 - 2.0 * self.2.1 + self.1.1),
        )
    }

    /// t で 2 つに分ける。
    pub fn split(&self, t: f64) -> (Cubic, Cubic) {
        let ab = lerp(self.0, self.1, t);
        let bc = lerp(self.1, self.2, t);
        let cd = lerp(self.2, self.3, t);
        let abc = lerp(ab, bc, t);
        let bcd = lerp(bc, cd, t);
        let m = lerp(abc, bcd, t);
        (Cubic(self.0, ab, abc, m), Cubic(m, bcd, cd, self.3))
    }

    pub fn map(&self, f: impl Fn(P) -> P) -> Self {
        Cubic(f(self.0), f(self.1), f(self.2), f(self.3))
    }

    fn length(&self) -> f64 {
        let mut len = 0.0;
        let mut prev = self.0;
        for i in 1..=16 {
            let p = self.at(i as f64 / 16.0);
            len += (p.0 - prev.0).hypot(p.1 - prev.1);
            prev = p;
        }
        len
    }

    /// 始点での向き（制御点が始点と重なれば次の点へ）。
    fn start_dir(&self) -> P {
        for q in [self.1, self.2, self.3] {
            let (dx, dy) = (q.0 - self.0.0, q.1 - self.0.1);
            let l = dx.hypot(dy);
            if l > 1e-9 {
                return (dx / l, dy / l);
            }
        }
        (1.0, 0.0)
    }

    fn end_dir(&self) -> P {
        for q in [self.2, self.1, self.0] {
            let (dx, dy) = (self.3.0 - q.0, self.3.1 - q.1);
            let l = dx.hypot(dy);
            if l > 1e-9 {
                return (dx / l, dy / l);
            }
        }
        (1.0, 0.0)
    }
}

/// 点を順に通る道筋。`axis` は流れの向き（0 で縦、1 で横）: 中の点ではその向きに通り抜ける。`straight` は折れ線。
pub fn through(points: &[P], straight: bool, vertical: bool) -> Vec<Cubic> {
    let mut out = Vec::new();
    if points.len() < 2 {
        return out;
    }
    if straight {
        for w in points.windows(2) {
            out.push(Cubic::line(w[0], w[1]));
        }
        return out;
    }
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        if vertical {
            let dy = (b.1 - a.1) * 0.45;
            out.push(Cubic(a, (a.0, a.1 + dy), (b.0, b.1 - dy), b));
        } else {
            let dx = (b.0 - a.0) * 0.45;
            out.push(Cubic(a, (a.0 + dx, a.1), (b.0 - dx, b.1), b));
        }
    }
    out
}

/// 点を順に通るなめらかな曲線（Catmull–Rom の曲線を 3 次ベジェに直したもの）。2 点なら直線。
pub fn smooth(points: &[P]) -> Vec<Cubic> {
    if points.len() < 3 {
        return through(points, true, false);
    }
    let n = points.len();
    (0..n - 1)
        .map(|i| {
            let p0 = points[i.saturating_sub(1)];
            let (p1, p2) = (points[i], points[i + 1]);
            let p3 = points[(i + 2).min(n - 1)];
            Cubic(
                p1,
                (p1.0 + (p2.0 - p0.0) / 6.0, p1.1 + (p2.1 - p0.1) / 6.0),
                (p2.0 - (p3.0 - p1.0) / 6.0, p2.1 - (p3.1 - p1.1) / 6.0),
                p2,
            )
        })
        .collect()
}

/// 始めのほうを `inside` の外に出る所で切る。始点が外なら何もしない。
pub fn clip_start(path: &mut Vec<Cubic>, inside: &dyn Fn(P) -> bool) {
    if path.is_empty() || !inside(path[0].0) {
        return;
    }
    for i in 0..path.len() {
        let seg = path[i];
        // この切れ端の中で外に出るか（細かく見る）
        let mut prev_t = 0.0;
        let mut found = None;
        for k in 1..=24 {
            let t = k as f64 / 24.0;
            if !inside(seg.at(t)) {
                found = Some((prev_t, t));
                break;
            }
            prev_t = t;
        }
        if let Some((mut lo, mut hi)) = found {
            for _ in 0..20 {
                let mid = (lo + hi) / 2.0;
                if inside(seg.at(mid)) {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let (_, rest) = seg.split(hi);
            path.drain(..i);
            path[0] = rest;
            return;
        }
    }
    // 全体が中: 切らない
}

/// 終わりのほうを `inside` に入る所で切る。
pub fn clip_end(path: &mut Vec<Cubic>, inside: &dyn Fn(P) -> bool) {
    let mut rev: Vec<Cubic> = path
        .iter()
        .rev()
        .map(|c| Cubic(c.3, c.2, c.1, c.0))
        .collect();
    clip_start(&mut rev, inside);
    *path = rev
        .into_iter()
        .rev()
        .map(|c| Cubic(c.3, c.2, c.1, c.0))
        .collect();
}

/// 終わりを長さ `len` だけ縮め、もとの終点と、そこへの向きを返す（矢じりの先と向き）。
pub fn shorten_end(path: &mut Vec<Cubic>, len: f64) -> Option<(P, P)> {
    let last = *path.last()?;
    let tip = last.3;
    if len <= 0.0 {
        return Some((tip, last.end_dir()));
    }
    // 終わりから長さを数え、len の所で切る
    let mut remaining = len;
    while let Some(seg) = path.last().copied() {
        let l = seg.length();
        if l > remaining {
            // 切れ端の中で、終わりからの長さが remaining になる t
            let (mut lo, mut hi) = (0.0, 1.0);
            for _ in 0..20 {
                let mid = (lo + hi) / 2.0;
                let (_, tail) = seg.split(mid);
                if tail.length() > remaining {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let (head, _) = seg.split(lo);
            *path.last_mut().unwrap() = head;
            let base = head.3;
            let (dx, dy) = (tip.0 - base.0, tip.1 - base.1);
            let d = dx.hypot(dy);
            let dir = if d > 1e-9 {
                (dx / d, dy / d)
            } else {
                seg.end_dir()
            };
            return Some((tip, dir));
        }
        remaining -= l;
        path.pop();
        if path.is_empty() {
            // 道筋が矢じりより短い: 矢じりだけ（向きは最後の切れ端）
            return Some((tip, seg.end_dir()));
        }
    }
    None
}

pub fn reverse(path: &[Cubic]) -> Vec<Cubic> {
    path.iter()
        .rev()
        .map(|c| Cubic(c.3, c.2, c.1, c.0))
        .collect()
}

pub fn total_length(path: &[Cubic]) -> f64 {
    path.iter().map(Cubic::length).sum()
}

/// 道筋の長さの半ばの点。
pub fn midpoint(path: &[Cubic]) -> Option<P> {
    let total = total_length(path);
    let mut walked = 0.0;
    for seg in path {
        let l = seg.length();
        if walked + l >= total / 2.0 {
            let want = total / 2.0 - walked;
            let (mut lo, mut hi) = (0.0, 1.0);
            for _ in 0..20 {
                let mid = (lo + hi) / 2.0;
                if seg.split(mid).0.length() < want {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            return Some(seg.at(lo));
        }
        walked += l;
    }
    path.last().map(|c| c.3)
}

/// 道筋を左へ `d` だけずらした近似（制御点を端の法線の向きにずらす）。
pub fn offset(path: &[Cubic], d: f64) -> Vec<Cubic> {
    path.iter()
        .map(|c| {
            let s = c.start_dir();
            let e = c.end_dir();
            let (ns, ne) = ((-s.1 * d, s.0 * d), (-e.1 * d, e.0 * d));
            Cubic(
                (c.0.0 + ns.0, c.0.1 + ns.1),
                (c.1.0 + ns.0, c.1.1 + ns.1),
                (c.2.0 + ne.0, c.2.1 + ne.1),
                (c.3.0 + ne.0, c.3.1 + ne.1),
            )
        })
        .collect()
}

/// SVG の `d`。
/// 太さの変わる線（`style=tapered`）の外形の SVG のパス。`width(s)` は線に沿った割合 s（0〜1）での太さ。
pub fn tapered(path: &[Cubic], width: impl Fn(f64) -> f64) -> String {
    use amonhen_pele::svg::builder::num;
    // 線に沿って点を取る（1 本の曲線に 24 点）
    let mut pts: Vec<P> = Vec::new();
    for c in path {
        for k in 0..=24 {
            if k == 0 && !pts.is_empty() {
                continue;
            }
            pts.push(c.at(k as f64 / 24.0));
        }
    }
    pts.dedup_by(|a, b| (a.0 - b.0).hypot(a.1 - b.1) < 1e-9);
    if pts.len() < 2 {
        return String::new();
    }
    let mut along = vec![0.0];
    for w in pts.windows(2) {
        let l = along[along.len() - 1] + (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1);
        along.push(l);
    }
    let total = along[along.len() - 1].max(1e-9);
    let mut left = Vec::with_capacity(pts.len());
    let mut right = Vec::with_capacity(pts.len());
    for (i, p) in pts.iter().enumerate() {
        let (a, b) = (pts[i.saturating_sub(1)], pts[(i + 1).min(pts.len() - 1)]);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let l = dx.hypot(dy).max(1e-9);
        let half = width(along[i] / total).max(0.0) / 2.0;
        let n = (-dy / l * half, dx / l * half);
        left.push((p.0 + n.0, p.1 + n.1));
        right.push((p.0 - n.0, p.1 - n.1));
    }
    let mut d = String::new();
    for (i, q) in left.iter().chain(right.iter().rev()).enumerate() {
        d.push_str(&format!(
            "{}{},{}",
            if i == 0 { "M" } else { "L" },
            num(q.0),
            num(q.1)
        ));
    }
    d.push('Z');
    d
}

pub fn svg_d(path: &[Cubic]) -> String {
    use amonhen_pele::svg::builder::num;
    let mut d = String::new();
    let mut at: Option<P> = None;
    for c in path {
        if at.is_none_or(|p| (p.0 - c.0.0).abs() > 1e-6 || (p.1 - c.0.1).abs() > 1e-6) {
            d.push_str(&format!("M{},{}", num(c.0.0), num(c.0.1)));
        }
        d.push_str(&format!(
            "C{},{} {},{} {},{}",
            num(c.1.0),
            num(c.1.1),
            num(c.2.0),
            num(c.2.1),
            num(c.3.0),
            num(c.3.1)
        ));
        at = Some(c.3);
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clips_and_shortens() {
        let mut path = through(&[(0.0, 0.0), (0.0, 100.0)], false, true);
        clip_start(&mut path, &|p| p.0.hypot(p.1) < 10.0);
        assert!((path[0].0.1 - 10.0).abs() < 0.01, "{path:?}");
        clip_end(&mut path, &|p| (p.1 - 100.0).abs() < 20.0);
        assert!((path.last().unwrap().3.1 - 80.0).abs() < 0.01);
        let (tip, dir) = shorten_end(&mut path, 10.0).unwrap();
        assert!((tip.1 - 80.0).abs() < 0.01 && (dir.1 - 1.0).abs() < 1e-6);
        assert!((path.last().unwrap().3.1 - 70.0).abs() < 0.1);
        assert!((total_length(&path) - 60.0).abs() < 0.1);
        let m = midpoint(&path).unwrap();
        assert!((m.1 - 40.0).abs() < 0.5);
        let off = offset(&path, 2.0);
        assert!((off[0].0.0 + 2.0).abs() < 1e-9);
        assert!(svg_d(&path).starts_with("M0,10C"));
    }

    #[test]
    fn short_paths_keep_the_arrow() {
        let mut path = vec![Cubic::line((0.0, 0.0), (0.0, 4.0))];
        let (tip, dir) = shorten_end(&mut path, 10.0).unwrap();
        assert_eq!(tip, (0.0, 4.0));
        assert!((dir.1 - 1.0).abs() < 1e-9);
        assert!(path.is_empty());
    }
}
