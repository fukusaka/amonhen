//! 描けない図の知らせ（pele の `src/errors.ts`）。位置は JS と同じく UTF-16 の位置で数える。

use std::cell::RefCell;
use std::fmt;

/// 知らせの種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCode {
    UnsupportedDiagram,
    Syntax,
    Semantic,
    Limit,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::UnsupportedDiagram => "unsupported-diagram",
            ErrorCode::Syntax => "syntax",
            ErrorCode::Semantic => "semantic",
            ErrorCode::Limit => "limit",
        }
    }
}

/// `PeleError`。
#[derive(Clone, Debug, PartialEq)]
pub struct PeleError {
    pub message: String,
    pub code: ErrorCode,
    pub type_: Option<String>,
    pub mermaid_type: Option<String>,
    pub line: usize,
    pub column: usize,
    pub snippet: String,
}

impl PeleError {
    pub fn new(message: impl Into<String>, code: ErrorCode) -> Self {
        PeleError {
            message: message.into(),
            code,
            type_: None,
            mermaid_type: None,
            line: 0,
            column: 0,
            snippet: String::new(),
        }
    }

    pub fn typed(message: impl Into<String>, code: ErrorCode, type_: &str) -> Self {
        let mut e = Self::new(message, code);
        e.type_ = Some(type_.to_string());
        e
    }
}

/// JS の文字列の長さの上限（V8 の 64 ビット版の `String::kMaxLength`）。pele はこれより長い文字列を作ろうとすると
/// RangeError になり、スタックの溢れと同じく [`PeleError::too_deep`] にする。
pub const JS_MAX_STRING_LENGTH: usize = (1 << 29) - 24;

impl PeleError {
    /// pele が JS の RangeError（スタックの溢れ、長すぎる文字列）を言い換えた知らせ。
    pub fn too_deep() -> Self {
        Self::new("Diagram is nested too deeply.", ErrorCode::Limit)
    }

    /// 出力が上限より長い。
    pub fn output_too_long(limit: usize) -> Self {
        Self::new(
            format!("Diagram output is longer than the limit of {limit} characters."),
            ErrorCode::Limit,
        )
    }

    /// 出力が少なくとも `at_least` 文字になると分かったときの知らせ。pele は描いてから上限と比べ、JS の文字列の上限を
    /// 超えるならその前に RangeError になる。
    pub fn output_at_least(at_least: usize, limit: usize) -> Option<Self> {
        if at_least > JS_MAX_STRING_LENGTH {
            Some(Self::too_deep())
        } else if at_least > limit {
            Some(Self::output_too_long(limit))
        } else {
            None
        }
    }
}

impl fmt::Display for PeleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PeleError {}

pub type Result<T> = std::result::Result<T, PeleError>;

/// 入力の `[from, to)` を出力の `length` 文字に置き換えた、という記録（UTF-16 の位置）。
pub type Edit = (usize, usize, usize);

fn step_input(edits: &[Edit], at: usize) -> usize {
    let mut shift: isize = 0;
    let at = at as isize;
    for &(from, to, length) in edits {
        let (from, to, length) = (from as isize, to as isize, length as isize);
        let start = from - shift;
        if at < start {
            break;
        }
        if at < start + length {
            return (from + (at - start).min(to - from - 1)) as usize;
        }
        shift += to - from - length;
    }
    (at + shift) as usize
}

pub fn input_offset(steps: &[Vec<Edit>], mut at: usize) -> usize {
    for s in steps.iter().rev() {
        at = step_input(s, at);
    }
    at
}

/// 読んでいる文字列から元の文字列の位置への写し。
pub struct Origin {
    pub parsed: Vec<u16>,
    pub find: Box<dyn Fn(usize) -> (Vec<u16>, usize)>,
}

thread_local! {
    static ORIGIN: RefCell<Option<std::rc::Rc<Origin>>> = const { RefCell::new(None) };
}

/// `run` が読む間、位置を元の文字列で知らせる。
pub fn locating_in<T>(next: Origin, run: impl FnOnce() -> T) -> T {
    let outer = ORIGIN.with(|o| o.replace(Some(std::rc::Rc::new(next))));
    let out = run();
    ORIGIN.with(|o| *o.borrow_mut() = outer);
    out
}

/// `src` がちょうど読んでいる文字列のときだけ写す。
pub fn written(src: &[u16], offset: usize) -> (Vec<u16>, usize) {
    let origin = ORIGIN.with(|o| o.borrow().clone());
    match origin {
        Some(o) if o.parsed == src => (o.find)(offset),
        _ => (src.to_vec(), offset),
    }
}

fn index_of(s: &[u16], c: u16, from: usize) -> Option<usize> {
    if from >= s.len() {
        return None;
    }
    s[from..].iter().position(|&x| x == c).map(|i| i + from)
}

fn is_ws16(c: u16) -> bool {
    char::from_u32(c as u32).is_some_and(crate::js::is_js_space)
}

pub struct Located {
    pub line: usize,
    pub column: usize,
    pub snippet: String,
}

pub fn locate(src: &[u16], offset: usize) -> Located {
    // 終わりでの知らせは最後の中身の行に。
    let mut end = src.len();
    while end > 0 && is_ws16(src[end - 1]) {
        end -= 1;
    }
    let (src, offset) = if end > 0 && offset >= end {
        let (s, o) = written(src, end - 1);
        (s, o + 1)
    } else {
        written(src, offset)
    };
    let mut line = 1;
    let mut line_start = 0;
    let mut i = index_of(&src, b'\n' as u16, 0);
    while let Some(k) = i {
        if k >= offset {
            break;
        }
        line += 1;
        line_start = k + 1;
        i = index_of(&src, b'\n' as u16, k + 1);
    }
    let line_end = index_of(&src, b'\n' as u16, offset).unwrap_or(src.len());
    let column = offset - line_start + 1;
    let text =
        crate::js::from16(&src[line_start.min(src.len())..line_end.max(line_start).min(src.len())]);
    Located {
        line,
        column,
        snippet: format!("{}\n{}^", text, "-".repeat(column - 1)),
    }
}

pub fn syntax_error(
    type_: &str,
    src: &[u16],
    offset: usize,
    detail: &str,
    lexical: bool,
) -> PeleError {
    let loc = locate(src, offset);
    let message = if lexical {
        format!(
            "Lexical error on line {}. Unrecognized text.\n{}",
            loc.line, loc.snippet
        )
    } else {
        format!(
            "Parse error on line {}:\n{}\n{}",
            loc.line, loc.snippet, detail
        )
    };
    PeleError {
        message,
        code: ErrorCode::Syntax,
        type_: Some(type_.to_string()),
        mermaid_type: None,
        line: loc.line,
        column: loc.column,
        snippet: loc.snippet,
    }
}
