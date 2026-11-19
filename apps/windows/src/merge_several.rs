//! いくつものブランチ・タグ・コミットを今のブランチに merge する（GTK4 版の `mergeseveral.rs`、macOS 版の
//! `MergeSeveralSheet.swift`。`git merge a b c`、1 つのコミットにまとめる octopus）。git は最後のもの以外で
//! コンフリクトすると断るので、前もって調べ、そうなりそうなら、ぶつかるものを最後に回すか、1 つずつ merge する
//! ことを勧める。ここには画面によらない部分を置く。

use amonhen_model::{RefKind, tr};
use serde_json::Value;

/// merge で取り込むもの 1 つ。種類は merge のメッセージの見本に使う。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub rev: String,
    pub kind: Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Branch,
    RemoteBranch,
    Tag,
    Commit,
}

/// 順に、コンフリクトする見込みのファイル（git が古くて調べられなければ None）。
pub type Steps = Option<Vec<Vec<String>>>;

/// History で選んだコミット（上から）のうち、今のコミット以外を取り込むものにする。ブランチ・タグが指して
/// いればその名前で（merge のメッセージが git と同じになる。GTK4 版の `sources`）。
pub fn sources(refs: Option<&amonhen_model::Refs>, ids: &[&str]) -> Vec<Source> {
    let head = refs.and_then(|r| r.head).map(|h| h.to_string());
    ids.iter()
        .filter(|id| Some(**id) != head.as_deref())
        .map(|id| {
            let commit = Source {
                rev: (*id).to_owned(),
                kind: Kind::Commit,
            };
            let Some(refs) = refs else {
                return commit;
            };
            // History の行の名前と同じ順（ローカル、リモート、タグ、名前順）の最初のもの。今のブランチは除く。
            let label = refs
                .entries
                .iter()
                .filter(|e| e.target.is_some_and(|t| t.to_string() == *id))
                .filter(|e| matches!(e.kind, RefKind::Local | RefKind::Remote | RefKind::Tag))
                .filter(|e| {
                    !(e.kind == RefKind::Local
                        && Some(e.name.as_str()) == refs.head_branch.as_deref())
                })
                .min_by_key(|e| (e.kind, e.name.clone()));
            match label {
                Some(e) => Source {
                    rev: e.name.clone(),
                    kind: match e.kind {
                        RefKind::Local => Kind::Branch,
                        RefKind::Remote => Kind::RemoteBranch,
                        _ => Kind::Tag,
                    },
                },
                None => commit,
            }
        })
        .collect()
}

/// git が付ける merge のメッセージ（`git fmt-merge-msg` と同じ形）。ブランチ、リモートブランチ、タグの順に
/// 種類ごとにまとめ、コミットは `;` の後に。`main`・`master` 以外へなら ` into <branch>` が付く。
pub fn merge_message(sources: &[Source], branch: Option<&str>) -> String {
    fn joined(names: &[&str]) -> String {
        let quoted: Vec<String> = names.iter().map(|n| format!("'{n}'")).collect();
        match quoted.split_last() {
            Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
            _ => quoted.first().cloned().unwrap_or_default(),
        }
    }
    let names = |kind: Kind| -> Vec<&str> {
        sources
            .iter()
            .filter(|s| s.kind == kind)
            .map(|s| s.rev.as_str())
            .collect()
    };
    let mut groups = Vec::new();
    for (kind, one, many) in [
        (Kind::Branch, "branch", "branches"),
        (
            Kind::RemoteBranch,
            "remote-tracking branch",
            "remote-tracking branches",
        ),
        (Kind::Tag, "tag", "tags"),
    ] {
        let list = names(kind);
        if !list.is_empty() {
            let word = if list.len() == 1 { one } else { many };
            groups.push(format!("{word} {}", joined(&list)));
        }
    }
    let mut text = format!("Merge {}", groups.join(", "));
    let commits = names(Kind::Commit);
    if !commits.is_empty() {
        let word = if commits.len() == 1 {
            "commit"
        } else {
            "commits"
        };
        let sep = if groups.is_empty() { "" } else { "; " };
        text.push_str(&format!("{sep}{word} {}", joined(&commits)));
    }
    if !matches!(branch, Some("main" | "master")) {
        text.push_str(&format!(" into {}", branch.unwrap_or("HEAD")));
    }
    text
}

/// `predictMerge` の答えの、順ごとのコンフリクトするファイル。命令が失敗したら None（git が古くて調べられ
/// なければ Some(None)）。
pub fn steps(reply: &Value) -> Option<Steps> {
    if reply["ok"].as_bool() != Some(true) {
        return None;
    }
    Some(reply["info"]["steps"].as_array().map(|steps| {
        steps
            .iter()
            .map(|s| {
                s["conflicts"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|c| c.as_str().map(str::to_owned))
                    .collect()
            })
            .collect()
    }))
}

/// 見込みの文、コンフリクトがありそうか、最後に回せば済むもの（1 つだけ最後より前でぶつかるとき、その位置）。
/// `answer` が None なら調べられなかった（文は空。GTK4 版の `show` と同じ）。
pub fn forecast(sources: &[Source], answer: Option<&Steps>) -> (String, bool, Option<usize>) {
    let Some(steps) = answer else {
        return (String::new(), false, None);
    };
    let conflicting: Vec<usize> = steps
        .iter()
        .flatten()
        .enumerate()
        .filter(|(_, c)| !c.is_empty())
        .map(|(i, _)| i)
        .collect();
    let early: Vec<usize> = conflicting
        .iter()
        .copied()
        .filter(|&i| i + 1 < sources.len())
        .collect();
    if early.is_empty() {
        let Some(steps) = steps else {
            return (
                tr!("Can’t check for conflicts ahead (needs git 2.38 or newer)."),
                false,
                None,
            );
        };
        let mut all = steps.concat();
        all.sort();
        all.dedup();
        if all.is_empty() {
            return (tr!("No conflicts expected."), false, None);
        }
        let mut names = all.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
        if all.len() > 3 {
            names.push_str(", …");
        }
        return (
            tr!(
                "⚠︎ Conflicts expected in {count} files: {names}",
                count = all.len(),
                names = names
            ),
            true,
            None,
        );
    }
    let names: Vec<&str> = early
        .iter()
        .filter_map(|&i| sources.get(i).map(|s| s.rev.as_str()))
        .collect();
    if conflicting.len() == 1 {
        (
            tr!(
                "⚠︎ {name} conflicts, and git stops when one before the last conflicts. Put it last to resolve the conflicts in this merge, or merge them one by one.",
                name = names.first().copied().unwrap_or_default()
            ),
            true,
            early.first().copied(),
        )
    } else {
        (
            tr!(
                "⚠︎ {names} conflict, and git stops when one before the last conflicts. Merge them one by one instead.",
                names = amonhen_model::i18n::list(&names)
            ),
            true,
            None,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn s(rev: &str, kind: Kind) -> Source {
        Source {
            rev: rev.into(),
            kind,
        }
    }

    #[test]
    fn writes_the_message_as_git_does() {
        let sources = [
            s("a", Kind::Branch),
            s("b", Kind::Branch),
            s("c", Kind::Branch),
        ];
        assert_eq!(
            merge_message(&sources, Some("main")),
            "Merge branches 'a', 'b' and 'c'"
        );
        let sources = [
            s("topic", Kind::Branch),
            s("origin/x", Kind::RemoteBranch),
            s("v1", Kind::Tag),
            s("1234", Kind::Commit),
        ];
        assert_eq!(
            merge_message(&sources, Some("dev")),
            "Merge branch 'topic', remote-tracking branch 'origin/x', tag 'v1'; commit '1234' into dev"
        );
        assert_eq!(
            merge_message(&[s("1", Kind::Commit), s("2", Kind::Commit)], None),
            "Merge commits '1' and '2' into HEAD"
        );
    }

    #[test]
    fn one_early_conflict_can_be_put_last() {
        let sources = [s("a", Kind::Branch), s("b", Kind::Branch)];
        let reply = json!({ "ok": true, "info": { "steps": [
            { "conflicts": ["f"] }, { "conflicts": [] },
        ] } });
        let answer = steps(&reply).expect("答え");
        let (_, conflicts, last) = forecast(&sources, Some(&answer));
        assert!(conflicts);
        assert_eq!(last, Some(0));
        // 最後だけがぶつかるなら、この merge で解ける。
        let reply = json!({ "ok": true, "info": { "steps": [
            { "conflicts": [] }, { "conflicts": ["f"] },
        ] } });
        let answer = steps(&reply).expect("答え");
        let (_, conflicts, last) = forecast(&sources, Some(&answer));
        assert!(conflicts);
        assert_eq!(last, None);
        // 失敗した答え。
        assert!(steps(&json!({ "ok": false })).is_none());
    }
}
