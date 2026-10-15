//! 内部モデル。FE のことを知らず、git の処理と計算に都合の良い形だけを持つ。

mod backend;
mod change;
mod extras;
mod files;
mod graph;
mod oid;
mod ops;
mod refs;
pub mod search;
mod status;
mod text;
pub mod topo;

pub use backend::{ConnectionState, Error, GitBackend, Result};
pub use change::{ChangeKind, FileChange};
pub use extras::{CommitExtras, FileStat, SignatureCheck, SignatureVerifier};
pub use files::{LocalFiles, RepoFiles, Root};
pub use graph::{CommitGraph, CommitGraphBuilder};
pub use oid::{Oid, OidMap, OidSet};
pub use ops::{HeadState, Operations, Outcome, ResetMode, Sequence};
pub use refs::{RefEntry, RefKind, Refs, RepoPaths};
pub use status::{Status, StatusEntry, StatusKind};
pub use text::CommitText;
pub use topo::RawCommit;
