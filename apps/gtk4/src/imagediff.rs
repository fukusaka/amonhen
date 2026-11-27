//! History の右の列の、画像の前と後の比較（macOS 版の `ImageDiffPanel`・`ImageCompareCanvas`）。変更の見出しの
//! 下に置く欄で、並べる・スワイプ・オニオンスキン・違うピクセルを切り替える。比べ方は窓を閉じるまで覚える
//! （次に開く画像も同じ比べ方）。SVG は「テキストで見る」で差分に戻せる。すべてのファイルで開いた画像も、同じ欄の
//! 1 枚だけの形で見せる。
//!
//! 拡大縮小: 帯のボタン・ピンチ・Ctrl＋スクロール。拡大している間は、ドラッグ（スワイプ・オニオンスキンでは
//! Alt を押しながら）かスクロールで絵を動かす。枠に合わせているときのスクロールは差分に回す。前と後は同じ倍率。

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::OnceLock;

use amonhen_model::tr;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, gio, glib, graphene, gsk, pango};

/// 画像として見せる拡張子: gdk-pixbuf が読める形式と、GTK が自分で読む PNG・JPEG・TIFF と SVG。PDF などの文書は
/// プレビューに任せる（macOS 版と同じ）。
fn extensions() -> &'static HashSet<String> {
    static SET: OnceLock<HashSet<String>> = OnceLock::new();
    SET.get_or_init(|| {
        let mut set: HashSet<String> = ["png", "jpg", "jpeg", "tif", "tiff", "svg"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        for format in gtk::gdk_pixbuf::Pixbuf::formats() {
            set.extend(format.extensions().iter().map(|e| e.to_ascii_lowercase()));
        }
        for doc in ["pdf", "ai", "eps"] {
            set.remove(doc);
        }
        set
    })
}

fn extension(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

pub fn is_image(path: &str) -> bool {
    extensions().contains(&extension(path))
}

pub fn is_svg(path: &str) -> bool {
    extension(path) == "svg"
}

/// 中身が SVG か（頭の 1 KB に `<svg` があるか）。
fn looks_like_svg(data: &[u8]) -> bool {
    let head = &data[..data.len().min(1024)];
    head.windows(4).any(|w| w == b"<svg")
}

/// 比べ方。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    TwoUp,
    Swipe,
    Onion,
    Difference,
}

impl Mode {
    const ALL: [Mode; 4] = [Mode::TwoUp, Mode::Swipe, Mode::Onion, Mode::Difference];

    fn title(self) -> String {
        match self {
            Mode::TwoUp => tr!("2-Up"),
            Mode::Swipe => tr!("Swipe"),
            Mode::Onion => tr!("Onion Skin"),
            Mode::Difference => tr!("Difference"),
        }
    }

    fn icon(self) -> crate::icon::Kind {
        match self {
            Mode::TwoUp => crate::icon::Kind::TwoUp,
            Mode::Swipe => crate::icon::Kind::SwipeHalf,
            Mode::Onion => crate::icon::Kind::HalfCircle,
            Mode::Difference => crate::icon::Kind::PlusMinus,
        }
    }
}

/// 画像のバイト列を読む（読めなければ None。SVG は GdkPixbuf の読み手があれば読める）。
pub fn texture(data: Option<&[u8]>) -> Option<gdk::Texture> {
    gdk::Texture::from_bytes(&glib::Bytes::from(data?)).ok()
}

/// SVG の根の `width`・`height`（論理の画素）。書いてなければ None。
pub fn svg_size(svg: &str) -> Option<(i32, i32)> {
    let start = svg.find("<svg")?;
    let open = &svg[start..start + svg[start..].find('>')?];
    let attr = |name: &str| -> Option<i32> {
        let at = open.find(&format!(" {name}=\""))? + name.len() + 3;
        let value = &open[at..at + open[at..].find('"')?];
        value
            .trim_end_matches("px")
            .parse::<f64>()
            .ok()
            .map(|v| v.ceil() as i32)
    };
    Some((attr("width")?, attr("height")?))
}

/// 画素の並び（スレッドをまたいで渡せる。メインスレッドで `texture` にする）。
pub struct Pixels {
    bytes: glib::Bytes,
    width: i32,
    height: i32,
    stride: usize,
    alpha: bool,
}

impl Pixels {
    pub fn texture(&self) -> gdk::Texture {
        let format = if self.alpha {
            gdk::MemoryFormat::R8g8b8a8
        } else {
            gdk::MemoryFormat::R8g8b8
        };
        gdk::MemoryTexture::new(self.width, self.height, format, &self.bytes, self.stride).upcast()
    }
}

/// SVG を `width`×`height` の画素で読む（裏のスレッドで呼んでよい）。
pub fn svg_pixels(svg: &[u8], width: i32, height: i32) -> Option<Pixels> {
    let stream = gio::MemoryInputStream::from_bytes(&glib::Bytes::from(svg));
    let pixbuf = gtk::gdk_pixbuf::Pixbuf::from_stream_at_scale(
        &stream,
        width,
        height,
        true,
        gio::Cancellable::NONE,
    )
    .ok()?;
    Some(Pixels {
        bytes: pixbuf.read_pixel_bytes(),
        width: pixbuf.width(),
        height: pixbuf.height(),
        stride: pixbuf.rowstride() as usize,
        alpha: pixbuf.has_alpha(),
    })
}

/// 画像の大きさとバイト数（「64 × 48 px · 1.2 kB」）。
fn describe(data: Option<&[u8]>, image: Option<&gdk::Texture>) -> String {
    let Some(data) = data else {
        return tr!("None");
    };
    let bytes = glib::format_size(data.len() as u64).to_string();
    match image {
        Some(t) => format!("{} × {} px · {bytes}", t.width(), t.height()),
        None => bytes,
    }
}

/// 比べる欄を作る。`before`・`after` は前と後のバイト列（足した・消したファイルなら片方が None）。`svg` なら
/// 「テキストで見る」を出す（押すと `on_show_text`）。比べ方を変えたら `on_mode`。
pub fn panel(
    before: Option<&[u8]>,
    after: Option<&[u8]>,
    svg: bool,
    mode: Mode,
    on_mode: Rc<dyn Fn(Mode)>,
    on_show_text: Rc<dyn Fn()>,
) -> gtk::Widget {
    let (old, new) = (texture(before), texture(after));
    let caption = format!(
        "{}: {}    →    {}: {}",
        tr!("Before"),
        describe(before, old.as_ref()),
        tr!("After"),
        describe(after, new.as_ref())
    );
    build(
        old,
        new,
        caption,
        Some((mode, on_mode)),
        svg.then_some(on_show_text),
        None,
        false,
        None,
        &[],
        None,
    )
}

/// すべてのファイルで、2 つの絵を比べる欄（SVG のモバイル向けの確認: 元の絵と、部品が崩した近似）。`titles` は
/// 前と後の見出し。欄の高さいっぱいに広げ、`notes` があれば絵の下に置く。`leading` は帯の左端（比べ方の前）に
/// 置く部品（モバイルの部品の切り替え）。
pub fn compare(
    before: &[u8],
    after: &[u8],
    titles: (String, String),
    caption: String,
    notes: Option<gtk::Widget>,
    leading: Option<gtk::Widget>,
) -> gtk::Widget {
    let panel = build(
        texture(Some(before)),
        texture(Some(after)),
        caption,
        Some((Mode::TwoUp, Rc::new(|_| {}))),
        None,
        Some(titles),
        true,
        leading,
        &[],
        None,
    );
    with_notes(panel, notes)
}

/// 画像の大きさとバイト数（すべてのファイルの説明。読めなければバイト数だけ）。
pub fn size_caption(data: &[u8]) -> String {
    describe(Some(data), texture(Some(data)).as_ref())
}

/// すべてのファイルで開いた画像（比べずに 1 枚だけ。比べ方の切り替えもスライダーも出さない）。`notes` があれば
/// 絵の下に置く（SVG のモバイル向けの確認の一覧。欄の高さの 35% ほど、仕切りで変えられる）。読めなければ None。
pub fn single(data: &[u8], notes: Option<gtk::Widget>) -> Option<gtk::Widget> {
    let image = texture(Some(data))?;
    let caption = describe(Some(data), Some(&image));
    let vector = looks_like_svg(data).then(|| glib::Bytes::from(data));
    let panel = build(
        None,
        Some(image),
        caption,
        None,
        None,
        None,
        true,
        None,
        &[],
        vector,
    );
    Some(with_notes(panel, notes))
}

/// すべてのファイルで開いた Android の VectorDrawable（SVG にしたもの `svg`）。`caption` は大きさの説明、
/// `footnote` は絵の下に添える短い説明（引けなかった色など。Markdown の図の注記と同じ見た目）。読めなければ None。
pub fn drawable(svg: &[u8], caption: String, footnote: &[String]) -> Option<gtk::Widget> {
    let image = texture(Some(svg))?;
    Some(build(
        None,
        Some(image),
        caption,
        None,
        None,
        None,
        true,
        None,
        footnote,
        Some(glib::Bytes::from(svg)),
    ))
}

/// `notes` があれば、`panel` の下に一覧の欄を置く（欄の高さの 35% ほど、仕切りで変えられる）。
fn with_notes(panel: gtk::Widget, notes: Option<gtk::Widget>) -> gtk::Widget {
    let Some(notes) = notes else {
        return panel;
    };
    let scroll = gtk::ScrolledWindow::builder()
        .child(&notes)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();
    scroll.set_margin_start(12);
    scroll.set_margin_end(12);
    scroll.set_margin_bottom(6);
    let paned = gtk::Paned::new(gtk::Orientation::Vertical);
    paned.set_vexpand(true);
    paned.set_start_child(Some(&panel));
    paned.set_end_child(Some(&scroll));
    paned.set_shrink_end_child(false);
    // 最初に大きさが決まったら、一覧を下の 35% ほどに（macOS 版と同じ割合）。
    let placed = Cell::new(false);
    paned.connect_notify_local(Some("max-position"), move |p, _| {
        let max = p.max_position();
        if !placed.get() && max > 0 {
            placed.set(true);
            p.set_position((max as f32 * 0.65) as i32);
        }
    });
    paned.upcast()
}

/// 欄を作る。`modes` がなければ 1 枚だけ（`after` を出す）。`titles` は前と後の見出し（なければ「前」「後」）。
/// `fill` なら高さいっぱいに広げる（すべてのファイル。下の仕切りの線は引かない）。`leading` は帯の左端に置く。
/// `footnote` は大きさの説明の下に添える文。`vector` は 1 枚の絵が SVG のときの中身で、拡大したときにその大きさで
/// 描き直す（テクスチャを伸ばすとぼやける）。
#[allow(clippy::too_many_arguments)]
fn build(
    old: Option<gdk::Texture>,
    new: Option<gdk::Texture>,
    base_caption: String,
    modes: Option<(Mode, OnMode)>,
    on_show_text: Option<Rc<dyn Fn()>>,
    titles: Option<(String, String)>,
    fill: bool,
    leading: Option<gtk::Widget>,
    footnote: &[String],
    vector: Option<glib::Bytes>,
) -> gtk::Widget {
    let single = modes.is_none();
    // 市松模様の明暗: 既定は絵の色から（比べるときは両方を合わせて。確かめる用に `AMH_CHECKERBOARD=dark|light`）。
    // 開いている間はボタンが勝つ
    let tone = checker_default(&[old.as_ref(), new.as_ref()]);
    let dark = match std::env::var("AMH_CHECKERBOARD").as_deref() {
        Ok("dark") => true,
        Ok("light") => false,
        _ => tone.dark(),
    };
    let canvas = Canvas::new(old, new);
    canvas.imp().vector.replace(vector);
    canvas.imp().dark_checker.set(dark);
    if let Some(titles) = titles {
        canvas.imp().titles.replace(titles);
    }
    canvas.imp().single.set(single);
    canvas.set_vexpand(true);
    canvas.set_hexpand(true);
    // 確かめる用（`--snapshot`）: 最初の倍率（`AMH_IMAGE_ZOOM=4` で 400%。macOS 版の `AMHImageZoom`）。
    if let Some(zoom) = std::env::var("AMH_IMAGE_ZOOM")
        .ok()
        .and_then(|v| v.trim().parse::<f32>().ok())
        .filter(|z| *z > 0.0)
    {
        canvas.set_zoom(Some(zoom));
    }
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    if let Some(leading) = &leading {
        bar.append(leading);
    }
    let mut buttons: Vec<(Mode, gtk::ToggleButton)> = Vec::new();
    if modes.is_some() {
        let group = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        group.add_css_class("linked");
        // macOS 版の小さなセグメント。
        group.add_css_class("small-pushes");
        group.set_valign(gtk::Align::Center);
        for m in Mode::ALL {
            let icon = crate::icon::Icon::default();
            icon.set(m.icon(), false);
            let button = gtk::ToggleButton::new();
            button.set_child(Some(&icon));
            button.set_tooltip_text(Some(&m.title()));
            button.update_property(&[gtk::accessible::Property::Label(&m.title())]);
            if let Some((_, first)) = buttons.first() {
                button.set_group(Some(first));
            }
            group.append(&button);
            buttons.push((m, button));
        }
        bar.append(&group);
    }
    if let Some(on_show_text) = on_show_text {
        let icon = crate::icon::Icon::default();
        icon.set(crate::icon::Kind::Doc, false);
        let text = gtk::Button::new();
        text.set_child(Some(&icon));
        text.add_css_class("flat");
        text.set_tooltip_text(Some(&tr!("Show as Text")));
        text.update_property(&[gtk::accessible::Property::Label(&tr!("Show as Text"))]);
        text.connect_clicked(move |_| on_show_text());
        bar.append(&text);
    }
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    bar.append(&spacer);
    let slider = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 0.01);
    slider.set_draw_value(false);
    slider.set_value(0.5);
    slider.set_size_request(160, -1);
    slider.set_visible(false);
    bar.append(&slider);
    // 市松模様の明暗の切り替え（透けた画素があるときだけ。押しているあいだ暗い）。
    let checker_icon = crate::icon::Icon::default();
    checker_icon.set(crate::icon::Kind::Checkerboard, false);
    let checker_button = gtk::ToggleButton::new();
    checker_button.set_child(Some(&checker_icon));
    checker_button.add_css_class("flat");
    checker_button.set_visible(tone.transparent());
    let checker_tip = |b: &gtk::ToggleButton| {
        let title = if b.is_active() {
            tr!("Lighten the Checkerboard")
        } else {
            tr!("Darken the Checkerboard")
        };
        b.set_tooltip_text(Some(&title));
        b.update_property(&[gtk::accessible::Property::Label(&title)]);
    };
    checker_button.set_active(dark);
    checker_tip(&checker_button);
    let c = canvas.downgrade();
    checker_button.connect_toggled(move |b| {
        checker_tip(b);
        if let Some(c) = c.upgrade() {
            c.imp().dark_checker.set(b.is_active());
            c.queue_draw();
        }
    });
    bar.append(&checker_button);
    // 拡大縮小: 倍率（押すと 100% と合わせるを切り替える）・縮める・広げる・合わせる。
    let zoom_label = gtk::Button::with_label(&tr!("Fit"));
    zoom_label.add_css_class("flat");
    zoom_label.add_css_class("numeric");
    zoom_label.set_tooltip_text(Some(&tr!("Actual Size (100%) or Zoom to Fit")));
    bar.append(&zoom_label);
    let zooms = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    zooms.add_css_class("linked");
    zooms.add_css_class("small-pushes");
    zooms.set_valign(gtk::Align::Center);
    for (kind, title) in [
        (crate::icon::Kind::ZoomOut, tr!("Zoom Out")),
        (crate::icon::Kind::ZoomIn, tr!("Zoom In")),
        (crate::icon::Kind::ZoomFit, tr!("Zoom to Fit")),
    ] {
        let icon = crate::icon::Icon::default();
        icon.set(kind, false);
        let button = gtk::Button::new();
        button.set_child(Some(&icon));
        button.set_tooltip_text(Some(&title));
        button.update_property(&[gtk::accessible::Property::Label(&title)]);
        let c = canvas.downgrade();
        button.connect_clicked(move |_| {
            let Some(c) = c.upgrade() else { return };
            match kind {
                crate::icon::Kind::ZoomOut => c.zoom_by(1.0 / 1.5),
                crate::icon::Kind::ZoomIn => c.zoom_by(1.5),
                _ => c.set_zoom(None),
            }
        });
        zooms.append(&button);
    }
    bar.append(&zooms);
    let c = canvas.downgrade();
    zoom_label.connect_clicked(move |_| {
        if let Some(c) = c.upgrade() {
            c.set_zoom(if c.imp().zoom.get() == Some(1.0) {
                None
            } else {
                Some(1.0)
            });
        }
    });
    let update_label = {
        let (label, c) = (zoom_label.downgrade(), canvas.downgrade());
        move || {
            let (Some(label), Some(c)) = (label.upgrade(), c.upgrade()) else {
                return;
            };
            label.set_label(&match c.imp().zoom.get() {
                Some(z) => format!("{}%", (z * 100.0).round() as i64),
                None => tr!("Fit"),
            });
        }
    };
    update_label();
    canvas.connect_zoom(update_label);
    let caption = gtk::Label::new(None);
    caption.set_xalign(0.0);
    caption.set_ellipsize(pango::EllipsizeMode::End);
    caption.add_css_class("caption");
    caption.add_css_class("dim-label");
    caption.set_text(&base_caption);
    let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
    root.set_margin_top(8);
    root.set_margin_start(12);
    root.set_margin_end(12);
    root.append(&bar);
    root.append(&canvas);
    root.append(&caption);
    if !footnote.is_empty() {
        let note = gtk::Label::new(Some(&footnote.join("\n")));
        note.set_xalign(0.0);
        note.set_wrap(true);
        note.set_wrap_mode(pango::WrapMode::WordChar);
        note.set_selectable(true);
        note.add_css_class("dim-label");
        // Markdown の図の注記と同じ、少し小さい字
        let attrs = pango::AttrList::new();
        attrs.insert(pango::AttrFloat::new_scale(0.9));
        note.set_attributes(Some(&attrs));
        root.append(&note);
    }
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 6);
    outer.append(&root);
    if fill {
        root.set_margin_bottom(8);
        root.set_vexpand(true);
        outer.set_vexpand(true);
    }
    if single {
        return outer.upcast();
    }
    if !fill {
        outer.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    }
    let apply = {
        let (canvas, slider, caption) = (canvas.clone(), slider.clone(), caption.clone());
        Rc::new(move |mode: Mode| {
            canvas.set_mode(mode);
            slider.set_visible(matches!(mode, Mode::Swipe | Mode::Onion));
            caption.set_text(&if mode == Mode::Difference {
                canvas.difference_summary()
            } else {
                base_caption.clone()
            });
        })
    };
    if let Some((mode, on_mode)) = modes {
        apply(mode);
        for (m, button) in &buttons {
            button.set_active(*m == mode);
            let (apply, on_mode, m) = (apply.clone(), on_mode.clone(), *m);
            button.connect_toggled(move |b| {
                if b.is_active() {
                    apply(m);
                    on_mode(m);
                }
            });
        }
    }
    let c = canvas.clone();
    slider.connect_value_changed(move |s| c.set_position(s.value() as f32));
    let s = slider.downgrade();
    canvas.connect_position(move |p| {
        if let Some(s) = s.upgrade() {
            s.set_value(p as f64);
        }
    });
    outer.upcast()
}

/// 比べ方を変えたときの知らせ先。
type OnMode = Rc<dyn Fn(Mode)>;

/// スワイプ・オニオンスキンの位置を動かしたときの知らせ先。
type OnPosition = Box<dyn Fn(f32)>;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Canvas {
        pub before: RefCell<Option<gdk::Texture>>,
        pub after: RefCell<Option<gdk::Texture>>,
        pub mode: Cell<Mode>,
        pub position: Cell<f32>,
        /// 違うピクセルの絵と、違う数・全体の数（はじめて要るときに作る）。
        pub difference: OnceCell<(Option<gdk::Texture>, usize, usize)>,
        pub on_position: RefCell<Option<OnPosition>>,
        /// 1 枚だけ見せる（比べない。`after` を出す）。
        pub single: Cell<bool>,
        /// 市松模様を暗くする（既定は絵の色から。`checker_default`）。
        pub dark_checker: Cell<bool>,
        /// 倍率（画像の 1 ピクセルを何ピクセルで描くか）。None なら枠に合わせる。
        pub zoom: Cell<Option<f32>>,
        /// 拡大したときの、枠の中心からのずらし。
        pub pan: Cell<(f32, f32)>,
        /// 倍率が変わった（帯の倍率の表示）。
        pub on_zoom: RefCell<Option<Box<dyn Fn()>>>,
        /// 絵を動かしているドラッグの、始めのずらし（境を動かしているなら None）。
        pub drag: Cell<Option<(f32, f32)>>,
        /// ピンチを始めたときの倍率。
        pub pinch: Cell<f32>,
        /// 前と後の見出し。
        pub titles: RefCell<(String, String)>,
        /// 1 枚の絵が SVG のときの中身（拡大した大きさで描き直す）。
        pub vector: RefCell<Option<glib::Bytes>>,
        /// 描き直した絵と、その画素の大きさ。
        pub sharp: RefCell<Option<((i32, i32), gdk::Texture)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Canvas {
        const NAME: &'static str = "AmonhenImageCanvas";
        type Type = super::Canvas;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Canvas {
        fn constructed(&self) {
            self.parent_constructed();
            self.position.set(0.5);
            self.obj().set_overflow(gtk::Overflow::Hidden);
            // 拡大している間は、並べる・違うピクセル・1 枚ではドラッグで絵を動かす（スワイプ・オニオンスキンは
            // 境を動かすので Alt を押しながら）。そのほかは、押した・ドラッグした位置まで境を動かす。
            let drag = gtk::GestureDrag::new();
            let weak = self.obj().downgrade();
            drag.connect_drag_begin(move |g, x, _| {
                g.set_state(gtk::EventSequenceState::Claimed);
                let Some(c) = weak.upgrade() else { return };
                let imp = c.imp();
                let alt = g
                    .current_event_state()
                    .contains(gdk::ModifierType::ALT_MASK);
                let pans = imp.single.get()
                    || matches!(imp.mode.get(), Mode::TwoUp | Mode::Difference)
                    || alt;
                if imp.zoom.get().is_some() && pans {
                    imp.drag.set(Some(imp.pan.get()));
                } else {
                    imp.drag.set(None);
                    c.track(x as f32);
                }
            });
            let weak = self.obj().downgrade();
            drag.connect_drag_update(move |g, dx, dy| {
                let Some(c) = weak.upgrade() else { return };
                if let Some((x, y)) = c.imp().drag.get() {
                    c.imp().pan.set((x + dx as f32, y + dy as f32));
                    c.queue_draw();
                } else if let Some((x, _)) = g.start_point() {
                    c.track((x + dx) as f32);
                }
            });
            self.obj().add_controller(drag);
            // ピンチで拡大縮小。
            let pinch = gtk::GestureZoom::new();
            let weak = self.obj().downgrade();
            pinch.connect_begin(move |_, _| {
                if let Some(c) = weak.upgrade() {
                    c.imp().pinch.set(c.scale());
                }
            });
            let weak = self.obj().downgrade();
            pinch.connect_scale_changed(move |_, scale| {
                if let Some(c) = weak.upgrade() {
                    c.set_zoom(Some(c.imp().pinch.get() * scale as f32));
                }
            });
            self.obj().add_controller(pinch);
            // Ctrl＋スクロールで拡大縮小。拡大している間のスクロールは絵を動かし、合わせているときは差分に回す。
            let scroll =
                gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
            let weak = self.obj().downgrade();
            scroll.connect_scroll(move |ctl, dx, dy| {
                let Some(c) = weak.upgrade() else {
                    return glib::Propagation::Proceed;
                };
                let wheel = ctl.unit() == gdk::ScrollUnit::Wheel;
                if ctl
                    .current_event_state()
                    .contains(gdk::ModifierType::CONTROL_MASK)
                {
                    let steps = if wheel { -dy } else { -dy / 10.0 };
                    if steps != 0.0 {
                        c.zoom_by(1.1f32.powf(steps as f32));
                    }
                    return glib::Propagation::Stop;
                }
                if c.imp().zoom.get().is_none() {
                    return glib::Propagation::Proceed;
                }
                let step = if wheel { 40.0 } else { 1.0 };
                let (x, y) = c.imp().pan.get();
                c.imp()
                    .pan
                    .set((x - (dx * step) as f32, y - (dy * step) as f32));
                c.queue_draw();
                glib::Propagation::Stop
            });
            self.obj().add_controller(scroll);
        }
    }

    impl WidgetImpl for Canvas {
        fn measure(&self, orientation: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            match orientation {
                gtk::Orientation::Horizontal => (0, 200, -1, -1),
                _ => (60, 200, -1, -1),
            }
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            self.obj().draw(snapshot);
        }
    }
}

glib::wrapper! {
    pub struct Canvas(ObjectSubclass<imp::Canvas>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Canvas {
    fn new(before: Option<gdk::Texture>, after: Option<gdk::Texture>) -> Self {
        let canvas: Self = glib::Object::new();
        canvas.imp().before.replace(before);
        canvas.imp().after.replace(after);
        canvas.imp().titles.replace((tr!("Before"), tr!("After")));
        canvas
    }

    fn set_mode(&self, mode: Mode) {
        self.imp().mode.set(mode);
        self.queue_draw();
    }

    fn set_position(&self, position: f32) {
        self.imp().position.set(position.clamp(0.0, 1.0));
        self.queue_draw();
    }

    fn connect_position(&self, f: impl Fn(f32) + 'static) {
        self.imp().on_position.replace(Some(Box::new(f)));
    }

    fn connect_zoom(&self, f: impl Fn() + 'static) {
        self.imp().on_zoom.replace(Some(Box::new(f)));
    }

    /// 倍率を決める（None なら枠に合わせる）。
    fn set_zoom(&self, zoom: Option<f32>) {
        let zoom = zoom.map(|z| z.clamp(0.05, 64.0));
        self.imp().zoom.set(zoom);
        if zoom.is_none() {
            self.imp().pan.set((0.0, 0.0));
        }
        self.queue_draw();
        if let Some(f) = self.imp().on_zoom.borrow().as_ref() {
            f();
        }
    }

    fn zoom_by(&self, factor: f32) {
        self.set_zoom(Some(self.scale() * factor));
    }

    /// 今の倍率（合わせているならその倍率）。
    fn scale(&self) -> f32 {
        if let Some(zoom) = self.imp().zoom.get() {
            return zoom;
        }
        let (w, h) = (self.width() as f32, self.height() as f32);
        let area = if self.imp().single.get() || self.imp().mode.get() != Mode::TwoUp {
            self.inset()
        } else {
            graphene::Rect::new(0.0, 16.0, ((w - 16.0) / 2.0).max(0.0), (h - 16.0).max(0.0))
                .inset_r(4.0, 4.0)
        };
        Self::fit_scale(self.common(), &area)
    }

    fn size_of(t: &Option<gdk::Texture>) -> (f32, f32) {
        t.as_ref()
            .map_or((0.0, 0.0), |t| (t.width() as f32, t.height() as f32))
    }

    /// 前と後を重ねる枠の大きさ（大きい方）。
    fn common(&self) -> (f32, f32) {
        let (a, b) = (
            Self::size_of(&self.imp().before.borrow()),
            Self::size_of(&self.imp().after.borrow()),
        );
        (a.0.max(b.0), a.1.max(b.1))
    }

    /// 枠に合わせるときの倍率（小さな絵は整数倍まで拡げ、ぼかさない）。
    fn fit_scale(size: (f32, f32), area: &graphene::Rect) -> f32 {
        if size.0 <= 0.0 || size.1 <= 0.0 {
            return 1.0;
        }
        let mut scale = (area.width() / size.0).min(area.height() / size.1);
        if scale > 1.0 {
            scale = scale.min(8.0).floor().max(1.0);
        }
        scale
    }

    /// `size` を `area` に置く: 倍率を決めていなければ収め、決めていればその倍率で中心からずらして。
    fn fit(&self, size: (f32, f32), area: &graphene::Rect) -> graphene::Rect {
        if size.0 <= 0.0 || size.1 <= 0.0 {
            return graphene::Rect::zero();
        }
        let zoom = self.imp().zoom.get();
        let scale = zoom.unwrap_or_else(|| Self::fit_scale(size, area));
        let (dx, dy) = if zoom.is_some() {
            self.imp().pan.get()
        } else {
            (0.0, 0.0)
        };
        let (w, h) = ((size.0 * scale).round(), (size.1 * scale).round());
        graphene::Rect::new(
            (area.x() + (area.width() - w) / 2.0 + dx).round(),
            (area.y() + (area.height() - h) / 2.0 + dy).round(),
            w,
            h,
        )
    }

    fn inset(&self) -> graphene::Rect {
        graphene::Rect::new(
            4.0,
            4.0,
            (self.width() as f32 - 8.0).max(0.0),
            (self.height() as f32 - 8.0).max(0.0),
        )
    }

    /// 重ねる絵の中の、その画像の位置（左上にそろえる）。
    fn fitted(&self, image: &Option<gdk::Texture>, rect: &graphene::Rect) -> graphene::Rect {
        let common = self.common();
        let size = Self::size_of(image);
        if common.0 <= 0.0 {
            return *rect;
        }
        let scale = rect.width() / common.0;
        graphene::Rect::new(rect.x(), rect.y(), size.0 * scale, size.1 * scale)
    }

    fn track(&self, x: f32) {
        if self.imp().single.get() || !matches!(self.imp().mode.get(), Mode::Swipe | Mode::Onion) {
            return;
        }
        let rect = self.fit(self.common(), &self.inset());
        if rect.width() <= 0.0 {
            return;
        }
        let p = ((x - rect.x()) / rect.width()).clamp(0.0, 1.0);
        self.set_position(p);
        if let Some(f) = self.imp().on_position.borrow().as_ref() {
            f(p);
        }
    }

    /// 違うピクセルの数の説明（「N of M pixels differ (x%)」）。
    fn difference_summary(&self) -> String {
        let (_, changed, total) = self.difference();
        if *total == 0 {
            return tr!("Nothing to compare");
        }
        tr!(
            "{changed} of {total} pixels differ ({percent}%)",
            changed = changed,
            total = total,
            percent = format!("{:.1}", *changed as f64 * 100.0 / *total as f64)
        )
    }

    fn difference(&self) -> &(Option<gdk::Texture>, usize, usize) {
        self.imp().difference.get_or_init(|| {
            make_difference(
                self.imp().before.borrow().as_ref(),
                self.imp().after.borrow().as_ref(),
            )
        })
    }

    /// SVG の絵を `rect` に描くとき、テクスチャより大きく描くなら、その画素の大きさで描き直した絵（ぼやけない）。
    /// 描き直すまでもない、大きすぎる、SVG ではないときは None。
    fn sharp(&self, image: &Option<gdk::Texture>, rect: &graphene::Rect) -> Option<gdk::Texture> {
        // 描き直す絵の画素の上限（4096 × 4096）。超えるときはテクスチャを伸ばす。
        const MAX_PIXELS: i64 = 4096 * 4096;
        let image = image.as_ref()?;
        let vector = self.imp().vector.borrow().clone()?;
        let factor = self.scale_factor().max(1) as f32;
        let size = (
            (rect.width() * factor).round() as i32,
            (rect.height() * factor).round() as i32,
        );
        if size.0 <= image.width() && size.1 <= image.height() {
            return None;
        }
        if size.0 <= 0 || size.1 <= 0 || i64::from(size.0) * i64::from(size.1) > MAX_PIXELS {
            return None;
        }
        if let Some((at, texture)) = self.imp().sharp.borrow().as_ref()
            && *at == size
        {
            return Some(texture.clone());
        }
        let texture = svg_pixels(&vector, size.0, size.1)?.texture();
        self.imp().sharp.replace(Some((size, texture.clone())));
        Some(texture)
    }

    fn draw(&self, snapshot: &gtk::Snapshot) {
        let dark = self.imp().dark_checker.get();
        let checker =
            |snapshot: &gtk::Snapshot, rect: &graphene::Rect| checker(snapshot, rect, dark);
        let fg = self.color();
        let secondary = gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), fg.alpha() * 0.6);
        let label = |text: &str, x: f32, y: f32| {
            let l = self.create_pango_layout(Some(text));
            let attrs = pango::AttrList::new();
            attrs.insert(pango::AttrFloat::new_scale(0.85));
            attrs.insert(pango::AttrInt::new_weight(pango::Weight::Medium));
            l.set_attributes(Some(&attrs));
            snapshot.save();
            snapshot.translate(&graphene::Point::new(x, y));
            snapshot.append_layout(&l, &secondary);
            snapshot.restore();
        };
        let (before, after) = (
            self.imp().before.borrow().clone(),
            self.imp().after.borrow().clone(),
        );
        let (w, h) = (self.width() as f32, self.height() as f32);
        let position = self.imp().position.get();
        let titles = self.imp().titles.borrow().clone();
        if self.imp().single.get() {
            let rect = self.fit(Self::size_of(&after), &self.inset());
            checker(snapshot, &rect);
            let sharp = self.sharp(&after, &rect);
            draw_texture(
                snapshot,
                if sharp.is_some() { &sharp } else { &after },
                &rect,
                1.0,
            );
            return;
        }
        match self.imp().mode.get() {
            Mode::TwoUp => {
                let half = ((w - 16.0) / 2.0).max(0.0);
                let red = gdk::RGBA::new(0.88, 0.11, 0.14, 0.8);
                let green = gdk::RGBA::new(0.18, 0.76, 0.49, 0.8);
                for (i, (image, title, color)) in [
                    (&before, titles.0.clone(), red),
                    (&after, titles.1.clone(), green),
                ]
                .into_iter()
                .enumerate()
                {
                    let area = graphene::Rect::new(
                        i as f32 * (half + 16.0),
                        16.0,
                        half,
                        (h - 16.0).max(0.0),
                    );
                    label(&title, area.x(), 0.0);
                    if image.is_some() {
                        // 拡大しても、それぞれの側の枠からはみ出さない。
                        snapshot.push_clip(&area);
                        let rect = self.fit(Self::size_of(image), &area.inset_r(4.0, 4.0));
                        checker(snapshot, &rect);
                        draw_texture(snapshot, image, &rect, 1.0);
                        frame(snapshot, &rect.inset_r(-1.5, -1.5), 2.0, &color);
                        snapshot.pop();
                    } else {
                        let dash = area.inset_r(30.0, 20.0);
                        let path = gsk::PathBuilder::new();
                        path.add_rect(&dash);
                        let stroke = gsk::Stroke::new(1.0);
                        stroke.set_dash(&[4.0, 4.0]);
                        snapshot.append_stroke(&path.to_path(), &stroke, &secondary);
                        label(
                            &tr!("None"),
                            area.x() + area.width() / 2.0 - 14.0,
                            area.y() + area.height() / 2.0 - 7.0,
                        );
                    }
                }
            }
            Mode::Swipe | Mode::Onion => {
                let rect = self.fit(self.common(), &self.inset());
                checker(snapshot, &rect);
                if self.imp().mode.get() == Mode::Onion {
                    draw_texture(snapshot, &before, &self.fitted(&before, &rect), 1.0);
                    draw_texture(snapshot, &after, &self.fitted(&after, &rect), position);
                } else {
                    draw_texture(snapshot, &after, &self.fitted(&after, &rect), 1.0);
                    let split = rect.x() + rect.width() * position;
                    snapshot.push_clip(&graphene::Rect::new(
                        rect.x(),
                        rect.y(),
                        (split - rect.x()).max(0.0),
                        rect.height(),
                    ));
                    checker(snapshot, &rect);
                    draw_texture(snapshot, &before, &self.fitted(&before, &rect), 1.0);
                    snapshot.pop();
                    let accent = gdk::RGBA::new(0.21, 0.52, 0.89, 1.0);
                    snapshot.append_color(
                        &accent,
                        &graphene::Rect::new(split - 1.0, rect.y() - 4.0, 2.0, rect.height() + 8.0),
                    );
                    let knob = gsk::PathBuilder::new();
                    knob.add_circle(
                        &graphene::Point::new(split, rect.y() + rect.height() / 2.0),
                        7.0,
                    );
                    snapshot.append_fill(&knob.to_path(), gsk::FillRule::Winding, &accent);
                }
                label(&titles.0, rect.x() - 56.0, rect.y());
                label(&titles.1, rect.x() + rect.width() + 8.0, rect.y());
            }
            Mode::Difference => {
                let rect = self.fit(self.common(), &self.inset());
                snapshot.append_color(&gdk::RGBA::BLACK, &rect);
                let image = self.difference().0.clone();
                if let Some(image) = image {
                    snapshot.append_scaled_texture(&image, gsk::ScalingFilter::Nearest, &rect);
                }
            }
        }
    }
}

/// 透けたところを見せる市松模様（`dark` なら暗い色で）。
fn checker(snapshot: &gtk::Snapshot, rect: &graphene::Rect, dark: bool) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let (base, square) = if dark { (0.16, 0.24) } else { (1.0, 0.86) };
    snapshot.append_color(&gdk::RGBA::new(base, base, base, 1.0), rect);
    let gray = gdk::RGBA::new(square, square, square, 1.0);
    let s = 8.0;
    let (right, bottom) = (rect.x() + rect.width(), rect.y() + rect.height());
    let mut y = rect.y();
    let mut row = 0;
    while y < bottom {
        let mut x = rect.x() + if row % 2 == 0 { 0.0 } else { s };
        while x < right {
            snapshot.append_color(
                &gray,
                &graphene::Rect::new(x, y, s.min(right - x), s.min(bottom - y)),
            );
            x += 2.0 * s;
        }
        y += s;
        row += 1;
    }
}

/// 画像を `rect` に描く（2 倍以上に拡げるならぼかさない）。
fn draw_texture(
    snapshot: &gtk::Snapshot,
    image: &Option<gdk::Texture>,
    rect: &graphene::Rect,
    alpha: f32,
) {
    let Some(image) = image else { return };
    let filter = if rect.width() >= image.width() as f32 * 2.0 {
        gsk::ScalingFilter::Nearest
    } else {
        gsk::ScalingFilter::Linear
    };
    if alpha < 1.0 {
        snapshot.push_opacity(alpha as f64);
    }
    snapshot.append_scaled_texture(image, filter, rect);
    if alpha < 1.0 {
        snapshot.pop();
    }
}

/// 枠の線。
fn frame(snapshot: &gtk::Snapshot, rect: &graphene::Rect, width: f32, color: &gdk::RGBA) {
    let path = gsk::PathBuilder::new();
    path.add_rect(rect);
    snapshot.append_stroke(&path.to_path(), &gsk::Stroke::new(width), color);
}

/// 市松模様の既定（絵の色から。比べるときは両方を合わせて 1 つ）と、透けた画素があるか。
fn checker_default(images: &[Option<&gdk::Texture>]) -> amonhen_core::checker::Tone {
    let mut tone = amonhen_core::checker::Tone::default();
    for image in images.iter().flatten() {
        let (w, h) = (image.width() as usize, image.height() as usize);
        let mut own = vec![0u8; w * h * 4];
        // 既定の並びは、不透明度を掛けた B8G8R8A8（リトルエンディアン）
        image.download(&mut own, w * 4);
        tone.add(&own, w, h, w * 4, amonhen_core::checker::Order::Bgra);
    }
    tone
}

/// 画像のピクセル（左上にそろえて `w`×`h` に置く。外は透明）。
fn pixels(image: Option<&gdk::Texture>, w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h * 4];
    let Some(image) = image else { return out };
    let (iw, ih) = (image.width() as usize, image.height() as usize);
    let mut own = vec![0u8; iw * ih * 4];
    image.download(&mut own, iw * 4);
    for y in 0..ih.min(h) {
        let from = &own[y * iw * 4..(y * iw + iw.min(w)) * 4];
        out[y * w * 4..y * w * 4 + from.len()].copy_from_slice(from);
    }
    out
}

/// 違うピクセルを赤紫で、同じところは後の画像を薄く（違う数と全体の数も）。
fn make_difference(
    before: Option<&gdk::Texture>,
    after: Option<&gdk::Texture>,
) -> (Option<gdk::Texture>, usize, usize) {
    let size =
        |t: Option<&gdk::Texture>| t.map_or((0, 0), |t| (t.width() as usize, t.height() as usize));
    let (a, b) = (size(before), size(after));
    let (w, h) = (a.0.max(b.0), a.1.max(b.1));
    if w == 0 || h == 0 {
        return (None, 0, 0);
    }
    let (old, new) = (pixels(before, w, h), pixels(after, w, h));
    let mut out = vec![0u8; w * h * 4];
    let mut changed = 0;
    for i in (0..out.len()).step_by(4) {
        if old[i..i + 4] != new[i..i + 4] {
            changed += 1;
            out[i..i + 4].copy_from_slice(&[255, 0, 200, 255]);
        } else {
            let gray = ((new[i] as u32 + new[i + 1] as u32 + new[i + 2] as u32) / 3 / 3) as u8;
            out[i..i + 4].copy_from_slice(&[gray, gray, gray, 255]);
        }
    }
    let texture = gdk::MemoryTexture::new(
        w as i32,
        h as i32,
        gdk::MemoryFormat::R8g8b8a8,
        &glib::Bytes::from_owned(out),
        w * 4,
    );
    (Some(texture.upcast()), changed, w * h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_image_extensions() {
        assert!(is_image("a/b/icon.PNG"));
        assert!(is_image("logo.svg"));
        assert!(is_svg("logo.SVG"));
        assert!(!is_svg("logo.png"));
        assert!(!is_image("src/main.rs"));
        assert!(!is_image("Makefile"));
        // 文書はプレビューに任せる。
        assert!(!is_image("manual.pdf"));
        assert!(!is_image("figure.eps"));
        assert!(is_image("photo.JPEG"));
    }
}
