//! 内部モデル。FE のことを知らず、git の処理と計算に都合の良い形だけを持つ。

mod backend;
mod graph;
mod oid;
mod text;
pub mod topo;

pub use backend::{Error, GitBackend, Result};
pub use graph::{CommitGraph, CommitGraphBuilder};
pub use oid::Oid;
pub use text::CommitText;
pub use topo::RawCommit;
