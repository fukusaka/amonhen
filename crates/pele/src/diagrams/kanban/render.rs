//! kanban を描く（pele の `src/diagrams/kanban/render.ts`）。

use super::db::{KanbanDb, KanbanNode};
use crate::diagrams::common::fit_width::title_room;
use crate::diagrams::common::icon::icon_svg;
use crate::js;
use crate::preprocess::Config;
use crate::svg::builder::{esc, esc_text, label_svg, num, text_style};
use crate::svg::root::{Accessible, svg_document};
use crate::svg::theme::{CRITICAL, ROUNDED, class_names};
use crate::svg::title::title_label;
use crate::text::icons::icon_markup;
use crate::text::label::{Label, Span, label_line, layout_label, line_middle, with_heights};
use crate::text::measurer::{TextMeasurer, default_measurer, style};
use crate::types::{IconResolver, LinkInfo, RenderOptions, Rendered};
use crate::util::url::{link_url, rel_attr, safe_url};
use crate::util::yaml::Value;

const COLUMN_GAP: f64 = 12.0;
const COLUMN_PAD: f64 = 8.0;
const CARD_PAD: f64 = 10.0;
const CARD_GAP: f64 = 8.0;
const META_GAP: f64 = 4.0;
const ICON_GAP: f64 = 6.0;

/// Mermaid は 5 つの優先度を名づけ、Medium には色を付けない。大文字小文字は区別しない。高いものは重要の色、
/// 低いものは淡い色で、Medium に近いほど薄く。系列の色ではない（テーマはそれを好きな順に並べうる）。
fn priority_color(priority: &str) -> Option<String> {
    match priority {
        "very high" => Some(format!("stroke=\"{CRITICAL}\"")),
        "high" => Some(format!("stroke=\"{CRITICAL}\" opacity=\"0.5\"")),
        "low" => Some("stroke=\"var(--_m)\" opacity=\"0.5\"".to_string()),
        "very low" => Some("stroke=\"var(--_m)\"".to_string()),
        _ => None,
    }
}

/// 決まった幅にラベルを組む。行に収まらない単語は文字の間で折る（カードは横に広がれない）。
fn fit_label(
    raw: Option<&str>,
    markdown: bool,
    measurer: &dyn TextMeasurer,
    size: f64,
    max: f64,
    base: u32,
) -> Label {
    let label = layout_label(raw, markdown, measurer, size, max, base);
    if label.width <= max {
        return label;
    }
    let mut lines: Vec<Vec<Span>> = Vec::new();
    let mut widths: Vec<f64> = Vec::new();
    let mut widest: f64 = 0.0;
    let mut line: Vec<Span> = Vec::new();
    let mut width = 0.0;
    let commit = |line: &mut Vec<Span>,
                  width: &mut f64,
                  lines: &mut Vec<Vec<Span>>,
                  widths: &mut Vec<f64>,
                  widest: &mut f64| {
        lines.push(std::mem::take(line));
        widths.push(*width);
        *widest = js::max(*widest, *width);
        *width = 0.0;
    };
    let text_span = |text: String, style: u32, width: f64| Span {
        text,
        style,
        icon: None,
        math: None,
        width,
    };
    for i in 0..label.lines.len() {
        if label.widths[i] <= max {
            line = label.lines[i].clone();
            width = label.widths[i];
            commit(&mut line, &mut width, &mut lines, &mut widths, &mut widest);
            continue;
        }
        for span in &label.lines[i] {
            if span.icon.is_some() || span.math.is_some() {
                if width > 0.0 && width + span.width > max {
                    commit(&mut line, &mut width, &mut lines, &mut widths, &mut widest);
                }
                line.push(span.clone());
                width += span.width;
                continue;
            }
            let mut text = String::new();
            let mut from = width;
            for ch in span.text.chars() {
                let w = measurer.width(ch.encode_utf8(&mut [0; 4]), size, span.style);
                if width > 0.0 && width + w > max {
                    if !text.is_empty() {
                        line.push(text_span(
                            std::mem::take(&mut text),
                            span.style,
                            width - from,
                        ));
                    }
                    commit(&mut line, &mut width, &mut lines, &mut widths, &mut widest);
                    from = 0.0;
                }
                text.push(ch);
                width += w;
            }
            if !text.is_empty() {
                line.push(text_span(text, span.style, width - from));
            }
        }
        commit(&mut line, &mut width, &mut lines, &mut widths, &mut widest);
    }
    let height = lines.len() as f64 * label.line_height;
    let has_heights = label.heights.is_some();
    let fitted = Label {
        lines,
        widths,
        width: widest,
        height,
        ..label
    };
    if has_heights {
        with_heights(fitted)
    } else {
        fitted
    }
}

/// ラベルの各行を x から（`end` なら x まで）書き、最初の行の箱を y から始める。
fn text_block(
    label: &Label,
    x: f64,
    y: f64,
    attrs: &str,
    icons: Option<&IconResolver>,
    end: bool,
) -> String {
    let mut out = String::new();
    for i in 0..label.lines.len() {
        let line = &label.lines[i];
        let cy = y + line_middle(label, i);
        if line.len() == 1
            && line[0].icon.is_none()
            && line[0].math.is_none()
            && line[0].style & (style::SUB | style::SUP | style::UNDERLINE) == 0
        {
            out.push_str(&format!(
                "<text{} x=\"{}\" y=\"{}\"{}>{}</text>",
                text_style(attrs, line[0].style),
                num(x),
                num(cy + label.size * 0.35),
                if end { " text-anchor=\"end\"" } else { "" },
                esc(&line[0].text)
            ));
        } else {
            let width = label.widths[i];
            out.push_str(&label_svg(
                &label_line(label, i),
                x + if end { -width } else { width } / 2.0,
                cy,
                attrs,
                icons,
            ));
        }
    }
    out
}

struct Card<'a> {
    item: &'a KanbanNode,
    label: Label,
    ticket: Label,
    assigned: Label,
    /// 文の下の、チケットと担当の行の高さ。
    meta: f64,
    height: f64,
}

fn has(name: Option<&str>) -> bool {
    name.is_some_and(|n| !n.is_empty() && !icon_markup(n).is_empty())
}

pub fn render_kanban(model: &KanbanDb, config: &Config, options: &RenderOptions) -> Rendered {
    let size = options.font_size.unwrap_or(16.0);
    let small = js::round(size * 0.875);
    let line = js::round(size * 1.5);
    let small_line = js::round(small * 1.5);
    let measurer: &dyn TextMeasurer = match &options.measurer {
        Some(m) => m.as_ref(),
        None => default_measurer(),
    };
    let pad = options.padding.unwrap_or(8.0);
    let icons = options.icons.as_ref();
    let conf = config.get("kanban").and_then(Value::as_obj);
    let section_width = conf.and_then(|c| c.get("sectionWidth"));
    let glyph = small * 1.25;
    // 数式は行をまたいで折れないので、列はいちばん広い式に合わせて広がる。
    let mut math_width: f64 = 0.0;
    let mut holds = |raw: Option<&str>, font_size: f64, reserved: f64| {
        let Some(raw) = raw.filter(|r| r.contains("$$")) else {
            return;
        };
        for spans in &layout_label(Some(raw), true, measurer, font_size, f64::INFINITY, 0).lines {
            for span in spans {
                if let Some(m) = &span.math {
                    math_width = js::max(math_width, m.width() + reserved);
                }
            }
        }
    };
    let sections = &model.sections;
    for section in sections {
        let count = measurer.width(&section.items.len().to_string(), small, 0);
        holds(
            Some(&section.node.label),
            size,
            2.0 * COLUMN_PAD
                + 8.0
                + count
                + 12.0
                + if has(section.node.icon.as_deref()) {
                    glyph + ICON_GAP
                } else {
                    0.0
                },
        );
        for item in &section.items {
            let reserved = 2.0 * (COLUMN_PAD + CARD_PAD);
            holds(
                Some(&item.label),
                small,
                reserved
                    + if has(item.icon.as_deref()) {
                        glyph + ICON_GAP
                    } else {
                        0.0
                    },
            );
            holds(item.ticket.as_deref(), small, reserved);
            holds(item.assigned.as_deref(), small, reserved);
        }
    }
    let own_width = js::max(
        match section_width {
            Some(Value::Num(w)) if *w > 0.0 => js::min(js::max(*w, 80.0), 2000.0),
            _ => 200.0,
        },
        math_width.ceil(),
    );
    // ホストの幅より広い板は、列の行に折りたたみ、列が行を分け合う。
    let room = match options.max_width {
        Some(m) if m > 0.0 => m - 2.0 * pad,
        _ => f64::INFINITY,
    };
    let count = sections.len() as f64;
    let fits = count * (own_width + COLUMN_GAP) - COLUMN_GAP <= room;
    let per_row = if fits {
        js::max(count, 1.0)
    } else {
        js::max(
            1.0,
            ((room + COLUMN_GAP) / (own_width + COLUMN_GAP)).floor(),
        )
    };
    let column_width = if fits {
        own_width
    } else {
        js::max_of(&[
            js::min(own_width, room),
            js::min(
                ((room + COLUMN_GAP) / per_row).floor() - COLUMN_GAP,
                own_width * 2.0,
            ),
            math_width.ceil(),
        ])
    };
    let base_url = match conf.and_then(|c| c.get("ticketBaseUrl")) {
        Some(Value::Str(s)) => s.as_str(),
        _ => "",
    };
    let card_width = column_width - 2.0 * COLUMN_PAD;
    let text_width = card_width - 2.0 * CARD_PAD;
    let mut links: Vec<LinkInfo> = Vec::new();

    let mut titles: Vec<Label> = Vec::new();
    let mut count_widths: Vec<f64> = Vec::new();
    let mut cards: Vec<Vec<Card>> = Vec::new();
    let mut head_height = line;
    let mut body_heights: Vec<f64> = Vec::new();
    for section in sections {
        let count_width = measurer.width(&section.items.len().to_string(), small, 0);
        let reserved = count_width
            + 12.0
            + if has(section.node.icon.as_deref()) {
                glyph + ICON_GAP
            } else {
                0.0
            };
        let title = fit_label(
            Some(&section.node.label),
            true,
            measurer,
            size,
            card_width - 8.0 - reserved,
            style::BOLD,
        );
        head_height = js::max(head_height, title.height);
        count_widths.push(count_width);
        titles.push(title);

        let mut list: Vec<Card> = Vec::new();
        let mut height = 0.0;
        for item in &section.items {
            let label = fit_label(
                Some(&item.label),
                true,
                measurer,
                small,
                text_width
                    - if has(item.icon.as_deref()) {
                        glyph + ICON_GAP
                    } else {
                        0.0
                    },
                0,
            );
            let ticket = fit_label(
                item.ticket.as_deref(),
                false,
                measurer,
                small,
                text_width,
                0,
            );
            let assigned = fit_label(
                item.assigned.as_deref(),
                false,
                measurer,
                small,
                text_width,
                0,
            );
            let meta = if ticket.width + assigned.width + 8.0 > text_width {
                ticket.height + assigned.height
            } else {
                js::max(ticket.height, assigned.height)
            };
            let card_height = 2.0 * CARD_PAD
                + js::max(label.height, small_line)
                + if meta != 0.0 { META_GAP + meta } else { 0.0 };
            list.push(Card {
                item,
                label,
                ticket,
                assigned,
                meta,
                height: card_height,
            });
            height += card_height + CARD_GAP;
        }
        cards.push(list);
        body_heights.push(height);
    }
    head_height += 2.0 * COLUMN_PAD;
    let per_row_n = per_row as usize;
    let mut row_heights: Vec<f64> = Vec::new();
    let mut row_tops: Vec<f64> = Vec::new();
    let mut first = 0;
    let mut y = 0.0;
    while first < sections.len() {
        let mut tallest = CARD_GAP;
        for height in &body_heights[first..(first + per_row_n).min(sections.len())] {
            tallest = js::max(tallest, *height);
        }
        row_tops.push(y);
        row_heights.push(head_height + tallest);
        y += head_height + tallest + COLUMN_GAP;
        first += per_row_n;
    }
    let board_height = match (row_tops.last(), row_heights.last()) {
        (Some(t), Some(h)) => t + h,
        _ => 0.0,
    };

    let title = title_label(model.title.as_deref(), measurer, size, title_room(options));
    let title_height = if title.height > 0.0 {
        title.height + 16.0
    } else {
        0.0
    };
    let board_width = js::max(
        js::min(count, per_row) * (column_width + COLUMN_GAP) - COLUMN_GAP,
        0.0,
    );
    let width = js::max(board_width, title.width);
    let left = pad + (width - board_width) / 2.0;
    let edge = column_width - COLUMN_PAD - 4.0;

    let mut out = String::new();
    for (index, section) in sections.iter().enumerate() {
        let row = index / per_row_n;
        let mut body = format!(
            "<rect width=\"{}\" height=\"{}\" fill=\"var(--_s)\"{ROUNDED}/>{}<text class=\"pele-count\" x=\"{}\" y=\"{}\" text-anchor=\"end\" font-size=\"{}\" fill=\"var(--_m)\">{}</text>",
            js::fmt(column_width),
            num(row_heights[row]),
            text_block(
                &titles[index],
                COLUMN_PAD + 4.0,
                COLUMN_PAD,
                " class=\"pele-cluster-label\" font-weight=\"var(--_hw)\"",
                icons,
                false
            ),
            js::fmt(edge),
            num(COLUMN_PAD + line / 2.0 + small * 0.35),
            js::fmt(small),
            section.items.len()
        );
        if has(section.node.icon.as_deref()) {
            body.push_str(&icon_svg(
                section.node.icon.as_deref().unwrap_or_default(),
                edge - count_widths[index] - ICON_GAP - glyph,
                COLUMN_PAD + (line - glyph) / 2.0,
                glyph,
                icons,
            ));
        }

        let mut y = head_height;
        for card in &cards[index] {
            let item = card.item;
            let priority = js::trim(item.priority.as_deref().unwrap_or_default()).to_lowercase();
            let color = priority_color(&priority);
            let mut inner = format!(
                "<rect width=\"{}\" height=\"{}\" fill=\"var(--_bg)\" stroke=\"var(--_b)\"{ROUNDED}/>",
                js::fmt(card_width),
                num(card.height)
            );
            if let Some(color) = &color {
                inner.push_str(&format!(
                    "<path class=\"pele-priority\" d=\"M2.5,5V{}\" fill=\"none\" {color} stroke-width=\"3\" stroke-linecap=\"round\"/>",
                    num(card.height - 5.0)
                ));
            }
            inner.push_str(&text_block(
                &card.label,
                CARD_PAD,
                CARD_PAD,
                " class=\"pele-label\"",
                icons,
                false,
            ));
            if has(item.icon.as_deref()) {
                inner.push_str(&icon_svg(
                    item.icon.as_deref().unwrap_or_default(),
                    card_width - CARD_PAD - glyph,
                    CARD_PAD + (small_line - glyph) / 2.0,
                    glyph,
                    icons,
                ));
            }
            let bottom = card.height - CARD_PAD;
            if let Some(ticket) = item.ticket.as_deref().filter(|t| !t.is_empty()) {
                let top = bottom - card.meta;
                let href = if base_url.is_empty() {
                    None
                } else {
                    link_url(
                        &safe_url(&base_url.replacen("#TICKET#", ticket, 1)),
                        options,
                    )
                };
                match href {
                    None => inner.push_str(&text_block(
                        &card.ticket,
                        CARD_PAD,
                        top,
                        " class=\"pele-ticket\" fill=\"var(--_m)\"",
                        icons,
                        false,
                    )),
                    Some(href) => {
                        inner.push_str(&format!(
                            "<a href=\"{}\" target=\"_blank\"{}>{}</a>",
                            esc(&href),
                            rel_attr(options),
                            text_block(
                                &card.ticket,
                                CARD_PAD,
                                top,
                                " class=\"pele-ticket\" fill=\"var(--_c)\"",
                                icons,
                                false
                            )
                        ));
                        links.push(LinkInfo {
                            id: item.id.clone(),
                            href,
                            internal: false,
                        });
                    }
                }
            }
            if item.assigned.as_deref().is_some_and(|a| !a.is_empty()) {
                inner.push_str(&text_block(
                    &card.assigned,
                    card_width - CARD_PAD,
                    bottom - card.assigned.height,
                    " class=\"pele-assigned\" fill=\"var(--_m)\"",
                    icons,
                    true,
                ));
            }
            body.push_str(&format!(
                "<g class=\"pele-node pele-card{}{}\" data-id=\"{}\" transform=\"translate({},{})\" font-size=\"{}\">{inner}</g>",
                if color.is_none() {
                    String::new()
                } else {
                    format!(" pele-priority-{}", priority.replacen(' ', "-", 1))
                },
                class_names(item.css_classes.as_deref().unwrap_or_default()),
                esc_text(&item.id),
                js::fmt(COLUMN_PAD),
                num(y),
                js::fmt(small)
            ));
            y += card.height + CARD_GAP;
        }

        out.push_str(&format!(
            "<g class=\"pele-cluster pele-column{}\" data-id=\"{}\" transform=\"translate({},{})\">{body}</g>",
            class_names(section.node.css_classes.as_deref().unwrap_or_default()),
            esc_text(&section.node.id),
            num(left + (index % per_row_n) as f64 * (column_width + COLUMN_GAP)),
            num(pad + title_height + row_tops[row])
        ));
    }

    let total_width = (width + 2.0 * pad).ceil();
    let total_height = (board_height + title_height + 2.0 * pad).ceil();
    let content = label_svg(
        &title,
        total_width / 2.0,
        pad + title.height / 2.0,
        " class=\"pele-title\" font-weight=\"var(--_tw)\"",
        None,
    ) + &out;

    Rendered {
        svg: svg_document(
            "kanban",
            total_width,
            total_height,
            size,
            options,
            &Accessible::default(),
            &content,
        ),
        width: total_width,
        height: total_height,
        links,
    }
}
