use amonhen_model::{HeadState, Oid, Operations, Outcome, ResetMode, Result, Sequence};

use crate::Git;

fn hex(oid: &Oid) -> String {
    oid.to_string()
}

fn text(out: Vec<u8>) -> Result<String> {
    Ok(String::from_utf8(out)?.trim().to_owned())
}

impl Git {
    /// 失敗しても、途中で止まっていればコンフリクトとして扱う。成功しても止まっていれば Stopped。
    fn stoppable(&self, args: &[&str], env: &[(&str, &str)]) -> Result<Outcome> {
        let mut cmd = self.command(args);
        for (k, v) in env {
            cmd.env(k, v);
        }
        match crate::run(cmd, None) {
            Ok(_) if self.sequence_in_progress()?.is_some() => Ok(Outcome::Stopped),
            Ok(_) => Ok(Outcome::Done),
            Err(_) if self.sequence_in_progress()?.is_some() => Ok(Outcome::Conflict),
            Err(e) => Err(e),
        }
    }

    fn git_dir(&self) -> Result<std::path::PathBuf> {
        let dir = text(self.read(&["rev-parse", "--absolute-git-dir"])?)?;
        Ok(dir.into())
    }

    /// rebase に todo を差し込む（コミットの編集）。git は `$GIT_SEQUENCE_EDITOR <todo>` を sh で
    /// 実行するので、用意した todo を上書きコピーさせる。
    pub fn rebase_todo(&self, base: Option<&Oid>, todo: &std::path::Path) -> Result<Outcome> {
        let base = base.map(hex);
        let editor = format!("cp '{}'", todo.display().to_string().replace('\'', r"'\''"));
        let mut args = vec!["rebase", "--interactive", "--autostash", "--no-autosquash"];
        args.push(base.as_deref().unwrap_or("--root"));
        self.stoppable(&args, &[("GIT_SEQUENCE_EDITOR", &editor)])
    }
}

impl Operations for Git {
    fn head_state(&self) -> Result<HeadState> {
        let branch = self
            .read(&["symbolic-ref", "-q", "HEAD"])
            .ok()
            .map(text)
            .transpose()?;
        let oid = self
            .read(&["rev-parse", "-q", "--verify", "HEAD"])
            .ok()
            .map(text)
            .transpose()?;
        Ok(HeadState {
            branch: branch.filter(|b| !b.is_empty()),
            oid: oid.as_deref().and_then(Oid::from_hex),
        })
    }

    fn resolve_refs(&self, names: &[String]) -> Result<Vec<(String, Option<Oid>)>> {
        let mut result: Vec<(String, Option<Oid>)> =
            names.iter().map(|n| (n.clone(), None)).collect();
        if names.is_empty() {
            return Ok(result);
        }
        let mut args = vec!["for-each-ref", "--format=%(refname)%00%(objectname)"];
        args.extend(names.iter().map(String::as_str));
        let out = text(self.read(&args)?)?;
        // for-each-ref のパターンは前方一致なので、完全一致だけ拾う。
        for line in out.lines() {
            if let Some((name, oid)) = line.split_once('\0')
                && let Some(slot) = result.iter_mut().find(|(n, _)| n == name)
            {
                slot.1 = Oid::from_hex(oid);
            }
        }
        Ok(result)
    }

    fn update_ref(&self, name: &str, value: Option<&Oid>, expected: Option<&Oid>) -> Result<()> {
        let expected = expected.map(hex).unwrap_or_default();
        match value {
            Some(v) => self.write(&["update-ref", name, &hex(v), &expected]),
            None => self.write(&["update-ref", "-d", name, &expected]),
        }
        .map(drop)
    }

    fn reset(&self, mode: ResetMode, oid: &Oid) -> Result<()> {
        let mode = match mode {
            ResetMode::Soft => "--soft",
            ResetMode::Keep => "--keep",
        };
        self.write(&["reset", mode, &hex(oid)]).map(drop)
    }

    fn switch_to(&self, head: &HeadState) -> Result<()> {
        match (&head.branch, &head.oid) {
            (Some(branch), _) => self.write(&["switch", branch.trim_start_matches("refs/heads/")]),
            (None, Some(oid)) => self.write(&["switch", "--detach", &hex(oid)]),
            (None, None) => return Err("切り替え先がない".into()),
        }
        .map(drop)
    }

    fn write_blob(&self, content: &[u8]) -> Result<Oid> {
        let out = text(
            self.write_with_input(&["hash-object", "-w", "--no-filters", "--stdin"], content)?,
        )?;
        Oid::from_hex(&out).ok_or_else(|| format!("blob の ID を読めない: {out}").into())
    }

    fn read_blob(&self, oid: &Oid) -> Result<Vec<u8>> {
        self.read(&["cat-file", "blob", &hex(oid)])
    }

    fn switch_branch(&self, name: &str) -> Result<()> {
        // ブランチ名は `-` で始められないので、オプションと誤認されない。
        self.write(&["switch", name]).map(drop)
    }

    fn track_branch(&self, remote_branch: &str) -> Result<()> {
        self.write(&["switch", "--track", remote_branch]).map(drop)
    }

    fn create_branch(&self, name: &str, start: &str, checkout: bool) -> Result<()> {
        if checkout {
            self.write(&["switch", "--create", name, start])
        } else {
            self.write(&["branch", "--", name, start])
        }
        .map(drop)
    }

    fn delete_branch(&self, name: &str, force: bool) -> Result<()> {
        self.write(&["branch", if force { "-D" } else { "-d" }, "--", name])
            .map(drop)
    }

    fn rename_branch(&self, from: &str, to: &str) -> Result<()> {
        self.write(&["branch", "--move", "--", from, to]).map(drop)
    }

    fn create_tag(&self, name: &str, target: &str, message: Option<&str>) -> Result<()> {
        match message {
            Some(m) => self.write_with_input(
                &["tag", "--annotate", "--file=-", "--", name, target],
                m.as_bytes(),
            ),
            None => self.write(&["tag", "--", name, target]),
        }
        .map(drop)
    }

    fn check_branch_name(&self, name: &str) -> Result<Option<String>> {
        Ok(self
            .read(&["check-ref-format", "--branch", name])
            .err()
            .map(|e| e.to_string()))
    }

    fn merge(&self, rev: &str) -> Result<Outcome> {
        self.stoppable(&["merge", "--no-edit", rev], &[])
    }

    fn rebase(&self, onto: &str) -> Result<Outcome> {
        self.stoppable(&["rebase", onto], &[])
    }

    fn cherry_pick(&self, oid: &Oid, is_merge: bool) -> Result<Outcome> {
        let oid = hex(oid);
        if is_merge {
            self.stoppable(&["cherry-pick", "-m", "1", &oid], &[])
        } else {
            self.stoppable(&["cherry-pick", &oid], &[])
        }
    }

    fn revert(&self, oid: &Oid, is_merge: bool) -> Result<Outcome> {
        let oid = hex(oid);
        if is_merge {
            self.stoppable(&["revert", "--no-edit", "-m", "1", &oid], &[])
        } else {
            self.stoppable(&["revert", "--no-edit", &oid], &[])
        }
    }

    fn sequence_in_progress(&self) -> Result<Option<Sequence>> {
        let dir = self.git_dir()?;
        let has = |name: &str| dir.join(name).exists();
        Ok(if has("rebase-merge") || has("rebase-apply") {
            Some(Sequence::Rebase)
        } else if has("MERGE_HEAD") {
            Some(Sequence::Merge)
        } else if has("CHERRY_PICK_HEAD") {
            Some(Sequence::CherryPick)
        } else if has("REVERT_HEAD") {
            Some(Sequence::Revert)
        } else {
            None
        })
    }

    fn abort(&self, sequence: Sequence) -> Result<()> {
        self.write(&[sequence.command(), "--abort"]).map(drop)
    }

    fn continue_sequence(&self, sequence: Sequence) -> Result<Outcome> {
        // 続けるときにメッセージの編集画面を開かせない。
        self.stoppable(
            &[sequence.command(), "--continue"],
            &[("GIT_EDITOR", "true")],
        )
    }

    fn skip_sequence(&self, sequence: Sequence) -> Result<Outcome> {
        self.stoppable(&[sequence.command(), "--skip"], &[])
    }

    fn fetch(&self) -> Result<()> {
        self.write(&["fetch", "--all", "--prune"]).map(drop)
    }

    fn push(
        &self,
        remote: &str,
        branch: &str,
        remote_branch: &str,
        force: bool,
        set_upstream: bool,
    ) -> Result<()> {
        let refspec = format!("refs/heads/{branch}:refs/heads/{remote_branch}");
        let mut args = vec!["push"];
        if force {
            args.push("--force-with-lease");
        }
        if set_upstream {
            args.push("--set-upstream");
        }
        args.extend([remote, &refspec]);
        self.write(&args).map(drop)
    }

    fn restore_worktree(&self, paths: &[&str]) -> Result<()> {
        let mut args = vec!["checkout", "--"];
        args.extend(paths);
        self.write(&args).map(drop)
    }

    fn stash_push(&self, message: Option<&str>, include_untracked: bool) -> Result<()> {
        let mut args = vec!["stash", "push"];
        if include_untracked {
            args.push("--include-untracked");
        }
        if let Some(m) = message {
            args.extend(["--message", m]);
        }
        self.write(&args).map(drop)
    }

    fn stash_apply(&self, name: &str, pop: bool) -> Result<Outcome> {
        let verb = if pop { "pop" } else { "apply" };
        for with_index in [true, false] {
            let mut args = vec!["stash", verb];
            if with_index {
                args.push("--index");
            }
            args.push(name);
            match self.write(&args) {
                Ok(_) => return Ok(Outcome::Done),
                Err(e) => {
                    let conflicted = self
                        .status()?
                        .entries
                        .iter()
                        .any(|e| e.kind == amonhen_model::StatusKind::Unmerged);
                    if conflicted {
                        return Ok(Outcome::Conflict);
                    }
                    // stage していた変更を index に戻せないときは「Try without --index」と言われる。
                    if with_index && e.to_string().contains("without --index") {
                        continue;
                    }
                    return Err(e);
                }
            }
        }
        Ok(Outcome::Done)
    }

    fn stash_drop(&self, name: &str) -> Result<()> {
        self.write(&["stash", "drop", name]).map(drop)
    }

    fn stash_store(&self, oid: &Oid, message: &str) -> Result<()> {
        self.write(&["stash", "store", "--message", message, &hex(oid)])
            .map(drop)
    }

    fn update_submodules(&self, paths: &[&str]) -> Result<()> {
        let mut args = vec!["submodule", "update", "--init", "--"];
        args.extend(paths);
        self.write(&args).map(drop)
    }
}
