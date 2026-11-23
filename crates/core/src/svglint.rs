//! SVG の、モバイル向けの描画部品で崩れる機能を探す（History の「すべてのファイル」）。対象は Android の
//! VectorDrawable（Android Studio の取り込み）・iOS のアセットカタログ（CoreSVG）・flutter_svg・react-native-svg。
//! 規則はそれぞれの取り込みのソースと文書（2026-11 時点: Android Studio の Svg2Vector、vector_graphics_compiler
//! 1.3.6、react-native-svg 15.16）から。CoreSVG は公開された資料がないので、知られている崩れだけを挙げる
//! （macOS の FE は CoreSVG で描いた絵も並べる）。
//!
//! 要素と属性がわかればよいので、XML は小さく自前で読む（スクリプトや外の読み込みはしない）。

use std::collections::BTreeMap;

use amonhen_model::tr;

/// モバイル向けの描画部品。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Target {
    Android,
    Ios,
    Flutter,
    ReactNative,
}

impl Target {
    pub fn name(self) -> &'static str {
        match self {
            Self::Android => "Android VectorDrawable",
            Self::Ios => "iOS (CoreSVG)",
            Self::Flutter => "flutter_svg",
            Self::ReactNative => "react-native-svg",
        }
    }

    /// FE 向けの窓口・JSON の名前。
    pub fn key(self) -> &'static str {
        match self {
            Self::Android => "android",
            Self::Ios => "ios",
            Self::Flutter => "flutter",
            Self::ReactNative => "reactNative",
        }
    }
}

/// 崩れる機能 1 つ。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    /// 崩れる部品（並びは Android・iOS・Flutter・React Native）。
    pub targets: Vec<Target>,
    /// 機能（`<filter>`・`stroke-dasharray` など、SVG のまま）。
    pub feature: String,
    /// どう崩れるか（訳した文）。
    pub message: String,
    /// その機能を使っている数。
    pub count: usize,
}

struct Element {
    name: String,
    attrs: Vec<(String, String)>,
    parent: Option<usize>,
    children: usize,
}

impl Element {
    /// 属性か、`style` の中の同じ名前の宣言。
    fn prop(&self, key: &str) -> Option<String> {
        if let Some((_, v)) = self.attrs.iter().find(|(k, _)| k == key) {
            return Some(v.clone());
        }
        let style = self.attrs.iter().find(|(k, _)| k == "style")?;
        style.1.split(';').find_map(|decl| {
            let (k, v) = decl.split_once(':')?;
            (k.trim() == key).then(|| v.trim().to_owned())
        })
    }

    fn local(&self) -> &str {
        self.name.rsplit(':').next().unwrap_or(&self.name)
    }
}

/// 要素を順に読む（コメント・CDATA・処理命令・DOCTYPE は飛ばす）。壊れた XML でも読めたところまで。
fn parse(text: &str) -> Vec<Element> {
    let bytes = text.as_bytes();
    let mut out: Vec<Element> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut i = 0;
    let skip_to = |from: usize, end: &str| {
        text[from..]
            .find(end)
            .map_or(text.len(), |p| from + p + end.len())
    };
    while let Some(p) = text[i..].find('<') {
        i += p;
        let rest = &text[i..];
        if rest.starts_with("<!--") {
            i = skip_to(i + 4, "-->");
        } else if rest.starts_with("<![CDATA[") {
            i = skip_to(i + 9, "]]>");
        } else if rest.starts_with("<?") {
            i = skip_to(i + 2, "?>");
        } else if rest.starts_with("<!") {
            i = skip_to(i + 2, ">");
        } else if rest.starts_with("</") {
            stack.pop();
            i = skip_to(i + 2, ">");
        } else {
            i += 1;
            let start = i;
            while i < bytes.len()
                && !bytes[i].is_ascii_whitespace()
                && bytes[i] != b'>'
                && bytes[i] != b'/'
            {
                i += 1;
            }
            let name = text[start..i].to_owned();
            let mut attrs = Vec::new();
            let mut closed = false;
            loop {
                while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i >= bytes.len() {
                    break;
                }
                match bytes[i] {
                    b'>' => {
                        i += 1;
                        break;
                    }
                    b'/' => {
                        closed = true;
                        i += 1;
                        continue;
                    }
                    _ => {}
                }
                let k0 = i;
                while i < bytes.len()
                    && !bytes[i].is_ascii_whitespace()
                    && !matches!(bytes[i], b'=' | b'>' | b'/')
                {
                    i += 1;
                }
                let key = text[k0..i].to_owned();
                while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                let mut value = String::new();
                if i < bytes.len() && bytes[i] == b'=' {
                    i += 1;
                    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
                        let quote = bytes[i] as char;
                        let v0 = i + 1;
                        let end = text[v0..].find(quote).map_or(text.len(), |p| v0 + p);
                        value = text[v0..end].to_owned();
                        i = (end + 1).min(text.len());
                    }
                }
                if key.is_empty() {
                    i += 1;
                } else {
                    attrs.push((key, value));
                }
            }
            if name.is_empty() {
                continue;
            }
            let parent = stack.last().copied();
            if let Some(p) = parent {
                out[p].children += 1;
            }
            out.push(Element {
                name,
                attrs,
                parent,
                children: 0,
            });
            if !closed {
                stack.push(out.len() - 1);
            }
        }
    }
    out
}

/// 数を数えながら、同じ機能をまとめる。
#[derive(Default)]
struct Found {
    issues: BTreeMap<(String, String), (Vec<Target>, usize)>,
    order: Vec<(String, String)>,
}

impl Found {
    fn add(&mut self, targets: &[Target], feature: &str, message: String) {
        let key = (feature.to_owned(), message);
        let entry = self.issues.entry(key.clone()).or_insert_with(|| {
            self.order.push(key.clone());
            (targets.to_vec(), 0)
        });
        entry.1 += 1;
    }

    fn finish(mut self) -> Vec<Issue> {
        self.order
            .into_iter()
            .filter_map(|key| {
                let (mut targets, count) = self.issues.remove(&key)?;
                targets.sort();
                Some(Issue {
                    targets,
                    feature: key.0,
                    message: key.1,
                    count,
                })
            })
            .collect()
    }
}

/// フィルターの部品のうち、flutter_svg（vector_graphics_compiler 1.3.6）が描けるもの。ほかが 1 つでもあれば、
/// フィルター全体を描かない。
pub(crate) const FLUTTER_FILTERS: [&str; 4] =
    ["feGaussianBlur", "feOffset", "feMerge", "feMergeNode"];
/// react-native-svg がネイティブで描けるフィルターの部品。
pub(crate) const RN_FILTERS: [&str; 9] = [
    "feBlend",
    "feColorMatrix",
    "feComposite",
    "feDropShadow",
    "feFlood",
    "feGaussianBlur",
    "feMerge",
    "feMergeNode",
    "feOffset",
];

/// `svg` の中の、モバイル向けの描画部品で崩れる機能。
pub fn lint(svg: &[u8]) -> Vec<Issue> {
    use Target::*;
    let text = String::from_utf8_lossy(svg);
    let elements = parse(&text);
    let mut found = Found::default();
    let all3 = [Android, Flutter, ReactNative];
    let root = elements.iter().position(|e| e.local() == "svg");
    if let Some(r) = root
        && elements[r].prop("viewBox").is_none()
    {
        found.add(
            &[Android],
            "viewBox",
            tr!("The root <svg> has no viewBox; Android Studio refuses to import it"),
        );
    }
    for (i, e) in elements.iter().enumerate() {
        let name = e.local();
        let tag = format!("<{name}>");
        match name {
            "svg" if Some(i) != root => {
                found.add(&[Android, Flutter], &tag, tr!("A nested <svg> isn’t drawn"));
            }
            "filter" => {
                found.add(&[Android], &tag, tr!("Filters aren’t imported"));
                found.add(
                    &[Ios],
                    &tag,
                    tr!("Filters have made asset catalogs fail to compile"),
                );
            }
            n if n.starts_with("fe") && n.len() > 2 && n.as_bytes()[2].is_ascii_uppercase() => {
                if !FLUTTER_FILTERS.contains(&n) {
                    found.add(&[Flutter], &tag, tr!("Only blur, offset and merge filters are drawn; a filter with this one is dropped"));
                }
                if !RN_FILTERS.contains(&n) {
                    found.add(
                        &[ReactNative],
                        &tag,
                        tr!("This filter primitive isn’t drawn natively (only on the web)"),
                    );
                }
            }
            "style" => {
                found.add(&[Android], &tag, tr!("Only .class selectors are applied"));
                found.add(
                    &[Flutter],
                    &tag,
                    tr!("<style> is ignored; only style=\"…\" attributes are read"),
                );
                found.add(
                    &[ReactNative],
                    &tag,
                    tr!("<style> is ignored unless the app uses SvgCss or SvgCssUri"),
                );
            }
            "text" | "tspan" => {
                if name == "text" {
                    found.add(
                        &[Android],
                        &tag,
                        tr!("Text is turned into outlines with the fonts on the importing machine"),
                    );
                    found.add(&[Ios], &tag, tr!("Text should be converted to outlines"));
                }
                let lists = ["x", "y", "dx", "dy"].iter().any(|k| {
                    e.prop(k).is_some_and(|v| {
                        v.split(|c: char| c == ',' || c.is_whitespace())
                            .filter(|s| !s.is_empty())
                            .count()
                            > 1
                    })
                });
                if lists {
                    found.add(
                        &[Android, Flutter],
                        &tag,
                        tr!("Per-character x, y, dx or dy lists aren’t supported"),
                    );
                }
            }
            "textPath" => found.add(
                &[Android, Flutter],
                &tag,
                tr!("Text on a path isn’t supported"),
            ),
            "marker" => found.add(&[Android, Flutter], &tag, tr!("Markers aren’t drawn")),
            "pattern" => found.add(&[Android], &tag, tr!("Patterns aren’t imported")),
            "mask" => found.add(
                &[Android],
                &tag,
                tr!("A mask becomes a clip path; anything but solid white is lost"),
            ),
            "image" => {
                found.add(&[Android], &tag, tr!("Embedded images aren’t imported"));
                let href = e
                    .prop("href")
                    .or_else(|| e.prop("xlink:href"))
                    .unwrap_or_default();
                let raster = [
                    "data:image/png",
                    "data:image/jpeg",
                    "data:image/jpg",
                    "data:image/webp",
                    "data:image/gif",
                    "data:image/bmp",
                ];
                if !raster.iter().any(|p| href.starts_with(p)) {
                    found.add(&[Flutter], &tag, tr!("Only PNG, JPEG, WebP, GIF or BMP images embedded as data: URIs are drawn"));
                }
            }
            "symbol" => found.add(&[Android], &tag, tr!("Symbols aren’t imported")),
            "foreignObject" | "switch" | "script" | "animate" | "animateMotion"
            | "animateTransform" | "set" => {
                found.add(&all3, &tag, tr!("Not drawn"));
            }
            "a" => found.add(
                &[ReactNative],
                &tag,
                tr!("Links aren’t drawn, nor what they contain"),
            ),
            "radialGradient" if e.prop("fx").is_some() || e.prop("fy").is_some() => {
                found.add(&[Android], "fx, fy", tr!("The focal point is dropped"));
                found.add(
                    &[ReactNative],
                    "fx, fy",
                    tr!("The focal point isn’t applied on Android"),
                );
            }
            "clipPath" => {
                let shapes = [
                    "path", "rect", "circle", "ellipse", "line", "polyline", "polygon", "use",
                    "text",
                ];
                let other = elements.iter().any(|c| {
                    c.parent == Some(i)
                        && !shapes.contains(&c.local())
                        && !matches!(c.local(), "title" | "desc")
                });
                if other {
                    found.add(
                        &[Flutter],
                        &tag,
                        tr!("Only shapes inside a clip path are used"),
                    );
                }
            }
            "g" if e.prop("opacity").is_some_and(|v| v != "1") && e.children > 1 => {
                found.add(
                    &[Android],
                    "opacity",
                    tr!("Group opacity is applied to each shape, so overlaps look different"),
                );
            }
            _ => {}
        }
        if matches!(name, "linearGradient" | "radialGradient") {
            found.add(
                &[Android],
                &tag,
                tr!("Gradients need Android 7.0 (API 24) or AndroidX"),
            );
        }
        if e.prop("stroke-dasharray").is_some_and(|v| v != "none") {
            found.add(
                &[Android],
                "stroke-dasharray",
                tr!("Dashed strokes are drawn solid"),
            );
        }
        for key in ["fill-rule", "clip-rule"] {
            if e.prop(key).as_deref() == Some("evenodd") {
                found.add(&[Android], key, tr!("evenodd needs Android 7.0 (API 24)"));
            }
        }
        if e.prop("mix-blend-mode").is_some_and(|v| v != "normal") {
            found.add(
                &[Android],
                "mix-blend-mode",
                tr!("Blend modes aren’t supported"),
            );
        }
        if ["marker-start", "marker-mid", "marker-end"]
            .iter()
            .any(|k| e.prop(k).is_some())
        {
            found.add(&[Android, Flutter], "marker-*", tr!("Markers aren’t drawn"));
        }
        let p3 = e.attrs.iter().any(|(_, v)| v.contains("color(display-p3"))
            || e.prop("fill").is_some_and(|v| v.contains("color("));
        if p3 {
            found.add(
                &[Android, Ios],
                "color()",
                tr!("color(display-p3 …) isn’t understood (iOS draws it black)"),
            );
        }
        let units = [
            "width", "height", "x", "y", "x1", "y1", "x2", "y2", "cx", "cy", "r", "rx", "ry",
        ];
        let absolute = units.iter().any(|k| {
            e.prop(k).is_some_and(|v| {
                let v = v.trim();
                ["mm", "cm", "in", "pt", "pc"]
                    .iter()
                    .any(|u| v.ends_with(u) && v[..v.len() - 2].parse::<f64>().is_ok())
            })
        });
        if absolute {
            found.add(
                &[Android, Flutter],
                "mm, cm, in, pt, pc",
                tr!("Absolute units aren’t converted"),
            );
        }
    }
    found.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn features(svg: &str) -> Vec<(String, Vec<Target>)> {
        lint(svg.as_bytes())
            .into_iter()
            .map(|i| (i.feature, i.targets))
            .collect()
    }

    #[test]
    fn a_plain_icon_has_no_issues() {
        let svg = r#"<?xml version="1.0"?><!-- icon --><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
            <path d="M0 0h24v24H0z" fill="none"/><g opacity="1"><circle cx="12" cy="12" r="5"/></g></svg>"#;
        assert!(lint(svg.as_bytes()).is_empty());
    }

    #[test]
    fn finds_what_breaks_on_each_target() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="120">
            <style>.c{fill:#2a7}</style>
            <defs><filter id="b"><feGaussianBlur stdDeviation="4"/><feTurbulence/></filter>
            <mask id="m"><rect width="200" height="120" fill="white"/></mask>
            <linearGradient id="g"><stop offset="0"/></linearGradient></defs>
            <rect class="c" mask="url(#m)" style="stroke-dasharray: 4 2"/>
            <text x="1 2 3" y="10">Hi</text>
            <!-- <pattern> in a comment isn't counted -->
            <svg/></svg>"#;
        let found = features(svg);
        let has = |f: &str, t: Target| found.iter().any(|(name, ts)| name == f && ts.contains(&t));
        assert!(has("viewBox", Target::Android));
        assert!(has("<style>", Target::Flutter) && has("<style>", Target::ReactNative));
        assert!(has("<filter>", Target::Android) && has("<filter>", Target::Ios));
        // ぼかしは Flutter でも React Native でも描けるが、feTurbulence はどちらも描けない。
        assert!(!has("<feGaussianBlur>", Target::Flutter));
        assert!(
            has("<feTurbulence>", Target::Flutter) && has("<feTurbulence>", Target::ReactNative)
        );
        assert!(has("<mask>", Target::Android));
        assert!(has("<linearGradient>", Target::Android));
        assert!(has("stroke-dasharray", Target::Android));
        assert!(has("<text>", Target::Flutter));
        assert!(has("<svg>", Target::Flutter));
        assert!(!found.iter().any(|(name, _)| name == "<pattern>"));
    }

    #[test]
    fn counts_repeated_features_once() {
        let svg = r#"<svg viewBox="0 0 1 1"><marker/><marker/><path marker-end="url(#a)"/></svg>"#;
        let issues = lint(svg.as_bytes());
        let marker = issues.iter().find(|i| i.feature == "<marker>").unwrap();
        assert_eq!(marker.count, 2);
        assert_eq!(marker.targets, vec![Target::Android, Target::Flutter]);
    }
}
