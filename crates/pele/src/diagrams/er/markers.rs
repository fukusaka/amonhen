//! 多重度の印（pele の `src/diagrams/er/markers.ts`）。

use crate::svg::builder::num;

/// 印が、触れる箱から線に沿って戻る長さ。
pub const MARKER_LENGTH: f64 = 18.0;

const BAR: f64 = 5.0;
const FOOT: f64 = 6.0;
const FOOT_LENGTH: f64 = 10.0;
const RING: f64 = 3.5;

/// (x, y) で箱に着き、単位の向き (dx, dy) に進む線の端に、カラスの足の多重度の印を描く。種類は Mermaid の
/// 多重度を小文字にしたもの。
pub fn er_marker(type_: &str, x: f64, y: f64, dx: f64, dy: f64) -> String {
    let at = |back: f64, side: f64| -> String {
        format!(
            "{},{}",
            num(x - dx * back - dy * side),
            num(y - dy * back + dx * side)
        )
    };
    let bar = |back: f64| -> String { format!("M{}L{}", at(back, -BAR), at(back, BAR)) };
    let foot = || {
        format!(
            "M{}L{}L{}",
            at(0.0, -FOOT),
            at(FOOT_LENGTH, 0.0),
            at(0.0, FOOT)
        )
    };
    let ring = |back: f64| -> String {
        format!(
            "<circle class=\"pele-marker\" cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"var(--_bg)\"/>",
            num(x - dx * back),
            num(y - dy * back),
            num(RING)
        )
    };
    let path = |d: String| -> String { format!("<path class=\"pele-marker\" d=\"{d}\"/>") };
    match type_ {
        "only_one" => path(bar(6.0) + &bar(10.0)),
        "zero_or_one" => path(bar(6.0)) + &ring(13.5),
        "one_or_more" => path(foot() + &bar(FOOT_LENGTH + 3.5)),
        "zero_or_more" => path(foot()) + &ring(FOOT_LENGTH + RING + 0.5),
        "md_parent" => format!(
            "<path class=\"pele-marker\" d=\"M{}L{}L{}L{}Z\" fill=\"var(--_bg)\"/>",
            at(0.0, 0.0),
            at(6.0, 4.0),
            at(12.0, 0.0),
            at(6.0, -4.0)
        ),
        _ => String::new(),
    }
}
