//! Svg2Vector の出力に出る Java の標準ライブラリのふるまい（JDK 21 で確かめた）。数の読み書き（`Float.parseFloat`・
//! `Float.toString`・`DecimalFormat` の HALF_UP）、`String.split`・`trim`、`HashMap` の並び、`java.awt.geom` の
//! `AffineTransform` と `Path2D` の外接矩形。
//!
//! `AffineTransform` は Java と同じく状態（state）で計算の道を分ける。道によって 0 の符号が変わり、それが
//! "-0" として出力に出るため。

use std::fmt;

/// Java の例外。`Display` は `Throwable.toString()`（スタックトレースの 1 行目）と同じ。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Thrown {
    pub class: &'static str,
    pub message: Option<String>,
}

pub const NUMBER_FORMAT: &str = "java.lang.NumberFormatException";
pub const ILLEGAL_ARGUMENT: &str = "java.lang.IllegalArgumentException";
pub const ILLEGAL_STATE: &str = "java.lang.IllegalStateException";
pub const RUNTIME: &str = "java.lang.RuntimeException";
pub const INDEX_OUT_OF_BOUNDS: &str = "java.lang.ArrayIndexOutOfBoundsException";
pub const NULL_POINTER: &str = "java.lang.NullPointerException";
pub const CLASS_CAST: &str = "java.lang.ClassCastException";
pub const ERROR: &str = "java.lang.Error";

impl Thrown {
    pub fn new(class: &'static str, message: impl Into<String>) -> Self {
        Self {
            class,
            message: Some(message.into()),
        }
    }

    /// `catch (IllegalArgumentException e)` で捕まるか（`NumberFormatException` は子）。
    pub fn is_illegal_argument(&self) -> bool {
        self.class == ILLEGAL_ARGUMENT || self.class == NUMBER_FORMAT
    }

    pub fn is_number_format(&self) -> bool {
        self.class == NUMBER_FORMAT
    }

    pub fn is_index_out_of_bounds(&self) -> bool {
        self.class == INDEX_OUT_OF_BOUNDS
    }
}

impl fmt::Display for Thrown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.message {
            Some(m) => write!(f, "{}: {m}", self.class),
            None => f.write_str(self.class),
        }
    }
}

pub type J<T> = Result<T, Thrown>;

/// 配列の添字。範囲の外は `ArrayIndexOutOfBoundsException`。
pub fn at<T: Copy>(array: &[T], index: isize) -> J<T> {
    if index >= 0 && (index as usize) < array.len() {
        Ok(array[index as usize])
    } else {
        Err(index_out_of_bounds(index, array.len()))
    }
}

pub fn index_out_of_bounds(index: isize, len: usize) -> Thrown {
    Thrown::new(
        INDEX_OUT_OF_BOUNDS,
        format!("Index {index} out of bounds for length {len}"),
    )
}

// ---- 文字列 ----

/// `String.trim()`: 両端の U+0020 以下を落とす。
pub fn trim(s: &str) -> &str {
    s.trim_matches(|c: char| c <= ' ')
}

/// 正規表現の `\s`。
pub fn is_regex_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r')
}

/// `String.split(regex)`: 区切り（`is_sep` に合う文字の 1 つ、`plus` なら続く並び）で分け、末尾の空を落とす。
/// 区切りがなければ元の文字列 1 つ。
pub fn split(s: &str, plus: bool, is_sep: impl Fn(char) -> bool) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut found = false;
    let mut chars = s.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if is_sep(c) {
            found = true;
            out.push(&s[start..i]);
            let mut end = i + c.len_utf8();
            if plus {
                while let Some(&(j, d)) = chars.peek() {
                    if !is_sep(d) {
                        break;
                    }
                    end = j + d.len_utf8();
                    chars.next();
                }
            }
            start = end;
        }
    }
    if !found {
        return vec![s];
    }
    out.push(&s[start..]);
    while out.last().is_some_and(|p| p.is_empty()) {
        out.pop();
    }
    out
}

/// `String.split` で 1 文字の区切り。
pub fn split_char(s: &str, sep: char) -> Vec<&str> {
    split(s, false, |c| c == sep)
}

/// `String.compareTo`（UTF-16 の単位で比べる）。
pub fn compare(a: &str, b: &str) -> std::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// `String.hashCode()`。
pub fn string_hash(s: &str) -> i32 {
    s.encode_utf16()
        .fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32))
}

// ---- 数を読む ----

fn number_format(s: &str) -> Thrown {
    Thrown::new(NUMBER_FORMAT, format!("For input string: \"{s}\""))
}

/// `FloatingDecimal.readJavaFormatString` の形を確かめ、Rust の `parse` に渡せる形にする。
fn java_decimal(s: &str) -> J<JavaNumber<'_>> {
    let t = trim(s);
    if t.is_empty() {
        return Err(Thrown::new(NUMBER_FORMAT, "empty String"));
    }
    let b = t.as_bytes();
    let mut i = 0;
    let mut negative = false;
    if b[0] == b'+' || b[0] == b'-' {
        negative = b[0] == b'-';
        i = 1;
    }
    let rest = &t[i..];
    if rest == "NaN" {
        return Ok(JavaNumber::NaN);
    }
    if rest == "Infinity" {
        return Ok(JavaNumber::Infinite(negative));
    }
    if rest.len() > 1 && (rest.starts_with("0x") || rest.starts_with("0X")) {
        return parse_hex(rest, negative).ok_or_else(|| number_format(t));
    }
    let mut digits = 0;
    let mut dot = false;
    while i < b.len() {
        match b[i] {
            b'0'..=b'9' => digits += 1,
            b'.' if !dot => dot = true,
            _ => break,
        }
        i += 1;
    }
    if digits == 0 {
        return Err(number_format(t));
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let exp_start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == exp_start {
            return Err(number_format(t));
        }
    }
    let end = i;
    if i < b.len() && matches!(b[i], b'f' | b'F' | b'd' | b'D') {
        i += 1;
    }
    if i != b.len() {
        return Err(number_format(t));
    }
    // Rust の parse は先頭の符号を受けつける。f・d の接尾辞だけ落とす
    Ok(JavaNumber::Decimal(&t[..end]))
}

enum JavaNumber<'a> {
    NaN,
    Infinite(bool),
    Decimal(&'a str),
    Hex(f64),
}

/// 16 進の浮動小数点（`0x1.8p3`）。めったに使われないので double で近似する。
fn parse_hex(s: &str, negative: bool) -> Option<JavaNumber<'static>> {
    let body = &s[2..];
    let (mantissa, exp) = body.split_once(['p', 'P'])?;
    let exp = exp.trim_end_matches(['f', 'F', 'd', 'D']);
    let exp: i32 = exp.parse().ok()?;
    let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if int.is_empty() && frac.is_empty() {
        return None;
    }
    let mut value = 0f64;
    for c in int.chars() {
        value = value * 16.0 + c.to_digit(16)? as f64;
    }
    let mut scale = 1.0 / 16.0;
    for c in frac.chars() {
        value += c.to_digit(16)? as f64 * scale;
        scale /= 16.0;
    }
    let value = value * 2f64.powi(exp);
    Some(JavaNumber::Hex(if negative { -value } else { value }))
}

/// `Double.parseDouble`。
pub fn parse_double(s: &str) -> J<f64> {
    Ok(match java_decimal(s)? {
        JavaNumber::NaN => f64::NAN,
        JavaNumber::Infinite(n) => {
            if n {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            }
        }
        JavaNumber::Decimal(d) => d.parse().map_err(|_| number_format(trim(s)))?,
        JavaNumber::Hex(v) => v,
    })
}

/// `Float.parseFloat`。
pub fn parse_float(s: &str) -> J<f32> {
    Ok(match java_decimal(s)? {
        JavaNumber::NaN => f32::NAN,
        JavaNumber::Infinite(n) => {
            if n {
                f32::NEG_INFINITY
            } else {
                f32::INFINITY
            }
        }
        JavaNumber::Decimal(d) => d.parse().map_err(|_| number_format(trim(s)))?,
        JavaNumber::Hex(v) => v as f32,
    })
}

/// `Integer.parseInt`（10 進）。
pub fn parse_int(s: &str) -> J<i32> {
    let b = s.as_bytes();
    let digits = match b.first() {
        Some(b'+') | Some(b'-') => &s[1..],
        _ => s,
    };
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return Err(number_format(s));
    }
    s.parse().map_err(|_| number_format(s))
}

/// `Integer.parseUnsignedInt(s, 16)`。
pub fn parse_unsigned_hex(s: &str) -> J<u32> {
    let radix = |s: &str| {
        Thrown::new(
            NUMBER_FORMAT,
            format!("For input string: \"{s}\" under radix 16"),
        )
    };
    if s.is_empty() {
        return Err(Thrown::new(NUMBER_FORMAT, ""));
    }
    if s.starts_with('-') {
        return Err(Thrown::new(
            NUMBER_FORMAT,
            format!("Illegal leading minus sign on unsigned string {s}."),
        ));
    }
    let digits = s.strip_prefix('+').unwrap_or(s);
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(radix(s));
    }
    let value = u64::from_str_radix(digits, 16).map_err(|_| radix(s))?;
    u32::try_from(value).map_err(|_| {
        Thrown::new(
            NUMBER_FORMAT,
            format!("String value {s} exceeds range of unsigned int."),
        )
    })
}

// ---- 数を書く ----

/// 最短の 10 進の桁と指数（`d.ddd × 10^exp`）。
fn shortest_digits(a: impl fmt::LowerExp) -> (String, i32) {
    let s = format!("{a:e}");
    let (mantissa, exp) = s.split_once('e').unwrap_or((&s, "0"));
    let digits: String = mantissa.chars().filter(|c| c.is_ascii_digit()).collect();
    (digits, exp.parse().unwrap_or(0))
}

/// `Float.toString(float)`（JDK 19 以降の最短の表記）。
pub fn float_to_string(f: f32) -> String {
    if f.is_nan() {
        return "NaN".into();
    }
    if f.is_infinite() {
        return if f < 0.0 { "-Infinity" } else { "Infinity" }.into();
    }
    let sign = if f.is_sign_negative() { "-" } else { "" };
    if f == 0.0 {
        return format!("{sign}0.0");
    }
    let a = f.abs();
    let (digits, exp) = shortest_digits(a);
    if (1e-3..1e7).contains(&a) {
        if exp >= 0 {
            let int_len = exp as usize + 1;
            let mut int = digits.clone();
            let frac = if int.len() > int_len {
                int.split_off(int_len)
            } else {
                while int.len() < int_len {
                    int.push('0');
                }
                String::new()
            };
            format!("{sign}{int}.{}", if frac.is_empty() { "0" } else { &frac })
        } else {
            format!("{sign}0.{}{digits}", "0".repeat((-exp - 1) as usize))
        }
    } else {
        let (first, rest) = digits.split_at(1);
        format!(
            "{sign}{first}.{}E{exp}",
            if rest.is_empty() { "0" } else { rest }
        )
    }
}

/// `DecimalUtils.trimInsignificantZeros(s)`（小数点 '.'、指数 "E"）。
pub fn trim_insignificant_zeros(s: &str) -> String {
    let b = s.as_bytes();
    let Some(dot) = s.rfind('.') else {
        return s.to_owned();
    };
    let dot = if dot == 0 { 2 } else { dot };
    let exp = s
        .get(dot.min(s.len())..)
        .and_then(|t| t.find(['E', 'e']))
        .map(|i| i + dot);
    let mut end = exp.unwrap_or(s.len()) as isize;
    loop {
        end -= 1;
        if end <= dot as isize {
            break;
        }
        if b[end as usize] != b'0' {
            end += 1;
            break;
        }
    }
    let end = end.max(0) as usize;
    match exp {
        None => s[..end.min(s.len())].to_owned(),
        Some(e) if e == end => s.to_owned(),
        Some(e) => format!("{}{}", &s[..end], &s[e..]),
    }
}

/// `XmlUtils.formatFloatValue(double)`。
pub fn format_float_value(d: f64) -> J<String> {
    if !d.is_finite() {
        return Err(Thrown::new(
            ILLEGAL_ARGUMENT,
            format!("Invalid number: {}", double_to_string(d)),
        ));
    }
    Ok(trim_insignificant_zeros(&float_to_string(d as f32)))
}

/// `Double.toString`（ここでは有限でない値と簡単な値にだけ使う）。
pub fn double_to_string(d: f64) -> String {
    if d.is_nan() {
        "NaN".into()
    } else if d.is_infinite() {
        if d < 0.0 { "-Infinity" } else { "Infinity" }.into()
    } else {
        let (digits, exp) = shortest_digits(d.abs());
        let sign = if d.is_sign_negative() { "-" } else { "" };
        if d == 0.0 {
            return format!("{sign}0.0");
        }
        let a = d.abs();
        if (1e-3..1e7).contains(&a) {
            if exp >= 0 {
                let int_len = exp as usize + 1;
                let mut int = digits.clone();
                let frac = if int.len() > int_len {
                    int.split_off(int_len)
                } else {
                    while int.len() < int_len {
                        int.push('0');
                    }
                    String::new()
                };
                format!("{sign}{int}.{}", if frac.is_empty() { "0" } else { &frac })
            } else {
                format!("{sign}0.{}{digits}", "0".repeat((-exp - 1) as usize))
            }
        } else {
            let (first, rest) = digits.split_at(1);
            format!(
                "{sign}{first}.{}E{exp}",
                if rest.is_empty() { "0" } else { rest }
            )
        }
    }
}

/// `VdUtil.getCoordinateFormat` の小数の桁数（0 なら "#"）。
pub fn coordinate_fraction_digits(max_viewport_size: f32) -> u32 {
    let log = (max_viewport_size as f64).log10().floor();
    // Kotlin の Double.toInt(): NaN は 0、範囲の外は端に寄せる
    let exponent = if log.is_nan() { 0 } else { log as i32 };
    let digits = 4i32.wrapping_sub(exponent);
    if digits > 0 { digits.min(6) as u32 } else { 0 }
}

/// `DecimalFormat("#.###", Locale.ROOT)` に `RoundingMode.HALF_UP`。丸めは double の正確な値で決める。
pub fn format_decimal(x: f64, fraction_digits: u32) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    let negative = x.is_sign_negative();
    let sign = if negative { "-" } else { "" };
    if x.is_infinite() {
        return format!("{sign}\u{221E}");
    }
    let a = x.abs();
    let n = fraction_digits as usize;
    let body = if a >= 1e16 {
        // DigitList は最短の桁を使い、残りを 0 で埋める
        let (digits, exp) = shortest_digits(a);
        let int_len = exp as usize + 1;
        let mut s = digits;
        while s.len() < int_len {
            s.push('0');
        }
        s.truncate(int_len.max(1));
        s
    } else {
        // ちょうど半分（a·10^n の小数部が 0.5）なら切り上げ、ほかは Rust の正しい丸めと同じ
        let y = a * 2f64.powi(fraction_digits as i32 + 1);
        if y.fract() == 0.0 && y % 2.0 == 1.0 {
            let up = (y as u128 * 5u128.pow(fraction_digits)).div_ceil(2);
            let s = up.to_string();
            if n == 0 {
                s
            } else {
                let s = format!("{s:0>width$}", width = n + 1);
                let (int, frac) = s.split_at(s.len() - n);
                format!("{int}.{frac}")
            }
        } else {
            format!("{a:.n$}")
        }
    };
    // '#' の桁は末尾の 0 を書かない
    let body = if body.contains('.') {
        let t = body.trim_end_matches('0');
        t.trim_end_matches('.').to_owned()
    } else {
        body
    };
    format!("{sign}{body}")
}

/// `Math.round(float)`。
pub fn round_float(x: f32) -> i32 {
    if x.is_nan() {
        return 0;
    }
    let floor = x.floor();
    let r = if x - floor >= 0.5 { floor + 1.0 } else { floor };
    r as i32
}

/// `Math.max(double, double)`（NaN を伝える）。
pub fn max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == 0.0 && b == 0.0 {
        if a.is_sign_negative() { b } else { a }
    } else if a >= b {
        a
    } else {
        b
    }
}

/// `Math.min(double, double)`（NaN を伝える）。
pub fn min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == 0.0 && b == 0.0 {
        if a.is_sign_negative() { a } else { b }
    } else if a <= b {
        a
    } else {
        b
    }
}

/// `Math.max(float, float)`。
pub fn max_f32(a: f32, b: f32) -> f32 {
    max(a as f64, b as f64) as f32
}

pub fn to_radians(deg: f64) -> f64 {
    deg * 0.017453292519943295
}

pub fn to_degrees(rad: f64) -> f64 {
    rad * 57.29577951308232
}

// ---- HashMap ----

/// `java.util.HashMap<String, V>` の並びをまねた表。キーのハッシュで決まる桶の順に、同じ桶の中は入れた順に並ぶ。
/// 容量は縮まない（16 から、要素が 3/4 を超えると倍）。
#[derive(Clone, Debug)]
pub struct HashMap<V> {
    entries: Vec<(String, V)>,
    capacity: usize,
}

impl<V> Default for HashMap<V> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            capacity: 16,
        }
    }
}

impl<V> HashMap<V> {
    pub fn get(&self, key: &str) -> Option<&V> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut V> {
        self.entries
            .iter_mut()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.entries.iter().any(|(k, _)| k == key)
    }

    /// 前の値を返す。
    pub fn put(&mut self, key: &str, value: V) -> Option<V> {
        if let Some(v) = self.get_mut(key) {
            return Some(std::mem::replace(v, value));
        }
        self.entries.push((key.to_owned(), value));
        while self.entries.len() * 4 > self.capacity * 3 {
            self.capacity *= 2;
        }
        None
    }

    pub fn remove(&mut self, key: &str) -> Option<V> {
        let i = self.entries.iter().position(|(k, _)| k == key)?;
        Some(self.entries.remove(i).1)
    }

    fn bucket(&self, key: &str) -> usize {
        let h = string_hash(key);
        let h = h ^ ((h as u32) >> 16) as i32;
        (h as u32 as usize) & (self.capacity - 1)
    }

    /// Java の `entrySet()` の順。
    pub fn iter(&self) -> impl Iterator<Item = (&str, &V)> {
        let mut order: Vec<usize> = (0..self.entries.len()).collect();
        order.sort_by_key(|&i| self.bucket(&self.entries[i].0));
        order
            .into_iter()
            .map(|i| (self.entries[i].0.as_str(), &self.entries[i].1))
    }
}

impl<V: Clone> HashMap<V> {
    /// Java の順の写し（回しながら表を変えるところ向け）。
    pub fn to_vec(&self) -> Vec<(String, V)> {
        self.iter()
            .map(|(k, v)| (k.to_owned(), v.clone()))
            .collect()
    }
}

// ---- AffineTransform ----

pub const TYPE_IDENTITY: i32 = 0;
pub const TYPE_TRANSLATION: i32 = 1;
pub const TYPE_UNIFORM_SCALE: i32 = 2;
pub const TYPE_GENERAL_SCALE: i32 = 4;
pub const TYPE_MASK_SCALE: i32 = TYPE_UNIFORM_SCALE | TYPE_GENERAL_SCALE;
const TYPE_FLIP: i32 = 64;
const TYPE_QUADRANT_ROTATION: i32 = 8;
const TYPE_GENERAL_ROTATION: i32 = 16;
const TYPE_GENERAL_TRANSFORM: i32 = 32;
const TYPE_UNKNOWN: i32 = -1;

const APPLY_IDENTITY: u8 = 0;
const APPLY_TRANSLATE: u8 = 1;
const APPLY_SCALE: u8 = 2;
const APPLY_SHEAR: u8 = 4;
const HI_SHIFT: u8 = 3;
const HI_TRANSLATE: u8 = APPLY_TRANSLATE << HI_SHIFT;
const HI_SCALE: u8 = APPLY_SCALE << HI_SHIFT;
const HI_SHEAR: u8 = APPLY_SHEAR << HI_SHIFT;

/// `java.awt.geom.AffineTransform`。
#[derive(Clone, Copy, Debug)]
pub struct AffineTransform {
    pub m00: f64,
    pub m10: f64,
    pub m01: f64,
    pub m11: f64,
    pub m02: f64,
    pub m12: f64,
    state: u8,
    kind: i32,
}

impl Default for AffineTransform {
    fn default() -> Self {
        Self {
            m00: 1.0,
            m10: 0.0,
            m01: 0.0,
            m11: 1.0,
            m02: 0.0,
            m12: 0.0,
            state: APPLY_IDENTITY,
            kind: TYPE_IDENTITY,
        }
    }
}

impl AffineTransform {
    pub fn new(m00: f64, m10: f64, m01: f64, m11: f64, m02: f64, m12: f64) -> Self {
        let mut t = Self {
            m00,
            m10,
            m01,
            m11,
            m02,
            m12,
            state: 0,
            kind: 0,
        };
        t.update_state();
        t
    }

    pub fn get_type(&mut self) -> i32 {
        if self.kind == TYPE_UNKNOWN {
            self.calculate_type();
        }
        self.kind
    }

    fn calculate_type(&mut self) {
        let mut ret = TYPE_IDENTITY;
        self.update_state();
        let s = self.state;
        if s & APPLY_SHEAR != 0 && s & APPLY_SCALE != 0 {
            if s & APPLY_TRANSLATE != 0 {
                ret = TYPE_TRANSLATION;
            }
            let (m0, m2, m3, m1) = (self.m00, self.m01, self.m10, self.m11);
            if m0 * m2 + m3 * m1 != 0.0 {
                self.kind = TYPE_GENERAL_TRANSFORM;
                return;
            }
            let sgn0 = m0 >= 0.0;
            let sgn1 = m1 >= 0.0;
            if sgn0 == sgn1 {
                if m0 != m1 || m2 != -m3 {
                    ret |= TYPE_GENERAL_ROTATION | TYPE_GENERAL_SCALE;
                } else if m0 * m1 - m2 * m3 != 1.0 {
                    ret |= TYPE_GENERAL_ROTATION | TYPE_UNIFORM_SCALE;
                } else {
                    ret |= TYPE_GENERAL_ROTATION;
                }
            } else if m0 != -m1 || m2 != m3 {
                ret |= TYPE_GENERAL_ROTATION | TYPE_FLIP | TYPE_GENERAL_SCALE;
            } else if m0 * m1 - m2 * m3 != 1.0 {
                ret |= TYPE_GENERAL_ROTATION | TYPE_FLIP | TYPE_UNIFORM_SCALE;
            } else {
                ret |= TYPE_GENERAL_ROTATION | TYPE_FLIP;
            }
        } else if s & APPLY_SHEAR != 0 {
            if s & APPLY_TRANSLATE != 0 {
                ret = TYPE_TRANSLATION;
            }
            let m0 = self.m01;
            let m1 = self.m10;
            let sgn0 = m0 >= 0.0;
            let sgn1 = m1 >= 0.0;
            if sgn0 != sgn1 {
                if m0 != -m1 {
                    ret |= TYPE_QUADRANT_ROTATION | TYPE_GENERAL_SCALE;
                } else if m0 != 1.0 && m0 != -1.0 {
                    ret |= TYPE_QUADRANT_ROTATION | TYPE_UNIFORM_SCALE;
                } else {
                    ret |= TYPE_QUADRANT_ROTATION;
                }
            } else if m0 == m1 {
                ret |= TYPE_QUADRANT_ROTATION | TYPE_FLIP | TYPE_UNIFORM_SCALE;
            } else {
                ret |= TYPE_QUADRANT_ROTATION | TYPE_FLIP | TYPE_GENERAL_SCALE;
            }
        } else if s & APPLY_SCALE != 0 {
            if s & APPLY_TRANSLATE != 0 {
                ret = TYPE_TRANSLATION;
            }
            let m0 = self.m00;
            let m1 = self.m11;
            let sgn0 = m0 >= 0.0;
            let sgn1 = m1 >= 0.0;
            if sgn0 == sgn1 {
                if sgn0 {
                    if m0 == m1 {
                        ret |= TYPE_UNIFORM_SCALE;
                    } else {
                        ret |= TYPE_GENERAL_SCALE;
                    }
                } else if m0 != m1 {
                    ret |= TYPE_QUADRANT_ROTATION | TYPE_GENERAL_SCALE;
                } else if m0 != -1.0 {
                    ret |= TYPE_QUADRANT_ROTATION | TYPE_UNIFORM_SCALE;
                } else {
                    ret |= TYPE_QUADRANT_ROTATION;
                }
            } else if m0 == -m1 {
                if m0 == 1.0 || m0 == -1.0 {
                    ret |= TYPE_FLIP;
                } else {
                    ret |= TYPE_FLIP | TYPE_UNIFORM_SCALE;
                }
            } else {
                ret |= TYPE_FLIP | TYPE_GENERAL_SCALE;
            }
        } else if s & APPLY_TRANSLATE != 0 {
            ret = TYPE_TRANSLATION;
        }
        self.kind = ret;
    }

    pub fn get_determinant(&self) -> f64 {
        let s = self.state;
        if s & APPLY_SHEAR != 0 && s & APPLY_SCALE != 0 {
            self.m00 * self.m11 - self.m01 * self.m10
        } else if s & APPLY_SHEAR != 0 {
            -(self.m01 * self.m10)
        } else if s & APPLY_SCALE != 0 {
            self.m00 * self.m11
        } else {
            1.0
        }
    }

    fn update_state(&mut self) {
        let translate = !(self.m02 == 0.0 && self.m12 == 0.0);
        if self.m01 == 0.0 && self.m10 == 0.0 {
            if self.m00 == 1.0 && self.m11 == 1.0 {
                if translate {
                    self.state = APPLY_TRANSLATE;
                    self.kind = TYPE_TRANSLATION;
                } else {
                    self.state = APPLY_IDENTITY;
                    self.kind = TYPE_IDENTITY;
                }
            } else {
                self.state = APPLY_SCALE | if translate { APPLY_TRANSLATE } else { 0 };
                self.kind = TYPE_UNKNOWN;
            }
        } else if self.m00 == 0.0 && self.m11 == 0.0 {
            self.state = APPLY_SHEAR | if translate { APPLY_TRANSLATE } else { 0 };
            self.kind = TYPE_UNKNOWN;
        } else {
            self.state = APPLY_SHEAR | APPLY_SCALE | if translate { APPLY_TRANSLATE } else { 0 };
            self.kind = TYPE_UNKNOWN;
        }
    }

    pub fn is_identity(&mut self) -> bool {
        self.state == APPLY_IDENTITY || self.get_type() == TYPE_IDENTITY
    }

    pub fn translate(&mut self, tx: f64, ty: f64) {
        match self.state {
            7 => {
                self.m02 += tx * self.m00 + ty * self.m01;
                self.m12 += tx * self.m10 + ty * self.m11;
                if self.m02 == 0.0 && self.m12 == 0.0 {
                    self.state = APPLY_SHEAR | APPLY_SCALE;
                    if self.kind != TYPE_UNKNOWN {
                        self.kind -= TYPE_TRANSLATION;
                    }
                }
            }
            6 => {
                self.m02 = tx * self.m00 + ty * self.m01;
                self.m12 = tx * self.m10 + ty * self.m11;
                if self.m02 != 0.0 || self.m12 != 0.0 {
                    self.state = 7;
                    self.kind |= TYPE_TRANSLATION;
                }
            }
            5 => {
                self.m02 += ty * self.m01;
                self.m12 += tx * self.m10;
                if self.m02 == 0.0 && self.m12 == 0.0 {
                    self.state = APPLY_SHEAR;
                    if self.kind != TYPE_UNKNOWN {
                        self.kind -= TYPE_TRANSLATION;
                    }
                }
            }
            4 => {
                self.m02 = ty * self.m01;
                self.m12 = tx * self.m10;
                if self.m02 != 0.0 || self.m12 != 0.0 {
                    self.state = 5;
                    self.kind |= TYPE_TRANSLATION;
                }
            }
            3 => {
                self.m02 += tx * self.m00;
                self.m12 += ty * self.m11;
                if self.m02 == 0.0 && self.m12 == 0.0 {
                    self.state = APPLY_SCALE;
                    if self.kind != TYPE_UNKNOWN {
                        self.kind -= TYPE_TRANSLATION;
                    }
                }
            }
            2 => {
                self.m02 = tx * self.m00;
                self.m12 = ty * self.m11;
                if self.m02 != 0.0 || self.m12 != 0.0 {
                    self.state = 3;
                    self.kind |= TYPE_TRANSLATION;
                }
            }
            1 => {
                self.m02 += tx;
                self.m12 += ty;
                if self.m02 == 0.0 && self.m12 == 0.0 {
                    self.state = APPLY_IDENTITY;
                    self.kind = TYPE_IDENTITY;
                }
            }
            _ => {
                self.m02 = tx;
                self.m12 = ty;
                if tx != 0.0 || ty != 0.0 {
                    self.state = APPLY_TRANSLATE;
                    self.kind = TYPE_TRANSLATION;
                }
            }
        }
    }

    const ROT90_CONVERSION: [u8; 8] = [4, 5, 4, 5, 2, 3, 6, 7];

    fn rotate90(&mut self) {
        let m0 = self.m00;
        self.m00 = self.m01;
        self.m01 = -m0;
        let m0 = self.m10;
        self.m10 = self.m11;
        self.m11 = -m0;
        let mut state = Self::ROT90_CONVERSION[self.state as usize];
        if state & (APPLY_SHEAR | APPLY_SCALE) == APPLY_SCALE && self.m00 == 1.0 && self.m11 == 1.0
        {
            state -= APPLY_SCALE;
        }
        self.state = state;
        self.kind = TYPE_UNKNOWN;
    }

    fn rotate180(&mut self) {
        self.m00 = -self.m00;
        self.m11 = -self.m11;
        let state = self.state;
        if state & APPLY_SHEAR != 0 {
            self.m01 = -self.m01;
            self.m10 = -self.m10;
        } else if self.m00 == 1.0 && self.m11 == 1.0 {
            self.state = state & !APPLY_SCALE;
        } else {
            self.state = state | APPLY_SCALE;
        }
        self.kind = TYPE_UNKNOWN;
    }

    fn rotate270(&mut self) {
        let m0 = self.m00;
        self.m00 = -self.m01;
        self.m01 = m0;
        let m0 = self.m10;
        self.m10 = -self.m11;
        self.m11 = m0;
        let mut state = Self::ROT90_CONVERSION[self.state as usize];
        if state & (APPLY_SHEAR | APPLY_SCALE) == APPLY_SCALE && self.m00 == 1.0 && self.m11 == 1.0
        {
            state -= APPLY_SCALE;
        }
        self.state = state;
        self.kind = TYPE_UNKNOWN;
    }

    pub fn rotate(&mut self, theta: f64) {
        let sin = theta.sin();
        if sin == 1.0 {
            self.rotate90();
        } else if sin == -1.0 {
            self.rotate270();
        } else {
            let cos = theta.cos();
            if cos == -1.0 {
                self.rotate180();
            } else if cos != 1.0 {
                let (m0, m1) = (self.m00, self.m01);
                self.m00 = cos * m0 + sin * m1;
                self.m01 = -sin * m0 + cos * m1;
                let (m0, m1) = (self.m10, self.m11);
                self.m10 = cos * m0 + sin * m1;
                self.m11 = -sin * m0 + cos * m1;
                self.update_state();
            }
        }
    }

    pub fn rotate_around(&mut self, theta: f64, anchor_x: f64, anchor_y: f64) {
        self.translate(anchor_x, anchor_y);
        self.rotate(theta);
        self.translate(-anchor_x, -anchor_y);
    }

    pub fn scale(&mut self, sx: f64, sy: f64) {
        let mut state = self.state;
        match state {
            4..=7 => {
                if state & APPLY_SCALE != 0 {
                    self.m00 *= sx;
                    self.m11 *= sy;
                }
                self.m01 *= sy;
                self.m10 *= sx;
                if self.m01 == 0.0 && self.m10 == 0.0 {
                    state &= APPLY_TRANSLATE;
                    if self.m00 == 1.0 && self.m11 == 1.0 {
                        self.kind = if state == APPLY_IDENTITY {
                            TYPE_IDENTITY
                        } else {
                            TYPE_TRANSLATION
                        };
                    } else {
                        state |= APPLY_SCALE;
                        self.kind = TYPE_UNKNOWN;
                    }
                    self.state = state;
                }
            }
            2 | 3 => {
                self.m00 *= sx;
                self.m11 *= sy;
                if self.m00 == 1.0 && self.m11 == 1.0 {
                    state &= APPLY_TRANSLATE;
                    self.state = state;
                    self.kind = if state == APPLY_IDENTITY {
                        TYPE_IDENTITY
                    } else {
                        TYPE_TRANSLATION
                    };
                } else {
                    self.kind = TYPE_UNKNOWN;
                }
            }
            _ => {
                self.m00 = sx;
                self.m11 = sy;
                if sx != 1.0 || sy != 1.0 {
                    self.state = state | APPLY_SCALE;
                    self.kind = TYPE_UNKNOWN;
                }
            }
        }
    }

    pub fn shear(&mut self, shx: f64, shy: f64) {
        let state = self.state;
        match state {
            6 | 7 => {
                let (m0, m1) = (self.m00, self.m01);
                self.m00 = m0 + m1 * shy;
                self.m01 = m0 * shx + m1;
                let (m0, m1) = (self.m10, self.m11);
                self.m10 = m0 + m1 * shy;
                self.m11 = m0 * shx + m1;
                self.update_state();
            }
            4 | 5 => {
                self.m00 = self.m01 * shy;
                self.m11 = self.m10 * shx;
                if self.m00 != 0.0 || self.m11 != 0.0 {
                    self.state = state | APPLY_SCALE;
                }
                self.kind = TYPE_UNKNOWN;
            }
            2 | 3 => {
                self.m01 = self.m00 * shx;
                self.m10 = self.m11 * shy;
                if self.m01 != 0.0 || self.m10 != 0.0 {
                    self.state = state | APPLY_SHEAR;
                }
                self.kind = TYPE_UNKNOWN;
            }
            _ => {
                self.m01 = shx;
                self.m10 = shy;
                if self.m01 != 0.0 || self.m10 != 0.0 {
                    self.state = state | APPLY_SCALE | APPLY_SHEAR;
                    self.kind = TYPE_UNKNOWN;
                }
            }
        }
    }

    pub fn set_transform(&mut self, m00: f64, m10: f64, m01: f64, m11: f64, m02: f64, m12: f64) {
        *self = Self::new(m00, m10, m01, m11, m02, m12);
    }

    pub fn concatenate(&mut self, tx: &AffineTransform) {
        let mystate = self.state;
        let txstate = tx.state;
        match (txstate << HI_SHIFT) | mystate {
            0..=7 => return,
            x if x == HI_SHEAR | HI_SCALE | HI_TRANSLATE => {
                self.m01 = tx.m01;
                self.m10 = tx.m10;
                self.m00 = tx.m00;
                self.m11 = tx.m11;
                self.m02 = tx.m02;
                self.m12 = tx.m12;
                self.state = txstate;
                self.kind = tx.kind;
                return;
            }
            x if x == HI_SCALE | HI_TRANSLATE => {
                self.m00 = tx.m00;
                self.m11 = tx.m11;
                self.m02 = tx.m02;
                self.m12 = tx.m12;
                self.state = txstate;
                self.kind = tx.kind;
                return;
            }
            x if x == HI_TRANSLATE => {
                self.m02 = tx.m02;
                self.m12 = tx.m12;
                self.state = txstate;
                self.kind = tx.kind;
                return;
            }
            x if x == HI_SHEAR | HI_SCALE => {
                self.m01 = tx.m01;
                self.m10 = tx.m10;
                self.m00 = tx.m00;
                self.m11 = tx.m11;
                self.state = txstate;
                self.kind = tx.kind;
                return;
            }
            x if x == HI_SCALE => {
                self.m00 = tx.m00;
                self.m11 = tx.m11;
                self.state = txstate;
                self.kind = tx.kind;
                return;
            }
            x if x == HI_SHEAR | HI_TRANSLATE => {
                self.m02 = tx.m02;
                self.m12 = tx.m12;
                self.m01 = tx.m01;
                self.m10 = tx.m10;
                self.m00 = 0.0;
                self.m11 = 0.0;
                self.state = txstate;
                self.kind = tx.kind;
                return;
            }
            x if x == HI_SHEAR => {
                self.m01 = tx.m01;
                self.m10 = tx.m10;
                self.m00 = 0.0;
                self.m11 = 0.0;
                self.state = txstate;
                self.kind = tx.kind;
                return;
            }
            x if (x >> HI_SHIFT) == APPLY_TRANSLATE => {
                self.translate(tx.m02, tx.m12);
                return;
            }
            x if (x >> HI_SHIFT) == APPLY_SCALE => {
                self.scale(tx.m00, tx.m11);
                return;
            }
            x if (x >> HI_SHIFT) == APPLY_SHEAR && (mystate == 6 || mystate == 7) => {
                let (t01, t10) = (tx.m01, tx.m10);
                let m0 = self.m00;
                self.m00 = self.m01 * t10;
                self.m01 = m0 * t01;
                let m0 = self.m10;
                self.m10 = self.m11 * t10;
                self.m11 = m0 * t01;
                self.kind = TYPE_UNKNOWN;
                return;
            }
            x if (x >> HI_SHIFT) == APPLY_SHEAR && (mystate == 4 || mystate == 5) => {
                self.m00 = self.m01 * tx.m10;
                self.m01 = 0.0;
                self.m11 = self.m10 * tx.m01;
                self.m10 = 0.0;
                self.state = mystate ^ (APPLY_SHEAR | APPLY_SCALE);
                self.kind = TYPE_UNKNOWN;
                return;
            }
            x if (x >> HI_SHIFT) == APPLY_SHEAR && (mystate == 2 || mystate == 3) => {
                self.m01 = self.m00 * tx.m01;
                self.m00 = 0.0;
                self.m10 = self.m11 * tx.m10;
                self.m11 = 0.0;
                self.state = mystate ^ (APPLY_SHEAR | APPLY_SCALE);
                self.kind = TYPE_UNKNOWN;
                return;
            }
            x if (x >> HI_SHIFT) == APPLY_SHEAR && mystate == 1 => {
                self.m00 = 0.0;
                self.m01 = tx.m01;
                self.m10 = tx.m10;
                self.m11 = 0.0;
                self.state = APPLY_TRANSLATE | APPLY_SHEAR;
                self.kind = TYPE_UNKNOWN;
                return;
            }
            _ => {}
        }
        let (t00, t01, t02) = (tx.m00, tx.m01, tx.m02);
        let (t10, t11, t12) = (tx.m10, tx.m11, tx.m12);
        match mystate {
            6 | 7 => {
                if mystate == 6 {
                    self.state = mystate | txstate;
                }
                let (m0, m1) = (self.m00, self.m01);
                self.m00 = t00 * m0 + t10 * m1;
                self.m01 = t01 * m0 + t11 * m1;
                self.m02 += t02 * m0 + t12 * m1;
                let (m0, m1) = (self.m10, self.m11);
                self.m10 = t00 * m0 + t10 * m1;
                self.m11 = t01 * m0 + t11 * m1;
                self.m12 += t02 * m0 + t12 * m1;
                self.kind = TYPE_UNKNOWN;
                return;
            }
            4 | 5 => {
                let m0 = self.m01;
                self.m00 = t10 * m0;
                self.m01 = t11 * m0;
                self.m02 += t12 * m0;
                let m0 = self.m10;
                self.m10 = t00 * m0;
                self.m11 = t01 * m0;
                self.m12 += t02 * m0;
            }
            2 | 3 => {
                let m0 = self.m00;
                self.m00 = t00 * m0;
                self.m01 = t01 * m0;
                self.m02 += t02 * m0;
                let m0 = self.m11;
                self.m10 = t10 * m0;
                self.m11 = t11 * m0;
                self.m12 += t12 * m0;
            }
            1 => {
                self.m00 = t00;
                self.m01 = t01;
                self.m02 += t02;
                self.m10 = t10;
                self.m11 = t11;
                self.m12 += t12;
                self.state = txstate | APPLY_TRANSLATE;
                self.kind = TYPE_UNKNOWN;
                return;
            }
            _ => {}
        }
        self.update_state();
    }

    pub fn pre_concatenate(&mut self, tx: &AffineTransform) {
        let mut mystate = self.state;
        let txstate = tx.state;
        match txstate {
            0 => return,
            APPLY_TRANSLATE => {
                if mystate & APPLY_TRANSLATE == 0 {
                    self.m02 = tx.m02;
                    self.m12 = tx.m12;
                    self.state = mystate | APPLY_TRANSLATE;
                    self.kind |= TYPE_TRANSLATION;
                } else {
                    self.m02 += tx.m02;
                    self.m12 += tx.m12;
                }
                return;
            }
            APPLY_SCALE => {
                if mystate == APPLY_TRANSLATE || mystate == APPLY_IDENTITY {
                    self.state = mystate | APPLY_SCALE;
                }
                let (t00, t11) = (tx.m00, tx.m11);
                if mystate & APPLY_SHEAR != 0 {
                    self.m01 *= t00;
                    self.m10 *= t11;
                    if mystate & APPLY_SCALE != 0 {
                        self.m00 *= t00;
                        self.m11 *= t11;
                    }
                } else {
                    self.m00 *= t00;
                    self.m11 *= t11;
                }
                if mystate & APPLY_TRANSLATE != 0 {
                    self.m02 *= t00;
                    self.m12 *= t11;
                }
                self.kind = TYPE_UNKNOWN;
                return;
            }
            APPLY_SHEAR => {
                let original = mystate;
                if original == 4 || original == 5 {
                    mystate |= APPLY_SCALE;
                }
                if original != 6 && original != 7 {
                    self.state = mystate ^ APPLY_SHEAR;
                }
                let (t01, t10) = (tx.m01, tx.m10);
                let m0 = self.m00;
                self.m00 = self.m10 * t01;
                self.m10 = m0 * t10;
                let m0 = self.m01;
                self.m01 = self.m11 * t01;
                self.m11 = m0 * t10;
                let m0 = self.m02;
                self.m02 = self.m12 * t01;
                self.m12 = m0 * t10;
                self.kind = TYPE_UNKNOWN;
                return;
            }
            _ => {}
        }
        let (t00, t01, mut t02) = (tx.m00, tx.m01, tx.m02);
        let (t10, t11, mut t12) = (tx.m10, tx.m11, tx.m12);
        match mystate {
            6 | 7 => {
                if mystate == 7 {
                    let (m0, m1) = (self.m02, self.m12);
                    t02 += m0 * t00 + m1 * t01;
                    t12 += m0 * t10 + m1 * t11;
                }
                self.m02 = t02;
                self.m12 = t12;
                let (m0, m1) = (self.m00, self.m10);
                self.m00 = m0 * t00 + m1 * t01;
                self.m10 = m0 * t10 + m1 * t11;
                let (m0, m1) = (self.m01, self.m11);
                self.m01 = m0 * t00 + m1 * t01;
                self.m11 = m0 * t10 + m1 * t11;
            }
            4 | 5 => {
                if mystate == 5 {
                    let (m0, m1) = (self.m02, self.m12);
                    t02 += m0 * t00 + m1 * t01;
                    t12 += m0 * t10 + m1 * t11;
                }
                self.m02 = t02;
                self.m12 = t12;
                let m0 = self.m10;
                self.m00 = m0 * t01;
                self.m10 = m0 * t11;
                let m0 = self.m01;
                self.m01 = m0 * t00;
                self.m11 = m0 * t10;
            }
            2 | 3 => {
                if mystate == 3 {
                    let (m0, m1) = (self.m02, self.m12);
                    t02 += m0 * t00 + m1 * t01;
                    t12 += m0 * t10 + m1 * t11;
                }
                self.m02 = t02;
                self.m12 = t12;
                let m0 = self.m00;
                self.m00 = m0 * t00;
                self.m10 = m0 * t10;
                let m0 = self.m11;
                self.m01 = m0 * t01;
                self.m11 = m0 * t11;
            }
            _ => {
                if mystate == 1 {
                    let (m0, m1) = (self.m02, self.m12);
                    t02 += m0 * t00 + m1 * t01;
                    t12 += m0 * t10 + m1 * t11;
                }
                self.m02 = t02;
                self.m12 = t12;
                self.m00 = t00;
                self.m10 = t10;
                self.m01 = t01;
                self.m11 = t11;
                self.state = mystate | txstate;
                self.kind = TYPE_UNKNOWN;
                return;
            }
        }
        self.update_state();
    }

    /// `invert()`。逆がなければ `NoninvertibleTransformException` の文。
    pub fn invert(&mut self) -> Result<(), String> {
        match self.state {
            7 | 6 => {
                let (m00, m01, m02) = (self.m00, self.m01, self.m02);
                let (m10, m11, m12) = (self.m10, self.m11, self.m12);
                let det = m00 * m11 - m01 * m10;
                if det.abs() <= f64::from_bits(1) {
                    return Err(format!("Determinant is {}", double_to_string(det)));
                }
                self.m00 = m11 / det;
                self.m10 = -m10 / det;
                self.m01 = -m01 / det;
                self.m11 = m00 / det;
                if self.state == 7 {
                    self.m02 = (m01 * m12 - m11 * m02) / det;
                    self.m12 = (m10 * m02 - m00 * m12) / det;
                }
            }
            5 | 4 => {
                let (m01, m02, m10, m12) = (self.m01, self.m02, self.m10, self.m12);
                if m01 == 0.0 || m10 == 0.0 {
                    return Err("Determinant is 0".into());
                }
                self.m10 = 1.0 / m01;
                self.m01 = 1.0 / m10;
                if self.state == 5 {
                    self.m02 = -m12 / m10;
                    self.m12 = -m02 / m01;
                }
            }
            3 | 2 => {
                let (m00, m02, m11, m12) = (self.m00, self.m02, self.m11, self.m12);
                if m00 == 0.0 || m11 == 0.0 {
                    return Err("Determinant is 0".into());
                }
                self.m00 = 1.0 / m00;
                self.m11 = 1.0 / m11;
                if self.state == 3 {
                    self.m02 = -m02 / m00;
                    self.m12 = -m12 / m11;
                }
            }
            1 => {
                self.m02 = -self.m02;
                self.m12 = -self.m12;
            }
            _ => {}
        }
        Ok(())
    }

    /// 点を 1 つ写す（`transform(double[] …)`・`transform(Point2D …)` の式）。
    pub fn transform_point(&self, x: f64, y: f64) -> (f64, f64) {
        match self.state {
            7 => (
                self.m00 * x + self.m01 * y + self.m02,
                self.m10 * x + self.m11 * y + self.m12,
            ),
            6 => (self.m00 * x + self.m01 * y, self.m10 * x + self.m11 * y),
            5 => (self.m01 * y + self.m02, self.m10 * x + self.m12),
            4 => (self.m01 * y, self.m10 * x),
            3 => (self.m00 * x + self.m02, self.m11 * y + self.m12),
            2 => (self.m00 * x, self.m11 * y),
            1 => (x + self.m02, y + self.m12),
            _ => (x, y),
        }
    }

    /// `transform(float[] src, …, float[] dst, …, n)`。
    pub fn transform_floats(&self, pts: &mut [f32], offset: usize, count: usize) {
        for i in 0..count {
            let j = offset + 2 * i;
            if self.state == APPLY_IDENTITY {
                continue;
            }
            let (x, y) = self.transform_point(pts[j] as f64, pts[j + 1] as f64);
            pts[j] = x as f32;
            pts[j + 1] = y as f32;
        }
    }

    /// `deltaTransform` の 1 点。
    pub fn delta_transform_point(&self, x: f64, y: f64) -> (f64, f64) {
        match self.state {
            6 | 7 => (x * self.m00 + y * self.m01, x * self.m10 + y * self.m11),
            4 | 5 => (y * self.m01, x * self.m10),
            2 | 3 => (x * self.m00, y * self.m11),
            _ => (x, y),
        }
    }
}

// ---- Path2D の外接矩形 ----

#[derive(Clone, Copy, Debug)]
enum Segment {
    Move(f64, f64),
    Line(f64, f64),
    Quad([f64; 4]),
    Cubic([f64; 6]),
    Close,
}

/// `Path2D.Double` のうち、外接矩形に要るところ。
#[derive(Default, Debug)]
pub struct Path2D {
    segments: Vec<Segment>,
}

impl Path2D {
    pub fn move_to(&mut self, x: f64, y: f64) {
        if let Some(Segment::Move(px, py)) = self.segments.last_mut() {
            *px = x;
            *py = y;
        } else {
            self.segments.push(Segment::Move(x, y));
        }
    }

    pub fn line_to(&mut self, x: f64, y: f64) {
        self.segments.push(Segment::Line(x, y));
    }

    pub fn quad_to(&mut self, x1: f64, y1: f64, x2: f64, y2: f64) {
        self.segments.push(Segment::Quad([x1, y1, x2, y2]));
    }

    pub fn curve_to(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, x3: f64, y3: f64) {
        self.segments.push(Segment::Cubic([x1, y1, x2, y2, x3, y3]));
    }

    pub fn close_path(&mut self) {
        if !matches!(self.segments.last(), None | Some(Segment::Close)) {
            self.segments.push(Segment::Close);
        }
    }

    /// `getBounds2D()`（JDK 19 以降: 曲線の極値まで見る）。(x, y, width, height)。
    pub fn bounds(&self) -> (f64, f64, f64, f64) {
        let mut bounds: Option<[f64; 4]> = None;
        let (mut last_x, mut last_y) = (0.0, 0.0);
        let (mut start_x, mut start_y) = (0.0, 0.0);
        for seg in &self.segments {
            let (end_x, end_y) = match *seg {
                Segment::Move(x, y) => {
                    if bounds.is_none() {
                        bounds = Some([x, x, y, y]);
                    }
                    start_x = x;
                    start_y = y;
                    (x, y)
                }
                Segment::Line(x, y) => (x, y),
                Segment::Quad(c) => (c[2], c[3]),
                Segment::Cubic(c) => (c[4], c[5]),
                Segment::Close => (start_x, start_y),
            };
            // Path2D は moveTo から始まるので bounds はある
            let Some(b) = bounds.as_mut() else {
                continue;
            };
            if end_x < b[0] {
                b[0] = end_x;
            }
            if end_x > b[1] {
                b[1] = end_x;
            }
            if end_y < b[2] {
                b[2] = end_y;
            }
            if end_y > b[3] {
                b[3] = end_y;
            }
            match *seg {
                Segment::Quad(c) => {
                    quad_extrema(b, 0, last_x, c[0], c[2]);
                    quad_extrema(b, 2, last_y, c[1], c[3]);
                }
                Segment::Cubic(c) => {
                    cubic_extrema(b, 0, last_x, c[0], c[2], c[4]);
                    cubic_extrema(b, 2, last_y, c[1], c[3], c[5]);
                }
                _ => {}
            }
            last_x = end_x;
            last_y = end_y;
        }
        match bounds {
            Some(b) => (b[0], b[2], b[1] - b[0], b[3] - b[2]),
            None => (0.0, 0.0, 0.0, 0.0),
        }
    }
}

/// `Math.ulp(double)`。
fn ulp(x: f64) -> f64 {
    let x = x.abs();
    if !x.is_finite() {
        return x;
    }
    if x == f64::MAX {
        return 2f64.powi(971);
    }
    f64::from_bits(x.to_bits() + 1) - x
}

/// `Curve.accumulateExtremaBoundsForQuad`。
fn quad_extrema(bounds: &mut [f64; 4], off: usize, x1: f64, ctrl: f64, x2: f64) {
    if ctrl < bounds[off] || ctrl > bounds[off + 1] {
        let dx21 = ctrl - x1;
        let c2 = (x2 - ctrl) - dx21;
        let c1 = 2.0 * dx21;
        let c0 = x1;
        let d0 = c1;
        let d1 = 2.0 * c2;
        let t = -d0 / d1;
        if t > 0.0 && t < 1.0 {
            let v = c0 + t * (c1 + t * c2);
            let margin = ulp(c0.abs() + c1.abs() + c2.abs());
            if v - margin < bounds[off] {
                bounds[off] = v - margin;
            }
            if v + margin > bounds[off + 1] {
                bounds[off + 1] = v + margin;
            }
        }
    }
}

/// `Curve.accumulateExtremaBoundsForCubic`。
fn cubic_extrema(bounds: &mut [f64; 4], off: usize, x1: f64, c1x: f64, c2x: f64, x2: f64) {
    if c1x < bounds[off] || c1x > bounds[off + 1] || c2x < bounds[off] || c2x > bounds[off + 1] {
        let dx32 = 3.0 * (c2x - c1x);
        let dx21 = 3.0 * (c1x - x1);
        let c3 = (x2 - x1) - dx32;
        let c2 = dx32 - dx21;
        let c1 = dx21;
        let c0 = x1;
        let eqn = [c1, 2.0 * c2, 3.0 * c3];
        let mut res = [0.0; 2];
        let count = solve_quadratic(eqn, &mut res);
        if count > 0 {
            let margin = ulp(c0.abs() + c1.abs() + c2.abs() + c3.abs());
            for &t in &res[..count as usize] {
                if t > 0.0 && t < 1.0 {
                    let v = c0 + t * (c1 + t * (c2 + t * c3));
                    if v - margin < bounds[off] {
                        bounds[off] = v - margin;
                    }
                    if v + margin > bounds[off + 1] {
                        bounds[off + 1] = v + margin;
                    }
                }
            }
        }
    }
}

/// `QuadCurve2D.solveQuadratic`。
fn solve_quadratic(eqn: [f64; 3], res: &mut [f64; 2]) -> i32 {
    let (c, b, a) = (eqn[0], eqn[1], eqn[2]);
    let mut roots = 0;
    if a == 0.0 {
        if b == 0.0 {
            return -1;
        }
        res[roots] = -c / b;
        roots += 1;
    } else {
        let mut d = b * b - 4.0 * a * c;
        if d < 0.0 {
            return 0;
        }
        d = d.sqrt();
        if b < 0.0 {
            d = -d;
        }
        let q = (b + d) / -2.0;
        res[roots] = q / a;
        roots += 1;
        if q != 0.0 {
            res[roots] = c / q;
            roots += 1;
        }
    }
    roots as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_like_java() {
        assert_eq!(format_decimal(0.5, 2), "0.5");
        assert_eq!(format_decimal(-0.001, 2), "-0");
        assert_eq!(format_decimal(-0.0, 2), "-0");
        assert_eq!(format_decimal(1.005, 2), "1");
        assert_eq!(format_decimal(0.125, 2), "0.13");
        assert_eq!(format_decimal(0.135, 2), "0.14");
        assert_eq!(format_decimal(2.5, 0), "3");
        assert_eq!(format_decimal(-2.5, 0), "-3");
        assert_eq!(format_decimal(1e20, 2), "100000000000000000000");
        assert_eq!(float_to_string(1e-4), "1.0E-4");
        assert_eq!(float_to_string(1e7), "1.0E7");
        assert_eq!(float_to_string(123456.7), "123456.7");
        assert_eq!(float_to_string(0.001), "0.001");
        assert_eq!(float_to_string(100.0), "100.0");
        assert_eq!(float_to_string(-0.0), "-0.0");
        assert_eq!(trim_insignificant_zeros("1.0E-4"), "1E-4");
        assert_eq!(trim_insignificant_zeros("1.50"), "1.5");
        assert_eq!(trim_insignificant_zeros("1.0"), "1");
        assert_eq!(trim_insignificant_zeros("-0"), "-0");
        assert_eq!(
            parse_float(" 1in ").unwrap_err().to_string(),
            "java.lang.NumberFormatException: For input string: \"1in\""
        );
        assert_eq!(
            parse_float("  ").unwrap_err().to_string(),
            "java.lang.NumberFormatException: empty String"
        );
        assert_eq!(parse_float("1.5f").unwrap(), 1.5);
        assert_eq!(parse_float("+.5e1").unwrap(), 5.0);
        assert_eq!(parse_float("1.").unwrap(), 1.0);
        assert_eq!(parse_float("0x1p3").unwrap(), 8.0);
        assert_eq!(split_char("a;b;;", ';'), ["a", "b"]);
        assert_eq!(split_char("", ';'), [""]);
        assert!(split_char(";", ';').is_empty());
        assert_eq!(
            split(" 1, 2", true, |c| c == ',' || is_regex_space(c)),
            ["", "1", "2"]
        );
    }
}
