//! UTF-16 の符号単位の判定（pele の `src/util/chars.ts`）。字句解析は `charCodeAt` で読むので、ここも符号単位で
//! 判定する。文字列の終わりの先は JS では NaN で、どれにも当たらない（ここでは `u32::MAX` を渡す）。

/// 終わりの先を表す値（JS の NaN）。
pub const NONE: u32 = u32::MAX;

/// `\s` が空白とみなすもの。
pub fn is_space(c: u32) -> bool {
    if c < 128 {
        return c == 32 || (9..=13).contains(&c);
    }
    c == 0xa0
        || c == 0x1680
        || (0x2000..=0x200a).contains(&c)
        || c == 0x2028
        || c == 0x2029
        || c == 0x202f
        || c == 0x205f
        || c == 0x3000
        || c == 0xfeff
}

pub fn is_blank(c: u32) -> bool {
    c == 32 || c == 9
}

/// 行の終わり（`.` が合わないもの）。
pub fn is_line_end(c: u32) -> bool {
    c == 10 || c == 13 || c == 0x2028 || c == 0x2029
}

pub fn is_digit(c: u32) -> bool {
    (48..=57).contains(&c)
}

pub fn is_letter(c: u32) -> bool {
    (65..=90).contains(&c) || (97..=122).contains(&c)
}

pub fn is_hex(c: u32) -> bool {
    (48..=57).contains(&c) || (65..=70).contains(&c) || (97..=102).contains(&c)
}

/// `\w` の文字。
pub fn is_word(c: u32) -> bool {
    (48..=57).contains(&c) || (65..=90).contains(&c) || (97..=122).contains(&c) || c == 95
}

/// `\s` の文字（`char` で）。
pub fn is_space_char(c: char) -> bool {
    is_space(c as u32)
}

/// `\w` の文字（`char` で）。
pub fn is_word_char(c: char) -> bool {
    is_word(c as u32)
}
