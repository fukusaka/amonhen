//! mailmap を読む（git と同じ所から同じ順に。gix の `open_mailmap`）。
//!
//! History の行を読むたびに呼ばれるので、読んだもとのファイル（`.mailmap`・`mailmap.file`・設定ファイル）の
//! 更新時刻と大きさ、`mailmap.blob`（bare なら `HEAD:.mailmap`）の指すオブジェクトが変わっていなければ、覚えて
//! いるものを返す（調べるのは stat が数回）。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use amonhen_model::{Mailmap, MailmapEntry, Result};
use gix::bstr::ByteSlice;

#[derive(Default)]
pub(crate) struct MailmapCache(Mutex<Option<Loaded>>);

struct Loaded {
    stamp: Stamp,
    /// 変わったかを調べるファイル。
    files: Vec<PathBuf>,
    /// 中身を読む blob の指定（`mailmap.blob`、bare で指定がなければ `HEAD:.mailmap`）。
    blob: Option<String>,
    mailmap: Arc<Mailmap>,
}

#[derive(PartialEq, Eq)]
struct Stamp {
    files: Vec<Option<(SystemTime, u64)>>,
    blob: Option<gix::ObjectId>,
}

fn stamp(repo: &gix::Repository, files: &[PathBuf], blob: Option<&str>) -> Stamp {
    Stamp {
        files: files
            .iter()
            .map(|f| {
                let m = std::fs::metadata(f).ok()?;
                Some((m.modified().ok()?, m.len()))
            })
            .collect(),
        blob: blob.and_then(|spec| repo.rev_parse_single(spec).ok().map(|id| id.detach())),
    }
}

/// 設定を読み直すため、開き直す（開いたままのリポジトリの設定は、開いたときのもの）。
fn load(from: &Path) -> Result<Loaded> {
    let repo = gix::ThreadSafeRepository::discover(from)?.to_thread_local();
    let config = repo.config_snapshot();
    let enabled = config.boolean("log.mailmap").unwrap_or(true);
    let mut files: Vec<PathBuf> = Vec::new();
    if let Some(workdir) = repo.workdir() {
        files.push(workdir.join(".mailmap"));
    }
    if let Ok(Some(file)) = config.trusted_path("mailmap.file") {
        files.push(file);
    }
    // 設定を読んだファイルと、まだなくても後から作られうるもの（リポジトリ・worktree・利用者の設定）。
    files.push(repo.common_dir().join("config"));
    files.push(repo.git_dir().join("config.worktree"));
    if let Some(home) = gix::path::env::home_dir() {
        files.push(home.join(".gitconfig"));
        files.push(home.join(".config/git/config"));
    }
    for section in config.plumbing().sections() {
        if let Some(path) = &section.meta().path {
            files.push(path.clone());
        }
    }
    files.sort();
    files.dedup();
    let blob = config
        .string("mailmap.blob")
        .map(|s| s.to_str_lossy().into_owned())
        .or_else(|| repo.workdir().is_none().then(|| "HEAD:.mailmap".to_owned()));
    let text = |b: &gix::bstr::BStr| b.to_str_lossy().into_owned();
    let entries = repo
        .open_mailmap()
        .iter()
        .map(|e| MailmapEntry {
            old_email: text(e.old_email()),
            old_name: e.old_name().map(text),
            new_name: e.new_name().map(text),
            new_email: e.new_email().map(text),
        })
        .collect();
    Ok(Loaded {
        stamp: stamp(&repo, &files, blob.as_deref()),
        files,
        blob,
        mailmap: Arc::new(Mailmap::new(enabled, entries)),
    })
}

impl MailmapCache {
    /// `repo` は開いたままのリポジトリ（blob を引くだけに使う）、`from` は開き直すときの場所。
    pub(crate) fn get(&self, repo: &gix::Repository, from: &Path) -> Result<Arc<Mailmap>> {
        let mut cached = self.0.lock().unwrap();
        if let Some(loaded) = cached.as_ref()
            && stamp(repo, &loaded.files, loaded.blob.as_deref()) == loaded.stamp
        {
            return Ok(loaded.mailmap.clone());
        }
        let loaded = load(from)?;
        let mailmap = loaded.mailmap.clone();
        *cached = Some(loaded);
        Ok(mailmap)
    }
}
