//! 窓の背景（macOS 版の `Backdrop.swift`）: 型（System・Sidebar and Toolbar Only・One Layer for the Whole
//! Window）、背景画像とその置き方、見出しの帯の色。アプリの設定（Settings > Theme、`gtk.ini` の鍵は macOS 版の
//! UserDefaults と同じ）と、リポジトリごとの上書き（ブックマークの `backdrop`。形は macOS 版と同じ）。
//!
//! 当て方: 窓ごとに CSS の組（`amonhen-bd-<番号>`）を付け、窓の背景（色・画像・1 枚の地）を窓そのものに描き、
//! 中の面（見出しの帯・サイドバー・一覧・差分）の背景を型に合わせて消す・不透明にする。規則は全部の窓の分を
//! 1 つの CssProvider にまとめる。窓を透かす（画像なし）のは合成のある画面だけで、画像があれば窓は不透明のまま
//! （macOS 版と同じ。透ける窓は合成で表示が遅れる）。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gdk, gio, glib, graphene};
use serde_json::{Map, Value, json};

use crate::settings;

const STYLE: &str = "backdropStyle";
const IMAGE: &str = "backdropImage";
const IMAGE_PLACEMENT: &str = "backdropImagePlacement";
const CONTENT_OPACITY: &str = "backdropContentOpacity";
const TOOLBAR_COLOR: &str = "backdropToolbarColor";
const TOOLBAR_OPACITY: &str = "backdropToolbarOpacity";
const KEYS: [&str; 6] = [
    STYLE,
    IMAGE,
    IMAGE_PLACEMENT,
    CONTENT_OPACITY,
    TOOLBAR_COLOR,
    TOOLBAR_OPACITY,
];

/// 窓の背景の型。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Style {
    /// これまでの見た目（面ごとの色）。
    #[default]
    System,
    /// サイドバーと見出しの帯だけ透け、読む面（一覧・差分）は 1 つの不透明な色。
    Chrome,
    /// 窓全体に地を 1 枚敷き、どの面も自分では塗らない。読む面の濃さは `content_opacity`。
    Single,
}

impl Style {
    pub const ALL: [Style; 3] = [Style::System, Style::Chrome, Style::Single];

    fn raw(self) -> &'static str {
        match self {
            Style::System => "system",
            Style::Chrome => "chrome",
            Style::Single => "single",
        }
    }

    fn from_raw(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.raw() == s)
    }

    pub fn title(self) -> String {
        match self {
            Style::System => tr!("System"),
            Style::Chrome => tr!("Sidebar and Toolbar Only"),
            Style::Single => tr!("One Layer for the Whole Window"),
        }
    }
}

/// 背景画像の置き方。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Placement {
    /// 窓いっぱいに広げる（縦横の比は保ち、はみ出た分は切る）。
    #[default]
    Fill,
    /// 元の大きさで隅に置く（窓より大きければ窓に収まるまで縮める）。
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Placement {
    pub const ALL: [Placement; 5] = [
        Placement::Fill,
        Placement::TopLeft,
        Placement::TopRight,
        Placement::BottomLeft,
        Placement::BottomRight,
    ];

    fn raw(self) -> &'static str {
        match self {
            Placement::Fill => "fill",
            Placement::TopLeft => "topLeft",
            Placement::TopRight => "topRight",
            Placement::BottomLeft => "bottomLeft",
            Placement::BottomRight => "bottomRight",
        }
    }

    fn from_raw(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.raw() == s)
    }

    pub fn title(self) -> String {
        match self {
            Placement::Fill => tr!("Fill the Window"),
            Placement::TopLeft => tr!("Top Left"),
            Placement::TopRight => tr!("Top Right"),
            Placement::BottomLeft => tr!("Bottom Left"),
            Placement::BottomRight => tr!("Bottom Right"),
        }
    }

    fn css_position(self) -> &'static str {
        match self {
            Placement::Fill => "center",
            Placement::TopLeft => "left top",
            Placement::TopRight => "right top",
            Placement::BottomLeft => "left bottom",
            Placement::BottomRight => "right bottom",
        }
    }
}

/// 窓の背景の設定。アプリの設定と、リポジトリごとの上書きで同じ形。
#[derive(Clone, Debug, PartialEq)]
pub struct Backdrop {
    pub style: Style,
    /// 背景画像のパス（なければ窓の後ろが透ける）。
    pub image: Option<String>,
    pub placement: Placement,
    /// 1 枚の地の濃さ（0.4〜1）。
    pub content_opacity: f64,
    /// 見出しの帯に敷く色（`#rrggbb`）。
    pub toolbar_color: Option<String>,
    pub toolbar_opacity: f64,
}

impl Default for Backdrop {
    fn default() -> Self {
        Self {
            style: Style::System,
            image: None,
            placement: Placement::Fill,
            content_opacity: 0.8,
            toolbar_color: None,
            toolbar_opacity: 0.35,
        }
    }
}

impl Backdrop {
    /// アプリの設定。
    pub fn app() -> Self {
        let d = Self::default();
        let number = |key: &str, default: f64| {
            settings::string(key)
                .and_then(|s| s.parse().ok())
                .unwrap_or(default)
        };
        let text = |key: &str| settings::string(key).filter(|s| !s.is_empty());
        Self {
            style: text(STYLE)
                .and_then(|s| Style::from_raw(&s))
                .unwrap_or(d.style),
            image: text(IMAGE),
            placement: text(IMAGE_PLACEMENT)
                .and_then(|s| Placement::from_raw(&s))
                .unwrap_or(d.placement),
            content_opacity: number(CONTENT_OPACITY, d.content_opacity),
            toolbar_color: text(TOOLBAR_COLOR),
            toolbar_opacity: number(TOOLBAR_OPACITY, d.toolbar_opacity),
        }
    }

    /// アプリの設定にする（変わった鍵だけ書く）。
    pub fn set_app(&self) {
        let old = Self::app();
        let set = |key: &str, value: Option<String>| match value {
            Some(v) => settings::set_string(key, &v),
            None => settings::remove(key),
        };
        if old.style != self.style {
            set(STYLE, Some(self.style.raw().into()));
        }
        if old.image != self.image {
            set(IMAGE, self.image.clone());
        }
        if old.placement != self.placement {
            set(IMAGE_PLACEMENT, Some(self.placement.raw().into()));
        }
        if old.content_opacity != self.content_opacity {
            set(CONTENT_OPACITY, Some(self.content_opacity.to_string()));
        }
        if old.toolbar_color != self.toolbar_color {
            set(TOOLBAR_COLOR, self.toolbar_color.clone());
        }
        if old.toolbar_opacity != self.toolbar_opacity {
            set(TOOLBAR_OPACITY, Some(self.toolbar_opacity.to_string()));
        }
    }

    /// `location` の窓で使う設定（リポジトリの上書きがあればそれ）。
    pub fn resolved(location: &str) -> Self {
        crate::bookmarks::backdrop(location).unwrap_or_else(Self::app)
    }

    /// macOS 版の JSON（ブックマークの `backdrop`）。
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("style".into(), json!(self.style.raw()));
        m.insert("imagePlacement".into(), json!(self.placement.raw()));
        m.insert("contentOpacity".into(), json!(self.content_opacity));
        m.insert("toolbarOpacity".into(), json!(self.toolbar_opacity));
        if let Some(image) = &self.image {
            m.insert("image".into(), json!(image));
        }
        if let Some(color) = &self.toolbar_color {
            m.insert("toolbarColor".into(), json!(color));
        }
        Value::Object(m)
    }

    /// 後から足した項目がない記録も読めるように、なければ既定の値。
    pub fn from_json(v: &Value) -> Option<Self> {
        let d = Self::default();
        v.as_object()?;
        Some(Self {
            style: v["style"]
                .as_str()
                .and_then(Style::from_raw)
                .unwrap_or(d.style),
            image: v["image"].as_str().map(str::to_owned),
            placement: v["imagePlacement"]
                .as_str()
                .and_then(Placement::from_raw)
                .unwrap_or(d.placement),
            content_opacity: v["contentOpacity"].as_f64().unwrap_or(d.content_opacity),
            toolbar_color: v["toolbarColor"].as_str().map(str::to_owned),
            toolbar_opacity: v["toolbarOpacity"].as_f64().unwrap_or(d.toolbar_opacity),
        })
    }

    /// 差分の見出し（スクロールしても上に残る帯）の地。`None` はこれまでどおり（不透明）。
    pub fn band_alpha(&self) -> Option<f32> {
        (self.style == Style::Single).then_some(self.content_opacity as f32)
    }
}

/// 画像なしのサイドバーとツールバーだけ透過で、窓の地の濃さ。GNOME には後ろをぼかす仕組みがないので、
/// 後ろの窓の文字が読みの邪魔にならない程度に濃くする（macOS 版はぼかしたガラス）。
const CHROME_SEE_THROUGH: f32 = 0.85;

/// 窓ごとの当てた設定。
struct Entry {
    window: glib::WeakRef<gtk::Window>,
    id: u32,
    backdrop: Backdrop,
    /// この窓の背景に合わせる窓（マージエディタの別の窓は、開いたリポジトリの窓に合わせる）。
    source: Option<glib::WeakRef<gtk::Window>>,
    /// 隅に置いた画像を縮める大きさを決めたときの窓の大きさ（変われば規則を作り直す）。
    size: Cell<(i32, i32)>,
}

thread_local! {
    static ENTRIES: RefCell<Vec<Entry>> = const { RefCell::new(Vec::new()) };
    static NEXT_ID: Cell<u32> = const { Cell::new(1) };
    static PROVIDER: RefCell<Option<gtk::CssProvider>> = const { RefCell::new(None) };
    /// 画像の大きさ（パス → 点。読めなければ None）。
    static IMAGE_SIZES: RefCell<HashMap<String, Option<(i32, i32)>>> = RefCell::default();
    /// 規則を作り直す予定（続けて変わったときは 1 回にまとめる）。
    static PENDING: Cell<bool> = const { Cell::new(false) };
}

/// 窓に当てている設定（なければ System）。
pub fn of(widget: &impl IsA<gtk::Widget>) -> Backdrop {
    let Some(window) = widget.root().and_downcast::<gtk::Window>() else {
        return Backdrop::default();
    };
    ENTRIES.with(|e| {
        let e = e.borrow();
        let entry = e
            .iter()
            .find(|x| x.window.upgrade().as_ref() == Some(&window));
        let source = entry
            .and_then(|x| x.source.as_ref())
            .and_then(|s| s.upgrade());
        match source {
            Some(source) => e
                .iter()
                .find(|x| x.window.upgrade().as_ref() == Some(&source))
                .map(|x| x.backdrop.clone()),
            None => entry.map(|x| x.backdrop.clone()),
        }
        .unwrap_or_default()
    })
}

/// 窓に背景を当てる（変わっていなければ何もしない）。
pub fn apply(window: &impl IsA<gtk::Window>, backdrop: Backdrop) {
    let window = window.upcast_ref::<gtk::Window>();
    let changed = ENTRIES.with(|e| {
        let mut e = e.borrow_mut();
        if let Some(x) = e
            .iter_mut()
            .find(|x| x.window.upgrade().as_ref() == Some(window))
        {
            let changed = x.backdrop != backdrop;
            x.backdrop = backdrop;
            return changed;
        }
        let id = NEXT_ID.with(|n| n.replace(n.get() + 1));
        window.add_css_class(&format!("amonhen-bd-{id}"));
        watch_size(window);
        e.push(Entry {
            window: window.downgrade(),
            id,
            backdrop,
            source: None,
            size: Cell::new((0, 0)),
        });
        true
    });
    if changed {
        schedule();
    }
}

/// `window` の背景を `source` の窓に合わせる（`source` の設定が変わればついて行く）。
pub fn follow(window: &impl IsA<gtk::Window>, source: &impl IsA<gtk::Window>) {
    let window = window.upcast_ref::<gtk::Window>();
    let id = NEXT_ID.with(|n| n.replace(n.get() + 1));
    window.add_css_class(&format!("amonhen-bd-{id}"));
    watch_size(window);
    ENTRIES.with(|e| {
        e.borrow_mut().push(Entry {
            window: window.downgrade(),
            id,
            backdrop: Backdrop::default(),
            source: Some(source.upcast_ref::<gtk::Window>().downgrade()),
            size: Cell::new((0, 0)),
        })
    });
    schedule();
}

/// 大きさが変わったら、隅に置いた大きな画像を縮め直す（そのときだけ規則を作り直す）。
fn watch_size(window: &gtk::Window) {
    let check = |w: &gtk::Window| {
        let needs = ENTRIES.with(|e| {
            e.borrow().iter().any(|x| {
                x.window.upgrade().as_ref() == Some(w) && x.size.get() != (w.width(), w.height())
            })
        });
        if needs {
            schedule();
        }
    };
    window.connect_default_width_notify(check);
    window.connect_default_height_notify(check);
    window.connect_maximized_notify(check);
}

/// 設定・ブックマークが変わったら、窓の背景を当て直す `f` を呼ぶ（起動したときに一度、窓の側から登録する）。
pub fn connect_changed(f: impl Fn() + 'static) {
    let f = std::rc::Rc::new(f);
    let g = f.clone();
    settings::connect_changed(move |key| {
        if KEYS.contains(&key) {
            g();
        }
    });
    crate::bookmarks::connect(move |path| {
        if path.is_none() {
            f();
        }
        true
    });
}

fn schedule() {
    if PENDING.with(|p| p.replace(true)) {
        return;
    }
    glib::idle_add_local_once(|| {
        PENDING.with(|p| p.set(false));
        rebuild();
    });
}

/// 窓を透かせるか（合成のない X11 では、透かすと黒くなる）。
fn can_see_through() -> bool {
    gdk::Display::default().is_some_and(|d| d.is_rgba() && d.is_composited())
}

fn image_size(path: &str) -> Option<(i32, i32)> {
    IMAGE_SIZES.with(|s| {
        *s.borrow_mut().entry(path.to_owned()).or_insert_with(|| {
            let texture = gdk::Texture::from_filename(path).ok()?;
            Some((texture.width(), texture.height()))
        })
    })
}

fn rebuild() {
    let mut css = String::new();
    ENTRIES.with(|e| {
        let mut e = e.borrow_mut();
        e.retain(|x| x.window.upgrade().is_some());
        let settings: Vec<(u32, Backdrop)> = e
            .iter()
            .map(|x| {
                let source = x.source.as_ref().and_then(|s| s.upgrade()).and_then(|s| {
                    e.iter()
                        .find(|y| y.window.upgrade().as_ref() == Some(&s))
                        .map(|y| y.backdrop.clone())
                });
                (x.id, source.unwrap_or_else(|| x.backdrop.clone()))
            })
            .collect();
        for (x, (id, backdrop)) in e.iter().zip(settings) {
            let Some(window) = x.window.upgrade() else {
                continue;
            };
            x.size.set((window.width(), window.height()));
            css.push_str(&rules(id, &backdrop, (window.width(), window.height())));
        }
    });
    PROVIDER.with(|p| {
        let mut p = p.borrow_mut();
        let provider = p.get_or_insert_with(|| {
            let provider = gtk::CssProvider::new();
            if let Some(display) = gdk::Display::default() {
                // テーマ・アプリのほかの規則より上に（面の背景を消すので）。
                gtk::style_context_add_provider_for_display(
                    &display,
                    &provider,
                    gtk::STYLE_PROVIDER_PRIORITY_USER,
                );
            }
            provider
        });
        provider.load_from_string(&css);
    });
    // 差分の見出しの帯など、自分で描く部品を描き直す。
    for window in gtk::Window::list_toplevels() {
        redraw(&window);
    }
}

fn redraw(widget: &gtk::Widget) {
    widget.queue_draw();
    let mut child = widget.first_child();
    while let Some(c) = child {
        redraw(&c);
        child = c.next_sibling();
    }
}

/// 1 つの窓の規則。
fn rules(id: u32, b: &Backdrop, (width, height): (i32, i32)) -> String {
    let w = format!("window.amonhen-bd-{id}");
    // 読む面の中（一覧・差分・文字の欄など）は自分では塗らず、下の 1 つの色（Single では地）を見せる。
    let panes = format!(
        "{w} .amonhen-pages .view, {w} .amonhen-pages list, {w} .amonhen-pages listview, \
         {w} .amonhen-pages columnview, {w} .amonhen-pages scrolledwindow, {w} .amonhen-pages viewport, \
         {w} .amonhen-pages textview, {w} .amonhen-pages textview > text, {w} .amonhen-pages paned, \
         {w} .amonhen-pages stack, {w} .amonhen-pages .amonhen-merge, {w} .amonhen-merge .view, \
         {w} .amonhen-merge textview > text, {w} .amonhen-merge scrolledwindow \
         {{ background-color: transparent; background-image: none; }}\n"
    );
    // 読む面（右のページとステータスバー、マージの窓）の色。
    let opaque =
        format!("{w} .amonhen-pages, {w} .amonhen-merge {{ background-color: @view_bg_color; }}\n");
    if b.style == Style::System {
        // 窓・見出しの帯・サイドバーはふだんのまま、読む面だけ 1 つの色にそろえる。
        return opaque + &panes;
    }
    let picture = b
        .image
        .as_deref()
        .and_then(|path| image_size(path).map(|size| (path, size)));
    let see_through = picture.is_none() && can_see_through();
    let mut css = String::new();
    // 窓そのもの: 地の色と画像（1 枚の地は、画像の上に地を重ねる）。
    let ground = format!("alpha(@view_bg_color, {:.3})", b.content_opacity);
    let color = match (b.style, see_through) {
        (Style::Single, true) => ground.clone(),
        (Style::Single, false) if picture.is_none() => "@view_bg_color".to_owned(),
        (Style::Chrome, true) => format!("alpha(@window_bg_color, {CHROME_SEE_THROUGH})"),
        _ => "@window_bg_color".to_owned(),
    };
    let mut images = Vec::new();
    let mut sizes = Vec::new();
    let mut positions = Vec::new();
    if b.style == Style::Single && picture.is_some() {
        images.push(format!("linear-gradient({ground}, {ground})"));
        sizes.push("100% 100%".to_owned());
        positions.push("center".to_owned());
    }
    if let Some((path, (iw, ih))) = picture {
        let uri = gio::File::for_path(path).uri();
        images.push(format!("url(\"{}\")", uri.replace('"', "%22")));
        sizes.push(if b.placement == Placement::Fill {
            "cover".to_owned()
        } else {
            // 元の大きさ。窓より大きければ比を保って縮める。
            let scale = 1f64
                .min(width.max(1) as f64 / iw.max(1) as f64)
                .min(height.max(1) as f64 / ih.max(1) as f64);
            format!(
                "{}px {}px",
                (iw as f64 * scale).round(),
                (ih as f64 * scale).round()
            )
        });
        positions.push(b.placement.css_position().to_owned());
    }
    css.push_str(&format!("{w} {{ background-color: {color};"));
    if !images.is_empty() {
        css.push_str(&format!(
            " background-image: {}; background-size: {}; background-position: {}; background-repeat: no-repeat;",
            images.join(", "),
            sizes.join(", "),
            positions.join(", ")
        ));
    }
    css.push_str(" }\n");
    // 見出しの帯とタブの帯: 窓の背景を見せる（色を付けるなら、その色を重ねる）。
    let tint = b
        .toolbar_color
        .as_deref()
        .and_then(crate::theme::parse_hex)
        .map(|(r, g, bl)| {
            format!(
                "rgba({}, {}, {}, {:.3})",
                (r * 255.0).round(),
                (g * 255.0).round(),
                (bl * 255.0).round(),
                b.toolbar_opacity
            )
        })
        .unwrap_or_else(|| "transparent".to_owned());
    css.push_str(&format!(
        "{w} headerbar, {w} tabbar, {w} tabbar > revealer > box {{ background-color: {tint}; background-image: none; box-shadow: none; }}\n"
    ));
    // サイドバー: どちらの型でも透ける。
    css.push_str(&format!(
        "{w} .amonhen-sidebar, {w} .amonhen-sidebar .navigation-sidebar, {w} .amonhen-sidebar scrolledwindow, \
         {w} .amonhen-sidebar .view, {w} .amonhen-sidebar list {{ background-color: transparent; background-image: none; }}\n"
    ));
    match b.style {
        // 読む面は 1 つの不透明な色。
        Style::Chrome => css.push_str(&opaque),
        // 読む面は自分では塗らない（地が見える）。
        Style::Single => css.push_str(&format!(
            "{w} .amonhen-pages {{ background-color: transparent; background-image: none; }}\n"
        )),
        Style::System => {}
    }
    css.push_str(&panes);
    css
}

/// `--snapshot` 用に、窓の背景（CSS で窓に描いているもの）を `snapshot` に描く。System なら何もせず false。
pub fn snapshot_background(window: &gtk::Window, snapshot: &gtk::Snapshot, w: f32, h: f32) -> bool {
    let b = of(window);
    if b.style == Style::System {
        return false;
    }
    #[allow(deprecated)]
    let color = |name: &str| {
        window
            .style_context()
            .lookup_color(name)
            .unwrap_or(gdk::RGBA::WHITE)
    };
    let (view, base) = (color("view_bg_color"), color("window_bg_color"));
    let bounds = graphene::Rect::new(0.0, 0.0, w, h);
    let picture = b
        .image
        .as_deref()
        .and_then(|path| gdk::Texture::from_filename(path).ok());
    let with_alpha = |c: gdk::RGBA, a: f32| gdk::RGBA::new(c.red(), c.green(), c.blue(), a);
    let ground = with_alpha(view, b.content_opacity as f32);
    let see_through = picture.is_none() && can_see_through();
    let fill = match (b.style, see_through) {
        (Style::Single, true) => ground,
        (Style::Single, false) if picture.is_none() => view,
        (Style::Chrome, true) => with_alpha(base, CHROME_SEE_THROUGH),
        _ => base,
    };
    snapshot.append_color(&fill, &bounds);
    if let Some(texture) = picture {
        let (iw, ih) = (texture.width() as f32, texture.height() as f32);
        let scale = if b.placement == Placement::Fill {
            (w / iw).max(h / ih)
        } else {
            1f32.min(w / iw).min(h / ih)
        };
        let (sw, sh) = (iw * scale, ih * scale);
        let (x, y) = match b.placement {
            Placement::Fill => ((w - sw) / 2.0, (h - sh) / 2.0),
            Placement::TopLeft => (0.0, 0.0),
            Placement::TopRight => (w - sw, 0.0),
            Placement::BottomLeft => (0.0, h - sh),
            Placement::BottomRight => (w - sw, h - sh),
        };
        snapshot.push_clip(&bounds);
        snapshot.append_texture(&texture, &graphene::Rect::new(x, y, sw, sh));
        snapshot.pop();
        if b.style == Style::Single {
            snapshot.append_color(&ground, &bounds);
        }
    }
    true
}
