//! packet を描く（pele の `src/diagrams/packet/render.ts`）。

use std::collections::HashMap;

use super::model::{PacketBlock, PacketModel, packet_option};
use crate::diagrams::common::fit_text::{Fitted, fit_text};
use crate::diagrams::common::fit_width::{title_room, turn_to_fit};
use crate::diagrams::common::literal_label::literal_label;
use crate::errors::Result;
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc, label_svg, num};
use crate::svg::root::{Accessible, svg_document};
use crate::svg::theme::ROUNDED;
use crate::text::entities::decode_entities;
use crate::text::label::{Label, layout_label};
use crate::text::measurer::{TextMeasurer, default_measurer, style};
use crate::types::{RenderOptions, Rendered};
use crate::util::yaml::Value;

pub fn render_packet(
    model: &PacketModel,
    config: &Config,
    options: &RenderOptions,
) -> Result<Rendered> {
    turn_to_fit(options, |down| Ok(draw(model, config, options, down)))
}

fn draw(model: &PacketModel, config: &Config, options: &RenderOptions, down: bool) -> Rendered {
    let size = options.font_size.unwrap_or(16.0);
    let label_size = js::round(size * 0.875);
    let bit_size = js::round(size * 0.625);
    let measurer: &dyn TextMeasurer = match &options.measurer {
        Some(m) => m.as_ref(),
        None => default_measurer(),
    };
    let pad = options.padding.unwrap_or(8.0);

    let packet = config.get("packet").and_then(Value::as_obj);
    let row_height = packet_option(config, "rowHeight", 32.0, 1.0, 400.0);
    let mut bit_width = packet_option(config, "bitWidth", 32.0, 1.0, 400.0);
    let padding_x = packet_option(config, "paddingX", 5.0, 0.0, 400.0);
    let padding_y = packet_option(config, "paddingY", 5.0, 0.0, 400.0);
    let show_bits = packet.and_then(|p| p.get("showBits")) != Some(&Value::Bool(false));
    let descending = packet
        .and_then(|p| p.get("bitOrder"))
        .and_then(Value::as_str)
        == Some("descending");
    let mut bits_per_row = model.bits_per_row;
    // 行ごとの欄と、縦に描くときのそのラベル。
    let mut rows: Vec<Vec<(PacketBlock, Option<Label>)>> = Vec::new();
    if down {
        let max_width = options.max_width.unwrap_or(320.0);
        let count = js::max(
            1.0,
            ((max_width - 2.0 * pad) / js::min(bit_width, 18.0)).floor(),
        );
        // 1 バイトがいちばん短い行。それより短いと、どのラベルも 1 行に 1 文字になり、長い packet は 1 ビットに
        // 1 行になる。
        bits_per_row = js::min(bits_per_row, js::max(8.0, 2f64.powf(count.log2().floor())));
        let last = model
            .rows
            .last()
            .and_then(|r| r.last())
            .map_or(0.0, |b| b.end);
        if ((last + 1.0) / bits_per_row).ceil() > 10000.0 {
            bits_per_row = model.bits_per_row;
        }
        bit_width = js::min(
            bit_width,
            js::max(18.0, (max_width - 2.0 * pad) / bits_per_row),
        );
        // 形はそれぞれの欄を自分の行で分けてある。新しい行の終わりでだけ分かれるよう、先に切れをつなぎ直す。
        // 切れは 1 ビット少なく数える。
        let piece = |block: &PacketBlock| block.bits == block.end - block.start;
        let mut whole: Vec<PacketBlock> = Vec::new();
        for row in &model.rows {
            for block in row {
                match whole.last_mut() {
                    Some(prev)
                        if piece(prev)
                            && piece(block)
                            && prev.label == block.label
                            && block.start == prev.end + 1.0
                            && block.start % model.bits_per_row == 0.0 =>
                    {
                        prev.end = block.end;
                        prev.bits = prev.end - prev.start;
                    }
                    _ => whole.push(block.clone()),
                }
            }
        }
        for block in &whole {
            let mut start = block.start;
            while start <= block.end {
                let end = js::min(
                    block.end,
                    ((start / bits_per_row).floor() + 1.0) * bits_per_row - 1.0,
                );
                let index = (start / bits_per_row).floor() as usize;
                while rows.len() <= index {
                    rows.push(Vec::new());
                }
                let mut piece = block.clone();
                piece.start = start;
                piece.end = end;
                rows[index].push((piece, None));
                start = end + 1.0;
            }
        }
        for row in &mut rows {
            for (block, label) in row.iter_mut() {
                let room = js::max(
                    1.0,
                    (block.end - block.start + 1.0) * bit_width
                        - js::min(padding_x, bit_width - 1.0)
                        - 6.0,
                );
                *label = Some(literal_label(
                    js::trim(&decode_entities(&block.label)),
                    measurer,
                    label_size,
                    room,
                    0,
                ));
            }
        }
    } else {
        rows = model
            .rows
            .iter()
            .map(|row| row.iter().map(|b| (b.clone(), None)).collect())
            .collect();
    }
    // ビットの番号は各行の上にあり、その場所が要る。
    let numbers = if show_bits { bit_size + 4.0 } else { 0.0 };
    let row_heights: Vec<f64> = rows
        .iter()
        .map(|row| {
            let mut height = row_height;
            for (_, label) in row {
                if let Some(label) = label {
                    height = js::max(height, label.height + 10.0);
                }
            }
            height
        })
        .collect();
    let mut tops: Vec<f64> = Vec::new();
    let mut rows_height = 0.0;
    for height in &row_heights {
        tops.push(rows_height);
        rows_height += numbers + height + padding_y;
    }
    if !rows.is_empty() {
        rows_height -= padding_y;
    }

    let gap = js::min(padding_x, bit_width - 1.0);
    let width = if rows.is_empty() {
        0.0
    } else {
        bits_per_row * bit_width - gap
    };
    let title = layout_label(
        model.title.as_deref(),
        false,
        measurer,
        size,
        if down { title_room(options) } else { 4000.0 },
        style::BOLD,
    );
    let title_height = if title.height > 0.0 {
        title.height + if rows_height > 0.0 { 12.0 } else { 0.0 }
    } else {
        0.0
    };

    // 行をまたいで分けた欄の切れはラベルが同じなので、幅ごとに 1 度だけ合わせる。
    let mut fits: HashMap<(String, u64), Option<Fitted>> = HashMap::new();
    let mut fit = |label: &str, max: f64| -> Option<Fitted> {
        fits.entry((label.to_string(), max.to_bits()))
            .or_insert_with(|| {
                fit_text(
                    js::trim(&decode_entities(label)),
                    max,
                    label_size,
                    js::round(size * 0.5),
                    measurer,
                    0,
                )
            })
            .clone()
    };

    let left = pad + js::max(0.0, (title.width - width) / 2.0);
    let mut fields = String::new();
    let mut bits = String::new();
    for (index, row) in rows.iter().enumerate() {
        let y = pad + tops[index] + numbers;
        let row_height = row_heights[index];
        for (block, label) in row {
            let count = block.end - block.start + 1.0;
            let first = block.start % bits_per_row;
            let x = left
                + if descending {
                    bits_per_row - first - count
                } else {
                    first
                } * bit_width;
            let w = count * bit_width - gap;
            fields.push_str(&format!(
                "<g class=\"pele-node\" data-id=\"{}-{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"var(--_s)\" stroke=\"var(--_b)\"{ROUNDED}/>",
                js::fmt(block.start),
                js::fmt(block.end),
                num(x),
                num(y),
                num(w),
                num(row_height)
            ));
            if let Some(label) = label {
                fields.push_str(&label_svg(
                    label,
                    x + w / 2.0,
                    y + row_height / 2.0,
                    "",
                    None,
                ));
            } else if let Some(text) = fit(&block.label, w - 6.0)
                && text.size <= row_height
            {
                let font = if text.size == label_size {
                    String::new()
                } else {
                    format!(" font-size=\"{}\"", num(text.size))
                };
                fields.push_str(&format!(
                    "<text x=\"{}\" y=\"{}\"{font}>{}</text>",
                    num(x + w / 2.0),
                    num(y + row_height / 2.0 + text.size * 0.35),
                    esc(&text.text)
                ));
            }
            fields.push_str("</g>");
            if !show_bits {
                continue;
            }
            let leading = if descending { block.end } else { block.start };
            let trailing = if descending { block.start } else { block.end };
            if count == 1.0 {
                bits.push_str(&format!(
                    "<text x=\"{}\" y=\"{}\" text-anchor=\"middle\">{}</text>",
                    num(x + w / 2.0),
                    num(y - 4.0),
                    js::fmt(leading)
                ));
            } else {
                bits.push_str(&format!(
                    "<text x=\"{}\" y=\"{}\">{}</text><text x=\"{}\" y=\"{}\" text-anchor=\"end\">{}</text>",
                    num(x),
                    num(y - 4.0),
                    js::fmt(leading),
                    num(x + w),
                    num(y - 4.0),
                    js::fmt(trailing)
                ));
            }
        }
    }

    let total_width = (js::max(width, title.width) + 2.0 * pad).ceil();
    let total_height = (rows_height + title_height + 2.0 * pad).ceil();
    let mut content = String::new();
    if !fields.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-packet-fields\" font-size=\"{}\" text-anchor=\"middle\">{fields}</g>",
            js::fmt(label_size)
        ));
    }
    if !bits.is_empty() {
        content.push_str(&format!(
            "<g class=\"pele-packet-bits\" font-size=\"{}\" fill=\"var(--_m)\">{bits}</g>",
            js::fmt(bit_size)
        ));
    }
    // Mermaid は packet の題を下に置く。
    content.push_str(&label_svg(
        &title,
        total_width / 2.0,
        total_height - pad - title.height / 2.0,
        " class=\"pele-title\" font-weight=\"var(--_tw)\"",
        None,
    ));

    let acc = Accessible {
        acc_title: model.acc_title.as_deref(),
        acc_descr: model.acc_descr.as_deref(),
    };
    Rendered {
        svg: svg_document(
            "packet",
            total_width,
            total_height,
            size,
            options,
            &acc,
            &content,
        ),
        width: total_width,
        height: total_height,
        links: Vec::new(),
    }
}
