//! 大文字と小文字を区別しない語の照合（pele の `src/diagrams/common/caseless.ts`）。

use crate::util::chars::NONE;

/// 小文字で書いた `word` が `at` にあるか。どちらの大きさでも合うのは ASCII の英字だけ（`u` のない正規表現の
/// `i` と同じ）。`src` は UTF-16 の符号単位。
pub fn word_at(src: &[u16], at: usize, word: &str) -> bool {
    for (k, w) in word.encode_utf16().enumerate() {
        let c = src.get(at + k).map_or(NONE, |&c| c as u32);
        let w = w as u32;
        if c != w && !((97..=122).contains(&w) && c == w - 32) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::js::utf16;

    #[test]
    fn matches_ascii_letters_in_either_case() {
        let src = utf16("xyChart-Beta");
        assert!(word_at(&src, 0, "xychart-beta"));
        assert!(!word_at(&src, 1, "xychart"));
        assert!(!word_at(&src, 8, "betas"));
        assert!(word_at(&utf16("ÄB"), 1, "b"));
        assert!(!word_at(&utf16("ä"), 0, "Ä"));
    }
}
