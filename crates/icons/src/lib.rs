//! どの FE でも同じ形で描くアイコン。
//!
//! 形は `svg/` の SVG（16×16 の格子、太さ 1.5 の線、端と角は丸）で、ビルドのときに線のパスへ直して持つ。各 FE は
//! パスを自分の描画で線として引く（塗りではない）。色は FE が決める（ふつうは文字の色）。決めごとと経緯は
//! `docs/icons.md`。

use std::fmt::Write as _;

/// 形を決めた格子の一辺。
pub const GRID: f32 = 16.0;
/// 線の太さ（格子の単位）。
pub const STROKE_WIDTH: f32 = 1.5;

/// 1 本の線（端と角は丸）。
#[derive(Debug)]
pub struct Shape {
    /// SVG のパスの書き方（`M`・`L`・`C`・`A` など、相対も含む）。
    pub d: &'static str,
    /// 破線の長さ（空なら実線）。長さ 0 の線と丸い端で点になる。
    pub dash: &'static [f32],
    /// 同じ線を、移動・直線・3 次ベジェ・閉じるだけの列にしたもの（弧はベジェで近づけてある）。操作の番号（[`Op`]）
    /// に続けて座標: 移動と直線は x y、ベジェは x1 y1 x2 y2 x y、閉じるは座標なし。弧を描けない描画（CGPath に渡す
    /// とき、Direct2D）で使う。
    pub segments: &'static [f32],
}

/// [`Shape::segments`] の操作の番号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Op {
    Move = 0,
    Line = 1,
    Cubic = 2,
    Close = 3,
}

impl Op {
    /// 列の中の番号から。
    pub fn from_code(code: f32) -> Option<Self> {
        Some(match code as u8 {
            0 => Op::Move,
            1 => Op::Line,
            2 => Op::Cubic,
            3 => Op::Close,
            _ => return None,
        })
    }

    /// 番号に続く座標の数。
    pub fn arity(self) -> usize {
        match self {
            Op::Move | Op::Line => 2,
            Op::Cubic => 6,
            Op::Close => 0,
        }
    }
}

/// 1 つのアイコン。
#[derive(Debug)]
pub struct Icon {
    pub shapes: &'static [Shape],
}

include!(concat!(env!("OUT_DIR"), "/icons.rs"));

/// 描く大きさ・画素の細かさ・線の太さに合わせた 1 本の線（[`Icon::strokes`]）。座標と破線の長さは描く単位（pt・px）。
#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    /// [`Shape::segments`] と同じ形の列（操作の番号と座標）。
    pub segments: Vec<f32>,
    /// 破線の長さ（空なら実線）。
    pub dash: Vec<f32>,
}

impl Stroke {
    /// `M`・`L`・`C`・`Z` だけのパス（SVG と XAML のどちらでも読める）。
    pub fn markup(&self) -> String {
        let mut out = String::new();
        for (op, coords) in ops(&self.segments) {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push(match op {
                Op::Move => 'M',
                Op::Line => 'L',
                Op::Cubic => 'C',
                Op::Close => 'Z',
            });
            for &v in coords {
                write!(out, " {}", r(v as f64)).unwrap();
            }
        }
        out
    }
}

impl Icon {
    /// [`Icon::strokes`] を 1 枚の SVG にする（一辺 `size`、色 `color`、端と角は丸）。VSCode のように画像として描く
    /// FE のため。
    pub fn svg(&self, size: f32, pixel_scale: f32, stroke: f32, color: &str) -> String {
        let mut out = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{s}\" height=\"{s}\" viewBox=\"0 0 {s} {s}\" fill=\"none\" \
             stroke=\"{color}\" stroke-width=\"{w}\" stroke-linecap=\"round\" stroke-linejoin=\"round\">",
            s = r(size as f64),
            w = r(stroke as f64)
        );
        for s in self.strokes(size, pixel_scale, stroke) {
            write!(out, "<path d=\"{}\"", s.markup()).unwrap();
            if !s.dash.is_empty() {
                let dash: Vec<String> = s.dash.iter().map(|d| r(*d as f64)).collect();
                write!(out, " stroke-dasharray=\"{}\"", dash.join(" ")).unwrap();
            }
            out.push_str("/>");
        }
        out.push_str("</svg>\n");
        out
    }
}

/// 列を操作と座標の組に分ける。
fn ops(s: &[f32]) -> impl Iterator<Item = (Op, &[f32])> {
    let mut i = 0;
    std::iter::from_fn(move || {
        let op = Op::from_code(*s.get(i)?)?;
        let coords = &s[i + 1..i + 1 + op.arity()];
        i += 1 + op.arity();
        Some((op, coords))
    })
}

/// 移動から次の移動の前までの、1 つながりの線。点は画素の単位。
struct Part {
    /// 操作と、その座標（ベジェは制御点 2 つと端）。
    ops: Vec<(Op, Vec<(f64, f64)>)>,
}

impl Part {
    /// 各操作の終わりの点（閉じるは始まりの点）。
    fn ends(&self) -> Vec<(f64, f64)> {
        let start = self.ops[0].1[0];
        self.ops
            .iter()
            .map(|(op, pts)| {
                if *op == Op::Close {
                    start
                } else {
                    *pts.last().unwrap()
                }
            })
            .collect()
    }
}

impl Icon {
    /// 一辺 `size` の正方形に、太さ `stroke`（描く単位）で描く線。`pixel_scale` は描く単位あたりの画素の数（Retina
    /// なら 2）。縦と横の線を画素に合わせる（太さが奇数の画素なら画素の中心、偶数なら画素の境目に置く）ので、1 倍の
    /// 画面でもにじまない。同じ座標の縦・横の線はどこでも同じだけ動かし、縦・横の線を持たない部分（矢じり・丸）は、
    /// つながる点と同じだけ（なければ丸の真ん中を画素に合わせて）丸ごと動かして、形を崩さない。
    pub fn strokes(&self, size: f32, pixel_scale: f32, stroke: f32) -> Vec<Stroke> {
        let k = (size / GRID * pixel_scale) as f64;
        let odd = ((stroke * pixel_scale).round().max(1.0) as i64) % 2 == 1;
        let snap = |v: f64| {
            if odd {
                (v - 0.5).round() + 0.5
            } else {
                v.round()
            }
        };
        let key = |v: f64| (v * 1000.0).round() as i64;
        let parts: Vec<Vec<Part>> = self
            .shapes
            .iter()
            .map(|shape| {
                let mut parts: Vec<Part> = Vec::new();
                for (op, coords) in ops(shape.segments) {
                    let pts = coords
                        .chunks(2)
                        .map(|c| (c[0] as f64 * k, c[1] as f64 * k))
                        .collect();
                    if op == Op::Move {
                        parts.push(Part { ops: Vec::new() });
                    }
                    parts
                        .last_mut()
                        .expect("starts with a move")
                        .ops
                        .push((op, pts));
                }
                parts
            })
            .collect();
        // 縦の線の x と横の線の y を、画素に合わせた値へ。
        let mut xs = std::collections::HashMap::new();
        let mut ys = std::collections::HashMap::new();
        for part in parts.iter().flatten() {
            let ends = part.ends();
            for (n, (op, _)) in part.ops.iter().enumerate().skip(1) {
                if !matches!(op, Op::Line | Op::Close) {
                    continue;
                }
                let (a, b) = (ends[n - 1], ends[n]);
                if (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() > 1e-6 {
                    xs.insert(key(a.0), snap(a.0));
                }
                if (a.1 - b.1).abs() < 1e-6 && (a.0 - b.0).abs() > 1e-6 {
                    ys.insert(key(a.1), snap(a.1));
                }
            }
        }
        self.shapes
            .iter()
            .zip(&parts)
            .map(|(shape, parts)| {
                let mut out: Vec<f32> = Vec::new();
                for part in parts {
                    let ends = part.ends();
                    let has_axis = part.ops.iter().enumerate().skip(1).any(|(n, (op, _))| {
                        let (a, b) = (ends[n - 1], ends[n]);
                        matches!(op, Op::Line | Op::Close)
                            && ((a.0 - b.0).abs() < 1e-6 || (a.1 - b.1).abs() < 1e-6)
                            && (a.0 - b.0).abs() + (a.1 - b.1).abs() > 1e-6
                    });
                    // 丸ごと動かすときのずれ。つながる点（縦・横の線と同じ座標）があればそれに、なければ曲線だけの部分
                    // （丸）は真ん中を画素に合わせる。
                    let whole = (!has_axis).then(|| {
                        let dx = ends.iter().find_map(|p| xs.get(&key(p.0)).map(|v| v - p.0));
                        let dy = ends.iter().find_map(|p| ys.get(&key(p.1)).map(|v| v - p.1));
                        let curves = part
                            .ops
                            .iter()
                            .all(|(op, _)| matches!(op, Op::Move | Op::Cubic | Op::Close));
                        let lo = ends
                            .iter()
                            .fold((f64::MAX, f64::MAX), |m, p| (m.0.min(p.0), m.1.min(p.1)));
                        let hi = ends
                            .iter()
                            .fold((f64::MIN, f64::MIN), |m, p| (m.0.max(p.0), m.1.max(p.1)));
                        let (cx, cy) = ((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0);
                        let center = |c: f64| if curves { snap(c) - c } else { 0.0 };
                        (
                            dx.unwrap_or_else(|| center(cx)),
                            dy.unwrap_or_else(|| center(cy)),
                        )
                    });
                    let moved = |p: (f64, f64)| match whole {
                        Some((dx, dy)) => (p.0 + dx, p.1 + dy),
                        None => (
                            xs.get(&key(p.0)).copied().unwrap_or(p.0),
                            ys.get(&key(p.1)).copied().unwrap_or(p.1),
                        ),
                    };
                    let unit = pixel_scale as f64;
                    let put = |out: &mut Vec<f32>, p: (f64, f64)| {
                        out.extend([(p.0 / unit) as f32, (p.1 / unit) as f32])
                    };
                    // 直前の端（動かす前と後）。
                    let mut last = ((0.0, 0.0), (0.0, 0.0));
                    for (op, pts) in &part.ops {
                        out.push(*op as u8 as f32);
                        match op {
                            Op::Move | Op::Line => {
                                let p = moved(pts[0]);
                                put(&mut out, p);
                                last = (pts[0], p);
                            }
                            Op::Cubic => {
                                // 制御点は、つながる端と同じだけ動かす。
                                let end = moved(pts[2]);
                                let d0 = (last.1.0 - last.0.0, last.1.1 - last.0.1);
                                let d1 = (end.0 - pts[2].0, end.1 - pts[2].1);
                                put(&mut out, (pts[0].0 + d0.0, pts[0].1 + d0.1));
                                put(&mut out, (pts[1].0 + d1.0, pts[1].1 + d1.1));
                                put(&mut out, end);
                                last = (pts[2], end);
                            }
                            Op::Close => {}
                        }
                    }
                }
                Stroke {
                    segments: out,
                    dash: shape.dash.iter().map(|d| d * size / GRID).collect(),
                }
            })
            .collect()
    }
}

/// VSCode 拡張の `media/icons/` に置く SVG（パスと中身）。VSCode は拡張の SVG を画像として描き、テーマの色を受け継げ
/// ないので、ライトとダークの 2 枚に色を書き込む（VSCode の `icon.foreground` と `charts.green` の既定の色）。大きさは
/// 16、線は codicon と同じ 1px で、1 倍の画面に合わせて画素に乗せる（2 倍の画面でもそのまま画素に乗る）。
pub fn vscode_files() -> Vec<(String, String)> {
    let colors = [
        ("light", "", "#424242"),
        ("light", "-green", "#388A34"),
        ("dark", "", "#C5C5C5"),
        ("dark", "-green", "#89D185"),
    ];
    let mut files = Vec::new();
    for (name, icon) in ICONS {
        for (theme, suffix, color) in colors {
            files.push((
                format!("{theme}/{name}{suffix}.svg"),
                icon.svg(16.0, 1.0, 1.0, color),
            ));
        }
    }
    files
}

/// 名前（`svg/` のファイル名）でアイコンを引く。
pub fn get(name: &str) -> Option<&'static Icon> {
    ICONS.iter().find(|(n, _)| *n == name).map(|(_, icon)| icon)
}

/// 座標を短く書く（1/1000 まで）。
fn r(v: f64) -> String {
    let s = format!("{:.3}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_toolbar_icon_is_there() {
        for name in [
            "sidebar",
            "repositories",
            "branch",
            "new-branch",
            "fetch",
            "pull",
            "push",
            "publish",
            "merge",
            "rebase",
            "stash",
            "pop",
            "activity",
            "refresh",
            "undo",
            "search",
            "terminal",
            "editor",
            "files",
        ] {
            let icon = get(name).unwrap_or_else(|| panic!("{name}"));
            assert!(!icon.shapes.is_empty(), "{name}");
        }
    }

    /// どの列も移動で始まり、番号と座標の数が合い、座標は格子の中（少しはみ出すのは弧の近似）。
    #[test]
    fn segments_are_well_formed() {
        for (name, icon) in ICONS {
            for shape in icon.shapes {
                let s = shape.segments;
                assert_eq!(Op::from_code(s[0]), Some(Op::Move), "{name}");
                let mut i = 0;
                while i < s.len() {
                    let op = Op::from_code(s[i]).unwrap_or_else(|| panic!("{name}: op {}", s[i]));
                    for &v in &s[i + 1..i + 1 + op.arity()] {
                        assert!((-0.5..=16.5).contains(&v), "{name}: {v}");
                    }
                    i += 1 + op.arity();
                }
                assert_eq!(i, s.len(), "{name}");
            }
        }
    }

    /// 円（2 つの半円の弧）は 4 つのベジェになり、元の点に戻る。
    #[test]
    fn circle_becomes_four_cubics() {
        let pull = get("pull").unwrap();
        let circle = pull
            .shapes
            .iter()
            .find(|s| s.d.starts_with("M6.5 13A"))
            .unwrap();
        let s = circle.segments;
        let mut ops = Vec::new();
        let mut i = 0;
        while i < s.len() {
            let op = Op::from_code(s[i]).unwrap();
            ops.push(op);
            i += 1 + op.arity();
        }
        assert_eq!(
            ops.iter().filter(|&&op| op == Op::Cubic).count(),
            4,
            "{s:?}"
        );
        let n = s.len();
        assert_eq!(s[n - 1], 3.0);
        assert!(
            (s[n - 3] - 6.5).abs() < 1e-4 && (s[n - 2] - 13.0).abs() < 1e-4,
            "{s:?}"
        );
    }

    /// 縦・横の線が画素に乗る: 太さ 1px なら画素の中心（.5）、2px なら境目（整数）。
    #[test]
    fn strokes_snap_to_pixels() {
        for (stroke, frac) in [(1.0, 0.5), (2.0, 0.0)] {
            for (name, icon) in ICONS {
                for s in icon.strokes(16.0, 1.0, stroke) {
                    let pts: Vec<(Op, &[f32])> = ops(&s.segments).collect();
                    for w in pts.windows(2) {
                        let (a, b) = (w[0].1, w[1].1);
                        if w[1].0 != Op::Line || a.len() < 2 {
                            continue;
                        }
                        let (a, b) = ((a[a.len() - 2], a[a.len() - 1]), (b[0], b[1]));
                        if a.0 == b.0 && a.1 != b.1 {
                            assert_eq!(a.0.rem_euclid(1.0), frac, "{name} x {}", a.0);
                        }
                        if a.1 == b.1 && a.0 != b.0 {
                            assert_eq!(a.1.rem_euclid(1.0), frac, "{name} y {}", a.1);
                        }
                    }
                }
            }
        }
    }

    /// 矢じりは軸と一緒に動き、左右の形がそろったまま（Fetch の矢じりの先は軸の x）。
    #[test]
    fn arrowheads_follow_their_shaft() {
        let fetch = get("fetch").unwrap().strokes(16.0, 1.0, 1.0);
        let shaft = &fetch[1].segments;
        let head = &fetch[2].segments;
        assert_eq!(shaft[1], 8.5);
        // 矢じり: 移動 (5,10.5) → 線 (8,13.5) → 線 (11,10.5) が x に +0.5
        assert_eq!(head[4], 8.5);
        assert_eq!(head[4] - head[1], head[7] - head[4]);
    }

    /// Retina（2 倍）では、1pt の線は 2 画素なので、縦・横の線は画素の境目（0.5pt 刻み）に乗る。
    #[test]
    fn retina_snaps_to_half_points() {
        for s in get("sidebar").unwrap().strokes(16.0, 2.0, 1.0) {
            let pts: Vec<(Op, &[f32])> = ops(&s.segments).collect();
            for w in pts.windows(2) {
                let (a, b) = (w[0].1, w[1].1);
                if w[1].0 != Op::Line || a.len() < 2 {
                    continue;
                }
                let (a, b) = ((a[a.len() - 2], a[a.len() - 1]), (b[0], b[1]));
                if a.0 == b.0 {
                    assert_eq!((a.0 * 2.0).fract(), 0.0, "x {}", a.0);
                }
                if a.1 == b.1 {
                    assert_eq!((a.1 * 2.0).fract(), 0.0, "y {}", a.1);
                }
            }
        }
    }

    /// VSCode 拡張の `media/icons/` が今の形から作ったものと同じ（形を変えたら
    /// `cargo run -p amonhen-icons --example vscode` で作り直す）。
    #[test]
    fn vscode_icons_are_current() {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/vscode/media/icons");
        for (path, svg) in vscode_files() {
            // Windows の取り出しは改行を CRLF にすることがある（core.autocrlf）。
            let committed = std::fs::read_to_string(dir.join(&path))
                .unwrap_or_default()
                .replace("\r\n", "\n");
            assert_eq!(
                committed, svg,
                "{path} is stale: cargo run -p amonhen-icons --example vscode"
            );
        }
    }

    #[test]
    fn shapes_become_paths() {
        let pull = get("pull").unwrap();
        // 円は 2 つの弧のパスになる。
        assert!(
            pull.shapes
                .iter()
                .any(|s| s.d.starts_with("M6.5 13A1.5 1.5"))
        );
        let rebase = get("rebase").unwrap();
        assert!(rebase.shapes.iter().any(|s| !s.dash.is_empty()));
    }
}
