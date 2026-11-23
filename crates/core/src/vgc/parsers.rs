//! 数・単位・変換の読み方（vgc の `svg/numbers.dart`・`svg/parsers.dart`）。

use std::sync::OnceLock;

use regex::Regex;

use super::dart::{self, Error, R};
use super::geometry::{AffineMatrix, FillType};

/// テーマの文字の大きさと x の高さ（vgc の `--font-size` の既定 14、x-height はその半分）。
const THEME_FONT_SIZE: f64 = 14.0;
const THEME_X_HEIGHT: f64 = 7.0;

/// `parseDouble`（単位の文字を 1 つずつ外してから読む）。
pub fn parse_double(raw: Option<&str>, try_parse: bool) -> R<Option<f64>> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let mut s = raw.to_owned();
    for unit in ["rem", "em", "ex", "px", "pt", "%"] {
        s = dart::remove_first(&s, unit);
    }
    let s = dart::trim(&s);
    if try_parse {
        return Ok(dart::try_parse_double(s));
    }
    dart::parse_double(s).map(Some)
}

/// `parseDoubleWithUnits`。
pub fn parse_double_with_units(
    raw: Option<&str>,
    try_parse: bool,
    percentage_ref: Option<f64>,
) -> R<Option<f64>> {
    let Some(raw_s) = raw else {
        return Ok(None);
    };
    let mut unit = 1.0;
    if dart::trim(raw_s).ends_with('%') {
        let Some(reference) = percentage_ref.filter(|r| !r.is_infinite()) else {
            if try_parse {
                return Ok(None);
            }
            return Err(Error::format_only(&format!(
                "Percentage value \"{raw_s}\" requires a reference dimension \
                 (viewport width/height) but none was available."
            )));
        };
        let value = parse_double(raw, try_parse)?;
        return Ok(value.map(|v| (v / 100.0) * reference));
    }
    if raw_s.contains("pt") {
        unit = 96.0 / 72.0;
    } else if raw_s.contains("rem") || raw_s.contains("em") {
        unit = THEME_FONT_SIZE;
    } else if raw_s.contains("ex") {
        unit = THEME_X_HEIGHT;
    }
    let value = parse_double(raw, try_parse)?;
    Ok(value.map(|v| v * unit))
}

fn req(raw: &str) -> R<f64> {
    parse_double(Some(raw), false)?.ok_or_else(Error::null_check)
}

pub fn is_percentage(val: &str) -> bool {
    val.ends_with('%')
}

/// `parsePercentage`。
pub fn parse_percentage(val: &str) -> R<f64> {
    let head = &val[..val.len() - 1];
    Ok(req(head)? / 100.0 * 1.0)
}

/// `parseDecimalOrPercentage`。
pub fn parse_decimal_or_percentage(val: &str) -> R<f64> {
    if is_percentage(val) {
        parse_percentage(val)
    } else {
        req(val)
    }
}

/// 末尾の 2 文字を除いた部分（UTF-16 の長さが 2 未満なら Dart は失敗する）。
fn without_last_two(s: &str) -> Option<&str> {
    let mut idx = s.char_indices().rev();
    idx.next()?;
    let (i, _) = idx.next()?;
    Some(&s[..i])
}

/// `isEm`。
pub fn is_em(val: &str) -> bool {
    let trimmed = dart::trim(val);
    trimmed.to_lowercase().ends_with("em")
        && without_last_two(trimmed).is_some_and(|h| dart::try_parse_double(h).is_some())
}

/// `parseEm`。
pub fn parse_em(val: &str) -> R<f64> {
    let trimmed = dart::trim(val);
    dart::parse_double(without_last_two(trimmed).unwrap_or(""))
}

/// `parsePatternUnitToDouble`。
pub fn parse_pattern_unit(raw: &str, view_box_value: Option<f64>) -> R<Option<f64>> {
    if raw.contains('%') {
        let head: String = {
            let mut chars = raw.chars();
            chars.next_back();
            chars.collect()
        };
        let v = dart::parse_double(&head)?;
        return Ok(Some(
            (v / 100.0) * view_box_value.ok_or_else(Error::null_check)?,
        ));
    }
    if raw.starts_with("0.") {
        let v = dart::parse_double(raw)?;
        return Ok(Some(v * view_box_value.ok_or_else(Error::null_check)?));
    }
    if !raw.is_empty() {
        return Ok(Some(dart::parse_double(raw)?));
    }
    Ok(None)
}

/// `parseRawFillRule`。
pub fn parse_fill_rule(raw: Option<&str>) -> Option<FillType> {
    match raw {
        None | Some("inherit") => None,
        Some("evenodd") => Some(FillType::EvenOdd),
        Some(_) => Some(FillType::NonZero),
    }
}

/// `split(RegExp(r'[\s,]+'))`。
pub fn split_list(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_sep = false;
    for c in s.chars() {
        if c == ',' || dart::is_js_space(c) {
            if !in_sep {
                out.push(std::mem::take(&mut current));
                in_sep = true;
            }
        } else {
            in_sep = false;
            current.push(c);
        }
    }
    out.push(current);
    out
}

fn transform_regexes() -> &'static (Regex, Regex) {
    static RE: OnceLock<(Regex, Regex)> = OnceLock::new();
    RE.get_or_init(|| {
        let atom = r" *,?([^(]+)\(([^)]*)\)";
        (
            Regex::new(&format!("^(?:{atom})*$")).expect("transform validator"),
            Regex::new(atom).expect("transform command"),
        )
    })
}

/// `_parseTransformParams`。
fn parse_transform_params(params: &str) -> R<Vec<f64>> {
    let chars: Vec<char> = params.chars().collect();
    let mut result = Vec::new();
    let mut current = String::new();
    for (i, &c) in chars.iter().enumerate() {
        let is_separator = c == ' ' || c == '-' || c == ',';
        let is_exponent = i > 0 && chars[i - 1].to_lowercase().eq(['e']);
        if is_separator && !is_exponent {
            if !current.is_empty() {
                result.push(req(&current)?);
            }
            current = if c == '-' { "-".into() } else { String::new() };
        } else {
            if c == '.' && current.contains('.') {
                result.push(req(&current)?);
                current.clear();
            }
            current.push(c);
        }
    }
    if !current.is_empty() {
        result.push(req(&current)?);
    }
    Ok(result)
}

fn at(params: &[f64], i: usize) -> R<f64> {
    params
        .get(i)
        .copied()
        .ok_or_else(|| Error::index(i as i64, params.len() as i64))
}

/// `parseTransform`。
pub fn parse_transform(transform: Option<&str>) -> R<Option<AffineMatrix>> {
    let Some(transform) = transform.filter(|t| !t.is_empty()) else {
        return Ok(None);
    };
    let (validator, command) = transform_regexes();
    if !validator.is_match(transform) {
        return Err(Error::state(format!(
            "illegal or unsupported transform: {transform}"
        )));
    }
    let matches: Vec<_> = command.captures_iter(transform).collect();
    let mut result = AffineMatrix::IDENTITY;
    for m in matches.iter().rev() {
        let name = dart::trim(m.get(1).map_or("", |g| g.as_str()));
        let params = parse_transform_params(dart::trim(m.get(2).map_or("", |g| g.as_str())))?;
        result = match name {
            "matrix" => AffineMatrix::new(
                at(&params, 0)?,
                at(&params, 1)?,
                at(&params, 2)?,
                at(&params, 3)?,
                at(&params, 4)?,
                at(&params, 5)?,
                1.0,
            )
            .multiplied(&result),
            "translate" => {
                let x = *params.first().ok_or_else(Error::no_element)?;
                let y = if params.len() < 2 { 0.0 } else { params[1] };
                AffineMatrix::new6(1.0, 0.0, 0.0, 1.0, x, y).multiplied(&result)
            }
            "scale" => {
                let x = at(&params, 0)?;
                let y = if params.len() < 2 { x } else { params[1] };
                AffineMatrix::new6(x, 0.0, 0.0, y, 0.0, 0.0).multiplied(&result)
            }
            "rotate" => {
                let a = at(&params, 0)? * std::f64::consts::PI / 180.0;
                let rotate = AffineMatrix::IDENTITY.rotated(a);
                if params.len() > 1 {
                    let x = params[1];
                    let y = if params.len() == 3 { params[2] } else { x };
                    AffineMatrix::new6(1.0, 0.0, 0.0, 1.0, x, y)
                        .multiplied(&rotate)
                        .translated(-x, -y)
                        .multiplied(&result)
                } else {
                    rotate.multiplied(&result)
                }
            }
            "skewX" => {
                let t = params.first().ok_or_else(Error::no_element)?.tan();
                AffineMatrix::new6(1.0, 0.0, t, 1.0, 0.0, 0.0).multiplied(&result)
            }
            "skewY" => {
                let t = params.first().ok_or_else(Error::no_element)?.tan();
                AffineMatrix::new6(1.0, t, 0.0, 1.0, 0.0, 0.0).multiplied(&result)
            }
            _ => return Err(Error::state(format!("Unsupported transform: {name}"))),
        };
    }
    Ok(Some(result))
}

/// `base64.decode`（標準と URL の文字、`=` の埋め。長さは 4 の倍数でなければならない）。
pub fn base64_decode(s: &str) -> R<Vec<u8>> {
    let bytes = s.as_bytes();
    let value = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        })
    };
    let mut data = Vec::with_capacity(bytes.len());
    let mut padding = 0;
    for (i, &c) in bytes.iter().enumerate() {
        if c == b'=' {
            padding += 1;
            continue;
        }
        if padding > 0 {
            return Err(Error::format_at("Invalid character", s, i));
        }
        match value(c) {
            Some(v) => data.push(v),
            None => return Err(Error::format_at("Invalid character", s, i)),
        }
    }
    let total = data.len() + padding;
    if !total.is_multiple_of(4) || padding > 2 || (padding > 0 && data.len() % 4 + padding != 4) {
        return Err(Error::format_at(
            "Invalid length, must be multiple of four",
            s,
            s.len(),
        ));
    }
    let mut out = Vec::with_capacity(data.len() * 3 / 4);
    for chunk in data.chunks(4) {
        let mut n = 0u32;
        for (k, v) in chunk.iter().enumerate() {
            n |= v << (18 - 6 * k);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Ok(out)
}
