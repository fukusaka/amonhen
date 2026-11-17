//! ブランチの切り替え（Switch Branch…、Ctrl+Shift+B。GTK4 版の `branchswitcher.rs`、macOS 版の
//! `BranchSwitcher.swift`）: ブランチをあいまい検索して切り替える。最近切り替えたものを上に出す。リモートの
//! ブランチは、追跡するローカルブランチを作って切り替える。一致するものがなければ、打った名前で新しいブランチを
//! 作れる。

use std::collections::{HashMap, HashSet};

use amonhen_model::{RefKind, Refs, tr};
use serde_json::{Value, json};

/// 最近切り替えたブランチの記録（場所 → ブランチの名前、新しい順）。GTK4 版・macOS 版と同じ鍵。
const RECENT: &str = "recentBranches";
/// 覚えておく数。
const KEEP: usize = 10;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    Local(String),
    /// リモートのブランチ（追跡するローカルブランチを作って切り替える）。
    Remote(String),
    /// 打った名前で新しいブランチを作って切り替える。
    Create(String),
}

impl Choice {
    /// 一覧の行の文字。
    pub fn label(&self, recent: &[String]) -> String {
        match self {
            Choice::Local(name) if recent.contains(name) => format!("{name}{}", tr!("  · recent")),
            Choice::Local(name) => name.clone(),
            Choice::Remote(name) => format!("{name}{}", tr!("  · creates a tracking branch")),
            Choice::Create(name) => tr!("Create branch {name} and switch to it", name = name),
        }
    }

    /// 選んだときの命令（選んだこと自体を確かめとし、確認は出さない。GTK4 版と同じ）。
    pub fn command(&self) -> Value {
        match self {
            Choice::Local(name) => json!({ "op": "checkout", "branch": name }),
            Choice::Remote(name) => json!({ "op": "track", "remoteBranch": name }),
            Choice::Create(name) => {
                json!({ "op": "createBranch", "name": name, "start": "HEAD", "checkout": true })
            }
        }
    }
}

fn all_recent() -> serde_json::Map<String, Value> {
    crate::prefs::string(RECENT)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// `location` のリポジトリで最近切り替えたブランチ（新しい順）。
pub fn recent(location: &str) -> Vec<String> {
    all_recent()
        .get(location)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// 今のブランチを覚える（ref を読むたびに。先頭と同じなら何もしない）。
pub fn remember(location: &str, branch: &str) {
    let mut list = recent(location);
    if list.first().map(String::as_str) == Some(branch) {
        return;
    }
    list.retain(|b| b != branch);
    list.insert(0, branch.to_owned());
    list.truncate(KEEP);
    let mut all = all_recent();
    all.insert(location.to_owned(), json!(list));
    let _ = crate::prefs::set_string(RECENT, &Value::Object(all).to_string());
}

/// `query` の文字を `text` の中に順に（飛び飛びでよい）見つけられれば、点数（続いているほど・先頭や区切りの直後
/// ほど高い）。見つからなければ None（GTK4 版の `quickopen::score`。Quick Open も使う）。
pub fn score(query: &str, text: &str) -> Option<i64> {
    let t: Vec<char> = text.chars().collect();
    let (mut score, mut ti, mut previous) = (0i64, 0usize, -2i64);
    for c in query.chars() {
        while ti < t.len() && t[ti] != c {
            ti += 1;
        }
        if ti >= t.len() {
            return None;
        }
        if ti as i64 == previous + 1 {
            score += 5;
        }
        if ti == 0 || "/\\-_. ".contains(t[ti - 1]) {
            score += 8;
        }
        previous = ti as i64;
        ti += 1;
    }
    // 短いものほど少し高く。
    Some(score * 100 - t.len() as i64)
}

/// 問い合わせ `query` に合う候補を、出す順に。最近切り替えたものを先に、残りは名前の順。問い合わせがあれば
/// 合い方の良い順（ローカルのブランチを少し上に）。ちょうど同じ名前のローカルブランチがなく、ブランチの名前として
/// 使えれば、最後に作る行を足す。
pub fn candidates(refs: &Refs, recent: &[String], query: &str) -> Vec<Choice> {
    let query = query.trim();
    let lower = query.to_lowercase();
    let head = refs.head_branch.as_deref();
    let locals: Vec<&str> = refs
        .entries
        .iter()
        .filter(|e| e.kind == RefKind::Local && Some(e.name.as_str()) != head)
        .map(|e| e.name.as_str())
        .collect();
    let local_set: HashSet<&str> = locals.iter().copied().chain(head).collect();
    // ローカルにもう同じ名前があるリモートのブランチ（origin/main など）は出さない。
    let remotes: Vec<&str> = refs
        .entries
        .iter()
        .filter(|e| e.kind == RefKind::Remote && !e.name.ends_with("/HEAD"))
        .filter(|e| {
            let short = e.name.split_once('/').map_or(e.name.as_str(), |(_, b)| b);
            !local_set.contains(short)
        })
        .map(|e| e.name.as_str())
        .collect();
    let rank: HashMap<&str, usize> = recent
        .iter()
        .enumerate()
        .rev()
        .map(|(i, b)| (b.as_str(), i))
        .collect();
    let mut ordered = locals.clone();
    ordered.sort_by_key(|n| (rank.get(n).copied().unwrap_or(usize::MAX), *n));
    let mut list: Vec<(Choice, &str, i64)> = ordered
        .iter()
        .enumerate()
        .map(|(i, n)| (Choice::Local((*n).to_owned()), *n, 2000 - i as i64))
        .chain(
            remotes
                .iter()
                .enumerate()
                .map(|(i, n)| (Choice::Remote((*n).to_owned()), *n, 1000 - i as i64)),
        )
        .collect();
    if !lower.is_empty() {
        list = list
            .into_iter()
            .filter_map(|(c, name, base)| {
                let bonus = if base > 1000 { 500 } else { 0 };
                score(&lower, &name.to_lowercase()).map(|s| (c, name, s + bonus))
            })
            .collect();
        list.sort_by_key(|(_, _, s)| -s);
    }
    let mut shown: Vec<Choice> = list.into_iter().map(|(c, _, _)| c).collect();
    if !query.is_empty()
        && !local_set.contains(query)
        && !query.contains(' ')
        && !query.starts_with('-')
    {
        shown.push(Choice::Create(query.to_owned()));
    }
    shown
}

#[cfg(test)]
mod tests {
    use super::*;
    use amonhen_model::RefEntry;

    fn entry(kind: RefKind, name: &str) -> RefEntry {
        RefEntry {
            kind,
            name: name.to_owned(),
            target: None,
            detail: None,
            ahead: 0,
            behind: 0,
            gone: false,
        }
    }

    fn refs() -> Refs {
        Refs {
            head_branch: Some("main".to_owned()),
            entries: vec![
                entry(RefKind::Local, "main"),
                entry(RefKind::Local, "alpha"),
                entry(RefKind::Local, "feature/login"),
                entry(RefKind::Remote, "origin/main"),
                entry(RefKind::Remote, "origin/topic"),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn recent_first_then_names_then_remotes() {
        let shown = candidates(&refs(), &["feature/login".to_owned()], "");
        assert_eq!(
            shown,
            [
                Choice::Local("feature/login".into()),
                Choice::Local("alpha".into()),
                Choice::Remote("origin/topic".into()),
            ]
        );
    }

    #[test]
    fn query_filters_and_offers_a_new_branch() {
        let shown = candidates(&refs(), &[], "log");
        assert_eq!(
            shown,
            [
                Choice::Local("feature/login".into()),
                Choice::Create("log".into())
            ]
        );
    }
}
