//! 状態・スナップショット・Intent の処理。通信手段にも FE にも依存しない。

pub mod cache;
pub mod command;
pub mod conflict;
pub mod diff;
pub mod graph;
pub mod lanes;
mod repository;
pub mod rewrite;
pub mod undo;
pub mod words;

pub use repository::{HistorySnapshot, RefsSnapshot, Repository, StatusSnapshot, WorkingDiff};
