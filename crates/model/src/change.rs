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

/// コミットの木の 1 段にあるもの（`GitBackend::tree_entries`）。
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TreeEntry {
    pub name: String,
    pub kind: TreeEntryKind,
    /// blob・木・submodule のコミットの ID。
    pub oid: crate::Oid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TreeEntryKind {
    Folder,
    File,
    Executable,
    Link,
    Submodule,
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

/// 名前を変えたファイルの見せ方（git の `--stat` と同じく、共通の前と後ろをくくり出す）:
/// `src/old.rs` → `src/new.rs` は `src/{old.rs → new.rs}`、くくれなければ `old → new`。
pub fn rename_label(old: &str, new: &str) -> String {
    let (a, b) = (old.as_bytes(), new.as_bytes());
    // 共通の前: 最後の `/` まで。
    let mut prefix = 0;
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        if x != y {
            break;
        }
        if *x == b'/' {
            prefix = i + 1;
        }
    }
    // 共通の後ろ: `/` から。前の終わりの `/` までは重ねてよい（`src/x.rs` → `src/lib/x.rs` は
    // `src/{ → lib}/x.rs`。git と同じ）。
    let mut suffix = 0;
    let room = a.len().min(b.len()) - prefix + usize::from(prefix > 0);
    for i in 0..room {
        let (x, y) = (a[a.len() - 1 - i], b[b.len() - 1 - i]);
        if x != y {
            break;
        }
        if x == b'/' {
            suffix = i + 1;
        }
    }
    if prefix + suffix == 0 {
        return format!("{old} → {new}");
    }
    let middle = |s: &'_ str| -> String { s[prefix..(s.len() - suffix).max(prefix)].to_owned() };
    format!(
        "{}{{{} → {}}}{}",
        &old[..prefix],
        middle(old),
        middle(new),
        &old[old.len() - suffix..]
    )
}

#[cfg(test)]
mod tests {
    use super::rename_label;

    #[test]
    fn renames_factor_out_the_common_folders() {
        assert_eq!(
            rename_label("src/old.rs", "src/new.rs"),
            "src/{old.rs → new.rs}"
        );
        assert_eq!(rename_label("a/b/x.rs", "c/b/x.rs"), "{a → c}/b/x.rs");
        assert_eq!(rename_label("src/a/x.rs", "src/b/x.rs"), "src/{a → b}/x.rs");
        assert_eq!(rename_label("old.rs", "new.rs"), "old.rs → new.rs");
        assert_eq!(rename_label("x.rs", "src/x.rs"), "x.rs → src/x.rs");
        assert_eq!(
            rename_label("src/x.rs", "src/lib/x.rs"),
            "src/{ → lib}/x.rs"
        );
        assert_eq!(
            rename_label("日本/a.txt", "日本/b.txt"),
            "日本/{a.txt → b.txt}"
        );
    }
}
