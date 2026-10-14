//! 変わった行の中の、変わった単語。削除行と追加行を 1 行ずつ組にして、単語単位で比べる。

use std::ops::Range;

use imara_diff::{Algorithm, Diff, InternedInput, TokenSource};

/// 単語（英数字と `_` の並び）、空白の並び、それ以外の 1 文字を 1 つの単位にする。
#[derive(Clone, Copy)]
struct Words<'a>(&'a [u8]);

/// 単語の区切りの位置（バイト）。
fn boundaries(text: &[u8]) -> Vec<usize> {
    let class = |b: u8| -> u8 {
        if b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80 {
            0
        } else if b.is_ascii_whitespace() {
            1
        } else {
            2
        }
    };
    let mut cuts = vec![0];
    for i in 1..text.len() {
        let (a, b) = (class(text[i - 1]), class(text[i]));
        // 記号は 1 文字ずつ、そのほかは同じ種類が続く間を 1 つにする。UTF-8 の続きのバイトでは切らない。
        if (a != b || a == 2) && text[i] & 0xC0 != 0x80 {
            cuts.push(i);
        }
    }
    cuts.push(text.len());
    cuts
}

impl<'a> TokenSource for Words<'a> {
    type Token = &'a [u8];
    type Tokenizer = std::vec::IntoIter<&'a [u8]>;

    fn tokenize(&self) -> Self::Tokenizer {
        let cuts = boundaries(self.0);
        cuts.windows(2)
            .map(|w| &self.0[w[0]..w[1]])
            .collect::<Vec<_>>()
            .into_iter()
    }

    fn estimate_tokens(&self) -> u32 {
        (self.0.len() / 4) as u32 + 1
    }
}

/// これより長い行は比べない（時間がかかり、強調しても読みにくい）。
const MAX_LINE: usize = 1000;

/// 変わった部分（バイト範囲）の並び。
pub type Ranges = Vec<Range<usize>>;

/// `old` と `new` の中で変わった部分（バイト範囲）。ほとんどが変わっていれば強調しない（行全体の色で足りる）。
pub fn changed_words(old: &[u8], new: &[u8]) -> Option<(Ranges, Ranges)> {
    if old.len() > MAX_LINE || new.len() > MAX_LINE {
        return None;
    }
    let input = InternedInput::new(Words(old), Words(new));
    let diff = Diff::compute(Algorithm::Histogram, &input);
    let (oc, nc) = (boundaries(old), boundaries(new));
    let mut removed = Vec::new();
    let mut added = Vec::new();
    for h in diff.hunks() {
        if h.before.start < h.before.end {
            removed.push(oc[h.before.start as usize]..oc[h.before.end as usize]);
        }
        if h.after.start < h.after.end {
            added.push(nc[h.after.start as usize]..nc[h.after.end as usize]);
        }
    }
    let changed: usize = removed.iter().chain(&added).map(|r| r.len()).sum();
    // 変わっていない部分が少なければ、単語の強調は役に立たない。
    if changed * 2 > old.len() + new.len() {
        return None;
    }
    Some((removed, added))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_changed_words() {
        let (r, a) = changed_words(b"let x = foo(1);", b"let x = bar(1);").unwrap();
        assert_eq!(r, vec![Range { start: 8, end: 11 }]);
        assert_eq!(a, vec![Range { start: 8, end: 11 }]);
        // 範囲は UTF-8 の文字の境界にそろう。
        let (old, new) = (
            "名前は太郎です。ok".as_bytes(),
            "名前は花子です。ok".as_bytes(),
        );
        if let Some((r, a)) = changed_words(old, new) {
            assert!(
                r.iter()
                    .all(|x| std::str::from_utf8(&old[x.clone()]).is_ok())
            );
            assert!(
                a.iter()
                    .all(|x| std::str::from_utf8(&new[x.clone()]).is_ok())
            );
        }
    }

    #[test]
    fn skips_lines_that_changed_almost_entirely() {
        assert!(changed_words(b"completely different", b"nothing alike here").is_none());
    }
}
