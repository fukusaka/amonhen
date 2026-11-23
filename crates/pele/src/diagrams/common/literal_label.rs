//! 書いたとおりのラベル（pele の `src/diagrams/common/literal-label.ts`）。

use crate::js;
use crate::svg::builder::{esc, label_svg, num, shared_style, span_style, text_style};
use crate::text::label::{Label, Span, with_heights};
use crate::text::measurer::TextMeasurer;
use crate::util::chars::is_space_char;

fn text_span(text: String, style: u32, width: f64) -> Span {
    Span {
        text,
        style,
        icon: None,
        math: None,
        width,
    }
}

/// ファイル名やプロトコルの欄のような、書いたとおりのラベル。行を折る所の空白のほかは 1 文字も落とさない。長い
/// 単語は符号位置の間で折る。測るのは 1 行の文字の長さまで。
pub fn literal_label(
    text: &str,
    measurer: &dyn TextMeasurer,
    size: f64,
    room: f64,
    style: u32,
) -> Label {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let chars: Vec<char> = raw.chars().collect();
        if chars.is_empty() {
            lines.push(String::new());
        }
        let mut from = 0;
        while from < chars.len() {
            let join = |from: usize, length: usize| -> String {
                chars[from..from + length].iter().collect()
            };
            let fits = |length: usize| measurer.width(&join(from, length), size, style) <= room;
            let left = chars.len() - from;
            let mut hi = 1;
            while hi < left && fits(hi) {
                hi = left.min(hi * 2);
            }
            let mut lo = 1;
            while lo < hi {
                let mid = (lo + hi).div_ceil(2);
                if fits(mid) {
                    lo = mid;
                } else {
                    hi = mid - 1;
                }
            }
            let mut take = lo;
            let wraps = from + take < chars.len();
            if wraps && !is_space_char(chars[from + take]) {
                for i in (1..take).rev() {
                    if is_space_char(chars[from + i]) {
                        take = i + 1;
                        break;
                    }
                }
            }
            let line = join(from, take);
            lines.push(if wraps {
                let trimmed = js::trim_end(&line);
                if trimmed.is_empty() {
                    line.clone()
                } else {
                    trimmed.to_string()
                }
            } else {
                line
            });
            from += take;
            while wraps && from < chars.len() && is_space_char(chars[from]) {
                from += 1;
            }
        }
    }
    if text.is_empty() {
        lines.clear();
    }
    let widths: Vec<f64> = lines
        .iter()
        .map(|line| measurer.width(line, size, style))
        .collect();
    let mut width = 0.0;
    for &w in &widths {
        width = js::max(width, w);
    }
    let line_height = js::round(size * 1.5);
    let height = lines.len() as f64 * line_height;
    Label {
        lines: lines
            .into_iter()
            .zip(&widths)
            .map(|(line, &w)| vec![text_span(line, style, w)])
            .collect(),
        widths,
        width,
        height,
        size,
        line_height,
        heights: None,
    }
}

/// `room` より広い行を、文字の間で折る。アイコンと数式は分けない。
pub fn break_label(label: &Label, measurer: &dyn TextMeasurer, room: f64) -> Label {
    if label.width <= room {
        return label.clone();
    }
    let mut lines: Vec<Vec<Span>> = Vec::new();
    let mut widths: Vec<f64> = Vec::new();
    for (i, spans) in label.lines.iter().enumerate() {
        if label.widths[i] <= room {
            lines.push(spans.clone());
            widths.push(label.widths[i]);
            continue;
        }
        let mut current: Vec<Span> = Vec::new();
        let mut width = 0.0;
        let commit = |current: &mut Vec<Span>,
                      width: &mut f64,
                      lines: &mut Vec<Vec<Span>>,
                      widths: &mut Vec<f64>| {
            lines.push(std::mem::take(current));
            widths.push(*width);
            *width = 0.0;
        };
        for span in spans {
            if span.icon.is_some() || span.math.is_some() {
                if !current.is_empty() && width + span.width > room {
                    commit(&mut current, &mut width, &mut lines, &mut widths);
                }
                current.push(span.clone());
                width += span.width;
                continue;
            }
            for ch in span.text.chars() {
                let ch = ch.to_string();
                let joinable = |last: Option<&Span>| {
                    last.is_some_and(|l| {
                        l.icon.is_none() && l.math.is_none() && l.style == span.style
                    })
                };
                let joined = joinable(current.last());
                let next_width = match current.last() {
                    Some(last) if joined => {
                        measurer.width(&(last.text.clone() + &ch), label.size, span.style)
                    }
                    _ => measurer.width(&ch, label.size, span.style),
                };
                let mut added = next_width
                    - if joined {
                        current.last().unwrap().width
                    } else {
                        0.0
                    };
                if !current.is_empty() && width + added > room {
                    commit(&mut current, &mut width, &mut lines, &mut widths);
                    added = measurer.width(&ch, label.size, span.style);
                }
                if joinable(current.last()) {
                    let last = current.last_mut().unwrap();
                    last.text.push_str(&ch);
                    last.width += added;
                } else {
                    current.push(text_span(ch, span.style, added));
                }
                width += added;
            }
        }
        if !current.is_empty() || spans.is_empty() {
            commit(&mut current, &mut width, &mut lines, &mut widths);
        }
    }
    let mut width = 0.0;
    for &w in &widths {
        width = js::max(width, w);
    }
    let height = lines.len() as f64 * label.line_height;
    let broken = Label {
        lines,
        widths,
        width,
        height,
        ..label.clone()
    };
    if label.heights.is_some() {
        with_heights(broken)
    } else {
        broken
    }
}

/// どの行も x から書く。行は頭を留め、空白を残すので、どこから始まるかは文字の測り方によらない。
pub fn left_label_svg(label: &Label, x: f64, top: f64, attrs: &str) -> String {
    let shared = shared_style(label);
    let open = format!("<text{}", text_style(attrs, shared));
    let mut body = String::new();
    for (i, spans) in label.lines.iter().enumerate() {
        let cy = top + (i as f64 + 0.5) * label.line_height;
        if spans.iter().any(|span| span.icon.is_some()) {
            let width = label.widths[i];
            let line = Label {
                lines: vec![spans.clone()],
                widths: vec![width],
                width,
                height: label.line_height,
                ..label.clone()
            };
            body.push_str(&label_svg(&line, x + width / 2.0, cy, attrs, None));
            continue;
        }
        let mut text = String::new();
        for span in spans {
            let own = span_style(span.style & !shared);
            if own.is_empty() {
                text.push_str(&esc(&span.text));
            } else {
                text.push_str(&format!("<tspan{own}>{}</tspan>", esc(&span.text)));
            }
        }
        if !text.is_empty() {
            body.push_str(&format!(
                "{open} x=\"{}\" y=\"{}\" text-anchor=\"start\" xml:space=\"preserve\">{text}</text>",
                num(x),
                num(cy + label.size * 0.35)
            ));
        }
    }
    body
}
