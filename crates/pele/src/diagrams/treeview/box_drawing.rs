//! Mermaid の罫線の前処理（pele の `src/diagrams/treeview/boxDrawing.ts`）: `├──`・`└──`・`│`（と太い形）を、文法の
//! 読む字下げにする。位置はどれも UTF-16 の符号単位。

use std::collections::HashMap;

use crate::errors::{ErrorCode, PeleError, Result, locate};
use crate::js;
use crate::util::chars::is_space;

const INDENT_UNIT: &str = "    ";

/// 出力の行（1 から）から元の行（1 から）への写し。行ごとの、書き直した位置から元の位置への写し。
pub type Columns = HashMap<usize, Box<dyn Fn(usize) -> usize>>;

pub struct PreprocessResult {
    pub text: String,
    /// 出力の行の番号から元の行の番号（どちらも 1 から）。文字が変わらなければ空。
    pub line_map: HashMap<usize, usize>,
    pub columns: Columns,
}

fn is_box(c: u16) -> bool {
    matches!(
        c,
        0x2500 | 0x2501 | 0x2502 | 0x2503 | 0x2514 | 0x2517 | 0x251c | 0x2523
    )
}

fn is_branch(c: u16) -> bool {
    matches!(c, 0x2514 | 0x2517 | 0x251c | 0x2523)
}

fn is_dash(c: u16) -> bool {
    matches!(c, 0x2500 | 0x2501)
}

fn is_vertical(c: u16) -> bool {
    matches!(c, 0x2502 | 0x2503)
}

fn any_box(s: &[u16]) -> bool {
    s.iter().any(|&c| is_box(c))
}

fn sp(c: u16) -> bool {
    is_space(c as u32)
}

/// `^[\s│┃]+$`
fn decoration_only(s: &[u16]) -> bool {
    !s.is_empty() && s.iter().all(|&c| sp(c) || is_vertical(c))
}

/// `^[\s─━│┃└┗├┣]+$`
fn box_only(s: &[u16]) -> bool {
    !s.is_empty() && s.iter().all(|&c| sp(c) || is_box(c))
}

fn has(s: &[u16], p: usize, word: &str) -> bool {
    s.get(p..)
        .is_some_and(|s| s.len() >= word.len() && word.bytes().zip(s).all(|(w, &c)| w as u16 == c))
}

fn blanks(s: &[u16], mut p: usize) -> usize {
    while matches!(s.get(p), Some(9 | 32)) {
        p += 1;
    }
    p
}

fn leading_space(s: &[u16]) -> usize {
    s.iter().take_while(|&&c| sp(c)).count()
}

/// `^\s*(title[\t ]|accTitle[\t ]*:|accDescr[\t ]*[:{])`
fn metadata_line(s: &[u16]) -> bool {
    let p = leading_space(s);
    if has(s, p, "title") && matches!(s.get(p + 5), Some(9 | 32)) {
        return true;
    }
    if has(s, p, "accTitle") && s.get(blanks(s, p + 8)) == Some(&58) {
        return true;
    }
    has(s, p, "accDescr") && matches!(s.get(blanks(s, p + 8)), Some(58 | 123))
}

/// `^\s*%%`
fn comment_line(s: &[u16]) -> bool {
    has(s, leading_space(s), "%%")
}

fn expand_tabs(s: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(s.len());
    for &c in s {
        if c == 9 {
            out.extend_from_slice(&[32; 4]);
        } else {
            out.push(c);
        }
    }
    out
}

fn infer_segment_width(content_lines: &[Vec<u16>]) -> usize {
    for line in content_lines {
        if let Some(index) = line.iter().position(|&c| is_branch(c))
            && index > 0
        {
            return index;
        }
    }
    4
}

/// `message.replace(/\bline\s+(\d+)\b/gi, …)`: 写しにある行の番号を元の番号に。
pub fn remap_error_lines(message: &str, line_map: &HashMap<usize, usize>) -> String {
    let s: Vec<char> = message.chars().collect();
    let word = |i: usize| {
        s.get(i)
            .is_some_and(|&c| c.is_ascii_alphanumeric() || c == '_')
    };
    let mut out = String::with_capacity(message.len());
    let mut i = 0;
    while i < s.len() {
        let starts = (i == 0 || !word(i - 1))
            && i + 4 <= s.len()
            && s[i..i + 4]
                .iter()
                .collect::<String>()
                .eq_ignore_ascii_case("line");
        if starts {
            let mut j = i + 4;
            while j < s.len() && js::is_js_space(s[j]) {
                j += 1;
            }
            let d = j;
            while j < s.len() && s[j].is_ascii_digit() {
                j += 1;
            }
            if j > d && d > i + 4 && !word(j) {
                let digits: String = s[d..j].iter().collect();
                if let Some(&original) = digits.parse::<usize>().ok().and_then(|n| line_map.get(&n))
                    && original != 0
                {
                    out.push_str(&format!("line {original}"));
                } else {
                    out.extend(&s[i..j]);
                }
                i = j;
                continue;
            }
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

/// `line` 行目（1 から）の頭の位置。
pub fn line_start(text: &[u16], line: usize) -> usize {
    let mut at = 0;
    for _ in 1..line {
        at = text[at.min(text.len())..]
            .iter()
            .position(|&c| c == 10)
            .map_or(0, |k| at + k + 1);
    }
    at
}

fn fail(input: &[u16], index: usize, message: &str) -> PeleError {
    let line = locate(input, line_start(input, index + 1)).line;
    let mut e = PeleError::typed(
        format!("Line {line}: {message}"),
        ErrorCode::Syntax,
        "treeView",
    );
    e.line = line;
    e
}

/// 4 つの空白にしたタブを戻す。
fn before_tabs(line: &[u16], at: usize) -> usize {
    let mut i = 0;
    let mut n = 0;
    while i < line.len() && n < at {
        n += if line[i] == 9 { 4 } else { 1 };
        i += 1;
    }
    i
}

pub fn preprocess_box_drawing(input: &str) -> Result<PreprocessResult> {
    let mut line_map = HashMap::new();
    let mut columns: Columns = HashMap::new();
    let unchanged = |line_map, columns| PreprocessResult {
        text: input.to_string(),
        line_map,
        columns,
    };
    let src = js::utf16(input);
    if !any_box(&src) {
        return Ok(unchanged(line_map, columns));
    }
    let lines: Vec<&[u16]> = src.split(|&c| c == 10).collect();

    let Some(keyword_idx) = lines
        .iter()
        .position(|l| js::trim(&js::from16(l)) == "treeView-beta")
    else {
        return Ok(unchanged(line_map, columns));
    };

    let passes = |line: &[u16]| {
        js::trim(&js::from16(line)).is_empty() || comment_line(line) || metadata_line(line)
    };

    let mut content_lines: Vec<Vec<u16>> = Vec::new();
    for line in &lines[keyword_idx + 1..] {
        if passes(line) || decoration_only(line) {
            continue;
        }
        content_lines.push(expand_tabs(line));
    }
    if !content_lines.iter().any(|l| any_box(l)) {
        return Ok(unchanged(line_map, columns));
    }

    let segment_width = infer_segment_width(&content_lines);
    let mut out: Vec<Vec<u16>> = Vec::new();
    let mut keep = |text: Vec<u16>, index: usize, column: Option<Box<dyn Fn(usize) -> usize>>| {
        out.push(text);
        line_map.insert(out.len(), index + 1);
        if let Some(c) = column {
            columns.insert(out.len(), c);
        }
    };

    for (i, line) in lines.iter().enumerate().take(keyword_idx + 1) {
        keep(line.to_vec(), i, None);
    }
    for (i, &line) in lines.iter().enumerate().skip(keyword_idx + 1) {
        if passes(line) {
            keep(line.to_vec(), i, None);
            continue;
        }
        if decoration_only(line) {
            continue;
        }

        let normalized = expand_tabs(line);
        if let Some(index) = normalized.iter().position(|&c| is_branch(c)) {
            let depth = js::round(index as f64 / segment_width as f64) as usize + 1;
            let mut pos = index + 1;
            while pos < normalized.len() && is_dash(normalized[pos]) {
                pos += 1;
            }
            while pos < normalized.len() && normalized[pos] == 32 {
                pos += 1;
            }
            let content = js::utf16(js::trim_end(&js::from16(&normalized[pos..])));
            if content.is_empty() {
                return Err(fail(
                    &src,
                    i,
                    "Empty node — expected a filename or directory name after the box-drawing prefix",
                ));
            }
            let indent = INDENT_UNIT.repeat(depth);
            let indent_len = indent.len();
            let mut text = js::utf16(&indent);
            text.extend_from_slice(&content);
            let original = line.to_vec();
            keep(
                text,
                i,
                Some(Box::new(move |at| {
                    before_tabs(&original, pos + at.saturating_sub(indent_len))
                })),
            );
        } else if box_only(&normalized) {
            continue;
        } else if any_box(&normalized) {
            // "Section ─ A.txt" のように罫線の文字を含む名前は、根の項目。
            keep(line.to_vec(), i, None);
        } else if normalized.first().is_some_and(|&c| sp(c)) {
            return Err(fail(
                &src,
                i,
                "Unexpected indentation without box-drawing characters. In box-drawing format, use ├── or └── prefixes for indented nodes.",
            ));
        } else {
            keep(line.to_vec(), i, None);
        }
    }
    let joined = out.join(&10u16);
    Ok(PreprocessResult {
        text: js::from16(&joined),
        line_map,
        columns,
    })
}
