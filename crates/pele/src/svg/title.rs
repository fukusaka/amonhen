//! 図の題（pele の `src/svg/title.ts`）。

use crate::svg::builder::{label_svg, num};
use crate::text::label::{Label, layout_label};
use crate::text::math::without_math;
use crate::text::measurer::{TextMeasurer, style};

/// Mermaid は図の題を、何を含んでも文字として描く。
pub fn title_label(
    title: Option<&str>,
    measurer: &dyn TextMeasurer,
    size: f64,
    max_width: f64,
) -> Label {
    without_math(|| layout_label(title, false, measurer, size, max_width, style::BOLD))
}

pub struct Titled {
    pub width: f64,
    pub height: f64,
    pub content: String,
}

/// 題なしで配置した中身の上に、図の題を置く。
pub fn with_title(
    title: Option<&str>,
    measurer: &dyn TextMeasurer,
    size: f64,
    pad: f64,
    width: f64,
    height: f64,
    content: String,
) -> Titled {
    let label = title_label(title, measurer, size, 4000.0);
    if label.height == 0.0 {
        return Titled {
            width,
            height,
            content,
        };
    }
    let shift = label.height + 12.0;
    let full = crate::js::max(width, (label.width + 2.0 * pad).ceil());
    Titled {
        width: full,
        height: (height + shift).ceil(),
        content: label_svg(
            &label,
            full / 2.0,
            pad + label.height / 2.0,
            " class=\"pele-title\" font-weight=\"var(--_tw)\"",
            None,
        ) + &format!(
            "<g transform=\"translate({},{})\">{}</g>",
            num((full - width) / 2.0),
            num(shift),
            content
        ),
    }
}
