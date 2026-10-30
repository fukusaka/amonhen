//! 要求と知らせの形。応答の中身は要求ごとに決まった型を postcard で書いたもの（コメントに書く）。

use amonhen_model::tr;
use std::path::PathBuf;
use std::sync::Arc;

use amonhen_layout::{Pod, Slab};
use amonhen_model::search::TextTerm;
use amonhen_model::{
    ApplyPatch, CommitGraph, CommitText, FormatPatch, HeadState, Oid, PullMode, RefUpdate,
    RemoteChange, ResetMode, Result, Root, Sequence, WorktreeChange,
};
use serde::{Deserialize, Serialize};

/// このビルドの指紋（ワークスペースのソースのハッシュ。`build.rs`）。daemon・SSH 先のエンジンとは、これが同じとき
/// だけつなぐ。手元の共有の daemon のソケットの名前・`--version` にも入れる。
pub const BUILD_ID: &str = env!("AMONHEN_BUILD_ID");

/// エンジンの実行ファイルに埋め込む印（`BUILD_ID`）。SSH 先へ送る前に、送るエンジンがこのビルドのものかを中身で
/// 確かめる（違うものを送って、つないでから断られないように）。
pub const BUILD_MARK: &str = concat!("amonhen-build=", env!("AMONHEN_BUILD_ID"), ";");

/// 挨拶の `version`。照合は `build` でするので変えない。前の版（通信の形の版を数で比べていた）は、自分の数と
/// 違うので断る（どの版の数とも重ならない）。
pub const HELLO_VERSION: u32 = 1000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Request {
    /// → `Hello`
    Hello {
        /// `HELLO_VERSION`（前の版に断らせるため）。
        version: u32,
        /// 手元の利用者の言語（`amonhen_model::i18n::Lang::code`）。この接続のエラーの文をその言語で作る。
        language: String,
        /// 手元の `BUILD_ID`。違えば断る。
        build: String,
    },
    // GitBackend
    /// → `Option<Oid>`
    Head,
    /// → `Vec<Oid>`
    Tips,
    /// → `Vec<Oid>`（shallow clone の境界）
    Shallow,
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
    /// → `Option<String>`: 共有の daemon が History を用意した永続キャッシュのファイル（用意できない daemon なら
    /// `None`）。手元の共有の daemon でだけ使う。
    PreparedHistory,
    /// → `DelegateReply`: コアの処理を daemon のコアに任せる（中身の形はコアが決める）。手元の共有の daemon でだけ
    /// 使う。
    Delegate(Bytes),
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
    /// → `Bytes`
    ToWorktreeForm {
        path: String,
        content: Bytes,
    },
    /// → `String`
    LineEndingRule(String),
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
    WriteCommitGraph {
        split: bool,
    },
    /// → `Vec<CommitTextWire>`
    CommitTexts(Vec<Oid>),
    /// → `Vec<CommitTextWire>`（`raw` は件名と作者の名前・メールだけ）
    CommitSummaries(Vec<Oid>),
    /// → `CommitExtras`
    CommitExtras(Oid),
    /// → `Vec<SignatureCheck>`（`ids` と同じ順）
    VerifySignatures(Vec<Oid>),
    /// → `Vec<Option<Bytes>>`: タグのオブジェクトの中身（`ids` と同じ順。タグでなければ `None`）
    TagObjects(Vec<Oid>),
    /// → `Vec<SignatureCheck>`: タグの署名の検証（ID と中身。同じ順）
    VerifyTags(Vec<(Oid, Bytes)>),
    /// → `Vec<Option<Oid>>`: ローカルブランチを作った点（`names` と同じ順）
    BranchCreatedAt(Vec<String>),
    /// → `Vec<ReflogEntry>`: ref の reflog（新しい順に `limit` 行まで）
    Reflog {
        name: String,
        limit: usize,
    },
    /// → `Vec<Vec<Oid>>`（条件ごとに合ったコミット。取り消せる）。`mailmap` なら、作者は daemon の側の mailmap で
    /// 置き換えた名前でも探す。
    SearchTexts {
        tips: Vec<Oid>,
        terms: Vec<TextTerm>,
        mailmap: bool,
    },
    /// → `Option<Mailmap>`: mailmap と `log.mailmap`。指紋（`Mailmap::fingerprint`）が `known` と同じなら `None`。
    Mailmap {
        known: Option<u64>,
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
    /// → `()`。この先の要求で動かす git・ssh に渡す FE の環境を差し替える（`ClientEnv::encode` の形）。FE の環境が
    /// 変わったとき（アプリの設定・シェルの読み直し）に、つなぎ直さずに送る。
    Environment(String),
    /// → `Option<String>`: 共有の daemon がこのリポジトリのためにつないでいる SSH 先のエンジンと、その用意のしかた
    /// （`ssh::RemoteEngine::describe`）。SSH 先でなければ・daemon でなければ `None`。手元の共有の daemon でだけ使う。
    RemoteEngine,
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
        limit: Option<usize>,
        merges: bool,
    },
    HeadState,
    ResolveRefs(Vec<String>),
    UpdateRef {
        name: String,
        value: Option<Oid>,
        expected: Option<Oid>,
    },
    UpdateRefs(Vec<RefUpdate>),
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
    DeleteTag(String),
    RemoteTags(String),
    PushTags {
        remote: String,
        names: Vec<String>,
        force: bool,
        delete: bool,
    },
    CheckBranchName(String),
    IsAncestor {
        ancestor: String,
        descendant: String,
    },
    PredictMerge {
        ours: String,
        theirs: Vec<String>,
    },
    Resolve(String),
    Merge(Vec<String>),
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
        message: Option<PathBuf>,
    },
    SequenceInProgress,
    RebaseProgress,
    Abort(Sequence),
    ContinueSequence(Sequence),
    SkipSequence(Sequence),
    FormatPatch {
        commits: Vec<Oid>,
        options: FormatPatch,
    },
    Am {
        mbox: Bytes,
        three_way: Option<bool>,
    },
    AmThreeWay,
    ApplyPatch {
        patch: Bytes,
        options: ApplyPatch,
    },
    TakeSide {
        path: String,
        ours: bool,
    },
    /// → `[Option<Oid>; 3]`
    ConflictStages(String),
    Fetch,
    Pull(PullMode),
    BranchTracking,
    Push {
        remote: String,
        branch: String,
        remote_branch: String,
        force: bool,
        expect: Option<Oid>,
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
    StashBranch {
        branch: String,
        name: String,
    },
    CountApart {
        left: String,
        right: String,
    },
    UpdateSubmodules(Vec<String>),
    ConfigList,
    ConfigKeys,
    ConfigSet(String, Option<String>),
    ConfigSetGlobal(String, Option<String>),
    GitVersion,
    Remotes,
    CheckRemote(String),
    SigningKeys(String),
    /// → `Vec<Program>`（daemon の機械にある `git`・`gpg` などの候補）
    Programs(String),
    /// → `Directory`（daemon の機械のフォルダの中身。`~` はその機械のホーム）
    ListDirectory(String),
    /// → `(String, Option<String>)`（ユーザー全体の無視するパターンのファイルのパスと中身）
    ExcludesFile,
    /// → `()`
    SaveExcludesFile(String),
    /// → `Vec<(String, String)>`（エンジンのプロセスの環境変数）
    ProcessEnvironment,
    /// → `MachineGit`（エンジンの機械で使う git と、その Amon Hen の設定で選んだもの）
    MachineGit,
    /// → `()`（エンジンの機械の Amon Hen の設定に、使う git を書く。None なら消す）
    SetMachineGit(Option<String>),
    LfsTrack(String, bool),
    LfsFileCount,
    ChangeRemote(RemoteChange),
    WorkingStats,
    IgnoreMatches(Vec<String>, usize),
    Activity,
    CancelActivity(u64),
    /// → `Bytes`（`git worktree list --porcelain -z`）
    WorktreeList,
    /// → `()`
    ChangeWorktree(WorktreeChange),
    /// → `Vec<WorktreeChanges>`
    WorktreeChanges(Vec<String>, bool),
    /// → `Vec<RangePair>`
    RangeDiff(Vec<String>, Option<u32>),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hello {
    pub version: u32,
    pub build: String,
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
/// 任せた処理の答え。大きなバイト列は共有メモリの領域（`region` の名前と各部分の位置）で渡す。共有メモリを
/// 作れなかったときだけ、`inline` に入れて送る。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DelegateReply {
    pub reply: Bytes,
    pub region: Option<String>,
    pub parts: Vec<(u64, u64)>,
    pub inline: Vec<Bytes>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommitTextWire {
    pub raw: Bytes,
    pub subject: (u32, u32),
    pub author_name: (u32, u32),
    pub author_email: (u32, u32),
    pub author_time: i64,
    pub signature: u8,
}

impl From<&CommitText> for CommitTextWire {
    fn from(t: &CommitText) -> Self {
        Self {
            raw: Bytes(t.raw.to_vec()),
            subject: (t.subject.start, t.subject.end),
            author_name: (t.author_name.start, t.author_name.end),
            author_email: (t.author_email.start, t.author_email.end),
            author_time: t.author_time,
            signature: t.signature,
        }
    }
}

impl CommitTextWire {
    /// 件名と作者の名前・メールだけを残した本文（History の行に出す分。メールは手元で mailmap を当てるため）。
    pub fn summary(t: &CommitText) -> Self {
        let mut raw = t.subject().to_vec();
        let subject = (0, raw.len() as u32);
        raw.extend_from_slice(t.author_name());
        let author_name = (subject.1, raw.len() as u32);
        raw.extend_from_slice(t.author_email());
        Self {
            raw: Bytes(raw.clone()),
            subject,
            author_name,
            author_email: (author_name.1, raw.len() as u32),
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
            author_email: w.author_email.0..w.author_email.1,
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
        return Err(tr!("Invalid array size").into());
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
        .ok_or_else(|| tr!("The received History is corrupt").into())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn build_mark_holds_the_build_id() {
        assert_eq!(super::BUILD_ID.len(), 16);
        assert_eq!(
            super::BUILD_MARK,
            format!("amonhen-build={};", super::BUILD_ID)
        );
    }
}
