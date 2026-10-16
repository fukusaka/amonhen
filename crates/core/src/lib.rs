//! 状態・スナップショット・Intent の処理。通信手段にも FE にも依存しない。

pub mod blame;
pub mod cache;
pub mod command;
pub mod conflict;
pub mod diff;
pub mod graph;
pub mod lanes;
pub mod lineage;
pub mod query;
mod repository;
pub mod rewrite;
pub mod undo;
pub mod words;

pub use repository::{
    HistorySnapshot, PairMode, RefsSnapshot, Repository, Selection, StatusSnapshot, WorkingDiff,
};

/// Activity の記録（git の起動とコマンドの実行、古い順）の JSON の配列。
pub fn activity_json() -> String {
    serde_json::to_string(&amonhen_model::activity::records()).unwrap_or_else(|_| "[]".into())
}
