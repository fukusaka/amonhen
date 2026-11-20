//! 窓の背景（macOS 版の `Backdrop.swift`、GTK4 版の `backdrop.rs`）: 背景画像とその位置・透過率、ツールバーの
//! 色。アプリの設定（Settings > Appearance、`windows.json` の鍵は macOS 版の UserDefaults と同じ）と、リポジトリ
//! ごとの上書き（ブックマークの `backdrop`。形は macOS 版・GTK4 版と同じ）。
//!
//! 窓の後ろを透かす場所・透過率・ぼかしは、記録の形を保つために読み書きするが、この FE ではまだ当てない。
//! 背景画像は窓の中身の上に薄く重ね（押す・引くは下へ通す）、ツールバーの色はメニューとツールバーの行の後ろに
//! 敷く。

use amonhen_model::tr;
use serde_json::{Map, Value, json};
use windows_reactor::Color;

const STYLE: &str = "backdropStyle";
const TRANSPARENCY: &str = "backdropTransparency";
const BLUR: &str = "backdropBlur";
const IMAGE: &str = "backdropImage";
const IMAGE_PLACEMENT: &str = "backdropImagePlacement";
const IMAGE_TRANSPARENCY: &str = "backdropImageTransparency";
const TOOLBAR_COLOR: &str = "backdropToolbarColor";
const TOOLBAR_OPACITY: &str = "backdropToolbarOpacity";

/// 背景画像の置き方。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Placement {
    /// 窓いっぱいに広げる（縦横の比は保ち、はみ出た分は切る）。
    #[default]
    Fill,
    /// 元の大きさで隅に置く。
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
}

/// 窓の背景の設定。アプリの設定と、リポジトリごとの上書きで同じ形。
#[derive(Clone, Debug, PartialEq)]
pub struct Backdrop {
    /// 窓の後ろを透かす場所（`system`・`chrome`・`single`）。記録を保つだけ。
    pub style: String,
    pub transparency: f64,
    pub blur: f64,
    /// 背景画像のパス。窓の中身の上に薄く重ねる。
    pub image: Option<String>,
    pub placement: Placement,
    /// 背景画像の透過率（0.5〜0.95）。
    pub image_transparency: f64,
    /// ツールバーに敷く色（`#rrggbb`）。
    pub toolbar_color: Option<String>,
    /// ツールバーの色の濃さ（0.1〜1）。
    pub toolbar_opacity: f64,
}

impl Default for Backdrop {
    fn default() -> Self {
        Self {
            style: "system".into(),
            transparency: 0.4,
            blur: 0.4,
            image: None,
            placement: Placement::Fill,
            image_transparency: 0.85,
            toolbar_color: None,
            toolbar_opacity: 0.35,
        }
    }
}

/// ツールバーの色を入れたときの最初の色（macOS 版・GTK4 版と同じ強調の色）。
pub const FIRST_TOOLBAR_COLOR: &str = "#3584e4";

impl Backdrop {
    /// アプリの設定。
    pub fn app() -> Self {
        let d = Self::default();
        let number = |key: &str, default: f64| crate::prefs::number(key).unwrap_or(default);
        let text = |key: &str| crate::prefs::string(key).filter(|s| !s.is_empty());
        Self {
            style: text(STYLE).unwrap_or(d.style),
            transparency: number(TRANSPARENCY, d.transparency),
            blur: number(BLUR, d.blur),
            image: text(IMAGE),
            placement: text(IMAGE_PLACEMENT)
                .and_then(|s| Placement::from_raw(&s))
                .unwrap_or(d.placement),
            image_transparency: number(IMAGE_TRANSPARENCY, d.image_transparency),
            toolbar_color: text(TOOLBAR_COLOR),
            toolbar_opacity: number(TOOLBAR_OPACITY, d.toolbar_opacity),
        }
    }

    /// アプリの設定にする（変わった鍵だけ書く）。
    pub fn set_app(&self) -> Result<(), String> {
        let old = Self::app();
        let set = |key: &str, value: Option<String>| match value {
            Some(v) => crate::prefs::set_string(key, &v),
            None => crate::prefs::remove(key),
        };
        if old.image != self.image {
            set(IMAGE, self.image.clone())?;
        }
        if old.placement != self.placement {
            set(IMAGE_PLACEMENT, Some(self.placement.raw().into()))?;
        }
        if old.image_transparency != self.image_transparency {
            set(
                IMAGE_TRANSPARENCY,
                Some(self.image_transparency.to_string()),
            )?;
        }
        if old.toolbar_color != self.toolbar_color {
            set(TOOLBAR_COLOR, self.toolbar_color.clone())?;
        }
        if old.toolbar_opacity != self.toolbar_opacity {
            set(TOOLBAR_OPACITY, Some(self.toolbar_opacity.to_string()))?;
        }
        Ok(())
    }

    /// `location` の窓で使う設定（リポジトリの上書きがあればそれ）。
    pub fn resolved(location: &str) -> Self {
        crate::repositories::backdrop(location)
            .and_then(|v| Self::from_json(&v))
            .unwrap_or_else(Self::app)
    }

    /// macOS 版の JSON（ブックマークの `backdrop`）。リポジトリだけの設定を書くときに使う（まだ書く口はない）。
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("style".into(), json!(self.style));
        m.insert("transparency".into(), json!(self.transparency));
        m.insert("blur".into(), json!(self.blur));
        m.insert("imagePlacement".into(), json!(self.placement.raw()));
        m.insert("imageTransparency".into(), json!(self.image_transparency));
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
            style: v["style"].as_str().map_or(d.style, str::to_owned),
            transparency: v["transparency"].as_f64().unwrap_or(d.transparency),
            blur: v["blur"].as_f64().unwrap_or(d.blur),
            image: v["image"].as_str().map(str::to_owned),
            placement: v["imagePlacement"]
                .as_str()
                .and_then(Placement::from_raw)
                .unwrap_or(d.placement),
            image_transparency: v["imageTransparency"]
                .as_f64()
                .unwrap_or(d.image_transparency),
            toolbar_color: v["toolbarColor"].as_str().map(str::to_owned),
            toolbar_opacity: v["toolbarOpacity"].as_f64().unwrap_or(d.toolbar_opacity),
        })
    }

    /// ツールバーに敷く色（濃さを含めて。色がなければ None）。
    pub fn toolbar_tint(&self) -> Option<Color> {
        let (r, g, b) = parse_hex(self.toolbar_color.as_deref()?)?;
        let a = (self.toolbar_opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
        Some(Color::argb(a, r, g, b))
    }

    /// 背景画像の濃さ。
    pub fn image_opacity(&self) -> f64 {
        1.0 - self.image_transparency.clamp(0.0, 1.0)
    }
}

/// Settings > Appearance の窓の背景の欄の操作。
#[derive(Clone, Debug)]
pub enum Event {
    ChooseImage,
    RemoveImage,
    Placement(Option<usize>),
    ImageTransparency(f64),
    Tint(bool),
    ToolbarColor(Color),
    ToolbarOpacity(f64),
}

/// 欄の操作をアプリの設定に当てて書く（書けなければ理由）。
pub fn apply(event: Event) -> Result<(), String> {
    let mut b = Backdrop::app();
    // 横棒は 5% ずつ（GTK4 版と同じ）。
    let step = |v: f64| (v * 20.0).round() / 20.0;
    match event {
        Event::ChooseImage => {
            let Some(path) = crate::files::open_image(&tr!("Background Image"), &tr!("Image"))
            else {
                return Ok(());
            };
            b.image = Some(path.display().to_string());
        }
        Event::RemoveImage => b.image = None,
        Event::Placement(Some(i)) => b.placement = Placement::ALL[i.min(Placement::ALL.len() - 1)],
        Event::Placement(None) => return Ok(()),
        Event::ImageTransparency(v) => b.image_transparency = step(v / 100.0),
        Event::Tint(on) => {
            if on != b.toolbar_color.is_some() {
                b.toolbar_color = on.then(|| FIRST_TOOLBAR_COLOR.to_owned());
            }
        }
        Event::ToolbarColor(c) => {
            if b.toolbar_color.is_some() {
                b.toolbar_color = Some(to_hex(c));
            }
        }
        Event::ToolbarOpacity(v) => b.toolbar_opacity = step(v / 100.0),
    }
    if b == Backdrop::app() {
        return Ok(());
    }
    b.set_app()?;
    changed();
    Ok(())
}

/// [`current`] の前の結果の形。
type Cached = (String, Option<std::time::SystemTime>, u32, Backdrop);

thread_local! {
    /// [`current`] の前の結果（場所、bookmarks.json の更新時刻、設定の版、結果）。
    static CACHE: std::cell::RefCell<Option<Cached>> =
        const { std::cell::RefCell::new(None) };
    /// アプリの設定を書いた数（書いたら読み直す）。
    static VERSION: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// `location` の窓で使う設定（描くたびに呼ぶので、変わっていなければ前の結果）。
pub fn current(location: &str) -> Backdrop {
    let modified = crate::repositories::modified();
    let version = VERSION.with(std::cell::Cell::get);
    if let Some(b) = CACHE.with(|c| {
        c.borrow()
            .as_ref()
            .filter(|(l, m, v, _)| l == location && *m == modified && *v == version)
            .map(|(.., b)| b.clone())
    }) {
        return b;
    }
    let b = Backdrop::resolved(location);
    CACHE.with(|c| *c.borrow_mut() = Some((location.to_owned(), modified, version, b.clone())));
    b
}

/// アプリの設定を変えた（次に描くときに読み直す）。
pub fn changed() {
    VERSION.with(|v| v.set(v.get().wrapping_add(1)));
}

/// `#rrggbb` を読む。
pub fn parse_hex(text: &str) -> Option<(u8, u8, u8)> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some((channel(0)?, channel(2)?, channel(4)?))
}

/// `#rrggbb` に書く。
pub fn to_hex(color: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_macos_record() {
        let v: Value = serde_json::from_str(
            r##"{"blur":0.2,"image":"C:/i.png","imagePlacement":"bottomRight","imageTransparency":0.9,"style":"single","toolbarColor":"#3b5bdb","toolbarOpacity":0.35,"transparency":0.6}"##,
        )
        .unwrap();
        let b = Backdrop::from_json(&v).unwrap();
        assert_eq!(b.placement, Placement::BottomRight);
        assert_eq!(b.image.as_deref(), Some("C:/i.png"));
        assert_eq!(b.style, "single");
        assert_eq!(b.toolbar_tint(), Some(Color::argb(89, 0x3b, 0x5b, 0xdb)));
        assert_eq!(Backdrop::from_json(&b.to_json()), Some(b));
        // 後から足した項目がない記録。
        let old: Value = serde_json::from_str(r#"{"style":"chrome"}"#).unwrap();
        let b = Backdrop::from_json(&old).unwrap();
        assert_eq!(b.image_transparency, 0.85);
        assert_eq!(b.toolbar_tint(), None);
    }

    #[test]
    fn hex_colors_round_trip() {
        assert_eq!(parse_hex("#3584e4"), Some((0x35, 0x84, 0xe4)));
        assert_eq!(parse_hex("3584e4"), None);
        assert_eq!(to_hex(Color::rgb(0x35, 0x84, 0xe4)), "#3584e4");
    }
}
