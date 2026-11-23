//! 名前のアイコンの正方形（pele の `src/diagrams/common/icon.ts`）。

use crate::svg::builder::{esc_text, num};
use crate::types::IconResolver;

/// 名前のアイコンを、ラベルと同じ形の正方形に。絵はホストの解き手があれば、それが出す。
pub fn icon_svg(name: &str, x: f64, y: f64, side: f64, icons: Option<&IconResolver>) -> String {
    format!(
        "<svg class=\"pele-icon\" data-icon=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\">{}</svg>",
        esc_text(name),
        num(x),
        num(y),
        num(side),
        num(side),
        icons.and_then(|f| f(name)).unwrap_or_default()
    )
}
