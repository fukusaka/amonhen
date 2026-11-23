//! journey と timeline のラベル（pele の `src/diagrams/common/fit.ts`）。

use crate::js;
use crate::text::label::{Label, layout_label};
use crate::text::measurer::TextMeasurer;
use crate::util::chars::is_space_char;

/// `br\s*\/?>`（大文字小文字を区別しない）が `chars[i..]` に合えば、その長さ。
fn br_tag(chars: &[char], i: usize) -> Option<usize> {
    let at = |k: usize| chars.get(i + k).copied();
    if !matches!(at(0), Some('b' | 'B')) || !matches!(at(1), Some('r' | 'R')) {
        return None;
    }
    let mut k = 2;
    while at(k).is_some_and(is_space_char) {
        k += 1;
    }
    if at(k) == Some('/') {
        k += 1;
    }
    (at(k) == Some('>')).then_some(k + 1)
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// 単語を切ってよい単位（`RE_UNIT`）: 改行のタグ、符号にした文字、サロゲートの対、1 文字。
fn units(raw: &str) -> Vec<String> {
    let chars: Vec<char> = raw.chars().collect();
    let n = chars.len();
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        let c = chars[i];
        let mut len = 1;
        if c == '<' {
            if let Some(k) = br_tag(&chars, i + 1) {
                len = k + 1;
            }
        } else if c == 'ﬂ' && chars.get(i + 1) == Some(&'°') {
            let mut k = i + 2;
            if chars.get(k) == Some(&'°') {
                k += 1;
            }
            let w = k;
            while k < n && is_word(chars[k]) {
                k += 1;
            }
            if k > w && chars.get(k) == Some(&'¶') && chars.get(k + 1) == Some(&'ß') {
                len = k + 2 - i;
            }
        } else if c == '&' {
            let mut k = i + 1;
            if chars.get(k) == Some(&'#') {
                k += 1;
            }
            let w = k;
            while k < n && is_word(chars[k]) {
                k += 1;
            }
            if k > w && chars.get(k) == Some(&';') {
                len = k + 1 - i;
            }
        }
        out.push(chars[i..i + len].iter().collect());
        i += len;
    }
    out
}

/// Mermaid の journey と timeline が描くとおりにラベルを組む: `<br>` だけを印とする文字として。ほかのタグや
/// アイコンの名前は、飾りとして読まずに書いたとおり見せる。
pub fn plain_label(
    raw: Option<&str>,
    measurer: &dyn TextMeasurer,
    size: f64,
    max: f64,
    base: u32,
) -> Label {
    let text = raw.map(|text| {
        let mut text = text.to_string();
        if text.contains('<') {
            let chars: Vec<char> = text.chars().collect();
            let mut out = String::with_capacity(text.len() + 8);
            for (i, &c) in chars.iter().enumerate() {
                if c == '<' && br_tag(&chars, i + 1).is_none() {
                    out.push_str("&lt;");
                } else {
                    out.push(c);
                }
            }
            text = out;
        }
        if text.contains(":fa-") {
            text = text.replace(":fa-", "&colon;fa-");
        }
        text
    });
    layout_label(text.as_deref(), false, measurer, size, max, base)
}

/// `layout_label` は空白でしか折り返さない。幅の決まった箱のために、収まらない長い単語も切る。
pub fn fit_label(
    raw: Option<&str>,
    measurer: &dyn TextMeasurer,
    size: f64,
    max: f64,
    base: u32,
) -> Label {
    let label = plain_label(raw, measurer, size, max, base);
    let Some(raw) = raw else {
        return label;
    };
    if label.width <= max {
        return label;
    }
    let mut out = String::new();
    let mut width = 0.0;
    for unit in units(raw) {
        let long = js::len16(&unit) > 2;
        if js::trim(&unit).is_empty() || (long && unit.starts_with('<')) {
            width = 0.0;
        } else {
            let w = measurer.width(if long { "x" } else { &unit }, size, base);
            if width > 0.0 && width + w > max {
                out.push('\n');
                width = 0.0;
            }
            width += w;
        }
        out.push_str(&unit);
    }
    plain_label(Some(&out), measurer, size, max, base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_units() {
        assert_eq!(
            units("a<BR />b&amp;ﬂ°°x1¶ß😀<b>"),
            vec!["a", "<BR />", "b", "&amp;", "ﬂ°°x1¶ß", "😀", "<", "b", ">"]
        );
        assert_eq!(units("&;ﬂ°¶ß"), vec!["&", ";", "ﬂ", "°", "¶", "ß"]);
    }
}
