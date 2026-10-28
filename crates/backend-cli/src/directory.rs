//! この機械のフォルダの中身（SSH 先のパスを選ぶシートが、SSH 先のエンジンに読ませる）。

use std::path::{Component, Path, PathBuf};

use amonhen_model::{Directory, DirectoryEntry, Result, tr};

/// `path` のフォルダの中身。`~` で始まればホームから、相対パスもホームから。
pub fn list(path: &str) -> Result<Directory> {
    let home = std::env::home_dir().filter(|h| !h.as_os_str().is_empty());
    let dir = resolve(path, home.as_deref());
    let shown = dir.display().to_string();
    let meta = std::fs::metadata(&dir).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => tr!("There’s no folder at {path}", path = shown),
        _ => tr!("Can’t read {path}: {e}", path = shown, e = e),
    })?;
    if !meta.is_dir() {
        return Err(tr!("{path} isn’t a folder", path = shown).into());
    }
    let read =
        std::fs::read_dir(&dir).map_err(|e| tr!("Can’t read {path}: {e}", path = shown, e = e))?;
    let mut entries: Vec<DirectoryEntry> = read.filter_map(|e| e.ok()).map(|e| entry(&e)).collect();
    entries.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(Directory {
        path: shown,
        home: home.map(|h| h.display().to_string()),
        entries,
    })
}

/// `~`・`~/…`（Windows は `~\…` も）をホームから読む。ほかはそのまま。SSH 先のエンジンに渡すパス（`--stdio`・
/// `find-repositories`）で使う。Windows のシェルは `~` を展開しないので、エンジンが読む。
pub fn expand_home(path: &str) -> PathBuf {
    let home = std::env::home_dir().filter(|h| !h.as_os_str().is_empty());
    match (home, home_relative(path)) {
        (Some(home), Some("")) => home,
        (Some(home), Some(rest)) => home.join(rest),
        _ => PathBuf::from(path),
    }
}

/// `~` なら空、`~/x` なら `x`（Windows は `~\x` も）。ホームからでなければ None。
fn home_relative(path: &str) -> Option<&str> {
    if path == "~" {
        return Some("");
    }
    path.strip_prefix("~/")
        .or_else(|| cfg!(windows).then(|| path.strip_prefix("~\\")).flatten())
}

/// `~`・相対パスをホームから読み、`.`・`..` を除いた絶対パス。
fn resolve(path: &str, home: Option<&Path>) -> PathBuf {
    let path = path.trim();
    let root = || {
        home.map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("/"))
    };
    let full = if path.is_empty() {
        root()
    } else if let Some(rest) = home_relative(path) {
        root().join(rest)
    } else if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        root().join(path)
    };
    let mut out = PathBuf::new();
    for c in full.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    // Windows の `C:` だけ（そのドライブの今のフォルダ）は、ドライブの根に。
    if matches!(
        out.components().collect::<Vec<_>>()[..],
        [Component::Prefix(_)]
    ) {
        out.push(std::path::MAIN_SEPARATOR_STR);
    }
    out
}

fn entry(e: &std::fs::DirEntry) -> DirectoryEntry {
    let name = e.file_name().to_string_lossy().into_owned();
    let link = e.file_type().is_ok_and(|t| t.is_symlink());
    // シンボリックリンクは、その先を見る。
    let meta = if link {
        std::fs::metadata(e.path()).ok()
    } else {
        e.metadata().ok()
    };
    let kind = match &meta {
        Some(m) if m.is_dir() => "folder",
        Some(m) if m.is_file() => "file",
        _ => "other",
    };
    let executable = meta
        .as_ref()
        .is_some_and(|m| m.is_file() && executable(m, &name));
    let repository = kind == "folder" && std::fs::symlink_metadata(e.path().join(".git")).is_ok();
    DirectoryEntry {
        name,
        kind: kind.to_owned(),
        link,
        executable,
        repository,
    }
}

#[cfg(unix)]
fn executable(meta: &std::fs::Metadata, _name: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn executable(_meta: &std::fs::Metadata, name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".exe", ".com", ".bat", ".cmd"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_home_relative_and_dot_paths() {
        let home = Path::new("/home/ada");
        assert_eq!(resolve("~", Some(home)), PathBuf::from("/home/ada"));
        assert_eq!(resolve("", Some(home)), PathBuf::from("/home/ada"));
        assert_eq!(
            resolve("~/bin/", Some(home)),
            PathBuf::from("/home/ada/bin")
        );
        assert_eq!(
            resolve("src/../bin", Some(home)),
            PathBuf::from("/home/ada/bin")
        );
        assert_eq!(resolve("/usr/./bin/..", Some(home)), PathBuf::from("/usr"));
        assert_eq!(resolve("~/x", None), PathBuf::from("/x"));
    }

    #[test]
    fn expands_only_the_home_folder() {
        let home = std::env::home_dir().unwrap();
        assert_eq!(expand_home("~"), home);
        assert_eq!(expand_home("~/src/repo"), home.join("src/repo"));
        assert_eq!(expand_home("/srv/repo"), PathBuf::from("/srv/repo"));
        assert_eq!(expand_home("~other/repo"), PathBuf::from("~other/repo"));
    }

    #[cfg(windows)]
    #[test]
    fn resolves_drives_on_windows() {
        let home = Path::new(r"C:\Users\ada");
        let shown = |path| resolve(path, Some(home)).display().to_string();
        assert_eq!(shown("~"), r"C:\Users\ada");
        assert_eq!(shown("~/prj/x"), r"C:\Users\ada\prj\x");
        assert_eq!(shown(r"~\prj"), r"C:\Users\ada\prj");
        assert_eq!(shown("C:"), r"C:\");
        assert_eq!(shown("D:/src/../bin"), r"D:\bin");
        assert_eq!(shown(r"C:\.."), r"C:\");
        // `/` は、ホームのあるドライブの根。
        assert_eq!(shown("/"), r"C:\");
    }
}
