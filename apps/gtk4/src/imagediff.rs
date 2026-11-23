//! History の右の列の、画像の前と後の比較（macOS 版の `ImageDiffPanel`・`ImageCompareCanvas`）。変更の見出しの
//! 下に置く欄で、並べる・スワイプ・オニオンスキン・違うピクセルを切り替える。比べ方は窓を閉じるまで覚える
//! （次に開く画像も同じ比べ方）。SVG は「テキストで見る」で差分に戻せる。

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;

use amonhen_model::tr;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib, graphene, gsk, pango};

/// 画像として比べるファイルの拡張子（macOS 版と同じ）。
const EXTENSIONS: [&str; 11] = [
    "png", "jpg", "jpeg", "gif", "bmp", "tif", "tiff", "webp", "heic", "ico", "svg",
];

fn extension(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

pub fn is_image(path: &str) -> bool {
    EXTENSIONS.contains(&extension(path).as_str())
}

pub fn is_svg(path: &str) -> bool {
    extension(path) == "svg"
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
    let base_caption = format!(
        "{}: {}    →    {}: {}",
        tr!("Before"),
        describe(before, old.as_ref()),
        tr!("After"),
        describe(after, new.as_ref())
    );
    let canvas = Canvas::new(old, new);
    canvas.set_vexpand(true);
    canvas.set_hexpand(true);
    let modes = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    modes.add_css_class("linked");
    let mut buttons: Vec<gtk::ToggleButton> = Vec::new();
    for m in Mode::ALL {
        let icon = crate::icon::Icon::default();
        icon.set(m.icon(), false);
        let button = gtk::ToggleButton::new();
        button.set_child(Some(&icon));
        button.set_tooltip_text(Some(&m.title()));
        button.update_property(&[gtk::accessible::Property::Label(&m.title())]);
        if let Some(first) = buttons.first() {
            button.set_group(Some(first));
        }
        button.set_active(m == mode);
        modes.append(&button);
        buttons.push(button);
    }
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    bar.append(&modes);
    if svg {
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
    bar.append(&slider);
    let caption = gtk::Label::new(None);
    caption.set_xalign(0.0);
    caption.set_ellipsize(pango::EllipsizeMode::End);
    caption.add_css_class("caption");
    caption.add_css_class("dim-label");
    let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
    root.set_margin_top(8);
    root.set_margin_start(12);
    root.set_margin_end(12);
    root.append(&bar);
    root.append(&canvas);
    root.append(&caption);
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 6);
    outer.append(&root);
    outer.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
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
    apply(mode);
    for (m, button) in Mode::ALL.into_iter().zip(&buttons) {
        let (apply, on_mode) = (apply.clone(), on_mode.clone());
        button.connect_toggled(move |b| {
            if b.is_active() {
                apply(m);
                on_mode(m);
            }
        });
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
            // スワイプ・オニオンスキンでは、押した・ドラッグした位置まで動かす。
            let drag = gtk::GestureDrag::new();
            let weak = self.obj().downgrade();
            drag.connect_drag_begin(move |g, x, _| {
                g.set_state(gtk::EventSequenceState::Claimed);
                if let Some(c) = weak.upgrade() {
                    c.track(x as f32);
                }
            });
            let weak = self.obj().downgrade();
            drag.connect_drag_update(move |g, dx, _| {
                if let (Some(c), Some((x, _))) = (weak.upgrade(), g.start_point()) {
                    c.track((x + dx) as f32);
                }
            });
            self.obj().add_controller(drag);
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

    /// `size` を `area` に収める（小さな絵は整数倍まで拡げ、ぼかさない）。
    fn fit(size: (f32, f32), area: &graphene::Rect) -> graphene::Rect {
        if size.0 <= 0.0 || size.1 <= 0.0 {
            return graphene::Rect::zero();
        }
        let mut scale = (area.width() / size.0).min(area.height() / size.1);
        if scale > 1.0 {
            scale = scale.min(8.0).floor().max(1.0);
        }
        let (w, h) = ((size.0 * scale).round(), (size.1 * scale).round());
        graphene::Rect::new(
            (area.x() + (area.width() - w) / 2.0).round(),
            (area.y() + (area.height() - h) / 2.0).round(),
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
        if !matches!(self.imp().mode.get(), Mode::Swipe | Mode::Onion) {
            return;
        }
        let rect = Self::fit(self.common(), &self.inset());
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

    fn draw(&self, snapshot: &gtk::Snapshot) {
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
        match self.imp().mode.get() {
            Mode::TwoUp => {
                let half = ((w - 16.0) / 2.0).max(0.0);
                let red = gdk::RGBA::new(0.88, 0.11, 0.14, 0.8);
                let green = gdk::RGBA::new(0.18, 0.76, 0.49, 0.8);
                for (i, (image, title, color)) in
                    [(&before, tr!("Before"), red), (&after, tr!("After"), green)]
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
                        let rect = Self::fit(Self::size_of(image), &area.inset_r(4.0, 4.0));
                        checker(snapshot, &rect);
                        draw_texture(snapshot, image, &rect, 1.0);
                        frame(snapshot, &rect.inset_r(-1.5, -1.5), 2.0, &color);
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
                let rect = Self::fit(self.common(), &self.inset());
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
                label(&tr!("Before"), rect.x() - 56.0, rect.y());
                label(&tr!("After"), rect.x() + rect.width() + 8.0, rect.y());
            }
            Mode::Difference => {
                let rect = Self::fit(self.common(), &self.inset());
                snapshot.append_color(&gdk::RGBA::BLACK, &rect);
                let image = self.difference().0.clone();
                if let Some(image) = image {
                    snapshot.append_scaled_texture(&image, gsk::ScalingFilter::Nearest, &rect);
                }
            }
        }
    }
}

/// 透けたところを見せる市松模様。
fn checker(snapshot: &gtk::Snapshot, rect: &graphene::Rect) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    snapshot.append_color(&gdk::RGBA::WHITE, rect);
    let gray = gdk::RGBA::new(0.86, 0.86, 0.86, 1.0);
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
    }
}
