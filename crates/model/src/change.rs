use crate::Oid;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ChangeKind {
    Added = 0,
    Deleted = 1,
    Modified = 2,
    Renamed = 3,
    Copied = 4,
}

/// コミットで変わったファイル 1 つ。
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

/// 差分の見せ方にかかる、ファイルごとの属性（`.gitattributes` と設定から）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DiffAttributes {
    /// hunk の見出しに出す関数名の規則（`diff=<名前>` の `diff.<名前>.xfuncname`・`funcname`）。
    /// `None` は git の既定の規則（英字・`_`・`$` で始まる行）。
    pub funcname: Option<FuncNameRule>,
    /// `linguist-generated`（生成物）。`Some(false)` は生成物ではないと明示したもの、`None` は指定なし。
    pub generated: Option<bool>,
}

/// hunk の見出しの関数名を探す正規表現（改行で区切って複数、`!` で始まるものは否定）。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FuncNameRule {
    pub pattern: String,
    /// 拡張正規表現（`xfuncname`）。false は基本正規表現（`funcname`）。
    pub extended: bool,
}

