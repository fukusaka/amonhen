//! 履歴を書き換える操作の Undo。
//!
//! 操作の前後で HEAD と関係する ref を記録し、`<git_dir>/amonhen/undo.json` に残す。Undo するときは、
//! 今の状態が記録した「操作後」と一致することを確かめてから戻す。形式は VSCode 拡張版と同じで、
//! ファイルも共有する（どちらで行った操作も、どちらからでも Undo できる）。

use amonhen_model::tr;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use amonhen_model::{HeadState, Oid, Operations, RepoFiles, ResetMode, Result, Root};
use serde::{Deserialize, Serialize};

const MAX_ENTRIES: usize = 50;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Head {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    oid: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct RefSnapshot {
    head: Head,
    /// 完全な ref 名 → oid。なければ null。
    refs: BTreeMap<String, Option<String>>,
}

impl RefSnapshot {
    fn same(&self, other: &RefSnapshot) -> bool {
        self.head == other.head
            && self.refs.keys().chain(other.refs.keys()).all(|name| {
                self.refs.get(name).cloned().flatten() == other.refs.get(name).cloned().flatten()
            })
    }
}

/// 作業中のファイルの、操作の前後の内容（変更の破棄を Undo で戻すため）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct FileBackup {
    path: String,
    /// 操作の前の内容を保存した blob。ファイルがなければ null。
    before: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mode: Option<u32>,
    /// 操作の後の内容の sha1。Undo の前に、ほかで変わっていないかを確かめる。
    after: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct StashBackup {
    oid: String,
    message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Mode {
    Soft,
    /// reset --mixed の Undo（index を戻し、作業ツリーはそのまま）。
    Mixed,
    Keep,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    label: String,
    /// UNIX 時刻（ミリ秒）。
    time: u64,
    mode: Mode,
    before: RefSnapshot,
    after: RefSnapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    files: Option<Vec<FileBackup>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stash: Option<StashBackup>,
}

/// 記録した後に、ほかの操作でブランチや HEAD が動いている。
#[derive(Debug)]
pub struct UndoConflict(pub String);

impl std::fmt::Display for UndoConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&tr!(
            "Can’t undo: branches or HEAD changed after “{label}”",
            label = self.0
        ))
    }
}

impl std::error::Error for UndoConflict {}

/// 記録（`.git` からの相対）。
const LOG_FILE: &str = "amonhen/undo.json";
/// 途中で止まった操作（edit・split）の、操作の前の状態。rebase が終わったら確定する。
const PENDING_FILE: &str = "amonhen/undo-pending.json";

pub struct UndoLog {
    /// 記録と作業ツリーのファイルの読み書き（SSH 越しのリポジトリでも同じに扱う）。
    files: Arc<dyn RepoFiles>,
    /// 書き込みを 1 つずつにする（読み込み → 追記 → 保存）。
    lock: Mutex<()>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn sha1_hex(data: &[u8]) -> String {
    sha1_smol::Sha1::from(data).digest().to_string()
}

impl UndoLog {
    pub fn new(files: Arc<dyn RepoFiles>) -> Self {
        Self {
            files,
            lock: Mutex::new(()),
        }
    }

    /// 作業ツリーのファイルの中身のハッシュ。なければ `None`。
    fn content_hash(&self, path: &str) -> Result<Option<String>> {
        Ok(self.files.read(Root::Workdir, path)?.map(|d| sha1_hex(&d)))
    }

    fn load(&self) -> Vec<Entry> {
        self.files
            .read(Root::GitDir, LOG_FILE)
            .ok()
            .flatten()
            .and_then(|d| serde_json::from_slice(&d).ok())
            .unwrap_or_default()
    }

    fn save(&self, entries: &[Entry]) -> Result<()> {
        self.files
            .write(Root::GitDir, LOG_FILE, &serde_json::to_vec_pretty(entries)?)
    }

    fn push(&self, entry: Entry) -> Result<()> {
        let _guard = self.lock.lock().unwrap();
        let mut entries = self.load();
        entries.push(entry);
        let excess = entries.len().saturating_sub(MAX_ENTRIES);
        entries.drain(..excess);
        self.save(&entries)
    }

    /// 記録した操作の名前と時刻（UNIX 時刻のミリ秒）。新しい順。
    pub fn entries(&self) -> Vec<serde_json::Value> {
        self.load()
            .iter()
            .rev()
            .map(|e| serde_json::json!({ "label": e.label, "time": e.time }))
            .collect()
    }

    /// 次に Undo される操作の名前。
    pub fn peek(&self) -> Option<String> {
        self.load().last().map(|e| e.label.clone())
    }

    fn snapshot(ops: &dyn Operations, refs: &[String]) -> Result<RefSnapshot> {
        let HeadState { branch, oid } = ops.head_state()?;
        Ok(RefSnapshot {
            head: Head {
                branch,
                oid: oid.map(|o| o.to_string()),
            },
            refs: ops
                .resolve_refs(refs)?
                .into_iter()
                .map(|(n, o)| (n, o.map(|o| o.to_string())))
                .collect(),
        })
    }

    /// `f` を実行し、成功して ref が動いたら記録する。`refs` は動きうる ref の完全な名前
    /// （HEAD のブランチは自動で含める）。`skip` が真を返したら記録しない（コンフリクトで止まったときなど）。
    pub fn record<T>(
        &self,
        ops: &dyn Operations,
        label: &str,
        refs: &[String],
        soft: bool,
        f: impl FnOnce() -> Result<T>,
        skip: impl FnOnce(&T) -> bool,
    ) -> Result<T> {
        let mode = if soft { Mode::Soft } else { Mode::Keep };
        self.record_mode(ops, label, refs, mode, f, skip)
    }

    /// 今のブランチ（または detached HEAD）を `mode` で動かす reset を実行し、記録する。Undo は同じ種類の reset
    /// で戻す（hard は作業中の変更を残す keep で。消した変更は戻らない）。
    pub fn record_reset(
        &self,
        ops: &dyn Operations,
        label: &str,
        mode: ResetMode,
        f: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        let mode = match mode {
            ResetMode::Soft => Mode::Soft,
            ResetMode::Mixed => Mode::Mixed,
            ResetMode::Hard | ResetMode::Keep => Mode::Keep,
        };
        self.record_mode(ops, label, &[], mode, f, |_| false)
    }

    fn record_mode<T>(
        &self,
        ops: &dyn Operations,
        label: &str,
        refs: &[String],
        mode: Mode,
        f: impl FnOnce() -> Result<T>,
        skip: impl FnOnce(&T) -> bool,
    ) -> Result<T> {
        let mut refs = refs.to_vec();
        if let Some(branch) = ops.head_state()?.branch
            && !refs.contains(&branch)
        {
            refs.push(branch);
        }
        let before = Self::snapshot(ops, &refs)?;
        let result = f()?;
        if skip(&result) {
            return Ok(result);
        }
        let after = Self::snapshot(ops, &refs)?;
        if !before.same(&after) {
            self.push(Entry {
                label: label.to_owned(),
                time: now_ms(),
                mode,
                before,
                after,
                files: None,
                stash: None,
            })?;
        }
        Ok(result)
    }

    /// `record` と同じだが、`pending` が真を返したら操作の前の状態だけを保留しておき、
    /// `settle_pending` で確定する（edit や split で rebase が途中で止まったとき）。
    pub fn record_stoppable<T>(
        &self,
        ops: &dyn Operations,
        label: &str,
        f: impl FnOnce() -> Result<T>,
        skip: impl FnOnce(&T) -> bool,
        pending: impl FnOnce(&T) -> bool,
    ) -> Result<T> {
        let refs: Vec<String> = ops.head_state()?.branch.into_iter().collect();
        let before = Self::snapshot(ops, &refs)?;
        let result = f()?;
        if pending(&result) {
            #[derive(Serialize)]
            struct Pending<'a> {
                label: &'a str,
                time: u64,
                mode: Mode,
                before: &'a RefSnapshot,
            }
            let p = Pending {
                label,
                time: now_ms(),
                mode: Mode::Keep,
                before: &before,
            };
            self.files
                .write(Root::GitDir, PENDING_FILE, &serde_json::to_vec_pretty(&p)?)?;
            return Ok(result);
        }
        if skip(&result) {
            return Ok(result);
        }
        let after = Self::snapshot(ops, &refs)?;
        if !before.same(&after) {
            self.push(Entry {
                label: label.to_owned(),
                time: now_ms(),
                mode: Mode::Keep,
                before,
                after,
                files: None,
                stash: None,
            })?;
        }
        Ok(result)
    }

    /// 保留している操作があり、rebase などがもう終わっていれば確定する。abort した場合は
    /// 操作の前と同じ状態なので、記録は残らない。確定したら true。
    pub fn settle_pending(&self, ops: &dyn Operations) -> Result<bool> {
        #[derive(Deserialize)]
        struct Pending {
            label: String,
            time: u64,
            mode: Mode,
            before: RefSnapshot,
        }
        let Some(pending) = self
            .files
            .read(Root::GitDir, PENDING_FILE)?
            .and_then(|d| serde_json::from_slice::<Pending>(&d).ok())
        else {
            return Ok(false);
        };
        if ops.sequence_in_progress()?.is_some() {
            return Ok(false);
        }
        let refs: Vec<String> = pending.before.refs.keys().cloned().collect();
        let after = Self::snapshot(ops, &refs)?;
        let changed = !pending.before.same(&after);
        if changed {
            self.push(Entry {
                label: pending.label,
                time: pending.time,
                mode: pending.mode,
                before: pending.before,
                after,
                files: None,
                stash: None,
            })?;
        }
        self.files.remove(Root::GitDir, PENDING_FILE)?;
        Ok(changed)
    }

    /// 作業中のファイルを書き換える操作（変更の破棄）を実行し、変わったファイルの前の内容を記録する。
    pub fn record_files<T>(
        &self,
        ops: &dyn Operations,
        label: &str,
        paths: &[&str],
        f: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        let mut before = Vec::new();
        for path in paths {
            let data = self.files.read(Root::Workdir, path)?;
            let mode = match data {
                Some(_) => self.files.mode(Root::Workdir, path)?,
                None => None,
            };
            let blob = data.as_deref().map(|d| ops.write_blob(d)).transpose()?;
            before.push((path.to_string(), data, mode, blob));
        }
        let result = f()?;
        let mut files = Vec::new();
        for (path, data, mode, blob) in before {
            let after = self.content_hash(&path)?;
            if after != data.as_deref().map(sha1_hex) {
                files.push(FileBackup {
                    path,
                    before: blob.map(|b| b.to_string()),
                    mode,
                    after,
                });
            }
        }
        if !files.is_empty() {
            let snapshot = Self::snapshot(ops, &[])?;
            self.push(Entry {
                label: label.to_owned(),
                time: now_ms(),
                mode: Mode::Keep,
                before: snapshot.clone(),
                after: snapshot,
                files: Some(files),
                stash: None,
            })?;
        }
        Ok(result)
    }

    /// stash を消し、Undo で戻せるように記録する。
    pub fn record_stash_drop(
        &self,
        ops: &dyn Operations,
        name: &str,
        oid: &Oid,
        message: &str,
    ) -> Result<()> {
        ops.stash_drop(name)?;
        let snapshot = Self::snapshot(ops, &[])?;
        self.push(Entry {
            label: format!("Drop {message}"),
            time: now_ms(),
            mode: Mode::Keep,
            before: snapshot.clone(),
            after: snapshot,
            files: None,
            stash: Some(StashBackup {
                oid: oid.to_string(),
                message: message.to_owned(),
            }),
        })
    }

    /// 最後の操作を戻す。戻した操作の名前を返す。
    pub fn undo(&self, ops: &dyn Operations) -> Result<Option<String>> {
        let _guard = self.lock.lock().unwrap();
        let mut entries = self.load();
        let Some(entry) = entries.last().cloned() else {
            return Ok(None);
        };
        let oid = |s: &str| {
            Oid::from_hex(s).ok_or_else(|| tr!("Can’t read the recorded oid: {oid}", oid = s))
        };

        if let Some(stash) = &entry.stash {
            ops.stash_store(&oid(&stash.oid)?, &stash.message)?;
        } else if let Some(files) = &entry.files {
            self.undo_files(ops, &entry, files)?;
        } else {
            let refs: Vec<String> = entry.after.refs.keys().cloned().collect();
            if !Self::snapshot(ops, &refs)?.same(&entry.after) {
                return Err(UndoConflict(entry.label.clone()).into());
            }
            self.undo_refs(ops, &entry)?;
        }
        entries.pop();
        self.save(&entries)?;
        Ok(Some(entry.label))
    }

    fn undo_refs(&self, ops: &dyn Operations, entry: &Entry) -> Result<()> {
        let (before, after) = (&entry.before, &entry.after);
        let mode = match entry.mode {
            Mode::Soft => ResetMode::Soft,
            Mode::Mixed => ResetMode::Mixed,
            Mode::Keep => ResetMode::Keep,
        };
        let parse = |s: &Option<String>| s.as_deref().and_then(Oid::from_hex);
        let mut done = Vec::new();
        // 1. いまチェックアウトしているブランチ（または detached HEAD）を、作業中の変更を残したまま戻す。
        if let Some(branch) = &after.head.branch {
            // 操作の中で作ったブランチ（前は なし）は、手順 2 で離れてから手順 3 で消す。
            if let Some(old) = parse(before.refs.get(branch).unwrap_or(&None)) {
                if Some(old) != parse(after.refs.get(branch).unwrap_or(&None)) {
                    ops.reset(mode, &old)?;
                }
                done.push(branch.clone());
            }
        } else if before.head.branch.is_none()
            && let Some(old) = parse(&before.head.oid)
            && before.head.oid != after.head.oid
        {
            ops.reset(mode, &old)?;
        }
        // 2. 操作の中で別のブランチに切り替えていたら、元に戻る。
        if before.head.oid.is_some() && before.head.branch != after.head.branch {
            ops.switch_to(&HeadState {
                branch: before.head.branch.clone(),
                oid: parse(&before.head.oid),
            })?;
        }
        // 3. 残りの ref を戻す。
        for (name, value) in &before.refs {
            let current = after.refs.get(name).cloned().flatten();
            if done.contains(name) || *value == current {
                continue;
            }
            let value = parse(value);
            if Some(name) == before.head.branch.as_ref()
                && let Some(v) = value
            {
                // チェックアウト中のブランチは update-ref だと作業ツリーとずれるので reset で戻す。
                ops.reset(mode, &v)?;
            } else {
                ops.update_ref(name, value.as_ref(), parse(&current).as_ref())?;
            }
        }
        Ok(())
    }

    /// ファイルを操作の前の内容に戻す。ほかで書き換えられていたら何もしない。
    fn undo_files(&self, ops: &dyn Operations, entry: &Entry, files: &[FileBackup]) -> Result<()> {
        for f in files {
            if self.content_hash(&f.path)? != f.after {
                return Err(UndoConflict(entry.label.clone()).into());
            }
        }
        for f in files {
            let Some(before) = &f.before else {
                let _ = self.files.remove(Root::Workdir, &f.path);
                continue;
            };
            let blob = Oid::from_hex(before).ok_or_else(|| tr!("Can’t read the recorded blob"))?;
            self.files
                .write(Root::Workdir, &f.path, &ops.read_blob(&blob)?)?;
            if let Some(mode) = f.mode {
                self.files.set_mode(Root::Workdir, &f.path, mode & 0o777)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use amonhen_backend_cli::Git;
    use amonhen_model::Outcome;

    fn repo() -> (tempfile::TempDir, Git, UndoLog) {
        let tmp = tempfile::tempdir().unwrap();
        let git = Git::new(tmp.path());
        for args in [
            &["init", "-q", "-b", "main"][..],
            &["config", "user.name", "a"],
            &["config", "user.email", "a@example.com"],
            // Windows の CI では autocrlf が有効で、取り出したファイルの改行が変わる。
            &["config", "core.autocrlf", "false"],
        ] {
            git.write(args).unwrap();
        }
        let log = UndoLog::new(amonhen_model::LocalFiles::new(
            tmp.path().join(".git"),
            Some(tmp.path().to_owned()),
        ));
        (tmp, git, log)
    }

    fn commit(tmp: &tempfile::TempDir, git: &Git, name: &str, content: &str) -> Oid {
        std::fs::write(tmp.path().join(name), content).unwrap();
        git.stage(&[name]).unwrap();
        git.commit(content, false).unwrap();
        git.head_state().unwrap().oid.unwrap()
    }

    #[test]
    fn undoes_a_commit_softly_and_a_merge_with_keep() {
        let (tmp, git, log) = repo();
        let first = commit(&tmp, &git, "a.txt", "1\n");

        std::fs::write(tmp.path().join("a.txt"), "2\n").unwrap();
        git.stage(&["a.txt"]).unwrap();
        log.record(
            &git,
            "Commit",
            &[],
            true,
            || git.commit("2", false),
            |_| false,
        )
        .unwrap();
        assert_eq!(log.peek().as_deref(), Some("Commit"));
        assert_eq!(log.undo(&git).unwrap().as_deref(), Some("Commit"));
        assert_eq!(git.head_state().unwrap().oid, Some(first));
        assert!(
            git.status().unwrap().entries[0].is_staged(),
            "コミットした変更は index に戻る"
        );
        git.commit("2", false).unwrap();

        git.create_branch("topic", "main", true).unwrap();
        commit(&tmp, &git, "b.txt", "b\n");
        git.switch_branch("main").unwrap();
        let before_merge = git.head_state().unwrap().oid.unwrap();
        let merged = log
            .record(
                &git,
                "Merge topic",
                &[],
                false,
                || git.merge("topic"),
                |o| *o != Outcome::Done,
            )
            .unwrap();
        assert_eq!(merged, Outcome::Done);
        log.undo(&git).unwrap();
        assert_eq!(git.head_state().unwrap().oid, Some(before_merge));
        assert!(git.status().unwrap().entries.is_empty());
        assert_eq!(log.peek(), None);
    }

    #[test]
    fn refuses_when_the_branch_moved_since() {
        let (tmp, git, log) = repo();
        commit(&tmp, &git, "a.txt", "1\n");
        std::fs::write(tmp.path().join("a.txt"), "2\n").unwrap();
        git.stage(&["a.txt"]).unwrap();
        log.record(
            &git,
            "Commit",
            &[],
            true,
            || git.commit("2", false),
            |_| false,
        )
        .unwrap();
        commit(&tmp, &git, "a.txt", "3\n");
        let err = log.undo(&git).unwrap_err();
        assert!(err.downcast_ref::<UndoConflict>().is_some());
    }

    #[test]
    fn restores_discarded_files_and_dropped_stashes() {
        let (tmp, git, log) = repo();
        commit(&tmp, &git, "a.txt", "1\n");
        std::fs::write(tmp.path().join("a.txt"), "edited\n").unwrap();
        log.record_files(&git, "Discard a.txt", &["a.txt"], || {
            git.restore_worktree(&["a.txt"])
        })
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
            "1\n"
        );
        log.undo(&git).unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
            "edited\n"
        );

        git.stash_push(Some("wip"), false).unwrap();
        let oid = Oid::from_hex(
            String::from_utf8(git.read(&["rev-parse", "stash@{0}"]).unwrap())
                .unwrap()
                .trim(),
        )
        .unwrap();
        log.record_stash_drop(&git, "stash@{0}", &oid, "wip")
            .unwrap();
        log.undo(&git).unwrap();
        assert!(
            git.read(&["rev-parse", "-q", "--verify", "refs/stash"])
                .is_ok()
        );
    }
}
