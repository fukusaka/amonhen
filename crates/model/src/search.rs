//! コミットの本文（件名・作者・ID）での検索。手元のコアと daemon で同じものを使う。

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::{CommitText, Oid, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextField {
    /// 件名・作者名・コミット ID の先頭のどれか。
    Any,
    Author,
    Subject,
    /// コミット ID の先頭（16 進数 4 文字以上）。
    Commit,
}

/// 1 つの条件。`needle` は小文字にしたもの（大文字・小文字を区別しない）。
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextTerm {
    pub field: TextField,
    pub needle: String,
}

impl TextTerm {
    pub fn new(field: TextField, value: &str) -> Self {
        Self {
            field,
            needle: value.to_lowercase(),
        }
    }
}

fn contains(text: &[u8], needle: &str) -> bool {
    if needle.is_ascii() {
        let n = needle.as_bytes();
        text.len() >= n.len() && text.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n))
    } else {
        String::from_utf8_lossy(text)
            .to_lowercase()
            .contains(needle)
    }
}

fn id_matches(id: &Oid, needle: &str) -> bool {
    needle.len() >= 4
        && needle.bytes().all(|b| b.is_ascii_hexdigit())
        && id.to_string().starts_with(needle)
}

/// `text`（ID は `id`）が `term` に合うか。
pub fn matches(term: &TextTerm, text: &CommitText, id: &Oid) -> bool {
    let n = term.needle.as_str();
    match term.field {
        TextField::Author => contains(text.author_name(), n),
        TextField::Subject => contains(text.subject(), n),
        TextField::Commit => id_matches(id, n),
        TextField::Any => {
            contains(text.subject(), n) || contains(text.author_name(), n) || id_matches(id, n)
        }
    }
}

/// `ids` のコミットを、行を分けて並列に読みながら調べる。条件ごとに、合ったコミットの添字（`ids` の中の
/// 位置）のビット列を返す。`cancelled` が true になったら `Err`。
pub fn scan(
    ids: &[Oid],
    terms: &[TextTerm],
    read: &(dyn Fn(&[Oid]) -> Result<Vec<CommitText>> + Sync),
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Vec<Vec<u64>>> {
    // バッチは 64 の倍数にして、ビット列の語をスレッドどうしで分け合わないようにする。
    const BATCH: usize = 4096;
    let threads = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .min(16);
    let next = AtomicUsize::new(0);
    let parts = Mutex::new(Vec::new());
    let failed = Mutex::new(None);
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let start = next.fetch_add(BATCH, Ordering::Relaxed);
                    if start >= ids.len() || cancelled() {
                        break;
                    }
                    let end = (start + BATCH).min(ids.len());
                    let texts = match read(&ids[start..end]) {
                        Ok(t) => t,
                        Err(e) => {
                            *failed.lock().unwrap() = Some(e.to_string());
                            break;
                        }
                    };
                    let mut words = vec![vec![0u64; (end - start).div_ceil(64)]; terms.len()];
                    for (i, t) in texts.iter().enumerate() {
                        for (k, term) in terms.iter().enumerate() {
                            if matches(term, t, &ids[start + i]) {
                                words[k][i / 64] |= 1 << (i % 64);
                            }
                        }
                    }
                    parts.lock().unwrap().push((start, words));
                }
            });
        }
    });
    if let Some(e) = failed.into_inner().unwrap() {
        return Err(e.into());
    }
    if cancelled() {
        return Err("検索を取り消しました".into());
    }
    let mut sets = vec![vec![0u64; ids.len().div_ceil(64)]; terms.len()];
    for (start, words) in parts.into_inner().unwrap() {
        for (k, w) in words.into_iter().enumerate() {
            sets[k][start / 64..start / 64 + w.len()].copy_from_slice(&w);
        }
    }
    Ok(sets)
}
