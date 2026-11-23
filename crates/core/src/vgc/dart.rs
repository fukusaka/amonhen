//! Dart の実行時のふるまい（`double.parse`・`trim`・`math.min`・`toString`・例外の文言）を写したもの。vgc と同じ
//! 結果と同じエラー文を出すために使う。

use std::fmt;

/// vgc が投げる例外。文言は Dart の `toString()` と同じ。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

pub type R<T> = std::result::Result<T, Error>;

impl Error {
    /// `StateError`。
    pub fn state(message: impl fmt::Display) -> Self {
        Self(format!("Bad state: {message}"))
    }

    /// `UnsupportedError`。
    pub fn unsupported(message: impl fmt::Display) -> Self {
        Self(format!("Unsupported operation: {message}"))
    }

    /// `UnimplementedError`。
    pub fn unimplemented(message: impl fmt::Display) -> Self {
        Self(format!("UnimplementedError: {message}"))
    }

    /// `FormatException(message, source)`（位置なし）。
    pub fn format(message: &str, source: &str) -> Self {
        let n = source.encode_utf16().count();
        let source = if n > 78 {
            let head: Vec<u16> = source.encode_utf16().take(75).collect();
            format!("{}...", String::from_utf16_lossy(&head))
        } else {
            source.to_owned()
        };
        Self(format!("FormatException: {message}\n{source}"))
    }

    /// `FormatException(message)`（元の文字列なし）。
    pub fn format_only(message: &str) -> Self {
        Self(format!("FormatException: {message}"))
    }

    /// `FormatException(message, source, offset)`（1 行の元の文字列）。
    pub fn format_at(message: &str, source: &str, offset: usize) -> Self {
        Self(format!(
            "FormatException: {message} (at character {})\n{source}\n{}^\n",
            offset + 1,
            " ".repeat(offset)
        ))
    }

    /// `ArgumentError(message)`。
    pub fn argument(message: impl fmt::Display) -> Self {
        Self(format!("Invalid argument(s): {message}"))
    }

    /// `ArgumentError.value(value, name, message)`。
    pub fn argument_value(value: &str, name: &str, message: &str) -> Self {
        Self(format!(
            "Invalid argument ({name}): {message}: {}",
            safe_string(value)
        ))
    }

    /// `RangeError.range(value, start, end, name)`。
    pub fn range(value: i64, start: i64, end: i64, name: &str) -> Self {
        let explanation = if end < start {
            "Valid value range is empty".to_owned()
        } else if end == start {
            format!("Only valid value is {start}")
        } else {
            format!("Not in inclusive range {start}..{end}")
        };
        Self(format!(
            "RangeError ({name}): Invalid value: {explanation}: {value}"
        ))
    }

    /// リストの範囲外（Dart VM の `List.[]`）。
    pub fn index(index: i64, length: i64) -> Self {
        Self::range(index, 0, length - 1, "length")
    }

    /// 空のリストの `first` など。
    pub fn no_element() -> Self {
        Self::state("No element")
    }

    /// `!` が null に当たった。
    pub fn null_check() -> Self {
        Self("Null check operator used on a null value".into())
    }

    /// `toInt()` などが NaN・無限に当たった。
    pub fn to_int() -> Self {
        Self::unsupported("Infinity or NaN toInt")
    }
}

/// `Error.safeToString` の文字列（JSON と同じ引用）。
fn safe_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `String.trim`・`double.parse` が飛ばす空白（petitparser の `whitespace()` も同じ）。
pub fn is_ws(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{85}' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// 正規表現の `\s`（JavaScript と同じ。U+0085 を含まない）。
pub fn is_js_space(c: char) -> bool {
    c != '\u{85}' && is_ws(c)
}

/// `String.trim`。
pub fn trim(s: &str) -> &str {
    s.trim_matches(is_ws)
}

/// `String.replaceFirst(from, '')`。
pub fn remove_first(s: &str, from: &str) -> String {
    match s.find(from) {
        Some(i) => format!("{}{}", &s[..i], &s[i + from.len()..]),
        None => s.to_owned(),
    }
}

/// `double.tryParse`。
pub fn try_parse_double(s: &str) -> Option<f64> {
    let t = trim(s);
    if t.is_empty() {
        return None;
    }
    let (negative, body) = match t.as_bytes()[0] {
        b'-' => (true, &t[1..]),
        b'+' => (false, &t[1..]),
        _ => (false, t),
    };
    let signed = |v: f64| if negative { -v } else { v };
    match body {
        "NaN" => return Some(f64::NAN),
        "Infinity" => return Some(signed(f64::INFINITY)),
        _ => {}
    }
    let b = body.as_bytes();
    let mut i = 0;
    let mut digits = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
        digits += 1;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return None;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            return None;
        }
    }
    if i != b.len() {
        return None;
    }
    t.parse::<f64>().ok()
}

/// `double.parse`。
pub fn parse_double(s: &str) -> R<f64> {
    try_parse_double(s).ok_or_else(|| Error::format("Invalid double", s))
}

/// `int.tryParse(s, radix: radix)`。
pub fn try_parse_int(s: &str, radix: u32) -> Option<i64> {
    let t = trim(s);
    let (negative, body) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    if body.is_empty() || !body.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    let v = i64::from_str_radix(body, radix).ok()?;
    Some(if negative { -v } else { v })
}

/// `int.parse(source, radix: radix)`（失敗したら、最初のおかしな文字の位置を示す）。
pub fn parse_int(source: &str, radix: u32) -> R<i64> {
    let chars: Vec<char> = source.chars().collect();
    // Dart VM は位置を 0 として報せる
    let message = format!("Invalid radix-{radix} number");
    let fail = |_at: usize| Error::format_at(&message, source, 0);
    let Some(last) = chars.iter().rposition(|c| !is_ws(*c)) else {
        return Err(fail(chars.len()));
    };
    let end = last + 1;
    let mut i = chars.iter().position(|c| !is_ws(*c)).unwrap_or(0);
    let mut negative = false;
    if chars[i] == '+' || chars[i] == '-' {
        negative = chars[i] == '-';
        i += 1;
        if i == end {
            return Err(fail(end));
        }
    }
    let mut value: i64 = 0;
    for (k, c) in chars.iter().enumerate().take(end).skip(i) {
        let Some(d) = c.to_digit(radix) else {
            return Err(fail(k));
        };
        value = value
            .wrapping_mul(i64::from(radix))
            .wrapping_add(i64::from(d));
    }
    Ok(if negative { -value } else { value })
}

/// `math.min`（NaN を伝え、-0.0 を区別する）。
pub fn min(a: f64, b: f64) -> f64 {
    if a > b {
        return b;
    }
    if a < b {
        return a;
    }
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a == 0.0 {
        return if a.is_sign_negative() || b.is_sign_negative() {
            -0.0
        } else {
            0.0
        };
    }
    a
}

/// `math.max`（NaN を伝え、-0.0 を区別する）。
pub fn max(a: f64, b: f64) -> f64 {
    if a > b {
        return a;
    }
    if a < b {
        return b;
    }
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a == 0.0 {
        return a + b;
    }
    a
}

/// `num.clamp(lower, upper)`。
pub fn clamp(v: f64, lower: f64, upper: f64) -> f64 {
    if v < lower {
        lower
    } else if v > upper {
        upper
    } else {
        v
    }
}

/// `double.toInt()`（0 の方へ切る）。
pub fn to_int(v: f64) -> R<i64> {
    if !v.is_finite() {
        return Err(Error::to_int());
    }
    Ok(v.trunc() as i64)
}

/// `double.round()`（0 から遠い方へ）。
pub fn round(v: f64) -> R<i64> {
    if !v.is_finite() {
        return Err(Error::to_int());
    }
    Ok(v.round() as i64)
}

/// `double.toString()`。
pub fn double_to_string(v: f64) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if v == 0.0 {
        return if v.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    // 最短の桁と指数
    let e = format!("{v:e}");
    let (mantissa, exp) = e.split_once('e').unwrap_or((&e, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let negative = mantissa.starts_with('-');
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let sign = if negative { "-" } else { "" };
    if !(-6..21).contains(&exp) {
        let (first, rest) = digits.split_at(1);
        let frac = if rest.is_empty() {
            String::new()
        } else {
            format!(".{rest}")
        };
        let exp_sign = if exp >= 0 { "+" } else { "-" };
        return format!("{sign}{first}{frac}e{exp_sign}{}", exp.abs());
    }
    let n = digits.len() as i32;
    if exp >= n - 1 {
        // 整数
        let zeros = "0".repeat((exp - (n - 1)) as usize);
        format!("{sign}{digits}{zeros}.0")
    } else if exp >= 0 {
        let (int, frac) = digits.split_at((exp + 1) as usize);
        format!("{sign}{int}.{frac}")
    } else {
        let zeros = "0".repeat((-exp - 1) as usize);
        format!("{sign}0.{zeros}{digits}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doubles() {
        assert_eq!(try_parse_double(" 1.5 "), Some(1.5));
        assert_eq!(try_parse_double("1."), Some(1.0));
        assert_eq!(try_parse_double(".5"), Some(0.5));
        assert_eq!(try_parse_double("+1e2"), Some(100.0));
        assert_eq!(try_parse_double("-Infinity"), Some(f64::NEG_INFINITY));
        assert!(try_parse_double("NaN").unwrap().is_nan());
        for bad in ["", ".", "1mm", "e5", "1e", "inf", "0x10", "1 2"] {
            assert_eq!(try_parse_double(bad), None, "{bad}");
        }
        assert_eq!(
            parse_double("1mm").unwrap_err().0,
            "FormatException: Invalid double\n1mm"
        );
        assert_eq!(double_to_string(39.0), "39.0");
        assert_eq!(double_to_string(-0.5), "-0.5");
        assert_eq!(double_to_string(1e21), "1e+21");
        assert_eq!(double_to_string(1.5e-7), "1.5e-7");
        assert_eq!(double_to_string(0.001), "0.001");
        assert_eq!(double_to_string(123.456), "123.456");
        assert!(min(f64::NAN, 1.0).is_nan());
        assert!(min(0.0, -0.0).is_sign_negative());
        assert!(!max(0.0, -0.0).is_sign_negative());
    }
}
