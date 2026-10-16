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
    fn merges_words_unless_both_sides_touch_the_same_place() {
        assert_eq!(
            merge3(b"f(a, b)", b"f(x, b)", b"f(a, y)").as_deref(),
            Some(&b"f(x, y)"[..])
        );
        // 同じ位置に別々のものを足した・同じ単語を別々に変えた
        assert_eq!(merge3(b"a b", b"a x b", b"a y b"), None);
        assert_eq!(merge3(b"v = 0", b"v = 1", b"v = 2"), None);
        // 同じ変更は 1 つにする
        assert_eq!(merge3(b"a b", b"a c", b"a c").as_deref(), Some(&b"a c"[..]));
    }

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

/// これより長いコンフリクトは単語単位でマージしない（時間がかかる）。
const MAX_MERGE: usize = 64 * 1024;

/// 共通の祖先 `base` から `ours`・`theirs` への変更を、単語単位で 3-way マージする。両方の変更が重なる
/// （同じ所か隣り合う所を変えた）なら、同じ変更でない限り None。
pub fn merge3(base: &[u8], ours: &[u8], theirs: &[u8]) -> Option<Vec<u8>> {
    if base.len() + ours.len() + theirs.len() > MAX_MERGE {
        return None;
    }
    let tokens = |text: &[u8]| -> Vec<Range<usize>> {
        boundaries(text).windows(2).map(|w| w[0]..w[1]).collect()
    };
    let (bt, ot, tt) = (tokens(base), tokens(ours), tokens(theirs));
    // 祖先からの変更: 祖先の単語の範囲と、置き換える中身（バイト）。
    let changes = |side: &[u8], st: &[Range<usize>]| -> Vec<(Range<u32>, Vec<u8>)> {
        let input = InternedInput::new(Words(base), Words(side));
        let mut diff = Diff::compute(Algorithm::Histogram, &input);
        diff.postprocess_no_heuristic(&input);
        diff.hunks()
            .map(|h| {
                let bytes = if h.after.is_empty() {
                    Vec::new()
                } else {
                    side[st[h.after.start as usize].start..st[h.after.end as usize - 1].end]
                        .to_vec()
                };
                (h.before, bytes)
            })
            .collect()
    };
    let (mut a, mut b) = (changes(ours, &ot), changes(theirs, &tt));
    // 祖先の位置の順に、両側の変更を 1 つの列にする。重なるものは、同じ変更なら 1 つにし、違えば諦める。
    let mut merged: Vec<(Range<u32>, Vec<u8>)> = Vec::new();
    a.reverse();
    b.reverse();
    loop {
        let next = match (a.last(), b.last()) {
            (None, None) => break,
            (Some(_), None) => a.pop().unwrap(),
            (None, Some(_)) => b.pop().unwrap(),
            (Some(x), Some(y)) => {
                // 重なる・隣り合う（片方が挿入で同じ位置を含む）なら、同じ変更でなければマージできない。
                if x.0.start <= y.0.end && y.0.start <= x.0.end {
                    if x == y {
                        b.pop();
                        a.pop().unwrap()
                    } else {
                        return None;
                    }
                } else if x.0.start < y.0.start {
                    a.pop().unwrap()
                } else {
                    b.pop().unwrap()
                }
            }
        };
        merged.push(next);
    }
    let byte = |i: u32| -> usize {
        if (i as usize) < bt.len() {
            bt[i as usize].start
        } else {
            base.len()
        }
    };
    let mut out = Vec::with_capacity(base.len().max(ours.len()).max(theirs.len()));
    let mut at = 0usize;
    for (range, bytes) in merged {
        out.extend_from_slice(&base[at..byte(range.start)]);
        out.extend_from_slice(&bytes);
        at = byte(range.end);
    }
    out.extend_from_slice(&base[at..]);
    Some(out)
}
