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
