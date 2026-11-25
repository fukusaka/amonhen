//! どの FE でも同じ形で描くアイコン。
//!
//! 形は `svg/` の SVG（16×16 の格子、太さ 1.5 の線、端と角は丸）で、ビルドのときに線のパスへ直して持つ。各 FE は
//! パスを自分の描画で線として引く（塗りではない）。色は FE が決める（ふつうは文字の色）。決めごとと経緯は
//! `docs/icons.md`。

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

/// 名前（`svg/` のファイル名）でアイコンを引く。
pub fn get(name: &str) -> Option<&'static Icon> {
    ICONS.iter().find(|(n, _)| *n == name).map(|(_, icon)| icon)
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
