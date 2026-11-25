//! History の差分の、画像の前と後の比較（macOS 版の `ImageDiffPanel`、GTK4 版の `imagediff.rs`）。開いた画像の
//! ファイルの見出しの下に置く欄で、並べる（2-Up）・スワイプ・オニオンスキン・違うピクセルを切り替える。比べ方は
//! 窓を閉じるまで覚える（次に開く画像も同じ比べ方）。拡大縮小は帯のボタンで、拡大している間はドラッグ
//! （スワイプ・オニオンスキンでは Alt を押しながら）で絵を動かす。
//!
//! 欄は差分の欄（Direct2D で描く）の中に描く。ここは形（読み込み・違うピクセル・置き方・押した所）だけを持ち、
//! 描くのは `render.rs`、押したときの処理は `app.rs`。

use std::cell::Cell;

use amonhen_model::tr;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
    WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};

/// 欄の高さ（差分の行の数。18 DIP の行で 306 DIP。macOS 版の 300pt）。
pub const PANEL_LINES: usize = 17;
/// 帯（比べ方・拡大縮小のボタン）の高さと、下の説明の高さ。
const BAR_HEIGHT: f32 = 26.0;
const CAPTION_HEIGHT: f32 = 18.0;
const MARGIN: f32 = 12.0;

fn extension(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// 比べる欄で見せるファイル（WIC が読める画像と SVG）。
pub fn is_picture(path: &str) -> bool {
    is_image(path) || is_svg(path)
}

pub fn is_svg(path: &str) -> bool {
    extension(path) == "svg"
}

/// SVG の絵の大きさ（ルートの `width`・`height`、なければ `viewBox`。どちらもなければ 300 × 150。大きすぎれば
/// 2048 に収める）。
pub fn svg_size(bytes: &[u8]) -> (u32, u32) {
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(8192)]);
    let Some(start) = text.find("<svg") else {
        return (300, 150);
    };
    let tag = &text[start..text[start..].find('>').map_or(text.len(), |e| start + e)];
    let attr = |name: &str| -> Option<String> {
        let at = tag.find(&format!(" {name}="))? + name.len() + 2;
        let quote = tag[at..].chars().next()?;
        let rest = &tag[at + 1..];
        Some(rest[..rest.find(quote)?].to_owned())
    };
    let number = |v: &str| -> Option<f32> {
        let v = v.trim().trim_end_matches("px");
        v.parse::<f32>().ok().filter(|n| *n > 0.0)
    };
    let view_box = attr("viewBox").and_then(|v| {
        let n: Vec<f32> = v
            .split([' ', ','])
            .filter_map(|p| p.trim().parse().ok())
            .collect();
        (n.len() == 4 && n[2] > 0.0 && n[3] > 0.0).then(|| (n[2], n[3]))
    });
    let (mut w, mut h) = match (
        attr("width").as_deref().and_then(number),
        attr("height").as_deref().and_then(number),
    ) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (w, view_box.map_or(150.0, |(vw, vh)| w * vh / vw)),
        (None, Some(h)) => (view_box.map_or(300.0, |(vw, vh)| h * vw / vh), h),
        (None, None) => view_box.unwrap_or((300.0, 150.0)),
    };
    let largest = w.max(h);
    if largest > 2048.0 {
        w = w * 2048.0 / largest;
        h = h * 2048.0 / largest;
    }
    (w.round().max(1.0) as u32, h.round().max(1.0) as u32)
}

/// WIC が読める画像。
pub fn is_image(path: &str) -> bool {
    matches!(
        extension(path).as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "tif" | "tiff" | "ico" | "webp" | "jxr"
    )
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
    pub const ALL: [Mode; 4] = [Mode::TwoUp, Mode::Swipe, Mode::Onion, Mode::Difference];

    pub fn title(self) -> String {
        match self {
            Mode::TwoUp => tr!("2-Up"),
            Mode::Swipe => tr!("Swipe"),
            Mode::Onion => tr!("Onion Skin"),
            Mode::Difference => tr!("Difference"),
        }
    }
}

thread_local! {
    /// 窓を閉じるまで覚える比べ方（macOS 版・GTK4 版と同じ）。
    static MODE: Cell<Mode> = const { Cell::new(Mode::TwoUp) };
}

pub fn mode() -> Mode {
    MODE.with(Cell::get)
}

pub fn set_mode(mode: Mode) {
    MODE.with(|m| m.set(mode));
}

/// 1 つの欄の見せ方（スワイプ・オニオンスキンの位置、倍率と、拡大したときのずらし）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub position: f32,
    /// 画像の 1 ピクセルを何 DIP で描くか。None なら枠に合わせる。
    pub zoom: Option<f32>,
    pub pan: (f32, f32),
    /// 透けた絵の下の市松模様を暗くするか（None なら絵の色から決めた既定。帯のボタンで替える）。
    pub dark: Option<bool>,
}

impl Default for View {
    fn default() -> Self {
        Self {
            position: 0.5,
            zoom: None,
            pan: (0.0, 0.0),
            dark: None,
        }
    }
}

/// 読んだ画像（乗算済みの BGRA）。`scale` は絵の 1 ピクセルあたりのピクセルの数（SVG は細かく描くので 1 より大きい）。
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
    pub scale: u32,
}

impl Pixels {
    /// 絵としての大きさ（SVG なら描いた細かさで割ったもの）。
    pub fn size(&self) -> (f32, f32) {
        let s = self.scale.max(1) as f32;
        (self.width as f32 / s, self.height as f32 / s)
    }
}

/// 画像のバイト列を WIC で読む（最初のフレーム）。読めなければ None。
pub fn decode(bytes: &[u8]) -> Option<Pixels> {
    if bytes.is_empty() {
        return None;
    }
    let read = || -> windows::core::Result<Pixels> {
        unsafe {
            let factory: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            let stream = factory.CreateStream()?;
            stream.InitializeFromMemory(bytes)?;
            let decoder = factory.CreateDecoderFromStream(
                &stream,
                std::ptr::null(),
                WICDecodeMetadataCacheOnDemand,
            )?;
            let frame = decoder.GetFrame(0)?;
            let converter = factory.CreateFormatConverter()?;
            converter.Initialize(
                &frame,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )?;
            let (mut width, mut height) = (0, 0);
            converter.GetSize(&mut width, &mut height)?;
            let mut data = vec![0u8; width as usize * height as usize * 4];
            converter.CopyPixels(std::ptr::null(), width * 4, &mut data)?;
            Ok(Pixels {
                width,
                height,
                data,
                scale: 1,
            })
        }
    };
    read().ok().filter(|p| p.width > 0 && p.height > 0)
}

/// バイト数（「1.2 kB」。GTK4 版の `glib::format_size` と同じ 1000 区切り）。
pub fn format_size(bytes: usize) -> String {
    let b = bytes as f64;
    if b < 1000.0 {
        format!("{bytes} B")
    } else if b < 1_000_000.0 {
        format!("{:.1} kB", b / 1000.0)
    } else {
        format!("{:.1} MB", b / 1_000_000.0)
    }
}

/// 画像の大きさとバイト数（「64 × 48 px · 1.2 kB」）。なければ「なし」。
pub fn describe(bytes: Option<&[u8]>, pixels: Option<&Pixels>) -> String {
    let Some(bytes) = bytes else {
        return tr!("None");
    };
    let size = format_size(bytes.len());
    match pixels {
        Some(p) => {
            let (w, h) = p.size();
            format!("{} × {} px · {size}", w.round(), h.round())
        }
        None => size,
    }
}

/// 前と後の説明（「Before: … → After: …」）。
pub fn caption(before: String, after: String) -> String {
    format!(
        "{}: {before}    →    {}: {after}",
        tr!("Before"),
        tr!("After")
    )
}

/// 違うピクセルの数の説明（「N of M pixels differ (x%)」）。
pub fn difference_summary(changed: usize, total: usize) -> String {
    if total == 0 {
        return tr!("Nothing to compare");
    }
    tr!(
        "{changed} of {total} pixels differ ({percent}%)",
        changed = changed,
        total = total,
        percent = format!("{:.1}", changed as f64 * 100.0 / total as f64)
    )
}

/// 違うピクセルを赤紫で、同じところは後の画像を薄く（左上にそろえて大きい方の大きさ。違う数と全体の数も。
/// GTK4 版の `make_difference`）。
pub fn difference(
    before: Option<&Pixels>,
    after: Option<&Pixels>,
) -> (Option<Pixels>, usize, usize) {
    let size = |p: Option<&Pixels>| p.map_or((0, 0), |p| (p.width as usize, p.height as usize));
    let (a, b) = (size(before), size(after));
    let (w, h) = (a.0.max(b.0), a.1.max(b.1));
    if w == 0 || h == 0 {
        return (None, 0, 0);
    }
    let at = |p: Option<&Pixels>, x: usize, y: usize| -> [u8; 4] {
        match p {
            Some(p) if x < p.width as usize && y < p.height as usize => {
                let i = (y * p.width as usize + x) * 4;
                [p.data[i], p.data[i + 1], p.data[i + 2], p.data[i + 3]]
            }
            _ => [0; 4],
        }
    };
    let mut data = vec![0u8; w * h * 4];
    let mut changed: usize = 0;
    for y in 0..h {
        for x in 0..w {
            let (old, new) = (at(before, x, y), at(after, x, y));
            let i = (y * w + x) * 4;
            if old != new {
                changed += 1;
                // BGRA の赤紫（R 255・G 0・B 200）。
                data[i..i + 4].copy_from_slice(&[200, 0, 255, 255]);
            } else {
                let gray = ((new[0] as u32 + new[1] as u32 + new[2] as u32) / 3 / 3) as u8;
                data[i..i + 4].copy_from_slice(&[gray, gray, gray, 255]);
            }
        }
    }
    let scale = before.or(after).map_or(1, |p| p.scale);
    let pixels = Pixels {
        scale,
        width: w as u32,
        height: h as u32,
        data,
    };
    // 数は絵のピクセルで（SVG は細かく描いた分を割り戻す）。
    let per = (scale * scale).max(1) as usize;
    (Some(pixels), changed.div_ceil(per), (w * h) / per)
}

/// 欄の中の四角（DIP）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    fn inset(&self, dx: f32, dy: f32) -> Rect {
        Rect {
            x: self.x + dx,
            y: self.y + dy,
            w: (self.w - dx * 2.0).max(0.0),
            h: (self.h - dy * 2.0).max(0.0),
        }
    }
}

/// 欄の置き方: 帯の比べ方のボタン・スライダー・拡大縮小のボタン、絵の枠、下の説明。
pub struct Layout {
    pub modes: [Rect; 4],
    /// SVG の「テキストで見る」（SVG のときだけ）。
    pub text: Option<Rect>,
    /// スワイプ・オニオンスキンの位置のスライダー（そのときだけ）。
    pub slider: Option<Rect>,
    /// 倍率（押すと 100% と合わせるを切り替える）・縮める・広げる・合わせる。
    pub zoom_label: Rect,
    /// 市松模様の明暗の切り替え（透けた絵のときだけ出す）。
    pub checker: Rect,
    pub zoom_out: Rect,
    pub zoom_in: Rect,
    pub zoom_fit: Rect,
    pub canvas: Rect,
    pub caption: Rect,
}

/// 幅 `width`・上端 `top` の欄の置き方。
pub fn layout(top: f32, width: f32, mode: Mode, svg: bool) -> Layout {
    let height = PANEL_LINES as f32 * crate::render::DIFF_ROW_HEIGHT;
    let bar_y = top + 8.0;
    let mut x = MARGIN;
    let mut modes = [Rect::default(); 4];
    for (i, m) in Mode::ALL.into_iter().enumerate() {
        let w = match m {
            Mode::TwoUp => 52.0,
            Mode::Swipe => 60.0,
            Mode::Onion => 92.0,
            Mode::Difference => 84.0,
        };
        modes[i] = Rect {
            x,
            y: bar_y,
            w,
            h: BAR_HEIGHT - 4.0,
        };
        x += w;
    }
    let text = svg.then_some(Rect {
        x: x + 10.0,
        y: bar_y,
        w: 28.0,
        h: BAR_HEIGHT - 4.0,
    });
    let right = width - MARGIN;
    let button = |x: f32, w: f32| Rect {
        x,
        y: bar_y,
        w,
        h: BAR_HEIGHT - 4.0,
    };
    let zoom_fit = button(right - 28.0, 28.0);
    let zoom_in = button(zoom_fit.x - 28.0, 28.0);
    let zoom_out = button(zoom_in.x - 28.0, 28.0);
    let zoom_label = button(zoom_out.x - 56.0, 52.0);
    let checker = button(zoom_label.x - 32.0, 28.0);
    let slider = matches!(mode, Mode::Swipe | Mode::Onion).then(|| Rect {
        x: checker.x - 172.0,
        y: bar_y,
        w: 160.0,
        h: BAR_HEIGHT - 4.0,
    });
    let canvas_top = bar_y + BAR_HEIGHT + 4.0;
    let caption_top = top + height - CAPTION_HEIGHT - 6.0;
    Layout {
        modes,
        text,
        slider,
        zoom_label,
        checker,
        zoom_out,
        zoom_in,
        zoom_fit,
        canvas: Rect {
            x: MARGIN,
            y: canvas_top,
            w: (width - MARGIN * 2.0).max(0.0),
            h: (caption_top - canvas_top - 4.0).max(0.0),
        },
        caption: Rect {
            x: MARGIN,
            y: caption_top,
            w: (width - MARGIN * 2.0).max(0.0),
            h: CAPTION_HEIGHT,
        },
    }
}

/// すべてのファイルで開いた画像の置き方（欄いっぱい。比べ方のボタンとスライダーはない）。
pub fn single_layout(width: f32, height: f32) -> Layout {
    let mut layout = layout(0.0, width, Mode::TwoUp, false);
    layout.modes = [Rect::default(); 4];
    layout.caption.y = height - CAPTION_HEIGHT - 6.0;
    layout.canvas.h = (layout.caption.y - layout.canvas.y - 4.0).max(0.0);
    layout
}

/// 枠に合わせるときの倍率（小さな絵は整数倍まで拡げ、ぼかさない）。
fn fit_scale(size: (f32, f32), area: Rect) -> f32 {
    if size.0 <= 0.0 || size.1 <= 0.0 {
        return 1.0;
    }
    let mut scale = (area.w / size.0).min(area.h / size.1);
    if scale > 1.0 {
        scale = scale.min(8.0).floor().max(1.0);
    }
    scale
}

/// `size` の絵を `area` に置く: 倍率を決めていなければ収め、決めていればその倍率で中心からずらして。
pub fn fit(size: (f32, f32), area: Rect, view: &View) -> Rect {
    if size.0 <= 0.0 || size.1 <= 0.0 {
        return Rect::default();
    }
    let scale = view.zoom.unwrap_or_else(|| fit_scale(size, area));
    let (dx, dy) = if view.zoom.is_some() {
        view.pan
    } else {
        (0.0, 0.0)
    };
    let (w, h) = ((size.0 * scale).round(), (size.1 * scale).round());
    Rect {
        x: (area.x + (area.w - w) / 2.0 + dx).round(),
        y: (area.y + (area.h - h) / 2.0 + dy).round(),
        w,
        h,
    }
}

/// 並べるときの、前（0）と後（1）の枠（上に見出しの分を空ける）。
pub fn two_up_area(canvas: Rect, side: usize) -> Rect {
    let half = ((canvas.w - 16.0) / 2.0).max(0.0);
    Rect {
        x: canvas.x + side as f32 * (half + 16.0),
        y: canvas.y + 16.0,
        w: half,
        h: (canvas.h - 16.0).max(0.0),
    }
    .inset(4.0, 4.0)
}

/// 重ねるときの枠（見出しを左右に置く分を空ける）。
pub fn overlay_area(canvas: Rect) -> Rect {
    Rect {
        x: canvas.x + 60.0,
        y: canvas.y + 4.0,
        w: (canvas.w - 120.0).max(0.0),
        h: (canvas.h - 8.0).max(0.0),
    }
}

/// 重ねる絵の中の、その画像の位置（左上にそろえる）。
pub fn fitted(common: (f32, f32), size: (f32, f32), rect: Rect) -> Rect {
    if common.0 <= 0.0 {
        return rect;
    }
    let scale = rect.w / common.0;
    Rect {
        x: rect.x,
        y: rect.y,
        w: size.0 * scale,
        h: size.1 * scale,
    }
}

/// 倍率を `factor` 倍に（合わせているときは、その倍率から）。0.1〜32 倍。
pub fn zoom_by(view: &mut View, factor: f32, size: (f32, f32), area: Rect) {
    let current = view.zoom.unwrap_or_else(|| fit_scale(size, area));
    view.zoom = Some((current * factor).clamp(0.1, 32.0));
}

/// 市松模様を暗くするか: 帯のボタンで替えていればそれ、でなければ絵の色から決めた既定（macOS 版・GTK4 版と同じ。
/// 確かめる用に `AMH_CHECKERBOARD=dark|light`）。
pub fn dark_checker(view: &View, tone: amonhen_core::checker::Tone) -> bool {
    view.dark
        .unwrap_or_else(|| match std::env::var("AMH_CHECKERBOARD").as_deref() {
            Ok("dark") => true,
            Ok("light") => false,
            _ => tone.dark(),
        })
}

/// 読んだ絵（乗算済みの BGRA）の明るさを数える（比べるときは両方を合わせる）。
pub fn tone(pixels: &[Option<&Pixels>]) -> amonhen_core::checker::Tone {
    let mut tone = amonhen_core::checker::Tone::default();
    for p in pixels.iter().flatten() {
        tone.add(
            &p.data,
            p.width as usize,
            p.height as usize,
            p.width as usize * 4,
            amonhen_core::checker::Order::Bgra,
        );
    }
    tone
}

/// 帯の倍率の表示（「Fit」か「150%」）。
pub fn zoom_text(view: &View) -> String {
    match view.zoom {
        Some(z) => format!("{}%", (z * 100.0).round() as i64),
        None => tr!("Fit"),
    }
}

/// 欄の中で押した所。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    Mode(Mode),
    /// SVG をテキストの差分で見る。
    ShowText,
    /// スライダーか、重ねた絵（スワイプ・オニオンスキンの位置。0〜1）。
    Position(f32),
    ZoomActual,
    /// 市松模様の明暗を替える（透けた絵のときだけ。押した側で確かめる）。
    Checker,
    ZoomOut,
    ZoomIn,
    ZoomFit,
    /// 拡大した絵を動かし始める。
    Pan,
}

/// 欄の `(x, y)` を押した（`common` は前と後を重ねる大きさ、`alt` は Alt を押しているか）。
pub fn hit(
    layout: &Layout,
    mode: Mode,
    view: &View,
    common: (f32, f32),
    x: f32,
    y: f32,
    alt: bool,
) -> Option<Hit> {
    for (i, r) in layout.modes.iter().enumerate() {
        if r.contains(x, y) {
            return Some(Hit::Mode(Mode::ALL[i]));
        }
    }
    if layout.text.is_some_and(|r| r.contains(x, y)) {
        return Some(Hit::ShowText);
    }
    for (r, hit) in [
        (layout.zoom_label, Hit::ZoomActual),
        (layout.checker, Hit::Checker),
        (layout.zoom_out, Hit::ZoomOut),
        (layout.zoom_in, Hit::ZoomIn),
        (layout.zoom_fit, Hit::ZoomFit),
    ] {
        if r.contains(x, y) {
            return Some(hit);
        }
    }
    if let Some(slider) = layout.slider
        && slider.contains(x, y)
    {
        return Some(Hit::Position(((x - slider.x) / slider.w).clamp(0.0, 1.0)));
    }
    if !layout.canvas.contains(x, y) {
        return None;
    }
    let overlay = matches!(mode, Mode::Swipe | Mode::Onion);
    if overlay && !alt {
        let rect = fit(common, overlay_area(layout.canvas), view);
        if rect.w > 0.0 {
            return Some(Hit::Position(((x - rect.x) / rect.w).clamp(0.0, 1.0)));
        }
    }
    view.zoom.is_some().then_some(Hit::Pan)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_image_extensions() {
        assert!(is_image("a/b/icon.PNG"));
        assert!(is_image("photo.jpeg"));
        assert!(!is_image("drawing.svg"));
        assert!(is_picture("drawing.svg"));
        assert!(!is_image("README.md"));
    }

    #[test]
    fn difference_counts_differing_pixels_over_the_larger_size() {
        let one = |w: u32, h: u32, v: u8| Pixels {
            width: w,
            height: h,
            data: vec![v; (w * h * 4) as usize],
            scale: 1,
        };
        let (image, changed, total) = difference(Some(&one(2, 2, 10)), Some(&one(2, 1, 10)));
        // 2×2 のうち、後にない下の 2 ピクセルが違う。
        assert_eq!((changed, total), (2, 4));
        assert_eq!(image.map(|p| (p.width, p.height)), Some((2, 2)));
        assert_eq!(difference(None, None).2, 0);
    }

    #[test]
    fn reads_the_size_of_an_svg() {
        let svg = |s: &str| svg_size(s.as_bytes());
        assert_eq!(svg(r#"<svg width="64" height="48px">"#), (64, 48));
        assert_eq!(
            svg(r#"<?xml?><svg xmlns="x" viewBox="0 0 24 12">"#),
            (24, 12)
        );
        assert_eq!(svg(r#"<svg width="100" viewBox="0 0 50 25">"#), (100, 50));
        assert_eq!(svg("<svg>"), (300, 150));
        assert_eq!(svg(r#"<svg width="4096" height="1024">"#), (2048, 512));
    }

    #[test]
    fn fits_small_images_at_whole_multiples() {
        let area = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
        };
        let r = fit((10.0, 10.0), area, &View::default());
        assert_eq!((r.w, r.h), (50.0, 50.0));
        let r = fit((400.0, 100.0), area, &View::default());
        assert_eq!((r.w, r.h), (100.0, 25.0));
    }
}
