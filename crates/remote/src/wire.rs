//! 要求と知らせの形。応答の中身は要求ごとに決まった型を postcard で書いたもの（コメントに書く）。

use std::path::PathBuf;
use std::sync::Arc;

use amonhen_layout::{Pod, Slab};
use amonhen_model::search::TextTerm;
use amonhen_model::{
    CommitGraph, CommitText, HeadState, Oid, PullMode, RemoteChange, ResetMode, Result, Root,
    Sequence,
};
use serde::{Deserialize, Serialize};

/// 形を変えたら上げる。daemon と手元で違えば、つないだところで断る。
pub const PROTOCOL_VERSION: u32 = 12;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Request {
    /// → `Hello`
    Hello {
        version: u32,
    },
    // GitBackend
    /// → `Option<Oid>`
    Head,
    /// → `Vec<Oid>`
    Tips,
    /// → `Vec<RawCommit>`（`tips` から辿れ、`known_tips` から辿れないもの）
    NewCommits {
        tips: Vec<Oid>,
        known_tips: Vec<Oid>,
    },
    /// → `GraphWire`（`tips` から辿れるすべて、トポロジカル順）
    CommitGraph {
        tips: Vec<Oid>,
    },
    /// → `RepoPaths`
    Paths,
    /// → `Refs`
    Refs,
    /// → `Vec<FileChange>`
    CommitChanges(Oid),
    /// → `Vec<FileChange>`
    StashChanges(Oid),
    ChangesBetween(Option<Oid>, Oid),
    /// → `Bytes`
    Blob(Oid),
    /// → `Status`
    Status,
    /// → `Bytes`
    WorktreeFile(String),
    /// → `()`
    Stage(Vec<String>),
    /// → `()`
    Unstage(Vec<String>),
    /// → `()`
    SetIndexContent {
        path: String,
        mode: u32,
        content: Bytes,
    },
    /// → `()`
    Commit {
        message: String,
        amend: bool,
    },
    /// → `bool`
    HasCommitGraph,
    /// → `()`
    WriteCommitGraph,
    /// → `Vec<CommitTextWire>`
    CommitTexts(Vec<Oid>),
    /// → `Vec<CommitTextWire>`（`raw` は件名と作者名だけ）
    CommitSummaries(Vec<Oid>),
    /// → `CommitExtras`
    CommitExtras(Oid),
    /// → `Vec<SignatureCheck>`（`ids` と同じ順）
    VerifySignatures(Vec<Oid>),
    /// → `Vec<Vec<Oid>>`（条件ごとに合ったコミット。取り消せる）
    SearchTexts {
        tips: Vec<Oid>,
        terms: Vec<TextTerm>,
    },
    // RepoFiles
    /// → `Option<Bytes>`
    FileRead {
        root: Root,
        path: String,
    },
    /// → `()`
    FileWrite {
        root: Root,
        path: String,
        data: Bytes,
    },
    /// → `()`
    FileRemove {
        root: Root,
        path: String,
    },
    /// → `Option<u32>`
    FileMode {
        root: Root,
        path: String,
    },
    /// → `()`
    FileSetMode {
        root: Root,
        path: String,
        mode: u32,
    },
    /// Operations（応答は `Op` のコメント）
    Op(Op),
}

/// 書き込み系の操作。応答の型はそれぞれの `Operations` のメソッドの戻り値。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Op {
    LogMatching {
        tips: Vec<Oid>,
        path: Option<String>,
        contents: Option<String>,
    },
    Blame {
        path: String,
        rev: Option<Oid>,
    },
    FileLog {
        path: String,
    },
    HeadState,
    ResolveRefs(Vec<String>),
    UpdateRef {
        name: String,
        value: Option<Oid>,
        expected: Option<Oid>,
    },
    Reset {
        mode: ResetMode,
        oid: Oid,
    },
    SwitchTo(HeadState),
    WriteBlob(Bytes),
    ReadBlob(Oid),
    SwitchBranch(String),
    TrackBranch(String),
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
    CheckBranchName(String),
    IsAncestor {
        ancestor: String,
        descendant: String,
    },
    Resolve(String),
    Merge(String),
    Rebase(String),
    CherryPick {
        oid: Oid,
        is_merge: bool,
    },
    Revert {
        oid: Oid,
        is_merge: bool,
    },
    RebaseTodo {
        base: Option<Oid>,
        todo: PathBuf,
    },
    SequenceInProgress,
    RebaseProgress,
    Abort(Sequence),
    ContinueSequence(Sequence),
    SkipSequence(Sequence),
    TakeSide {
        path: String,
        ours: bool,
    },
    Fetch,
    Pull(PullMode),
    BranchTracking,
    Push {
        remote: String,
        branch: String,
        remote_branch: String,
        force: bool,
        set_upstream: bool,
    },
    RestoreWorktree(Vec<String>),
    StashPush {
        message: Option<String>,
        include_untracked: bool,
    },
    StashApply {
        name: String,
        pop: bool,
    },
    StashDrop(String),
    StashStore {
        oid: Oid,
        message: String,
    },
    UpdateSubmodules(Vec<String>),
    ConfigList,
    ConfigSet(String, Option<String>),
    Remotes,
    ChangeRemote(RemoteChange),
    WorkingStats,
    IgnoreMatches(Vec<String>, usize),
    Activity,
    CancelActivity(u64),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hello {
    pub version: u32,
}

/// daemon からの知らせ。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Notification {
    /// リポジトリが変わった（`amonhen_watch::Changes` のビット）。
    Changed(u32),
}

/// バイト列（serde の既定の「u8 の並び」ではなく、まとめて書く）。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bytes(#[serde(with = "serde_bytes")] pub Vec<u8>);

/// コミットの本文。範囲は `raw` の中のバイト位置。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommitTextWire {
    pub raw: Bytes,
    pub subject: (u32, u32),
    pub author_name: (u32, u32),
    pub author_time: i64,
    pub signature: u8,
}

impl From<&CommitText> for CommitTextWire {
    fn from(t: &CommitText) -> Self {
        Self {
            raw: Bytes(t.raw.to_vec()),
            subject: (t.subject.start, t.subject.end),
            author_name: (t.author_name.start, t.author_name.end),
            author_time: t.author_time,
            signature: t.signature,
        }
    }
}

impl CommitTextWire {
    /// 件名と作者名だけを残した本文（History の行に出す分）。
    pub fn summary(t: &CommitText) -> Self {
        let mut raw = t.subject().to_vec();
        let subject = (0, raw.len() as u32);
        raw.extend_from_slice(t.author_name());
        Self {
            raw: Bytes(raw.clone()),
            subject,
            author_name: (subject.1, raw.len() as u32),
            author_time: t.author_time,
            signature: t.signature,
        }
    }
}

impl From<CommitTextWire> for CommitText {
    fn from(w: CommitTextWire) -> Self {
        Self {
            raw: Arc::new(w.raw.0),
            subject: w.subject.0..w.subject.1,
            author_name: w.author_name.0..w.author_name.1,
            author_time: w.author_time,
            signature: w.signature,
        }
    }
}

/// トポロジカル順に並べたコミット。配列はメモリ上の形のまま送る（親は 20 バイトの ID ではなく行番号）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphWire {
    pub ids: Bytes,
    pub commit_times: Bytes,
    pub parent_offsets: Bytes,
    pub parent_rows: Bytes,
}

fn bytes_of<T: Pod>(values: &[T]) -> Bytes {
    // `T: Pod` なので、どの値もそのままのバイト列として読める。
    Bytes(
        unsafe {
            std::slice::from_raw_parts(values.as_ptr().cast::<u8>(), std::mem::size_of_val(values))
        }
        .to_vec(),
    )
}

fn values_of<T: Pod + Default + Clone>(bytes: &Bytes) -> Result<Slab<T>> {
    let size = std::mem::size_of::<T>();
    if !bytes.0.len().is_multiple_of(size) {
        return Err("配列の大きさがおかしい".into());
    }
    let mut out = vec![T::default(); bytes.0.len() / size];
    // `out` は `T` の境界に揃っていて、`T: Pod` なのでどのビット列も有効。
    unsafe {
        std::ptr::copy_nonoverlapping(
            bytes.0.as_ptr(),
            out.as_mut_ptr().cast::<u8>(),
            bytes.0.len(),
        )
    };
    Ok(out.into())
}

impl From<&CommitGraph> for GraphWire {
    fn from(g: &CommitGraph) -> Self {
        Self {
            ids: bytes_of(g.ids()),
            commit_times: bytes_of(g.commit_times_slab()),
            parent_offsets: bytes_of(g.parent_offsets()),
            parent_rows: bytes_of(g.parent_rows()),
        }
    }
}

impl GraphWire {
    pub fn into_graph(self) -> Result<CommitGraph> {
        CommitGraph::from_parts(
            values_of(&self.ids)?,
            values_of(&self.commit_times)?,
            values_of(&self.parent_offsets)?,
            values_of(&self.parent_rows)?,
        )
        .ok_or_else(|| "受け取った History が壊れている".into())
    }
}
