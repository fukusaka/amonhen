//! FE から受け取る操作。頻度の低い操作はまとめてここを通し、Undo の記録もここで行う。

use amonhen_model::{Oid, Outcome, Result};
use serde::{Deserialize, Serialize};

use crate::Repository;
use crate::rewrite::{self, ChainCommit, HistoryEdit};

/// 編集できる範囲を調べるときに辿るコミットの数。
const CHAIN_LIMIT: usize = 500;

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Command {
    Checkout {
        branch: String,
    },
    /// リモートブランチ（`origin/main`）を追跡するローカルブランチを作って切り替える。
    Track {
        remote_branch: String,
    },
    /// コミットを detached HEAD でチェックアウトする。
    Detach {
        oid: String,
    },
    CreateBranch {
        name: String,
        start: String,
        checkout: bool,
    },
    DeleteBranch {
        name: String,
        force: bool,
    },
    RenameBranch {
        from: String,
        to: String,
    },
    CreateTag {
        name: String,
        target: String,
        message: Option<String>,
    },
    /// 今のブランチに `rev` を merge する。
    Merge {
        rev: String,
    },
    /// ブランチ `target` に `rev` を merge する。`target` を checkout していなくても、fast-forward で
    /// 済むならブランチだけ進める。そうでなければ checkout してから merge する。
    MergeInto {
        target: String,
        rev: String,
    },
    /// 今のブランチを `onto` の上に rebase する。
    Rebase {
        onto: String,
    },
    CherryPick {
        oid: String,
        is_merge: bool,
    },
    Revert {
        oid: String,
        is_merge: bool,
    },
    Abort,
    Continue,
    Skip,
    Fetch,
    /// 今のブランチに上流を取り込む（Undo できる）。
    Pull,
    Push {
        remote: String,
        branch: String,
        remote_branch: String,
        force: bool,
        set_upstream: bool,
    },
    /// 作業ツリーのファイルを index の内容に戻す（Undo できる）。
    Discard {
        paths: Vec<String>,
    },
    StashPush {
        message: Option<String>,
        include_untracked: bool,
    },
    StashApply {
        name: String,
        pop: bool,
    },
    /// stash を消す（Undo できる）。
    StashDrop {
        name: String,
    },
    UpdateSubmodules {
        paths: Vec<String>,
    },
    Undo,
    /// 次に Undo される操作の名前。
    UndoPeek,
    /// コミットの編集（reword・squash・fixup・drop・edit・split・move）。
    Rewrite {
        label: String,
        edit: HistoryEdit,
    },
    /// コミットのメッセージ全文（`text`）。
    CommitMessage {
        oid: String,
    },
    /// 途中の操作（`text` に "rebase"・"merge"・"cherry-pick"・"revert"、なければなし）。
    Sequence,
    /// 編集できるコミット（HEAD から first-parent で最初の merge の手前まで、新しい順）。`text` に改行区切り。
    EditableCommits,
    /// `oid` を親にまとめるときの、メッセージの初期値（親のメッセージ ＋ `oid` のメッセージ）。
    SquashMessage {
        oid: String,
    },
    /// `path` の `index` 番目のコンフリクトを解決する（Undo できる）。
    ResolveConflict {
        path: String,
        index: usize,
        choice: crate::conflict::Choice,
    },
    /// コミットの詳細（`info`: message・author・authorEmail・authorTime・committer・committerTime・parents・refs）。
    CommitInfo {
        oid: String,
    },
    /// `path` を片側にして解決済みにする。
    TakeSide {
        path: String,
        ours: bool,
    },
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    pub ok: bool,
    /// "done"・"conflict"・"stopped"。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<&'static str>,
    /// Undo した・次に Undo される操作の名前。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub info: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

fn outcome_name(o: Outcome) -> &'static str {
    match o {
        Outcome::Done => "done",
        Outcome::Conflict => "conflict",
        Outcome::Stopped => "stopped",
    }
}

fn oid(hex: &str) -> Result<Oid> {
    Oid::from_hex(hex).ok_or_else(|| format!("コミット ID を読めない: {hex}").into())
}

fn heads(name: &str) -> String {
    format!("refs/heads/{name}")
}

impl Repository {
    /// コミットのメッセージ全文。
    pub fn commit_message(&self, commit: &Oid) -> Result<String> {
        let text = self
            .backend()
            .commit_texts(std::slice::from_ref(commit))?
            .remove(0);
        // ヘッダーの後の最初の空行から後ろがメッセージ。
        let raw = &text.raw[..];
        let start = raw
            .windows(2)
            .position(|w| w == b"\n\n")
            .map_or(raw.len(), |i| i + 2);
        Ok(String::from_utf8_lossy(&raw[start..]).into_owned())
    }

    /// コミットの詳細。ヘッダーの行（parent・author・committer）とメッセージ、そのコミットを指す ref。
    pub fn commit_info(&self, commit: &Oid) -> Result<serde_json::Value> {
        let text = self
            .backend()
            .commit_texts(std::slice::from_ref(commit))?
            .remove(0);
        let raw = String::from_utf8_lossy(&text.raw).into_owned();
        let (header, message) = raw.split_once("\n\n").unwrap_or((&raw, ""));
        // "Name <email> 1700000000 +0900" を名前・メール・時刻に分ける。
        let signature = |line: &str| -> (String, String, i64) {
            let (name, rest) = line.split_once(" <").unwrap_or((line, ""));
            let (email, rest) = rest.split_once("> ").unwrap_or((rest, ""));
            let time = rest
                .split(' ')
                .next()
                .and_then(|t| t.parse().ok())
                .unwrap_or(0);
            (name.to_owned(), email.to_owned(), time)
        };
        let mut parents = Vec::new();
        let (mut author, mut committer) = (None, None);
        for line in header.lines() {
            if let Some(p) = line.strip_prefix("parent ") {
                parents.push(p.to_owned());
            } else if let Some(a) = line.strip_prefix("author ") {
                author = Some(signature(a));
            } else if let Some(c) = line.strip_prefix("committer ") {
                committer = Some(signature(c));
            }
        }
        let (author, email, author_time) = author.unwrap_or_default();
        let (committer, _, committer_time) = committer.unwrap_or_default();
        let refs: Vec<String> = self
            .load_refs()?
            .refs
            .entries
            .iter()
            .filter(|e| {
                e.target.as_ref() == Some(commit) && e.kind != amonhen_model::RefKind::Stash
            })
            .map(|e| e.name.clone())
            .collect();
        Ok(serde_json::json!({
            "oid": commit.to_string(),
            "message": message,
            "author": author,
            "authorEmail": email,
            "authorTime": author_time,
            "committer": committer,
            "committerTime": committer_time,
            "parents": parents,
            "refs": refs,
        }))
    }

    /// HEAD から first-parent で辿り、最初の merge の手前まで（新しい順）。
    fn editable_chain(&self) -> Result<Vec<ChainCommit>> {
        let history = match self.history() {
            Some(h) => h,
            None => self.load_history()?,
        };
        let head = self.backend().head()?.ok_or("まだコミットがありません")?;
        let mut row = history
            .row_of(&head)
            .ok_or("HEAD が History にない（読み直してください）")? as usize;
        let graph = &history.graph;
        let mut chain = Vec::new();
        while chain.len() < CHAIN_LIMIT {
            let parents = graph.parents(row);
            if parents.len() > 1 {
                break;
            }
            let parent = parents.first().map(|&p| graph.ids()[p as usize]);
            chain.push(ChainCommit {
                oid: graph.ids()[row],
                parent,
            });
            match parents.first() {
                Some(&p) => row = p as usize,
                None => break,
            }
        }
        Ok(chain)
    }

    /// コミットを編集する。非対話の rebase で行い、Undo できるように記録する。
    pub fn rewrite(&self, label: &str, edit: &HistoryEdit) -> Result<Outcome> {
        let ops = self.backend().operations()?;
        if ops.sequence_in_progress()?.is_some() {
            return Err("merge や rebase の途中です。先に終わらせるか中止してください".into());
        }
        // 書き換える前に、最新の History で範囲を決める。
        self.load_history()?;
        let chain = self.editable_chain()?;
        let dir = self.backend().paths().git_dir.join("amonhen");
        let plan = rewrite::plan_rebase(&chain, edit, &dir)?;
        std::fs::create_dir_all(&dir)?;
        for (name, message) in &plan.messages {
            std::fs::write(dir.join(name), message)?;
        }
        let todo = dir.join("rebase-todo");
        std::fs::write(&todo, &plan.todo)?;
        self.undo_log()?.record_stoppable(
            ops,
            label,
            || ops.rebase_todo(plan.base.as_ref(), &todo),
            |o| *o == Outcome::Conflict,
            |o| *o == Outcome::Stopped,
        )
    }

    /// JSON の命令を実行し、JSON の結果を返す。
    pub fn run_json(&self, json: &str) -> String {
        let response = match serde_json::from_str::<Command>(json) {
            Ok(command) => match self.run(command) {
                Ok(r) => r,
                Err(e) => Response {
                    error: Some(e.to_string()),
                    ..Default::default()
                },
            },
            Err(e) => Response {
                error: Some(format!("命令を読めない: {e}")),
                ..Default::default()
            },
        };
        serde_json::to_string(&response).unwrap_or_default()
    }

    pub fn run(&self, command: Command) -> Result<Response> {
        let ops = self.backend().operations()?;
        let undo = self.undo_log()?;
        let not_done = |o: &Outcome| *o != Outcome::Done;
        let done = |outcome: Outcome| {
            Ok(Response {
                ok: true,
                outcome: Some(outcome_name(outcome)),
                ..Default::default()
            })
        };
        let ok = || done(Outcome::Done);
        match command {
            Command::Checkout { branch } => {
                ops.switch_branch(&branch)?;
                ok()
            }
            Command::Track { remote_branch } => {
                ops.track_branch(&remote_branch)?;
                ok()
            }
            Command::Detach { oid: o } => {
                ops.switch_to(&amonhen_model::HeadState {
                    branch: None,
                    oid: Some(oid(&o)?),
                })?;
                ok()
            }
            Command::CreateBranch {
                name,
                start,
                checkout,
            } => {
                undo.record(
                    ops,
                    &format!("Create branch {name}"),
                    &[heads(&name)],
                    false,
                    || ops.create_branch(&name, &start, checkout),
                    |_| false,
                )?;
                ok()
            }
            Command::DeleteBranch { name, force } => {
                undo.record(
                    ops,
                    &format!("Delete branch {name}"),
                    &[heads(&name)],
                    false,
                    || ops.delete_branch(&name, force),
                    |_| false,
                )?;
                ok()
            }
            Command::RenameBranch { from, to } => {
                undo.record(
                    ops,
                    &format!("Rename {from} to {to}"),
                    &[heads(&from), heads(&to)],
                    false,
                    || ops.rename_branch(&from, &to),
                    |_| false,
                )?;
                ok()
            }
            Command::CreateTag {
                name,
                target,
                message,
            } => {
                undo.record(
                    ops,
                    &format!("Create tag {name}"),
                    &[format!("refs/tags/{name}")],
                    false,
                    || ops.create_tag(&name, &target, message.as_deref()),
                    |_| false,
                )?;
                ok()
            }
            // コンフリクトや途中で止まったときは記録しない（戻すのは Abort の役目）。
            Command::Merge { rev } => done(undo.record(
                ops,
                &format!("Merge {rev}"),
                &[],
                false,
                || ops.merge(&rev),
                not_done,
            )?),
            Command::MergeInto { target, rev } => {
                let current = ops.head_state()?.branch;
                let full = heads(&target);
                if current.as_deref() != Some(full.as_str()) && ops.is_ancestor(&full, &rev)? {
                    let from = ops.resolve(&full)?;
                    let to = ops.resolve(&rev)?;
                    undo.record(
                        ops,
                        &format!("Fast-forward {target} to {rev}"),
                        std::slice::from_ref(&full),
                        false,
                        || ops.update_ref(&full, Some(&to), Some(&from)),
                        |_| false,
                    )?;
                    return ok();
                }
                if current.as_deref() != Some(full.as_str()) {
                    ops.switch_branch(&target)?;
                }
                done(undo.record(
                    ops,
                    &format!("Merge {rev} into {target}"),
                    &[],
                    false,
                    || ops.merge(&rev),
                    not_done,
                )?)
            }
            Command::Rebase { onto } => done(undo.record(
                ops,
                &format!("Rebase onto {onto}"),
                &[],
                false,
                || ops.rebase(&onto),
                not_done,
            )?),
            Command::CherryPick { oid: o, is_merge } => {
                let id = oid(&o)?;
                done(undo.record(
                    ops,
                    &format!("Cherry-pick {}", &o[..o.len().min(8)]),
                    &[],
                    false,
                    || ops.cherry_pick(&id, is_merge),
                    not_done,
                )?)
            }
            Command::Revert { oid: o, is_merge } => {
                let id = oid(&o)?;
                done(undo.record(
                    ops,
                    &format!("Revert {}", &o[..o.len().min(8)]),
                    &[],
                    false,
                    || ops.revert(&id, is_merge),
                    not_done,
                )?)
            }
            Command::Abort => {
                let sequence = ops.sequence_in_progress()?.ok_or("途中の操作がない")?;
                ops.abort(sequence)?;
                undo.settle_pending(ops)?;
                ok()
            }
            Command::Continue => {
                let sequence = ops.sequence_in_progress()?.ok_or("途中の操作がない")?;
                let outcome = ops.continue_sequence(sequence)?;
                undo.settle_pending(ops)?;
                done(outcome)
            }
            Command::Skip => {
                let sequence = ops.sequence_in_progress()?.ok_or("途中の操作がない")?;
                let outcome = ops.skip_sequence(sequence)?;
                undo.settle_pending(ops)?;
                done(outcome)
            }
            Command::Fetch => {
                ops.fetch()?;
                ok()
            }
            Command::Pull => done(undo.record(ops, "Pull", &[], false, || ops.pull(), not_done)?),
            Command::Push {
                remote,
                branch,
                remote_branch,
                force,
                set_upstream,
            } => {
                ops.push(&remote, &branch, &remote_branch, force, set_upstream)?;
                ok()
            }
            Command::Discard { paths } => {
                let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
                let label = match paths.as_slice() {
                    [one] => format!("Discard {one}"),
                    many => format!("Discard {} files", many.len()),
                };
                undo.record_files(ops, &label, &paths, || ops.restore_worktree(&paths))?;
                ok()
            }
            Command::StashPush {
                message,
                include_untracked,
            } => {
                ops.stash_push(message.as_deref(), include_untracked)?;
                ok()
            }
            Command::StashApply { name, pop } => done(ops.stash_apply(&name, pop)?),
            Command::StashDrop { name } => {
                let refs = self.load_refs()?;
                let entry = refs
                    .refs
                    .entries
                    .iter()
                    .find(|e| e.kind == amonhen_model::RefKind::Stash && e.name == name)
                    .ok_or_else(|| format!("{name} がない"))?;
                let target = entry.target.ok_or("stash のコミットがない")?;
                let message = entry.detail.clone().unwrap_or_else(|| name.clone());
                undo.record_stash_drop(ops, &name, &target, &message)?;
                ok()
            }
            Command::UpdateSubmodules { paths } => {
                let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
                ops.update_submodules(&paths)?;
                ok()
            }
            Command::Undo => {
                let label = undo.undo(ops)?;
                Ok(Response {
                    ok: true,
                    label,
                    ..Default::default()
                })
            }
            Command::UndoPeek => {
                // 外で rebase を終わらせた場合も、ここで確定させる。
                undo.settle_pending(ops)?;
                Ok(Response {
                    ok: true,
                    label: undo.peek(),
                    ..Default::default()
                })
            }
            Command::Rewrite { label, edit } => done(self.rewrite(&label, &edit)?),
            Command::CommitMessage { oid: o } => {
                // "HEAD" なら今のコミット（Amend の初期値）。
                let id = if o == "HEAD" {
                    self.backend().head()?.ok_or("まだコミットがありません")?
                } else {
                    oid(&o)?
                };
                Ok(Response {
                    ok: true,
                    text: Some(self.commit_message(&id)?),
                    ..Default::default()
                })
            }
            Command::EditableCommits => {
                let chain = self.editable_chain()?;
                let text = chain
                    .iter()
                    .map(|c| c.oid.to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
                Ok(Response {
                    ok: true,
                    text: Some(text),
                    ..Default::default()
                })
            }
            Command::SquashMessage { oid: o } => {
                let id = oid(&o)?;
                let chain = self.editable_chain()?;
                let parent = chain
                    .iter()
                    .find(|c| c.oid == id)
                    .and_then(|c| c.parent)
                    .ok_or("親が編集できる範囲にないので、まとめられません")?;
                let text = format!(
                    "{}\n\n{}",
                    self.commit_message(&parent)?.trim_end(),
                    self.commit_message(&id)?.trim_end()
                );
                Ok(Response {
                    ok: true,
                    text: Some(text),
                    ..Default::default()
                })
            }
            Command::ResolveConflict {
                path,
                index,
                choice,
            } => {
                let workdir = self
                    .backend()
                    .paths()
                    .workdir
                    .ok_or("作業ツリーのないリポジトリ")?;
                let file = workdir.join(&path);
                let data = std::fs::read(&file)?;
                let resolved = crate::conflict::resolve(&data, index, choice)
                    .ok_or("そのコンフリクトはもうない")?;
                undo.record_files(
                    ops,
                    &format!("Resolve conflict in {path}"),
                    &[&path],
                    || Ok(std::fs::write(&file, &resolved)?),
                )?;
                ok()
            }
            Command::CommitInfo { oid: o } => Ok(Response {
                ok: true,
                info: Some(self.commit_info(&oid(&o)?)?),
                ..Default::default()
            }),
            Command::TakeSide { path, ours } => {
                ops.take_side(&path, ours)?;
                ok()
            }
            Command::Sequence => {
                // rebase なら何番目かも添える（"rebase (2/5)"）。
                let text = match ops.sequence_in_progress()? {
                    Some(amonhen_model::Sequence::Rebase) => Some(match ops.rebase_progress()? {
                        Some((current, total)) => format!("rebase ({current}/{total})"),
                        None => "rebase".to_owned(),
                    }),
                    other => other.map(|s| s.command().to_owned()),
                };
                Ok(Response {
                    ok: true,
                    text,
                    ..Default::default()
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use amonhen_backend_gix::GixBackend;
    use amonhen_projection::FrontendProfile;
    use std::process::Command as Process;

    fn git(dir: &std::path::Path, args: &[&str]) -> String {
        let out = Process::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }

    fn setup() -> (tempfile::TempDir, Repository) {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        git(dir, &["init", "-q", "-b", "main"]);
        git(dir, &["config", "user.name", "a"]);
        git(dir, &["config", "user.email", "a@example.com"]);
        std::fs::write(dir.join("a.txt"), "1\n2\n3\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "first"]);
        let repo = Repository::new(GixBackend::open(dir).unwrap());
        (tmp, repo)
    }

    fn run(repo: &Repository, json: &str) -> serde_json::Value {
        serde_json::from_str(&repo.run_json(json)).unwrap()
    }

    #[test]
    fn runs_commands_and_undoes_them() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let r = run(
            &repo,
            r#"{"op":"createBranch","name":"topic","start":"main","checkout":false}"#,
        );
        assert_eq!(r["ok"], true);
        assert_eq!(
            run(&repo, r#"{"op":"undoPeek"}"#)["label"],
            "Create branch topic"
        );
        assert_eq!(
            run(&repo, r#"{"op":"undo"}"#)["label"],
            "Create branch topic"
        );
        assert!(git(dir, &["branch", "--list", "topic"]).is_empty());

        let bad = run(&repo, r#"{"op":"merge","rev":"no-such-branch"}"#);
        assert_eq!(bad["ok"], false);
        assert!(!bad["error"].as_str().unwrap().is_empty());
        assert_eq!(run(&repo, r#"{"op":"nonsense"}"#)["ok"], false);
    }

    fn log(dir: &std::path::Path) -> Vec<String> {
        git(dir, &["log", "--format=%s"])
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn commit_file(dir: &std::path::Path, name: &str, subject: &str) -> String {
        std::fs::write(dir.join(name), subject).unwrap();
        git(dir, &["add", name]);
        git(dir, &["commit", "-q", "-m", subject]);
        git(dir, &["rev-parse", "HEAD"])
    }

    fn rewrite(repo: &Repository, edit: serde_json::Value) -> serde_json::Value {
        let json = serde_json::json!({"op": "rewrite", "label": "edit", "edit": edit});
        run(repo, &json.to_string())
    }

    #[test]
    fn rewrites_history_and_undoes_it() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        let b = commit_file(dir, "b.txt", "b");
        let c = commit_file(dir, "c.txt", "c");
        commit_file(dir, "d.txt", "d");
        assert_eq!(log(dir), ["d", "c", "b", "first"]);

        assert_eq!(
            run(&repo, &format!(r#"{{"op":"commitMessage","oid":"{c}"}}"#))["text"],
            "c\n"
        );
        let r = rewrite(
            &repo,
            serde_json::json!({"kind": "reword", "oid": c, "message": "C!"}),
        );
        assert_eq!(r["outcome"], "done", "{r}");
        assert_eq!(log(dir), ["d", "C!", "b", "first"]);
        assert_eq!(run(&repo, r#"{"op":"undo"}"#)["ok"], true);
        assert_eq!(log(dir), ["d", "c", "b", "first"]);

        rewrite(&repo, serde_json::json!({"kind": "fixup", "oid": c}));
        assert_eq!(log(dir), ["d", "b", "first"]);
        run(&repo, r#"{"op":"undo"}"#);
        rewrite(&repo, serde_json::json!({"kind": "drop", "oid": b}));
        assert_eq!(log(dir), ["d", "c", "first"]);
        run(&repo, r#"{"op":"undo"}"#);
        rewrite(
            &repo,
            serde_json::json!({"kind": "move", "oid": b, "to": 0}),
        );
        assert_eq!(log(dir), ["b", "d", "c", "first"]);
        run(&repo, r#"{"op":"undo"}"#);
        assert_eq!(log(dir), ["d", "c", "b", "first"]);
    }

    #[test]
    fn splits_a_commit_and_records_undo_after_continuing() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        std::fs::write(dir.join("x.txt"), "x").unwrap();
        std::fs::write(dir.join("y.txt"), "y").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "x and y"]);
        let xy = git(dir, &["rev-parse", "HEAD"]);
        commit_file(dir, "z.txt", "z");

        let r = rewrite(&repo, serde_json::json!({"kind": "split", "oid": xy}));
        assert_eq!(r["outcome"], "stopped", "{r}");
        // todo は pick・exec・break・pick の 4 行で、break まで進んだところで止まる。
        assert_eq!(run(&repo, r#"{"op":"sequence"}"#)["text"], "rebase (3/4)");
        // 止まっている間は記録しない（保留）。
        assert!(run(&repo, r#"{"op":"undoPeek"}"#)["label"].is_null());
        git(dir, &["add", "x.txt"]);
        git(dir, &["commit", "-q", "-m", "x"]);
        git(dir, &["add", "y.txt"]);
        git(dir, &["commit", "-q", "-m", "y"]);
        assert_eq!(run(&repo, r#"{"op":"continue"}"#)["outcome"], "done");
        assert_eq!(log(dir), ["z", "y", "x", "first"]);
        assert_eq!(run(&repo, r#"{"op":"undoPeek"}"#)["label"], "edit");
        run(&repo, r#"{"op":"undo"}"#);
        assert_eq!(log(dir), ["z", "x and y", "first"]);
    }

    #[test]
    fn resolves_merge_conflicts() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        git(dir, &["switch", "-q", "-c", "topic"]);
        commit_file(dir, "a.txt", "1\nTOPIC\n3\n");
        git(dir, &["switch", "-q", "main"]);
        commit_file(dir, "a.txt", "1\nMAIN\n3\n");
        assert_eq!(
            run(&repo, r#"{"op":"merge","rev":"topic"}"#)["outcome"],
            "conflict"
        );

        let v = repo
            .conflict_view("a.txt", &FrontendProfile::MACOS)
            .unwrap();
        assert_eq!(v.removed, 1, "コンフリクトの数");
        let r = run(
            &repo,
            r#"{"op":"resolveConflict","path":"a.txt","index":0,"choice":"theirs"}"#,
        );
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "1\nTOPIC\n3\n"
        );
        // 解決は Undo できる。
        assert_eq!(run(&repo, r#"{"op":"undo"}"#)["ok"], true);
        assert!(
            std::fs::read_to_string(dir.join("a.txt"))
                .unwrap()
                .contains("<<<<<<<")
        );

        assert_eq!(
            run(&repo, r#"{"op":"takeSide","path":"a.txt","ours":true}"#)["ok"],
            true
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "1\nMAIN\n3\n"
        );
        let status = repo.load_status().unwrap();
        assert!(
            status
                .status
                .entries
                .iter()
                .all(|e| e.kind != amonhen_model::StatusKind::Unmerged)
        );
        assert_eq!(run(&repo, r#"{"op":"continue"}"#)["outcome"], "done");
        assert_eq!(
            run(&repo, r#"{"op":"sequence"}"#)["text"],
            serde_json::Value::Null
        );
    }

    #[test]
    fn fast_forwards_a_branch_without_checking_it_out() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        git(dir, &["branch", "old"]);
        commit_file(dir, "b.txt", "b");
        let head = git(dir, &["rev-parse", "HEAD"]);
        let r = run(&repo, r#"{"op":"mergeInto","target":"old","rev":"main"}"#);
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(git(dir, &["rev-parse", "old"]), head);
        assert_eq!(
            git(dir, &["symbolic-ref", "--short", "HEAD"]),
            "main",
            "checkout しない"
        );
        run(&repo, r#"{"op":"undo"}"#);
        assert_ne!(git(dir, &["rev-parse", "old"]), head);
    }

    #[test]
    fn commits_and_discards_lines_undoably() {
        let (tmp, repo) = setup();
        let dir = tmp.path();
        std::fs::write(dir.join("a.txt"), "one\n2\nthree\n").unwrap();
        let status = repo.load_status().unwrap();
        let diff = repo
            .working_diff(&status.status.entries[0], false, &FrontendProfile::MACOS)
            .unwrap();
        // @@, -1, +one, 2, -3, +three: 1 → one だけ戻す。
        repo.discard_rows(&diff, &[1, 2]).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "1\n2\nthree\n"
        );
        assert_eq!(run(&repo, r#"{"op":"undo"}"#)["ok"], true);
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "one\n2\nthree\n"
        );

        repo.stage(&["a.txt"]).unwrap();
        repo.commit("second", false).unwrap();
        assert_eq!(run(&repo, r#"{"op":"undo"}"#)["label"], "Commit");
        assert_eq!(git(dir, &["log", "--format=%s"]), "first");
        assert_eq!(git(dir, &["diff", "--cached", "--name-only"]), "a.txt");
    }
}
