//! Markdown の整形表示（History の「すべてのファイル」。macOS 版の `MarkdownView`）。コアが読んだブロック
//! （`amonhen_core::markdown`）を、読むだけの GtkTextView の文字に自前で組み立てる（WebKit も JavaScript も
//! 使わない）。画像はそのコミットの中のものだけを読み、外へは何も読みに行かない（外の画像は代わりの文を薄く）。
//! リンクは押すとブラウザで開き、リポジトリの中のファイルへのリンクは、そのファイルを「すべてのファイル」で開く。
//!
//! GtkTextView にない枠は、段落の背景で代える: 引用・囲みは左を下げて薄い背景、コードは背景、横線は細い背景の行。
//! 表は GtkGrid を文字の間に埋め込む。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use amonhen_core::markdown::{Align, Block, Run};
use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gdk, gio, glib, pango};

/// 本文の大きさ（pt。GTK のふだんの文字の大きさ）。
const BODY: f64 = 11.0;
/// 見出しの大きさ（pt。macOS 版と同じ数）。
const HEADINGS: [f64; 6] = [26.0, 21.0, 17.0, 15.0, 14.0, 13.0];
/// リストの 1 段の深さ（px）。
const STEP: i32 = 22;
/// 画像の幅の上限（px）。
const MAX_IMAGE_WIDTH: i32 = 640;

const LINK: gdk::RGBA = gdk::RGBA::new(0.21, 0.52, 0.89, 1.0);
const DIM: gdk::RGBA = gdk::RGBA::new(0.5, 0.5, 0.5, 1.0);
const SHADE: gdk::RGBA = gdk::RGBA::new(0.5, 0.5, 0.5, 0.12);

/// リンクを押したとき（リポジトリの中のファイルのパス）。
pub type OnOpenPath = Rc<dyn Fn(String)>;

/// Markdown のファイル `from` からの相対パス `link` を、リポジトリの根からのパスにする（`../` も解く。macOS 版の
/// `resolve`）。外の URL・空なら None。
pub fn resolve(link: &str, from: &str) -> Option<String> {
    let target = link.split('#').next().unwrap_or(link);
    if target.is_empty() || target.contains("://") {
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
    let joined = parts.join("/");
    let path = glib::Uri::unescape_string(&joined, None::<&str>)
        .map(|s| s.to_string())
        .unwrap_or(joined);
    (!path.is_empty()).then_some(path)
}

thread_local! {
    /// 裏で画像にしている図の数（`--snapshot` はなくなるまで待つ）。
    static DIAGRAMS_PENDING: Cell<u32> = const { Cell::new(0) };
}

/// 裏で画像にしている図があるか。
pub fn diagrams_pending() -> bool {
    DIAGRAMS_PENDING.with(|p| p.get() > 0)
}

/// 図の絵（幅は 640 px まで）。色は図の中の指定 `own`、なければ設定の「図の配色」と今の外観・テーマで当て、
/// 外観・テーマ・図の配色が変わったら色だけを当て直す（形と位置はそのまま）。色を当てられなければ `None`。
pub fn diagram_picture(
    svg: String,
    own: Option<amonhen_core::diagram::MermaidConfig>,
    alt: &str,
) -> Option<gtk::Picture> {
    // 高い解像度の画面でぼやけないよう、2 倍の画素で画像にして論理の大きさで見せる（GTK4 の画面の倍率は 1 か 2）。
    // 画像にするのは裏のスレッドで（大きな図は数十 ms かかる）。大きさは SVG の属性から先に決めて場所を取っておく。
    const SCALE: i32 = 2;
    let colored = move |svg: &str, dark: bool| {
        let (vars, backdrop) = crate::theme::diagram_palette(dark, own.as_ref());
        amonhen_core::diagram::colorize(svg, &vars, backdrop)
    };
    let style = adw::StyleManager::default();
    let first = colored(&svg, style.is_dark())?;
    let (natural_w, natural_h) = crate::imagediff::svg_size(&first)?;
    let (w, h) = if natural_w > MAX_IMAGE_WIDTH {
        (
            MAX_IMAGE_WIDTH,
            natural_h * MAX_IMAGE_WIDTH / natural_w.max(1),
        )
    } else {
        (natural_w, natural_h)
    };
    let picture = gtk::Picture::new();
    picture.set_can_shrink(true);
    picture.set_content_fit(gtk::ContentFit::Contain);
    picture.set_size_request(w, h);
    picture.set_alternative_text(Some(alt));
    // 色を当てた SVG を裏で画像にして見せる。続けて頼まれたら、最後のものだけを見せる。
    let generation = Rc::new(Cell::new(0u32));
    let show = {
        let weak = picture.downgrade();
        let generation = generation.clone();
        Rc::new(move |colored_svg: String| {
            let mine = generation.get().wrapping_add(1);
            generation.set(mine);
            let weak = weak.clone();
            let generation = generation.clone();
            DIAGRAMS_PENDING.with(|p| p.set(p.get() + 1));
            glib::spawn_future_local(async move {
                let pixels = gio::spawn_blocking(move || {
                    crate::imagediff::svg_pixels(
                        colored_svg.as_bytes(),
                        natural_w * SCALE,
                        natural_h * SCALE,
                    )
                })
                .await
                .ok()
                .flatten();
                DIAGRAMS_PENDING.with(|p| p.set(p.get() - 1));
                if generation.get() != mine {
                    return;
                }
                if let (Some(picture), Some(pixels)) = (weak.upgrade(), pixels) {
                    picture.set_paintable(Some(&pixels.texture()));
                }
            });
        })
    };
    show(first);
    let recolor = {
        let show = show.clone();
        Rc::new(move || {
            if let Some(s) = colored(&svg, adw::StyleManager::default().is_dark()) {
                show(s);
            }
        })
    };
    let again = recolor.clone();
    style.connect_dark_notify(move |_| again());
    crate::settings::connect_changed_while(&picture, move |key| {
        if matches!(
            key,
            crate::theme::SYNTAX_THEME_LIGHT
                | crate::theme::SYNTAX_THEME_DARK
                | crate::theme::IMPORTED_SYNTAX_THEMES
                | crate::theme::DIAGRAM_COLORS
                | crate::theme::IMPORTED_MERMAID_THEMES
        ) {
            recolor();
        }
    });
    Some(picture)
}

/// `blocks` の中の画像の場所（重ならずに、出てくる順）。
pub fn image_sources(blocks: &[Block]) -> Vec<String> {
    fn runs(runs: &[Run], out: &mut Vec<String>) {
        for run in runs {
            if let Some(src) = &run.image
                && !out.contains(src)
            {
                out.push(src.clone());
            }
        }
    }
    fn walk(blocks: &[Block], out: &mut Vec<String>) {
        for block in blocks {
            match block {
                Block::Heading { runs: r, .. } | Block::Paragraph { runs: r } => runs(r, out),
                Block::Quote { blocks, .. } | Block::Footnote { blocks, .. } => walk(blocks, out),
                Block::List { items, .. } => {
                    for item in items {
                        walk(&item.blocks, out);
                    }
                }
                Block::Table { head, rows, .. } => {
                    for cell in head.iter().chain(rows.iter().flatten()) {
                        runs(cell, out);
                    }
                }
                Block::Code { .. } | Block::Diagram { .. } | Block::Rule | Block::Html { .. } => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(blocks, &mut out);
    out
}

/// フェンスの言葉から、構文の色付けの拡張子を決める（言葉がそのまま拡張子のものはそのまま。macOS 版と同じ）。
fn file_extension(language: &str) -> String {
    let lower = language.to_lowercase();
    match lower.as_str() {
        "rust" => "rs",
        "python" => "py",
        "javascript" => "js",
        "typescript" => "ts",
        "shell" | "bash" | "zsh" => "sh",
        "ruby" => "rb",
        "kotlin" => "kt",
        "markdown" => "md",
        "yaml" => "yml",
        "csharp" | "c#" => "cs",
        "c++" => "cpp",
        "objective-c" => "m",
        "golang" => "go",
        _ => return lower,
    }
    .to_owned()
}

/// GitHub の囲みの色と見出し。訳の鍵は「GitHub alert: 」を付けたもの（"Tip" はブランチの「先端」と重なるため）
/// で、英語では付けたところを落とす。
fn alert(kind: Option<&str>) -> Option<(gdk::RGBA, String)> {
    let (color, title) = match kind? {
        "note" => (
            gdk::RGBA::new(0.21, 0.52, 0.89, 1.0),
            tr!("GitHub alert: Note"),
        ),
        "tip" => (
            gdk::RGBA::new(0.18, 0.76, 0.49, 1.0),
            tr!("GitHub alert: Tip"),
        ),
        "important" => (
            gdk::RGBA::new(0.57, 0.25, 0.67, 1.0),
            tr!("GitHub alert: Important"),
        ),
        "warning" => (
            gdk::RGBA::new(0.90, 0.38, 0.0, 1.0),
            tr!("GitHub alert: Warning"),
        ),
        "caution" => (
            gdk::RGBA::new(0.88, 0.11, 0.14, 1.0),
            tr!("GitHub alert: Caution"),
        ),
        _ => return None,
    };
    let title = title
        .strip_prefix("GitHub alert: ")
        .map_or(title.clone(), str::to_owned);
    Some((color, title))
}

/// 表のマスの CSS（はじめて表を出すときに一度だけ入れる）。
fn install_css() {
    thread_local! {
        static DONE: Cell<bool> = const { Cell::new(false) };
    }
    if DONE.with(|d| d.replace(true)) {
        return;
    }
    let provider = gtk::CssProvider::new();
    provider.load_from_string(
        ".md-table { background-color: alpha(currentColor, 0.18); \
           border: 1px solid alpha(currentColor, 0.18); } \
         .md-cell { background-color: @view_bg_color; padding: 4px 8px; } \
         .md-head { background-color: mix(@view_bg_color, @view_fg_color, 0.06); }",
    );
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// 整形表示を作る（スクロールする欄）。`path` は Markdown のファイル、`images` は画像の場所 → 読んだ絵（この
/// コミットの中で読めたものだけ）。リポジトリの中のファイルへのリンクを押したら `on_open`。
pub fn view(
    blocks: &[Block],
    path: &str,
    images: &HashMap<String, gdk::Texture>,
    on_open: OnOpenPath,
) -> gtk::Widget {
    let text = gtk::TextView::new();
    text.set_editable(false);
    text.set_cursor_visible(false);
    text.set_wrap_mode(gtk::WrapMode::WordChar);
    text.set_left_margin(24);
    text.set_right_margin(24);
    text.set_top_margin(20);
    text.set_bottom_margin(20);
    let builder = Builder::new(&text, images);
    builder.blocks(blocks, &Ctx::default());
    let links = Rc::new(builder.links.take());
    // 末尾の余分な改行を落とす。
    let buffer = text.buffer();
    let mut end = buffer.end_iter();
    let mut before = end;
    if before.backward_char() && before.char() == '\n' {
        buffer.delete(&mut before, &mut end);
    }
    let from = path.to_owned();
    let open = {
        let text = text.downgrade();
        move |target: &str| {
            let window = text
                .upgrade()
                .and_then(|t| t.root())
                .and_downcast::<gtk::Window>();
            open_link(target, &from, window.as_ref(), &on_open);
        }
    };
    let open = Rc::new(open);
    // リンクを押す（文字を選んでいるときは押したことにしない）。
    let click = gtk::GestureClick::new();
    {
        let (weak, links, open) = (text.downgrade(), links.clone(), open.clone());
        click.connect_released(move |_, _, x, y| {
            let Some(text) = weak.upgrade() else { return };
            if text.buffer().has_selection() {
                return;
            }
            if let Some(target) = link_at(&text, &links, x, y) {
                open(&target);
            }
        });
    }
    text.add_controller(click);
    // リンクの上では指のカーソル。
    let motion = gtk::EventControllerMotion::new();
    {
        let (weak, links) = (text.downgrade(), links.clone());
        motion.connect_motion(move |_, x, y| {
            let Some(text) = weak.upgrade() else { return };
            let over = link_at(&text, &links, x, y).is_some();
            text.set_cursor_from_name(Some(if over { "pointer" } else { "text" }));
        });
    }
    text.add_controller(motion);
    // 表のマスの中のリンク（ラベルの <a>）。
    for label in builder.labels.take() {
        let open = open.clone();
        label.connect_activate_link(move |_, uri| {
            open(uri);
            glib::Propagation::Stop
        });
    }
    gtk::ScrolledWindow::builder()
        .child(&text)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build()
        .upcast()
}

/// 押したリンクを開く: http・https・mailto は既定のアプリで、リポジトリの中のファイルは `on_open` で。
fn open_link(target: &str, from: &str, window: Option<&gtk::Window>, on_open: &OnOpenPath) {
    let scheme = target.split_once(':').map(|(s, _)| s.to_ascii_lowercase());
    if matches!(scheme.as_deref(), Some("http" | "https" | "mailto")) {
        gtk::UriLauncher::new(target).launch(window, gio::Cancellable::NONE, |_| {});
    } else if !target.starts_with('#')
        && let Some(path) = resolve(target, from)
    {
        on_open(path);
    }
}

/// 部品の中の位置 (`x`, `y`) のリンク先。
fn link_at(text: &gtk::TextView, links: &[(i32, i32, String)], x: f64, y: f64) -> Option<String> {
    let (bx, by) = text.window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
    let iter = text.iter_at_location(bx, by)?;
    let offset = iter.offset();
    links
        .iter()
        .find(|(start, end, _)| (*start..*end).contains(&offset))
        .map(|(_, _, url)| url.clone())
}

/// 入れ子の様子（左の下げ・引用の背景）。
#[derive(Clone, Default)]
struct Ctx {
    margin: i32,
    /// 引用・囲みの中（背景の色）。
    quote: Option<gdk::RGBA>,
    /// 引用の文字を薄くするか（囲みでない引用）。
    dim: bool,
    /// リストの項目の最初の段落の頭に置く印（置いたら消す）。
    marker: Rc<RefCell<Option<String>>>,
}

struct Builder<'a> {
    view: gtk::TextView,
    buffer: gtk::TextBuffer,
    images: &'a HashMap<String, gdk::Texture>,
    /// リンク（文字の範囲と先）。
    links: RefCell<Vec<(i32, i32, String)>>,
    /// 表のマスのラベル（中のリンクを押せるように）。
    labels: RefCell<Vec<gtk::Label>>,
}

impl<'a> Builder<'a> {
    fn new(view: &gtk::TextView, images: &'a HashMap<String, gdk::Texture>) -> Self {
        Self {
            view: view.clone(),
            buffer: view.buffer(),
            images,
            links: RefCell::default(),
            labels: RefCell::default(),
        }
    }

    /// 名前 `name` の飾りを、なければ `make` で作って返す。
    fn tag(&self, name: &str, make: impl FnOnce(&gtk::TextTag)) -> gtk::TextTag {
        let table = self.buffer.tag_table();
        table.lookup(name).unwrap_or_else(|| {
            let tag = gtk::TextTag::new(Some(name));
            make(&tag);
            table.add(&tag);
            tag
        })
    }

    fn offset(&self) -> i32 {
        self.buffer.end_iter().offset()
    }

    fn insert(&self, text: &str, tags: &[gtk::TextTag]) {
        let start = self.offset();
        let mut end = self.buffer.end_iter();
        self.buffer.insert(&mut end, text);
        self.apply(start, tags);
    }

    /// `start` から終わりまでに `tags` を付ける。
    fn apply(&self, start: i32, tags: &[gtk::TextTag]) {
        let (s, e) = (self.buffer.iter_at_offset(start), self.buffer.end_iter());
        for tag in tags {
            self.buffer.apply_tag(tag, &s, &e);
        }
    }

    /// 段落の形（左の下げ・ぶら下げ・引用の背景・下の間隔）を、`start` から終わりまでに付ける。
    fn paragraph(&self, start: i32, ctx: &Ctx, hanging: bool, gap: i32) {
        let margin = ctx.margin;
        let indent = if hanging { -STEP } else { 0 };
        let shape = self.tag(&format!("shape-{margin}-{indent}"), |t| {
            t.set_left_margin(24 + margin);
            t.set_indent(indent);
            if hanging {
                let mut tabs = pango::TabArray::new(1, true);
                tabs.set_tab(0, pango::TabAlign::Left, STEP);
                t.set_tabs(Some(&tabs));
            }
        });
        let gap = self.tag(&format!("gap-{gap}"), |t| t.set_pixels_below_lines(gap));
        let mut tags = vec![shape, gap];
        if let Some(color) = ctx.quote {
            let name = format!("quote-{}", color.to_str());
            tags.push(self.tag(&name, |t| {
                t.set_paragraph_background_rgba(Some(&gdk::RGBA::new(
                    color.red(),
                    color.green(),
                    color.blue(),
                    0.10,
                )));
            }));
        }
        if ctx.dim {
            tags.push(self.dim());
        }
        self.apply(start, &tags);
    }

    fn dim(&self) -> gtk::TextTag {
        self.tag("dim", |t| t.set_foreground_rgba(Some(&DIM)))
    }

    /// リストの印があれば、段落の頭に置く（印はぶら下げる）。置いたら true。
    fn marker(&self, ctx: &Ctx) -> bool {
        let Some(marker) = ctx.marker.borrow_mut().take() else {
            return false;
        };
        let dim = self.dim();
        self.insert(&format!("{marker}\t"), &[dim]);
        true
    }

    /// 飾りの文字。`size` は pt、`bold` なら見出し・表の頭。
    fn runs(&self, runs: &[Run], size: f64, bold: bool) {
        for run in runs {
            if run.line_break {
                self.insert("\u{2028}", &[]);
                continue;
            }
            if let Some(src) = &run.image {
                self.image(src, &run.text);
                continue;
            }
            let mut tags = vec![self.tag(&format!("size-{size}"), |t| t.set_size_points(size))];
            if bold || run.bold {
                tags.push(self.tag("bold", |t| t.set_weight(700)));
            }
            if run.italic {
                tags.push(self.tag("italic", |t| t.set_style(pango::Style::Italic)));
            }
            if run.strike {
                tags.push(self.tag("strike", |t| t.set_strikethrough(true)));
            }
            if run.code || run.html {
                tags.push(self.tag("mono", |t| {
                    t.set_family(Some("monospace"));
                    t.set_scale(0.9);
                }));
            }
            if run.code {
                tags.push(self.tag("code", |t| t.set_background_rgba(Some(&SHADE))));
            }
            if run.html {
                tags.push(self.dim());
            }
            if run.custom_emoji.is_some() {
                // GitHub 独自の絵文字は画像を持たないので、名前を薄い札で見せる。
                tags.push(self.tag("custom-emoji", |t| {
                    t.set_scale(0.85);
                    t.set_background_rgba(Some(&SHADE));
                    t.set_foreground_rgba(Some(&gdk::RGBA::new(0.6, 0.6, 0.6, 1.0)));
                }));
            }
            let mut text = run.text.clone();
            if let Some(note) = &run.footnote {
                text = format!("[{note}]");
                tags.push(self.tag("footnote", |t| {
                    t.set_scale(0.7);
                    t.set_rise((size * 0.35 * pango::SCALE as f64) as i32);
                }));
            }
            let start = self.offset();
            if let Some(link) = &run.link {
                tags.push(self.tag("link", |t| {
                    t.set_foreground_rgba(Some(&LINK));
                    t.set_underline(pango::Underline::Single);
                }));
                self.insert(&text, &tags);
                self.links
                    .borrow_mut()
                    .push((start, self.offset(), link.clone()));
            } else {
                self.insert(&text, &tags);
            }
        }
    }

    /// 画像: このコミットの中で読めたものは絵（幅は 640 px まで）、ほか（外の URL など）は代わりの文を薄く。
    fn image(&self, src: &str, alt: &str) {
        match self.images.get(src) {
            Some(texture) => {
                let (w, h) = (texture.width(), texture.height());
                let (w, h) = if w > MAX_IMAGE_WIDTH {
                    (MAX_IMAGE_WIDTH, h * MAX_IMAGE_WIDTH / w.max(1))
                } else {
                    (w, h)
                };
                let picture = gtk::Picture::for_paintable(texture);
                picture.set_can_shrink(true);
                picture.set_content_fit(gtk::ContentFit::Contain);
                picture.set_size_request(w, h);
                if !alt.is_empty() {
                    picture.set_alternative_text(Some(alt));
                }
                let mut end = self.buffer.end_iter();
                let anchor = self.buffer.create_child_anchor(&mut end);
                self.view.add_child_at_anchor(&picture, &anchor);
            }
            None => {
                let dim = self.dim();
                self.insert(
                    &format!("[{}]", if alt.is_empty() { src } else { alt }),
                    &[dim],
                );
            }
        }
    }

    fn blocks(&self, blocks: &[Block], ctx: &Ctx) {
        for block in blocks {
            self.block(block, ctx);
        }
    }

    fn block(&self, block: &Block, ctx: &Ctx) {
        match block {
            Block::Heading { level, runs } => {
                let start = self.offset();
                let hanging = self.marker(ctx);
                let size = HEADINGS[(*level).clamp(1, 6) as usize - 1];
                self.runs(runs, size, true);
                self.insert("\n", &[]);
                let above = self.tag("heading-above", |t| t.set_pixels_above_lines(10));
                if start > 0 {
                    self.apply_range(start, &above);
                }
                self.paragraph(start, ctx, hanging, if *level <= 2 { 12 } else { 8 });
            }
            Block::Paragraph { runs } => {
                let start = self.offset();
                let hanging = self.marker(ctx);
                self.runs(runs, BODY, false);
                self.insert("\n", &[]);
                let gap = if ctx.marker_list() { 4 } else { 10 };
                self.paragraph(start, ctx, hanging, gap);
            }
            Block::Code { language, text } => {
                self.lone_marker(ctx);
                let start = self.offset();
                // 行は段落を分けずに改行する（背景を 1 つにつなぐ）。長さは変わらないので構文の色の位置はそのまま。
                let code = text.trim_end_matches('\n');
                let mono = self.tag("codeblock", |t| {
                    t.set_family(Some("monospace"));
                    t.set_size_points(BODY - 1.0);
                    t.set_paragraph_background_rgba(Some(&SHADE));
                    t.set_pixels_above_lines(6);
                    t.set_right_margin(24);
                });
                self.insert(&code.replace('\n', "\u{2028}"), &[mono]);
                self.syntax(start, code, &format!("code.{}", file_extension(language)));
                self.insert("\n", &[]);
                self.paragraph(start, ctx, false, 12);
            }
            Block::Diagram {
                language,
                text,
                svg,
                config,
                note,
                error,
            } => {
                let Some(picture) = svg
                    .as_ref()
                    .and_then(|svg| diagram_picture(svg.clone(), config.clone(), text))
                else {
                    // 描けない図はコードとして見せ、理由を添える
                    self.block(
                        &Block::Code {
                            language: language.clone(),
                            text: text.clone(),
                        },
                        ctx,
                    );
                    if let Some(error) = error {
                        let start = self.offset();
                        let red = self.tag("diagram-error", |t| {
                            t.set_foreground_rgba(Some(&gdk::RGBA::new(0.85, 0.2, 0.2, 1.0)));
                            // 誤りの位置を ^ で指すので等幅で
                            t.set_family(Some("monospace"));
                            t.set_scale(0.9);
                        });
                        self.insert(&error.replace('\n', "\u{2028}"), &[red]);
                        self.insert("\n", &[]);
                        self.paragraph(start, ctx, false, 10);
                    }
                    return;
                };
                self.lone_marker(ctx);
                let start = self.offset();
                let mut end = self.buffer.end_iter();
                let anchor = self.buffer.create_child_anchor(&mut end);
                self.view.add_child_at_anchor(&picture, &anchor);
                self.insert("\n", &[]);
                self.paragraph(start, ctx, false, 12);
                // 近いもので描いた図には、そのことを絵の下に添える
                if let Some(note) = note {
                    let start = self.offset();
                    let dim = self.tag("diagram-note", |t| {
                        t.set_foreground_rgba(Some(&gdk::RGBA::new(0.5, 0.5, 0.5, 1.0)));
                        t.set_scale(0.9);
                    });
                    self.insert(note, &[dim]);
                    self.insert("\n", &[]);
                    self.paragraph(start, ctx, false, 10);
                }
            }
            Block::Quote {
                alert: kind,
                blocks,
            } => {
                self.lone_marker(ctx);
                let (color, title) = match alert(kind.as_deref()) {
                    Some((color, title)) => (color, Some(title)),
                    None => (gdk::RGBA::new(0.5, 0.5, 0.5, 1.0), None),
                };
                let inner = Ctx {
                    margin: ctx.margin + 14,
                    quote: Some(color),
                    dim: ctx.dim || title.is_none(),
                    marker: Rc::default(),
                };
                if let Some(title) = title {
                    let start = self.offset();
                    let name = format!("alert-{}", color.to_str());
                    let tag = self.tag(&name, |t| {
                        t.set_foreground_rgba(Some(&color));
                        t.set_weight(700);
                    });
                    self.insert(&title, &[tag]);
                    self.insert("\n", &[]);
                    self.paragraph(start, &inner, false, 4);
                }
                self.blocks(blocks, &inner);
            }
            Block::List { start, items } => {
                // 項目の最初が入れ子のリストなら、外の印はその前の行に。
                self.lone_marker(ctx);
                let depth = (ctx.margin / STEP) as usize;
                for (i, item) in items.iter().enumerate() {
                    let marker = match (item.task, start) {
                        (Some(true), _) => "☑".to_owned(),
                        (Some(false), _) => "☐".to_owned(),
                        (None, Some(n)) => format!("{}.", n + i as u64),
                        (None, None) => ["•", "◦", "▪\u{fe0e}"][depth % 3].to_owned(),
                    };
                    let inner = Ctx {
                        margin: ctx.margin + STEP,
                        marker: Rc::new(RefCell::new(Some(marker))),
                        ..ctx.clone()
                    };
                    self.blocks(&item.blocks, &inner);
                    // 中身のない項目でも印は出す。
                    self.lone_marker(&inner);
                }
            }
            Block::Table { aligns, head, rows } => {
                self.lone_marker(ctx);
                self.table(aligns, head, rows, ctx);
            }
            Block::Rule => {
                self.lone_marker(ctx);
                let start = self.offset();
                let rule = self.tag("rule", |t| {
                    t.set_size_points(1.0);
                    t.set_paragraph_background_rgba(Some(&gdk::RGBA::new(0.5, 0.5, 0.5, 0.35)));
                    t.set_pixels_above_lines(8);
                });
                self.insert(" \n", &[rule]);
                self.paragraph(start, ctx, false, 12);
            }
            Block::Html { text } => {
                self.lone_marker(ctx);
                let start = self.offset();
                let mono = self.tag("mono", |t| {
                    t.set_family(Some("monospace"));
                    t.set_scale(0.9);
                });
                let dim = self.dim();
                self.insert(text.trim_end_matches('\n'), &[mono, dim]);
                self.insert("\n", &[]);
                self.paragraph(start, ctx, false, 10);
            }
            Block::Footnote { label, blocks } => {
                self.lone_marker(ctx);
                let start = self.offset();
                let dim = self.dim();
                let small = self.tag(&format!("size-{}", BODY - 2.0), |t| {
                    t.set_size_points(BODY - 2.0)
                });
                self.insert(&format!("[{label}]"), &[dim, small]);
                self.insert("\n", &[]);
                self.paragraph(start, ctx, false, 2);
                let inner = Ctx {
                    margin: ctx.margin + 16,
                    marker: Rc::default(),
                    ..ctx.clone()
                };
                self.blocks(blocks, &inner);
            }
        }
    }

    /// 段落でないブロック（コード・表など）の前に、残っているリストの印を 1 行で置く。
    fn lone_marker(&self, ctx: &Ctx) {
        let start = self.offset();
        if self.marker(ctx) {
            self.insert("\n", &[]);
            self.paragraph(start, ctx, true, 2);
        }
    }

    fn apply_range(&self, start: i32, tag: &gtk::TextTag) {
        self.apply(start, std::slice::from_ref(tag));
    }

    /// コードに構文の色を付ける（`start` は文字の位置、`code` は改行を置き換える前の文字）。
    fn syntax(&self, start: i32, code: &str, name: &str) {
        let spans = amonhen_core::diff::text_syntax(name, code.as_bytes());
        if spans.is_empty() {
            return;
        }
        // UTF-16 の位置を文字の位置に。
        let mut units = Vec::with_capacity(code.len() + 1);
        for (i, c) in code.chars().enumerate() {
            for _ in 0..c.len_utf16() {
                units.push(i as i32);
            }
        }
        units.push(code.chars().count() as i32);
        let fg = self.view.color();
        for span in spans {
            let Some((r, g, b)) = crate::diff::syntax_color(span.kind, &fg) else {
                continue;
            };
            let tag = self.tag(&format!("syntax-{}", span.kind), |t| {
                t.set_foreground_rgba(Some(&gdk::RGBA::new(r, g, b, 1.0)));
            });
            let at = |u: u32| start + units.get(u as usize).copied().unwrap_or(0);
            let (s, e) = (
                self.buffer.iter_at_offset(at(span.start)),
                self.buffer.iter_at_offset(at(span.end)),
            );
            self.buffer.apply_tag(&tag, &s, &e);
        }
    }

    /// 表（GtkGrid を埋め込む。頭の行は太字と背景、列のそろえ方、マスの枠）。
    fn table(&self, aligns: &[Align], head: &[Vec<Run>], rows: &[Vec<Vec<Run>>], ctx: &Ctx) {
        install_css();
        let grid = gtk::Grid::new();
        grid.add_css_class("md-table");
        grid.set_row_spacing(1);
        grid.set_column_spacing(1);
        let columns = rows
            .iter()
            .map(Vec::len)
            .chain(std::iter::once(head.len()))
            .max()
            .unwrap_or(0);
        for (r, row) in std::iter::once(head)
            .chain(rows.iter().map(Vec::as_slice))
            .enumerate()
        {
            for column in 0..columns {
                let label = gtk::Label::new(None);
                label.set_use_markup(true);
                label.set_selectable(true);
                label.set_wrap(true);
                label.set_max_width_chars(60);
                label.add_css_class("md-cell");
                if r == 0 {
                    label.add_css_class("md-head");
                }
                let (xalign, justify) = match aligns.get(column).copied().unwrap_or(Align::None) {
                    Align::Center => (0.5, gtk::Justification::Center),
                    Align::Right => (1.0, gtk::Justification::Right),
                    _ => (0.0, gtk::Justification::Left),
                };
                label.set_xalign(xalign);
                label.set_justify(justify);
                label.set_hexpand(true);
                label.set_markup(&markup(
                    row.get(column).map_or(&[][..], Vec::as_slice),
                    r == 0,
                ));
                grid.attach(&label, column as i32, r as i32, 1, 1);
                self.labels.borrow_mut().push(label);
            }
        }
        let start = self.offset();
        let mut end = self.buffer.end_iter();
        let anchor = self.buffer.create_child_anchor(&mut end);
        self.view.add_child_at_anchor(&grid, &anchor);
        self.insert("\n", &[]);
        self.paragraph(start, ctx, false, 10);
    }
}

impl Ctx {
    /// リストの項目の中か（段落の下の間隔を狭くする）。
    fn marker_list(&self) -> bool {
        self.margin >= STEP && self.quote.is_none()
    }
}

/// 表のマスの文字（Pango のマークアップ）。画像は代わりの文。
fn markup(runs: &[Run], head: bool) -> String {
    let esc = |s: &str| glib::markup_escape_text(s).to_string();
    let mut out = String::new();
    for run in runs {
        if run.line_break {
            out.push('\n');
            continue;
        }
        if let Some(src) = &run.image {
            let alt = if run.text.is_empty() { src } else { &run.text };
            out += &format!("<span alpha=\"60%\">[{}]</span>", esc(alt));
            continue;
        }
        let mut text = esc(&run.text);
        if let Some(note) = &run.footnote {
            text = format!("<sup>[{}]</sup>", esc(note));
        }
        if run.code || run.html {
            text = format!("<tt>{text}</tt>");
        }
        if run.bold || head {
            text = format!("<b>{text}</b>");
        }
        if run.italic {
            text = format!("<i>{text}</i>");
        }
        if run.strike {
            text = format!("<s>{text}</s>");
        }
        if let Some(link) = &run.link {
            text = format!("<a href=\"{}\">{text}</a>", esc(link));
        }
        out += &text;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_links_from_the_markdown_file() {
        assert_eq!(
            resolve("img/a.png", "docs/README.md").as_deref(),
            Some("docs/img/a.png")
        );
        assert_eq!(
            resolve("../LICENSE", "docs/README.md").as_deref(),
            Some("LICENSE")
        );
        assert_eq!(resolve("./b.md#part", "a.md").as_deref(), Some("b.md"));
        assert_eq!(
            resolve("/src/main.rs", "docs/a.md").as_deref(),
            Some("src/main.rs")
        );
        assert_eq!(
            resolve("my%20file.md", "a.md").as_deref(),
            Some("my file.md")
        );
        assert_eq!(resolve("https://example.com/x.png", "a.md"), None);
        assert_eq!(resolve("#top", "a.md"), None);
    }

    #[test]
    fn finds_images_in_every_block() {
        let blocks = amonhen_core::markdown::parse(
            "![a](a.png)\n\n- ![b](b.png)\n\n| x |\n|---|\n| ![c](c.png) |\n\n![a](a.png)\n",
        );
        assert_eq!(image_sources(&blocks), ["a.png", "b.png", "c.png"]);
    }

    #[test]
    fn maps_fence_languages_to_extensions() {
        assert_eq!(file_extension("Rust"), "rs");
        assert_eq!(file_extension("bash"), "sh");
        assert_eq!(file_extension("swift"), "swift");
    }
}
