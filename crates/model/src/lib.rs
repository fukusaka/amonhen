//! 内部モデル。FE のことを知らず、git の処理と計算に都合の良い形だけを持つ。

pub mod activity;
mod backend;
mod change;
mod eol;
mod extras;
mod files;
mod graph;
pub mod i18n;
mod oid;
mod ops;
pub mod process;
mod refs;
pub mod scan;
pub mod search;
mod status;
mod text;
pub mod topo;

pub use backend::{ConnectionState, Delegated, Error, GitBackend, Result};
pub use change::{ChangeKind, FileChange};
pub use eol::{Eol, LineEndings};
pub use extras::{CommitExtras, FileStat, SignatureCheck, SignatureVerifier, SignerEmail};
pub use files::{LocalFiles, RepoFiles, Root};
pub use graph::{CommitGraph, CommitGraphBuilder};
pub use oid::{Oid, OidMap, OidSet};
pub use ops::{
    ConfigEntry, HeadState, Operations, Outcome, PullMode, RemoteChange, RemoteEntry, ResetMode,
    Sequence, WorktreeChange, WorktreeChanges,
};
pub use refs::{RefEntry, RefKind, Refs, RepoPaths};
pub use status::{Status, StatusEntry, StatusKind};
pub use text::CommitText;
pub use topo::{Order as HistoryOrder, RawCommit};
