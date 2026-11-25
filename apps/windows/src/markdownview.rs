//! Markdown の整形表示（History の「すべてのファイル」。macOS 版の `MarkdownView`、GTK4 版の `markdownview.rs`）。
//! コアが読んだブロック（`amonhen_core::markdown`）を、WinUI の RichTextBlock・Border・Grid に組み立てる（WebView も
//! JavaScript も使わない）。外へは何も読みに行かない（画像は代わりの文を薄く）。リンクは押すとブラウザで開く。

use std::cell::Cell;
use std::sync::{Arc, OnceLock};

use amonhen_core::diagram::{IMPORTED_PREFIX, MermaidConfig, Scheme, ThemeColors};
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

/// 図の配色の設定（Settings > Appearance の Mermaid の「配色」。鍵は `Scheme::key`、読み込んだテーマは
/// `mermaid-import:名前`）と、読み込んだ Mermaid のテーマ（名前 → 中身）。鍵は GTK4 版と同じ。
pub const DIAGRAM_COLORS: &str = "diagramColors";
pub const IMPORTED_MERMAID_THEMES: &str = "importedMermaidThemes";

/// 読み込んだ Mermaid のテーマ（名前の順）。
pub fn imported_mermaid() -> Vec<(String, MermaidConfig)> {
    let mut all: Vec<(String, MermaidConfig)> = crate::prefs::string(IMPORTED_MERMAID_THEMES)
        .and_then(|s| {
            serde_json::from_str::<std::collections::HashMap<String, MermaidConfig>>(&s).ok()
        })
        .unwrap_or_default()
        .into_iter()
        .collect();
    all.sort_by(|a, b| a.0.cmp(&b.0));
    all
}

pub fn set_imported_mermaid(themes: &[(String, MermaidConfig)]) {
    let map: std::collections::HashMap<&str, &MermaidConfig> =
        themes.iter().map(|(k, v)| (k.as_str(), v)).collect();
    if let Err(e) = crate::prefs::set_string(
        IMPORTED_MERMAID_THEMES,
        &serde_json::to_string(&map).unwrap_or_default(),
    ) {
        eprintln!("読み込んだ Mermaid のテーマを覚えられない: {e}");
    }
}

/// 「配色」に並べるもの（設定の鍵と、出す名前。GTK4 版の `diagram_scheme_label`）。読み込んだテーマは名前で。
pub fn diagram_schemes() -> Vec<(String, String)> {
    let mut entries: Vec<(String, String)> = Scheme::all()
        .into_iter()
        .map(|scheme| {
            let label = match scheme {
                Scheme::Syntax => tr!("Syntax colors"),
                Scheme::Pele => tr!("Obsidian style"),
                Scheme::Mermaid(None) => tr!("Mermaid (follows the appearance)"),
                Scheme::Mermaid(Some(theme)) => format!("Mermaid {}", theme.name()),
                Scheme::Imported => "Mermaid".into(),
            };
            (scheme.key(), label)
        })
        .collect();
    entries.extend(
        imported_mermaid()
            .into_iter()
            .map(|(name, _)| (format!("{IMPORTED_PREFIX}{name}"), name)),
    );
    entries
}

/// 図に当てる色の元: 今のテーマの構文の色と、本文の文字と地の色、外観、「配色」（読み込んだテーマならその中身）。
/// 図を絵にする前に UI のスレッドで取る（GTK4 版の `diagram_palette`）。図の中で指定した Mermaid のテーマが先。
pub struct DiagramColors {
    theme: ThemeColors,
    dark: bool,
    scheme: Scheme,
    imported: Option<MermaidConfig>,
}

pub fn diagram_theme() -> DiagramColors {
    let key = crate::prefs::string(DIAGRAM_COLORS).unwrap_or_default();
    let imported = key.strip_prefix(IMPORTED_PREFIX).and_then(|name| {
        imported_mermaid()
            .into_iter()
            .find(|(n, _)| n == name)
            .map(|(_, c)| c)
    });
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
    DiagramColors {
        theme: ThemeColors {
            text: text.to_owned(),
            background: background.to_owned(),
            kinds,
        },
        dark,
        scheme: Scheme::from_key(&key),
        imported,
    }
}

impl DiagramColors {
    /// 図の SVG に色を当てる（図の中で指定したテーマ `own` が先）。
    fn colorize(&self, svg: &str, own: Option<&MermaidConfig>) -> Option<String> {
        let (vars, backdrop) = amonhen_core::diagram::colors_for(
            own,
            self.scheme,
            self.imported.as_ref(),
            self.dark,
            &self.theme,
        );
        amonhen_core::diagram::colorize(svg, &vars, backdrop)
    }
}

/// 見本を描いたときの配色・外観・テーマの世代と、その絵。
type Sample = ((String, bool, u32), Option<Arc<DiagramPicture>>);

thread_local! {
    /// Settings の図の見本（配色・外観・テーマが同じなら描き直さない）。
    static SAMPLE: std::cell::RefCell<Option<Sample>> = const { std::cell::RefCell::new(None) };
}

/// Settings の図の見本（「配色」と今の外観・テーマで色を当てる。GTK4 版の `diagram_sample`）。UI のスレッドで。
pub fn diagram_sample() -> View {
    let colors = diagram_theme();
    let key = (
        crate::prefs::string(DIAGRAM_COLORS).unwrap_or_default(),
        colors.dark,
        crate::theme::generation(),
    );
    let picture = SAMPLE.with(|cell| {
        let mut cell = cell.borrow_mut();
        if let Some((k, p)) = cell.as_ref()
            && *k == key
        {
            return p.clone();
        }
        let picture = amonhen_core::diagram::sample()
            .and_then(|svg| colors.colorize(&svg, None))
            .and_then(|svg| render_diagram(&svg))
            .map(Arc::new);
        *cell = Some((key, picture.clone()));
        picture
    });
    match picture {
        Some(picture) => Image::new()
            .source_data(picture.png.clone())
            .stretch(Stretch::Uniform)
            .width(picture.width)
            .height(picture.height)
            .horizontal_alignment(HorizontalAlignment::Left)
            .into(),
        None => View::empty(),
    }
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
pub fn render_diagrams(list: &[Block], colors: &DiagramColors) -> Vec<Option<DiagramPicture>> {
    let mut out = Vec::new();
    collect(list, colors, &mut out);
    out
}

fn collect(list: &[Block], colors: &DiagramColors, out: &mut Vec<Option<DiagramPicture>>) {
    for b in list {
        match b {
            Block::Diagram { svg, config, .. } => out.push(
                svg.as_ref()
                    .and_then(|svg| colors.colorize(svg, config.as_ref()))
                    .and_then(|svg| render_diagram(&svg)),
            ),
            Block::Quote { blocks: inner, .. }
            | Block::Footnote { blocks: inner, .. }
            | Block::Details { blocks: inner, .. } => collect(inner, colors, out),
            Block::List { items, .. } => {
                for item in items {
                    collect(&item.blocks, colors, out);
                }
            }
            _ => {}
        }
    }
}

/// 組み立てる間の、次の図の絵（`render_diagrams` と同じ順に取る）と、読めた画像、外観。
struct Pictures<'a> {
    list: &'a [Option<DiagramPicture>],
    next: Cell<usize>,
    images: &'a MdImages,
    dark: bool,
    /// 次の `<details>` の番号（文書の中で出てくる順）。
    details: Cell<usize>,
}

thread_local! {
    /// 開け閉めした `<details>`（文書の鍵と、番号 → 開いているか）。WinUI の Expander は自分で開け閉めするが、
    /// 組み直すたびに最初の状態を渡すと戻ってしまうので、押した後の状態を覚えて渡す。別の文書を開いたら忘れる。
    static DETAILS: std::cell::RefCell<(u64, std::collections::HashMap<usize, bool>)> =
        std::cell::RefCell::default();
}

/// Markdown の中の、このコミットから読めた画像（SVG は絵にしたもの）と、もとの大きさ（DIP）。
pub struct MdImage {
    data: EncodedImage,
    width: f64,
    height: f64,
}

/// 画像の場所（Markdown に書いたまま）→ 読めた画像。
pub type MdImages = std::collections::HashMap<String, MdImage>;

/// 画像の幅の上限（GTK4 版と同じ 640）。
const MAX_IMAGE_WIDTH: f64 = 640.0;

/// `from`（Markdown のファイルのパス）から見た `link` の、リポジトリの根からのパス（外の URL・`#` だけは None。
/// GTK4 版の `resolve`）。
pub fn resolve(link: &str, from: &str) -> Option<String> {
    let target = link.split('#').next().unwrap_or(link);
    if target.is_empty() || target.contains("://") || target.starts_with("data:") {
        return None;
    }
    let mut parts: Vec<&str> = if target.starts_with('/') {
        Vec::new()
    } else {
        from.rsplit_once('/')
            .map(|(dir, _)| dir.split('/').filter(|p| !p.is_empty()).collect())
            .unwrap_or_default()
    };
    for part in target.split('/').filter(|p| !p.is_empty()) {
        match part {
            ".." => {
                parts.pop();
            }
            "." => {}
            _ => parts.push(part),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// `blocks` の中の画像の場所（重ならずに、出てくる順。GTK4 版の `image_sources`）。
fn image_sources(blocks: &[Block]) -> Vec<String> {
    fn runs(runs: &[Run], out: &mut Vec<String>) {
        for run in runs {
            for src in [&run.image, &run.image_dark, &run.image_light]
                .into_iter()
                .flatten()
            {
                if !out.contains(src) {
                    out.push(src.clone());
                }
            }
        }
    }
    fn walk(blocks: &[Block], out: &mut Vec<String>) {
        for block in blocks {
            match block {
                Block::Heading { runs: r, .. } | Block::Paragraph { runs: r, .. } => runs(r, out),
                Block::Quote { blocks, .. } | Block::Footnote { blocks, .. } => walk(blocks, out),
                Block::Details {
                    summary, blocks, ..
                } => {
                    runs(summary, out);
                    walk(blocks, out);
                }
                Block::List { items, .. } => {
                    for item in items {
                        walk(&item.blocks, out);
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(blocks, &mut out);
    out
}

/// PNG・GIF・JPEG・BMP の、頭から読める大きさ（画素）。
fn raster_size(data: &[u8]) -> Option<(u32, u32)> {
    let be32 = |at: usize| Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?));
    let le16 = |at: usize| {
        Some(u32::from(u16::from_le_bytes(
            data.get(at..at + 2)?.try_into().ok()?,
        )))
    };
    let le32 =
        |at: usize| Some(i32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?).unsigned_abs());
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some((be32(16)?, be32(20)?));
    }
    if data.starts_with(b"GIF8") {
        return Some((le16(6)?, le16(8)?));
    }
    if data.starts_with(b"BM") {
        return Some((le32(18)?, le32(22)?));
    }
    if data.starts_with(&[0xff, 0xd8]) {
        // JPEG: SOF（0xC0〜0xCF の、DHT・JPG・DAC を除く）の高さと幅。
        let mut at = 2;
        while at + 9 < data.len() {
            if data[at] != 0xff {
                return None;
            }
            let marker = data[at + 1];
            let length = usize::from(u16::from_be_bytes([data[at + 2], data[at + 3]]));
            if (0xc0..=0xcf).contains(&marker) && !matches!(marker, 0xc4 | 0xc8 | 0xcc) {
                let h = u32::from(u16::from_be_bytes([data[at + 5], data[at + 6]]));
                let w = u32::from(u16::from_be_bytes([data[at + 7], data[at + 8]]));
                return Some((w, h));
            }
            at += 2 + length;
        }
    }
    None
}

/// 画像を読んでおく（このコミットの中のものだけ。外へは読みに行かない）。SVG は 2 倍の画素の絵にする。裏の
/// スレッドで呼ぶ。
pub fn load_images(
    blocks: &[Block],
    from: &str,
    read: &dyn Fn(&str) -> Option<Arc<Vec<u8>>>,
) -> MdImages {
    let mut out = MdImages::new();
    for src in image_sources(blocks) {
        let Some(data) = resolve(&src, from).and_then(|path| read(&path)) else {
            continue;
        };
        let image = if src
            .to_ascii_lowercase()
            .split(['?', '#'])
            .next()
            .is_some_and(|p| p.ends_with(".svg"))
            || data.windows(4).take(4096).any(|w| w == b"<svg")
        {
            let Some(picture) = render_svg(&data, 2.0) else {
                continue;
            };
            picture
        } else {
            let Some((w, h)) = raster_size(&data) else {
                continue;
            };
            MdImage {
                data: EncodedImage::new(data.to_vec()),
                width: f64::from(w),
                height: f64::from(h),
            }
        };
        out.insert(src, image);
    }
    out
}

/// SVG を `scale` 倍の画素で絵にする（大きさは SVG の大きさ）。
fn render_svg(data: &[u8], scale: f32) -> Option<MdImage> {
    let options = usvg::Options {
        fontdb: fonts(),
        ..Default::default()
    };
    let tree = usvg::Tree::from_data(data, &options).ok()?;
    let size = tree.size();
    // 大きすぎる絵は、幅の上限の 2 倍の画素までに収める。
    let scale = scale
        .min(2.0 * MAX_IMAGE_WIDTH as f32 / size.width().max(1.0))
        .max(0.01);
    let mut pixmap = tiny_skia::Pixmap::new(
        (size.width() * scale).ceil().max(1.0) as u32,
        (size.height() * scale).ceil().max(1.0) as u32,
    )?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Some(MdImage {
        data: EncodedImage::new(pixmap.encode_png().ok()?),
        width: f64::from(size.width()),
        height: f64::from(size.height()),
    })
}

impl Pictures<'_> {
    /// 外観に合う画像の場所（`<picture>` の暗い・明るい外観のもの、なければ `image`）。
    fn source<'r>(&self, run: &'r Run) -> Option<&'r String> {
        let themed = if self.dark {
            run.image_dark.as_ref()
        } else {
            run.image_light.as_ref()
        };
        themed.or(run.image.as_ref())
    }

    /// 読めた画像の、HTML の `width`・`height`（片方なら比を保つ）か、もとの大きさ（幅は 640 まで）で見せる絵。
    /// リンクの中の画像は押せる。
    fn image(&self, run: &Run) -> Option<View> {
        let image = self.images.get(self.source(run)?)?;
        let ratio = image.height / image.width.max(1.0);
        let (w, h) = match (run.width, run.height) {
            (Some(w), Some(h)) => (f64::from(w), f64::from(h)),
            (Some(w), None) => (f64::from(w), f64::from(w) * ratio),
            (None, Some(h)) => (f64::from(h) / ratio.max(0.0001), f64::from(h)),
            (None, None) => (image.width, image.height),
        };
        let (w, h) = if w > MAX_IMAGE_WIDTH {
            (MAX_IMAGE_WIDTH, h * MAX_IMAGE_WIDTH / w)
        } else {
            (w, h)
        };
        let picture: View = Image::new()
            .source_data(image.data.clone())
            .stretch(Stretch::Uniform)
            .width(w)
            .height(h)
            .into();
        let picture = if run.text.is_empty() {
            picture
        } else {
            Border::new().content(picture).tooltip(run.text.clone())
        };
        Some(
            match run
                .link
                .as_ref()
                .filter(|u| u.starts_with("http://") || u.starts_with("https://"))
                .and_then(|u| HyperlinkButton::new().navigate_uri(u.clone()).ok())
            {
                Some(button) => button.content(picture),
                None => picture,
            },
        )
    }
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
            } else {
                (r.text.clone(), None)
            };
            RichTextInline::Run(RichTextRun {
                text,
                is_bold: bold || r.bold,
                is_italic: r.italic || r.image.is_some(),
                foreground,
                is_monospace: r.code || r.kbd,
                // `<kbd>` はキーの札（枠と地のある小さな札。macOS 版・GTK4 版と同じ）。
                is_key_cap: r.kbd,
                is_strikethrough: r.strike,
                // `<sup>`・`<sub>` は上付き・下付き（GitHub と同じく小さく、上・下にずらす）。
                variant: if r.sup {
                    RunVariant::Superscript
                } else if r.sub {
                    RunVariant::Subscript
                } else {
                    RunVariant::Normal
                },
            })
        })
        .collect()
}

/// 段落・見出し: 読めた画像は絵で、ほかの文字はそのまま。画像だけの段落（バッジの並びなど）は横に並べる。HTML の
/// `align` でそろえる（短い行は真ん中・右に寄る。GTK4 版の `justify`）。
fn paragraph(runs: &[Run], size: f64, bold: bool, align: Align, pictures: &Pictures) -> View {
    let view = if runs.iter().any(|r| pictures.image(r).is_some()) {
        let mut parts: Vec<View> = Vec::new();
        let mut words: Vec<Run> = Vec::new();
        let mut only_images = true;
        let flush = |words: &mut Vec<Run>, parts: &mut Vec<View>, only_images: &mut bool| {
            if words
                .iter()
                .any(|w| !w.text.trim().is_empty() || w.line_break)
            {
                *only_images = false;
                parts.push(text(words, size, bold));
            }
            words.clear();
        };
        for run in runs {
            match pictures.image(run) {
                Some(picture) => {
                    flush(&mut words, &mut parts, &mut only_images);
                    parts.push(picture);
                }
                None => words.push(run.clone()),
            }
        }
        flush(&mut words, &mut parts, &mut only_images);
        StackPanel::new()
            .orientation(if only_images {
                Orientation::Horizontal
            } else {
                Orientation::Vertical
            })
            .spacing(4.0)
            .children((View::keyed_fragment(
                parts
                    .into_iter()
                    .enumerate()
                    .map(|(i, v)| (i.to_string(), v)),
            ),))
    } else {
        text(runs, size, bold)
    };
    match align {
        Align::Center => Border::new()
            .horizontal_alignment(HorizontalAlignment::Center)
            .content(view),
        Align::Right => Border::new()
            .horizontal_alignment(HorizontalAlignment::Right)
            .content(view),
        _ => view,
    }
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
        Block::Heading { level, runs, align } => {
            let size = HEADINGS[(*level as usize).clamp(1, 6) - 1];
            let heading = paragraph(runs, size, true, *align, pictures);
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
        Block::Paragraph { runs, align } => paragraph(runs, BODY, false, *align, pictures),
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
        // 数式はまだ描かない（コアの math フィーチャーを入れていない）。TeX をコードとして見せ、理由があれば添える。
        // 文中の式は原文がコードの文字で来る。
        Block::Math {
            text: code, error, ..
        } => StackPanel::new().spacing(4.0).children((
            Border::new()
                .background(shade())
                .corner_radius(4.0)
                .padding(Thickness::new(10.0, 8.0, 10.0, 8.0))
                .content(plain(code, BODY - 1.0, true, None)),
            match error {
                Some(error) => plain(error, BODY - 2.0, true, Some(Color::rgb(0xd9, 0x33, 0x33))),
                None => View::empty(),
            },
        )),
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
        // HTML の `<details>` は、見出しを押すと開く・閉じる（`open` なら開いて始める。GitHub・macOS 版・GTK4 版と
        // 同じ）。
        Block::Details {
            summary,
            open,
            blocks: inner,
        } => {
            let head = if summary.is_empty() {
                plain(&details_title(), BODY, false, None)
            } else {
                text(summary, BODY, true)
            };
            let index = pictures.details.get();
            pictures.details.set(index + 1);
            let expanded = DETAILS
                .with(|d| d.borrow().1.get(&index).copied())
                .unwrap_or(*open);
            Expander::new()
                .is_expanded(expanded)
                .on_is_expanded_changed(move |on: bool| {
                    DETAILS.with(|d| d.borrow_mut().1.insert(index, on));
                })
                .horizontal_alignment(HorizontalAlignment::Stretch)
                .horizontal_content_alignment(HorizontalAlignment::Left)
                .slots([
                    SlotView::new(ExpanderSlot::Header, head),
                    SlotView::new(ExpanderSlot::Content, blocks(inner, pictures)),
                ])
        }
        Block::HtmlTable {
            columns,
            rows,
            cells,
        } => html_table(*columns, *rows, cells),
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

/// 見出しのない `<details>` の見出し（GitHub と同じ「Details」。訳の鍵は「HTML details: 」を付けたもの）。
fn details_title() -> String {
    let title = tr!("HTML details: Details");
    title
        .strip_prefix("HTML details: ")
        .map_or(title.clone(), str::to_owned)
}

/// HTML の表（マスは `colspan`・`rowspan` をまたぐ。見出しのマスは太字と背景）。
fn html_table(columns: u32, rows: u32, cells: &[amonhen_core::markdown::TableCell]) -> View {
    let views = cells.iter().map(|c| -> View {
        let align = match c.align {
            Align::Center => HorizontalAlignment::Center,
            Align::Right => HorizontalAlignment::Right,
            _ => HorizontalAlignment::Left,
        };
        Border::new()
            .grid_row(c.row as i32)
            .grid_column(c.column as i32)
            .grid_row_span(c.rowspan as i32)
            .grid_column_span(c.colspan as i32)
            .border_thickness(Thickness::uniform(0.5))
            .border_brush(shade())
            .background(if c.header {
                shade()
            } else {
                Brush::Solid(Color::transparent())
            })
            .padding(Thickness::new(8.0, 4.0, 8.0, 4.0))
            .content(
                Border::new()
                    .horizontal_alignment(align)
                    .content(text(&c.runs, BODY, c.header)),
            )
    });
    Grid::new()
        .horizontal_alignment(HorizontalAlignment::Left)
        .columns(vec![GridLength::Auto; columns as usize])
        .rows(vec![GridLength::Auto; rows as usize])
        .children((View::keyed_fragment(
            views.enumerate().map(|(i, c)| (i.to_string(), c)),
        ),))
}

/// Markdown の整形表示（スクロールする欄）。読めなければ「読めない」。
pub fn view(
    key: u64,
    parsed: Option<&[Block]>,
    diagrams: &[Option<DiagramPicture>],
    images: &MdImages,
) -> View {
    DETAILS.with(|d| {
        let mut d = d.borrow_mut();
        if d.0 != key {
            *d = (key, Default::default());
        }
    });
    let pictures = Pictures {
        details: Cell::new(0),
        list: diagrams,
        next: Cell::new(0),
        images,
        dark: crate::palette::dark(),
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
