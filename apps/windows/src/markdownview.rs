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
/// 欄の最大の幅と、左右の余白（`view` の Border）。
const MAX_WIDTH: f64 = 900.0;
const SIDES: f64 = 48.0;

/// 整形表示を出す所の幅 `area`（DIP）での本文の欄の幅（幅の割合の画像の基準。macOS 版と同じく窓に合わせて変わる）。
pub fn column_width(area: f64) -> f64 {
    (area.min(MAX_WIDTH) - SIDES).clamp(1.0, f64::from(TEXT_WIDTH))
}

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
    /// 本文の欄の今の幅（幅の割合の画像の基準）。
    column: f64,
    /// 今いるリストの項目の入れ子の深さ（箇条書きの印を深さで変える）。
    lists: Cell<usize>,
}

/// 入れ子の深さ `depth`（いちばん外が 0）の箇条書きの印（macOS 版・GTK4 版と同じく 3 つを順に）。
fn bullet(depth: usize) -> &'static str {
    ["•", "◦", "▪\u{fe0e}"][depth % 3]
}

/// 読めた画像を見せる大きさ: HTML の `width`・`height`（片方なら比を保つ）か、もとの大きさ（幅は 640 まで）。
fn shown_size(image: &MdImage, run: &Run, column: f64) -> (f64, f64) {
    let ratio = image.height / image.width.max(1.0);
    let (w, h) = match (width(run, column), run.height) {
        (Some(w), Some(h)) => (w, f64::from(h)),
        (Some(w), None) => (w, w * ratio),
        (None, Some(h)) => (f64::from(h) / ratio.max(0.0001), f64::from(h)),
        (None, None) => (image.width, image.height),
    };
    if w > MAX_IMAGE_WIDTH {
        (MAX_IMAGE_WIDTH, h * MAX_IMAGE_WIDTH / w)
    } else {
        (w, h)
    }
}

/// 画像の HTML の幅（DIP）。幅の割合（`width="50%"`）は本文の欄の今の幅 `column` に対して決める。
fn width(run: &Run, column: f64) -> Option<f64> {
    run.width
        .map(f64::from)
        .or_else(|| run.width_percent.map(|p| percent_width(p, column)))
}

/// 幅と高さを両方決めた画像は、GitHub（ブラウザ）と同じく比を保たずにその大きさへ伸ばす。
fn fills(run: &Run) -> bool {
    (run.width.is_some() || run.width_percent.is_some()) && run.height.is_some()
}

/// 文の中に置く画像の一時ファイル、見せる大きさ、伸ばすか、代わりの文、基線より下に出す分（文中の数式の深さ）。
type InlineImage = (std::path::PathBuf, f64, f64, bool, String, f64);

thread_local! {
    /// 文の中に置く画像（鍵 → `InlineImage`）。段落を組むときに入れ、部品を作るときに引く。
    static INLINE_IMAGES: std::cell::RefCell<std::collections::HashMap<String, InlineImage>> =
        std::cell::RefCell::default();
}

/// 文の中に置く画像の部品（InlineUIContainer の中の Image。鍵は `Pictures::inline_key` が作ったもの）。
pub fn inline_image(key: &str) -> Option<windows_core_reactor::IInspectable> {
    let (file, w, h, fill, alt, depth) = INLINE_IMAGES.with(|m| m.borrow().get(key).cloned())?;
    let stretch = if fill { "Fill" } else { "Uniform" };
    let uri = url_of(&file);
    let escape = |s: &str| {
        s.replace('&', "&amp;")
            .replace('"', "&quot;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let tip = if alt.is_empty() {
        String::new()
    } else {
        format!(r#" ToolTipService.ToolTip="{}""#, escape(&alt))
    };
    let xaml = format!(
        r#"<InlineUIContainer xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"><Image Source="{}" Width="{w}" Height="{h}" Stretch="{stretch}" Margin="0,0,0,{}"{tip}/></InlineUIContainer>"#,
        escape(&uri),
        -depth
    );
    amonhen_win_xaml::XamlReader::Load(&windows_core_reactor::HSTRING::from(xaml)).ok()
}

/// 数式の大きさを覚える鍵（本文の字に対する倍率。macOS 版・GTK4 版と同じ鍵）。
pub const MATH_SCALE: &str = "mathScale";
/// 1 つの Markdown のファイルで数式を描く時間の上限（秒。0 は上限なし。macOS 版・GTK4 版と同じ鍵）。
pub const MATH_TIME_LIMIT: &str = "mathTimeLimit";

/// 今の数式の大きさ（本文の字に対する倍率。読めなければ既定の `math::FONT_SCALE`）。
pub fn math_scale() -> f64 {
    crate::prefs::string(MATH_SCALE)
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|s| s.is_finite() && *s > 0.0)
        .unwrap_or(amonhen_core::math::FONT_SCALE)
}

/// 今の、1 つのファイルで数式を描く時間の上限（秒。0 は上限なし。既定は 2 秒）。
pub fn math_time_limit() -> u64 {
    crate::prefs::string(MATH_TIME_LIMIT)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(2)
}

/// 設定の時間の上限をコアに渡す（起動したときと、設定を変えたとき）。
pub fn apply_math_time_limit() {
    let seconds = math_time_limit();
    amonhen_core::math::set_file_time_limit(
        (seconds > 0).then(|| std::time::Duration::from_secs(seconds)),
    );
}

/// 数式の 1 em の大きさ（DIP）: 本文の字 × 設定の倍率（macOS 版・GTK4 版と同じ）。
fn math_em(size: f64) -> f64 {
    size * math_scale()
}

/// 数式の絵（コアの SVG に本文の字の色を当て、resvg で 2 倍の細かさの PNG に）と、見せる大きさ（DIP）。色・大きさ
/// ごとに覚えておく（段落を組むたびに描き直さない）。
fn math_png(
    math: &amonhen_core::math::MathPicture,
    size: f64,
) -> Option<(Arc<[u8]>, f64, f64, f64)> {
    let em = math_em(size);
    let (w, h, depth) = (
        math.width * em,
        (math.height + math.depth) * em,
        math.depth * em,
    );
    let c = crate::palette::foreground();
    let fg = format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b);
    let key = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        math.svg.hash(&mut hasher);
        fg.hash(&mut hasher);
        em.to_bits().hash(&mut hasher);
        hasher.finish()
    };
    if let Some(png) = MATH_PNGS.with(|m| m.borrow().get(&key).cloned()) {
        return Some((png, w, h, depth));
    }
    let colored = amonhen_core::math::colorize(&math.svg, &fg)?;
    let options = usvg::Options {
        fontdb: fonts(),
        ..Default::default()
    };
    let tree = usvg::Tree::from_data(colored.as_bytes(), &options).ok()?;
    let scale = (2.0 * w / f64::from(tree.size().width()).max(0.0001)) as f32;
    let mut pixmap = tiny_skia::Pixmap::new(
        (2.0 * w).ceil().max(1.0) as u32,
        (2.0 * h).ceil().max(1.0) as u32,
    )?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let png: Arc<[u8]> = pixmap.encode_png().ok()?.into();
    MATH_PNGS.with(|m| {
        let mut m = m.borrow_mut();
        if m.len() > 256 {
            m.clear();
        }
        m.insert(key, png.clone());
    });
    Some((png, w, h, depth))
}

thread_local! {
    /// 描いた数式の PNG（SVG・色・大きさの鍵 → PNG）。
    static MATH_PNGS: std::cell::RefCell<std::collections::HashMap<u64, Arc<[u8]>>> =
        std::cell::RefCell::default();
}

/// 文中の数式を文の中に置く鍵（`inline_image` が部品にする）。基線より下に式の深さだけ出す（GitHub と同じ）。
fn math_key(run: &Run, size: f64) -> Option<String> {
    let (png, w, h, depth) = math_png(run.math.as_ref()?, size)?;
    let hash = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        png.hash(&mut hasher);
        hasher.finish()
    };
    let dir = std::env::temp_dir().join("amonhen-markdown-images");
    let file = dir.join(format!("{hash:016x}"));
    if !file.exists() {
        std::fs::create_dir_all(&dir).ok()?;
        std::fs::write(&file, &*png).ok()?;
    }
    let key = format!("math:{hash:016x}:{w}:{h}:{depth}");
    INLINE_IMAGES.with(|m| {
        let mut m = m.borrow_mut();
        if m.len() > 512 {
            m.clear();
        }
        m.insert(key.clone(), (file, w, h, false, run.text.clone(), depth));
    });
    Some(key)
}

/// ファイルの `file:///` の URL（`\` を `/` に、空白などは % で）。
fn url_of(path: &std::path::Path) -> String {
    let text = path.display().to_string().replace('\\', "/");
    let mut out = String::from("file:///");
    for c in text.chars() {
        if c.is_ascii_alphanumeric() || "/:._-~".contains(c) {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
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

/// 幅の割合（1〜100）を、`column` の幅に対する DIP にする。
fn percent_width(percent: u32, column: f64) -> f64 {
    (column * f64::from(percent.clamp(1, 100)) / 100.0).round()
}

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

/// `blocks` の中の、画像を置ける飾り（見出し・段落・`<details>` の見出し）を出てくる順に `f` へ渡す。
fn each_run(blocks: &[Block], f: &mut impl FnMut(&Run)) {
    for block in blocks {
        match block {
            Block::Heading { runs, .. } | Block::Paragraph { runs, .. } => {
                runs.iter().for_each(&mut *f)
            }
            Block::Quote { blocks, .. } | Block::Footnote { blocks, .. } => each_run(blocks, f),
            Block::Details {
                summary, blocks, ..
            } => {
                summary.iter().for_each(&mut *f);
                each_run(blocks, f);
            }
            Block::List { items, .. } => {
                for item in items {
                    each_run(&item.blocks, f);
                }
            }
            _ => {}
        }
    }
}

/// `blocks` の中の画像の場所（重ならずに、出てくる順。GTK4 版の `image_sources`）。
fn image_sources(blocks: &[Block]) -> Vec<String> {
    let mut out = Vec::new();
    each_run(blocks, &mut |run| {
        for src in [&run.image, &run.image_dark, &run.image_light]
            .into_iter()
            .flatten()
        {
            if !out.contains(src) {
                out.push(src.clone());
            }
        }
    });
    out
}

/// `blocks` に幅を割合で決めた画像があるか（あれば、欄の幅が変わるたびに描き直す）。
pub fn has_percent_images(blocks: &[Block]) -> bool {
    let mut any = false;
    each_run(blocks, &mut |run| {
        any |= run.width_percent.is_some() && run.width.is_none();
    });
    any
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

    /// 読めた画像の、HTML の `width`・`height`（片方なら比を保ち、両方なら比を保たずに伸ばす）か、もとの大きさ（幅は 640 まで）で見せる絵。
    /// 幅の割合（`width="50%"`）は本文の欄の今の幅に対して決める。リンクの中の画像は押せる。
    fn image(&self, run: &Run) -> Option<View> {
        let image = self.images.get(self.source(run)?)?;
        let (w, h) = shown_size(image, run, self.column);
        let stretch = if fills(run) {
            Stretch::Fill
        } else {
            Stretch::Uniform
        };
        let picture: View = Image::new()
            .source_data(image.data.clone())
            .stretch(stretch)
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
    /// 文の中に置く画像の鍵（読めた画像を一時ファイルに書き、`inline_image` が部品にする）。読めなければ None。
    fn inline_key(&self, run: &Run) -> Option<String> {
        let image = self.images.get(self.source(run)?)?;
        let (w, h) = shown_size(image, run, self.column);
        let bytes = image.data.as_bytes();
        let hash = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            bytes.hash(&mut hasher);
            hasher.finish()
        };
        let dir = std::env::temp_dir().join("amonhen-markdown-images");
        let file = dir.join(format!("{hash:016x}"));
        if !file.exists() {
            std::fs::create_dir_all(&dir).ok()?;
            std::fs::write(&file, bytes).ok()?;
        }
        let fill = fills(run);
        let key = format!("{hash:016x}:{w}:{h}:{fill}");
        INLINE_IMAGES.with(|m| {
            let mut m = m.borrow_mut();
            if m.len() > 512 {
                m.clear();
            }
            m.insert(key.clone(), (file, w, h, fill, run.text.clone(), 0.0));
        });
        Some(key)
    }

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
                inline: None,
            })
        })
        .collect()
}

/// 段落・見出し: 読めた画像は絵で、ほかの文字はそのまま。画像だけの段落（バッジの並びなど）は横に並べる。HTML の
/// `align` でそろえる（短い行は真ん中・右に寄る。GTK4 版の `justify`）。
fn paragraph(runs: &[Run], size: f64, bold: bool, align: Align, pictures: &Pictures) -> View {
    let has_text = runs
        .iter()
        .any(|r| (!r.text.trim().is_empty() && pictures.image(r).is_none()) || r.line_break);
    let has_math = runs.iter().any(|r| r.math.is_some());
    let view = if has_math || (has_text && runs.iter().any(|r| pictures.image(r).is_some())) {
        // 文の途中の画像と文中の数式: 文と同じ行に置く（GitHub と同じく、行は切らない。macOS 版・GTK4 版と同じ）。
        let mut inlines = inlines(runs, bold);
        for (inline, run) in inlines.iter_mut().zip(runs) {
            // リンクの中の画像も絵で（文の中の部品はリンクにできないので、リンクは外す）。
            let key = if run.math.is_some() {
                math_key(run, size)
            } else {
                pictures.inline_key(run)
            };
            if let Some(key) = key {
                *inline = RichTextInline::Run(RichTextRun {
                    text: run.text.clone(),
                    inline: Some(key),
                    ..RichTextRun::default()
                });
            }
        }
        RichTextBlock::new()
            .paragraphs(RichText::single_paragraph(inlines))
            .font_size(size)
            .is_text_selection_enabled(true)
            .text_wrapping(TextWrapping::Wrap)
            .into()
    } else if runs.iter().any(|r| pictures.image(r).is_some()) {
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
        // 描けた数式のブロックは、真ん中に絵で（幅は 640 まで、比を保って縮める。macOS 版・GTK4 版と同じ）。
        // 描けなければ TeX をコードとして見せ、理由を添える。
        Block::Math {
            text: code,
            math,
            error,
        } => match math.as_ref().and_then(|m| math_png(m, BODY)) {
            Some((png, w, h, _)) => {
                let (w, h) = if w > MAX_IMAGE_WIDTH {
                    (MAX_IMAGE_WIDTH, h * MAX_IMAGE_WIDTH / w)
                } else {
                    (w, h)
                };
                Border::new()
                    .horizontal_alignment(HorizontalAlignment::Center)
                    .content(
                        Image::new()
                            .source_data(EncodedImage::new(png.to_vec()))
                            .stretch(Stretch::Uniform)
                            .width(w)
                            .height(h),
                    )
                    .tooltip(code.clone())
            }
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
            // 項目の中身は 1 段深い（中の箇条書きの印を変える）。組み立て終えたら戻す。
            let depth = pictures.lists.get();
            pictures.lists.set(depth + 1);
            let list = StackPanel::new()
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
                                (None, None) => bullet(depth).to_owned(),
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
                ),));
            pictures.lists.set(depth);
            list
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
    column: f64,
) -> View {
    DETAILS.with(|d| {
        let mut d = d.borrow_mut();
        if d.0 != key {
            *d = (key, Default::default());
        }
    });
    let pictures = Pictures {
        details: Cell::new(0),
        lists: Cell::new(0),
        list: diagrams,
        next: Cell::new(0),
        images,
        dark: crate::palette::dark(),
        column,
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
            .max_width(MAX_WIDTH)
            .horizontal_alignment(HorizontalAlignment::Left)
            .padding(Thickness::new(SIDES / 2.0, 16.0, SIDES / 2.0, 24.0))
            .content(content),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bullets_change_with_depth() {
        assert_eq!(bullet(0), "•");
        assert_eq!(bullet(1), "◦");
        assert_eq!(bullet(2), "▪\u{fe0e}");
        assert_eq!(bullet(3), "•");
    }

    #[test]
    fn percent_widths_follow_the_column() {
        assert_eq!(percent_width(50, 852.0), 426.0);
        assert_eq!(percent_width(25, 852.0), 213.0);
        assert_eq!(percent_width(100, 852.0), 852.0);
        // 範囲の外は 1〜100 に収める（コアも 100 で止める）
        assert_eq!(percent_width(250, 852.0), 852.0);
        assert_eq!(percent_width(0, 852.0), 9.0);
    }

    #[test]
    fn the_column_follows_the_window_up_to_its_widest() {
        // 広ければ欄の最大（900 から左右の余白を除いた 852）、狭ければ余白を除いた幅。
        assert_eq!(column_width(1400.0), 852.0);
        assert_eq!(column_width(900.0), 852.0);
        assert_eq!(column_width(548.0), 500.0);
        assert_eq!(percent_width(50, column_width(548.0)), 250.0);
        assert_eq!(column_width(10.0), 1.0);
    }
}
