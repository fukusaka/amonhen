use crate::Oid;

/// status の 1 項目の種類。
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum StatusKind {
    Changed = 0,
    Renamed = 1,
    /// コンフリクト中。
    Unmerged = 2,
    Untracked = 3,
}

/// 作業ツリーと index の 1 ファイル。
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StatusEntry {
    pub kind: StatusKind,
    pub path: String,
    /// 名前を変えたときの元のパス。
    pub orig_path: Option<String>,
    /// index 側の状態（git の XY の X）。変更なしは `.`、untracked は `?`。
    pub index: u8,
    /// 作業ツリー側の状態（Y）。
    pub worktree: u8,
    /// HEAD の blob（HEAD になければ `None`）。
    pub head_id: Option<Oid>,
    /// index の blob（index になければ `None`）。
    pub index_id: Option<Oid>,
    /// index でのファイルモード（`0o100644` など）。index になければ 0。
    pub index_mode: u32,
    pub submodule: bool,
}

impl StatusEntry {
    /// stage 済みの変更があるか。
    pub fn is_staged(&self) -> bool {
        !matches!(self.index, b'.' | b'?')
    }

    /// stage していない変更があるか（untracked を含む）。
    pub fn is_unstaged(&self) -> bool {
        self.worktree != b'.'
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Status {
    /// チェックアウト中のブランチ。detached HEAD なら `None`。
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub entries: Vec<StatusEntry>,
}
