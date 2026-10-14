use crate::Oid;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChangeKind {
    Added = 0,
    Deleted = 1,
    Modified = 2,
    Renamed = 3,
    Copied = 4,
}

/// コミットで変わったファイル 1 つ。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileChange {
    pub kind: ChangeKind,
    pub path: String,
    /// 名前を変えた・コピーしたときの元のパス。
    pub old_path: Option<String>,
    /// 変更前の blob。追加なら `None`。
    pub old_id: Option<Oid>,
    /// 変更後の blob。削除なら `None`。
    pub new_id: Option<Oid>,
    /// サブモジュールの更新（blob ではなくコミットを指す）。
    pub submodule: bool,
}
