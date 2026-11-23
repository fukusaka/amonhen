//! モバイル向けの描画部品で SVG がどう見えるか（History の「すべてのファイル」の Mobile）。
//!
//! Android と flutter_svg は、本物の取り込みを移したもの（`svg2vector`・`vgc`）で変換し、その結果を CoreSVG・librsvg
//! で描ける SVG に戻す（`svgandroid`・`svgflutter`）。どちらも 1 ms 前後で終わるので、画面は近似を挟まずにこの結果を
//! そのまま出す。iOS（CoreSVG）と react-native-svg は、資料から作った近似（`svgsim`）。

use amonhen_model::tr;

use crate::svglint::Target;

/// ある部品での見え方。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preview {
    /// 描ける SVG。部品が読めない SVG なら `None`（理由は `error`）。
    pub svg: Option<Vec<u8>>,
    /// 絵の見出し（部品の名前。近似なら「（近似）」を付ける）。
    pub title: String,
    /// 絵の作り方の説明（1 行）。
    pub caption: String,
    /// 取り込みが出した警告と、SVG に戻すときに同じに描けなかったこと（取り込みの警告は英語のまま）。
    pub messages: Vec<String>,
    /// 部品が SVG を読めなかった理由（取り込みのエラーは英語のまま）。
    pub error: Option<String>,
    /// 本物の取り込みを移したものか（`false` なら近似）。
    pub exact: bool,
}

/// `svg` が `target` でどう見えるか。
pub fn preview(svg: &[u8], target: Target) -> Preview {
    match target {
        Target::Android => android(svg),
        Target::Flutter => flutter(svg),
        Target::Ios | Target::ReactNative => Preview {
            svg: Some(crate::svgsim::approximate(svg, target)),
            title: tr!("{name} (approximation)", name = target.name()),
            caption: crate::svgsim::description(target),
            messages: Vec::new(),
            error: None,
            exact: false,
        },
    }
}

fn android(svg: &[u8]) -> Preview {
    let mut preview = Preview {
        svg: None,
        title: Target::Android.name().to_owned(),
        caption: tr!(
            "Android Studio’s import (Svg2Vector, sdk-common {version}) turned back into SVG and drawn by this app",
            version = crate::svg2vector::VERSION
        ),
        messages: Vec::new(),
        error: None,
        exact: true,
    };
    let converted = match crate::svg2vector::convert(svg) {
        Ok(converted) => converted,
        Err(error) => {
            preview.error = Some(tr!(
                "Android Studio can’t import this SVG: {error}",
                error = error
            ));
            return preview;
        }
    };
    preview.messages = lines(&converted.messages);
    if converted.xml.trim().is_empty() {
        // 取り込みは描くものがないと空を返す（理由は messages に）
        preview.error = Some(tr!("Android Studio makes nothing to draw from this SVG"));
        return preview;
    }
    match crate::svgandroid::vector_drawable_to_svg_with_notes(&converted.xml) {
        Ok((out, notes)) => {
            preview.svg = Some(out.into_bytes());
            preview.messages.extend(notes);
        }
        Err(error) => preview.error = Some(error.to_string()),
    }
    preview
}

fn flutter(svg: &[u8]) -> Preview {
    let mut preview = Preview {
        svg: None,
        title: Target::Flutter.name().to_owned(),
        caption: tr!(
            "flutter_svg’s compiler (vector_graphics_compiler {version}) turned back into SVG and drawn by this app",
            version = crate::vgc::VERSION
        ),
        messages: Vec::new(),
        error: None,
        exact: true,
    };
    let compiled = match crate::vgc::compile(svg) {
        Ok(compiled) => compiled,
        Err(error) => {
            preview.error = Some(tr!(
                "flutter_svg can’t load this SVG: {error}",
                error = error
            ));
            return preview;
        }
    };
    preview.messages = compiled.warnings;
    match crate::svgflutter::vec_to_svg(&compiled.vec) {
        Ok(out) => preview.svg = Some(out.into_bytes()),
        Err(error) => preview.error = Some(error.to_string()),
    }
    preview
}

fn lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ICON: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><filter id="f"><feGaussianBlur stdDeviation="1"/></filter><path d="M2 2h20v20H2z" fill="#c00" filter="url(#f)"/></svg>"##;

    #[test]
    fn android_and_flutter_use_the_ports() {
        let android = preview(ICON, Target::Android);
        assert!(android.exact && android.error.is_none());
        let svg = String::from_utf8(android.svg.unwrap()).unwrap();
        assert!(svg.contains("<path") && !svg.contains("filter"), "{svg}");
        assert!(
            android.messages.iter().any(|m| m.contains("<filter>")),
            "{:?}",
            android.messages
        );

        let flutter = preview(ICON, Target::Flutter);
        assert!(flutter.exact && flutter.error.is_none());
        assert!(
            String::from_utf8(flutter.svg.unwrap())
                .unwrap()
                .contains("<path")
        );
    }

    #[test]
    fn failures_carry_the_reason() {
        let android = preview(
            br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 9 9"><circle cx="1in" r="2"/></svg>"#,
            Target::Android,
        );
        assert!(android.svg.is_none());
        assert!(
            android
                .error
                .as_deref()
                .unwrap()
                .contains("NumberFormatException"),
            "{:?}",
            android.error
        );
        let flutter = preview(b"<svg><rect/></svg>", Target::Flutter);
        assert!(flutter.svg.is_none() && flutter.error.is_some());
    }

    #[test]
    fn ios_and_react_native_are_approximations() {
        let ios = preview(ICON, Target::Ios);
        assert!(!ios.exact && ios.svg.is_some() && ios.title.contains("iOS"));
    }
}
