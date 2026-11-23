//! Markdown の図（` ```mermaid ` のブロック）を SVG に描く（History の「すべてのファイル」の整形表示）。描くのは
//! pele（Obsidian の Mermaid の描き手）を移した `amonhen-pele` で、`diagrams` フィーチャーのときだけ入る。できた
//! SVG は色を CSS の変数で決めるので、`svgcss` で属性に書き写して CoreSVG・librsvg で描ける形にする。明るい表示と
//! 暗い表示の 2 つを作る（どちらも 1 ms かからない）ので、表示を切り替えても読み直さなくてよい。

/// 描いた図。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Drawn {
    /// 明るい表示と暗い表示の SVG。
    Svg { light: String, dark: String },
    /// 描けない（書き方の誤りなど）。理由は pele の言葉のまま（英語）。
    Error(String),
}

/// 暗い表示の色（pele の既定は明るい表示のもの）。
const DARK: [(&str, &str); 9] = [
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
    match amonhen_pele::render(text) {
        Ok(rendered) => {
            let light = crate::svgcss::inline(&rendered.svg)?;
            let dark = crate::svgcss::inline_with(&rendered.svg, &DARK)?;
            Some(Drawn::Svg { light, dark })
        }
        Err(error) => Some(Drawn::Error(error.message)),
    }
}

#[cfg(not(feature = "diagrams"))]
fn draw_mermaid(_text: &str) -> Option<Drawn> {
    let _ = DARK;
    None
}

#[cfg(all(test, feature = "diagrams"))]
mod tests {
    use super::*;

    #[test]
    fn flowcharts_are_drawn_in_both_palettes() {
        let Some(Drawn::Svg { light, dark }) =
            draw("mermaid", "flowchart LR\n  A[Start] --> B[End]\n")
        else {
            panic!("not drawn");
        };
        assert!(!light.contains("var(") && !dark.contains("var("));
        assert!(
            light.contains("#f3f3f3") && dark.contains("#2b2b2b"),
            "{dark}"
        );
        assert!(light.contains(">Start</text>"));
    }

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
