//! JavaScript の数・文字列のふるまいのうち、出力に届くもの。pele と同じ SVG を書くために、`Math.round`・
//! `Math.min`/`Math.max`（NaN を返す）・`Math.hypot`（V8 の Kahan 和）・`Number.prototype.toString`・
//! `parseFloat`・`Number()` を JS と同じに写す。

use std::cmp::Ordering;

/// `Math.round`: 半分は +∞ の側へ。V8 と同じく ceil から戻す（`floor(x + 0.5)` は 0.49999999999999994 で違う）。
pub fn round(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let c = x.ceil();
    if c - 0.5 > x { c - 1.0 } else { c }
}

/// `Math.min(a, b)`: どちらかが NaN なら NaN。-0 は 0 より小さい。
pub fn min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a == b {
        // -0 と 0
        if a == 0.0 && (a.is_sign_negative() || b.is_sign_negative()) {
            return -0.0;
        }
        return a;
    }
    if a < b { a } else { b }
}

/// `Math.max(a, b)`: どちらかが NaN なら NaN。
pub fn max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a == b {
        if a == 0.0 && (a.is_sign_positive() || b.is_sign_positive()) {
            return 0.0;
        }
        return a;
    }
    if a > b { a } else { b }
}

/// `Math.min(...list)`。空なら Infinity。
pub fn min_of(list: &[f64]) -> f64 {
    list.iter().fold(f64::INFINITY, |m, &v| min(m, v))
}

/// `Math.max(...list)`。空なら -Infinity。
pub fn max_of(list: &[f64]) -> f64 {
    list.iter().fold(f64::NEG_INFINITY, |m, &v| max(m, v))
}

/// `Math.sign`。
pub fn sign(x: f64) -> f64 {
    if x.is_nan() || x == 0.0 {
        x
    } else if x > 0.0 {
        1.0
    } else {
        -1.0
    }
}

/// `Math.hypot(a, b)`。V8 は最大の値で割ってから Kahan 和で足し、sqrt に最大を掛ける。
pub fn hypot(a: f64, b: f64) -> f64 {
    hypot_of(&[a, b])
}

pub fn hypot_of(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut nan = false;
    let mut max = 0.0f64;
    for &v in values {
        if v.is_nan() {
            nan = true;
        } else {
            let a = v.abs();
            if a > max {
                max = a;
            }
        }
    }
    if max == f64::INFINITY {
        return f64::INFINITY;
    }
    if nan {
        return f64::NAN;
    }
    if max == 0.0 {
        return 0.0;
    }
    let mut sum = 0.0f64;
    let mut compensation = 0.0f64;
    for &v in values {
        let n = v.abs() / max;
        let summand = n * n - compensation;
        let preliminary = sum + summand;
        compensation = (preliminary - sum) - summand;
        sum = preliminary;
    }
    sum.sqrt() * max
}

/// `x || y` のうち数の真偽（0 と NaN は偽）。
pub fn truthy(x: f64) -> bool {
    x != 0.0 && !x.is_nan()
}

/// 比べる関数の返す数を `Ordering` に（NaN は等しい扱い。V8 は NaN を 0 とみなす）。
pub fn order(v: f64) -> Ordering {
    if v < 0.0 {
        Ordering::Less
    } else if v > 0.0 {
        Ordering::Greater
    } else {
        Ordering::Equal
    }
}

/// `ToInt32`（`x | 0`・`x >> 0`）。
pub fn to_int32(x: f64) -> i32 {
    if !x.is_finite() {
        return 0;
    }
    let t = x.trunc();
    let m = t.rem_euclid(4294967296.0);
    (m as u64 as u32) as i32
}

/// `Number.prototype.toString()`（基数 10）。
pub fn fmt(x: f64) -> String {
    let mut out = String::new();
    write_num(&mut out, x);
    out
}

/// `fmt` を `out` に足す。
pub fn write_num(out: &mut String, x: f64) {
    if x.is_nan() {
        out.push_str("NaN");
        return;
    }
    if x == 0.0 {
        out.push('0');
        return;
    }
    if x.is_infinite() {
        out.push_str(if x > 0.0 { "Infinity" } else { "-Infinity" });
        return;
    }
    // 整数で安全な範囲はそのまま。
    if x.fract() == 0.0 && x.abs() < 1e21 && x.abs() < 9.007_199_254_740_992e15 {
        use std::fmt::Write as _;
        let _ = write!(out, "{}", x as i64);
        return;
    }
    if x < 0.0 {
        out.push('-');
    }
    // Rust の `{:e}` は最短で戻せる桁を出す（JS と同じ桁）。
    let s = format!("{:e}", x.abs());
    let (mantissa, exp) = s.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exp + 1;
    if k <= n && n <= 21 {
        out.push_str(&digits);
        for _ in 0..(n - k) {
            out.push('0');
        }
    } else if 0 < n && n <= 21 {
        out.push_str(&digits[..n as usize]);
        out.push('.');
        out.push_str(&digits[n as usize..]);
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        for _ in 0..(-n) {
            out.push('0');
        }
        out.push_str(&digits);
    } else {
        out.push_str(&digits[..1]);
        if k > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        out.push(if n > 0 { '+' } else { '-' });
        use std::fmt::Write as _;
        let _ = write!(out, "{}", (n - 1).abs());
    }
}

/// JS の空白（`String.prototype.trim` が取るもの）。
pub fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\u{9}' | '\u{a}' | '\u{b}' | '\u{c}' | '\u{d}' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// `String.prototype.trim`。
pub fn trim(s: &str) -> &str {
    s.trim_matches(is_js_space)
}

pub fn trim_start(s: &str) -> &str {
    s.trim_start_matches(is_js_space)
}

pub fn trim_end(s: &str) -> &str {
    s.trim_end_matches(is_js_space)
}

/// 10 進の数の頭（`StrDecimalLiteral` の接頭辞）の長さ。`Infinity` も読む。
fn decimal_prefix(s: &str) -> usize {
    let b = s.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    if s[i..].starts_with("Infinity") {
        return i + 8;
    }
    let start = i;
    let mut digits = false;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
        digits = true;
    }
    if i < b.len() && b[i] == b'.' {
        let mut j = i + 1;
        let mut frac = false;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
            frac = true;
        }
        if digits || frac {
            i = j;
            digits = true;
        }
    }
    if !digits {
        let _ = start;
        return 0;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        let ds = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > ds {
            i = j;
        }
    }
    i
}

fn parse_decimal(s: &str) -> f64 {
    let t = s.trim_start_matches('+');
    if let Some(rest) = t.strip_prefix('-')
        && rest == "Infinity"
    {
        return f64::NEG_INFINITY;
    }
    if t == "Infinity" {
        return f64::INFINITY;
    }
    // Rust の f64 の読みは正しく丸める（JS と同じ）。".5"・"5." も読める。
    t.parse::<f64>().unwrap_or(f64::NAN)
}

/// `parseFloat`。
pub fn parse_float(s: &str) -> f64 {
    let s = trim_start(s);
    let n = decimal_prefix(s);
    if n == 0 {
        return f64::NAN;
    }
    parse_decimal(&s[..n])
}

/// `Number(s)`（文字列から）。
pub fn number(s: &str) -> f64 {
    let s = trim(s);
    if s.is_empty() {
        return 0.0;
    }
    let lower = s.get(..2).map(|p| p.to_ascii_lowercase());
    let radix = match lower.as_deref() {
        Some("0x") => 16,
        Some("0o") => 8,
        Some("0b") => 2,
        _ => 0,
    };
    if radix != 0 {
        let body = &s[2..];
        if body.is_empty() {
            return f64::NAN;
        }
        let mut v = 0.0f64;
        for c in body.chars() {
            match c.to_digit(radix) {
                Some(d) => v = v * radix as f64 + d as f64,
                None => return f64::NAN,
            }
        }
        return v;
    }
    let n = decimal_prefix(s);
    if n != s.len() {
        return f64::NAN;
    }
    parse_decimal(s)
}

/// `parseInt(s, radix)`（radix は 2〜36）。
pub fn parse_int(s: &str, radix: u32) -> f64 {
    let s = trim_start(s);
    let (neg, body) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let body = if radix == 16 && (body.starts_with("0x") || body.starts_with("0X")) {
        &body[2..]
    } else {
        body
    };
    if radix == 10 {
        // V8 は 10 進を正しく丸めて読む（桁を 1 つずつ掛け足すと、15 桁を超えて最後の桁がずれる）。
        let end = body
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(body.len());
        if end == 0 {
            return f64::NAN;
        }
        let v: f64 = body[..end].parse().unwrap_or(f64::NAN);
        return if neg { -v } else { v };
    }
    let mut v = 0.0f64;
    let mut any = false;
    for c in body.chars() {
        match c.to_digit(radix) {
            Some(d) => {
                v = v * radix as f64 + d as f64;
                any = true;
            }
            None => break,
        }
    }
    if !any {
        return f64::NAN;
    }
    if neg { -v } else { v }
}

/// UTF-16 の長さ（`String.prototype.length`）。
pub fn len16(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// UTF-16 に。
pub fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// UTF-16 から（対のない サロゲートは U+FFFD に）。
pub fn from16(s: &[u16]) -> String {
    String::from_utf16_lossy(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_like_js() {
        assert_eq!(fmt(1.0), "1");
        assert_eq!(fmt(-2.5), "-2.5");
        assert_eq!(fmt(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(fmt(1e21), "1e+21");
        assert_eq!(fmt(1.5e-7), "1.5e-7");
        assert_eq!(fmt(0.000001), "0.000001");
        assert_eq!(fmt(123456789012345680000.0), "123456789012345680000");
        assert_eq!(round(2.5), 3.0);
        assert_eq!(round(-2.5), -2.0);
        assert_eq!(round(0.49999999999999994), 0.0);
        assert_eq!(parse_float("12.5px"), 12.5);
        assert!(parse_float("px").is_nan());
        assert_eq!(number(" 0x1f "), 31.0);
        assert!(number("1px").is_nan());
        assert_eq!(number(""), 0.0);
        assert_eq!(hypot(3.0, 4.0), 5.0);
        assert_eq!(parse_int("99999999999999999999999*", 10), 1e23);
        assert_eq!(parse_int(" -12px", 10), -12.0);
        assert!(parse_int("x", 10).is_nan());
        assert_eq!(parse_int(&"9".repeat(400), 10), f64::INFINITY);
    }
}
