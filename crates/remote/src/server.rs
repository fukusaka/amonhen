//! daemon の側: 要求を読み、手元の `GitBackend` で実行して応答を書く。要求はそれぞれ別のスレッドで処理し、
//! 応答は番号で対応づける（重い要求が軽い要求を待たせない）。

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use amonhen_model::{GitBackend, Oid, Result};
use serde::Serialize;

use crate::frame::{self, Kind};
use crate::wire::{
    Bytes, CommitTextWire, GraphWire, Hello, Notification, Op, PROTOCOL_VERSION, Request,
};

type Writer = Arc<Mutex<Box<dyn Write + Send>>>;

/// 検索のために数えたコミット（同じ起点で何度も探すときに読み直さない）。
type CommitList = Mutex<Option<(Vec<Oid>, Arc<Vec<Oid>>)>>;

pub struct Server {
    backend: Arc<dyn GitBackend>,
    commits: Arc<CommitList>,
    writer: Writer,
    /// 走っている要求の、取り消しの印。
    cancels: Arc<Mutex<HashMap<u64, Arc<AtomicBool>>>>,
}

impl Server {
    pub fn new(backend: Arc<dyn GitBackend>, output: impl Write + Send + 'static) -> Self {
        Self {
            backend,
            writer: Arc::new(Mutex::new(Box::new(output))),
            cancels: Arc::default(),
            commits: Arc::default(),
        }
    }

    /// 知らせを送る関数（ファイルの監視から呼ぶ）。
    pub fn notifier(&self) -> impl Fn(Notification) + Send + Sync + 'static {
        let writer = self.writer.clone();
        move |n| {
            if let Ok(payload) = postcard::to_allocvec(&n) {
                let _ = frame::write(&mut *writer.lock().unwrap(), Kind::Notify, 0, &payload);
            }
        }
    }

    /// 相手が閉じるまで要求を処理する。
    pub fn run(&self, mut input: impl Read) -> io::Result<()> {
        while let Some(f) = frame::read(&mut input)? {
            match f.kind {
                Kind::Request => {
                    let request: Request = match postcard::from_bytes(&f.payload) {
                        Ok(r) => r,
                        Err(e) => {
                            self.reply(f.id, Err(format!("要求を読めない: {e}").into()));
                            continue;
                        }
                    };
                    let cancel = Arc::new(AtomicBool::new(false));
                    self.cancels.lock().unwrap().insert(f.id, cancel.clone());
                    let backend = self.backend.clone();
                    let writer = self.writer.clone();
                    let cancels = self.cancels.clone();
                    let commits = self.commits.clone();
                    std::thread::spawn(move || {
                        let result = handle(&*backend, &commits, request, &cancel);
                        cancels.lock().unwrap().remove(&f.id);
                        write_reply(&writer, f.id, result);
                    });
                }
                Kind::Cancel => {
                    if let Some(c) = self.cancels.lock().unwrap().get(&f.id) {
                        c.store(true, Ordering::Relaxed);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn reply(&self, id: u64, result: Result<Vec<u8>>) {
        write_reply(&self.writer, id, result);
    }
}

fn write_reply(writer: &Writer, id: u64, result: Result<Vec<u8>>) {
    let mut out = writer.lock().unwrap();
    let _ = match result {
        Ok(payload) => frame::write(&mut *out, Kind::Ok, id, &payload),
        Err(e) => frame::write(&mut *out, Kind::Err, id, e.to_string().as_bytes()),
    };
}

fn strs(v: &[String]) -> Vec<&str> {
    v.iter().map(String::as_str).collect()
}

fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    Ok(postcard::to_allocvec(value)?)
}

/// `tips` から辿れるコミット（並びは問わない）。前回と同じ起点なら数え直さない。
fn commits_from(
    backend: &dyn GitBackend,
    cache: &CommitList,
    tips: &[Oid],
) -> Result<Arc<Vec<Oid>>> {
    if let Some((cached_tips, ids)) = &*cache.lock().unwrap()
        && cached_tips == tips
    {
        return Ok(ids.clone());
    }
    let ids: Arc<Vec<Oid>> = Arc::new(
        backend
            .read_commits(tips, &|_| false)?
            .into_iter()
            .map(|c| c.id)
            .collect(),
    );
    *cache.lock().unwrap() = Some((tips.to_vec(), ids.clone()));
    Ok(ids)
}

fn handle(
    backend: &dyn GitBackend,
    commits: &CommitList,
    request: Request,
    cancel: &AtomicBool,
) -> Result<Vec<u8>> {
    let files = || backend.files();
    match request {
        Request::Hello { version } => {
            if version != PROTOCOL_VERSION {
                return Err(format!(
                    "daemon と通信の形が違います（手元 {version}、daemon {PROTOCOL_VERSION}）。同じ版の amonhend を入れてください"
                )
                .into());
            }
            encode(&Hello {
                version: PROTOCOL_VERSION,
            })
        }
        Request::Head => encode(&backend.head()?),
        Request::Tips => encode(&backend.tips()?),
        Request::NewCommits { tips, known_tips } => {
            let commits = if known_tips.is_empty() {
                backend.read_commits(&tips, &|_| false)?
            } else {
                backend.commits_since(&tips, &known_tips)?
            };
            encode(&commits)
        }
        Request::CommitGraph { tips } => encode(&GraphWire::from(&backend.commit_graph(&tips)?)),
        Request::Paths => encode(&backend.paths()),
        Request::Refs => encode(&backend.refs()?),
        Request::CommitChanges(id) => encode(&backend.commit_changes(&id)?),
        Request::StashChanges(id) => encode(&backend.stash_changes(&id)?),
        Request::Blob(id) => encode(&Bytes(backend.blob(&id)?.to_vec())),
        Request::Status => encode(&backend.status()?),
        Request::WorktreeFile(path) => encode(&Bytes(backend.worktree_file(&path)?.to_vec())),
        Request::Stage(paths) => encode(&backend.stage(&strs(&paths))?),
        Request::Unstage(paths) => encode(&backend.unstage(&strs(&paths))?),
        Request::SetIndexContent {
            path,
            mode,
            content,
        } => encode(&backend.set_index_content(&path, mode, &content.0)?),
        Request::Commit { message, amend } => encode(&backend.commit(&message, amend)?),
        Request::HasCommitGraph => encode(&backend.has_commit_graph()?),
        Request::WriteCommitGraph => encode(&backend.write_commit_graph()?),
        Request::CommitTexts(ids) => {
            let texts: Vec<CommitTextWire> =
                backend.commit_texts(&ids)?.iter().map(Into::into).collect();
            encode(&texts)
        }
        Request::CommitSummaries(ids) => {
            let texts: Vec<CommitTextWire> = backend
                .commit_texts(&ids)?
                .iter()
                .map(CommitTextWire::summary)
                .collect();
            encode(&texts)
        }
        Request::CommitExtras(id) => encode(&backend.commit_extras(&id)?),
        Request::SearchTexts { tips, terms } => {
            let ids = commits_from(backend, commits, &tips)?;
            let words = amonhen_model::search::scan(
                &ids,
                &terms,
                &|batch| backend.commit_texts(batch),
                &|| cancel.load(Ordering::Relaxed),
            )?;
            let found: Vec<Vec<Oid>> = words
                .iter()
                .map(|w| {
                    (0..ids.len())
                        .filter(|&i| w[i / 64] & (1 << (i % 64)) != 0)
                        .map(|i| ids[i])
                        .collect()
                })
                .collect();
            encode(&found)
        }
        Request::FileRead { root, path } => encode(&files().read(root, &path)?.map(Bytes)),
        Request::FileWrite { root, path, data } => encode(&files().write(root, &path, &data.0)?),
        Request::FileRemove { root, path } => encode(&files().remove(root, &path)?),
        Request::FileMode { root, path } => encode(&files().mode(root, &path)?),
        Request::FileSetMode { root, path, mode } => encode(&files().set_mode(root, &path, mode)?),
        Request::Op(op) => handle_op(backend, op, cancel),
    }
}

fn handle_op(backend: &dyn GitBackend, op: Op, cancel: &AtomicBool) -> Result<Vec<u8>> {
    let ops = backend.operations()?;
    match op {
        Op::LogMatching {
            tips,
            path,
            contents,
        } => encode(
            &ops.log_matching(&tips, path.as_deref(), contents.as_deref(), &|| {
                cancel.load(Ordering::Relaxed)
            })?,
        ),
        Op::Blame { path, rev } => encode(&Bytes(ops.blame(&path, rev.as_ref())?)),
        Op::FileLog { path } => encode(&Bytes(ops.file_log(&path)?)),
        Op::HeadState => encode(&ops.head_state()?),
        Op::ResolveRefs(names) => encode(&ops.resolve_refs(&names)?),
        Op::UpdateRef {
            name,
            value,
            expected,
        } => encode(&ops.update_ref(&name, value.as_ref(), expected.as_ref())?),
        Op::Reset { mode, oid } => encode(&ops.reset(mode, &oid)?),
        Op::SwitchTo(head) => encode(&ops.switch_to(&head)?),
        Op::WriteBlob(content) => encode(&ops.write_blob(&content.0)?),
        Op::ReadBlob(oid) => encode(&Bytes(ops.read_blob(&oid)?)),
        Op::SwitchBranch(name) => encode(&ops.switch_branch(&name)?),
        Op::TrackBranch(name) => encode(&ops.track_branch(&name)?),
        Op::CreateBranch {
            name,
            start,
            checkout,
        } => encode(&ops.create_branch(&name, &start, checkout)?),
        Op::DeleteBranch { name, force } => encode(&ops.delete_branch(&name, force)?),
        Op::RenameBranch { from, to } => encode(&ops.rename_branch(&from, &to)?),
        Op::CreateTag {
            name,
            target,
            message,
        } => encode(&ops.create_tag(&name, &target, message.as_deref())?),
        Op::CheckBranchName(name) => encode(&ops.check_branch_name(&name)?),
        Op::IsAncestor {
            ancestor,
            descendant,
        } => encode(&ops.is_ancestor(&ancestor, &descendant)?),
        Op::Resolve(rev) => encode(&ops.resolve(&rev)?),
        Op::Merge(rev) => encode(&ops.merge(&rev)?),
        Op::Rebase(onto) => encode(&ops.rebase(&onto)?),
        Op::CherryPick { oid, is_merge } => encode(&ops.cherry_pick(&oid, is_merge)?),
        Op::Revert { oid, is_merge } => encode(&ops.revert(&oid, is_merge)?),
        Op::RebaseTodo { base, todo } => encode(&ops.rebase_todo(base.as_ref(), &todo)?),
        Op::SequenceInProgress => encode(&ops.sequence_in_progress()?),
        Op::RebaseProgress => encode(&ops.rebase_progress()?),
        Op::Abort(s) => encode(&ops.abort(s)?),
        Op::ContinueSequence(s) => encode(&ops.continue_sequence(s)?),
        Op::SkipSequence(s) => encode(&ops.skip_sequence(s)?),
        Op::TakeSide { path, ours } => encode(&ops.take_side(&path, ours)?),
        Op::Fetch => encode(&ops.fetch()?),
        Op::Pull => encode(&ops.pull()?),
        Op::BranchTracking => encode(&ops.branch_tracking()?),
        Op::Push {
            remote,
            branch,
            remote_branch,
            force,
            set_upstream,
        } => encode(&ops.push(&remote, &branch, &remote_branch, force, set_upstream)?),
        Op::RestoreWorktree(paths) => encode(&ops.restore_worktree(&strs(&paths))?),
        Op::StashPush {
            message,
            include_untracked,
        } => encode(&ops.stash_push(message.as_deref(), include_untracked)?),
        Op::StashApply { name, pop } => encode(&ops.stash_apply(&name, pop)?),
        Op::StashDrop(name) => encode(&ops.stash_drop(&name)?),
        Op::StashStore { oid, message } => encode(&ops.stash_store(&oid, &message)?),
        Op::UpdateSubmodules(paths) => encode(&ops.update_submodules(&strs(&paths))?),
    }
}
