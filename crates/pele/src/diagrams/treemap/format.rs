//! treemap の値の書き方（pele の `src/diagrams/treemap/format.ts`）: d3-format の指定のうち、treemap の値が使う
//! 部分。頭の `$`、千の区切りの `,`、精度、型の `f`・`%`・`d`・`e` かなし。埋め・揃え・幅は読んで捨てる。ほかは
//! 既定の `,` になる。
//!
//! JS の `toFixed`・`toExponential`・`toPrecision` もここで写す（値の正確な 10 進の桁から、ちょうど半分は大きい方に
//! 丸める）。

use crate::js;

/// 正の有限の数の正確な 10 進の桁と、小数点の位置（値は 0.d1d2… × 10^point）。0 は空の桁。
fn exact_digits(x: f64) -> (Vec<u8>, i32) {
    if x == 0.0 {
        return (Vec::new(), 0);
    }
    // 倍精度の数の正確な 10 進の展開は 767 桁を超えない。
    let s = format!("{:.800e}", x);
    let (mantissa, exp) = s.split_once('e').unwrap();
    let mut digits: Vec<u8> = mantissa
        .bytes()
        .filter(u8::is_ascii_digit)
        .map(|b| b - b'0')
        .collect();
    while digits.last() == Some(&0) {
        digits.pop();
    }
    (digits, exp.parse::<i32>().unwrap() + 1)
}

/// 頭から `keep` 桁に、ちょうど半分は上へ丸める。答えの桁は `keep` 桁を超えない（繰り上がりで point が増える）。
fn round_digits(digits: &[u8], point: i32, keep: i32) -> (Vec<u8>, i32) {
    if keep < 0 || digits.is_empty() {
        return (Vec::new(), point);
    }
    let keep = keep as usize;
    if digits.len() <= keep {
        return (digits.to_vec(), point);
    }
    let mut out = digits[..keep].to_vec();
    if digits[keep] >= 5 {
        let mut i = keep;
        loop {
            if i == 0 {
                out.insert(0, 1);
                if out.len() > keep.max(1) {
                    out.pop();
                }
                return (out, point + 1);
            }
            i -= 1;
            if out[i] == 9 {
                out[i] = 0;
            } else {
                out[i] += 1;
                break;
            }
        }
    }
    (out, point)
}

fn text(digits: &[u8]) -> String {
    digits.iter().map(|d| (b'0' + d) as char).collect()
}

/// `Number.prototype.toFixed`（正の数）。
pub fn to_fixed(x: f64, f: usize) -> String {
    if !x.is_finite() || x >= 1e21 {
        return js::fmt(x);
    }
    let (digits, point) = exact_digits(x);
    let (r, p) = round_digits(&digits, point, point + f as i32);
    let total = p + f as i32;
    let mut m = if r.is_empty() || total <= 0 {
        "0".to_string()
    } else {
        let mut s = text(&r);
        while (s.len() as i32) < total {
            s.push('0');
        }
        s
    };
    if f != 0 {
        let mut k = m.len();
        if k <= f {
            m = "0".repeat(f + 1 - k) + &m;
            k = f + 1;
        }
        m = format!("{}.{}", &m[..k - f], &m[k - f..]);
    }
    m
}

/// 有効数字 `count` 桁の桁と指数（正の数。0 は 0 の並び）。
fn significant(x: f64, count: usize) -> (String, i32) {
    if x == 0.0 {
        return ("0".repeat(count), 0);
    }
    let (digits, point) = exact_digits(x);
    let (r, p) = round_digits(&digits, point, count as i32);
    let mut s = text(&r);
    while s.len() < count {
        s.push('0');
    }
    (s, p - 1)
}

fn exponent(e: i32) -> String {
    format!("e{}{}", if e >= 0 { "+" } else { "-" }, e.abs())
}

/// `Number.prototype.toExponential`（正の数）。
pub fn to_exponential(x: f64, f: usize) -> String {
    if !x.is_finite() {
        return js::fmt(x);
    }
    let (m, e) = significant(x, f + 1);
    let head = if f == 0 {
        m
    } else {
        format!("{}.{}", &m[..1], &m[1..])
    };
    head + &exponent(e)
}

/// `Number.prototype.toPrecision`（正の数）。
pub fn to_precision(x: f64, p: usize) -> String {
    if !x.is_finite() {
        return js::fmt(x);
    }
    let (m, e) = significant(x, p);
    if e < -6 || e >= p as i32 {
        let head = if p == 1 {
            m
        } else {
            format!("{}.{}", &m[..1], &m[1..])
        };
        return head + &exponent(e);
    }
    if e == p as i32 - 1 {
        return m;
    }
    if e >= 0 {
        let k = (e + 1) as usize;
        return format!("{}.{}", &m[..k], &m[k..]);
    }
    format!("0.{}{}", "0".repeat((-(e + 1)) as usize), m)
}

/// 指定の組: 符号、`$`、`,`、精度、`~`、型。
#[derive(Clone, Debug, Default, PartialEq)]
struct Spec {
    sign: Option<u16>,
    dollar: bool,
    comma: bool,
    precision: Option<String>,
    tilde: bool,
    type_: Option<u16>,
}

fn is_digit(c: u16) -> bool {
    (48..=57).contains(&c)
}

/// `([-+( ])?(\$)?0?\d*(,)?(?:\.(\d+))?(~)?([a-zA-Z%])?$`
fn rest(s: &[u16], mut i: usize) -> Option<Spec> {
    let mut spec = Spec::default();
    let at = |i: usize| s.get(i).copied();
    if let Some(c) = at(i).filter(|c| matches!(c, 45 | 43 | 40 | 32)) {
        spec.sign = Some(c);
        i += 1;
    }
    if at(i) == Some(36) {
        spec.dollar = true;
        i += 1;
    }
    if at(i) == Some(48) {
        i += 1;
    }
    while at(i).is_some_and(is_digit) {
        i += 1;
    }
    if at(i) == Some(44) {
        spec.comma = true;
        i += 1;
    }
    if at(i) == Some(46) && at(i + 1).is_some_and(is_digit) {
        let from = i + 1;
        i += 1;
        while at(i).is_some_and(is_digit) {
            i += 1;
        }
        spec.precision = Some(js::from16(&s[from..i]));
    }
    if at(i) == Some(126) {
        spec.tilde = true;
        i += 1;
    }
    if let Some(c) = at(i).filter(|&c| c == 37 || (c < 128 && (c as u8).is_ascii_alphabetic())) {
        spec.type_ = Some(c);
        i += 1;
    }
    (i == s.len()).then_some(spec)
}

fn align(c: u16) -> bool {
    matches!(c, 60 | 62 | 61 | 94)
}

/// `^(?:.?[<>=^])?…$`
fn read_spec(spec: &str) -> Option<Spec> {
    let s = js::utf16(spec);
    if s.len() >= 2
        && !matches!(s[0], 10 | 13 | 0x2028 | 0x2029)
        && align(s[1])
        && let Some(m) = rest(&s, 2)
    {
        return Some(m);
    }
    if s.first().is_some_and(|&c| align(c))
        && let Some(m) = rest(&s, 1)
    {
        return Some(m);
    }
    rest(&s, 0)
}

fn group(digits: &str) -> String {
    let b = digits.as_bytes();
    let mut out = String::new();
    let mut i = b.len();
    while i > 0 {
        let part = &digits[i.saturating_sub(3)..i];
        out = if out.is_empty() {
            part.to_string()
        } else {
            format!("{part},{out}")
        };
        i = i.saturating_sub(3);
    }
    out
}

pub struct Formatter {
    spec: Spec,
    precision: Option<usize>,
}

pub fn formatter(spec: &str) -> Formatter {
    // Mermaid は千の区切りつきのドルを `$0,0` と書くと説明している。
    let m = read_spec(if spec == "$0,0" { "$," } else { spec })
        .unwrap_or_else(|| read_spec(",").unwrap());
    let precision = m
        .precision
        .as_ref()
        .map(|p| js::min(js::number(p), 20.0) as usize);
    Formatter { spec: m, precision }
}

impl Formatter {
    pub fn format(&self, value: f64) -> String {
        let m = &self.spec;
        let abs = value.abs();
        let mut suffix = "";
        let mut body = match m.type_ {
            Some(102) => to_fixed(abs, self.precision.unwrap_or(6)),
            Some(37) => {
                suffix = "%";
                to_fixed(abs * 100.0, self.precision.unwrap_or(6))
            }
            Some(100) => to_fixed(js::round(abs), 0),
            Some(101) => to_exponential(abs, self.precision.unwrap_or(6)),
            _ => js::fmt(js::number(&to_precision(
                abs,
                self.precision.unwrap_or(12).max(1),
            ))),
        };
        if m.tilde && body.contains('.') && !body.contains('e') && body.ends_with('0') {
            body.truncate(body.trim_end_matches('0').len());
            if body.ends_with('.') {
                body.pop();
            }
        }
        if m.comma && !body.contains('e') {
            body = match body.find('.') {
                None => group(&body),
                Some(dot) => group(&body[..dot]) + &body[dot..],
            };
        }
        let sign = if value < 0.0 {
            "-".to_string()
        } else if m.sign == Some(43) {
            "+".to_string()
        } else {
            String::new()
        };
        format!("{sign}{}{body}{suffix}", if m.dollar { "$" } else { "" })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_like_js() {
        assert_eq!(to_fixed(2.5, 0), "3");
        assert_eq!(to_fixed(1.005, 2), "1.00");
        assert_eq!(to_fixed(0.000001, 2), "0.00");
        assert_eq!(to_fixed(0.5, 0), "1");
        assert_eq!(to_fixed(0.05, 1), "0.1");
        assert_eq!(to_fixed(99.99, 1), "100.0");
        assert_eq!(to_fixed(1e21, 2), "1e+21");
        assert_eq!(to_exponential(12345.0, 2), "1.23e+4");
        assert_eq!(to_exponential(0.0, 2), "0.00e+0");
        assert_eq!(to_exponential(9.99, 1), "1.0e+1");
        assert_eq!(to_precision(123456.0, 2), "1.2e+5");
        assert_eq!(to_precision(0.00000123, 2), "0.0000012");
        assert_eq!(to_precision(0.000000123, 2), "1.2e-7");
        assert_eq!(to_precision(123.456, 4), "123.5");
        assert_eq!(to_precision(0.1 + 0.2, 12), "0.300000000000");
        assert_eq!(formatter(",").format(1234567.0), "1,234,567");
        assert_eq!(formatter("$0,0").format(1234.5), "$1,234.5");
        assert_eq!(formatter(".1%").format(0.256), "25.6%");
        assert_eq!(formatter(">+.2f").format(3.0), "+3.00");
        assert_eq!(formatter("x<,.3~f").format(1.5), "1.5");
        assert_eq!(formatter("bogus").format(1000.0), "1,000");
    }
}
