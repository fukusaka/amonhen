//! どの FE でも同じ形のアイコン（`crates/icons`、`docs/icons.md`）を WinUI で描く。WinUI のアイコンの部品
//! （`PathIcon`・`FontIcon`）は塗るだけなので、形ごとに XAML の `Path`（線を引く図形）を重ねる。線は Segoe Fluent Icons
//! と同じ 1px で、縦・横の線は画素に合わせる。色は文字の色（テーマに従う）。

use std::cell::Cell;

use windows_reactor::*;

/// 線の太さ。Segoe Fluent Icons の線（100% で 1px）に合わせる。
pub const STROKE: f32 = 1.0;

thread_local! {
    /// 窓の画面の倍率（100% なら 1）。縦・横の線を画素に合わせるのに使う。
    static DISPLAY_SCALE: Cell<f32> = const { Cell::new(1.0) };
}

/// 窓の画面の倍率を覚える（History の描画の大きさを決めるときに知らされる）。
pub fn set_display_scale(scale: f32) {
    if scale > 0.0 {
        DISPLAY_SCALE.set(scale);
    }
}

/// 共通のアイコン `name` を一辺 `size` で。ないアイコンなら何も出さない。
pub fn icon(name: &str, size: f32) -> View {
    let Some(icon) = amonhen_icons::get(name) else {
        return View::empty();
    };
    let strokes = icon
        .strokes(size, DISPLAY_SCALE.get(), STROKE)
        .into_iter()
        .enumerate()
        .map(|(i, stroke)| {
            let mut path = Path::new()
                .data(stroke.markup())
                .stroke(Brush::Theme(ThemeBrush::PrimaryText))
                .stroke_thickness(f64::from(STROKE))
                .round_ends(true);
            if !stroke.dash.is_empty() {
                // XAML の破線は線の太さを 1 とした長さ。
                let dash: Vec<String> = stroke
                    .dash
                    .iter()
                    .map(|d| format!("{}", d / STROKE))
                    .collect();
                path = path.stroke_dash_array(dash.join(" "));
            }
            (i.to_string(), View::from(path))
        });
    Grid::new()
        .width(f64::from(size))
        .height(f64::from(size))
        .children((View::keyed_fragment(strokes),))
}

/// 共通のアイコン `name` を一辺 `size` で、XAML の文字に（`XamlReader` で読む部品の中に入れる。サイドバーの行）。
/// `color` はブラシ（`#rrggbb` か `{ThemeResource …}`）。ないアイコンなら何も描かない空の部品。
pub fn xaml(name: &str, size: f32, color: &str) -> String {
    let mut paths = String::new();
    if let Some(icon) = amonhen_icons::get(name) {
        for stroke in icon.strokes(size, DISPLAY_SCALE.get(), STROKE) {
            let dash = if stroke.dash.is_empty() {
                String::new()
            } else {
                let dash: Vec<String> = stroke
                    .dash
                    .iter()
                    .map(|d| format!("{}", d / STROKE))
                    .collect();
                format!(r#" StrokeDashArray="{}""#, dash.join(" "))
            };
            paths.push_str(&format!(
                r#"<Path Data="{}" Stroke="{color}" StrokeThickness="{STROKE}" StrokeStartLineCap="Round" StrokeEndLineCap="Round" StrokeDashCap="Round" StrokeLineJoin="Round"{dash}/>"#,
                stroke.markup()
            ));
        }
    }
    format!(r#"<Grid Width="{size}" Height="{size}">{paths}</Grid>"#)
}
