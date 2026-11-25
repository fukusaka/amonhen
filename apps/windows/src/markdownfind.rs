//! History の「すべてのファイル」で整形した Markdown の中を探す（macOS 版は差分と同じ検索の帯で見えている文字を
//! 探す。GTK4 版の `markdownfind`）。帯は差分の検索の帯（Ctrl+F）をそのまま使い、整形表示を出している間は
//! こちらを探す。
//!
//! 探すのは見えている文字: 並んだ RichTextBlock・TextBlock の文字（段落・見出し・コード・表のマス・`<kbd>` の
//! 札）。絵（画像・図・数式）は文字を持たないので当たらない。閉じた `<details>` の中は高さがないので探さない。
//! 一致は TextHighlighter で塗り（今の一致は橙、ほかは黄。差分と同じ）、今の一致へ寄せる。

use amonhen_win_xaml as xaml;
use windows_core_reactor::{IInspectable, Interface};

/// 一致の地の色（差分・詳細の検索と同じ。今の一致は橙、ほかは黄）。
const MATCH: (u8, u8, u8) = (0xff, 0xe0, 0x40);
const CURRENT: (u8, u8, u8) = (0xff, 0x94, 0x00);

/// 文字を持つ部品。
enum Holder {
    Rich(xaml::RichTextBlock),
    Plain(xaml::TextBlock),
}

impl Holder {
    fn highlighters(&self) -> windows_core_reactor::Result<xaml::IVector<xaml::TextHighlighter>> {
        match self {
            Holder::Rich(r) => r.TextHighlighters(),
            Holder::Plain(t) => t.TextHighlighters(),
        }
    }

    fn element(&self) -> windows_core_reactor::Result<xaml::UIElement> {
        match self {
            Holder::Rich(r) => r.cast(),
            Holder::Plain(t) => t.cast(),
        }
    }
}

/// 1 つの部品の文字（UTF-16）と、その中の Run（Run と、文字の中の始まり）。
struct Text {
    holder: Holder,
    text: Vec<u16>,
    runs: Vec<(xaml::TextElement, u32)>,
}

/// `root` の下の整形表示で `query` を探し、一致を塗って `current` 番目へ寄せる。一致の数を返す（`query` が空なら
/// 塗りを消して 0）。
pub fn find(root: &IInspectable, query: &str, current: usize) -> usize {
    let Ok(root) = root.cast::<xaml::DependencyObject>() else {
        return 0;
    };
    let mut texts = Vec::new();
    collect(&root, &mut texts, 0);
    let query = query.trim();
    let mut count = 0;
    for text in &texts {
        let Ok(list) = text.holder.highlighters() else {
            continue;
        };
        let _ = list.Clear();
        if query.is_empty() {
            continue;
        }
        let spots = find_all(&text.text, query);
        if spots.is_empty() {
            continue;
        }
        let here = (count..count + spots.len()).contains(&current);
        let (others, now): (Vec<_>, Vec<_>) = spots
            .iter()
            .enumerate()
            .partition(|(i, _)| !(here && count + i == current));
        let _ = add(&list, others.iter().map(|(_, s)| **s), MATCH);
        if let Some((_, spot)) = now.first() {
            let _ = add(&list, [**spot], CURRENT);
            reveal(text, spot.0);
        }
        count += spots.len();
    }
    count
}

/// 一致の範囲を `color` で塗る TextHighlighter を足す。
fn add(
    list: &xaml::IVector<xaml::TextHighlighter>,
    spots: impl IntoIterator<Item = (u32, u32)>,
    (r, g, b): (u8, u8, u8),
) -> windows_core_reactor::Result<()> {
    let highlighter = xaml::TextHighlighter::new()?;
    let ranges = highlighter.Ranges()?;
    let mut any = false;
    for (start, length) in spots {
        ranges.Append(xaml::TextRange {
            StartIndex: start as i32,
            Length: length as i32,
        })?;
        any = true;
    }
    if !any {
        return Ok(());
    }
    let brush = xaml::SolidColorBrush::CreateInstanceWithColor(xaml::Color {
        A: 0xff,
        R: r,
        G: g,
        B: b,
    })?;
    highlighter.SetBackground(&brush)?;
    list.Append(&highlighter)
}

/// 部品の `start` 文字目（UTF-16）を、見える所へ寄せる（まん中あたりに）。
fn reveal(text: &Text, start: u32) {
    let Some((run, at)) = text.runs.iter().rev().find(|(_, at)| *at <= start) else {
        return;
    };
    let show = || -> windows_core_reactor::Result<()> {
        let pointer = run
            .ContentStart()?
            .GetPositionAtOffset((start - at) as i32, xaml::LogicalDirection::Forward)?;
        let rect = pointer.GetCharacterRect(xaml::LogicalDirection::Forward)?;
        let options = xaml::BringIntoViewOptions::new()?;
        options.SetTargetRect(Some(rect))?;
        options.SetVerticalAlignmentRatio(0.5)?;
        text.holder
            .element()?
            .StartBringIntoViewWithOptions(&options)
    };
    if let Err(e) = show() {
        eprintln!("Markdown の一致へ寄せられない: {e}");
    }
}

/// `node` の下の、文字を持つ部品を上から順に集める（高さのないもの、閉じた `<details>` の中は除く）。
fn collect(node: &xaml::DependencyObject, out: &mut Vec<Text>, depth: u32) {
    if depth > 64 {
        return;
    }
    let shown = node
        .cast::<xaml::FrameworkElement>()
        .ok()
        .and_then(|e| e.ActualHeight().ok())
        .is_none_or(|h| h > 0.0);
    if !shown {
        return;
    }
    if let Ok(rich) = node.cast::<xaml::RichTextBlock>() {
        let mut text = Vec::new();
        let mut runs = Vec::new();
        if let Ok(blocks) = rich
            .Blocks()
            .and_then(|b| b.cast::<xaml::IVector<xaml::Block>>())
        {
            for i in 0..blocks.Size().unwrap_or(0) {
                if let Ok(inlines) = blocks
                    .GetAt(i)
                    .and_then(|b| b.cast::<xaml::Paragraph>())
                    .and_then(|p| p.Inlines())
                {
                    read_inlines(&inlines, &mut text, &mut runs);
                }
            }
        }
        out.push(Text {
            holder: Holder::Rich(rich),
            text,
            runs,
        });
        // 文の中に置いた部品（`<kbd>` の札）も探す（その字は Run でなく、中の TextBlock にある）。
    }
    if let Ok(plain) = node.cast::<xaml::TextBlock>() {
        let mut text = Vec::new();
        let mut runs = Vec::new();
        if let Ok(inlines) = plain.Inlines() {
            read_inlines(&inlines, &mut text, &mut runs);
        }
        out.push(Text {
            holder: Holder::Plain(plain),
            text,
            runs,
        });
        return;
    }
    let count = xaml::VisualTreeHelper::GetChildrenCount(node).unwrap_or(0);
    for i in 0..count {
        if let Ok(child) = xaml::VisualTreeHelper::GetChild(node, i) {
            collect(&child, out, depth + 1);
        }
    }
}

/// 飾り（Run・太字などの Span・リンク・改行）の文字を `text` に足し、Run の始まりを覚える。
fn read_inlines(
    inlines: &xaml::InlineCollection,
    text: &mut Vec<u16>,
    runs: &mut Vec<(xaml::TextElement, u32)>,
) {
    let Ok(list) = inlines.cast::<xaml::IVector<xaml::Inline>>() else {
        return;
    };
    for i in 0..list.Size().unwrap_or(0) {
        let Ok(inline) = list.GetAt(i) else {
            continue;
        };
        if let Ok(run) = inline.cast::<xaml::Run>() {
            if let (Ok(value), Ok(element)) = (run.Text(), run.cast::<xaml::TextElement>()) {
                runs.push((element, text.len() as u32));
                text.extend(value.to_string_lossy().encode_utf16());
            }
        } else if let Ok(span) = inline.cast::<xaml::Span>() {
            if let Ok(inner) = span.Inlines() {
                read_inlines(&inner, text, runs);
            }
        } else if inline.cast::<xaml::LineBreak>().is_ok() {
            text.push(u16::from(b'\n'));
        }
    }
}

/// `text`（UTF-16）の中の `query` の一致（UTF-16 の始まりと長さ）。大文字小文字を区別しない（差分の検索と同じ）。
pub fn find_all(text: &[u16], query: &str) -> Vec<(u32, u32)> {
    let fold = |c: char| c.to_lowercase().next().unwrap_or(c);
    let needle: Vec<char> = query.chars().map(fold).collect();
    if needle.is_empty() {
        return Vec::new();
    }
    // 文字と、その UTF-16 の位置。
    let mut chars = Vec::new();
    let mut at = 0u32;
    for c in char::decode_utf16(text.iter().copied()) {
        let c = c.unwrap_or(char::REPLACEMENT_CHARACTER);
        chars.push((fold(c), at));
        at += c.len_utf16() as u32;
    }
    let mut found = Vec::new();
    let mut i = 0;
    while i + needle.len() <= chars.len() {
        if chars[i..i + needle.len()]
            .iter()
            .map(|(c, _)| *c)
            .eq(needle.iter().copied())
        {
            let start = chars[i].1;
            let end = chars
                .get(i + needle.len())
                .map_or(at, |(_, position)| *position);
            found.push((start, end - start));
            i += needle.len();
        } else {
            i += 1;
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn finds_ignoring_case_in_utf16_positions() {
        assert_eq!(
            find_all(&utf16("Foo foo FOO"), "foo"),
            [(0, 3), (4, 3), (8, 3)]
        );
        // 絵文字（サロゲートの組）の後ろも UTF-16 の位置で。
        assert_eq!(find_all(&utf16("😀 Ctrl"), "ctrl"), [(3, 4)]);
        assert_eq!(find_all(&utf16("日本語の文"), "語の"), [(2, 2)]);
        assert!(find_all(&utf16("abc"), "").is_empty());
        // 重ならないように数える。
        assert_eq!(find_all(&utf16("aaaa"), "aa"), [(0, 2), (2, 2)]);
    }
}
