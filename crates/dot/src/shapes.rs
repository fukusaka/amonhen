//! ノードの形: 名前から形の決まり、ラベルを収める大きさ、外形（多角形・楕円）、線を止める内外の判定、SVG。
//! 多角形の形（辺の数・ゆがみ・傾き・回転）は Graphviz の属性の説明に合わせて自分で作った。

use std::f64::consts::PI;

use amonhen_pele::svg::builder::num;

/// 形の種類。
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// 多角形（単位の頂点。y は下向き、外接の箱は [-1, 1]）。
    Polygon(Vec<(f64, f64)>),
    Ellipse,
    Point,
    /// 外形を描かない（plaintext・plain・none）。
    Plain,
    Record {
        rounded: bool,
    },
    /// 箱に飾りを足す形（cylinder・note など）。名前は小文字。
    Special(&'static str),
}

/// 形の決まり。
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeSpec {
    pub kind: Kind,
    pub peripheries: usize,
    pub regular: bool,
    /// 角の小さな線（Msquare・Mdiamond・Mcircle）。
    pub diagonals: bool,
    /// 知らない形（箱で描く）。
    pub unknown: bool,
    /// 余白なし（plain）。
    pub no_margin: bool,
    /// ゆがみ・傾きのある多角形: 大きさはゆがみのない形（`base`）で決めて、`grow` 倍する（Graphviz と同じく、
    /// ゆがんだ形をそのまま囲む大きさにはしない）。
    pub base: Option<Kind>,
    pub grow: (f64, f64),
}

const SPECIALS: &[&str] = &[
    "cylinder",
    "note",
    "tab",
    "folder",
    "box3d",
    "component",
    "underline",
    "cds",
    "promoter",
    "terminator",
    "utr",
    "primersite",
    "restrictionsite",
    "fivepoverhang",
    "threepoverhang",
    "noverhang",
    "assembly",
    "signature",
    "insulator",
    "ribosite",
    "rnastab",
    "proteasesite",
    "proteinstab",
    "rpromoter",
    "rarrow",
    "larrow",
    "lpromoter",
];

/// 辺の数・回転（度、反時計回り）・ゆがみ・傾きからの単位の多角形。
pub fn polygon(sides: usize, orientation: f64, distortion: f64, skew: f64) -> Vec<(f64, f64)> {
    finish(rotate(raw_polygon(sides, distortion, skew), orientation))
}

/// ゆがみ・傾きの分だけ、回さない形より大きくする倍率（横、縦）。回してから外接の箱を比べる。
///
/// Graphviz の絵の大きさ（辺の数 3〜100・ゆがみ・傾き・回転の 700 通り）に合わせた形: 単位円の上の頂点を、
/// `x' = x·(k + d·y·0.5 / cos(π/n)) + 0.3·s·y`（`k = √(1 + (|d| + |s|)²)`）で動かす。大きさの違いは
/// 中央値で 2% ほど、いちばん離れる三角と四角の一部で 30% ほど。
pub fn polygon_growth(sides: usize, orientation: f64, distortion: f64, skew: f64) -> (f64, f64) {
    if distortion == 0.0 && skew == 0.0 {
        return (1.0, 1.0);
    }
    let half = |pts: Vec<(f64, f64)>| {
        pts.iter().fold((0f64, 0f64), |(w, h), &(x, y)| {
            (w.max(x.abs()), h.max(y.abs()))
        })
    };
    let (qw, qh) = half(rotate(raw_polygon(sides, distortion, skew), orientation));
    let (vw, vh) = half(rotate(raw_polygon(sides, 0.0, 0.0), orientation));
    let grow = |q: f64, v: f64| if v > 1e-9 { (q / v).max(1.0) } else { 1.0 };
    (grow(qw, vw), grow(qh, vh))
}

/// 単位円の上の頂点（底が平ら、y は下向き）をゆがみ・傾きで動かしたもの（大きさはそろえない）。
fn raw_polygon(sides: usize, distortion: f64, skew: f64) -> Vec<(f64, f64)> {
    let n = sides.clamp(3, 100);
    let k = (distortion.abs() + skew.abs()).hypot(1.0);
    let d = distortion * 0.5 / (PI / n as f64).cos();
    let mut pts: Vec<(f64, f64)> = Vec::with_capacity(n);
    for i in 0..n {
        // 底が平らになる角度から（y は上向きで作り、あとで下向きにする）
        let a = 1.5 * PI - PI / n as f64 + 2.0 * PI * i as f64 / n as f64;
        let (x, y) = (a.cos(), a.sin());
        let x = x * (k + d * y) + 0.3 * skew * y;
        pts.push((x, -y));
    }
    pts
}

/// 絵の上で反時計回りに回す（Graphviz の `orientation` と同じ向き。y は下向きなので角度の符号を逆に）。
fn rotate(pts: Vec<(f64, f64)>, degrees: f64) -> Vec<(f64, f64)> {
    if degrees % 360.0 == 0.0 {
        return pts;
    }
    let r = -degrees.to_radians();
    let (s, c) = r.sin_cos();
    pts.into_iter()
        .map(|(x, y)| (x * c - y * s, x * s + y * c))
        .collect()
}

/// 外接の箱を [-1, 1] に広げる。
fn finish(pts: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for &(x, y) in &pts {
        x0 = x0.min(x);
        x1 = x1.max(x);
        y0 = y0.min(y);
        y1 = y1.max(y);
    }
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let (sx, sy) = (((x1 - x0) / 2.0).max(1e-9), ((y1 - y0) / 2.0).max(1e-9));
    pts.into_iter()
        .map(|(x, y)| ((x - cx) / sx, (y - cy) / sy))
        .collect()
}

fn fixed(pts: &[(f64, f64)]) -> Vec<(f64, f64)> {
    finish(pts.to_vec())
}

fn star() -> Vec<(f64, f64)> {
    let mut pts = Vec::new();
    for k in 0..10 {
        let a = -PI / 2.0 + PI * k as f64 / 5.0;
        let r = if k % 2 == 0 { 1.0 } else { 0.382 };
        pts.push((r * a.cos(), r * a.sin()));
    }
    finish(pts)
}

/// 形の名前と多角形の属性から。
pub fn spec(
    name: &str,
    sides: Option<f64>,
    orientation: f64,
    distortion: f64,
    skew: f64,
    regular: bool,
) -> ShapeSpec {
    let lower = name.trim().to_ascii_lowercase();
    let mut s = ShapeSpec {
        kind: Kind::Polygon(fixed(&[(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)])),
        peripheries: 1,
        regular,
        diagonals: false,
        unknown: false,
        no_margin: false,
        base: None,
        grow: (1.0, 1.0),
    };
    let poly = |n: usize, o: f64, d: f64, k: f64| Kind::Polygon(polygon(n, o + orientation, d, k));
    s.kind = match lower.as_str() {
        "box" | "rect" | "rectangle" => {
            if orientation % 360.0 != 0.0 {
                poly(4, 0.0, 0.0, 0.0)
            } else {
                s.kind
            }
        }
        "square" => {
            s.regular = true;
            s.kind
        }
        "polygon" => {
            let n = sides.unwrap_or(4.0);
            let n = if n.is_finite() { n.round() } else { 4.0 };
            if n < 3.0 {
                // 辺が 3 未満は楕円（Graphviz と同じ）。ゆがみがあれば、egg と同じ多角形
                if distortion != 0.0 || skew != 0.0 {
                    s.base = Some(Kind::Ellipse);
                    s.grow = polygon_growth(48, orientation, distortion, skew);
                    poly(48, 0.0, distortion, skew)
                } else {
                    Kind::Ellipse
                }
            } else {
                if distortion != 0.0 || skew != 0.0 {
                    s.base = Some(poly(n as usize, 0.0, 0.0, 0.0));
                    s.grow = polygon_growth(n as usize, orientation, distortion, skew);
                }
                poly(n as usize, 0.0, distortion, skew)
            }
        }
        "ellipse" | "oval" | "" => Kind::Ellipse,
        "circle" => {
            s.regular = true;
            Kind::Ellipse
        }
        "doublecircle" => {
            s.regular = true;
            s.peripheries = 2;
            Kind::Ellipse
        }
        "mcircle" => {
            s.regular = true;
            s.diagonals = true;
            Kind::Ellipse
        }
        "point" => Kind::Point,
        "egg" => {
            s.base = Some(Kind::Ellipse);
            s.grow = polygon_growth(48, orientation, -0.3, 0.0);
            poly(48, 0.0, -0.3, 0.0)
        }
        "triangle" => poly(3, 0.0, 0.0, 0.0),
        "invtriangle" => poly(3, 180.0, 0.0, 0.0),
        "plaintext" | "none" => {
            s.peripheries = 0;
            Kind::Plain
        }
        "plain" => {
            s.peripheries = 0;
            s.no_margin = true;
            Kind::Plain
        }
        "diamond" => poly(4, 45.0, 0.0, 0.0),
        "mdiamond" => {
            s.diagonals = true;
            poly(4, 45.0, 0.0, 0.0)
        }
        "msquare" => {
            s.regular = true;
            s.diagonals = true;
            s.kind
        }
        "trapezium" => Kind::Polygon(finish(rotate(
            fixed(&[(-1.0, 1.0), (1.0, 1.0), (0.6, -1.0), (-0.6, -1.0)]),
            orientation,
        ))),
        "invtrapezium" => Kind::Polygon(finish(rotate(
            fixed(&[(-0.6, 1.0), (0.6, 1.0), (1.0, -1.0), (-1.0, -1.0)]),
            orientation,
        ))),
        "parallelogram" => Kind::Polygon(finish(rotate(
            fixed(&[(-1.0, 1.0), (0.6, 1.0), (1.0, -1.0), (-0.6, -1.0)]),
            orientation,
        ))),
        "house" => Kind::Polygon(finish(rotate(
            fixed(&[
                (-1.0, 1.0),
                (1.0, 1.0),
                (1.0, -0.2),
                (0.0, -1.0),
                (-1.0, -0.2),
            ]),
            orientation,
        ))),
        "invhouse" => Kind::Polygon(finish(rotate(
            fixed(&[
                (-1.0, -1.0),
                (1.0, -1.0),
                (1.0, 0.2),
                (0.0, 1.0),
                (-1.0, 0.2),
            ]),
            orientation,
        ))),
        "pentagon" => poly(5, 0.0, 0.0, 0.0),
        "hexagon" => poly(6, 0.0, 0.0, 0.0),
        "septagon" => poly(7, 0.0, 0.0, 0.0),
        "octagon" => poly(8, 0.0, 0.0, 0.0),
        "doubleoctagon" => {
            s.peripheries = 2;
            poly(8, 0.0, 0.0, 0.0)
        }
        "tripleoctagon" => {
            s.peripheries = 3;
            poly(8, 0.0, 0.0, 0.0)
        }
        "star" => Kind::Polygon(star()),
        "record" => Kind::Record { rounded: false },
        "mrecord" => Kind::Record { rounded: true },
        other => match SPECIALS.iter().find(|n| **n == other) {
            Some(n) => Kind::Special(n),
            None => {
                s.unknown = true;
                s.kind
            }
        },
    };
    s
}

fn inside_polygon(pts: &[(f64, f64)], x: f64, y: f64) -> bool {
    let mut inside = false;
    let n = pts.len();
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = pts[i];
        let (xj, yj) = pts[j];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// 単位の形の中か（多角形は箱の [-1, 1]）。
fn inside_unit(kind: &Kind, x: f64, y: f64) -> bool {
    match kind {
        Kind::Ellipse | Kind::Point => x * x + y * y <= 1.0 + 1e-9,
        Kind::Polygon(pts) => inside_polygon(pts, x, y),
        _ => x.abs() <= 1.0 + 1e-9 && y.abs() <= 1.0 + 1e-9,
    }
}

/// ラベルの箱 (w, h) を収める形の大きさ。
/// ラベルの箱（`w`×`h`）を入れる形の大きさ。`min_h` は形の高さの下限（なければ 0）。
///
/// 楕円と凸の多角形は Graphviz の絵で測った大きさ: ラベルの箱を囲む楕円（箱の √2 倍）にし、高さの下限が
/// その楕円より高ければ、下限の高さの楕円に箱が入る幅（`w / √(1 - (h / min_h)²)`）で高さは箱のまま。多角形は
/// さらに辺の数 n で `cos(π / n)` で割る（三角は 2.83 倍、四角は 2 倍、五角は 1.75 倍）。ラベルが角にはみ出す
/// こともあるが、Graphviz と同じ。星と円柱も測った形。ほかのへこんだ多角形は、箱の角がみな中に入る大きさ。
pub fn fit(kind: &Kind, w: f64, h: f64, min_h: f64) -> (f64, f64) {
    let ellipse = |w: f64, h: f64| -> (f64, f64) {
        let hh = h * std::f64::consts::SQRT_2;
        if min_h > hh && h > 0.0 {
            (w / (1.0 - (h / min_h).powi(2)).sqrt(), h)
        } else {
            (w * std::f64::consts::SQRT_2, hh)
        }
    };
    match kind {
        Kind::Ellipse => ellipse(w, h),
        Kind::Polygon(pts) if !is_box(pts) && convex(pts) => {
            let (ew, eh) = ellipse(w, h);
            let c = (PI / pts.len().max(3) as f64).cos();
            (ew / c, eh / c)
        }
        // 星（10 点のへこんだ形）は正しい星のまま: 幅は w の φ² 倍か h / sin 36° の φ² 倍の大きいほう、高さは
        // 幅の cos 18° 倍（Graphviz の絵で測った）
        Kind::Polygon(pts) if pts.len() == 10 && !convex(pts) => {
            let phi2 = (3.0 + 5f64.sqrt()) / 2.0;
            let width = w.max(h / (PI / 5.0).sin()) * phi2;
            (width, width * (PI / 10.0).cos())
        }
        // 円柱は上下のふたの分だけ高い（Graphviz の絵で測った、高さの 1.375 倍）
        Kind::Special("cylinder") => (w, h * 1.375),
        Kind::Polygon(pts) if !is_box(pts) => {
            if w <= 0.0 || h <= 0.0 {
                return (w, h);
            }
            // 横と縦の倍率 (kx, ky) のうち、ラベルの箱の角がみな中に入り、面積が最も小さいもの
            let fits = |kx: f64, ky: f64| {
                [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                    .iter()
                    .all(|(cx, cy)| inside_polygon(pts, cx / kx, cy / ky))
            };
            let mut best = (8.0, 8.0);
            for step in 0..=24 {
                // 横の倍率は縦の 0.5〜4 倍
                let ratio = 0.5 * 8f64.powf(step as f64 / 24.0);
                let (mut lo, mut hi) = (0.5, 8.0);
                if !fits(hi * ratio, hi) {
                    continue;
                }
                for _ in 0..30 {
                    let mid = (lo + hi) / 2.0;
                    if fits(mid * ratio, mid) {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                if hi * ratio * hi < best.0 * best.1 {
                    best = (hi * ratio, hi);
                }
            }
            (w * best.0, h * best.1)
        }
        _ => (w, h),
    }
}

/// 凸の多角形か（辺の向きの回りがみな同じ）。
fn convex(pts: &[(f64, f64)]) -> bool {
    let n = pts.len();
    let mut sign = 0.0;
    for i in 0..n {
        let (a, b, c) = (pts[i], pts[(i + 1) % n], pts[(i + 2) % n]);
        let cross = (b.0 - a.0) * (c.1 - b.1) - (b.1 - a.1) * (c.0 - b.0);
        if cross.abs() < 1e-12 {
            continue;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    true
}

fn is_box(pts: &[(f64, f64)]) -> bool {
    pts.len() == 4
        && pts
            .iter()
            .all(|(x, y)| (x.abs() - 1.0).abs() < 1e-9 && (y.abs() - 1.0).abs() < 1e-9)
}

/// 置いたノードの形。
#[derive(Clone, Debug)]
pub struct Geom {
    pub kind: Kind,
    /// いちばん内の外形の大きさ。
    pub w: f64,
    pub h: f64,
    pub peripheries: usize,
}

/// 外形の間。
pub const GAP: f64 = 4.0;

impl Geom {
    /// いちばん外の大きさ。
    pub fn outer(&self) -> (f64, f64) {
        let extra = 2.0 * GAP * self.peripheries.saturating_sub(1) as f64;
        (self.w + extra, self.h + extra)
    }

    /// 中心からの (x, y) が形の中か（辺の線を止めるのに使う。いちばん外の外形）。
    pub fn inside(&self, x: f64, y: f64) -> bool {
        let (w, h) = self.outer();
        let (hw, hh) = ((w / 2.0).max(1e-6), (h / 2.0).max(1e-6));
        inside_unit(&self.kind, x / hw, y / hh)
    }

    /// 中心から (dx, dy) の向きに出た所の外形の上の点。
    pub fn boundary(&self, dx: f64, dy: f64) -> (f64, f64) {
        let len = dx.hypot(dy);
        if len < 1e-9 {
            return (0.0, 0.0);
        }
        let (w, h) = self.outer();
        let far = w.max(h) + 1.0;
        let (ux, uy) = (dx / len * far, dy / len * far);
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..40 {
            let mid = (lo + hi) / 2.0;
            if self.inside(ux * mid, uy * mid) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        (ux * lo, uy * lo)
    }
}

/// 外形の線の SVG（中心 (cx, cy)）。`fill` は内の外形の塗りの属性、`stroke` は線の属性。`rounded` は角を丸める。
pub fn outline(
    g: &Geom,
    cx: f64,
    cy: f64,
    fill: &str,
    stroke: &str,
    rounded: bool,
    diagonals: bool,
) -> String {
    let mut out = String::new();
    for p in 0..g.peripheries.max(1) {
        let grow = 2.0 * GAP * p as f64;
        let (w, h) = (g.w + grow, g.h + grow);
        let paint = if p == 0 { fill } else { " fill=\"none\"" };
        if g.peripheries == 0 && p == 0 {
            // 線は描かないが、塗りは箱に
            if !fill.contains("fill=\"none\"") {
                out.push_str(&format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{fill} stroke=\"none\"/>",
                    num(cx - w / 2.0),
                    num(cy - h / 2.0),
                    num(w),
                    num(h)
                ));
            }
            break;
        }
        match &g.kind {
            Kind::Ellipse | Kind::Point => out.push_str(&format!(
                "<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"{paint}{stroke}/>",
                num(cx),
                num(cy),
                num(w / 2.0),
                num(h / 2.0)
            )),
            Kind::Polygon(pts) if !is_box(pts) => {
                let mut d = String::new();
                for (i, (x, y)) in pts.iter().enumerate() {
                    if i > 0 {
                        d.push(' ');
                    }
                    d.push_str(&format!(
                        "{},{}",
                        num(cx + x * w / 2.0),
                        num(cy + y * h / 2.0)
                    ));
                }
                out.push_str(&format!("<polygon points=\"{d}\"{paint}{stroke}/>"));
            }
            _ => {
                let r = if rounded {
                    (w.min(h) / 4.0).min(12.0)
                } else {
                    0.0
                };
                out.push_str(&format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}{paint}{stroke}/>",
                    num(cx - w / 2.0),
                    num(cy - h / 2.0),
                    num(w),
                    num(h),
                    if r > 0.0 {
                        format!(" rx=\"{}\"", num(r))
                    } else {
                        String::new()
                    }
                ));
            }
        }
    }
    if diagonals {
        out.push_str(&diagonal_marks(g, cx, cy, stroke));
    }
    out
}

/// Msquare・Mdiamond・Mcircle の角の線。
fn diagonal_marks(g: &Geom, cx: f64, cy: f64, stroke: &str) -> String {
    let (w, h) = (g.w, g.h);
    let mut d = String::new();
    let mut seg = |x1: f64, y1: f64, x2: f64, y2: f64| {
        d.push_str(&format!(
            "M{},{}L{},{}",
            num(cx + x1),
            num(cy + y1),
            num(cx + x2),
            num(cy + y2)
        ));
    };
    match &g.kind {
        Kind::Ellipse => {
            // 上と下の弦
            let y = h / 2.0 * 0.8;
            let x = w / 2.0 * (1.0f64 - 0.64).sqrt();
            seg(-x, -y, x, -y);
            seg(-x, y, x, y);
        }
        Kind::Polygon(pts) if !is_box(pts) => {
            // 菱形の 4 つの角を切る短い線
            let k = 0.25;
            let n = pts.len();
            for i in 0..n {
                let (px, py) = (pts[i].0 * w / 2.0, pts[i].1 * h / 2.0);
                let a = pts[(i + n - 1) % n];
                let b = pts[(i + 1) % n];
                let (ax, ay) = (a.0 * w / 2.0, a.1 * h / 2.0);
                let (bx, by) = (b.0 * w / 2.0, b.1 * h / 2.0);
                seg(
                    px + (ax - px) * k,
                    py + (ay - py) * k,
                    px + (bx - px) * k,
                    py + (by - py) * k,
                );
            }
        }
        _ => {
            let k = (w.min(h) * 0.25).min(12.0);
            let (hw, hh) = (w / 2.0, h / 2.0);
            seg(-hw, -hh + k, -hw + k, -hh);
            seg(hw - k, -hh, hw, -hh + k);
            seg(hw, hh - k, hw - k, hh);
            seg(-hw + k, hh, -hw, hh - k);
        }
    }
    format!("<path d=\"{d}\" fill=\"none\"{stroke}/>")
}

/// 飾りの形（cylinder など）の SVG。`fill`・`stroke` は属性。
pub fn special(name: &str, w: f64, h: f64, cx: f64, cy: f64, fill: &str, stroke: &str) -> String {
    let (l, t, r, b) = (cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0);
    let p = |x: f64, y: f64| format!("{},{}", num(x), num(y));
    let rect = format!(
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{fill}{stroke}/>",
        num(l),
        num(t),
        num(w),
        num(h)
    );
    let path = |d: String, filled: bool| {
        format!(
            "<path d=\"{d}\"{}{stroke}/>",
            if filled { fill } else { " fill=\"none\"" }
        )
    };
    let k = (w.min(h) * 0.2).clamp(3.0, 10.0);
    match name {
        "cylinder" => {
            let ry = (h * 0.1).clamp(2.0, 9.0);
            let rx = w / 2.0;
            format!(
                "{}{}",
                path(
                    format!(
                        "M{}L{}A{} {} 0 0 0 {}L{}A{} {} 0 0 0 {}Z",
                        p(l, t + ry),
                        p(l, b - ry),
                        num(rx),
                        num(ry),
                        p(r, b - ry),
                        p(r, t + ry),
                        num(rx),
                        num(ry),
                        p(l, t + ry)
                    ),
                    true
                ),
                path(
                    format!(
                        "M{}A{} {} 0 0 0 {}",
                        p(l, t + ry),
                        num(rx),
                        num(ry),
                        p(r, t + ry)
                    ),
                    false
                )
            )
        }
        "note" => format!(
            "{}{}",
            path(
                format!(
                    "M{}L{}L{}L{}L{}Z",
                    p(l, t),
                    p(r - k, t),
                    p(r, t + k),
                    p(r, b),
                    p(l, b)
                ),
                true
            ),
            path(
                format!("M{}L{}L{}", p(r - k, t), p(r - k, t + k), p(r, t + k)),
                false
            )
        ),
        "tab" => {
            let lip = (k * 0.6).min(h / 3.0);
            format!(
                "{rect}{}",
                path(
                    format!(
                        "M{}L{}L{}",
                        p(l + 2.0, t),
                        p(l + 2.0, t + lip),
                        p(l + 2.0 + 2.0 * k, t + lip)
                    ),
                    false
                )
            )
        }
        "folder" => {
            let lip = (k * 0.6).min(h / 3.0);
            path(
                format!(
                    "M{}L{}L{}L{}L{}L{}L{}Z",
                    p(l, b),
                    p(l, t + lip),
                    p(r - 3.0 * k, t + lip),
                    p(r - 2.5 * k, t),
                    p(r - 0.5 * k, t),
                    p(r, t + lip),
                    p(r, b)
                ),
                true,
            )
        }
        "box3d" => format!(
            "{}{}",
            path(
                format!(
                    "M{}L{}L{}L{}L{}L{}Z",
                    p(l, t + k),
                    p(l + k, t),
                    p(r, t),
                    p(r, b - k),
                    p(r - k, b),
                    p(l, b)
                ),
                true
            ),
            path(
                format!(
                    "M{}L{}L{}M{}L{}",
                    p(l, t + k),
                    p(r - k, t + k),
                    p(r - k, b),
                    p(r - k, t + k),
                    p(r, t)
                ),
                false
            )
        ),
        "component" => {
            let bw = k.min(8.0);
            let bh = (h * 0.15).clamp(3.0, 6.0);
            let box_at = |y: f64| {
                format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{fill}{stroke}/>",
                    num(l - bw / 2.0),
                    num(y - bh / 2.0),
                    num(bw),
                    num(bh)
                )
            };
            format!("{rect}{}{}", box_at(t + h * 0.3), box_at(b - h * 0.3))
        }
        "underline" => format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{fill} stroke=\"none\"/>{}",
            num(l),
            num(t),
            num(w),
            num(h),
            path(format!("M{}L{}", p(l, b), p(r, b)), false)
        ),
        "cds" => path(
            format!(
                "M{}L{}L{}L{}L{}Z",
                p(l, t),
                p(r - k, t),
                p(r, cy),
                p(r - k, b),
                p(l, b)
            ),
            true,
        ),
        "rarrow" | "larrow" => {
            let right = name == "rarrow";
            let (s, e) = if right { (l, r) } else { (r, l) };
            let dir = if right { 1.0 } else { -1.0 };
            let neck = e - dir * k * 1.5;
            path(
                format!(
                    "M{}L{}L{}L{}L{}L{}L{}Z",
                    p(s, t + h * 0.25),
                    p(neck, t + h * 0.25),
                    p(neck, t),
                    p(e, cy),
                    p(neck, b),
                    p(neck, b - h * 0.25),
                    p(s, b - h * 0.25)
                ),
                true,
            )
        }
        "promoter" | "rpromoter" | "lpromoter" => {
            // 箱と、上に折れた矢印（合成生物学の印）
            let right = name != "lpromoter";
            let (x0, x1) = if right {
                (l + w * 0.3, r - w * 0.15)
            } else {
                (r - w * 0.3, l + w * 0.15)
            };
            let dir = if right { 1.0 } else { -1.0 };
            let top = t - k;
            let mut s = rect.clone();
            if name != "promoter" {
                s = rect.clone();
            }
            s.push_str(&path(
                format!("M{}L{}L{}", p(x0, t), p(x0, top), p(x1, top)),
                false,
            ));
            s.push_str(&path(
                format!(
                    "M{}L{}L{}Z",
                    p(x1, top - k * 0.4),
                    p(x1 + dir * k * 0.6, top),
                    p(x1, top + k * 0.4)
                ),
                true,
            ));
            s
        }
        "terminator" => format!(
            "{rect}{}",
            path(
                format!(
                    "M{}L{}M{}L{}",
                    p(cx, t),
                    p(cx, t - k),
                    p(cx - k, t - k),
                    p(cx + k, t - k)
                ),
                false
            )
        ),
        "utr" | "ribosite" => format!(
            "{rect}{}",
            path(
                format!(
                    "M{}A{} {} 0 0 1 {}",
                    p(cx - k, t),
                    num(k),
                    num(k),
                    p(cx + k, t)
                ),
                false
            )
        ),
        "primersite" => format!(
            "{rect}{}",
            path(
                format!(
                    "M{}L{}L{}",
                    p(l + w * 0.2, t - k * 0.6),
                    p(r - w * 0.2, t - k * 0.6),
                    p(r - w * 0.2 - k * 0.6, t - k * 1.2)
                ),
                false
            )
        ),
        "restrictionsite" => format!(
            "{rect}{}",
            path(
                format!(
                    "M{}L{}L{}L{}",
                    p(l + w * 0.3, t - k),
                    p(cx, t - k),
                    p(cx, b + k),
                    p(r - w * 0.3, b + k)
                ),
                false
            )
        ),
        "fivepoverhang" | "threepoverhang" | "noverhang" | "assembly" | "signature" => {
            let mut s = rect.clone();
            let mid = cy;
            match name {
                "fivepoverhang" => {
                    s.push_str(&path(format!("M{}L{}", p(l, mid), p(cx, mid)), false))
                }
                "threepoverhang" => {
                    s.push_str(&path(format!("M{}L{}", p(cx, mid), p(r, mid)), false))
                }
                "noverhang" => s.push_str(&path(format!("M{}L{}", p(l, mid), p(r, mid)), false)),
                "assembly" => s.push_str(&path(
                    format!(
                        "M{}L{}M{}L{}",
                        p(l, mid - k * 0.3),
                        p(r, mid - k * 0.3),
                        p(l, mid + k * 0.3),
                        p(r, mid + k * 0.3)
                    ),
                    false,
                )),
                _ => s.push_str(&path(
                    format!(
                        "M{}L{}M{}L{}",
                        p(l + k * 0.4, b - k),
                        p(l + k, b - k * 0.4),
                        p(l + k * 0.4, b - k * 0.4),
                        p(l + k, b - k)
                    ),
                    false,
                )),
            }
            s
        }
        "insulator" => format!(
            "{rect}<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\"{stroke}/>",
            num(l + k * 0.5),
            num(t + k * 0.5),
            num((w - k).max(0.0)),
            num((h - k).max(0.0))
        ),
        "rnastab" | "proteinstab" | "proteasesite" => {
            let mut s = rect.clone();
            let rr = k * 0.5;
            s.push_str(&format!(
                "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"none\"{stroke}/>",
                num(cx),
                num(t - rr),
                num(rr)
            ));
            if name == "proteasesite" {
                s.push_str(&path(
                    format!(
                        "M{}L{}M{}L{}",
                        p(cx - rr, t - 2.0 * rr),
                        p(cx + rr, t),
                        p(cx + rr, t - 2.0 * rr),
                        p(cx - rr, t)
                    ),
                    false,
                ));
            }
            s
        }
        _ => rect,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distortion_and_skew_grow_the_node_as_in_graphviz() {
        // Graphviz の絵の大きさ（ラベルなし、10 インチの多角形）。(辺の数, ゆがみ, 回転, 傾き) → (横, 縦) の倍率
        let seen = [
            ((10, 0.85, 0.0, 0.0), (1.3625, 1.0)),
            ((10, 0.0, 0.0, -0.9), (1.3528, 1.0)),
            ((10, 0.5, 30.0, 0.0), (1.2306, 1.0514)),
            ((100, 1.0, 45.0, 0.0), (1.4653, 1.4653)),
            ((6, 2.0, 0.0, 0.0), (2.2361, 1.0)),
            ((10, 1.0, 90.0, 0.0), (1.0, 1.4972)),
        ];
        for ((n, d, o, k), (gw, gh)) in seen {
            let (w, h) = polygon_growth(n, o, d, k);
            assert!((w / gw - 1.0).abs() < 0.06, "{n} {d} {o} {k}: {w} vs {gw}");
            assert!((h / gh - 1.0).abs() < 0.06, "{n} {d} {o} {k}: {h} vs {gh}");
        }
        assert_eq!(polygon_growth(10, 32.0, 0.0, 0.0), (1.0, 1.0));
    }

    #[test]
    fn orientation_turns_counterclockwise() {
        // 底の平らな三角を 20 度: 頂点は左へ（Graphviz の絵と同じ）
        let pts = polygon(3, 20.0, 0.0, 0.0);
        let apex = pts
            .iter()
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .copied()
            .unwrap();
        assert!(apex.0 < 0.0, "{pts:?}");
        // 回した家も箱からはみ出さない
        let spec = spec("house", None, 30.0, 0.0, 0.0, false);
        let Kind::Polygon(pts) = spec.kind else {
            panic!()
        };
        assert!(
            pts.iter()
                .all(|p| p.0.abs() <= 1.0 + 1e-9 && p.1.abs() <= 1.0 + 1e-9)
        );
    }

    #[test]
    fn makes_named_shapes() {
        let s = spec("box", None, 0.0, 0.0, 0.0, false);
        assert!(matches!(&s.kind, Kind::Polygon(p) if is_box(p)));
        assert!(spec("weird", None, 0.0, 0.0, 0.0, false).unknown);
        assert_eq!(
            spec("doublecircle", None, 0.0, 0.0, 0.0, false).peripheries,
            2
        );
        assert_eq!(spec("plaintext", None, 0.0, 0.0, 0.0, false).peripheries, 0);
        assert!(matches!(
            spec("Mrecord", None, 0.0, 0.0, 0.0, false).kind,
            Kind::Record { rounded: true }
        ));
        assert!(matches!(
            spec("cylinder", None, 0.0, 0.0, 0.0, false).kind,
            Kind::Special("cylinder")
        ));
        let tri = polygon(3, 0.0, 0.0, 0.0);
        // 底が平ら、頂点は上
        assert_eq!(tri.len(), 3);
        assert!(tri.iter().filter(|p| (p.1 - 1.0).abs() < 1e-9).count() == 2);
        assert!(
            tri.iter()
                .any(|p| (p.1 + 1.0).abs() < 1e-9 && p.0.abs() < 1e-9)
        );
        // 辺が 3 未満の polygon は楕円、ゆがみがあれば卵の形
        assert!(matches!(
            spec("polygon", Some(0.0), 0.0, 0.0, 0.0, false).kind,
            Kind::Ellipse
        ));
        assert!(matches!(
            spec("polygon", Some(2.0), 0.0, -0.3, 0.0, false).kind,
            Kind::Polygon(p) if p.len() == 48
        ));
        let diamond = polygon(4, 45.0, 0.0, 0.0);
        assert!(
            diamond
                .iter()
                .any(|p| p.0.abs() < 1e-9 && (p.1 + 1.0).abs() < 1e-9)
        );
    }

    #[test]
    fn fits_labels_inside() {
        let (w, h) = fit(&Kind::Ellipse, 10.0, 10.0, 0.0);
        assert!((w - 14.142).abs() < 0.01 && (h - w).abs() < 1e-9);
        let diamond = Kind::Polygon(polygon(4, 45.0, 0.0, 0.0));
        let (w, h) = fit(&diamond, 10.0, 4.0, 0.0);
        assert!((w - 20.0).abs() < 0.01 && (h - 8.0).abs() < 0.01, "{w} {h}");
        let star = Kind::Polygon(star());
        let (w, _) = fit(&star, 10.0, 10.0, 0.0);
        assert!(w > 20.0);
    }

    #[test]
    fn finds_boundaries() {
        let g = Geom {
            kind: Kind::Ellipse,
            w: 40.0,
            h: 20.0,
            peripheries: 1,
        };
        let (x, y) = g.boundary(1.0, 0.0);
        assert!((x - 20.0).abs() < 0.01 && y.abs() < 0.01);
        let (x, y) = g.boundary(0.0, -5.0);
        assert!(x.abs() < 0.01 && (y + 10.0).abs() < 0.01);
        let two = Geom {
            peripheries: 2,
            ..g.clone()
        };
        assert!((two.boundary(1.0, 0.0).0 - 24.0).abs() < 0.01);
    }
}
