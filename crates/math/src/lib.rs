//! Markdown の数式（TeX）を SVG に描く（History の「すべてのファイル」の整形表示）。読み取りと組版は RaTeX
//! （KaTeX の Rust への移植）で、組んだ物の並びを自前で SVG にする（`svg`）。字形は同梱の KaTeX のフォントの輪郭
//! なので、OS のフォントを探さず、どの FE でも同じ形になる。KaTeX のフォントにない文字（`\text{日本語}` など）だけは
//! `<text>` にして FE の書体で描かせる。
//!
//! 色は CSS の変数（`--math-fg`、`COLOR_VAR`）で書くので、FE は描くときに文字の色を当てる（図と同じく、テーマや明るい・
//! 暗いを切り替えても色だけを当て直せばよい）。`\color{…}` で決めた色はその色のまま。
//!
//! 大きさは em で返す。FE は本文の字の大きさを掛けて描く。描いたものは原文ごとに覚える（同じ式は描き直さない）。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use ratex_layout::{LayoutOptions, layout, to_display_list, validate_layout};
use ratex_types::math_style::MathStyle;

mod svg;
mod ttf;

pub use svg::ROOT_FILL;

/// 色の CSS の変数（FE が文字の色を当てる）。
pub const COLOR_VAR: &str = "--math-fg";

/// 1 つの式の原文の長さの上限（バイト）。これより長いものは描かずに原文を見せる。
pub const MAX_SOURCE: usize = 8 * 1024;

/// 1 つの式の描く物（字形・線・四角）の数の上限。RaTeX の上限（65,536）より小さくし、1 つの式で数十 ms を超えない
/// ようにする。
pub const MAX_ITEMS: usize = 20_000;

/// 1 つの式の大きさの上限（em）。`\raisebox` などで途方もない大きさにしたものは描かない。
const MAX_EM: f64 = 500.0;

/// 1 つの式の SVG の大きさの上限（バイト）。FE は描くたびに色を当てて画像にするので、それが重くならないように。
pub const MAX_SVG: usize = 1 << 20;

/// 描いた式。大きさは em（本文の字の大きさを 1 とする）。
#[derive(Debug, Clone, PartialEq)]
pub struct Formula {
    /// SVG（色は `--math-fg` の変数のまま）。
    pub svg: String,
    /// 幅。
    pub width: f64,
    /// 基線から上の高さ。
    pub height: f64,
    /// 基線から下の深さ（文中の式は、これだけ基線より下に出す）。
    pub depth: f64,
}

/// 描けない理由。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// 書き方の誤り（RaTeX の言葉のまま、英語）。`at` は原文の中の位置（バイト）。
    Parse { message: String, at: Option<usize> },
    /// 原文が長すぎる（`MAX_SOURCE`）。
    TooLong,
    /// 描く物が多すぎる・大きすぎる（`MAX_ITEMS`・RaTeX の上限など）。
    TooLarge,
    /// RaTeX の中で止まった（panic）。
    Failed,
}

impl Error {
    /// 誤りの文と、原文の中で誤りを ^ で指した行（位置がわかるとき）。
    pub fn describe(&self, tex: &str) -> Option<String> {
        let Error::Parse { message, at } = self else {
            return None;
        };
        let mut out = message.clone();
        if let Some(at) = *at
            && at <= tex.len()
            && tex.is_char_boundary(at)
        {
            let line_start = tex[..at].rfind('\n').map_or(0, |i| i + 1);
            let line_end = tex[at..].find('\n').map_or(tex.len(), |i| at + i);
            let line = &tex[line_start..line_end];
            let column = tex[line_start..at].chars().count();
            if line.chars().count() <= 200 {
                out.push('\n');
                out.push_str(line);
                out.push('\n');
                out.push_str(&" ".repeat(column));
                out.push('^');
            }
        }
        Some(out)
    }
}

/// 覚えておく式の数と SVG の合計の大きさの上限（超えたら忘れて覚え直す）。
const CACHE_ENTRIES: usize = 2_048;
const CACHE_BYTES: usize = 32 << 20;

type Cache = Mutex<(HashMap<(bool, String), Result<Arc<Formula>, Error>>, usize)>;

fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(Cache::default)
}

/// TeX の `tex` を描く。`display` ならブロック（`$$…$$`・```` ```math ````）の組み方（大きな演算子・分数）、
/// そうでなければ文中の組み方。同じ原文は覚えたものを返す。
pub fn render(tex: &str, display: bool) -> Result<Arc<Formula>, Error> {
    if tex.len() > MAX_SOURCE {
        return Err(Error::TooLong);
    }
    let key = (display, tex.to_owned());
    if let Ok(cache) = cache().lock()
        && let Some(hit) = cache.0.get(&key)
    {
        return hit.clone();
    }
    let made = std::panic::catch_unwind(|| draw(tex, display))
        .unwrap_or(Err(Error::Failed))
        .map(Arc::new);
    if let Ok(mut cache) = cache().lock() {
        let size = made.as_ref().map_or(0, |f| f.svg.len()) + tex.len();
        if cache.0.len() >= CACHE_ENTRIES || cache.1 + size > CACHE_BYTES {
            cache.0.clear();
            cache.1 = 0;
        }
        cache.1 += size;
        cache.0.insert(key, made.clone());
    }
    made
}

fn draw(tex: &str, display: bool) -> Result<Formula, Error> {
    let ast = ratex_parser::parse(tex).map_err(|e| Error::Parse {
        message: e.message,
        at: e.loc.map(|l| l.start),
    })?;
    let style = if display {
        MathStyle::Display
    } else {
        MathStyle::Text
    };
    let options = LayoutOptions::default()
        .with_style(style)
        .with_color(svg::DEFAULT_MARK);
    let tree = layout(&ast, &options);
    validate_layout(&tree).map_err(|_| Error::TooLarge)?;
    let list = to_display_list(&tree);
    if list.items.len() > MAX_ITEMS
        || ![list.width, list.height, list.depth]
            .iter()
            .all(|v| v.is_finite() && v.abs() <= MAX_EM)
    {
        return Err(Error::TooLarge);
    }
    let written = svg::write(&list);
    if written.svg.len() > MAX_SVG
        || !(written.width <= MAX_EM && written.height + written.depth <= MAX_EM)
    {
        return Err(Error::TooLarge);
    }
    Ok(Formula {
        svg: written.svg,
        width: written.width,
        height: written.height,
        depth: written.depth,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_glyphs_as_paths_in_the_color_variable() {
        let f = render(r"\frac{a}{b} + x^2", false).unwrap();
        assert!(f.svg.starts_with("<svg xmlns="));
        assert!(
            f.svg.contains("fill=\"var(--math-fg,#000000)\""),
            "{}",
            f.svg
        );
        assert!(
            f.svg
                .contains("<use xlink:href=\"#g0\" transform=\"matrix("),
            "{}",
            f.svg
        );
        assert!(f.svg.contains("<defs><path id=\"g0\" d=\"M"), "{}", f.svg);
        // 分数の線
        assert!(f.svg.contains("<rect "), "{}", f.svg);
        assert!(!f.svg.contains("<text"), "{}", f.svg);
        assert!(!f.svg.contains("rgba("), "{}", f.svg);
        assert!(f.width > 1.0 && f.height > 0.5 && f.depth > 0.2, "{f:?}");
    }

    #[test]
    fn explicit_colors_stay() {
        let f = render(r"{\color{red} x} + \color{black} y", false).unwrap();
        assert!(f.svg.contains("fill=\"#ff0000\""), "{}", f.svg);
        // \color{black} は既定の色（変数）と区別する
        assert!(f.svg.contains("fill=\"#000000\""), "{}", f.svg);
        let plain = render("x", false).unwrap();
        assert!(!plain.svg.contains("fill=\"#"), "{}", plain.svg);
    }

    #[test]
    fn content_on_a_colored_box_gets_a_readable_color() {
        // 明るい地の上は #1f1f1f、濃い地の上は白（テーマの文字の色の変数は使わない）
        let f = render(r"\colorbox{yellow}{boxed}", false).unwrap();
        assert!(f.svg.contains("fill=\"#ffff00\""), "{}", f.svg);
        assert_eq!(f.svg.matches("fill=\"#1f1f1f\"").count(), 5, "{}", f.svg);
        let f = render(r"\colorbox{navy}{x} + y", false).unwrap();
        assert_eq!(f.svg.matches("fill=\"#ffffff\"").count(), 1, "{}", f.svg);
        // 地の外の y は既定の色のまま（fill を書かない <use> がある）
        assert!(f.svg.contains("\"/><use"), "{}", f.svg);
        let f = render(r"\fcolorbox{red}{black}{ab}", false).unwrap();
        assert_eq!(f.svg.matches("fill=\"#ffffff\"").count(), 2, "{}", f.svg);
        // \color で決めた色はそのまま
        let f = render(r"\colorbox{yellow}{\color{blue} z}", false).unwrap();
        assert!(f.svg.contains("fill=\"#0000ff\""), "{}", f.svg);
        assert!(!f.svg.contains("#1f1f1f"), "{}", f.svg);
    }

    #[test]
    fn missing_glyphs_become_text() {
        let f = render(r"\text{日本語} = x", true).unwrap();
        assert!(f.svg.contains(">日</text>"), "{}", f.svg);
        assert!(f.svg.contains("font-family=\"sans-serif\""), "{}", f.svg);
        let f = render(r"\text{a<b}", true).unwrap();
        assert!(!f.svg.contains("a<b"), "{}", f.svg);
    }

    #[test]
    fn errors_point_at_the_place() {
        let e = render(r"\frac{1}{2", true).unwrap_err();
        assert!(matches!(e, Error::Parse { .. }), "{e:?}");
        let e = render(r"x + \unknowncommand{y}", true).unwrap_err();
        let text = e.describe(r"x + \unknowncommand{y}").unwrap();
        assert!(text.contains("unknowncommand"), "{text}");
        assert!(text.contains("\n    ^"), "{text}");
    }

    #[test]
    fn limits() {
        let long = "x+".repeat(MAX_SOURCE);
        assert_eq!(render(&long, true).err(), Some(Error::TooLong));
        // 原文は短くても描く物が多いもの（破線の縦線が 2 万本を超える）
        let dashes = format!(
            r"\begin{{array}}{{{}:}}\rule{{1em}}{{400em}}{}\end{{array}}",
            ":c".repeat(24),
            "&x".repeat(23)
        );
        assert_eq!(render(&dashes, true).err(), Some(Error::TooLarge));
        // 途方もない大きさ
        assert_eq!(
            render(r"\raisebox{10000000000em}{x}", true).err(),
            Some(Error::TooLarge)
        );
        // SVG が大きすぎる（伸ばした括弧の形が並ぶ）
        let braces = r"\overbrace{x+x}^{a}".repeat(MAX_SOURCE / 20);
        assert!(braces.len() <= MAX_SOURCE);
        assert_eq!(render(&braces, true).err(), Some(Error::TooLarge));
        // 上限の中の大きな行列は描ける
        let row = vec!["x"; 60].join("&");
        let matrix = format!(
            r"\begin{{matrix}}{}\end{{matrix}}",
            vec![row; 60].join(r"\\")
        );
        assert!(matrix.len() <= MAX_SOURCE);
        assert!(render(&matrix, true).is_ok());
        // マクロの展開の爆発は RaTeX が止める
        assert!(matches!(
            render(r"\def\a{\a\a}\a", true),
            Err(Error::Parse { .. })
        ));
    }

    #[test]
    fn caches_by_source_and_style() {
        let a = render("a+b", true).unwrap();
        let b = render("a+b", true).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        let c = render("a+b", false).unwrap();
        assert!(!Arc::ptr_eq(&a, &c));
    }

    #[test]
    fn every_katex_font_reads() {
        for (id, font) in svg_fonts() {
            assert!(font, "{id}");
        }
    }

    fn svg_fonts() -> Vec<(String, bool)> {
        let mut out: Vec<(String, bool)> = Vec::new();
        for name in [
            "AMS-Regular",
            "Caligraphic-Regular",
            "Fraktur-Regular",
            "Main-Regular",
            "Main-Bold",
            "Math-Italic",
            "SansSerif-Regular",
            "Script-Regular",
            "Size1-Regular",
            "Size4-Regular",
            "Typewriter-Regular",
        ] {
            let bytes = ratex_katex_fonts::ttf_bytes(&format!("KaTeX_{name}.ttf"));
            let ok = match bytes {
                Some(std::borrow::Cow::Borrowed(b)) => ttf::Font::parse(b)
                    .and_then(|f| f.glyph_id('1').or(f.glyph_id('A')).or(f.glyph_id('(')))
                    .is_some(),
                _ => false,
            };
            out.push((name.to_owned(), ok));
        }
        out
    }
}
