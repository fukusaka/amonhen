//! 状態・スナップショット・Intent の処理。通信手段にも FE にも依存しない。

pub mod blame;
pub mod branches;
pub mod cache;
pub mod command;
pub mod conflict;
mod delegate;
pub mod diff;
pub mod fold;
pub mod graph;
pub mod lanes;
pub mod lineage;
pub mod query;
mod repository;
pub mod rewrite;
pub mod undo;
pub mod words;
pub mod worktree;

pub use repository::{
    HistorySnapshot, PairMode, RefsSnapshot, Repository, Selection, StatusSnapshot, WorkingDiff,
};

/// 手元で走っている操作を取り消す（取り消せる記録で、まだ走っていれば true）。
pub fn cancel_activity(seq: u64) -> bool {
    amonhen_model::activity::cancel(seq)
}

/// Activity の記録（git の起動とコマンドの実行、古い順）の JSON の配列。
pub fn activity_json() -> String {
    serde_json::to_string(&amonhen_model::activity::records()).unwrap_or_else(|_| "[]".into())
}
