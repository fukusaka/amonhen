//! フォルダの下のリポジトリを探す（リポジトリの一覧の「スキャンするフォルダ」）。手元のアプリと同じ規則で、SSH 先では
//! エンジンの `amonhen find-repositories` がこれを使う。

use std::path::{Path, PathBuf};

/// 入らないフォルダの名前（アプリの既定と同じ）。
pub const DEFAULT_EXCLUDES: &[&str] = &[
    "node_modules",
    "target",
    "build",
    ".build",
    "vendor",
    "Pods",
    "DerivedData",
    "dist",
];

/// `root` の下のリポジトリ（作業ツリーの直下に `.git` があるフォルダ）を、`root` からの相対パスで返す（`root` が
/// リポジトリなら `.`）。`depth` 段下まで探し、リポジトリの中・隠しフォルダ・シンボリックリンク・`excludes` の名前の
/// フォルダには入らない。名前の順。
pub fn find_repositories(root: &Path, depth: u32, excludes: &[String]) -> Vec<PathBuf> {
    fn walk(dir: &Path, rel: &Path, depth: u32, excludes: &[String], found: &mut Vec<PathBuf>) {
        if dir.join(".git").exists() {
            found.push(if rel.as_os_str().is_empty() {
                PathBuf::from(".")
            } else {
                rel.to_path_buf()
            });
            return;
        }
        if depth == 0 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with('.') || excludes.iter().any(|e| *e == name_str) {
                continue;
            }
            // シンボリックリンクはたどらない（file_type はリンクそのものの種類）。
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if !kind.is_dir() {
                continue;
            }
            walk(&entry.path(), &rel.join(&name), depth - 1, excludes, found);
        }
    }
    let mut found = Vec::new();
    walk(root, Path::new(""), depth, excludes, &mut found);
    found.sort();
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_repositories_within_depth_and_skips_excluded_hidden_and_nested() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        for dir in [
            "a/.git",
            "b/c/.git",
            "b/c/inner/.git",
            "d/e/f/g/.git",
            "node_modules/x/.git",
            ".hidden/y/.git",
        ] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let excludes: Vec<String> = DEFAULT_EXCLUDES.iter().map(|s| (*s).to_owned()).collect();
        let found = find_repositories(root, 3, &excludes);
        assert_eq!(found, vec![PathBuf::from("a"), PathBuf::from("b/c")]);
        // 深さを増やすと、深いものも見つかる。リポジトリの中（b/c/inner）には入らない。
        assert_eq!(find_repositories(root, 4, &excludes).len(), 3);
        // root そのものがリポジトリなら `.`。
        assert_eq!(
            find_repositories(&root.join("a"), 3, &excludes),
            vec![PathBuf::from(".")]
        );
    }
}
