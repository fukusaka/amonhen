//! mailmap（`.mailmap`・`mailmap.blob`・`mailmap.file`）による名前とメールの置き換え。
//!
//! コミットには記録された名前とメールをそのまま持ち、見せるときにここで置き換える（`.mailmap` を変えても History の
//! キャッシュは作り直さない）。読むのはリポジトリのある側（`GitBackend::mailmap`）、置き換えはコア。

use std::borrow::Cow;
use std::cmp::Ordering;

/// mailmap の 1 行。`old_name` があれば、名前とメールの両方が合うコミットだけに効く。
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MailmapEntry {
    pub old_email: String,
    pub old_name: Option<String>,
    pub new_name: Option<String>,
    pub new_email: Option<String>,
}

/// リポジトリの mailmap と、置き換えるか（git の `log.mailmap`、既定は true）。
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Mailmap {
    /// `log.mailmap`。false なら置き換えない（Blame は git が置き換えるので、これに関係なく置き換わる）。
    pub enabled: bool,
    /// `old_email`・`old_name` の順（大文字・小文字を区別しない）に並べたもの（`Mailmap::new`）。
    entries: Vec<MailmapEntry>,
}

impl Default for Mailmap {
    fn default() -> Self {
        Self::new(true, Vec::new())
    }
}

/// git と同じく、ASCII の大文字・小文字を区別せずに比べる。
fn cmp_ignore_case(a: &[u8], b: &[u8]) -> Ordering {
    a.iter()
        .map(u8::to_ascii_lowercase)
        .cmp(b.iter().map(u8::to_ascii_lowercase))
}

fn key(e: &MailmapEntry) -> (&[u8], Option<&[u8]>) {
    (
        e.old_email.as_bytes(),
        e.old_name.as_deref().map(str::as_bytes),
    )
}

fn cmp_key(a: (&[u8], Option<&[u8]>), b: (&[u8], Option<&[u8]>)) -> Ordering {
    cmp_ignore_case(a.0, b.0).then_with(|| match (a.1, b.1) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some(x), Some(y)) => cmp_ignore_case(x, y),
    })
}

/// 置き換えた名前とメール。置き換わらなかった方は記録のまま。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity<'a> {
    pub name: Cow<'a, [u8]>,
    pub email: Cow<'a, [u8]>,
}

impl Mailmap {
    /// `entries` は読んだ順（同じ鍵なら後のものが勝つ。git と同じ）。
    pub fn new(enabled: bool, entries: Vec<MailmapEntry>) -> Self {
        let mut merged: Vec<MailmapEntry> = Vec::with_capacity(entries.len());
        let mut entries = entries;
        // 同じ鍵の中では読んだ順を保つ（安定な並べ替え）ので、後のものを前のものに重ねられる。
        entries.sort_by(|a, b| cmp_key(key(a), key(b)));
        for e in entries {
            match merged.last_mut() {
                Some(last) if cmp_key(key(last), key(&e)) == Ordering::Equal => {
                    if e.new_name.is_some() {
                        last.new_name = e.new_name;
                    }
                    if e.new_email.is_some() {
                        last.new_email = e.new_email;
                    }
                }
                _ => merged.push(e),
            }
        }
        Self {
            enabled,
            entries: merged,
        }
    }

    pub fn entries(&self) -> &[MailmapEntry] {
        &self.entries
    }

    /// 置き換えるものがあるか（`log.mailmap` が false か、行がなければ false）。
    pub fn is_active(&self) -> bool {
        self.enabled && !self.entries.is_empty()
    }

    /// 中身の指紋（変わったかを安く比べるため。プロセスやビルドをまたいでも同じ値）。
    pub fn fingerprint(&self) -> u64 {
        // FNV-1a。
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut eat = |bytes: &[u8]| {
            for &b in bytes.iter().chain(&[0xff]) {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
        };
        eat(&[u8::from(self.enabled)]);
        for e in &self.entries {
            for part in [
                Some(&e.old_email),
                e.old_name.as_ref(),
                e.new_name.as_ref(),
                e.new_email.as_ref(),
            ] {
                eat(part.map_or(&[0xfe][..], |s| s.as_bytes()));
            }
        }
        h
    }

    /// 名前とメールで合う行。名前も合う行があればそれを、なければメールだけの行を返す（git の `map_user` と同じ）。
    fn find(&self, name: &[u8], email: &[u8]) -> Option<&MailmapEntry> {
        let start = self
            .entries
            .partition_point(|e| cmp_ignore_case(e.old_email.as_bytes(), email) == Ordering::Less);
        let same: &[MailmapEntry] = {
            let rest = &self.entries[start..];
            let n = rest
                .iter()
                .take_while(|e| cmp_ignore_case(e.old_email.as_bytes(), email) == Ordering::Equal)
                .count();
            &rest[..n]
        };
        same.iter()
            .find(|e| {
                e.old_name
                    .as_deref()
                    .is_some_and(|n| cmp_ignore_case(n.as_bytes(), name) == Ordering::Equal)
            })
            .or_else(|| same.first().filter(|e| e.old_name.is_none()))
    }

    /// 記録された `name`・`email` に効く行（`log.mailmap` が false か、合う行がなければ `None`）。
    pub fn lookup(&self, name: &[u8], email: &[u8]) -> Option<&MailmapEntry> {
        self.enabled.then(|| self.find(name, email)).flatten()
    }

    /// 置き換えた名前とメール（置き換えなければ記録のまま）。
    pub fn map<'a>(&'a self, name: &'a [u8], email: &'a [u8]) -> Identity<'a> {
        match self.lookup(name, email) {
            Some(e) => Identity {
                name: Cow::Borrowed(e.new_name.as_deref().map_or(name, str::as_bytes)),
                email: Cow::Borrowed(e.new_email.as_deref().map_or(email, str::as_bytes)),
            },
            None => Identity {
                name: Cow::Borrowed(name),
                email: Cow::Borrowed(email),
            },
        }
    }

    /// `map` の文字列版。
    pub fn map_str(&self, name: &str, email: &str) -> (String, String) {
        let id = self.map(name.as_bytes(), email.as_bytes());
        (
            String::from_utf8_lossy(&id.name).into_owned(),
            String::from_utf8_lossy(&id.email).into_owned(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(
        old_email: &str,
        old_name: Option<&str>,
        new_name: Option<&str>,
        new_email: Option<&str>,
    ) -> MailmapEntry {
        MailmapEntry {
            old_email: old_email.into(),
            old_name: old_name.map(Into::into),
            new_name: new_name.map(Into::into),
            new_email: new_email.map(Into::into),
        }
    }

    fn mapped(m: &Mailmap, name: &str, email: &str) -> (String, String) {
        m.map_str(name, email)
    }

    #[test]
    fn maps_like_git() {
        let m = Mailmap::new(
            true,
            vec![
                // Proper Name <commit@email>
                entry("old@example.com", None, Some("Taro Yamada"), None),
                // <proper@email> <commit@email>
                entry("t@example.com", None, None, Some("taro@example.com")),
                // Proper Name <proper@email> Commit Name <commit@email>
                entry(
                    "shared@example.com",
                    Some("Bob"),
                    Some("Robert"),
                    Some("bob@example.com"),
                ),
            ],
        );
        assert_eq!(
            mapped(&m, "T. Yamada", "Old@Example.com"),
            ("Taro Yamada".into(), "Old@Example.com".into())
        );
        assert_eq!(
            mapped(&m, "T", "t@example.com"),
            ("T".into(), "taro@example.com".into())
        );
        assert_eq!(
            mapped(&m, "bob", "shared@example.com"),
            ("Robert".into(), "bob@example.com".into())
        );
        // 名前の合わない人は、メールだけの行がないので置き換えない。
        assert_eq!(
            mapped(&m, "Alice", "shared@example.com"),
            ("Alice".into(), "shared@example.com".into())
        );
        assert!(m.lookup(b"Alice", b"shared@example.com").is_none());
        assert_eq!(
            mapped(&m, "Carol", "carol@example.com"),
            ("Carol".into(), "carol@example.com".into())
        );
    }

    #[test]
    fn a_name_entry_wins_over_the_email_entry_and_falls_back_to_it() {
        let m = Mailmap::new(
            true,
            vec![
                entry("x@example.com", None, Some("Everyone"), None),
                entry("x@example.com", Some("Bot"), Some("The Bot"), None),
            ],
        );
        assert_eq!(mapped(&m, "bot", "x@example.com").0, "The Bot");
        assert_eq!(mapped(&m, "Someone", "x@example.com").0, "Everyone");
    }

    #[test]
    fn later_lines_override_earlier_ones() {
        let m = Mailmap::new(
            true,
            vec![
                entry("a@example.com", None, Some("First"), None),
                entry(
                    "A@example.com",
                    None,
                    Some("Second"),
                    Some("new@example.com"),
                ),
            ],
        );
        assert_eq!(m.entries().len(), 1);
        assert_eq!(
            mapped(&m, "a", "a@example.com"),
            ("Second".into(), "new@example.com".into())
        );
    }

    #[test]
    fn log_mailmap_false_keeps_the_recorded_identity() {
        let m = Mailmap::new(false, vec![entry("a@example.com", None, Some("A"), None)]);
        assert!(!m.is_active());
        assert_eq!(mapped(&m, "a", "a@example.com").0, "a");
        assert_ne!(
            m.fingerprint(),
            Mailmap::new(true, m.entries().to_vec()).fingerprint()
        );
    }
}
