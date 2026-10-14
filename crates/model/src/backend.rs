use crate::{CommitGraph, Oid};

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// git の操作。デスクトップは gix ＋ `git` の spawn、モバイルは gix ＋ libgit2 で実装する。
///
/// 書き込み系の操作は、実装を足すときにここへ加える。
pub trait GitBackend: Send + Sync {
    /// HEAD が指すコミット。生まれたばかりのブランチなどでコミットがなければ `None`。
    fn head(&self) -> Result<Option<Oid>>;

    /// History の起点（HEAD・ローカルとリモートのブランチ・タグが指すコミット）。
    fn tips(&self) -> Result<Vec<Oid>>;

    /// `tips` から辿れるコミットを、子が親より先に来るトポロジカル順で返す。
    fn commit_graph(&self, tips: &[Oid]) -> Result<CommitGraph>;
}
