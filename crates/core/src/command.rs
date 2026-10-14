//! FE から受け取る操作。頻度の低い操作はまとめてここを通し、Undo の記録もここで行う。

use amonhen_model::{Oid, Outcome, Result};
use serde::{Deserialize, Serialize};

use crate::Repository;

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
                ok()
            }
            Command::Continue => {
                let sequence = ops.sequence_in_progress()?.ok_or("途中の操作がない")?;
                done(ops.continue_sequence(sequence)?)
            }
            Command::Skip => {
                let sequence = ops.sequence_in_progress()?.ok_or("途中の操作がない")?;
                done(ops.skip_sequence(sequence)?)
            }
            Command::Fetch => {
                ops.fetch()?;
                ok()
            }
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
            Command::UndoPeek => Ok(Response {
                ok: true,
                label: undo.peek(),
                ..Default::default()
            }),
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
