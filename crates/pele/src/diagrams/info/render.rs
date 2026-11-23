//! info を描く（pele の `src/diagrams/info/render.ts`）。

use super::model::InfoModel;
use crate::preprocess::Config;
use crate::svg::builder::label_svg;
use crate::svg::root::{Accessible, svg_document};
use crate::text::label::layout_label;
use crate::text::measurer::{TextMeasurer, default_measurer, style};
use crate::types::{RenderOptions, Rendered};

/// pele の `package.json` の版。
const VERSION: &str = "1.13.0";

pub fn render_info(model: &InfoModel, _config: &Config, options: &RenderOptions) -> Rendered {
    let size = options.font_size.unwrap_or(16.0);
    let measurer: &dyn TextMeasurer = match &options.measurer {
        Some(m) => m.as_ref(),
        None => default_measurer(),
    };
    let pad = options.padding.unwrap_or(8.0);

    let title = layout_label(
        model.title.as_deref(),
        false,
        measurer,
        size,
        4000.0,
        style::BOLD,
    );
    let title_height = if title.height > 0.0 {
        title.height + 8.0
    } else {
        0.0
    };
    let version = format!("Pele v{VERSION}");
    let text = layout_label(Some(&version), false, measurer, size, 4000.0, 0);

    let width = (title.width.max(text.width) + 2.0 * pad).ceil();
    let height = (title_height + text.height + 2.0 * pad).ceil();
    let content = label_svg(
        &title,
        width / 2.0,
        pad + title.height / 2.0,
        " class=\"pele-title\" font-weight=\"var(--_tw)\"",
        None,
    ) + &label_svg(
        &text,
        width / 2.0,
        pad + title_height + text.height / 2.0,
        " class=\"pele-version\" fill=\"var(--_m)\"",
        None,
    );
    let acc = Accessible {
        acc_title: model.acc_title.as_deref(),
        acc_descr: model.acc_descr.as_deref(),
    };
    Rendered {
        svg: svg_document("info", width, height, size, options, &acc, &content),
        width,
        height,
        links: Vec::new(),
    }
}
