use crate::Oid;

/// サイドバーの項目の種類。
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RefKind {
    Local = 0,
    Remote = 1,
    Tag = 2,
    Stash = 3,
    Submodule = 4,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefEntry {
    pub kind: RefKind,
    /// 表示名（`main`、`origin/main`、`v1.0`、`stash@{0}`、サブモジュールのパス）。
    pub name: String,
    /// 指すコミット。コミット以外を指すタグや、まだ取得していないサブモジュールは `None`。
    pub target: Option<Oid>,
    /// ローカルブランチの上流（`origin/main`）、stash のメッセージ。
    pub detail: Option<String>,
}

/// ブランチ・リモートブランチ・タグ・stash・サブモジュールの一覧。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Refs {
    /// チェックアウト中のブランチ。detached HEAD なら `None`。
    pub head_branch: Option<String>,
    pub head: Option<Oid>,
    /// 種類ごとにまとめ、種類の中は名前順（stash は新しい順）。
    pub entries: Vec<RefEntry>,
}
