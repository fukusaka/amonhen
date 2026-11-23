//! use case の字句解析（pele の `src/diagrams/usecase/lexer.ts`）。Mermaid の Chevrotain の字句解析と同じ字句を
//! 作る: 並びの最初に合う型が勝ち、キーワードはより長い名前に譲り、`json id@` と `<<` は JSON のオブジェクトと
//! ステレオタイプを読む状態に移る。JS と同じく UTF-16 の符号単位で読む。

use super::tokens as t;
use crate::errors::{ErrorCode, PeleError, Result, written};
use crate::js;
use crate::util::chars::{NONE, is_blank, is_digit, is_hex, is_letter};

pub struct Tokens {
    pub types: Vec<u8>,
    pub starts: Vec<usize>,
    pub ends: Vec<usize>,
}

const WORD: u8 = 1;
const PUNCTUATION: u8 = 2;

fn class(c: u32) -> u8 {
    if c >= 128 {
        return 0;
    }
    let ch = c as u8;
    if ch.is_ascii_alphanumeric() || ch == b'_' {
        WORD
    } else if b"!#$&*+/=?^|~".contains(&ch) {
        PUNCTUATION
    } else {
        0
    }
}

const KEYWORD_LENGTH: [usize; 21] = [
    0, 12, 5, 14, 3, 9, 2, 2, 2, 2, 2, 4, 3, 4, 8, 5, 5, 7, 6, 4, 5,
];

fn is_word(c: u32) -> bool {
    class(c) & WORD != 0
}

fn at(src: &[u16], i: usize) -> u32 {
    src.get(i).map_or(NONE, |&c| c as u32)
}

fn starts_with(src: &[u16], i: usize, word: &str) -> bool {
    let w = word.as_bytes();
    src.len() >= i + w.len() && w.iter().enumerate().all(|(k, &b)| src[i + k] == b as u16)
}

/// Mermaid の INCLUDE と EXTEND だけは大文字小文字を問わずに合う。
fn starts_with_no_case(src: &[u16], i: usize, word: &str) -> bool {
    word.bytes()
        .enumerate()
        .all(|(k, b)| at(src, i + k) | 32 == b as u32)
}

/// `at` からの文字の頭になるキーワード。Mermaid の字句の並びの順に試す。
fn keyword_at(src: &[u16], i: usize, c: u32) -> u8 {
    match c {
        97 => {
            if starts_with(src, i, "actor") {
                t::ACTOR
            } else {
                0
            }
        }
        115 => {
            if starts_with(src, i, "systemBoundary") {
                t::SYSTEM_BOUNDARY
            } else if starts_with(src, i, "style") {
                t::STYLE
            } else {
                0
            }
        }
        101 => {
            if starts_with(src, i, "end") {
                t::END
            } else if starts_with_no_case(src, i, "extend") {
                t::EXTEND
            } else {
                0
            }
        }
        69 => {
            if starts_with_no_case(src, i, "extend") {
                t::EXTEND
            } else {
                0
            }
        }
        100 => {
            if starts_with(src, i, "direction") {
                t::DIRECTION
            } else {
                0
            }
        }
        84 => {
            if starts_with(src, i, "TD") {
                t::TD
            } else if starts_with(src, i, "TB") {
                t::TB
            } else {
                0
            }
        }
        66 => {
            if starts_with(src, i, "BT") {
                t::BT
            } else {
                0
            }
        }
        76 => {
            if starts_with(src, i, "LR") {
                t::LR
            } else {
                0
            }
        }
        82 => {
            if starts_with(src, i, "RL") {
                t::RL
            } else {
                0
            }
        }
        110 => {
            if starts_with(src, i, "note") {
                t::NOTE
            } else {
                0
            }
        }
        102 => {
            if starts_with(src, i, "for") {
                t::FOR
            } else if starts_with(src, i, "false") {
                t::FALSE
            } else {
                0
            }
        }
        106 => {
            if starts_with(src, i, "json") {
                t::JSON
            } else {
                0
            }
        }
        99 => {
            if starts_with(src, i, "classDef") {
                t::CLASS_DEF
            } else if starts_with(src, i, "class") {
                t::CLASS
            } else {
                0
            }
        }
        105 | 73 => {
            if starts_with_no_case(src, i, "include") {
                t::INCLUDE
            } else {
                0
            }
        }
        116 => {
            if starts_with(src, i, "true") {
                t::TRUE
            } else {
                0
            }
        }
        117 if starts_with(src, i, "usecase-beta") => t::USECASE,
        _ => 0,
    }
}

pub fn line_column(src: &[u16], offset: usize) -> (usize, usize) {
    let (src, offset) = written(src, offset);
    let mut line = 1;
    let mut line_start = 0;
    let mut i = 0;
    while i < offset {
        let c = at(&src, i);
        if c != 10 && c != 13 {
            i += 1;
            continue;
        }
        if c == 13 && i + 1 < offset && at(&src, i + 1) == 10 {
            i += 1;
        }
        line += 1;
        line_start = i + 1;
        i += 1;
    }
    (line, offset - line_start + 1)
}

pub fn location_text(src: &[u16], start: usize, end: usize) -> String {
    let (line, column) = line_column(src, start);
    format!(
        "line {line}, column {column} [{},{})",
        written(src, start).1,
        written(src, end).1
    )
}

/// `at` で開く引用の文字列の終わり。行が先に終われば None。
fn string_end(src: &[u16], i: usize, quote: u32) -> Option<usize> {
    for (k, &c) in src.iter().enumerate().skip(i + 1) {
        let c = c as u32;
        if c == quote {
            return Some(k + 1);
        }
        if c == 10 || c == 13 {
            return None;
        }
    }
    None
}

fn starts_token(src: &[u16], i: usize) -> bool {
    let c = at(src, i);
    if c != 34 && c != 39 {
        return true;
    }
    (c == 34 && at(src, i + 1) == 96) || string_end(src, i, c).is_some()
}

/// Mermaid は、どの字句も始まらない最初の文字と、字句解析がそこから字句の始まるまでに飛ばした文字の数を知らせる。
fn lex_error(src: &[u16], offset: usize, length: usize) -> PeleError {
    let (line, column) = line_column(src, offset);
    let from = written(src, offset).1;
    let to = written(src, offset + length).1;
    let mut e = PeleError::typed(
        format!(
            "Error lexing usecase diagram: unexpected character: ->{}<- at offset: {from}, skipped {} characters. at line {line}, column {column} [{from},{to})",
            js::from16(&src[offset..offset + 1]),
            to - from
        ),
        ErrorCode::Syntax,
        "usecase",
    );
    e.line = line;
    e.column = column;
    e
}

pub fn tokenize(src: &[u16]) -> Result<Tokens> {
    let mut types: Vec<u8> = Vec::new();
    let mut starts: Vec<usize> = Vec::new();
    let mut ends: Vec<usize> = Vec::new();
    let n = src.len();
    let mut i = 0;
    // この行でここまで空白しかないか。コメントとアクセシビリティの行はそこでだけ始まる。
    let mut line_start = true;
    let mut no_brace = false;

    while i < n {
        let c = src[i] as u32;
        if c == 32 || c == 9 {
            i += 1;
            continue;
        }
        let type_: u8;
        let mut end = i + 1;
        let mut fresh = false;

        if is_word(c) {
            let mut ty = 0u8;
            if c == 97 && line_start && starts_with(src, i, "acc") {
                let descr = starts_with(src, i + 3, "Descr");
                if descr || starts_with(src, i + 3, "Title") {
                    let mut j = i + 8;
                    while is_blank(at(src, j)) {
                        j += 1;
                    }
                    let next = at(src, j);
                    if next == 58 {
                        ty = if descr {
                            t::ACC_DESCR_LINE
                        } else {
                            t::ACC_TITLE_LINE
                        };
                        end = j + 1;
                        while end < n && src[end] != 10 && src[end] != 13 {
                            end += 1;
                        }
                    } else if next == 123 && descr && !no_brace {
                        match src[j + 1..].iter().position(|&x| x == b'}' as u16) {
                            None => no_brace = true,
                            Some(k) => {
                                ty = t::ACC_DESCR_BLOCK;
                                end = j + 1 + k + 1;
                            }
                        }
                    }
                }
            } else if c == 106 && starts_with(src, i, "json") && is_blank(at(src, i + 4)) {
                let mut j = i + 5;
                while is_blank(at(src, j)) {
                    j += 1;
                }
                let id = j;
                while is_word(at(src, j)) {
                    j += 1;
                }
                if j > id {
                    while is_blank(at(src, j)) {
                        j += 1;
                    }
                    if at(src, j) == 64 {
                        j += 1;
                        while is_blank(at(src, j)) {
                            j += 1;
                        }
                        if at(src, j) == 123 {
                            types.push(t::JSON_DECLARATION_START);
                            starts.push(i);
                            ends.push(j);
                            let mut depth = 0i64;
                            let mut quoted = false;
                            let mut escaped = false;
                            let mut k = j;
                            while k < n {
                                let ch = src[k];
                                if quoted {
                                    if escaped {
                                        escaped = false;
                                    } else if ch == 92 {
                                        escaped = true;
                                    } else if ch == 34 {
                                        quoted = false;
                                    }
                                } else if ch == 34 {
                                    quoted = true;
                                } else if ch == 123 {
                                    depth += 1;
                                } else if ch == 125 {
                                    depth -= 1;
                                    if depth == 0 {
                                        break;
                                    }
                                }
                                k += 1;
                            }
                            types.push(if k < n {
                                t::JSON_OBJECT_LITERAL
                            } else {
                                t::UNCLOSED_JSON_OBJECT_LITERAL
                            });
                            starts.push(j);
                            i = if k < n { k + 1 } else { n };
                            ends.push(i);
                            line_start = false;
                            continue;
                        }
                    }
                }
            }
            if ty == 0 {
                while end < n && is_word(src[end] as u32) {
                    end += 1;
                }
                let keyword = keyword_at(src, i, c);
                if keyword != 0 {
                    // 長い名前の頭にすぎないキーワードは名前。
                    if keyword == t::USECASE || end - i == KEYWORD_LENGTH[keyword as usize] {
                        ty = keyword;
                        end = i + KEYWORD_LENGTH[keyword as usize];
                    } else {
                        ty = t::IDENTIFIER;
                    }
                } else if (c == 111 || c == 120) && at(src, i + 1) == 45 && at(src, i + 2) == 45 {
                    ty = if c == 111 {
                        t::BACKWARD_CIRCLE
                    } else {
                        t::BACKWARD_CROSS
                    };
                    end = i + 3;
                } else if is_digit(c) {
                    // `1mg` は名前、`1.5px` は数（名前は点で止まるので）。
                    let mut digits = i + 1;
                    while is_digit(at(src, digits)) {
                        digits += 1;
                    }
                    if at(src, digits) == 46 && is_digit(at(src, digits + 1)) {
                        ty = t::NUMBER;
                        end = digits + 2;
                        while is_digit(at(src, end)) {
                            end += 1;
                        }
                        while is_letter(at(src, end)) {
                            end += 1;
                        }
                    } else {
                        ty = t::IDENTIFIER;
                    }
                } else {
                    ty = t::IDENTIFIER;
                    while at(src, end) == 45 && is_word(at(src, end + 1)) {
                        ty = t::CSS_IDENTIFIER;
                        end += 2;
                        while is_word(at(src, end)) {
                            end += 1;
                        }
                    }
                }
            }
            type_ = ty;
        } else if c == 10 {
            type_ = t::NEWLINE;
            fresh = true;
        } else if c == 13 {
            type_ = t::NEWLINE;
            fresh = true;
            if at(src, end) == 10 {
                end += 1;
            }
        } else if c == 34 || c == 39 {
            if c == 34 && at(src, i + 1) == 96 {
                let close =
                    (i + 2..n.saturating_sub(1)).find(|&k| src[k] == 96 && src[k + 1] == 34);
                match close {
                    None => {
                        type_ = t::UNCLOSED_MARKDOWN_STRING;
                        end = n;
                    }
                    Some(k) => {
                        type_ = t::MARKDOWN_STRING;
                        end = k + 2;
                    }
                }
            } else {
                type_ = t::PLAIN_STRING;
                match string_end(src, i, c) {
                    Some(e) => end = e,
                    None => {
                        let mut resume = i + 1;
                        while resume < n && !starts_token(src, resume) {
                            resume += 1;
                        }
                        return Err(lex_error(src, i, resume - i));
                    }
                }
            }
        } else if c == 37 {
            if line_start && at(src, end) == 37 {
                type_ = t::COMMENT;
                while end < n && src[end] != 10 && src[end] != 13 {
                    end += 1;
                }
            } else {
                type_ = t::PERCENT;
            }
        } else if c == 45 {
            if at(src, end) != 45 {
                type_ = t::DASH;
            } else {
                while at(src, end) == 45 {
                    end += 1;
                }
                let next = at(src, end);
                let two = end - i == 2;
                if two && next == 124 && at(src, end + 1) == 62 {
                    type_ = t::GENERALIZATION;
                    end += 2;
                } else if next == 62 {
                    type_ = t::FORWARD_SOLID;
                    end += 1;
                } else if two && (next == 111 || next == 120) {
                    type_ = if next == 111 {
                        t::FORWARD_CIRCLE
                    } else {
                        t::FORWARD_CROSS
                    };
                    end += 1;
                } else {
                    type_ = t::MARKERLESS_SOLID;
                }
            }
        } else if c == 46 {
            let next = at(src, end);
            if next == 46 && at(src, end + 1) == 62 {
                type_ = t::DEPENDENCY_ARROW;
                end += 2;
            } else if is_digit(next) {
                type_ = t::NUMBER;
                while is_digit(at(src, end)) {
                    end += 1;
                }
                while is_letter(at(src, end)) {
                    end += 1;
                }
            } else {
                type_ = t::DOT;
            }
        } else if c == 60 {
            if at(src, end) == 60 {
                types.push(t::STEREOTYPE_START);
                starts.push(i);
                ends.push(i + 2);
                i += 2;
                if i >= n {
                    break;
                }
                if at(src, i) == 62 && at(src, i + 1) == 62 {
                    type_ = t::STEREOTYPE_END;
                    end = i + 2;
                } else {
                    let mut close = i;
                    let mut found = false;
                    while close < n {
                        let ch = src[close] as u32;
                        if ch == 10 || ch == 13 {
                            break;
                        }
                        if ch == 62 && at(src, close + 1) == 62 {
                            found = true;
                            break;
                        }
                        close += 1;
                    }
                    if found && !js::trim(&js::from16(&src[i..close])).is_empty() {
                        types.push(t::STEREOTYPE_TEXT);
                        starts.push(i);
                        ends.push(close);
                        type_ = t::STEREOTYPE_END;
                        i = close;
                        end = close + 2;
                    } else {
                        while close < n && src[close] != 10 && src[close] != 13 {
                            close += 1;
                        }
                        if close == i {
                            while close < n && (src[close] == 10 || src[close] == 13) {
                                close += 1;
                            }
                            return Err(lex_error(src, i, close - i));
                        }
                        type_ = t::UNCLOSED_STEREOTYPE_TEXT;
                        end = close;
                    }
                }
            } else if at(src, end) == 45 && at(src, end + 1) == 45 {
                type_ = t::BACKWARD_SOLID;
                end += 2;
                while at(src, end) == 45 {
                    end += 1;
                }
            } else {
                type_ = t::LABEL_PUNCTUATION;
            }
        } else if c == 58 {
            if at(src, end) == 58 && at(src, end + 1) == 58 {
                type_ = t::CLASS_SEPARATOR;
                end += 2;
            } else {
                type_ = t::COLON;
            }
        } else if c == 64 {
            if at(src, end) == 123 {
                type_ = t::METADATA_START;
                end += 1;
            } else {
                type_ = t::AT;
            }
        } else if c == 123 {
            type_ = t::LBRACE;
        } else if c == 125 {
            type_ = t::RBRACE;
        } else if c == 91 {
            type_ = t::LBRACKET;
        } else if c == 93 {
            type_ = t::RBRACKET;
        } else if c == 40 {
            type_ = t::LPAREN;
        } else if c == 41 {
            type_ = t::RPAREN;
        } else if c == 44 {
            type_ = t::COMMA;
        } else if c == 35 {
            while is_hex(at(src, end)) {
                end += 1;
            }
            type_ = if end > i + 1 {
                t::HASH_COLOR
            } else {
                t::CSS_PUNCTUATION
            };
        } else if c == 92 {
            if at(src, end) == 44 {
                type_ = t::CSS_ESCAPED_COMMA;
                end += 1;
            } else {
                type_ = t::LABEL_PUNCTUATION;
            }
        } else if c < 127 && c > 32 {
            type_ = if class(c) & PUNCTUATION != 0 {
                t::CSS_PUNCTUATION
            } else {
                t::LABEL_PUNCTUATION
            };
        } else {
            type_ = t::LABEL_SYMBOL;
            while end < n {
                let ch = src[end] as u32;
                if (32..127).contains(&ch) || ch == 9 || ch == 10 || ch == 13 {
                    break;
                }
                end += 1;
            }
        }

        types.push(type_);
        starts.push(i);
        ends.push(end);
        i = end;
        line_start = fresh;
    }

    for _ in 0..3 {
        types.push(t::EOF);
        starts.push(n);
        ends.push(n);
    }
    Ok(Tokens {
        types,
        starts,
        ends,
    })
}
