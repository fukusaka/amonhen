//! コミットの本文（件名・作者・ID）での検索。手元のコアと daemon で同じものを使う。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::{CommitText, Mailmap, Oid, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextField {
    /// 件名・作者名・コミット ID の先頭のどれか。
    Any,
    /// 作者名。mailmap があれば、置き換えた名前と記録された名前のどちらかに合えばよい。
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
    if needle.len() < 4 || needle.len() > 40 || !needle.bytes().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }
    // 16 進数の文字列を作らずに、半バイトずつ比べる（行ごとに文字列を作ると遅い）。
    needle.bytes().enumerate().all(|(i, c)| {
        let byte = id.0[i / 2];
        let nibble = if i % 2 == 0 { byte >> 4 } else { byte & 0x0f };
        (c as char).to_digit(16) == Some(u32::from(nibble))
    })
}

/// 作者（記録された `name`・`email`）が `needle` に合うか。mailmap があれば、置き換えた名前にも合わせる。
fn author_matches(needle: &str, name: &[u8], email: &[u8], mailmap: Option<&Mailmap>) -> bool {
    contains(name, needle)
        || mailmap
            .and_then(|m| m.lookup(name, email))
            .and_then(|e| e.new_name.as_deref())
            .is_some_and(|mapped| contains(mapped.as_bytes(), needle))
}

/// `text`（ID は `id`）が `term` に合うか。
pub fn matches(term: &TextTerm, text: &CommitText, id: &Oid, mailmap: Option<&Mailmap>) -> bool {
    let author = || {
        author_matches(
            &term.needle,
            text.author_name(),
            text.author_email(),
            mailmap,
        )
    };
    matches_parts(term, text.subject(), &author, id)
}

/// `author` は、作者が `term` の文字を含むか（作者を見る条件のときだけ呼ぶ）。
fn matches_parts(term: &TextTerm, subject: &[u8], author: &dyn Fn() -> bool, id: &Oid) -> bool {
    let n = term.needle.as_str();
    match term.field {
        TextField::Author => author(),
        TextField::Subject => contains(subject, n),
        TextField::Commit => id_matches(id, n),
        TextField::Any => contains(subject, n) || author() || id_matches(id, n),
    }
}

/// 読む件数のまとまり（64 の倍数にして、ビット列の語をスレッドどうしで分け合わないようにする）。
const BATCH: usize = 4096;

fn threads() -> usize {
    std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .min(16)
}

/// コミットの件名と作者を、`ids` の順に並べて置いたもの。初めて探すときに全部読み、次からはこれを探すだけに
/// する（毎回 git から読み直さない）。linux（148 万コミット）で、1 回の検索が 1.1 秒から 3〜20 ms になる。
pub struct TextIndex {
    ids: Vec<Oid>,
    /// 件名をつなげたもの。
    text: Vec<u8>,
    /// 行ごとの件名の始め。最後に全体の終わりを足す。
    bounds: Vec<u32>,
    /// 行ごとの作者（`people` の添字）。
    authors: Vec<u32>,
    /// 記録された作者の名前とメール（重ねずに 1 つずつ）。作者の条件は人ごとに 1 回だけ調べる（mailmap で
    /// 置き換えた名前も、人ごとに 1 回引けば済む）。
    people: Vec<(Vec<u8>, Vec<u8>)>,
}

/// 読んだ行: 行・件名・作者名・作者のメール。
type ReadRow = (usize, Vec<u8>, Vec<u8>, Vec<u8>);

/// 作者の名前とメールに番号を振る。
#[derive(Default)]
struct People {
    list: Vec<(Vec<u8>, Vec<u8>)>,
    /// 名前・0・メールをつなげたものから番号。
    index: std::collections::HashMap<Vec<u8>, u32>,
    key: Vec<u8>,
}

impl People {
    fn intern(&mut self, name: &[u8], email: &[u8]) -> u32 {
        self.key.clear();
        self.key.extend_from_slice(name);
        self.key.push(0);
        self.key.extend_from_slice(email);
        // ほとんどはもう見た人なので、引くだけで済ませる（行ごとに作らない）。
        if let Some(&i) = self.index.get(self.key.as_slice()) {
            return i;
        }
        let i = self.list.len() as u32;
        self.list.push((name.to_vec(), email.to_vec()));
        self.index.insert(self.key.clone(), i);
        i
    }
}

impl TextIndex {
    /// `ids` の索引を作る。`previous` にあるコミットはそこから写し、ないものだけ `read` で読む（並列に）。
    pub fn build(
        ids: &[Oid],
        previous: Option<&TextIndex>,
        read: &(dyn Fn(&[Oid]) -> Result<Vec<CommitText>> + Sync),
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Self> {
        // 前の索引の、コミットごとの位置。
        let old: Option<std::collections::HashMap<Oid, u32>> = previous.map(|p| {
            p.ids
                .iter()
                .enumerate()
                .map(|(i, id)| (*id, i as u32))
                .collect()
        });
        let missing: Vec<usize> = (0..ids.len())
            .filter(|&i| old.as_ref().is_none_or(|m| !m.contains_key(&ids[i])))
            .collect();
        let next = AtomicUsize::new(0);
        let read_parts = Mutex::new(Vec::new());
        let failed = Mutex::new(None);
        std::thread::scope(|scope| {
            for _ in 0..threads() {
                scope.spawn(|| {
                    loop {
                        let start = next.fetch_add(BATCH, Ordering::Relaxed);
                        if start >= missing.len() || cancelled() {
                            break;
                        }
                        let rows = &missing[start..(start + BATCH).min(missing.len())];
                        let batch: Vec<Oid> = rows.iter().map(|&r| ids[r]).collect();
                        match read(&batch) {
                            Ok(texts) => {
                                let part: Vec<ReadRow> = rows
                                    .iter()
                                    .zip(texts)
                                    .map(|(&r, t)| {
                                        (
                                            r,
                                            t.subject().to_vec(),
                                            t.author_name().to_vec(),
                                            t.author_email().to_vec(),
                                        )
                                    })
                                    .collect();
                                read_parts.lock().unwrap().push(part);
                            }
                            Err(e) => {
                                *failed.lock().unwrap() = Some(e.to_string());
                                break;
                            }
                        }
                    }
                });
            }
        });
        if let Some(e) = failed.into_inner().unwrap() {
            return Err(e.into());
        }
        if cancelled() {
            return Err(crate::i18n::cancelled().into());
        }
        type Fresh = Option<(Vec<u8>, Vec<u8>, Vec<u8>)>;
        let mut fresh: Vec<Fresh> = vec![None; ids.len()];
        for part in read_parts.into_inner().unwrap() {
            for (r, subject, name, email) in part {
                fresh[r] = Some((subject, name, email));
            }
        }
        let mut text = Vec::with_capacity(previous.map_or(0, |p| p.text.len()));
        let mut bounds = Vec::with_capacity(ids.len() + 1);
        let mut authors = Vec::with_capacity(ids.len());
        let mut people = People::default();
        for (r, id) in ids.iter().enumerate() {
            let (subject, name, email): (&[u8], &[u8], &[u8]) = match (&fresh[r], previous, &old) {
                (Some((s, n, e)), _, _) => (s, n, e),
                (None, Some(p), Some(m)) => {
                    let i = m[id] as usize;
                    let (n, e) = &p.people[p.authors[i] as usize];
                    (p.subject(i), n, e)
                }
                _ => return Err(crate::tr!("Can’t build the search index").into()),
            };
            bounds.push(text.len() as u32);
            text.extend_from_slice(subject);
            authors.push(people.intern(name, email));
        }
        bounds.push(text.len() as u32);
        Ok(Self {
            ids: ids.to_vec(),
            text,
            bounds,
            authors,
            people: people.list,
        })
    }

    fn subject(&self, row: usize) -> &[u8] {
        &self.text[self.bounds[row] as usize..self.bounds[row + 1] as usize]
    }

    /// 条件ごとに、合った行（`ids` の中の位置）のビット列（`scan` と同じ形）。行を分けて並列に調べる。作者の条件は、
    /// `mailmap` があれば置き換えた名前と記録された名前のどちらかに合う行。
    pub fn search(
        &self,
        terms: &[TextTerm],
        mailmap: Option<&Mailmap>,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Vec<Vec<u64>>> {
        // 条件ごとに、作者を見る条件なら人ごとに合うかを先に決めておく。
        let people: Vec<Vec<bool>> = terms
            .iter()
            .map(|term| match term.field {
                TextField::Author | TextField::Any => self
                    .people
                    .iter()
                    .map(|(name, email)| author_matches(&term.needle, name, email, mailmap))
                    .collect(),
                _ => Vec::new(),
            })
            .collect();
        let people = &people;
        let n = self.ids.len();
        let words = n.div_ceil(64);
        // スレッドごとの範囲も 64 の倍数にする（ビット列の語を分け合わない）。
        let per = words.div_ceil(threads()).max(1) * 64;
        let parts: Vec<(usize, Vec<Vec<u64>>)> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..n)
                .step_by(per)
                .map(|start| {
                    scope.spawn(move || {
                        let end = (start + per).min(n);
                        let mut out = vec![vec![0u64; (end - start).div_ceil(64)]; terms.len()];
                        for r in start..end {
                            if (r - start) % BATCH == 0 && cancelled() {
                                break;
                            }
                            let (subject, person) = (self.subject(r), self.authors[r] as usize);
                            for (k, term) in terms.iter().enumerate() {
                                let author = || people[k][person];
                                if matches_parts(term, subject, &author, &self.ids[r]) {
                                    out[k][(r - start) / 64] |= 1 << ((r - start) % 64);
                                }
                            }
                        }
                        (start, out)
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        if cancelled() {
            return Err(crate::i18n::cancelled().into());
        }
        let mut sets = vec![vec![0u64; words]; terms.len()];
        for (start, part) in parts {
            for (k, w) in part.into_iter().enumerate() {
                sets[k][start / 64..start / 64 + w.len()].copy_from_slice(&w);
            }
        }
        Ok(sets)
    }
}

/// 索引を 1 つ覚えておく所（リポジトリごと。共有の daemon では、つないでいる全員で 1 つ）。
#[derive(Default)]
pub struct TextIndexCache(Mutex<Option<Arc<TextIndex>>>);

impl TextIndexCache {
    /// `ids` を探す。覚えている索引が同じ `ids` のものならそれを、違えば前のものから引き継いで作り直してから。
    pub fn search(
        &self,
        ids: &[Oid],
        terms: &[TextTerm],
        mailmap: Option<&Mailmap>,
        read: &(dyn Fn(&[Oid]) -> Result<Vec<CommitText>> + Sync),
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Vec<Vec<u64>>> {
        let current = self.0.lock().unwrap().clone();
        let index = match current {
            Some(index) if index.ids == ids => index,
            previous => {
                let index = Arc::new(TextIndex::build(ids, previous.as_deref(), read, cancelled)?);
                *self.0.lock().unwrap() = Some(index.clone());
                index
            }
        };
        index.search(terms, mailmap, cancelled)
    }

    /// 覚えている索引の行の数と、件名のバイト数（なければ None）。
    pub fn stats(&self) -> Option<(usize, usize)> {
        self.0
            .lock()
            .unwrap()
            .as_ref()
            .map(|i| (i.ids.len(), i.text.len()))
    }
}

/// `ids` のコミットを、行を分けて並列に読みながら調べる。条件ごとに、合ったコミットの添字（`ids` の中の
/// 位置）のビット列を返す。`cancelled` が true になったら `Err`。
pub fn scan(
    ids: &[Oid],
    terms: &[TextTerm],
    mailmap: Option<&Mailmap>,
    read: &(dyn Fn(&[Oid]) -> Result<Vec<CommitText>> + Sync),
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Vec<Vec<u64>>> {
    let threads = threads();
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
                            if matches(term, t, &ids[start + i], mailmap) {
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
        return Err(crate::i18n::cancelled().into());
    }
    let mut sets = vec![vec![0u64; ids.len().div_ceil(64)]; terms.len()];
    for (start, words) in parts.into_inner().unwrap() {
        for (k, w) in words.into_iter().enumerate() {
            sets[k][start / 64..start / 64 + w.len()].copy_from_slice(&w);
        }
    }
    Ok(sets)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use super::*;

    fn oid(i: u8) -> Oid {
        let mut b = [0u8; 20];
        b[0] = 0xab;
        b[1] = i;
        Oid(b)
    }

    /// 件名は「Subject <i>」、作者は偶数なら Alice・奇数なら Bob。
    fn text(id: &Oid) -> CommitText {
        let i = id.0[1];
        let subject = format!("Fix Subject {i}");
        let author = if i.is_multiple_of(2) { "Alice" } else { "Bob" };
        let email = format!("{}@example.com", author.to_lowercase());
        let raw = format!("{subject}{author}{email}").into_bytes();
        let name_end = (subject.len() + author.len()) as u32;
        CommitText {
            subject: 0..subject.len() as u32,
            author_name: subject.len() as u32..name_end,
            author_email: name_end..raw.len() as u32,
            raw: Arc::new(raw),
            author_time: 0,
            signature: 0,
        }
    }

    fn rows(words: &[u64], n: usize) -> Vec<usize> {
        (0..n)
            .filter(|&i| words[i / 64] & (1 << (i % 64)) != 0)
            .collect()
    }

    #[test]
    fn searches_the_index_and_reads_only_new_commits_after_growing() {
        let reads = AtomicUsize::new(0);
        let read = |ids: &[Oid]| -> Result<Vec<CommitText>> {
            reads.fetch_add(ids.len(), Ordering::Relaxed);
            Ok(ids.iter().map(text).collect())
        };
        let never = || false;
        let cache = TextIndexCache::default();
        let ids: Vec<Oid> = (0..100).map(oid).collect();
        let terms = [
            TextTerm::new(TextField::Author, "alice"),
            TextTerm::new(TextField::Subject, "SUBJECT 7"),
            TextTerm::new(TextField::Any, "ab05"),
        ];
        let found = cache.search(&ids, &terms, None, &read, &never).unwrap();
        assert_eq!(rows(&found[0], 100).len(), 50);
        assert_eq!(
            rows(&found[1], 100),
            vec![7, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79]
        );
        assert_eq!(rows(&found[2], 100), vec![5], "ID の先頭（ab05…）");
        assert_eq!(reads.load(Ordering::Relaxed), 100);

        // 同じコミットなら読み直さない。
        cache.search(&ids, &terms, None, &read, &never).unwrap();
        assert_eq!(reads.load(Ordering::Relaxed), 100);

        // 新しいコミットが上に 2 つ増えたら、その 2 つだけ読む。
        let grown: Vec<Oid> = [oid(200), oid(201)]
            .into_iter()
            .chain(ids.iter().copied())
            .collect();
        let found = cache.search(&grown, &terms, None, &read, &never).unwrap();
        assert_eq!(reads.load(Ordering::Relaxed), 102);
        assert_eq!(
            rows(&found[1], 102)[0],
            9,
            "行がずれても同じコミットに合う（Subject 7 は 9 行目）"
        );
        assert!(
            rows(&found[0], 102).contains(&0),
            "新しいコミット（200、Alice）も探せる"
        );
    }

    #[test]
    fn an_author_matches_by_the_mapped_or_the_recorded_name() {
        let read = |ids: &[Oid]| -> Result<Vec<CommitText>> { Ok(ids.iter().map(text).collect()) };
        let never = || false;
        let ids: Vec<Oid> = (0..10).map(oid).collect();
        let mailmap = Mailmap::new(
            true,
            vec![crate::MailmapEntry {
                old_email: "bob@example.com".into(),
                old_name: None,
                new_name: Some("Robert".into()),
                new_email: None,
            }],
        );
        let terms = [
            TextTerm::new(TextField::Author, "robert"),
            TextTerm::new(TextField::Author, "bob"),
            TextTerm::new(TextField::Any, "rober"),
        ];
        let cache = TextIndexCache::default();
        let found = cache
            .search(&ids, &terms, Some(&mailmap), &read, &never)
            .unwrap();
        let odd = vec![1, 3, 5, 7, 9];
        assert_eq!(rows(&found[0], 10), odd, "置き換えた名前");
        assert_eq!(rows(&found[1], 10), odd, "記録された名前");
        assert_eq!(rows(&found[2], 10), odd);
        let found = cache.search(&ids, &terms, None, &read, &never).unwrap();
        assert!(
            rows(&found[0], 10).is_empty(),
            "mailmap がなければ置き換えた名前では合わない"
        );
        assert_eq!(
            scan(&ids, &terms[..1], Some(&mailmap), &read, &never).unwrap()[0],
            found_words(&odd),
        );
    }

    fn found_words(rows: &[usize]) -> Vec<u64> {
        let mut w = vec![0u64; 1];
        for &r in rows {
            w[0] |= 1 << r;
        }
        w
    }
}
