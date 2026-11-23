//! Markdown の図（` ```mermaid ` のブロック）を SVG に描く（History の「すべてのファイル」の整形表示）。描くのは
//! pele（Obsidian の Mermaid の描き手）を移した `amonhen-pele` で、`diagrams` フィーチャーのときだけ入る。
//!
//! 形と位置（pele の SVG）は Markdown を読むときに一度だけ作る。色は pele が CSS の変数（`--pele-*`）で決めるので、
//! 描くときに `colorize` で配色（構文の色・pele・Mermaid）の変数を当て、`svgcss` で属性に書き写して CoreSVG・
//! librsvg で描ける形にする（1 図 0.05〜0.3 ms）。テーマや明るい・暗いを切り替えても、色だけを当て直せばよい。

use std::collections::HashMap;

/// 描いた図。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Drawn {
    /// pele の SVG（色は CSS の変数のまま。`colorize` で色を当てる）。
    Svg(String),
    /// 描けない（書き方の誤りなど）。理由は pele の言葉のまま（英語）。
    Error(String),
}

/// フェンスの言葉が `language` のブロックの中身 `text` を図にする。図の言葉でない・まだ描けない種類なら `None`
/// （FE はコードとして見せる）。
pub fn draw(language: &str, text: &str) -> Option<Drawn> {
    if !language.eq_ignore_ascii_case("mermaid") {
        return None;
    }
    draw_mermaid(text)
}

#[cfg(feature = "diagrams")]
fn draw_mermaid(text: &str) -> Option<Drawn> {
    if !amonhen_pele::supports(text) {
        return None;
    }
    Some(match amonhen_pele::render(text) {
        Ok(rendered) => Drawn::Svg(rendered.svg),
        Err(error) => Drawn::Error(error.message),
    })
}

#[cfg(not(feature = "diagrams"))]
fn draw_mermaid(_text: &str) -> Option<Drawn> {
    None
}

/// 図の配色（設定の「図の配色」）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scheme {
    /// 構文の色と同じテーマから（既定）。
    #[default]
    Syntax,
    /// pele の既定の色（灰色を主にした色）。
    Pele,
    /// Mermaid 本家の default（明るい表示）・dark（暗い表示）の色。
    Mermaid,
}

impl Scheme {
    /// 設定に書く名前。
    pub fn key(self) -> &'static str {
        match self {
            Self::Syntax => "syntax",
            Self::Pele => "pele",
            Self::Mermaid => "mermaid",
        }
    }

    /// 設定の名前から（知らない名前は既定）。
    pub fn from_key(key: &str) -> Self {
        match key {
            "pele" => Self::Pele,
            "mermaid" => Self::Mermaid,
            _ => Self::Syntax,
        }
    }
}

/// 構文の色のテーマから配色を作るための色（`#rrggbb`）。`text`・`background` は画面の文字と地の色、ほかは構文の
/// 種類（`keyword` など、テーマの `colors` の鍵）→ 色。テーマが持たない種類は、FE がその外観の標準の色を入れる。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ThemeColors {
    pub text: String,
    pub background: String,
    pub kinds: HashMap<String, String>,
}

/// pele の暗い表示の色（pele の既定は明るい表示のもの）。
const PELE_DARK: [(&str, &str); 9] = [
    ("--pele-bg", "#1e1e1e"),
    ("--pele-fg", "#e4e4e4"),
    ("--pele-muted", "#9a9a9a"),
    ("--pele-line", "#d0d0d0"),
    ("--pele-surface", "#2b2b2b"),
    ("--pele-surface-alt", "#363636"),
    ("--pele-border", "#6b6b6b"),
    ("--pele-accent", "#7b9be5"),
    ("--pele-fg-on-dark", "#ffffff"),
];

/// Mermaid の default のテーマの色（mermaid の `theme-default`）。
const MERMAID_LIGHT: [(&str, &str); 8] = [
    ("--pele-bg", "#ffffff"),
    ("--pele-fg", "#333333"),
    ("--pele-muted", "#666666"),
    ("--pele-line", "#333333"),
    ("--pele-surface", "#ececff"),
    ("--pele-surface-alt", "#ffffde"),
    ("--pele-border", "#9370db"),
    ("--pele-accent", "#9370db"),
];

/// Mermaid の dark のテーマの色（mermaid の `theme-dark`）。
const MERMAID_DARK: [(&str, &str); 9] = [
    ("--pele-bg", "#1e1e1e"),
    ("--pele-fg", "#cccccc"),
    ("--pele-muted", "#999999"),
    ("--pele-line", "#d3d3d3"),
    ("--pele-surface", "#1f2020"),
    ("--pele-surface-alt", "#303030"),
    ("--pele-border", "#cccccc"),
    ("--pele-accent", "#81b1db"),
    ("--pele-fg-on-dark", "#ffffff"),
];

/// 系列（円グラフ・gitGraph などの 8 色）に使う構文の種類。
const SERIES: [&str; 8] = [
    "keyword",
    "string",
    "number",
    "function",
    "type",
    "constant",
    "attribute",
    "tag",
];

/// 配色の変数（`--pele-*` → 色）。`theme` は `Scheme::Syntax` のときだけ使う。
pub fn palette(scheme: Scheme, dark: bool, theme: &ThemeColors) -> Vec<(String, String)> {
    let own = |list: &[(&str, &str)]| {
        list.iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    };
    match scheme {
        Scheme::Pele if dark => own(&PELE_DARK),
        Scheme::Pele => Vec::new(),
        Scheme::Mermaid if dark => own(&MERMAID_DARK),
        Scheme::Mermaid => own(&MERMAID_LIGHT),
        Scheme::Syntax => {
            let kind = |name: &str| {
                theme
                    .kinds
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| theme.text.clone())
            };
            let keyword = kind("keyword");
            let mut out = vec![
                ("--pele-bg".to_owned(), theme.background.clone()),
                ("--pele-fg".to_owned(), theme.text.clone()),
                ("--pele-line".to_owned(), theme.text.clone()),
                ("--pele-muted".to_owned(), kind("comment")),
                ("--pele-border".to_owned(), keyword.clone()),
                ("--pele-accent".to_owned(), kind("function")),
                (
                    "--pele-surface".to_owned(),
                    mix(&keyword, &theme.background, 0.12),
                ),
                (
                    "--pele-surface-alt".to_owned(),
                    mix(&kind("type"), &theme.background, 0.18),
                ),
                ("--pele-fg-on-dark".to_owned(), "#ffffff".to_owned()),
            ];
            for (i, name) in SERIES.iter().enumerate() {
                out.push((format!("--pele-series-{}", i + 1), kind(name)));
            }
            out
        }
    }
}

/// pele の SVG `svg` に配色 `vars` を当て、CoreSVG・librsvg で描ける SVG にする。読めなければ `None`。
pub fn colorize(svg: &str, vars: &[(String, String)]) -> Option<String> {
    let vars: Vec<(&str, &str)> = vars.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    crate::svgcss::inline_with(svg, &vars)
}

/// `top` を `amount`（0〜1）だけ `base` に重ねた色。読めない色なら `top`。
fn mix(top: &str, base: &str, amount: f64) -> String {
    let (Some(a), Some(b)) = (rgb(top), rgb(base)) else {
        return top.to_owned();
    };
    let c = |i: usize| (f64::from(a[i]) * amount + f64::from(b[i]) * (1.0 - amount)).round() as u8;
    format!("#{:02x}{:02x}{:02x}", c(0), c(1), c(2))
}

fn rgb(hex: &str) -> Option<[u8; 3]> {
    let h = hex.trim().strip_prefix('#')?;
    let h = match h.len() {
        3 => h.chars().flat_map(|c| [c, c]).collect::<String>(),
        6 | 8 => h[..6].to_owned(),
        _ => return None,
    };
    let n = u32::from_str_radix(&h, 16).ok()?;
    Some([(n >> 16) as u8, (n >> 8) as u8, n as u8])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> ThemeColors {
        ThemeColors {
            text: "#202020".into(),
            background: "#ffffff".into(),
            kinds: [
                ("keyword", "#aa0000"),
                ("comment", "#808080"),
                ("function", "#0000aa"),
            ]
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect(),
        }
    }

    #[test]
    fn syntax_palette_comes_from_the_theme() {
        let vars: HashMap<_, _> = palette(Scheme::Syntax, false, &theme())
            .into_iter()
            .collect();
        assert_eq!(vars["--pele-border"], "#aa0000");
        assert_eq!(vars["--pele-muted"], "#808080");
        assert_eq!(vars["--pele-accent"], "#0000aa");
        assert_eq!(vars["--pele-surface"], "#f5e0e0");
        // テーマが持たない種類は文字の色
        assert_eq!(vars["--pele-series-2"], "#202020");
        assert!(palette(Scheme::Pele, false, &theme()).is_empty());
        assert_eq!(Scheme::from_key("mermaid"), Scheme::Mermaid);
        assert_eq!(Scheme::from_key("?"), Scheme::Syntax);
    }

    #[cfg(feature = "diagrams")]
    #[test]
    fn flowcharts_are_drawn_and_colored() {
        let Some(Drawn::Svg(svg)) = draw("mermaid", "flowchart LR\n  A[Start] --> B[End]\n") else {
            panic!("not drawn");
        };
        assert!(svg.contains("var(--"));
        let light = colorize(&svg, &palette(Scheme::Pele, false, &theme())).unwrap();
        let mermaid = colorize(&svg, &palette(Scheme::Mermaid, false, &theme())).unwrap();
        let syntax = colorize(&svg, &palette(Scheme::Syntax, false, &theme())).unwrap();
        assert!(!light.contains("var(") && light.contains("#f3f3f3"));
        assert!(mermaid.contains("#ececff") && mermaid.contains("#9370db"));
        assert!(syntax.contains("#f5e0e0") && syntax.contains("#aa0000"));
        assert!(light.contains(">Start</text>"));
    }

    #[cfg(feature = "diagrams")]
    #[test]
    fn other_languages_and_unported_kinds_stay_code() {
        assert_eq!(draw("rust", "fn main() {}"), None);
        assert_eq!(draw("mermaid", "sequenceDiagram\n  A->>B: hi\n"), None);
        assert!(matches!(
            draw("Mermaid", "flowchart LR\n  A -->"),
            Some(Drawn::Error(_))
        ));
    }
}
