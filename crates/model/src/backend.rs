use crate::{CommitGraph, CommitText, Oid, RawCommit, Refs, topo};

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

    /// `tips` から辿れるコミットを順不同で読む。`known` が真を返すコミットとその先は辿らない
    /// （前回読んだ History に当たったら止めるため）。各コミットは 1 回だけ読む。
    fn read_commits(
        &self,
        tips: &[Oid],
        known: &(dyn Fn(&Oid) -> bool + Sync),
    ) -> Result<Vec<RawCommit>>;

    /// `tips` から辿れるコミットを、子が親より先に来るトポロジカル順で返す。
    fn commit_graph(&self, tips: &[Oid]) -> Result<CommitGraph> {
        Ok(topo::sort(self.read_commits(tips, &|_| false)?))
    }

    /// サイドバーに出す ref の一覧。
    fn refs(&self) -> Result<Refs>;

    /// commit-graph ファイルがあるか。なければ History の初回の読み込みが数倍遅い。
    fn has_commit_graph(&self) -> Result<bool> {
        Ok(true)
    }

    /// commit-graph を書き出す（`git commit-graph write --reachable`）。リポジトリの `.git` に
    /// 書き込むので、利用者が設定で選んだときだけ呼ぶ。対応しない実装は何もしない。
    fn write_commit_graph(&self) -> Result<()> {
        Ok(())
    }

    /// コミットの本文。History で見えている行の分だけ呼ぶ。
    fn commit_texts(&self, ids: &[Oid]) -> Result<Vec<CommitText>>;
}
