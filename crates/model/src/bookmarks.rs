//! ブックマーク（`bookmarks.json`）: 個別に登録したリポジトリ（表示名・グループ・お気に入り）と、リポジトリを探す
//! フォルダ。アプリ・GTK4 のアプリ・TUI が同じものを使う。置き場所は設定と同じフォルダ（[`crate::settings::dir`]、
//! `~/.config/amonhen/bookmarks.json`）。形は macOS 版の `BookmarkData`（`items`・`groups`・`scanRoots`）。書くのは
//! アプリ（鍵 `bookmarks.lock` を取って書く）で、ここは読むだけ。知らないキーは読み飛ばす。探すフォルダの中の
//! リポジトリは [`crate::scan::find_repositories`] で探す（結果は保存しない）。

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

/// リポジトリを探すフォルダ（macOS 版の `ScanRoot`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanRoot {
    /// この機械のパスか、SSH 先のフォルダ（`ssh://host/path`。SSH 先のエンジンが探す）。
    pub path: String,
    /// フォルダから何段下まで探すか。
    pub depth: u32,
    /// 入らないフォルダの名前。
    pub excludes: Vec<String>,
}

impl ScanRoot {
    /// この機械のフォルダか（`ssh://`・`daemon:` で始まらない）。
    pub fn is_local(&self) -> bool {
        !self.path.starts_with("ssh://") && !self.path.starts_with("daemon:")
    }

    /// この機械のフォルダの下のリポジトリ（場所。名前の順）。SSH 先のフォルダは空。
    pub fn find_local(&self) -> Vec<String> {
        if !self.is_local() {
            return Vec::new();
        }
        let root = std::path::Path::new(&self.path);
        crate::scan::find_repositories(root, self.depth, &self.excludes)
            .into_iter()
            .map(|rel| {
                if rel.as_os_str() == "." {
                    self.path.clone()
                } else {
                    root.join(rel).to_string_lossy().into_owned()
                }
            })
            .collect()
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

/// リポジトリを探すフォルダ（ファイルの順）。ファイルがなければ空。読めない（JSON でない）ときはその理由。
/// 深さ・除外の名前がなければアプリの既定（3 段、[`crate::scan::DEFAULT_EXCLUDES`]）。
pub fn scan_roots() -> Result<Vec<ScanRoot>, String> {
    let Some(path) = path().filter(|p| p.exists()) else {
        return Ok(Vec::new());
    };
    let object = crate::settings::read(&path)?;
    Ok(object
        .get("scanRoots")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|root| {
            Some(ScanRoot {
                path: root
                    .get("path")?
                    .as_str()
                    .filter(|p| !p.is_empty())?
                    .to_owned(),
                depth: root
                    .get("depth")
                    .and_then(serde_json::Value::as_u64)
                    .map_or(3, |d| d.min(u64::from(u32::MAX)) as u32),
                excludes: match root.get("excludes").and_then(serde_json::Value::as_array) {
                    Some(names) => names
                        .iter()
                        .filter_map(|n| n.as_str().map(str::to_owned))
                        .collect(),
                    None => crate::scan::DEFAULT_EXCLUDES
                        .iter()
                        .map(|s| (*s).to_owned())
                        .collect(),
                },
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
            assert_eq!(scan_roots(), Ok(Vec::new()));
            std::fs::write(&file, "[").unwrap();
            assert!(read().unwrap_err().contains("bookmarks.json"));
            assert!(scan_roots().unwrap_err().contains("bookmarks.json"));
        });
    }

    /// 探すフォルダ（深さ・除外の名前がなければ既定）と、手元のフォルダの下のリポジトリ。
    #[test]
    fn reads_scan_roots_and_finds_local_repositories() {
        let dir = tempfile::tempdir().unwrap();
        let vars = crate::settings::config_vars(dir.path().to_str().unwrap());
        let env = crate::process::ClientEnv::from_vars(vars);
        let work = dir.path().join("work");
        for repo in ["a/.git", "b/c/.git", "target/d/.git"] {
            std::fs::create_dir_all(work.join(repo)).unwrap();
        }
        crate::process::with_client_env(&env, || {
            let file = path().unwrap();
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            let json = serde_json::json!({"scanRoots": [
                {"path": work.to_str().unwrap()},
                {"path": work.to_str().unwrap(), "depth": 1, "excludes": []},
                {"path": "ssh://host/src", "depth": 2},
                {"depth": 1}
            ]});
            std::fs::write(&file, json.to_string()).unwrap();
            let roots = scan_roots().unwrap();
            assert_eq!(roots.len(), 3, "a root without a path is skipped");
            assert_eq!(roots[0].depth, 3);
            assert!(roots[0].excludes.iter().any(|e| e == "target"));
            let local = |rel: &str| {
                let joined = rel.split('/').fold(work.clone(), |p, part| p.join(part));
                joined.to_string_lossy().into_owned()
            };
            assert_eq!(roots[0].find_local(), [local("a"), local("b/c")]);
            assert_eq!(roots[1].find_local(), [local("a")], "one level down only");
            assert!(!roots[2].is_local());
            assert!(roots[2].find_local().is_empty());
        });
        let inside = ScanRoot {
            path: work.join("a").to_string_lossy().into_owned(),
            depth: 3,
            excludes: Vec::new(),
        };
        assert_eq!(
            inside.find_local(),
            std::slice::from_ref(&inside.path),
            "the root itself"
        );
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
