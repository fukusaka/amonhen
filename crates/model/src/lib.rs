//! 内部モデル。FE のことを知らず、git の処理と計算に都合の良い形だけを持つ。

pub mod activity;
mod backend;
mod change;
mod eol;
mod extras;
mod files;
mod graph;
pub mod i18n;
pub mod mailmap;
mod oid;
mod ops;
pub mod process;
mod range_diff;
mod refs;
pub mod scan;
pub mod search;
pub mod settings;
mod status;
pub mod testing;
mod text;
pub mod topo;

pub use backend::{ConnectionState, Delegated, Error, GitBackend, Result};
pub use change::{ChangeKind, FileChange};
pub use eol::{Eol, LineEndings};
pub use extras::{CommitExtras, FileStat, SignatureCheck, SignatureVerifier, SignerEmail};
pub use files::{LocalFiles, RepoFiles, Root};
pub use graph::{CommitGraph, CommitGraphBuilder};
pub use mailmap::{Mailmap, MailmapEntry};
pub use oid::{Oid, OidMap, OidSet};
pub use ops::{
    ApplyPatch, ConfigEntry, Directory, DirectoryEntry, FormatPatch, HeadState, MachineGit,
    Operations, Outcome, PatchFile, Program, PullMode, RefUpdate, RemoteChange, RemoteCheck,
    RemoteEntry, ResetMode, Sequence, SigningKey, WorktreeChange, WorktreeChanges,
};
pub use range_diff::{
    RangeCommit, RangePair, RangeStatus, parse_range_diff, rebase_onto, rebase_onto_from_head,
    reflog_keeps_history,
};
pub use refs::{RefEntry, RefKind, ReflogEntry, Refs, RepoPaths};
pub use status::{Status, StatusEntry, StatusKind};
pub use text::CommitText;
pub use topo::{Order as HistoryOrder, RawCommit};
