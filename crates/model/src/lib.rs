//! 内部モデル。FE のことを知らず、git の処理と計算に都合の良い形だけを持つ。

mod backend;
mod change;
mod graph;
mod oid;
mod refs;
mod text;
pub mod topo;

pub use backend::{Error, GitBackend, Result};
pub use change::{ChangeKind, FileChange};
pub use graph::{CommitGraph, CommitGraphBuilder};
pub use oid::{Oid, OidMap, OidSet};
pub use refs::{RefEntry, RefKind, Refs, RepoPaths};
pub use text::CommitText;
pub use topo::RawCommit;
