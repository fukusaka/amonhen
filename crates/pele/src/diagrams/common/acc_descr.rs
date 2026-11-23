//! 閉じない `accDescr {` の後も字句解析が続く文法の accDescr（pele の `src/diagrams/common/accDescr.ts`）。

use super::tokens::{ACC_DESCR, Pattern, Scanner, TokenType};
use crate::util::chars::{NONE, is_space};

fn at(src: &[u16], p: usize) -> u32 {
    src.get(p).map_or(NONE, |&c| c as u32)
}

/// `[\t ]*accDescr(?:[\t ]*:(?:[^\n\r]*?(?=%%)|[^\n\r]*)|\s*({))` の終わりと、`{` の形か。
fn head(src: &[u16], p: usize) -> Option<(usize, bool)> {
    let mut q = p;
    while matches!(at(src, q), 9 | 32) {
        q += 1;
    }
    if !src
        .get(q..)
        .is_some_and(|s| s.starts_with(&b"accDescr".map(u16::from)))
    {
        return None;
    }
    q += 8;
    let mut r = q;
    while matches!(at(src, r), 9 | 32) {
        r += 1;
    }
    if at(src, r) == 58 {
        let mut e = r + 1;
        while e < src.len() && src[e] != 10 && src[e] != 13 {
            if src[e] == 37 && at(src, e + 1) == 37 {
                break;
            }
            e += 1;
        }
        return Some((e, false));
    }
    let mut r = q;
    while is_space(at(src, r)) {
        r += 1;
    }
    (at(src, r) == 123).then_some((r + 1, true))
}

/// 波かっこの形は閉じのかっこを探す。1 度見つからなければ先にもないので、後の波かっこの説明は文字の残りを
/// 走査し直さずに合わない。
fn scanner() -> Scanner {
    let mut unclosed = false;
    Box::new(move |src, p, _| {
        let (end, braced) = head(src, p)?;
        if !braced {
            return Some(end);
        }
        if unclosed {
            return None;
        }
        let close = src[end..].iter().position(|&c| c == 125).map(|k| end + k);
        unclosed = close.is_none();
        close.map(|c| c + 1)
    })
}

/// 共通の accDescr の字句。`accDescr` が名前にもなる文法では、閉じない `accDescr {` の後も字句解析が続く。
/// そのたびに文字の終わりまで読まないようにする。
pub fn acc_descr(name: &str) -> TokenType {
    ACC_DESCR
        .clone()
        .named(name)
        .with_pattern(Pattern::scanner(scanner))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::js::utf16;

    #[test]
    fn scans_once_for_a_missing_brace() {
        let t = acc_descr("ACC_DESCR");
        let Pattern::Scanner(make) = &t.pattern else {
            panic!()
        };
        let mut scan = make();
        let src = utf16("accDescr { a\naccDescr { b\naccDescr: c %% x");
        assert_eq!(scan(&src, 0, -1), None);
        assert_eq!(scan(&src, 13, -1), None);
        assert_eq!(scan(&src, 26, -1), Some(38));
        let mut scan = make();
        assert_eq!(scan(&utf16(" accDescr\n{x}y"), 0, -1), Some(13));
        assert_eq!(t.first.as_deref(), Some("\t a"));
    }
}
