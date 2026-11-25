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

impl Shape {
    /// [`Shape::segments`] を、一辺 `size` の正方形に描く大きさのパス（`M`・`L`・`C`・`Z` だけ、絶対座標、空白で
    /// 区切る）にする。SVG と XAML のパスの書き方のどちらでも読める（WinUI の `Path` に渡す）。
    pub fn markup(&self, size: f32) -> String {
        let scale = (size / GRID) as f64;
        let s = self.segments;
        let mut out = String::new();
        let mut i = 0;
        while i < s.len() {
            let Some(op) = Op::from_code(s[i]) else { break };
            let letter = match op {
                Op::Move => 'M',
                Op::Line => 'L',
                Op::Cubic => 'C',
                Op::Close => 'Z',
            };
            if !out.is_empty() {
                out.push(' ');
            }
            out.push(letter);
            for &v in &s[i + 1..i + 1 + op.arity()] {
                write!(out, " {}", r(v as f64 * scale)).unwrap();
            }
            i += 1 + op.arity();
        }
        out
    }
}

impl Icon {
    /// 線を引いた後の見た目を、塗る形にしたもの（SVG のパスの書き方、移動・直線・弧と閉じるだけ）。一辺 `size` の
    /// 正方形に描く大きさで、nonzero で塗る。線を引けず塗るだけの描画（WinUI の `PathIcon`、アイコンのフォント）で
    /// 使う。曲線は短い直線で近づける。
    pub fn outline(&self, size: f32) -> String {
        let mut out = String::new();
        for shape in self.shapes {
            outline_into(&mut out, shape.segments, shape.dash, (size / GRID) as f64);
        }
        out
    }
}

/// 名前（`svg/` のファイル名）でアイコンを引く。
pub fn get(name: &str) -> Option<&'static Icon> {
    ICONS.iter().find(|(n, _)| *n == name).map(|(_, icon)| icon)
}

/// 線（太さ `STROKE_WIDTH`、端と角は丸）で囲まれる所を、塗るパスとして `out` に足す。曲線は短い直線に分け、直線ごとに「長方形」と両端の「円」を足す。どれも
/// 同じ向きに回るので、nonzero で塗れば重なりは消えずに合わさる。破線は線に沿って区切り、長さ 0 の所は円（点）にする。
fn outline_into(out: &mut String, segments: &[f32], dash: &[f32], scale: f64) {
    let half = STROKE_WIDTH as f64 / 2.0 * scale;
    // 線を折れ線の列（移動ごとに 1 本）にする。
    let mut lines: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut i = 0;
    let at = |k: usize| (segments[k] as f64 * scale, segments[k + 1] as f64 * scale);
    while i < segments.len() {
        match segments[i] as u8 {
            0 => {
                lines.push(vec![at(i + 1)]);
                i += 3;
            }
            1 => {
                lines.last_mut().unwrap().push(at(i + 1));
                i += 3;
            }
            2 => {
                let line = lines.last_mut().unwrap();
                let p0 = *line.last().unwrap();
                let (p1, p2, p3) = (at(i + 1), at(i + 3), at(i + 5));
                const STEPS: usize = 12;
                for n in 1..=STEPS {
                    let t = n as f64 / STEPS as f64;
                    let u = 1.0 - t;
                    let x = u * u * u * p0.0
                        + 3.0 * u * u * t * p1.0
                        + 3.0 * u * t * t * p2.0
                        + t * t * t * p3.0;
                    let y = u * u * u * p0.1
                        + 3.0 * u * u * t * p1.1
                        + 3.0 * u * t * t * p2.1
                        + t * t * t * p3.1;
                    line.push((x, y));
                }
                i += 7;
            }
            _ => {
                let line = lines.last_mut().unwrap();
                let first = line[0];
                line.push(first);
                i += 1;
            }
        }
    }
    let circle = |out: &mut String, (x, y): (f64, f64)| {
        // 2 つの半円。旗の sweep = 1 で、長方形と同じ向きに回る。
        write!(
            out,
            "M{} {}A{} {} 0 1 1 {} {}A{} {} 0 1 1 {} {}Z",
            r(x - half),
            r(y),
            r(half),
            r(half),
            r(x + half),
            r(y),
            r(half),
            r(half),
            r(x - half),
            r(y)
        )
        .unwrap();
    };
    let bar = |out: &mut String, p: (f64, f64), q: (f64, f64)| {
        let (dx, dy) = (q.0 - p.0, q.1 - p.1);
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-6 {
            return;
        }
        let (nx, ny) = (-dy / len * half, dx / len * half);
        let mut quad = [
            (p.0 + nx, p.1 + ny),
            (q.0 + nx, q.1 + ny),
            (q.0 - nx, q.1 - ny),
            (p.0 - nx, p.1 - ny),
        ];
        // 角度の増える向き（円の弧と同じ向き）にそろえる。
        let area: f64 = (0..4)
            .map(|k| quad[k].0 * quad[(k + 1) % 4].1 - quad[(k + 1) % 4].0 * quad[k].1)
            .sum();
        if area < 0.0 {
            quad.reverse();
        }
        write!(out, "M{} {}", r(quad[0].0), r(quad[0].1)).unwrap();
        for v in &quad[1..] {
            write!(out, "L{} {}", r(v.0), r(v.1)).unwrap();
        }
        out.push('Z');
    };
    for line in lines {
        if dash.is_empty() {
            circle(out, line[0]);
            for w in line.windows(2) {
                bar(out, w[0], w[1]);
                circle(out, w[1]);
            }
            continue;
        }
        // 破線: 線に沿って、引く長さと空ける長さを繰り返す。
        let mut k = 0;
        let mut left = dash[0] as f64 * scale;
        let mut on = true;
        let mut pen = line[0];
        if on && left == 0.0 {
            circle(out, pen);
        }
        for w in line.windows(2) {
            let (mut p, q) = (w[0], w[1]);
            let mut seg = ((q.0 - p.0).powi(2) + (q.1 - p.1).powi(2)).sqrt();
            while seg > 1e-9 {
                let step = left.min(seg);
                let t = step / seg;
                let next = (p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t);
                if on && step > 0.0 {
                    bar(out, p, next);
                    circle(out, p);
                    circle(out, next);
                }
                seg -= step;
                left -= step;
                p = next;
                pen = next;
                while left <= 1e-9 {
                    k = (k + 1) % dash.len();
                    on = !on;
                    left = dash[k] as f64 * scale;
                    if on && left == 0.0 {
                        circle(out, pen);
                    }
                    if dash.iter().all(|&d| d == 0.0) {
                        break;
                    }
                }
            }
        }
    }
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

    /// 塗る形は、どのアイコンにもあり、描く大きさの正方形に収まる。
    #[test]
    fn outlines_stay_in_the_box() {
        for (name, icon) in ICONS {
            for size in [16.0, 20.0] {
                let outline = icon.outline(size);
                assert!(outline.starts_with('M'), "{name}");
                for n in outline.split(|c: char| c.is_ascii_alphabetic() || c == ' ') {
                    if let Ok(v) = n.parse::<f32>() {
                        assert!((-1.0..=size + 1.0).contains(&v), "{name}: {v}");
                    }
                }
            }
        }
        // Rebase の前の位置の点は、半径 0.75 の円として入る（大きさ 16 で）。
        assert!(
            get("rebase")
                .unwrap()
                .outline(16.0)
                .matches("A0.75")
                .count()
                > 10
        );
    }

    /// 描く大きさのパスは、移動で始まり、大きさに合わせて広がる。
    #[test]
    fn markup_scales() {
        let pull = get("pull").unwrap();
        assert_eq!(pull.shapes[0].markup(16.0), "M 3.5 2 L 12.5 2");
        assert_eq!(pull.shapes[0].markup(32.0), "M 7 4 L 25 4");
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
