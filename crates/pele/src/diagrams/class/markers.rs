//! 関係の端の印（pele の `src/diagrams/class/markers.ts`）。

use super::graph::EndMarker;
use crate::svg::builder::num;
use crate::svg::edges::{marker, marker_trim};

const TRIANGLE: f64 = 12.0;
const DIAMOND: f64 = 16.0;
const HALF: f64 = 6.0;
const RING: f64 = 5.0;

/// 線が印の後ろに届くよう、ノードの手前で止める長さ。
pub fn class_marker_trim(type_: EndMarker) -> f64 {
    match type_ {
        EndMarker::Extension => TRIANGLE,
        EndMarker::Composition | EndMarker::Aggregation => DIAMOND,
        EndMarker::Lollipop => 2.0 * RING,
        EndMarker::Dependency => marker_trim("arrow_point"),
        EndMarker::None => 0.0,
    }
}

/// 先が (x, y) にあり、(dx, dy) に向いた端の印を描く。
pub fn class_marker(type_: EndMarker, x: f64, y: f64, dx: f64, dy: f64, color: &str) -> String {
    let nx = -dy;
    let ny = dx;
    match type_ {
        EndMarker::Extension => {
            let bx = x - dx * TRIANGLE;
            let by = y - dy * TRIANGLE;
            format!(
                "<path class=\"pele-marker\" d=\"M{},{}L{},{}L{},{}Z\" stroke-linejoin=\"round\"/>",
                num(x),
                num(y),
                num(bx + nx * HALF),
                num(by + ny * HALF),
                num(bx - nx * HALF),
                num(by - ny * HALF)
            )
        }
        EndMarker::Composition | EndMarker::Aggregation => {
            let mx = x - (dx * DIAMOND) / 2.0;
            let my = y - (dy * DIAMOND) / 2.0;
            let half = HALF - 1.0;
            format!(
                "<path class=\"pele-marker\" d=\"M{},{}L{},{}L{},{}L{},{}Z\" stroke-linejoin=\"round\"{}/>",
                num(x),
                num(y),
                num(mx + nx * half),
                num(my + ny * half),
                num(x - dx * DIAMOND),
                num(y - dy * DIAMOND),
                num(mx - nx * half),
                num(my - ny * half),
                if type_ == EndMarker::Composition {
                    format!(" fill=\"{color}\"")
                } else {
                    String::new()
                }
            )
        }
        EndMarker::Lollipop => format!(
            "<circle class=\"pele-marker\" cx=\"{}\" cy=\"{}\" r=\"{}\"/>",
            num(x - dx * RING),
            num(y - dy * RING),
            num(RING)
        ),
        EndMarker::Dependency => marker("arrow_point", x, y, dx, dy, color, "none"),
        EndMarker::None => String::new(),
    }
}
