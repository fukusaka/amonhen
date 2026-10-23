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
    pub author: String,
    pub time: i64,
    /// そのコミットの時点のパス（名前を変えていれば今と違う）。
    pub path: String,
}

/// `Operations::file_log` の出力（`git log -z --name-only --format=%x01%H%x00%s%x00%an%x00%at` を、
/// 名前を変える前の分もつないだもの）を読む。
pub fn parse_file_log(out: &[u8]) -> Vec<FileLogEntry> {
    let text = String::from_utf8_lossy(out);
    text.split('\u{1}')
        .filter(|r| !r.is_empty())
        .filter_map(|record| {
            let fields: Vec<&str> = record.split('\0').collect();
            let [oid, subject, author, time, path, ..] = fields.as_slice() else {
                return None;
            };
            Some(FileLogEntry {
                oid: (*oid).to_owned(),
                subject: (*subject).to_owned(),
                author: (*author).to_owned(),
                time: time.parse().unwrap_or(0),
                path: path.trim_start_matches('\n').to_owned(),
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
        let out =
            "\u{1}abc\0renamed\0Ann\x00100\0\nnew.txt\0\u{1}def\0created\0Bob\x0050\0\nold.txt\0";
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
    }
}
