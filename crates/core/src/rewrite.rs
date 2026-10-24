//! History でのコミットの編集を、非対話の rebase の todo に組み立てる。
//!
//! todo はこちらで丸ごと書き、`GIT_SEQUENCE_EDITOR` で git の todo に上書きする。メッセージの
//! 書き換えは、対象のコミットの直後に `exec git commit --amend --file=<file>` を挟んで行う。

use amonhen_model::tr;
use std::path::{Path, PathBuf};

use amonhen_model::{Oid, Result};
use serde::Deserialize;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum HistoryEdit {
    Reword {
        oid: String,
        message: String,
    },
    /// `oid` を親にまとめ、まとめたコミットのメッセージを `message` にする。
    Squash {
        oid: String,
        message: String,
    },
    /// `oid` を親にまとめ、親のメッセージを残す。
    Fixup {
        oid: String,
    },
    /// `oid`（いちばん古い）から `last`（いちばん新しい）までの連続したコミットを 1 つにまとめ、
    /// メッセージを `message` にする。
    SquashRange {
        oid: String,
        last: String,
        message: String,
    },
    Drop {
        oid: String,
    },
    /// `oid` を当てたところで止まる。
    Edit {
        oid: String,
    },
    /// `oid` を取り消して変更を作業ツリーに戻したところで止まる。分けてコミットしてから続ける。
    Split {
        oid: String,
    },
    /// `oid` を、新しい順の並びで `to` 番目に動かす。
    Move {
        oid: String,
        to: usize,
    },
}

impl HistoryEdit {
    pub fn oid(&self) -> &str {
        match self {
            Self::Reword { oid, .. }
            | Self::Squash { oid, .. }
            | Self::Fixup { oid }
            | Self::SquashRange { oid, .. }
            | Self::Drop { oid }
            | Self::Edit { oid }
            | Self::Split { oid }
            | Self::Move { oid, .. } => oid,
        }
    }

    /// 途中で止まる編集か（Undo の記録を rebase が終わるまで保留する）。
    pub fn stops(&self) -> bool {
        matches!(self, Self::Edit { .. } | Self::Split { .. })
    }
}

/// HEAD から first-parent で辿ったコミット 1 つ。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChainCommit {
    pub oid: Oid,
    /// 最初の親。root なら `None`。
    pub parent: Option<Oid>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct RebasePlan {
    /// rebase の起点。`None` なら `--root`。
    pub base: Option<Oid>,
    pub todo: String,
    /// 書き出しておくメッセージ（ファイル名, 内容）。
    pub messages: Vec<(String, String)>,
}

/// sh に渡すパス。git は exec や `GIT_SEQUENCE_EDITOR` を sh で実行するので、Windows でも `/` 区切りにする
/// （Git for Windows の sh は `C:/...` を受け付ける）。
pub fn shell_path(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// `chain` は HEAD から first-parent で辿ったコミット（新しい順）で、間に merge がないこと。
/// `message_dir` はメッセージのファイルを置くディレクトリ（exec に絶対パスで渡す）。
/// `branches` は、編集の範囲にあるかもしれない、積み重ねたブランチの先頭と完全な名前。範囲にあるものは
/// todo に `update-ref` を入れる（先頭のコミットについて行く。消したコミットなら、その前のコミットへ）。
/// `rebase.updateRefs` が true でなければ、実行するときに除く（`Operations::rebase_todo`）。
pub fn plan_rebase(
    chain: &[ChainCommit],
    edit: &HistoryEdit,
    message_dir: &Path,
    branches: &[(Oid, String)],
) -> Result<RebasePlan> {
    // 古い順にする。
    let original: Vec<ChainCommit> = chain.iter().rev().copied().collect();
    let mut seq = original.clone();
    let target = Oid::from_hex(edit.oid()).ok_or_else(|| tr!("Can’t read the commit ID"))?;
    let index = seq.iter().position(|c| c.oid == target).ok_or_else(|| {
        tr!("This commit can’t be edited (it isn’t reachable from HEAD, or it’s before a merge)")
    })?;

    let mut start = index;
    // まとめる範囲の終わり（SquashRange のときだけ）。
    let mut range_end = index;
    match edit {
        HistoryEdit::SquashRange { last, .. } => {
            let last = Oid::from_hex(last).ok_or_else(|| tr!("Can’t read the commit ID"))?;
            range_end = seq.iter().position(|c| c.oid == last).ok_or_else(|| {
                tr!("This commit can’t be edited (it isn’t reachable from HEAD, or it’s before a merge)")
            })?;
            if range_end <= index {
                return Err(tr!("To squash, select two or more consecutive commits").into());
            }
        }
        HistoryEdit::Squash { .. } | HistoryEdit::Fixup { .. } => {
            if index == 0 {
                return Err(tr!("Can’t squash: the parent isn’t in the editable range").into());
            }
            start = index - 1;
        }
        HistoryEdit::Split { .. } if seq[index].parent.is_none() => {
            return Err(tr!("Can’t split the first commit").into());
        }
        HistoryEdit::Move { to, .. } => {
            let to = seq
                .len()
                .checked_sub(1 + to)
                .ok_or_else(|| tr!("The destination is outside the editable range"))?;
            let moved = seq.remove(index);
            seq.insert(to, moved);
            start = index.min(to);
        }
        _ => {}
    }

    // 起点は、元の並びで start より前にあるコミット。先頭まで含むなら、そのさらに親（なければ --root）。
    let base = if start > 0 {
        Some(original[start - 1].oid)
    } else {
        original[0].parent
    };

    let mut messages = Vec::new();
    let mut lines = Vec::new();
    let mut amend = |lines: &mut Vec<String>, oid: &Oid, message: &str| {
        let name = format!("message-{oid}.txt");
        let path: PathBuf = message_dir.join(&name);
        messages.push((name, message.to_owned()));
        lines.push(format!(
            "exec git commit --amend --allow-empty --no-verify --file={}",
            shell_quote(&shell_path(&path))
        ));
    };
    for (i, c) in seq.iter().enumerate().skip(start) {
        if let HistoryEdit::SquashRange { message, .. } = edit {
            if i == index {
                lines.push(format!("pick {}", c.oid));
            } else if i <= range_end {
                lines.push(format!("fixup {}", c.oid));
                if i == range_end {
                    amend(&mut lines, &c.oid, message);
                }
            } else {
                lines.push(format!("pick {}", c.oid));
            }
            continue;
        }
        if c.oid != target {
            lines.push(format!("pick {}", c.oid));
            continue;
        }
        match edit {
            HistoryEdit::Reword { message, .. } => {
                lines.push(format!("pick {}", c.oid));
                amend(&mut lines, &c.oid, message);
            }
            HistoryEdit::Squash { message, .. } => {
                lines.push(format!("fixup {}", c.oid));
                amend(&mut lines, &c.oid, message);
            }
            HistoryEdit::Fixup { .. } => lines.push(format!("fixup {}", c.oid)),
            HistoryEdit::SquashRange { .. } => unreachable!(),
            HistoryEdit::Drop { .. } => lines.push(format!("drop {}", c.oid)),
            HistoryEdit::Edit { .. } => lines.extend([format!("pick {}", c.oid), "break".into()]),
            HistoryEdit::Split { .. } => lines.extend([
                format!("pick {}", c.oid),
                "exec git reset --quiet HEAD^".into(),
                "break".into(),
            ]),
            HistoryEdit::Move { .. } => lines.push(format!("pick {}", c.oid)),
        }
    }
    let lines = with_update_refs(lines, branches);
    let mut todo = lines.join("\n");
    todo.push('\n');
    Ok(RebasePlan {
        base,
        todo,
        messages,
    })
}

/// 先頭が `pick`・`fixup`・`drop` したコミットのブランチの `update-ref` を、そのコミットのまとまり（続く fixup・
/// exec・break）の後、次の `pick`・`drop` の前に入れる。
fn with_update_refs(lines: Vec<String>, branches: &[(Oid, String)]) -> Vec<String> {
    let mut out = Vec::with_capacity(lines.len() + branches.len());
    let mut pending: Vec<&str> = Vec::new();
    for line in lines {
        let mut words = line.split(' ');
        let command = words.next().unwrap_or_default();
        if matches!(command, "pick" | "drop") {
            out.extend(pending.drain(..).map(|r| format!("update-ref {r}")));
        }
        if matches!(command, "pick" | "fixup" | "drop")
            && let Some(oid) = words.next().and_then(Oid::from_hex)
        {
            pending.extend(
                branches
                    .iter()
                    .filter(|(tip, _)| *tip == oid)
                    .map(|(_, name)| name.as_str()),
            );
        }
        out.push(line);
    }
    out.extend(pending.drain(..).map(|r| format!("update-ref {r}")));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oid(c: char) -> Oid {
        Oid::from_hex(&c.to_string().repeat(40)).unwrap()
    }

    fn hex(c: char) -> String {
        c.to_string().repeat(40)
    }

    fn c(o: char, parent: Option<char>) -> ChainCommit {
        ChainCommit {
            oid: oid(o),
            parent: parent.map(oid),
        }
    }

    /// 新しい順: d → c → b → a（a は root）。16 進数の文字で ID を作る。
    fn chain() -> Vec<ChainCommit> {
        vec![
            c('d', Some('c')),
            c('c', Some('b')),
            c('b', Some('a')),
            c('a', None),
        ]
    }

    fn plan(edit: HistoryEdit) -> Result<RebasePlan> {
        plan_rebase(&chain(), &edit, Path::new("/g"), &[])
    }

    /// todo の中の 40 桁の ID を 1 文字に縮めて読みやすくする。
    fn short(todo: &str) -> String {
        let mut s = todo.to_owned();
        for ch in ['a', 'b', 'c', 'd'] {
            s = s.replace(&hex(ch), &ch.to_string());
        }
        s
    }

    #[test]
    fn stacked_branches_follow_their_tips() {
        let branches = [
            (oid('b'), "refs/heads/one".to_owned()),
            (oid('c'), "refs/heads/two".to_owned()),
            // 編集の範囲より前は動かないので、todo に入れない。
            (oid('a'), "refs/heads/zero".to_owned()),
        ];
        let plan = |edit| plan_rebase(&chain(), &edit, Path::new("/g"), &branches).unwrap();
        // 消したコミットのブランチは、その前のコミットへ。
        assert_eq!(
            short(&plan(HistoryEdit::Drop { oid: hex('b') }).todo),
            "drop b\nupdate-ref refs/heads/one\npick c\nupdate-ref refs/heads/two\npick d\n"
        );
        // まとめたコミットのブランチは、まとめた後（メッセージを直した後）のコミットへ。
        let todo = short(
            &plan(HistoryEdit::Squash {
                oid: hex('c'),
                message: "b+c".into(),
            })
            .todo,
        );
        assert!(
            todo.ends_with(
                "message-c.txt'\nupdate-ref refs/heads/one\nupdate-ref refs/heads/two\npick d\n"
            ),
            "{todo}"
        );
    }

    #[test]
    fn reword_starts_at_the_commit_and_amends_right_after_it() {
        let p = plan(HistoryEdit::Reword {
            oid: hex('c'),
            message: "new".into(),
        })
        .unwrap();
        assert_eq!(p.base, Some(oid('b')));
        assert_eq!(
            short(&p.todo),
            "pick c\nexec git commit --amend --allow-empty --no-verify --file='/g/message-c.txt'\npick d\n"
        );
        assert_eq!(short(&p.messages[0].0), "message-c.txt");
        assert_eq!(p.messages[0].1, "new");
    }

    #[test]
    fn squash_and_fixup_start_at_the_parent() {
        assert_eq!(
            short(&plan(HistoryEdit::Fixup { oid: hex('c') }).unwrap().todo),
            "pick b\nfixup c\npick d\n"
        );
        let squash = plan(HistoryEdit::Squash {
            oid: hex('c'),
            message: "b+c".into(),
        })
        .unwrap();
        assert_eq!(squash.base, Some(oid('a')));
        let todo = short(&squash.todo);
        assert!(
            todo.starts_with("pick b\nfixup c\nexec git commit --amend "),
            "{todo}"
        );
        assert!(todo.ends_with("message-c.txt'\npick d\n"), "{todo}");
    }

    #[test]
    fn squash_range_picks_the_oldest_and_fixes_up_the_rest() {
        let p = plan(HistoryEdit::SquashRange {
            oid: hex('b'),
            last: hex('c'),
            message: "b..c".into(),
        })
        .unwrap();
        assert_eq!(p.base, Some(oid('a')));
        let todo = short(&p.todo);
        assert!(
            todo.starts_with("pick b\nfixup c\nexec git commit --amend "),
            "{todo}"
        );
        assert!(todo.ends_with("message-c.txt'\npick d\n"), "{todo}");
        assert_eq!(p.messages[0].1, "b..c");
        // 新しい側を先に渡したら断る。
        assert!(
            plan(HistoryEdit::SquashRange {
                oid: hex('c'),
                last: hex('b'),
                message: String::new(),
            })
            .is_err()
        );
    }

    #[test]
    fn drop_edit_and_split() {
        assert_eq!(
            short(&plan(HistoryEdit::Drop { oid: hex('d') }).unwrap().todo),
            "drop d\n"
        );
        assert_eq!(
            short(&plan(HistoryEdit::Edit { oid: hex('c') }).unwrap().todo),
            "pick c\nbreak\npick d\n"
        );
        assert_eq!(
            short(&plan(HistoryEdit::Split { oid: hex('c') }).unwrap().todo),
            "pick c\nexec git reset --quiet HEAD^\nbreak\npick d\n"
        );
    }

    #[test]
    fn editing_the_root_commit_uses_root() {
        assert_eq!(
            plan(HistoryEdit::Reword {
                oid: hex('a'),
                message: "x".into()
            })
            .unwrap()
            .base,
            None
        );
        assert!(plan(HistoryEdit::Split { oid: hex('a') }).is_err());
        assert!(plan(HistoryEdit::Fixup { oid: hex('a') }).is_err());
    }

    #[test]
    fn the_chain_may_end_at_a_merge_whose_parent_becomes_the_base() {
        let partial = [c('d', Some('c')), c('c', Some('e'))];
        let p = plan_rebase(
            &partial,
            &HistoryEdit::Drop { oid: hex('c') },
            Path::new("/g"),
            &[],
        )
        .unwrap();
        assert_eq!(p.base, Some(oid('e')));
    }

    #[test]
    fn move_reorders_from_the_earlier_of_the_two_positions() {
        // d を、新しい順で 2 番目（b の位置）へ。
        let p = plan(HistoryEdit::Move {
            oid: hex('d'),
            to: 2,
        })
        .unwrap();
        assert_eq!(p.base, Some(oid('a')));
        assert_eq!(short(&p.todo), "pick d\npick b\npick c\n");
        // b を一番上へ。
        assert_eq!(
            short(
                &plan(HistoryEdit::Move {
                    oid: hex('b'),
                    to: 0
                })
                .unwrap()
                .todo
            ),
            "pick c\npick d\npick b\n"
        );
        assert!(
            plan(HistoryEdit::Move {
                oid: hex('b'),
                to: 9
            })
            .is_err()
        );
    }

    #[test]
    fn commits_outside_the_chain_are_rejected() {
        assert!(plan(HistoryEdit::Drop { oid: hex('f') }).is_err());
    }

    #[test]
    fn quotes_for_the_shell() {
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }
}
