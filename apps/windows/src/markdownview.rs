//! Markdown の整形表示（History の「すべてのファイル」。macOS 版の `MarkdownView`、GTK4 版の `markdownview.rs`）。
//! コアが読んだブロック（`amonhen_core::markdown`）を、WinUI の RichTextBlock・Border・Grid に組み立てる（WebView も
//! JavaScript も使わない）。外へは何も読みに行かない（画像は代わりの文を薄く）。リンクは押すとブラウザで開く。

use std::cell::Cell;
use std::sync::{Arc, OnceLock};

use amonhen_core::diagram::{Scheme, ThemeColors};
use amonhen_core::markdown::{Align, Block, Run};
use amonhen_model::tr;
use resvg::{tiny_skia, usvg};
use windows_reactor::*;

/// 本文の大きさと、見出しの大きさ（macOS 版と同じ数）。
const BODY: f64 = 14.0;
const HEADINGS: [f64; 6] = [26.0, 21.0, 17.0, 15.0, 14.0, 13.0];
/// リストの 1 段の深さ。
const STEP: f64 = 22.0;
/// 本文の欄の幅（`view` の `max_width` から左右の余白を除いたもの）。図はこれより広く見せない。
const TEXT_WIDTH: f32 = 852.0;

/// Mermaid・DOT の図を絵にしたもの（PNG）と、見せる大きさ（DIP）。
pub struct DiagramPicture {
    png: EncodedImage,
    width: f64,
    height: f64,
}

/// 図に当てる色の元（今のテーマの構文の色と、本文の文字と地の色）。図を絵にする前に UI のスレッドで取る
/// （GTK4 版の `diagram_palette`）。図の中で指定した Mermaid のテーマが先。
pub fn diagram_theme() -> (ThemeColors, bool) {
    let dark = crate::palette::dark();
    let hex = |(r, g, b): (f32, f32, f32)| {
        format!(
            "#{:02x}{:02x}{:02x}",
            (r * 255.0).round() as u8,
            (g * 255.0).round() as u8,
            (b * 255.0).round() as u8
        )
    };
    let (text, background) = if dark {
        ("#ffffff", "#1e1e1e")
    } else {
        ("#000000", "#ffffff")
    };
    let kinds = crate::theme::KINDS
        .iter()
        .filter_map(|(name, kind)| {
            Some(((*name).to_owned(), hex(crate::render::syntax_rgb(*kind)?)))
        })
        .collect();
    (
        ThemeColors {
            text: text.to_owned(),
            background: background.to_owned(),
            kinds,
        },
        dark,
    )
}

/// Windows の字体（最初に図を描くときに 1 度だけ読む。千近い字体で 0.2 秒ほど）。
fn fonts() -> Arc<usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut db = usvg::fontdb::Database::new();
            db.load_system_fonts();
            Arc::new(db)
        })
        .clone()
}

/// 色を当てた図の SVG を、見せる大きさの 2 倍の画素で絵にする（高い解像度の画面でもぼやけないよう。GTK4 版と
/// 同じ）。欄より広い図は欄の幅に縮めて見せる。裏のスレッドで呼ぶ。
fn render_diagram(svg: &str) -> Option<DiagramPicture> {
    let options = usvg::Options {
        fontdb: fonts(),
        ..Default::default()
    };
    let tree = usvg::Tree::from_data(svg.as_bytes(), &options).ok()?;
    let size = tree.size();
    let shown = (size.width().min(TEXT_WIDTH), 0.0);
    let shown = (shown.0, size.height() * shown.0 / size.width().max(1.0));
    let scale = 2.0 * shown.0 / size.width().max(1.0);
    let mut pixmap = tiny_skia::Pixmap::new(
        (size.width() * scale).ceil().max(1.0) as u32,
        (size.height() * scale).ceil().max(1.0) as u32,
    )?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let png = pixmap.encode_png().ok()?;
    Some(DiagramPicture {
        png: EncodedImage::new(png),
        width: f64::from(shown.0),
        height: f64::from(shown.1),
    })
}

/// 文書の図を、出てくる順に絵にする（`blocks` と同じたどり方。描けない図は None）。裏のスレッドで呼ぶ。
pub fn render_diagrams(
    list: &[Block],
    theme: &ThemeColors,
    dark: bool,
) -> Vec<Option<DiagramPicture>> {
    let mut out = Vec::new();
    collect(list, theme, dark, &mut out);
    out
}

fn collect(list: &[Block], theme: &ThemeColors, dark: bool, out: &mut Vec<Option<DiagramPicture>>) {
    for b in list {
        match b {
            Block::Diagram { svg, config, .. } => out.push(svg.as_ref().and_then(|svg| {
                let (vars, backdrop) = amonhen_core::diagram::colors_for(
                    config.as_ref(),
                    Scheme::default(),
                    None,
                    dark,
                    theme,
                );
                render_diagram(&amonhen_core::diagram::colorize(svg, &vars, backdrop)?)
            })),
            Block::Quote { blocks: inner, .. } | Block::Footnote { blocks: inner, .. } => {
                collect(inner, theme, dark, out)
            }
            Block::List { items, .. } => {
                for item in items {
                    collect(&item.blocks, theme, dark, out);
                }
            }
            _ => {}
        }
    }
}

/// 組み立てる間の、次の図の絵（`render_diagrams` と同じ順に取る）。
struct Pictures<'a> {
    list: &'a [Option<DiagramPicture>],
    next: Cell<usize>,
}

impl Pictures<'_> {
    fn take(&self) -> Option<&DiagramPicture> {
        let i = self.next.get();
        self.next.set(i + 1);
        self.list.get(i).and_then(Option::as_ref)
    }
}

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
fn blocks(list: &[Block], pictures: &Pictures) -> View {
    StackPanel::new()
        .spacing(10.0)
        .children((View::keyed_fragment(
            list.iter()
                .enumerate()
                .map(|(i, b)| (i.to_string(), block(b, pictures))),
        ),))
}

fn block(b: &Block, pictures: &Pictures) -> View {
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
        // 図は絵にして、近いもので描いたならそのことを下に添える。描けなければコードとして見せ、理由を添える
        // （macOS 版・GTK4 版と同じ）。
        Block::Diagram {
            text: code,
            note,
            error,
            ..
        } => match pictures.take() {
            Some(picture) => StackPanel::new().spacing(4.0).children((
                Image::new()
                    .source_data(picture.png.clone())
                    .stretch(Stretch::Uniform)
                    .width(picture.width)
                    .height(picture.height)
                    .horizontal_alignment(HorizontalAlignment::Left),
                match note {
                    Some(note) => {
                        plain(note, BODY - 2.0, false, Some(Color::rgb(0x80, 0x80, 0x80)))
                    }
                    None => View::empty(),
                },
            )),
            None => StackPanel::new().spacing(4.0).children((
                Border::new()
                    .background(shade())
                    .corner_radius(4.0)
                    .padding(Thickness::new(10.0, 8.0, 10.0, 8.0))
                    .content(plain(code, BODY - 1.0, true, None)),
                match error {
                    Some(error) => {
                        plain(error, BODY - 2.0, true, Some(Color::rgb(0xd9, 0x33, 0x33)))
                    }
                    None => View::empty(),
                },
            )),
        },
        Block::Code { text: code, .. } => Border::new()
            .background(shade())
            .corner_radius(4.0)
            .padding(Thickness::new(10.0, 8.0, 10.0, 8.0))
            .content(plain(code, BODY - 1.0, true, None)),
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
                        .children((head, blocks(inner, pictures))),
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
                                    Border::new()
                                        .grid_column(1)
                                        .content(blocks(&item.blocks, pictures)),
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
                blocks(inner, pictures),
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
pub fn view(parsed: Option<&[Block]>, diagrams: &[Option<DiagramPicture>]) -> View {
    let pictures = Pictures {
        list: diagrams,
        next: Cell::new(0),
    };
    let content = match parsed {
        Some(list) => blocks(list, &pictures),
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
