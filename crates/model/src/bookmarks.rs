//! ブックマーク（`bookmarks.json`）: 個別に登録したリポジトリ（表示名・グループ・お気に入り）と、リポジトリを探す
//! フォルダ。アプリ・GTK4 のアプリ・TUI が同じものを使う。置き場所は設定と同じフォルダ（[`crate::settings::dir`]、
//! `~/.config/amonhen/bookmarks.json`）。形は macOS 版の `BookmarkData`（`items`・`groups`・`scanRoots`）。書くのは
//! アプリ（鍵 `bookmarks.lock` を取って書く）で、ここは読むだけ。知らないキーは読み飛ばす。

use std::path::PathBuf;

/// 個別に登録したリポジトリ。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bookmark {
    /// 場所（この機械のパスか、`ssh://`・`daemon:` で始まる場所）。
    pub path: String,
    /// 表示名の上書き。
    pub name: Option<String>,
    pub group: Option<String>,
    pub favorite: bool,
}

impl Bookmark {
    /// 一覧に出す名前（上書きがなければ、場所の最後の部分）。
    pub fn display_name(&self) -> String {
        if let Some(name) = self.name.as_deref().filter(|n| !n.is_empty()) {
            return name.to_owned();
        }
        let trimmed = self.path.trim_end_matches(['/', '\\']);
        trimmed
            .rsplit(['/', '\\'])
            .next()
            .filter(|last| !last.is_empty())
            .unwrap_or(trimmed)
            .to_owned()
    }
}

/// ブックマークのファイル。決められなければ None。
pub fn path() -> Option<PathBuf> {
    crate::settings::dir().map(|d| d.join("bookmarks.json"))
}

/// 登録したリポジトリ（ファイルの順）。ファイルがなければ空。読めない（JSON でない）ときはその理由。
pub fn read() -> Result<Vec<Bookmark>, String> {
    let Some(path) = path().filter(|p| p.exists()) else {
        return Ok(Vec::new());
    };
    let object = crate::settings::read(&path)?;
    let text = |item: &serde_json::Value, key: &str| {
        item.get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    Ok(object
        .get("items")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            Some(Bookmark {
                path: text(item, "path").filter(|p| !p.is_empty())?,
                name: text(item, "name"),
                group: text(item, "group"),
                favorite: item.get("favorite").and_then(serde_json::Value::as_bool) == Some(true),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 設定と同じフォルダの bookmarks.json を読む（macOS 版の形。知らないキーは読み飛ばす）。
    #[test]
    fn reads_the_shared_bookmarks() {
        let dir = tempfile::tempdir().unwrap();
        let vars = crate::settings::config_vars(dir.path().to_str().unwrap());
        let env = crate::process::ClientEnv::from_vars(vars);
        crate::process::with_client_env(&env, || {
            let file = dir.path().join("amonhen").join("bookmarks.json");
            assert_eq!(path(), Some(file.clone()));
            assert_eq!(read(), Ok(Vec::new()), "no file is no bookmarks");
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(
                &file,
                r#"{"items": [
                    {"path": "/w/amonhen", "favorite": true, "color": "blue"},
                    {"path": "ssh://host/src/tool/", "name": "Tool", "group": "Work"},
                    {"name": "no path"}
                ], "groups": ["Work"], "scanRoots": []}"#,
            )
            .unwrap();
            let items = read().unwrap();
            assert_eq!(items.len(), 2, "an item without a path is skipped");
            assert_eq!(items[0].display_name(), "amonhen");
            assert!(items[0].favorite);
            assert_eq!(items[1].display_name(), "Tool");
            assert_eq!(items[1].group.as_deref(), Some("Work"));
            std::fs::write(&file, "[").unwrap();
            assert!(read().unwrap_err().contains("bookmarks.json"));
        });
    }

    #[test]
    fn display_names_come_from_the_last_part() {
        let bookmark = |path: &str| Bookmark {
            path: path.to_owned(),
            name: None,
            group: None,
            favorite: false,
        };
        assert_eq!(bookmark("ssh://host/src/tool/").display_name(), "tool");
        assert_eq!(bookmark("daemon:/w/repo").display_name(), "repo");
        assert_eq!(bookmark("C:\\w\\repo").display_name(), "repo");
    }
}
