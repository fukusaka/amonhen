//! 状態・スナップショット・Intent の処理。通信手段にも FE にも依存しない。

pub mod cache;
pub mod diff;
pub mod graph;
mod repository;
pub mod undo;

pub use repository::{HistorySnapshot, RefsSnapshot, Repository, StatusSnapshot, WorkingDiff};
