//! 状態・スナップショット・Intent の処理。通信手段にも FE にも依存しない。

pub mod graph;
mod repository;

pub use repository::{HistorySnapshot, Repository};
