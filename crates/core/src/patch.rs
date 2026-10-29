//! パッチのファイルを読む（当てる前の確認と、Undo のために触るファイルを知るため）。当てるのは git
//! （`git am`・`git apply`）で、ここは中身を見るだけ。

use serde::Serialize;

/// パッチのファイルの種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// メールの形（`git format-patch` の出力・mbox・.eml）。`git am` でコミットとして当てる。
    Mailbox,
    /// ただの diff（`git diff` の出力など）。`git apply` で作業ツリーに当てる。
    Diff,
}

/// パッチのファイルの中身。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub kind: Kind,
    /// メールの形なら 1 通ずつ。diff なら 1 つ（件名・作者はなし）。
    pub patches: Vec<Patch>,
}

/// パッチ 1 つ（メールなら 1 通）。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Patch {
    /// `git am` がコミットの件名にするもの（`[PATCH 1/3]` や `Re:` を除いたもの）。
    pub subject: String,
    pub author: Option<String>,
    pub author_email: Option<String>,
    /// メールの `Date:` のまま。
    pub date: Option<String>,
    pub files: Vec<FileChange>,
}

/// パッチが触るファイル 1 つ。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    /// 当てた後のパス（消すファイルなら消す前のパス）。
    pub path: String,
    /// 名前が変わる・消える・新しく作るときの、当てる前のパス（新しく作るなら `None`）。
    pub old_path: Option<String>,
    pub added: u32,
    pub removed: u32,
    pub binary: bool,
    /// 新しく作る（`None`）・消す・名前を変えるを区別する。
    pub status: FileStatus,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FileStatus {
    #[default]
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
}

impl Summary {
    /// パッチが触るファイルのパス（当てる前と後の両方。重なりは除く）。Undo で前の内容を残すのに使う。
    pub fn paths(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for file in self.patches.iter().flat_map(|p| &p.files) {
            for path in std::iter::once(&file.path).chain(file.old_path.as_ref()) {
                if !out.contains(path) {
                    out.push(path.clone());
                }
            }
        }
        out
    }
}

/// パッチのファイルを読む。メールの形（先頭が `From ` の行か、メールのヘッダー）なら 1 通ずつに分ける。
pub fn parse(data: &[u8]) -> Summary {
    let text = String::from_utf8_lossy(data);
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    if !is_mailbox(&lines) {
        return Summary {
            kind: Kind::Diff,
            patches: vec![Patch {
                files: parse_diff(&lines),
                ..Default::default()
            }],
        };
    }
    // `git mailsplit` と同じく、`From ` で始まる区切りの行で分ける（本文の `From ` と取り違えないよう、空行の
    // 次か先頭にあって時刻らしい `:` を含むものだけ）。
    let mut messages: Vec<&[&str]> = Vec::new();
    let mut start = 0;
    for i in 1..lines.len() {
        if is_from_line(lines[i]) && lines[i - 1].trim().is_empty() {
            messages.push(&lines[start..i]);
            start = i;
        }
    }
    messages.push(&lines[start..]);
    Summary {
        kind: Kind::Mailbox,
        patches: messages
            .into_iter()
            .filter(|m| m.iter().any(|l| !l.trim().is_empty()))
            .map(parse_message)
            .collect(),
    }
}

fn is_from_line(line: &str) -> bool {
    line.starts_with("From ") && line.contains(':')
}

/// メールの形か。先頭の空でない行が `From ` の区切りか、`Name: value` のヘッダーが続いて `From:` か
/// `Subject:` を含む。
fn is_mailbox(lines: &[&str]) -> bool {
    let mut rest = lines.iter().skip_while(|l| l.trim().is_empty());
    let Some(first) = rest.next() else {
        return false;
    };
    if is_from_line(first) {
        return true;
    }
    let mut mail = false;
    for line in std::iter::once(first).chain(rest) {
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if line.starts_with([' ', '\t']) {
            continue;
        }
        let Some((name, _)) = line.split_once(':') else {
            return false;
        };
        if name.is_empty() || name.contains(' ') {
            return false;
        }
        mail |= name.eq_ignore_ascii_case("from") || name.eq_ignore_ascii_case("subject");
    }
    mail
}

/// メール 1 通。ヘッダー（折り返しをつなぐ）と、本文の先頭の `From:`・`Date:`・`Subject:`（作者が送り手と
/// 違うときに `git format-patch` が書くもの）を読み、本文の diff を数える。
fn parse_message(lines: &[&str]) -> Patch {
    let mut lines = lines;
    if lines.first().is_some_and(|l| is_from_line(l)) {
        lines = &lines[1..];
    }
    let end = lines
        .iter()
        .position(|l| l.trim_end_matches(['\r', '\n']).is_empty())
        .unwrap_or(lines.len());
    let headers = unfold(&lines[..end]);
    let body = lines.get(end + 1..).unwrap_or_default();
    let mut patch = Patch::default();
    let apply = |patch: &mut Patch, name: &str, value: &str| {
        if name.eq_ignore_ascii_case("subject") {
            patch.subject = clean_subject(&decode_words(value));
        } else if name.eq_ignore_ascii_case("from") {
            let (name, email) = split_address(&decode_words(value));
            patch.author = name;
            patch.author_email = email;
        } else if name.eq_ignore_ascii_case("date") {
            patch.date = Some(value.trim().to_owned());
        }
    };
    for (name, value) in &headers {
        apply(&mut patch, name, value);
    }
    // 本文の先頭のヘッダー（空行まで）。
    let mut body_start = 0;
    let in_body: Vec<(String, String)> = {
        let first = body
            .iter()
            .position(|l| !l.trim().is_empty())
            .unwrap_or(body.len());
        let rest = &body[first..];
        let looks_like_header = |l: &str| {
            ["From:", "Date:", "Subject:"]
                .iter()
                .any(|h| l.len() >= h.len() && l[..h.len()].eq_ignore_ascii_case(h))
        };
        if rest.first().is_some_and(|l| looks_like_header(l)) {
            let end = rest
                .iter()
                .position(|l| l.trim().is_empty())
                .unwrap_or(rest.len());
            body_start = first + end;
            unfold(&rest[..end])
        } else {
            Vec::new()
        }
    };
    for (name, value) in &in_body {
        apply(&mut patch, name, value);
    }
    patch.files = parse_diff(&body[body_start..]);
    patch
}

/// ヘッダーの行を、折り返し（空白で始まる行）をつないで名前と値にする。
fn unfold(lines: &[&str]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in lines {
        let line = line.trim_end_matches(['\r', '\n']);
        if line.starts_with([' ', '\t']) {
            if let Some((_, value)) = out.last_mut() {
                value.push(' ');
                value.push_str(line.trim());
            }
        } else if let Some((name, value)) = line.split_once(':') {
            out.push((name.trim().to_owned(), value.trim().to_owned()));
        }
    }
    out
}

/// `git mailinfo` と同じく、件名の先頭の `Re:` と `[...]` を除く。
fn clean_subject(subject: &str) -> String {
    let mut s = subject.trim();
    loop {
        if s.len() >= 3 && s[..3].eq_ignore_ascii_case("re:") {
            s = s[3..].trim_start();
        } else if s.starts_with('[')
            && let Some(end) = s.find(']')
        {
            s = s[end + 1..].trim_start();
        } else {
            return s.to_owned();
        }
    }
}

/// `Name <email>` を分ける。名前の引用符は外す。
fn split_address(value: &str) -> (Option<String>, Option<String>) {
    let value = value.trim();
    if let (Some(open), Some(close)) = (value.rfind('<'), value.rfind('>'))
        && open < close
    {
        let name = value[..open].trim().trim_matches('"').trim();
        let email = value[open + 1..close].trim();
        return (
            (!name.is_empty()).then(|| name.to_owned()),
            (!email.is_empty()).then(|| email.to_owned()),
        );
    }
    if value.contains('@') {
        (None, Some(value.to_owned()))
    } else {
        ((!value.is_empty()).then(|| value.to_owned()), None)
    }
}

/// RFC 2047 の encoded-word（`=?UTF-8?q?...?=`・`=?UTF-8?b?...?=`）を戻す。並んだ encoded-word の間の空白は
/// 除く。UTF-8 と ISO-8859-1 のほかの文字コードは UTF-8 として読む。
fn decode_words(value: &str) -> String {
    let mut out = String::new();
    let mut rest = value;
    let mut last_encoded = false;
    while !rest.is_empty() {
        let Some(start) = rest.find("=?") else {
            out.push_str(rest);
            break;
        };
        let decoded = decode_word(&rest[start..]);
        let Some((text, len)) = decoded else {
            out.push_str(&rest[..start + 2]);
            rest = &rest[start + 2..];
            last_encoded = false;
            continue;
        };
        let between = &rest[..start];
        if !(last_encoded && between.trim().is_empty()) {
            out.push_str(between);
        }
        out.push_str(&text);
        rest = &rest[start + len..];
        last_encoded = true;
    }
    out
}

/// `s` の先頭の encoded-word を戻し、その長さと一緒に返す。
fn decode_word(s: &str) -> Option<(String, usize)> {
    let inner = s.strip_prefix("=?")?;
    let (charset, inner) = inner.split_once('?')?;
    let (encoding, inner) = inner.split_once('?')?;
    let end = inner.find("?=")?;
    let data = &inner[..end];
    let len = 2 + charset.len() + 1 + encoding.len() + 1 + end + 2;
    let bytes = match encoding {
        "q" | "Q" => {
            let mut out = Vec::new();
            let mut b = data.bytes();
            while let Some(c) = b.next() {
                match c {
                    b'_' => out.push(b' '),
                    b'=' => {
                        let h = [b.next()?, b.next()?];
                        out.push(u8::from_str_radix(std::str::from_utf8(&h).ok()?, 16).ok()?);
                    }
                    c => out.push(c),
                }
            }
            out
        }
        "b" | "B" => base64(data)?,
        _ => return None,
    };
    // 言語の指定（`UTF-8*ja`）は読まない。
    let charset = charset.split('*').next().unwrap_or_default();
    let text =
        if charset.eq_ignore_ascii_case("iso-8859-1") || charset.eq_ignore_ascii_case("latin1") {
            bytes.iter().map(|&b| b as char).collect()
        } else {
            String::from_utf8_lossy(&bytes).into_owned()
        };
    Some((text, len))
}

fn base64(data: &str) -> Option<Vec<u8>> {
    let value = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for c in data.bytes().filter(|&c| c != b'=') {
        acc = (acc << 6) | value(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// diff のファイルごとの変更。`diff --git` のヘッダーと、`---`・`+++` だけの diff（`diff -u`）の両方を読む。
/// 行の数は hunk のヘッダーの数だけ数える（本文の `-- ` の署名などを取り違えないように）。
fn parse_diff(lines: &[&str]) -> Vec<FileChange> {
    let mut files: Vec<FileChange> = Vec::new();
    // 今の hunk の残りの行（前, 後）。
    let mut left = (0u32, 0u32);
    // `diff --git` の行から、最初の hunk までの間。
    let mut in_header = false;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim_end_matches(['\r', '\n']);
        i += 1;
        if left != (0, 0) {
            match line.as_bytes().first() {
                Some(b'+') => {
                    left.1 = left.1.saturating_sub(1);
                    if let Some(f) = files.last_mut() {
                        f.added += 1;
                    }
                }
                Some(b'-') => {
                    left.0 = left.0.saturating_sub(1);
                    if let Some(f) = files.last_mut() {
                        f.removed += 1;
                    }
                }
                Some(b'\\') => {}
                _ => {
                    left.0 = left.0.saturating_sub(1);
                    left.1 = left.1.saturating_sub(1);
                }
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let (a, b) = split_git_paths(rest);
            files.push(FileChange {
                path: b.unwrap_or_default(),
                old_path: a,
                ..Default::default()
            });
            in_header = true;
            continue;
        }
        if line.starts_with("@@ ") {
            left = hunk_counts(line).unwrap_or((0, 0));
            in_header = false;
            continue;
        }
        if let Some(path) = line.strip_prefix("--- ")
            && let Some(next) = lines.get(i).map(|l| l.trim_end_matches(['\r', '\n']))
            && let Some(new) = next.strip_prefix("+++ ")
        {
            i += 1;
            let old = diff_path(path);
            let new = diff_path(new);
            // `diff --git` のヘッダーの中なら、そのファイルのパス。そうでなければ新しいファイル。
            if !in_header {
                files.push(FileChange::default());
            }
            in_header = false;
            let file = files.last_mut().expect("pushed");
            match (old, new) {
                (None, Some(new)) => {
                    file.status = FileStatus::Added;
                    file.path = new;
                    file.old_path = None;
                }
                (Some(old), None) => {
                    file.status = FileStatus::Deleted;
                    file.path = old;
                    file.old_path = None;
                }
                (Some(old), Some(new)) => {
                    if file.status == FileStatus::Modified && old != new {
                        file.status = FileStatus::Renamed;
                    }
                    file.old_path = (old != new).then_some(old);
                    file.path = new;
                }
                (None, None) => {}
            }
            continue;
        }
        let Some(file) = files.last_mut() else {
            continue;
        };
        if line.starts_with("new file mode") {
            file.status = FileStatus::Added;
            file.old_path = None;
        } else if line.starts_with("deleted file mode") {
            file.status = FileStatus::Deleted;
            if let Some(old) = file.old_path.take() {
                file.path = old;
            }
        } else if let Some(p) = line.strip_prefix("rename from ") {
            file.status = FileStatus::Renamed;
            file.old_path = Some(unquote(p));
        } else if let Some(p) = line.strip_prefix("rename to ") {
            file.path = unquote(p);
        } else if let Some(p) = line.strip_prefix("copy from ") {
            file.status = FileStatus::Copied;
            file.old_path = Some(unquote(p));
        } else if let Some(p) = line.strip_prefix("copy to ") {
            file.path = unquote(p);
        } else if line == "GIT binary patch" || line.starts_with("Binary files ") {
            file.binary = true;
        }
    }
    // 名前の変わらない変更には、前のパスを持たせない。
    for file in &mut files {
        if file.old_path.as_deref() == Some(file.path.as_str()) {
            file.old_path = None;
        }
        if file.status == FileStatus::Modified && file.old_path.is_some() {
            file.status = FileStatus::Renamed;
        }
    }
    files
}

/// `@@ -a,b +c,d @@` の b と d（`,` がなければ 1）。
fn hunk_counts(line: &str) -> Option<(u32, u32)> {
    let mut parts = line.split(' ').skip(1);
    let count = |s: &str| -> Option<u32> {
        match s[1..].split_once(',') {
            Some((_, n)) => n.parse().ok(),
            None => Some(1),
        }
    };
    let old = count(parts.next()?)?;
    let new = count(parts.next()?)?;
    Some((old, new))
}

/// `---`・`+++` の行のパス（`/dev/null` なら `None`）。`a/`・`b/` と、後ろのタブ（日時）を除く。
fn diff_path(s: &str) -> Option<String> {
    let s = s.split('\t').next().unwrap_or_default().trim_end();
    if s == "/dev/null" {
        return None;
    }
    let s = unquote(s);
    Some(strip_prefix(&s).to_owned())
}

fn strip_prefix(s: &str) -> &str {
    s.split_once('/').map_or(s, |(_, rest)| rest)
}

/// `diff --git a/x b/y` の 2 つのパス。引用符で囲んだものも読む。空白を含むパスは、前と後ろが同じ名前なら
/// 真ん中で分ける（git の `git_header_name` と同じ考え）。
fn split_git_paths(rest: &str) -> (Option<String>, Option<String>) {
    if rest.starts_with('"') {
        let (a, tail) = take_quoted(rest);
        let tail = tail.trim_start();
        let b = if tail.starts_with('"') {
            take_quoted(tail).0
        } else {
            tail.to_owned()
        };
        return (
            Some(strip_prefix(&a).to_owned()),
            Some(strip_prefix(&b).to_owned()),
        );
    }
    if let Some(pos) = rest.find(" \"") {
        let b = take_quoted(&rest[pos + 1..]).0;
        return (
            Some(strip_prefix(&rest[..pos]).to_owned()),
            Some(strip_prefix(&b).to_owned()),
        );
    }
    // 同じ長さの 2 つに分けられれば、それ（名前を変えないとき）。
    let bytes = rest.len();
    if bytes % 2 == 1 {
        let mid = bytes / 2;
        if rest.is_char_boundary(mid)
            && rest.as_bytes()[mid] == b' '
            && strip_prefix(&rest[..mid]) == strip_prefix(&rest[mid + 1..])
        {
            let path = strip_prefix(&rest[..mid]).to_owned();
            return (Some(path.clone()), Some(path));
        }
    }
    match rest.split_once(" b/") {
        Some((a, b)) => (Some(strip_prefix(a).to_owned()), Some(b.to_owned())),
        None => match rest.split_once(' ') {
            Some((a, b)) => (
                Some(strip_prefix(a).to_owned()),
                Some(strip_prefix(b).to_owned()),
            ),
            None => (None, None),
        },
    }
}

/// 引用符で囲んだ文字列（C の書き方の `\303\251` などを戻す）と、その後ろ。
fn take_quoted(s: &str) -> (String, &str) {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 1;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => return (String::from_utf8_lossy(&out).into_owned(), &s[i + 1..]),
            b'\\' if i + 1 < bytes.len() => {
                i += 1;
                match bytes[i] {
                    b'n' => out.push(b'\n'),
                    b't' => out.push(b'\t'),
                    b'0'..=b'7' => {
                        let end = (i + 3).min(bytes.len());
                        let digits = &s[i..end];
                        out.push(u8::from_str_radix(digits, 8).unwrap_or(b'?'));
                        i = end - 1;
                    }
                    c => out.push(c),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    (String::from_utf8_lossy(&out).into_owned(), "")
}

fn unquote(s: &str) -> String {
    if s.starts_with('"') {
        take_quoted(s).0
    } else {
        s.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SERIES: &str = "From 7e7483545ca94fc356725e6d9d00ab04180d8218 Mon Sep 17 00:00:00 2001
From: =?UTF-8?q?Ren=C3=A9=20Example?= <rene@example.com>
Date: Thu, 29 Oct 2026 03:19:33 +0900
Subject: [PATCH 1/2] =?UTF-8?q?change=201=20=C3=A9?=
 and more

Body text.
---
 f | 2 +-
 1 file changed, 1 insertion(+), 1 deletion(-)

diff --git a/f b/f
index 83db48f..bf5f2b1 100644
--- a/f
+++ b/f
@@ -1,3 +1,3 @@
-1
+x1
 2
 3
--
2.56.0

From 1111111111111111111111111111111111111111 Mon Sep 17 00:00:00 2001
From: Sender <s@example.com>
Date: Fri, 30 Oct 2026 10:00:00 +0900
Subject: [PATCH v2 2/2] Re: add a file

From: Author Name <author@example.com>

Message.
---
diff --git a/new.txt b/new.txt
new file mode 100644
index 0000000..e69de29
--- /dev/null
+++ b/new.txt
@@ -0,0 +1,2 @@
+a
+b
diff --git a/old name.txt b/new name.txt
similarity index 100%
rename from old name.txt
rename to new name.txt
--
2.56.0
";

    #[test]
    fn reads_a_series_of_mails() {
        let summary = parse(SERIES.as_bytes());
        assert_eq!(summary.kind, Kind::Mailbox);
        assert_eq!(summary.patches.len(), 2);
        let first = &summary.patches[0];
        assert_eq!(first.subject, "change 1 é and more");
        assert_eq!(first.author.as_deref(), Some("René Example"));
        assert_eq!(first.author_email.as_deref(), Some("rene@example.com"));
        assert_eq!(
            first.files,
            vec![FileChange {
                path: "f".into(),
                added: 1,
                removed: 1,
                ..Default::default()
            }]
        );
        let second = &summary.patches[1];
        // 本文の先頭の From: が作者。
        assert_eq!(second.subject, "add a file");
        assert_eq!(second.author.as_deref(), Some("Author Name"));
        assert_eq!(second.files.len(), 2);
        assert_eq!(second.files[0].status, FileStatus::Added);
        assert_eq!(second.files[0].added, 2);
        assert_eq!(second.files[1].status, FileStatus::Renamed);
        assert_eq!(second.files[1].path, "new name.txt");
        assert_eq!(second.files[1].old_path.as_deref(), Some("old name.txt"));
        assert_eq!(
            summary.paths(),
            ["f", "new.txt", "new name.txt", "old name.txt"]
        );
    }

    #[test]
    fn reads_a_plain_diff() {
        let diff = "diff --git a/src/a.rs b/src/a.rs
deleted file mode 100644
index 1111111..0000000
--- a/src/a.rs
+++ /dev/null
@@ -1,2 +0,0 @@
-x
-y
diff --git \"a/caf\\303\\251\" \"b/caf\\303\\251\"
index 1111111..2222222 100644
Binary files a/caf\\303\\251 and b/caf\\303\\251 differ
";
        let summary = parse(diff.as_bytes());
        assert_eq!(summary.kind, Kind::Diff);
        let files = &summary.patches[0].files;
        assert_eq!(files[0].status, FileStatus::Deleted);
        assert_eq!(files[0].path, "src/a.rs");
        assert_eq!(files[0].removed, 2);
        assert_eq!(files[1].path, "café");
        assert!(files[1].binary);
    }

    #[test]
    fn reads_a_unified_diff_without_git_headers() {
        let diff = "--- a/x.c\t2026-10-29 10:00:00\n+++ b/x.c\t2026-10-29 10:01:00\n@@ -1 +1,2 @@\n-a\n+b\n+c\n";
        let summary = parse(diff.as_bytes());
        assert_eq!(summary.kind, Kind::Diff);
        assert_eq!(
            summary.patches[0].files,
            vec![FileChange {
                path: "x.c".into(),
                added: 2,
                removed: 1,
                ..Default::default()
            }]
        );
    }

    #[test]
    fn decodes_encoded_words() {
        assert_eq!(
            decode_words("=?UTF-8?B?44OR44OD44OB?= =?UTF-8?q?=E3=81=A7?="),
            "パッチで"
        );
        assert_eq!(decode_words("a =?iso-8859-1?q?caf=E9?= b"), "a café b");
        assert_eq!(clean_subject("Re: [PATCH 2/3] [RFC] fix it"), "fix it");
    }
}
