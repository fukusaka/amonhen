use crate::{Oid, Result};

/// HEAD の状態。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HeadState {
    /// チェックアウト中のブランチの完全な名前（`refs/heads/main`）。detached なら `None`。
    pub branch: Option<String>,
    /// まだコミットがなければ `None`。
    pub oid: Option<Oid>,
}

/// 途中で止まりうる操作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sequence {
    Rebase,
    Merge,
    CherryPick,
    Revert,
}

impl Sequence {
    pub fn command(self) -> &'static str {
        match self {
            Sequence::Rebase => "rebase",
            Sequence::Merge => "merge",
            Sequence::CherryPick => "cherry-pick",
            Sequence::Revert => "revert",
        }
    }
}

/// merge・rebase などの結果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Done,
    /// コンフリクトで止まった。
    Conflict,
    /// コンフリクトではなく途中で止まった（edit など）。
    Stopped,
}

/// Undo で現在のブランチを戻すときの reset の種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetMode {
    /// index と作業ツリーはそのまま（コミットの Undo）。
    Soft,
    /// 作業中の変更を残したまま戻す（merge・rebase の Undo）。
    Keep,
}

/// 書き込み系の操作。デスクトップは `git` を起動して行う。
pub trait Operations: Send + Sync {
    fn head_state(&self) -> Result<HeadState>;
    /// 完全な ref 名の値。なければ `None`。
    fn resolve_refs(&self, names: &[String]) -> Result<Vec<(String, Option<Oid>)>>;
    /// `expected` が今の値のときだけ書き換える（`None` は「まだない」）。`value` が `None` なら消す。
    fn update_ref(&self, name: &str, value: Option<&Oid>, expected: Option<&Oid>) -> Result<()>;
    fn reset(&self, mode: ResetMode, oid: &Oid) -> Result<()>;
    /// ブランチ（完全な名前）か、detached HEAD に切り替える。
    fn switch_to(&self, head: &HeadState) -> Result<()>;
    fn write_blob(&self, content: &[u8]) -> Result<Oid>;
    fn read_blob(&self, oid: &Oid) -> Result<Vec<u8>>;

    fn switch_branch(&self, name: &str) -> Result<()>;
    /// リモートブランチ（`origin/main`）を追跡するローカルブランチを作って切り替える。
    fn track_branch(&self, remote_branch: &str) -> Result<()>;
    fn create_branch(&self, name: &str, start: &str, checkout: bool) -> Result<()>;
    fn delete_branch(&self, name: &str, force: bool) -> Result<()>;
    fn rename_branch(&self, from: &str, to: &str) -> Result<()>;
    fn create_tag(&self, name: &str, target: &str, message: Option<&str>) -> Result<()>;
    /// ブランチ名として使えなければ理由。
    fn check_branch_name(&self, name: &str) -> Result<Option<String>>;

    fn merge(&self, rev: &str) -> Result<Outcome>;
    fn rebase(&self, onto: &str) -> Result<Outcome>;
    fn cherry_pick(&self, oid: &Oid, is_merge: bool) -> Result<Outcome>;
    fn revert(&self, oid: &Oid, is_merge: bool) -> Result<Outcome>;
    /// 用意した todo で非対話の rebase を行う（コミットの編集）。`base` が `None` なら `--root`。
    fn rebase_todo(&self, base: Option<&Oid>, todo: &std::path::Path) -> Result<Outcome>;
    fn sequence_in_progress(&self) -> Result<Option<Sequence>>;
    fn abort(&self, sequence: Sequence) -> Result<()>;
    fn continue_sequence(&self, sequence: Sequence) -> Result<Outcome>;
    fn skip_sequence(&self, sequence: Sequence) -> Result<Outcome>;

    fn fetch(&self) -> Result<()>;
    fn push(
        &self,
        remote: &str,
        branch: &str,
        remote_branch: &str,
        force: bool,
        set_upstream: bool,
    ) -> Result<()>;

    /// 作業ツリーのファイルを index の内容に戻す（変更の破棄）。
    fn restore_worktree(&self, paths: &[&str]) -> Result<()>;
    fn stash_push(&self, message: Option<&str>, include_untracked: bool) -> Result<()>;
    /// stage していた状態も戻す。戻せなければ作業ツリーにだけ当てる。
    fn stash_apply(&self, name: &str, pop: bool) -> Result<Outcome>;
    fn stash_drop(&self, name: &str) -> Result<()>;
    /// 消した stash を戻す。
    fn stash_store(&self, oid: &Oid, message: &str) -> Result<()>;
    fn update_submodules(&self, paths: &[&str]) -> Result<()>;
}
