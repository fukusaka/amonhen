//! blame とファイルの履歴（git の出力を読む）。

use std::collections::HashMap;

use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BlameCommit {
    /// 40 桁の ID。まだコミットしていない行は 0 が並ぶ。
    pub oid: String,
    pub author: String,
    pub time: i64,
    pub summary: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BlameLine {
    /// `Blame::commits` の添字。
    #[serde(rename = "c")]
    pub commit: usize,
    #[serde(rename = "t")]
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Blame {
    pub commits: Vec<BlameCommit>,
    pub lines: Vec<BlameLine>,
}

/// `git blame --porcelain` を読む。同じコミットの 2 行目以降は、ヘッダーと内容の行だけになる。
pub fn parse_blame(out: &[u8]) -> Blame {
    let text = String::from_utf8_lossy(out);
    let mut blame = Blame::default();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut current: Option<usize> = None;
    for line in text.lines() {
        if let Some(content) = line.strip_prefix('\t') {
            if let Some(c) = current {
                blame.lines.push(BlameLine {
                    commit: c,
                    text: content.to_owned(),
                });
            }
            continue;
        }
        let mut words = line.split(' ');
        let first = words.next().unwrap_or_default();
        if first.len() == 40 && first.bytes().all(|b| b.is_ascii_hexdigit()) {
            let next = blame.commits.len();
            let c = *index.entry(first.to_owned()).or_insert(next);
            if c == next {
                blame.commits.push(BlameCommit {
                    oid: first.to_owned(),
                    author: String::new(),
                    time: 0,
                    summary: String::new(),
                });
            }
            current = Some(c);
            continue;
        }
        let Some(c) = current else { continue };
        let value = line.split_once(' ').map_or("", |(_, v)| v);
        let commit = &mut blame.commits[c];
        match first {
            "author" => commit.author = value.to_owned(),
            "author-time" => commit.time = value.parse().unwrap_or(0),
            "summary" => commit.summary = value.to_owned(),
            _ => {}
        }
    }
    blame
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FileLogEntry {
    pub oid: String,
    pub subject: String,
    /// 作者の名前とメール（mailmap があれば置き換えたもの、`apply_mailmap`）。
    pub author: String,
    #[serde(rename = "authorEmail")]
    pub email: String,
    pub time: i64,
    /// そのコミットの時点のパス（名前を変えていれば今と違う）。
    pub path: String,
    /// mailmap で置き換えて記録と違えば、記録された作者（name・email）。
    #[serde(rename = "recordedAuthor", skip_serializing_if = "Option::is_none")]
    pub recorded: Option<RecordedIdentity>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RecordedIdentity {
    pub name: String,
    pub email: String,
}

impl FileLogEntry {
    /// 作者を mailmap で置き換える（記録と違えば `recorded` に記録を残す）。
    pub fn apply_mailmap(&mut self, mailmap: &amonhen_model::Mailmap) {
        let (name, email) = mailmap.map_str(&self.author, &self.email);
        if name != self.author || email != self.email {
            self.recorded = Some(RecordedIdentity {
                name: std::mem::replace(&mut self.author, name),
                email: std::mem::replace(&mut self.email, email),
            });
        }
    }
}

/// `Operations::file_log` の出力（`git log -z --name-only --format=%x01%H%x00%s%x00%an%x00%ae%x00%at` を、
/// 名前を変える前の分もつないだもの）を読む。名前とメールは記録のまま（`git log` の `%an` は mailmap を見ない）。
pub fn parse_file_log(out: &[u8]) -> Vec<FileLogEntry> {
    let text = String::from_utf8_lossy(out);
    text.split('\u{1}')
        .filter(|r| !r.is_empty())
        .filter_map(|record| {
            let fields: Vec<&str> = record.split('\0').collect();
            let [oid, subject, author, email, time, path, ..] = fields.as_slice() else {
                return None;
            };
            Some(FileLogEntry {
                oid: (*oid).to_owned(),
                subject: (*subject).to_owned(),
                author: (*author).to_owned(),
                email: (*email).to_owned(),
                time: time.parse().unwrap_or(0),
                path: path.trim_start_matches('\n').to_owned(),
                recorded: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_porcelain_blame_sharing_commit_details() {
        let a = "a".repeat(40);
        let b = "b".repeat(40);
        let out = format!(
            "{a} 1 1 2\nauthor Ann\nauthor-time 100\nsummary first\nfilename x\n\tline 1\n{a} 2 2\n\tline 2\n\
             {b} 3 3 1\nauthor Bob\nauthor-time 200\nsummary second\nprevious {a} x\nfilename x\n\t\n"
        );
        let blame = parse_blame(out.as_bytes());
        assert_eq!(blame.commits.len(), 2);
        assert_eq!(
            (blame.commits[0].author.as_str(), blame.commits[0].time),
            ("Ann", 100)
        );
        assert_eq!(blame.commits[1].summary, "second");
        let lines: Vec<(usize, &str)> = blame
            .lines
            .iter()
            .map(|l| (l.commit, l.text.as_str()))
            .collect();
        assert_eq!(lines, [(0, "line 1"), (0, "line 2"), (1, "")]);
    }

    #[test]
    fn reads_the_followed_file_log() {
        let out = "\u{1}abc\0renamed\0Ann\0ann@example.com\x00100\0\nnew.txt\0\u{1}def\0created\0Bob\0b@example.com\x0050\0\nold.txt\0";
        let log = parse_file_log(out.as_bytes());
        assert_eq!(log.len(), 2);
        assert_eq!(
            (log[0].oid.as_str(), log[0].path.as_str()),
            ("abc", "new.txt")
        );
        assert_eq!(
            (log[1].subject.as_str(), log[1].time, log[1].path.as_str()),
            ("created", 50, "old.txt")
        );
        assert_eq!(log[1].email, "b@example.com");
    }

    #[test]
    fn maps_the_file_log_author_and_keeps_the_recorded_one() {
        let mailmap = amonhen_model::Mailmap::new(
            true,
            vec![amonhen_model::MailmapEntry {
                old_email: "b@example.com".into(),
                old_name: None,
                new_name: Some("Robert".into()),
                new_email: Some("robert@example.com".into()),
            }],
        );
        let out = "\u{1}abc\0s\0Ann\0ann@example.com\x00100\0\na\0\u{1}def\0s\0Bob\0b@example.com\x0050\0\na\0";
        let mut log = parse_file_log(out.as_bytes());
        for e in &mut log {
            e.apply_mailmap(&mailmap);
        }
        assert_eq!(
            (log[0].author.as_str(), log[0].recorded.is_none()),
            ("Ann", true)
        );
        assert_eq!(
            (log[1].author.as_str(), log[1].email.as_str()),
            ("Robert", "robert@example.com")
        );
        let json = serde_json::to_value(&log[1]).unwrap();
        assert_eq!(json["recordedAuthor"]["name"], "Bob");
        assert_eq!(json["authorEmail"], "robert@example.com");
    }
}
