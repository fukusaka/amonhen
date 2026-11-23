//! Markdown の整形表示（History の「すべてのファイル」。macOS 版の `MarkdownView`、GTK4 版の `markdownview.rs`）。
//! コアが読んだブロック（`amonhen_core::markdown`）を、WinUI の RichTextBlock・Border・Grid に組み立てる（WebView も
//! JavaScript も使わない）。外へは何も読みに行かない（画像は代わりの文を薄く）。リンクは押すとブラウザで開く。

use amonhen_core::markdown::{Align, Block, Run};
use amonhen_model::tr;
use windows_reactor::*;

/// 本文の大きさと、見出しの大きさ（macOS 版と同じ数）。
const BODY: f64 = 14.0;
const HEADINGS: [f64; 6] = [26.0, 21.0, 17.0, 15.0, 14.0, 13.0];
/// リストの 1 段の深さ。
const STEP: f64 = 22.0;

fn link() -> Color {
    Color::rgb(0x36, 0x85, 0xe3)
}

fn shade() -> Brush {
    Brush::Solid(Color::argb(0x1f, 0x80, 0x80, 0x80))
}

/// GitHub の囲みの色と見出し（訳の鍵は「GitHub alert: 」を付けたもの。英語では落とす）。
fn alert(kind: Option<&str>) -> Option<(Color, String)> {
    let (color, title) = match kind? {
        "note" => (Color::rgb(0x36, 0x85, 0xe3), tr!("GitHub alert: Note")),
        "tip" => (Color::rgb(0x2e, 0xc2, 0x7e), tr!("GitHub alert: Tip")),
        "important" => (Color::rgb(0x91, 0x40, 0xab), tr!("GitHub alert: Important")),
        "warning" => (Color::rgb(0xe6, 0x61, 0x00), tr!("GitHub alert: Warning")),
        "caution" => (Color::rgb(0xe0, 0x1b, 0x24), tr!("GitHub alert: Caution")),
        _ => return None,
    };
    let title = title
        .strip_prefix("GitHub alert: ")
        .map_or(title.clone(), str::to_owned);
    Some((color, title))
}

/// 飾りつきの文字を、RichText の inline にする（太字・斜体・取り消し線・コード・リンク・改行。画像は代わりの文）。
fn inlines(runs: &[Run], bold: bool) -> Vec<RichTextInline> {
    runs.iter()
        .map(|r| {
            if r.line_break {
                return RichTextInline::LineBreak;
            }
            if let Some(url) = r
                .link
                .as_ref()
                .filter(|u| u.starts_with("http://") || u.starts_with("https://"))
            {
                return RichTextInline::Hyperlink(RichTextHyperlink {
                    text: r.text.clone(),
                    uri: url.clone(),
                });
            }
            // GitHub 独自の画像だけの絵文字（:octocat: など）は、名前を薄く（macOS 版・GTK4 版の薄い札）。
            let (text, foreground) = if r.custom_emoji.is_some() {
                (r.text.clone(), Some(Color::rgb(0x80, 0x80, 0x80)))
            } else if r.image.is_some() {
                // 外へ読みに行かない。代わりの文を薄く。
                (format!("[{}]", r.text), Some(Color::rgb(0x80, 0x80, 0x80)))
            } else if r.footnote.is_some() {
                (format!("[{}]", r.text), Some(link()))
            } else if r.link.is_some() {
                (r.text.clone(), Some(link()))
            } else if r.html {
                (r.text.clone(), Some(Color::rgb(0x80, 0x80, 0x80)))
            } else {
                (r.text.clone(), None)
            };
            RichTextInline::Run(RichTextRun {
                text,
                is_bold: bold || r.bold,
                is_italic: r.italic || r.image.is_some(),
                foreground,
                is_monospace: r.code || r.html,
                is_strikethrough: r.strike,
            })
        })
        .collect()
}

fn text(runs: &[Run], size: f64, bold: bool) -> View {
    RichTextBlock::new()
        .paragraphs(RichText::single_paragraph(inlines(runs, bold)))
        .font_size(size)
        .is_text_selection_enabled(true)
        .text_wrapping(TextWrapping::Wrap)
        .into()
}

fn plain(value: &str, size: f64, monospace: bool, color: Option<Color>) -> View {
    RichTextBlock::new()
        .paragraphs(RichText::single_paragraph([RichTextInline::Run(
            RichTextRun {
                text: value.trim_end_matches('\n').to_owned(),
                is_monospace: monospace,
                foreground: color,
                ..RichTextRun::default()
            },
        )]))
        .font_size(size)
        .is_text_selection_enabled(true)
        .text_wrapping(TextWrapping::Wrap)
        .into()
}

/// ブロックの並び。
fn blocks(list: &[Block]) -> View {
    StackPanel::new()
        .spacing(10.0)
        .children((View::keyed_fragment(
            list.iter()
                .enumerate()
                .map(|(i, b)| (i.to_string(), block(b))),
        ),))
}

fn block(b: &Block) -> View {
    match b {
        Block::Heading { level, runs } => {
            let size = HEADINGS[(*level as usize).clamp(1, 6) - 1];
            let heading = text(runs, size, true);
            // 大きな見出しは、下に細い線（GitHub と同じ）。
            if *level <= 2 {
                Border::new()
                    .border_thickness(Thickness::new(0.0, 0.0, 0.0, 1.0))
                    .border_brush(shade())
                    .padding(Thickness::new(0.0, 0.0, 0.0, 4.0))
                    .content(heading)
            } else {
                heading
            }
        }
        Block::Paragraph { runs } => text(runs, BODY, false),
        Block::Code { text: code, .. } => Border::new()
            .background(shade())
            .corner_radius(4.0)
            .padding(Thickness::new(10.0, 8.0, 10.0, 8.0))
            .content(plain(code, BODY - 1.0, true, None)),
        // 図（Mermaid など）は、まだ描かずに元の文をコードとして見せる（描けなかったときと同じ。理由があれば添える）。
        Block::Diagram {
            text: code, error, ..
        } => {
            let note: View = match error {
                Some(error) => plain(
                    error,
                    BODY - 1.0,
                    false,
                    Some(Color::argb(0xff, 0xc0, 0x60, 0x30)),
                ),
                None => View::empty(),
            };
            StackPanel::new().spacing(4.0).children((
                note,
                Border::new()
                    .background(shade())
                    .corner_radius(4.0)
                    .padding(Thickness::new(10.0, 8.0, 10.0, 8.0))
                    .content(plain(code, BODY - 1.0, true, None)),
            ))
        }
        Block::Quote {
            alert: kind,
            blocks: inner,
        } => {
            let (color, title) = match alert(kind.as_deref()) {
                Some((color, title)) => (color, Some(title)),
                None => (Color::argb(0x60, 0x80, 0x80, 0x80), None),
            };
            let head: View = match title {
                Some(title) => plain(&title, BODY, false, Some(color)),
                None => View::empty(),
            };
            Border::new()
                .border_thickness(Thickness::new(3.0, 0.0, 0.0, 0.0))
                .border_brush(Brush::Solid(color))
                .padding(Thickness::new(12.0, 2.0, 0.0, 2.0))
                .opacity(if kind.is_some() { 1.0 } else { 0.8 })
                .content(
                    StackPanel::new()
                        .spacing(6.0)
                        .children((head, blocks(inner))),
                )
        }
        Block::List { start, items } => {
            StackPanel::new()
                .spacing(4.0)
                .children((View::keyed_fragment(
                    items
                        .iter()
                        .enumerate()
                        .map(|(i, item)| {
                            let marker = match (item.task, start) {
                                (Some(true), _) => "☑".to_owned(),
                                (Some(false), _) => "☐".to_owned(),
                                (None, Some(n)) => format!("{}.", n + i as u64),
                                (None, None) => "•".to_owned(),
                            };
                            Grid::new()
                                .columns([GridLength::Pixel(STEP), GridLength::Star(1.0)])
                                .children((
                                    TextBlock::new()
                                        .text(marker)
                                        .font_size(BODY)
                                        .opacity(0.7)
                                        .grid_column(0),
                                    Border::new().grid_column(1).content(blocks(&item.blocks)),
                                ))
                        })
                        .enumerate()
                        .map(|(i, v)| (i.to_string(), v)),
                ),))
        }
        Block::Table { aligns, head, rows } => table(aligns, head, rows),
        Block::Rule => Border::new().height(1.0).background(shade()).into(),
        Block::Html { text: html } => {
            plain(html, BODY - 1.0, true, Some(Color::rgb(0x80, 0x80, 0x80)))
        }
        Block::Footnote {
            label,
            blocks: inner,
        } => StackPanel::new()
            .orientation(Orientation::Horizontal)
            .spacing(6.0)
            .opacity(0.85)
            .children((
                plain(&format!("[{label}]"), BODY - 2.0, false, Some(link())),
                blocks(inner),
            )),
    }
}

/// 表（見出しは太字、そろえ方は列ごと、枠は細い線）。
fn table(aligns: &[Align], head: &[Vec<Run>], rows: &[Vec<Vec<Run>>]) -> View {
    let columns = head.len().max(rows.iter().map(Vec::len).max().unwrap_or(0));
    let cell = |runs: &[Run], row: usize, column: usize, bold: bool| -> View {
        let align = match aligns.get(column) {
            Some(Align::Center) => HorizontalAlignment::Center,
            Some(Align::Right) => HorizontalAlignment::Right,
            _ => HorizontalAlignment::Left,
        };
        Border::new()
            .grid_row(row as i32)
            .grid_column(column as i32)
            .border_thickness(Thickness::uniform(0.5))
            .border_brush(shade())
            .background(if bold {
                shade()
            } else {
                Brush::Solid(Color::transparent())
            })
            .padding(Thickness::new(8.0, 4.0, 8.0, 4.0))
            .content(
                Border::new()
                    .horizontal_alignment(align)
                    .content(text(runs, BODY, bold)),
            )
    };
    let mut cells: Vec<View> = Vec::new();
    for (c, runs) in head.iter().enumerate() {
        cells.push(cell(runs, 0, c, true));
    }
    for (r, row) in rows.iter().enumerate() {
        for (c, runs) in row.iter().enumerate() {
            cells.push(cell(runs, r + 1, c, false));
        }
    }
    Grid::new()
        .horizontal_alignment(HorizontalAlignment::Left)
        .columns(vec![GridLength::Auto; columns])
        .rows(vec![GridLength::Auto; rows.len() + 1])
        .children((View::keyed_fragment(
            cells
                .into_iter()
                .enumerate()
                .map(|(i, c)| (i.to_string(), c)),
        ),))
}

/// Markdown の整形表示（スクロールする欄）。読めなければ「読めない」。
pub fn view(parsed: Option<&[Block]>) -> View {
    let content = match parsed {
        Some(list) => blocks(list),
        None => plain(
            &tr!("Loading…"),
            BODY,
            false,
            Some(Color::rgb(0x80, 0x80, 0x80)),
        ),
    };
    ScrollViewer::new().content(
        Border::new()
            .max_width(900.0)
            .horizontal_alignment(HorizontalAlignment::Left)
            .padding(Thickness::new(24.0, 16.0, 24.0, 24.0))
            .content(content),
    )
}
