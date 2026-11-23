//! 1 行の文字を幅に収める（pele の `src/diagrams/common/fit-text.ts`）。

use crate::js;
use crate::text::measurer::TextMeasurer;

#[derive(Clone, Debug, PartialEq)]
pub struct Fitted {
    pub text: String,
    pub size: f64,
    pub width: f64,
}

/// 1 行の文字を幅に収める: まず字を `min` まで小さくし、それでも収まらなければ切って省略の印を付ける。1 文字も
/// 収まらなければ `None`。
pub fn fit_text(
    text: &str,
    max: f64,
    size: f64,
    min: f64,
    measurer: &dyn TextMeasurer,
    style: u32,
) -> Option<Fitted> {
    if text.is_empty() || max <= 0.0 {
        return None;
    }
    let mut width = measurer.width(text, size, style);
    if width <= max {
        return Some(Fitted {
            text: text.to_string(),
            size,
            width,
        });
    }

    // 幅は字の大きさとともに増えるので、1 度の割り算で合う大きさか、そのすぐ上に着く。
    let mut fitted = js::max(min, ((size * max) / width).floor());
    while fitted >= min {
        width = measurer.width(text, fitted, style);
        if width <= max {
            return Some(Fitted {
                text: text.to_string(),
                size: fitted,
                width,
            });
        }
        fitted -= 1.0;
    }

    let chars: Vec<char> = text.chars().collect();
    let cut_at = |n: usize| -> String {
        let head: String = chars[..n].iter().collect();
        format!("{}…", js::trim_end(&head))
    };
    let mut low = 0usize;
    let mut high = chars.len() - 1;
    while low < high {
        let mid = (low + high + 1) >> 1;
        if measurer.width(&cut_at(mid), min, style) <= max {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    if low == 0 {
        return None;
    }
    let cut = cut_at(low);
    width = measurer.width(&cut, min, style);
    (width <= max).then_some(Fitted {
        text: cut,
        size: min,
        width,
    })
}
