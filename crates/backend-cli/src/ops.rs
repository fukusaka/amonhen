use amonhen_model::tr;
use amonhen_model::{
    ConfigEntry, HeadState, Oid, Operations, Outcome, PullMode, RefUpdate, RemoteChange,
    RemoteEntry, ResetMode, Result, Sequence, SigningKey, WorktreeChange, WorktreeChanges,
};

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
}

/// `-G` に渡す正規表現（git は拡張正規表現として読む）で、`text` をそのままの文字列として探すための書き方。
fn literal_regex(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for c in text.chars() {
        if matches!(
            c,
            '\\' | '.' | '*' | '[' | ']' | '^' | '$' | '(' | ')' | '+' | '?' | '{' | '}' | '|'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// マージは `--name-only` でもファイル名が出ないので、パスのないコミットに今のパスを添える（`-- <path>` で絞って
/// いるので、出てくるマージはそのパスを変えている）。
fn with_merge_paths(part: &[u8], path: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(part.len());
    for record in part.split(|&b| b == 1).filter(|r| !r.is_empty()) {
        out.push(1);
        out.extend_from_slice(record);
        // ID・件名・作者・時刻の 4 つの後に、パスがあるか。
        let has_path = record
            .split(|&b| b == 0)
            .nth(4)
            .is_some_and(|p| p.iter().any(|&b| b != b'\n'));
        if !has_path {
            if record.last() != Some(&0) {
                out.push(0);
            }
            out.push(b'\n');
            out.extend_from_slice(path.as_bytes());
            out.push(0);
        }
    }
    out
}

/// `git diff-tree -z --name-status` の出力から、`path` へ名前を変えたときの元の名前を探す。
fn renamed_from(out: &[u8], path: &str) -> Option<String> {
    let mut fields = out.split(|&b| b == 0).filter(|f| !f.is_empty());
    while let Some(status) = fields.next() {
        if status.starts_with(b"R") || status.starts_with(b"C") {
            let (from, to) = (fields.next()?, fields.next()?);
            if to == path.as_bytes() {
                return Some(String::from_utf8_lossy(from).into_owned());
            }
        } else {
            fields.next()?;
        }
    }
    None
}

impl Git {
    /// `scope`（`--local`・`--global`）の設定を変える。`value` が None なら消す。
    fn config_set_in(&self, scope: &str, key: &str, value: Option<&str>) -> Result<()> {
        match value {
            Some(v) => self.write(&["config", scope, key, v]).map(drop),
            None => match self.write(&["config", scope, "--unset-all", key]) {
                Ok(_) => Ok(()),
                // もともとない（終了コード 5）なら、消したのと同じ。
                Err(_) if self.read(&["config", scope, "--get-all", key]).is_err() => Ok(()),
                Err(e) => Err(e),
            },
        }
    }
}

impl Operations for Git {
    fn blame(&self, path: &str, rev: Option<&Oid>) -> Result<Vec<u8>> {
        let rev = rev.map(|r| r.to_string());
        let mut args = vec!["blame", "--porcelain"];
        if let Some(r) = &rev {
            args.push(r);
        }
        args.extend(["--", path]);
        self.read(&args)
    }

    /// `--follow` は使わない（履歴の単純化も changed-path の Bloom フィルターも効かず、全コミットを歩く）。
    /// パスを絞った log で辿り、いちばん古いコミットでそのパスが改名で生まれていたら、元の名前でその親から続ける。
    fn file_log(&self, path: &str, limit: Option<usize>, merges: bool) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut path = path.to_owned();
        let mut start = "HEAD".to_owned();
        let mut left = limit;
        loop {
            let max_count = left.map(|n| format!("--max-count={n}"));
            let mut args = vec!["log", "-z", "--name-only"];
            if !merges {
                args.push("--no-merges");
            }
            args.extend(max_count.as_deref());
            args.extend(["--format=%x01%H%x00%s%x00%an%x00%at", &start, "--", &path]);
            let mut part = self.read(&args)?;
            if merges {
                part = with_merge_paths(&part, &path);
            }
            // 各コミットは \x01 で始まり、ID が先頭に来る。
            let oldest = part
                .rsplit(|&b| b == 1)
                .next()
                .and_then(|r| r.split(|&b| b == 0).next())
                .map(|oid| String::from_utf8_lossy(oid).into_owned())
                .filter(|oid| !oid.is_empty());
            out.extend_from_slice(&part);
            let Some(oldest) = oldest else { break };
            if let Some(n) = &mut left {
                *n = n.saturating_sub(part.iter().filter(|&&b| b == 1).count());
                if *n == 0 {
                    break;
                }
            }
            let Some(from) = renamed_from(
                &self.read(&[
                    "diff-tree",
                    "--no-commit-id",
                    "-C",
                    "-r",
                    "-z",
                    "--name-status",
                    "--root",
                    &oldest,
                ])?,
                &path,
            ) else {
                break;
            };
            path = from;
            start = format!("{oldest}^");
        }
        Ok(out)
    }

    fn log_matching(
        &self,
        tips: &[Oid],
        path: Option<&str>,
        contents: Option<&str>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<Oid>> {
        use std::io::{BufRead, Write};
        let pattern = contents.map(|c| format!("-G{}", literal_regex(c)));
        let mut args = vec!["log", "--format=%H", "--stdin"];
        if let Some(p) = &pattern {
            args.push(p);
        }
        args.push("--");
        if let Some(p) = path {
            args.push(p);
        }
        let mut cmd = self.command(&args);
        cmd.env("GIT_OPTIONAL_LOCKS", "0")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = cmd.spawn()?;
        let mut stderr = child.stderr.take().unwrap();
        let errors = std::thread::spawn(move || {
            let mut text = String::new();
            let _ = std::io::Read::read_to_string(&mut stderr, &mut text);
            text
        });
        // 起点は引数に並べず標準入力で渡す（ref が数千あっても長さの制限にかからない）。
        let mut stdin = child.stdin.take().unwrap();
        let revs: String = tips.iter().map(|t| format!("{t}\n")).collect();
        std::thread::spawn(move || stdin.write_all(revs.as_bytes()));
        // 結果がなかなか出ない検索（内容の検索）でも取り消しに応えられるよう、読むのは別のスレッドで行う。
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut found = Vec::new();
        loop {
            if cancelled() {
                crate::kill_tree(&mut child);
                let _ = child.wait();
                return Err(amonhen_model::i18n::cancelled().into());
            }
            match rx.recv_timeout(std::time::Duration::from_millis(50)) {
                Ok(line) => found.extend(Oid::from_hex(line.trim())),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        let status = child.wait()?;
        if !status.success() {
            let message = errors.join().unwrap_or_default();
            return Err(tr!("git log failed: {message}", message = message.trim()).into());
        }
        Ok(found)
    }

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

    fn update_refs(&self, updates: &[RefUpdate]) -> Result<()> {
        // `update-ref --stdin` は全部を 1 つのトランザクションで書く（どれかの期待値が違えば何も変えない）。
        // 「まだない」は 0 の oid で表す。
        let zero = "0".repeat(40);
        let mut input = String::new();
        for u in updates {
            let expected = u.expected.as_ref().map_or_else(|| zero.clone(), hex);
            match &u.value {
                Some(v) => input.push_str(&format!("update {} {} {expected}\n", u.name, hex(v))),
                None => input.push_str(&format!("delete {} {expected}\n", u.name)),
            }
        }
        self.write_with_input(&["update-ref", "--stdin"], input.as_bytes())
            .map(drop)
    }

    fn reset(&self, mode: ResetMode, oid: &Oid) -> Result<()> {
        let mode = match mode {
            ResetMode::Soft => "--soft",
            ResetMode::Mixed => "--mixed",
            ResetMode::Hard => "--hard",
            ResetMode::Keep => "--keep",
        };
        self.write(&["reset", mode, &hex(oid)]).map(drop)
    }

    fn switch_to(&self, head: &HeadState) -> Result<()> {
        match (&head.branch, &head.oid) {
            (Some(branch), _) => self.write(&["switch", branch.trim_start_matches("refs/heads/")]),
            (None, Some(oid)) => self.write(&["switch", "--detach", &hex(oid)]),
            (None, None) => return Err(tr!("Nothing to switch to").into()),
        }
        .map(drop)
    }

    fn write_blob(&self, content: &[u8]) -> Result<Oid> {
        let out = text(
            self.write_with_input(&["hash-object", "-w", "--no-filters", "--stdin"], content)?,
        )?;
        Oid::from_hex(&out).ok_or_else(|| tr!("Can’t read the blob ID: {out}", out = out).into())
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

    fn delete_tag(&self, name: &str) -> Result<()> {
        self.write(&["tag", "--delete", "--", name]).map(drop)
    }

    fn remote_tags(&self, remote: &str) -> Result<Vec<(String, Oid)>> {
        // `<oid>\trefs/tags/<name>` の行。注釈付きのタグの後には、たどった先のコミットの `<name>^{}` の行が
        // 続くが、比べるのはタグのオブジェクトなので読まない。
        let out = text(self.read(&["ls-remote", "--tags", remote])?)?;
        Ok(out
            .lines()
            .filter_map(|line| {
                let (oid, name) = line.split_once('\t')?;
                let name = name.strip_prefix("refs/tags/")?;
                if name.ends_with("^{}") {
                    return None;
                }
                Some((name.to_owned(), Oid::from_hex(oid)?))
            })
            .collect())
    }

    fn push_tags(&self, remote: &str, names: &[&str], force: bool, delete: bool) -> Result<()> {
        // 同じ名前のブランチと取り違えないよう、完全な名前で送る（`+` は置き換え）。
        let refspecs: Vec<String> = names
            .iter()
            .map(|n| match (delete, force) {
                (true, _) => format!(":refs/tags/{n}"),
                (false, true) => format!("+refs/tags/{n}:refs/tags/{n}"),
                (false, false) => format!("refs/tags/{n}:refs/tags/{n}"),
            })
            .collect();
        let mut args = vec!["push", remote];
        args.extend(refspecs.iter().map(String::as_str));
        self.write(&args).map(drop)
    }

    fn check_branch_name(&self, name: &str) -> Result<Option<String>> {
        Ok(self
            .read(&["check-ref-format", "--branch", name])
            .err()
            .map(|e| e.to_string()))
    }

    fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool> {
        let mut cmd = self.command(&["merge-base", "--is-ancestor", ancestor, descendant]);
        cmd.env("GIT_OPTIONAL_LOCKS", "0");
        let status = cmd
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()?;
        match status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(tr!(
                "Can’t compare {ancestor} and {descendant}",
                ancestor = ancestor,
                descendant = descendant
            )
            .into()),
        }
    }

    fn predict_merge(&self, ours: &str, theirs: &str) -> Result<Option<Vec<String>>> {
        // `merge-tree --write-tree` は git 2.38 から。それより古い git では調べない（マージを試すと作業ツリーに触る）。
        if !crate::git_at_least(2, 38) {
            return Ok(None);
        }
        let args = [
            "merge-tree",
            "--write-tree",
            "--name-only",
            "--no-messages",
            "-z",
            ours,
            theirs,
        ];
        let mut cmd = self.command(&args);
        cmd.env("GIT_OPTIONAL_LOCKS", "0");
        let repo = self.workdir.to_string_lossy().into_owned();
        let label = format!("git {}", args.join(" "));
        let out = amonhen_model::activity::track("git", &repo, label, || cmd.output())?;
        // 終了コードは、きれいにマージできれば 0、コンフリクトすれば 1。1 は名前が読めないときにも返るので、
        // 先頭の木の ID で見分ける。出力は 木の ID、コンフリクトするファイル（-z で NUL 区切り）。
        let mut fields = out.stdout.split(|&b| b == 0);
        let tree = fields.next().and_then(|f| std::str::from_utf8(f).ok());
        match out.status.code() {
            Some(0 | 1) if tree.and_then(Oid::from_hex).is_some() => Ok(Some(
                fields
                    .filter(|f| !f.is_empty())
                    .map(|f| String::from_utf8_lossy(f).into_owned())
                    .collect(),
            )),
            _ => Err(tr!(
                "Can’t merge {theirs} into {ours}: {error}",
                theirs = theirs,
                ours = ours,
                error = String::from_utf8_lossy(&out.stderr).trim()
            )
            .into()),
        }
    }

    fn resolve(&self, rev: &str) -> Result<Oid> {
        let spec = format!("{rev}^{{commit}}");
        let out = text(self.read(&["rev-parse", "--verify", "--quiet", &spec])?)?;
        Oid::from_hex(&out).ok_or_else(|| tr!("{rev} not found", rev = rev).into())
    }

    fn merge(&self, rev: &str) -> Result<Outcome> {
        self.stoppable(&["merge", "--no-edit", rev], &[])
    }

    fn rebase(&self, onto: &str) -> Result<Outcome> {
        // 積み重ねたブランチも一緒に動かすかは、git の `rebase.updateRefs` に任せる（git の既定はオフ）。
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

    fn rebase_todo(&self, base: Option<&Oid>, todo: &std::path::Path) -> Result<Outcome> {
        // todo の `update-ref`（積み重ねたブランチを一緒に動かす）は、`rebase.updateRefs` が true で、git が
        // 2.38 以降のときだけ残す（git の rebase と同じ条件）。
        if !self.updates_refs_in_rebase() {
            let text = std::fs::read_to_string(todo)?;
            if text.lines().any(|l| l.starts_with("update-ref ")) {
                let kept: String = text
                    .lines()
                    .filter(|l| !l.starts_with("update-ref "))
                    .map(|l| format!("{l}\n"))
                    .collect();
                std::fs::write(todo, kept)?;
            }
        }
        // git は `$GIT_SEQUENCE_EDITOR <todo>` を sh で実行するので、用意した todo を上書きコピーさせる。
        let base = base.map(hex);
        // sh で実行されるので、Windows でも / 区切りにして引用符で囲む。
        let path = todo.display().to_string().replace('\\', "/");
        let editor = format!("cp '{}'", path.replace('\'', r"'\''"));
        let mut args = vec!["rebase", "--interactive", "--autostash", "--no-autosquash"];
        args.push(base.as_deref().unwrap_or("--root"));
        self.stoppable(&args, &[("GIT_SEQUENCE_EDITOR", &editor)])
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

    fn rebase_progress(&self) -> Result<Option<(u32, u32)>> {
        let dir = self.git_dir()?;
        let read = |path: std::path::PathBuf| -> Option<u32> {
            std::fs::read_to_string(path).ok()?.trim().parse().ok()
        };
        for (sub, current, total) in [
            ("rebase-merge", "msgnum", "end"),
            ("rebase-apply", "next", "last"),
        ] {
            let base = dir.join(sub);
            if let (Some(c), Some(t)) = (read(base.join(current)), read(base.join(total))) {
                return Ok(Some((c, t)));
            }
        }
        Ok(None)
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

    fn take_side(&self, path: &str, ours: bool) -> Result<()> {
        let side = if ours { "--ours" } else { "--theirs" };
        match self.write(&["checkout", side, "--", path]) {
            Ok(_) => self.write(&["add", "--", path]).map(drop),
            // その側にファイルがない（片方で消されていた）。
            Err(_) => self.write(&["rm", "--quiet", "--", path]).map(drop),
        }
    }

    fn fetch(&self) -> Result<()> {
        // リモートで消えたブランチを片づけるかは、端末の `git fetch` と同じく `fetch.prune` に任せる。
        self.write(&["fetch", "--all"]).map(drop)
    }

    fn pull(&self, mode: PullMode) -> Result<Outcome> {
        let how = match mode {
            // git の設定（pull.rebase・pull.ff）に任せる。
            PullMode::Git => return self.stoppable(&["pull", "--no-edit"], &[]),
            PullMode::Merge => "--no-rebase",
            PullMode::Rebase => "--rebase",
            PullMode::FastForwardOnly => "--ff-only",
        };
        self.stoppable(&["pull", "--no-edit", how], &[])
    }

    fn branch_tracking(&self) -> Result<Vec<(String, u32, u32, bool)>> {
        let out = text(self.read(&[
            "for-each-ref",
            "--format=%(refname:short)%00%(upstream:track,nobracket)",
            "refs/heads",
        ])?)?;
        Ok(out
            .lines()
            .filter_map(|line| {
                let (name, track) = line.split_once('\0')?;
                // "ahead 1, behind 2"・"ahead 1"・"behind 2"・"gone"・""
                let mut ahead = 0;
                let mut behind = 0;
                for part in track.split(", ") {
                    if let Some(n) = part.strip_prefix("ahead ") {
                        ahead = n.parse().unwrap_or(0);
                    } else if let Some(n) = part.strip_prefix("behind ") {
                        behind = n.parse().unwrap_or(0);
                    }
                }
                Some((name.to_owned(), ahead, behind, track == "gone"))
            })
            .collect())
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

    fn stash_branch(&self, branch: &str, name: &str) -> Result<Outcome> {
        match self.write(&["stash", "branch", branch, name]) {
            Ok(_) => Ok(Outcome::Done),
            // 土台の上に戻すのでふつうは重ならないが、untracked のファイルなどで重なれば止まる。
            Err(e) => {
                let conflicted = self
                    .status()?
                    .entries
                    .iter()
                    .any(|e| e.kind == amonhen_model::StatusKind::Unmerged);
                if conflicted {
                    Ok(Outcome::Conflict)
                } else {
                    Err(e)
                }
            }
        }
    }

    fn count_apart(&self, left: &str, right: &str) -> Result<(u32, u32)> {
        let range = format!("{left}...{right}");
        let out = text(self.read(&["rev-list", "--left-right", "--count", &range])?)?;
        let mut counts = out.split_whitespace().map(|n| n.parse().unwrap_or(0));
        Ok((counts.next().unwrap_or(0), counts.next().unwrap_or(0)))
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

    fn config_list(&self) -> Result<Vec<ConfigEntry>> {
        let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
        // `origin\0key\nvalue\0`（値のない項目は `key\0`）。
        let entry = |scope: String, origin: &[u8], item: &[u8]| {
            let (key, value) = match item.iter().position(|&b| b == b'\n') {
                Some(i) => (text(&item[..i]), text(&item[i + 1..])),
                None => (text(item), String::new()),
            };
            ConfigEntry {
                scope,
                origin: text(origin),
                key,
                value,
            }
        };
        if crate::git_at_least(2, 26) {
            let out = self.read(&["config", "--list", "--show-origin", "--show-scope", "-z"])?;
            // 1 項目は `scope\0origin\0key\nvalue\0`。
            let fields: Vec<&[u8]> = out.split(|&b| b == 0).collect();
            return Ok(fields
                .as_chunks::<3>()
                .0
                .iter()
                .map(|f| entry(text(f[0]), f[1], f[2]))
                .collect());
        }
        // `--show-scope` は git 2.26 から。それより前は場所ごとに読む（ファイルがなければ読めないので、空とみなす）。
        let mut entries = Vec::new();
        for scope in ["system", "global", "local"] {
            let Ok(out) = self.read(&[
                "config",
                "--list",
                "--show-origin",
                "-z",
                &format!("--{scope}"),
            ]) else {
                continue;
            };
            let fields: Vec<&[u8]> = out.split(|&b| b == 0).collect();
            entries.extend(
                fields
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|f| entry(scope.to_owned(), f[0], f[1])),
            );
        }
        Ok(entries)
    }

    fn config_keys(&self) -> Result<Vec<String>> {
        // `git help --config-for-completion`（git 2.20 から）は 1 行 1 変数。`remote.` のように `.` で終わる行は
        // `remote.<name>.…` の形の変数のまとめで、キーではないので外す。
        let out = text(self.read(&["help", "--config-for-completion"])?)?;
        Ok(out
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.ends_with('.'))
            .map(str::to_owned)
            .collect())
    }

    fn config_set(&self, key: &str, value: Option<&str>) -> Result<()> {
        self.config_set_in("--local", key, value)
    }

    fn config_set_global(&self, key: &str, value: Option<&str>) -> Result<()> {
        self.config_set_in("--global", key, value)
    }

    fn git_version(&self) -> Result<Option<(u32, u32, u32)>> {
        Ok(crate::git_version())
    }

    fn signing_keys(&self, format: &str) -> Result<Vec<SigningKey>> {
        self.signing_keys_for(format)
    }

    fn remotes(&self) -> Result<Vec<RemoteEntry>> {
        let out = text(self.read(&["remote", "-v"])?)?;
        let mut remotes: Vec<RemoteEntry> = Vec::new();
        for line in out.lines() {
            // "origin\thttps://… (fetch)"
            let Some((name, rest)) = line.split_once('\t') else {
                continue;
            };
            let Some((url, kind)) = rest.rsplit_once(' ') else {
                continue;
            };
            let entry = match remotes.iter_mut().find(|r| r.name == name) {
                Some(e) => e,
                None => {
                    remotes.push(RemoteEntry {
                        name: name.to_owned(),
                        fetch_url: String::new(),
                        push_url: String::new(),
                    });
                    remotes.last_mut().unwrap()
                }
            };
            if kind == "(push)" {
                entry.push_url = url.to_owned();
            } else {
                entry.fetch_url = url.to_owned();
            }
        }
        Ok(remotes)
    }

    fn working_stats(&self) -> Result<Vec<amonhen_model::FileStat>> {
        // まだコミットがなければ、空の木と比べる。
        let base = if self
            .read(&["rev-parse", "--verify", "--quiet", "HEAD"])
            .is_ok()
        {
            "HEAD"
        } else {
            "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
        };
        let out = self.read(&["diff", "-z", "--numstat", "-M", base])?;
        Ok(crate::extras::parse_numstat(&out))
    }

    fn ignore_matches(
        &self,
        patterns: &[String],
        limit: usize,
    ) -> Result<Vec<(Vec<String>, usize)>> {
        patterns
            .iter()
            .map(|pattern| {
                let exclude = format!("--exclude={pattern}");
                let out =
                    self.read(&["ls-files", "-z", "-c", "-o", "-i", "--directory", &exclude])?;
                let all: Vec<&[u8]> = out.split(|&b| b == 0).filter(|p| !p.is_empty()).collect();
                let examples = all
                    .iter()
                    .take(limit)
                    .map(|p| String::from_utf8_lossy(p).into_owned())
                    .collect();
                Ok((examples, all.len()))
            })
            .collect()
    }

    fn change_remote(&self, change: &RemoteChange) -> Result<()> {
        let args: Vec<&str> = match change {
            RemoteChange::Add { name, url } => vec!["remote", "add", name, url],
            RemoteChange::Rename { from, to } => vec!["remote", "rename", from, to],
            RemoteChange::SetUrl { name, url } => vec!["remote", "set-url", name, url],
            RemoteChange::Remove { name } => vec!["remote", "remove", name],
            RemoteChange::Prune { name } => vec!["remote", "prune", name],
        };
        self.write(&args).map(drop)
    }

    fn worktree_list(&self) -> Result<Vec<u8>> {
        if crate::git_at_least(2, 36) {
            return self.read(&["worktree", "list", "--porcelain", "-z"]);
        }
        // `-z` は git 2.36 から。それより前は行の区切りを NUL に替えて同じ形にする（パスに改行があれば読めない）。
        let mut out = self.read(&["worktree", "list", "--porcelain"])?;
        for b in &mut out {
            if *b == b'\n' {
                *b = 0;
            }
        }
        Ok(out)
    }

    fn change_worktree(&self, change: &WorktreeChange) -> Result<()> {
        let args: Vec<&str> = match change {
            WorktreeChange::Add {
                path,
                start,
                new_branch,
            } => {
                let mut args = vec!["worktree", "add"];
                if let Some(branch) = new_branch {
                    args.extend(["-b", branch]);
                }
                args.extend(["--", path]);
                if let Some(start) = start {
                    args.push(start);
                }
                args
            }
            WorktreeChange::Remove { path, force } => {
                let mut args = vec!["worktree", "remove"];
                if *force {
                    args.extend(["--force", "--force"]);
                }
                args.extend(["--", path]);
                args
            }
            WorktreeChange::Prune => vec!["worktree", "prune"],
        };
        self.write(&args).map(drop)
    }

    fn worktree_changes(&self, paths: &[String], fresh: bool) -> Result<Vec<WorktreeChanges>> {
        // このリポジトリの worktree のパスだけを読む（ほかの場所は読まない）。
        let listed = self.worktree_list()?;
        let known: Vec<&[u8]> = listed
            .split(|&b| b == 0)
            .filter_map(|field| field.strip_prefix(b"worktree "))
            .collect();
        let paths: Vec<&String> = paths
            .iter()
            .filter(|p| known.contains(&p.as_bytes()))
            .collect();
        let mut counts = self.worktrees.lock().unwrap();
        counts.start_watching(self);
        // 数え直すもの: 初めて・HEAD か index が変わった・変更があって前に数えてから 30 秒たった（作業ツリーの
        // ファイルは見張らないので、変更のあるものはときどき数え直す）。
        let generations: Vec<Option<u64>> = paths.iter().map(|p| counts.generation(p)).collect();
        let due: Vec<usize> = (0..paths.len())
            .filter(|&i| fresh || counts.is_due(paths[i], generations[i]))
            .collect();
        drop(counts);
        // 1 つずつ git status を動かす。数が多いので 8 つまで並べる。
        let next = std::sync::atomic::AtomicUsize::new(0);
        let results: std::sync::Mutex<Vec<Option<(u32, u64)>>> =
            std::sync::Mutex::new(vec![None; due.len()]);
        std::thread::scope(|scope| {
            for _ in 0..due.len().min(8) {
                scope.spawn(|| {
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(&index) = due.get(i) else { break };
                        let changes = Git::new(paths[index].as_str()).count_changes();
                        results.lock().unwrap()[i] = changes.ok();
                    }
                });
            }
        });
        let mut counts = self.worktrees.lock().unwrap();
        for (i, result) in due.iter().zip(results.into_inner().unwrap()) {
            counts.record(paths[*i], generations[*i], result);
        }
        Ok(paths
            .iter()
            .map(|path| {
                let (count, stamp) = counts.get(path);
                WorktreeChanges {
                    path: (*path).clone(),
                    count,
                    stamp,
                }
            })
            .collect())
    }
}

/// ほかの worktree の変更の数を覚えておき、変わったものだけ数え直す（`amonhen_watch::WorktreeWatch` で HEAD と
/// index を見張る）。
#[derive(Default)]
pub struct WorktreeCounts {
    #[cfg(not(target_family = "wasm"))]
    watch: Option<amonhen_watch::WorktreeWatch>,
    /// 見張りを作ったときに `<git dir>/worktrees` があったか（なければ、できたら作り直す）。
    watching_worktrees: bool,
    /// パス → 数えたときの番号・数・指紋・最後に指紋が変わった時刻。
    known: std::collections::HashMap<String, Known>,
}

impl std::fmt::Debug for WorktreeCounts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorktreeCounts")
            .field("known", &self.known.len())
            .finish()
    }
}

struct Known {
    generation: Option<u64>,
    count: Option<u32>,
    stamp: u64,
    counted: std::time::Instant,
}

/// 変更のある worktree を数え直すまでの間（作業ツリーのファイルは見張らないので）。
const RECOUNT_DIRTY: std::time::Duration = std::time::Duration::from_secs(30);

impl WorktreeCounts {
    fn start_watching(&mut self, git: &Git) {
        #[cfg(not(target_family = "wasm"))]
        {
            let Ok(dir) = git.read(&["rev-parse", "--path-format=absolute", "--git-common-dir"])
            else {
                return;
            };
            let dir = std::path::PathBuf::from(String::from_utf8_lossy(&dir).trim());
            let has_worktrees = dir.join("worktrees").is_dir();
            if self.watch.is_some() && (self.watching_worktrees || !has_worktrees) {
                return;
            }
            self.watching_worktrees = has_worktrees;
            self.watch = amonhen_watch::WorktreeWatch::new(&dir).ok();
            // 作り直した見張りの番号は 0 から。前に数えたものは数え直す。
            self.known.clear();
        }
        #[cfg(target_family = "wasm")]
        let _ = git;
    }

    /// 見張りの番号（見張れなければ None）。
    fn generation(&self, path: &str) -> Option<u64> {
        #[cfg(not(target_family = "wasm"))]
        if let Some(watch) = &self.watch {
            return Some(watch.generation(&canonical(path)));
        }
        let _ = path;
        None
    }

    fn is_due(&self, path: &str, generation: Option<u64>) -> bool {
        let Some(known) = self.known.get(path) else {
            return true;
        };
        let Some(generation) = generation else {
            return true;
        };
        known.generation != Some(generation)
            || known.count.is_none()
            || (known.count.unwrap_or(0) > 0 && known.counted.elapsed() >= RECOUNT_DIRTY)
    }

    fn record(&mut self, path: &str, generation: Option<u64>, result: Option<(u32, u64)>) {
        let (count, stamp) = result.map_or((None, 0), |(c, s)| (Some(c), s));
        self.known.insert(
            path.to_owned(),
            Known {
                generation,
                count,
                stamp,
                counted: std::time::Instant::now(),
            },
        );
    }

    fn get(&self, path: &str) -> (Option<u32>, u64) {
        self.known
            .get(path)
            .map_or((None, 0), |k| (k.count, k.stamp))
    }
}

#[cfg(not(target_family = "wasm"))]
fn canonical(path: &str) -> std::path::PathBuf {
    let path = std::path::Path::new(path);
    path.canonicalize().unwrap_or_else(|_| path.to_owned())
}

impl Git {
    /// 変更のあるファイルの数（untracked も 1 つずつ。Working Copy の一覧と同じ数え方）と、status の指紋。
    fn count_changes(&self) -> Result<(u32, u64)> {
        use std::hash::{Hash, Hasher};
        let out = self.read(&[
            "-c",
            "core.untrackedCache=true",
            "-c",
            "status.showUntrackedFiles=all",
            "status",
            "--porcelain=v2",
            "-z",
            "--untracked-files=all",
        ])?;
        let mut count = 0;
        let mut fields = out.split(|&b| b == 0);
        while let Some(field) = fields.next() {
            match field.first() {
                Some(b'1' | b'u' | b'?') => count += 1,
                // 名前を変えたものは、続く元の名前を飛ばす。
                Some(b'2') => {
                    count += 1;
                    fields.next();
                }
                _ => {}
            }
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        out.hash(&mut hasher);
        Ok((count, hasher.finish()))
    }
}
