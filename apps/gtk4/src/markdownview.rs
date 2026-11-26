//! Markdown の整形表示（History の「すべてのファイル」。macOS 版の `MarkdownView`）。コアが読んだブロック
//! （`amonhen_core::markdown`）を、読むだけの GtkTextView の文字に自前で組み立てる（WebKit も JavaScript も
//! 使わない）。画像はそのコミットの中のものだけを読み、外へは何も読みに行かない（外の画像は代わりの文を薄く）。
//! リンクは押すとブラウザで開き、リポジトリの中のファイルへのリンクは、そのファイルを「すべてのファイル」で開く。
//!
//! GtkTextView にない枠は、段落の背景で代える: 引用・囲みは左を下げて薄い背景、コードは背景、横線は細い背景の行。
//! 表は GtkGrid を、`<kbd>` は枠付きのラベルを文字の間に埋め込む。HTML の `<details>` は見出しを押すと開く・閉じる
//! （組み直す）。`<picture>` は外観に合う画像を出し、外観が変わったら組み直す。数式はコアが描いた SVG に文字の色を
//! 当てて絵にし（`formula`）、文中の式は文字として埋め込んで基線にそろえる（`rise`）。Ctrl+F で見えている文字を
//! 探す（`markdownfind`）。

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
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
/// `<details>` の見出しのリンク（後ろに番号）。
const DETAILS_LINK: &str = "amonhen-details:";
/// 1 pt の画素（Pango の既定の 96 dpi）。数式の大きさを、文字の大きさ（pt）から画素にする。
const PX_PER_PT: f64 = 96.0 / 72.0;

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

/// 数式の絵（macOS 版の `FormulaPicture`）。コアが描いた SVG（色は `--math-fg` の変数のまま）に、Markdown の
/// 本文と同じ文字の色（TextView のスタイルの色。ユーザーの GTK のテーマ・明るい暗い・ハイコントラストに従う）を
/// 当て、裏のスレッドで画像にする。テーマが変わったら色だけを当て直す（大きさは変わらない）。文中に埋め込める
/// よう `gdk::Paintable` にする（`TextBuffer::insert_paintable`）。
mod formula {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use gtk::{gdk, gio, glib};

    use super::DIAGRAMS_PENDING;

    /// 文字の色の CSS の値（不透明なら `#rrggbb`、ほかは `rgba()`。librsvg はどちらも読む）。
    fn css(color: &gdk::RGBA) -> String {
        let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let (r, g, b) = (c(color.red()), c(color.green()), c(color.blue()));
        if color.alpha() >= 1.0 {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("rgba({r},{g},{b},{:.3})", color.alpha().clamp(0.0, 1.0))
        }
    }

    mod imp {
        use super::*;

        #[derive(Default)]
        pub struct Formula {
            /// 論理の大きさ（画素）。
            pub size: Cell<(i32, i32)>,
            pub texture: RefCell<Option<gdk::Texture>>,
            /// テーマの変わり目の知らせ（知らせる側と、捨てるときに外す番号）。
            pub watches: RefCell<Vec<(glib::WeakRef<glib::Object>, glib::SignalHandlerId)>>,
        }

        #[glib::object_subclass]
        impl ObjectSubclass for Formula {
            const NAME: &'static str = "AmonhenFormula";
            type Type = super::Formula;
            type Interfaces = (gdk::Paintable,);
        }

        impl ObjectImpl for Formula {
            fn dispose(&self) {
                for (source, id) in self.watches.take() {
                    if let Some(source) = source.upgrade() {
                        source.disconnect(id);
                    }
                }
            }
        }

        impl PaintableImpl for Formula {
            fn flags(&self) -> gdk::PaintableFlags {
                gdk::PaintableFlags::STATIC_SIZE
            }

            fn intrinsic_width(&self) -> i32 {
                self.size.get().0
            }

            fn intrinsic_height(&self) -> i32 {
                self.size.get().1
            }

            fn snapshot(&self, snapshot: &gdk::Snapshot, width: f64, height: f64) {
                if let Some(texture) = self.texture.borrow().as_ref() {
                    texture.snapshot(snapshot, width, height);
                }
            }
        }
    }

    glib::wrapper! {
        pub struct Formula(ObjectSubclass<imp::Formula>) @implements gdk::Paintable;
    }

    impl Formula {
        /// `svg` を論理の大きさ `width`×`height`（画素）で、`view`（Markdown の TextView）の文字の色で描く絵。
        /// 画像は `scale` 倍の画素で作る（高い解像度の画面でぼやけないように）。
        pub fn new(
            svg: &str,
            width: i32,
            height: i32,
            scale: i32,
            view: &gtk::TextView,
        ) -> Formula {
            let obj: Formula = glib::Object::new();
            let (width, height) = (width.max(1), height.max(1));
            obj.imp().size.set((width, height));
            let svg: Rc<str> = Rc::from(svg);
            let generation = Rc::new(Cell::new(0u32));
            let last = Rc::new(RefCell::new(String::new()));
            let (weak, view_weak) = (obj.downgrade(), view.downgrade());
            let render = Rc::new(move || {
                let Some(view) = view_weak.upgrade() else {
                    return;
                };
                // 同じ色なら描き直さない
                let fg = css(&view.color());
                if *last.borrow() == fg {
                    return;
                }
                let Some(colored) = amonhen_core::math::colorize(&svg, &fg) else {
                    return;
                };
                last.replace(fg);
                // 続けて頼まれたら、最後のものだけを見せる。
                let mine = generation.get().wrapping_add(1);
                generation.set(mine);
                let (weak, generation) = (weak.clone(), generation.clone());
                DIAGRAMS_PENDING.with(|p| p.set(p.get() + 1));
                glib::spawn_future_local(async move {
                    let pixels = gio::spawn_blocking(move || {
                        crate::imagediff::svg_pixels(
                            colored.as_bytes(),
                            width * scale,
                            height * scale,
                        )
                    })
                    .await
                    .ok()
                    .flatten();
                    DIAGRAMS_PENDING.with(|p| p.set(p.get() - 1));
                    if generation.get() != mine {
                        return;
                    }
                    if let (Some(obj), Some(pixels)) = (weak.upgrade(), pixels) {
                        obj.imp().texture.replace(Some(pixels.texture()));
                        obj.invalidate_contents();
                    }
                });
            });
            render();
            // テーマが変わったら、スタイルが決め直された後（次のひま）で色を読み直す。部品が画面に出たとき
            // （スタイルが決まったとき）にも読み直す。
            let later = {
                let render = render.clone();
                move || {
                    let render = render.clone();
                    glib::idle_add_local_once(move || render());
                }
            };
            let mut watches: Vec<(glib::WeakRef<glib::Object>, glib::SignalHandlerId)> = Vec::new();
            let style = adw::StyleManager::default();
            for property in ["dark", "high-contrast"] {
                let later = later.clone();
                let id = style.connect_notify_local(Some(property), move |_, _| later());
                watches.push((style.upcast_ref::<glib::Object>().downgrade(), id));
            }
            if let Some(settings) = gtk::Settings::default() {
                for property in ["gtk-theme-name", "gtk-application-prefer-dark-theme"] {
                    let later = later.clone();
                    let id = settings.connect_notify_local(Some(property), move |_, _| later());
                    watches.push((settings.upcast_ref::<glib::Object>().downgrade(), id));
                }
            }
            let id = view.connect_map(move |_| later());
            watches.push((view.upcast_ref::<glib::Object>().downgrade(), id));
            obj.imp().watches.replace(watches);
            obj
        }
    }
}

/// 字の大きさ `size`（pt）で描く数式の大きさ（画素）と、基線から下の深さ（画素）。
fn formula_size(math: &amonhen_core::math::MathPicture, size: f64) -> (i32, i32, f64) {
    let em = size * PX_PER_PT * crate::theme::math_scale();
    (
        (math.width * em).ceil() as i32,
        ((math.height + math.depth) * em).ceil() as i32,
        math.depth * em,
    )
}

/// `blocks` の中の画像の場所（重ならずに、出てくる順）。
pub fn image_sources(blocks: &[Block]) -> Vec<String> {
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
                Block::HtmlTable { cells, .. } => {
                    for cell in cells {
                        runs(&cell.runs, out);
                    }
                }
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
                Block::Code { .. } | Block::Diagram { .. } | Block::Math { .. } | Block::Rule => {}
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

/// `blocks` を、`<details>` の番号の順（入れ子は親の後）にたどる（macOS 版の `walk`）。
fn walk(blocks: &[Block], visit: &mut impl FnMut(&Block)) {
    for block in blocks {
        visit(block);
        match block {
            Block::Quote { blocks, .. }
            | Block::Footnote { blocks, .. }
            | Block::Details { blocks, .. } => walk(blocks, visit),
            Block::List { items, .. } => {
                for item in items {
                    walk(&item.blocks, visit);
                }
            }
            _ => {}
        }
    }
}

/// 最初から開いている `<details>`（`open` の付いたもの）の番号。
fn initially_open(blocks: &[Block]) -> HashSet<usize> {
    let mut index = 0;
    let mut open = HashSet::new();
    walk(blocks, &mut |block| {
        if let Block::Details { open: o, .. } = block {
            if *o {
                open.insert(index);
            }
            index += 1;
        }
    });
    open
}

/// `blocks` の中の `<details>` の数。
fn count_details(blocks: &[Block]) -> usize {
    let mut count = 0;
    walk(blocks, &mut |block| {
        if matches!(block, Block::Details { .. }) {
            count += 1;
        }
    });
    count
}

/// 外観で選ぶ画像（`<picture>`）があるか。
fn has_pictures(blocks: &[Block]) -> bool {
    let mut found = false;
    let mut check = |runs: &[Run]| {
        found |= runs
            .iter()
            .any(|r| r.image_dark.is_some() || r.image_light.is_some());
    };
    walk(blocks, &mut |block| match block {
        Block::Heading { runs, .. }
        | Block::Paragraph { runs, .. }
        | Block::Details { summary: runs, .. } => check(runs),
        Block::Table { head, rows, .. } => {
            for cell in head.iter().chain(rows.iter().flatten()) {
                check(cell);
            }
        }
        Block::HtmlTable { cells, .. } => {
            for cell in cells {
                check(&cell.runs);
            }
        }
        _ => {}
    });
    found
}

/// 見出しのない `<details>` の見出し（GitHub と同じ「Details」。訳の鍵は「HTML details: 」を付けたもの）。
fn details_title() -> String {
    let title = tr!("HTML details: Details");
    title
        .strip_prefix("HTML details: ")
        .map_or(title.clone(), str::to_owned)
}

/// HTML の `align` の、文字のそろえ方の飾りの名前と値。
fn justification(align: Align) -> Option<(&'static str, gtk::Justification)> {
    match align {
        Align::Center => Some(("justify-center", gtk::Justification::Center)),
        Align::Right => Some(("justify-right", gtk::Justification::Right)),
        _ => None,
    }
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
         .md-head { background-color: mix(@view_bg_color, @view_fg_color, 0.06); } \
         .md-kbd { font-family: monospace; font-size: 0.8em; padding: 0 4px; border-radius: 4px; \
           border: 1px solid alpha(currentColor, 0.35); background-color: alpha(currentColor, 0.05); }",
    );
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// 組み立てる元（`<details>` を開け閉めしたとき・外観が変わったときに組み直す）。
struct Doc {
    blocks: Vec<Block>,
    images: HashMap<String, gdk::Texture>,
    /// 開いている `<details>` の番号。
    open: RefCell<HashSet<usize>>,
    /// リンク（文字の範囲と先）。
    links: RefCell<Vec<(i32, i32, String)>>,
    /// 組み立てたときの外観（暗いか）。
    dark: Cell<bool>,
    /// 表と、その段落の左の下げ（欄の幅が変わったら広げ直す）。
    tables: RefCell<Vec<(glib::WeakRef<gtk::Grid>, i32)>>,
    /// 幅を割合で決めた絵（欄の幅が変わったら大きさを決め直す）。
    percent: RefCell<Vec<(glib::WeakRef<gtk::Picture>, PercentSize)>>,
    /// Markdown のファイル。
    from: String,
    on_open: OnOpenPath,
    /// 検索の帯（組み直したら探し直す）。
    find: RefCell<std::rc::Weak<crate::markdownfind::MarkdownFind>>,
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
    let doc = Rc::new(Doc {
        blocks: blocks.to_vec(),
        images: images.clone(),
        open: RefCell::new(initially_open(blocks)),
        links: RefCell::default(),
        dark: Cell::new(false),
        tables: RefCell::default(),
        percent: RefCell::default(),
        from: path.to_owned(),
        on_open,
        find: RefCell::default(),
    });
    build(&doc, &text);
    // リンクを押す（文字を選んでいるときは押したことにしない）。
    let click = gtk::GestureClick::new();
    {
        let (weak, doc) = (text.downgrade(), doc.clone());
        click.connect_released(move |_, _, x, y| {
            let Some(text) = weak.upgrade() else { return };
            if text.buffer().has_selection() {
                return;
            }
            let target = link_at(&text, &doc.links.borrow(), x, y);
            if let Some(target) = target {
                activate(&doc, &text, &target);
            }
        });
    }
    text.add_controller(click);
    // リンクの上では指のカーソル。
    let motion = gtk::EventControllerMotion::new();
    {
        let (weak, doc) = (text.downgrade(), doc.clone());
        motion.connect_motion(move |_, x, y| {
            let Some(text) = weak.upgrade() else { return };
            let over = link_at(&text, &doc.links.borrow(), x, y).is_some();
            text.set_cursor_from_name(Some(if over { "pointer" } else { "text" }));
        });
    }
    text.add_controller(motion);
    // `<picture>` の画像は外観で選ぶので、外観が変わったら組み直す。
    if has_pictures(blocks) {
        let style = adw::StyleManager::default();
        let (weak, doc) = (text.downgrade(), doc.clone());
        let id = style.connect_dark_notify(move |style| {
            if let Some(text) = weak.upgrade()
                && style.is_dark() != doc.dark.get()
            {
                rebuild(&doc, &text);
            }
        });
        let id = RefCell::new(Some(id));
        text.connect_destroy(move |_| {
            if let Some(id) = id.take() {
                adw::StyleManager::default().disconnect(id);
            }
        });
    }
    let scrolled = gtk::ScrolledWindow::builder()
        .child(&text)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();
    // 欄の幅が変わったら、表を広げ直す（横の adjustment の page-size は欄の幅）。
    {
        let (weak, doc) = (text.downgrade(), doc.clone());
        scrolled.hadjustment().connect_page_size_notify(move |_| {
            // 割り当ての最中に大きさを決め直さないよう、後で。
            let (weak, doc) = (weak.clone(), doc.clone());
            glib::idle_add_local_once(move || {
                if let Some(text) = weak.upgrade() {
                    fit_tables(&doc, &text);
                }
            });
        });
    }
    // 検索の帯（差分の帯と同じく下に）。
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.set_vexpand(true);
    root.append(&scrolled);
    let find = crate::markdownfind::MarkdownFind::new(&text, &root);
    root.append(find.bar());
    doc.find.replace(Rc::downgrade(&find));
    // 帯は整形表示のある間だけ持つ。
    let keep = RefCell::new(Some(find));
    root.connect_destroy(move |_| drop(keep.take()));
    root.upcast()
}

/// 文字を組み立てる（前のものは消す）。
fn build(doc: &Rc<Doc>, text: &gtk::TextView) {
    let dark = adw::StyleManager::default().is_dark();
    doc.dark.set(dark);
    let buffer = text.buffer();
    buffer.set_text("");
    let open = doc.open.borrow().clone();
    let builder = Builder::new(text, &doc.images, dark, open);
    builder.blocks(&doc.blocks, &Ctx::default());
    // 末尾の余分な改行を落とす。
    let mut end = buffer.end_iter();
    let mut before = end;
    if before.backward_char() && before.char() == '\n' {
        buffer.delete(&mut before, &mut end);
    }
    *doc.links.borrow_mut() = builder.links.take();
    *doc.tables.borrow_mut() = builder
        .tables
        .take()
        .into_iter()
        .map(|(grid, margin)| (grid.downgrade(), margin))
        .collect();
    *doc.percent.borrow_mut() = builder
        .percent
        .take()
        .into_iter()
        .map(|(picture, size)| (picture.downgrade(), size))
        .collect();
    fit_tables(doc, text);
    if let Some(find) = doc.find.borrow().upgrade() {
        find.refresh();
    }
    // 表のマスの中のリンク（ラベルの <a>）。
    for label in builder.labels.take() {
        let (weak, doc) = (text.downgrade(), doc.clone());
        label.connect_activate_link(move |_, uri| {
            if let Some(text) = weak.upgrade() {
                activate(&doc, &text, uri);
            }
            glib::Propagation::Stop
        });
    }
    // リンクの中の絵（バッジなど）。
    for (widget, link) in builder.linked.take() {
        widget.set_cursor_from_name(Some("pointer"));
        widget.set_tooltip_text(Some(&link));
        let click = gtk::GestureClick::new();
        let (weak, doc) = (text.downgrade(), doc.clone());
        click.connect_released(move |_, _, _, _| {
            if let Some(text) = weak.upgrade() {
                activate(&doc, &text, &link);
            }
        });
        widget.add_controller(click);
    }
}

/// 表を欄の幅に広げる（macOS 版の幅 100% の表と同じ。列は収まれば 1 行で、収まらないときだけ折り返す）。
/// 文字の間に埋め込んだ部品は最小の大きさで置かれ、折り返すラベルは 1 語の幅まで縮むので、幅を決めておく。
/// 幅を割合で決めた絵も、欄の幅に合わせて大きさを決め直す。
fn fit_tables(doc: &Doc, text: &gtk::TextView) {
    let width = text.width();
    if width <= 0 {
        return;
    }
    for (picture, size) in doc.percent.borrow().iter() {
        if let Some(picture) = picture.upgrade() {
            let (w, h) = size.at(width);
            if picture.width_request() != w || picture.height_request() != h {
                picture.set_size_request(w, h);
            }
        }
    }
    for (grid, margin) in doc.tables.borrow().iter() {
        if let Some(grid) = grid.upgrade() {
            // 左右の余白（24 と 24）と段落の下げを除く。
            let available = (width - 48 - margin - 2).max(0);
            if grid.width_request() != available {
                grid.set_size_request(available, -1);
            }
        }
    }
}

/// 組み直す（スクロールの位置は保つ）。
fn rebuild(doc: &Rc<Doc>, text: &gtk::TextView) {
    let adjustment = text.vadjustment();
    let value = adjustment.as_ref().map(|a| a.value());
    build(doc, text);
    if let (Some(adjustment), Some(value)) = (adjustment, value) {
        // 組み直した文字の高さが決まってから戻す。
        glib::idle_add_local_once(move || adjustment.set_value(value));
    }
}

/// リンクを押した: `<details>` の見出しなら開く・閉じる、ほかは `open_link`。
fn activate(doc: &Rc<Doc>, text: &gtk::TextView, target: &str) {
    if let Some(number) = target
        .strip_prefix(DETAILS_LINK)
        .and_then(|n| n.parse::<usize>().ok())
    {
        {
            let mut open = doc.open.borrow_mut();
            if !open.remove(&number) {
                open.insert(number);
            }
        }
        // 押している最中の部品を消さないよう、後で組み直す。
        let (weak, doc) = (text.downgrade(), doc.clone());
        glib::idle_add_local_once(move || {
            if let Some(text) = weak.upgrade() {
                rebuild(&doc, &text);
            }
        });
        return;
    }
    let window = text.root().and_downcast::<gtk::Window>();
    open_link(target, &doc.from, window.as_ref(), &doc.on_open);
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

/// 幅を割合で決めた絵（HTML の `width="50%"`）の大きさの決め方。
#[derive(Clone, Copy)]
struct PercentSize {
    /// 段落の左の下げ。
    margin: i32,
    /// 表のマスの中なら、欄の幅に対するマスの幅の割合。
    share: Option<f64>,
    /// 幅の割合（1〜100）。
    percent: u32,
    /// HTML の高さ（なければ比を保つ）。
    height: Option<u32>,
    /// 絵の大きさ。
    natural: (i32, i32),
}

impl PercentSize {
    /// 欄の幅 `width` のときの大きさ（幅は 640 px まで、高さは決まっていなければ比を保つ）。
    fn at(&self, width: i32) -> (i32, i32) {
        // 左右の余白（24 と 24）と段落の下げを除く。マスの中なら、そのマスの分からマスの左右の余白（8 と 8）と枠を除く。
        let column = f64::from((width - 48 - self.margin - 2).max(0));
        let base = self
            .share
            .map_or(column, |share| (column * share - 18.0).max(0.0));
        let w = ((base * f64::from(self.percent) / 100.0).round() as i32).max(1);
        let (tw, th) = (self.natural.0.max(1), self.natural.1.max(1));
        let h = self.height.map_or(th * w / tw, |h| h as i32);
        if w > MAX_IMAGE_WIDTH {
            (MAX_IMAGE_WIDTH, h * MAX_IMAGE_WIDTH / w)
        } else {
            (w, h)
        }
    }
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
    /// リンクの中の絵・キーの札（押せるように）と、その先。
    linked: RefCell<Vec<(gtk::Widget, String)>>,
    /// 表（欄の幅に広げる）と、その段落の左の下げ。
    tables: RefCell<Vec<(gtk::Grid, i32)>>,
    /// 幅を割合で決めた絵（欄の幅に合わせる）。
    percent: RefCell<Vec<(gtk::Picture, PercentSize)>>,
    /// 今のブロックの段落の左の下げ（割合の幅の絵の基準）。
    margin: Cell<i32>,
    /// 暗い外観か（`<picture>` の画像を選ぶ）。
    dark: bool,
    /// 開いている `<details>` の番号と、次の `<details>` の番号（閉じたものの中のものも数える）。
    open: HashSet<usize>,
    details: Cell<usize>,
}

impl<'a> Builder<'a> {
    fn new(
        view: &gtk::TextView,
        images: &'a HashMap<String, gdk::Texture>,
        dark: bool,
        open: HashSet<usize>,
    ) -> Self {
        Self {
            view: view.clone(),
            buffer: view.buffer(),
            images,
            links: RefCell::default(),
            labels: RefCell::default(),
            linked: RefCell::default(),
            tables: RefCell::default(),
            percent: RefCell::default(),
            margin: Cell::new(0),
            dark,
            open,
            details: Cell::new(0),
        }
    }

    /// 文字の間に部品を埋め込む。
    fn embed(&self, widget: &impl IsA<gtk::Widget>) {
        let mut end = self.buffer.end_iter();
        let anchor = self.buffer.create_child_anchor(&mut end);
        self.view.add_child_at_anchor(widget, &anchor);
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

    /// 段落の形（左の下げ・ぶら下げ・引用の背景・下の間隔）を、`start` から終わりまでに付ける。`hanging` はぶら下げる
    /// 印の数（入れ子のリストの最初の項目は、外の項目の印と同じ行に並べる）。
    fn paragraph(&self, start: i32, ctx: &Ctx, hanging: usize, gap: i32) {
        let margin = ctx.margin;
        let levels = i32::try_from(hanging).unwrap_or(1);
        let indent = -STEP * levels;
        // 1 行目は左の余白から始まり、2 行目からが下がる。外の印の分だけ余白を戻して、外の項目の印の位置から始める。
        let back = STEP * (levels - 1).max(0);
        let shape = self.tag(&format!("shape-{margin}-{indent}"), |t| {
            t.set_left_margin(24 + margin - back);
            t.set_indent(indent);
            if levels > 0 {
                let mut tabs = pango::TabArray::new(levels, true);
                for level in 0..levels {
                    tabs.set_tab(level, pango::TabAlign::Left, STEP * (level + 1));
                }
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

    /// リストの印があれば、段落の頭に置く（印はぶら下げる）。置いた印の数（入れ子の外の印も並べたら 2 つ以上）。
    fn marker(&self, ctx: &Ctx) -> usize {
        let Some(marker) = ctx.marker.borrow_mut().take() else {
            return 0;
        };
        let dim = self.dim();
        self.insert(&format!("{marker}\t"), &[dim]);
        marker.matches('\t').count() + 1
    }

    /// 飾りの文字。`size` は pt、`bold` なら見出し・表の頭。
    fn runs(&self, runs: &[Run], size: f64, bold: bool) {
        for run in runs {
            if run.line_break {
                self.insert("\u{2028}", &[]);
                continue;
            }
            if let Some(src) = image_for(run, self.dark) {
                self.image(src, run);
                continue;
            }
            if let Some(math) = &run.math {
                self.inline_formula(math, size, run);
                continue;
            }
            if run.kbd {
                // キーは枠付きの札（macOS 版と同じ）。
                install_css();
                let label = gtk::Label::new(Some(&run.text));
                label.add_css_class("md-kbd");
                label.set_tooltip_text(Some(&run.text));
                if let Some(link) = &run.link {
                    self.linked
                        .borrow_mut()
                        .push((label.clone().upcast(), link.clone()));
                }
                self.embed(&label);
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
            if run.underline {
                tags.push(self.tag("underline", |t| t.set_underline(pango::Underline::Single)));
            }
            // 下付き・上付きは小さく、基線から上げ下げする（macOS 版と同じ割合）。
            if run.sub {
                tags.push(self.tag(&format!("sub-{size}"), |t| {
                    t.set_scale(0.75);
                    t.set_rise((-size * 0.2 * pango::SCALE as f64) as i32);
                }));
            }
            if run.sup {
                tags.push(self.tag(&format!("sup-{size}"), |t| {
                    t.set_scale(0.75);
                    t.set_rise((size * 0.35 * pango::SCALE as f64) as i32);
                }));
            }
            if run.code {
                tags.push(self.tag("mono", |t| {
                    t.set_family(Some("monospace"));
                    t.set_scale(0.9);
                }));
            }
            if run.code {
                tags.push(self.tag("code", |t| t.set_background_rgba(Some(&SHADE))));
            }
            if run.math_error.is_some() {
                // 描けなかった文中の数式は、原文をコードとして赤く
                tags.push(self.tag("math-error", |t| {
                    t.set_foreground_rgba(Some(&gdk::RGBA::new(0.85, 0.2, 0.2, 1.0)));
                }));
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

    /// 文中の数式: 文字として埋め込み、深さの分だけ基線より下げる（`rise`）。
    fn inline_formula(&self, math: &amonhen_core::math::MathPicture, size: f64, run: &Run) {
        let (w, h, depth) = formula_size(math, size);
        let scale = self.view.scale_factor().max(2);
        let paintable = formula::Formula::new(&math.svg, w, h, scale, &self.view);
        let rise = -(depth * pango::SCALE as f64).round() as i32;
        let lower = self.tag(&format!("rise{rise}"), |t| t.set_rise(rise));
        let start = self.offset();
        let mut end = self.buffer.end_iter();
        self.buffer.insert_paintable(&mut end, &paintable);
        self.apply(start, &[lower]);
        if let Some(link) = &run.link {
            self.links
                .borrow_mut()
                .push((start, self.offset(), link.clone()));
        }
    }

    /// 画像: このコミットの中で読めたものは絵（HTML の大きさがあればその大きさで、幅は 640 px まで）、ほか（外の
    /// URL など）は代わりの文を薄く。リンクの中の絵は押せる。
    fn image(&self, src: &str, run: &Run) {
        let alt = run.text.as_str();
        match self.images.get(src) {
            Some(texture) => {
                let (w, h) = image_size(texture, run.width, run.height);
                let (w, h) = if w > MAX_IMAGE_WIDTH {
                    (MAX_IMAGE_WIDTH, h * MAX_IMAGE_WIDTH / w.max(1))
                } else {
                    (w, h)
                };
                let picture = gtk::Picture::for_paintable(texture);
                picture.set_can_shrink(true);
                picture.set_content_fit(gtk::ContentFit::Contain);
                picture.set_size_request(w, h);
                self.percent_width(&picture, texture, run, None);
                if !alt.is_empty() {
                    picture.set_alternative_text(Some(alt));
                    picture.set_tooltip_text(Some(alt));
                }
                if let Some(link) = &run.link {
                    self.linked
                        .borrow_mut()
                        .push((picture.clone().upcast(), link.clone()));
                }
                self.embed(&picture);
            }
            None => {
                let dim = self.dim();
                let start = self.offset();
                self.insert(
                    &format!("[{}]", if alt.is_empty() { src } else { alt }),
                    &[dim],
                );
                if let Some(link) = &run.link {
                    self.links
                        .borrow_mut()
                        .push((start, self.offset(), link.clone()));
                }
            }
        }
    }

    /// 段落・見出しを `align` でそろえる（`start` から終わりまで）。
    fn justify(&self, start: i32, align: Align) {
        if let Some((name, value)) = justification(align) {
            let tag = self.tag(name, |t| t.set_justification(value));
            self.apply_range(start, &tag);
        }
    }

    fn blocks(&self, blocks: &[Block], ctx: &Ctx) {
        for block in blocks {
            self.block(block, ctx);
        }
    }

    fn block(&self, block: &Block, ctx: &Ctx) {
        self.margin.set(ctx.margin);
        match block {
            Block::Heading { level, runs, align } => {
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
                self.justify(start, *align);
            }
            Block::Paragraph { runs, align } => {
                let start = self.offset();
                let hanging = self.marker(ctx);
                self.runs(runs, BODY, false);
                self.insert("\n", &[]);
                let gap = if ctx.marker_list() { 4 } else { 10 };
                self.paragraph(start, ctx, hanging, gap);
                self.justify(start, *align);
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
                self.paragraph(start, ctx, 0, 12);
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
                        self.paragraph(start, ctx, 0, 10);
                    }
                    return;
                };
                self.lone_marker(ctx);
                let start = self.offset();
                let mut end = self.buffer.end_iter();
                let anchor = self.buffer.create_child_anchor(&mut end);
                self.view.add_child_at_anchor(&picture, &anchor);
                self.insert("\n", &[]);
                self.paragraph(start, ctx, 0, 12);
                // 近いもので描いた図には、そのことを絵の下に添える
                if let Some(note) = note {
                    let start = self.offset();
                    let dim = self.tag("diagram-note", |t| {
                        t.set_foreground_rgba(Some(&gdk::RGBA::new(0.5, 0.5, 0.5, 1.0)));
                        t.set_scale(0.9);
                    });
                    self.insert(note, &[dim]);
                    self.insert("\n", &[]);
                    self.paragraph(start, ctx, 0, 10);
                }
            }
            Block::Math { text, math, error } => {
                let Some(math) = math else {
                    // 描けない数式（数式を描かない作りも）は TeX をコードとして見せ、理由を添える
                    self.block(
                        &Block::Code {
                            language: "math".into(),
                            text: text.clone(),
                        },
                        ctx,
                    );
                    if let Some(error) = error {
                        let start = self.offset();
                        let red = self.tag("diagram-error", |t| {
                            t.set_foreground_rgba(Some(&gdk::RGBA::new(0.85, 0.2, 0.2, 1.0)));
                            t.set_family(Some("monospace"));
                            t.set_scale(0.9);
                        });
                        self.insert(&error.replace('\n', "\u{2028}"), &[red]);
                        self.insert("\n", &[]);
                        self.paragraph(start, ctx, 0, 10);
                    }
                    return;
                };
                // 数式のブロックは中央に（幅は 640 px まで、比を保って縮める）
                self.lone_marker(ctx);
                let (w, h, _) = formula_size(math, BODY);
                let (shown_w, shown_h) = if w > MAX_IMAGE_WIDTH {
                    (MAX_IMAGE_WIDTH, h * MAX_IMAGE_WIDTH / w.max(1))
                } else {
                    (w, h)
                };
                let scale = self.view.scale_factor().max(2);
                let picture = gtk::Picture::for_paintable(&formula::Formula::new(
                    &math.svg, w, h, scale, &self.view,
                ));
                picture.set_can_shrink(true);
                picture.set_content_fit(gtk::ContentFit::Contain);
                picture.set_size_request(shown_w, shown_h);
                picture.set_alternative_text(Some(text));
                picture.set_tooltip_text(Some(text));
                let start = self.offset();
                self.embed(&picture);
                self.insert("\n", &[]);
                self.paragraph(start, ctx, 0, 12);
                self.justify(start, Align::Center);
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
                    self.paragraph(start, &inner, 0, 4);
                }
                self.blocks(blocks, &inner);
            }
            Block::List { start, items } => {
                // 項目の最初が入れ子のリストなら、外の印は最初の項目の印と同じ行に並べる（GitHub と同じ）。
                let mut outer = ctx.marker.borrow_mut().take();
                let depth = (ctx.margin / STEP) as usize;
                for (i, item) in items.iter().enumerate() {
                    let marker = match (item.task, start) {
                        (Some(true), _) => "☑".to_owned(),
                        (Some(false), _) => "☐".to_owned(),
                        (None, Some(n)) => format!("{}.", n + i as u64),
                        (None, None) => ["•", "◦", "▪\u{fe0e}"][depth % 3].to_owned(),
                    };
                    let marker = match outer.take() {
                        Some(outer) => format!("{outer}\t{marker}"),
                        None => marker,
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
                self.paragraph(start, ctx, 0, 12);
            }
            Block::Details {
                summary, blocks, ..
            } => {
                self.lone_marker(ctx);
                let index = self.details.get();
                self.details.set(index + 1);
                let open = self.open.contains(&index);
                // 開閉の三角と見出し。押すと開く・閉じる（見出しの中のリンクはそのまま。macOS 版と同じ）。
                let start = self.offset();
                let triangle = self.tag("details-triangle", |t| {
                    t.set_scale(0.7);
                    t.set_foreground_rgba(Some(&DIM));
                    t.set_rise((1.5 * pango::SCALE as f64) as i32);
                });
                self.insert(if open { "▼" } else { "▶" }, &[triangle]);
                self.insert("\u{2009}\u{2009}", &[]);
                if summary.is_empty() {
                    let size = self.tag(&format!("size-{BODY}"), |t| t.set_size_points(BODY));
                    self.insert(&details_title(), &[size]);
                } else {
                    self.runs(summary, BODY, false);
                }
                // 見出しの中のリンクが先に当たるよう、後ろに足す。
                self.links.borrow_mut().push((
                    start,
                    self.offset(),
                    format!("{DETAILS_LINK}{index}"),
                ));
                self.insert("\n", &[]);
                self.paragraph(start, ctx, 0, if open { 6 } else { 10 });
                if open {
                    self.blocks(blocks, ctx);
                } else {
                    self.details.set(self.details.get() + count_details(blocks));
                }
            }
            Block::HtmlTable { cells, columns, .. } => {
                self.lone_marker(ctx);
                self.html_table(cells, *columns, ctx);
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
                self.paragraph(start, ctx, 0, 2);
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
        let hanging = self.marker(ctx);
        if hanging > 0 {
            self.insert("\n", &[]);
            self.paragraph(start, ctx, hanging, 2);
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
                let cell = self.cell(
                    row.get(column).map_or(&[][..], Vec::as_slice),
                    r == 0,
                    aligns.get(column).copied().unwrap_or(Align::None),
                    1.0 / columns as f64,
                );
                grid.attach(&cell, column as i32, r as i32, 1, 1);
            }
        }
        let start = self.offset();
        self.tables.borrow_mut().push((grid.clone(), ctx.margin));
        self.embed(&grid);
        self.insert("\n", &[]);
        self.paragraph(start, ctx, 0, 10);
    }

    /// HTML の表（マスは `colspan`・`rowspan` をまたぐ。見出しのマスは、決めなければ中央）。
    fn html_table(&self, cells: &[amonhen_core::markdown::TableCell], columns: u32, ctx: &Ctx) {
        install_css();
        let grid = gtk::Grid::new();
        grid.add_css_class("md-table");
        grid.set_row_spacing(1);
        grid.set_column_spacing(1);
        for cell in cells {
            let align = match (cell.align, cell.header) {
                (Align::None, true) => Align::Center,
                (align, _) => align,
            };
            let widget = self.cell(
                &cell.runs,
                cell.header,
                align,
                f64::from(cell.colspan.max(1)) / f64::from(columns.max(1)),
            );
            grid.attach(
                &widget,
                cell.column as i32,
                cell.row as i32,
                cell.colspan.max(1) as i32,
                cell.rowspan.max(1) as i32,
            );
        }
        let start = self.offset();
        self.tables.borrow_mut().push((grid.clone(), ctx.margin));
        self.embed(&grid);
        self.insert("\n", &[]);
        self.paragraph(start, ctx, 0, 10);
    }

    /// 幅を割合で決めた絵（HTML の `width="50%"`）なら、欄の幅（表のマスの中なら、欄の幅の `share` の分）に合わせる
    /// ように覚えておく（大きさは `fit_tables` で決める）。
    fn percent_width(
        &self,
        picture: &gtk::Picture,
        texture: &gdk::Texture,
        run: &Run,
        share: Option<f64>,
    ) {
        let Some(percent) = run.width_percent else {
            return;
        };
        let size = PercentSize {
            margin: self.margin.get(),
            share,
            percent,
            height: run.height,
            natural: (texture.width(), texture.height()),
        };
        let width = self.view.width();
        let (w, h) = size.at(if width > 0 {
            width
        } else {
            MAX_IMAGE_WIDTH + 50
        });
        picture.set_size_request(w, h);
        self.percent.borrow_mut().push((picture.clone(), size));
    }

    /// 表のマス: 文字はラベル（Pango のマークアップ）、画像はこのコミットの中で読めたものなら絵。改行で行を分ける。
    /// `share` は欄の幅に対するマスの幅の割合（割合の幅の絵の基準）。
    fn cell(&self, runs: &[Run], head: bool, align: Align, share: f64) -> gtk::Widget {
        let (xalign, justify, halign) = match align {
            Align::Center => (0.5, gtk::Justification::Center, gtk::Align::Center),
            Align::Right => (1.0, gtk::Justification::Right, gtk::Align::End),
            _ => (0.0, gtk::Justification::Left, gtk::Align::Start),
        };
        let label = |runs: &[Run]| {
            let label = gtk::Label::new(None);
            label.set_use_markup(true);
            label.set_selectable(true);
            label.set_wrap(true);
            label.set_max_width_chars(60);
            label.set_xalign(xalign);
            label.set_justify(justify);
            label.set_markup(&markup(runs, head));
            self.labels.borrow_mut().push(label.clone());
            label
        };
        let has_image = runs.iter().any(|r| {
            r.math.is_some()
                || image_for(r, self.dark).is_some_and(|src| self.images.contains_key(src))
        });
        let outer = gtk::Box::new(gtk::Orientation::Vertical, 2);
        outer.add_css_class("md-cell");
        if head {
            outer.add_css_class("md-head");
        }
        outer.set_hexpand(true);
        if !has_image {
            let l = label(runs);
            l.set_hexpand(true);
            outer.append(&l);
            return outer.upcast();
        }
        // 絵のあるマス: 行ごとに、文字のかたまりと絵を横に並べる。
        for line in runs.split(|r| r.line_break) {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
            row.set_halign(halign);
            let mut text: Vec<Run> = Vec::new();
            for run in line {
                if let Some(math) = &run.math {
                    // マスの中の数式は、文字のかたまりの間に絵として並べる
                    if !text.is_empty() {
                        row.append(&label(&std::mem::take(&mut text)));
                    }
                    let (w, h, _) = formula_size(math, BODY - 1.0);
                    let scale = self.view.scale_factor().max(2);
                    let picture = gtk::Picture::for_paintable(&formula::Formula::new(
                        &math.svg, w, h, scale, &self.view,
                    ));
                    picture.set_size_request(w, h);
                    picture.set_valign(gtk::Align::Center);
                    picture.set_tooltip_text(Some(&run.text));
                    row.append(&picture);
                    continue;
                }
                let texture = image_for(run, self.dark).and_then(|src| self.images.get(src));
                match texture {
                    Some(texture) => {
                        if !text.is_empty() {
                            row.append(&label(&std::mem::take(&mut text)));
                        }
                        let (w, h) = image_size(texture, run.width, run.height);
                        let picture = gtk::Picture::for_paintable(texture);
                        picture.set_can_shrink(true);
                        picture.set_content_fit(gtk::ContentFit::Contain);
                        picture.set_size_request(
                            w.min(MAX_IMAGE_WIDTH),
                            h * w.min(MAX_IMAGE_WIDTH) / w.max(1),
                        );
                        self.percent_width(&picture, texture, run, Some(share));
                        picture.set_valign(gtk::Align::Center);
                        if !run.text.is_empty() {
                            picture.set_tooltip_text(Some(&run.text));
                        }
                        if let Some(link) = &run.link {
                            self.linked
                                .borrow_mut()
                                .push((picture.clone().upcast(), link.clone()));
                        }
                        row.append(&picture);
                    }
                    None => text.push(run.clone()),
                }
            }
            if !text.is_empty() {
                row.append(&label(&text));
            }
            outer.append(&row);
        }
        outer.upcast()
    }
}

impl Ctx {
    /// リストの項目の中か（段落の下の間隔を狭くする）。
    fn marker_list(&self) -> bool {
        self.margin >= STEP && self.quote.is_none()
    }
}

/// 外観 `dark` で見せる画像（`<picture>` の外観に合うもの、なければ `image`）。
fn image_for(run: &Run, dark: bool) -> Option<&str> {
    let chosen = if dark {
        &run.image_dark
    } else {
        &run.image_light
    };
    chosen.as_deref().or(run.image.as_deref())
}

/// 絵を見せる大きさ: HTML の `width`・`height` があればそれ（片方だけなら比を保つ）、なければ絵の大きさ。
fn image_size(texture: &gdk::Texture, width: Option<u32>, height: Option<u32>) -> (i32, i32) {
    let (tw, th) = (texture.width().max(1), texture.height().max(1));
    match (width, height) {
        (Some(w), Some(h)) => (w as i32, h as i32),
        (Some(w), None) => (w as i32, th * w as i32 / tw),
        (None, Some(h)) => (tw * h as i32 / th, h as i32),
        (None, None) => (tw, th),
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
        if run.kbd {
            // マスの中のキーは、等幅に薄い背景（ラベルの中に枠は描けない）。
            out += &format!(
                "<span font_family=\"monospace\" size=\"smaller\" bgcolor=\"#808080\" bgalpha=\"18%\"> {} </span>",
                esc(&run.text)
            );
            continue;
        }
        let mut text = esc(&run.text);
        if let Some(note) = &run.footnote {
            text = format!("<sup>[{}]</sup>", esc(note));
        }
        if run.code {
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
        if run.underline {
            text = format!("<u>{text}</u>");
        }
        if run.sub {
            text = format!("<sub>{text}</sub>");
        }
        if run.sup {
            text = format!("<sup>{text}</sup>");
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
    fn numbers_details_and_finds_picture_sources() {
        let blocks = amonhen_core::markdown::parse(
            "<details open><summary>a</summary>\n\n<details><summary>b</summary>\n\nx\n\n</details>\n\n</details>\n\n\
             - <details open><summary>c</summary>y</details>\n\n\
             <picture><source media=\"(prefers-color-scheme: dark)\" srcset=\"d.png\"><img src=\"l.png\"></picture>\n",
        );
        assert_eq!(initially_open(&blocks), HashSet::from([0, 2]));
        assert_eq!(count_details(&blocks), 3);
        assert!(has_pictures(&blocks));
        assert_eq!(image_sources(&blocks), ["l.png", "d.png"]);
        let Some(Block::Paragraph { runs, .. }) = blocks.last() else {
            panic!("{blocks:?}")
        };
        assert_eq!(image_for(&runs[0], true), Some("d.png"));
        assert_eq!(image_for(&runs[0], false), Some("l.png"));
    }

    #[test]
    fn maps_fence_languages_to_extensions() {
        assert_eq!(file_extension("Rust"), "rs");
        assert_eq!(file_extension("bash"), "sh");
        assert_eq!(file_extension("swift"), "swift");
    }
}
