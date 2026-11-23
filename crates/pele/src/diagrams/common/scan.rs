//! Mermaid の journey と timeline の文法に共通の字句の規則（pele の `src/diagrams/common/scan.ts`）。Jison の
//! 規則から移した: コメント、空白、accTitle と accDescr の文。`src` は UTF-16 の符号単位。

use crate::js;
use crate::util::chars::{NONE, is_space};

pub struct Tokens {
    pub types: Vec<u8>,
    pub texts: Vec<String>,
    pub starts: Vec<usize>,
}

fn at(src: &[u16], p: usize) -> u32 {
    src.get(p).map_or(NONE, |&c| c as u32)
}

fn index_of(src: &[u16], c: u16, from: usize) -> Option<usize> {
    src.get(from..)?
        .iter()
        .position(|&x| x == c)
        .map(|k| from + k)
}

pub fn line_end(src: &[u16], from: usize) -> usize {
    index_of(src, b'\n' as u16, from).unwrap_or(src.len())
}

/// `word` は ASCII の小文字。文法は大文字小文字を区別しない。
pub fn keyword_at(src: &[u16], p: usize, word: &str) -> bool {
    word.bytes()
        .enumerate()
        .all(|(i, w)| at(src, p + i) | 32 == w as u32)
}

/// コメントと空白を飛ばす。字句を始める改行（NEWLINE）で止まる。Jison は `\%%` を `%` にするので、`%` 1 つで
/// コメントが始まる。
pub fn skip_trivia(src: &[u16], from: usize) -> usize {
    let n = src.len();
    let mut p = from;
    while p < n {
        let c = src[p] as u32;
        if c == 37 && at(src, p + 1) != 123 {
            p = line_end(src, p);
        } else if c == 10 {
            return p;
        } else if is_space(c) {
            p += 1;
            while p < n && is_space(src[p] as u32) {
                p += 1;
            }
        } else if c == 35 {
            p = line_end(src, p);
        } else {
            return p;
        }
    }
    p
}

fn skip_space(src: &[u16], from: usize) -> usize {
    let mut p = from;
    while is_space(at(src, p)) {
        p += 1;
    }
    p
}

/// `p` の accTitle か accDescr の文を区切り、その終わりを返す。なければ `p`。字句の種類 acc_title、
/// acc_title_value、acc_descr、acc_descr_value、acc_descr_multiline_value は `first` からの続き番号であること。
pub fn accessibility(src: &[u16], p: usize, out: &mut Tokens, first: u8) -> usize {
    let descr = keyword_at(src, p, "accdescr");
    if !descr && !keyword_at(src, p, "acctitle") {
        return p;
    }
    let n = src.len();
    let mut q = skip_space(src, p + 8);
    let c = at(src, q);
    let mut emit = |type_: u8, s: usize, e: usize| {
        out.types.push(type_);
        out.texts.push(js::from16(&src[s..e]));
        out.starts.push(s);
    };
    if c == 58 {
        q = skip_space(src, q + 1);
        let end = line_end(src, q);
        emit(if descr { first + 2 } else { first }, p, q);
        emit(if descr { first + 3 } else { first + 1 }, q, end);
        return end;
    }
    if !descr || c != 123 {
        return p;
    }
    q = skip_space(src, q + 1);
    while at(src, q) != 125 {
        let end = index_of(src, b'}' as u16, q).unwrap_or(n);
        emit(first + 4, q, end);
        if q == n {
            return n;
        }
        q = end;
    }
    q + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::js::utf16;

    fn lex(src: &str, p: usize) -> (usize, Vec<(u8, String, usize)>) {
        let src = utf16(src);
        let mut out = Tokens {
            types: vec![],
            texts: vec![],
            starts: vec![],
        };
        let end = accessibility(&src, p, &mut out, 10);
        let list = (0..out.types.len())
            .map(|i| (out.types[i], out.texts[i].clone(), out.starts[i]))
            .collect();
        (end, list)
    }

    #[test]
    fn lexes_accessibility() {
        assert_eq!(
            lex("accTitle : The title\nx", 0),
            (
                20,
                vec![(10, "accTitle : ".into(), 0), (11, "The title".into(), 11)]
            )
        );
        assert_eq!(
            lex("ACCDESCR: d", 0),
            (11, vec![(12, "ACCDESCR: ".into(), 0), (13, "d".into(), 10)])
        );
        assert_eq!(
            lex("accDescr {\n a\n b }", 0),
            (18, vec![(14, "a\n b ".into(), 12)])
        );
        assert_eq!(
            lex("accDescr { open", 0),
            (15, vec![(14, "open".into(), 11), (14, "".into(), 15)])
        );
        assert_eq!(lex("accTitle {x}", 0), (0, vec![]));
        assert_eq!(lex("title", 0), (0, vec![]));
    }

    #[test]
    fn skips_trivia() {
        let src = utf16("  %% c\n#x");
        assert_eq!(skip_trivia(&src, 0), 6);
        assert_eq!(skip_trivia(&utf16(" %{x"), 0), 1);
        assert_eq!(skip_trivia(&utf16("#c"), 0), 2);
        assert!(keyword_at(&utf16("Journey"), 0, "journey"));
        assert!(!keyword_at(&utf16("jour"), 0, "journey"));
    }
}
