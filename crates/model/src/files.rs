//! リポジトリの中のファイルの読み書き。コアはファイルに直接触れず、ここを通す（SSH 越しのリポジトリでは
//! daemon に送る）。パスは作業ツリーか `.git` からの相対で、外は指せない。

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use crate::Result;

/// パスの基点。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Root {
    Workdir,
    /// worktree ごとの `.git`。
    GitDir,
    /// refs・config がある `.git`（すべての worktree で共通。linked worktree でなければ `GitDir` と同じ）。
    CommonDir,
}

pub trait RepoFiles: Send + Sync {
    /// ファイルの中身。なければ `None`。
    fn read(&self, root: Root, path: &str) -> Result<Option<Vec<u8>>>;
    /// 書く。途中のディレクトリがなければ作る。
    fn write(&self, root: Root, path: &str, data: &[u8]) -> Result<()>;
    /// 消す。なければ何もしない。
    fn remove(&self, root: Root, path: &str) -> Result<()>;
    /// 実行の許可などのモード（Unix）。なければ `None`。
    fn mode(&self, root: Root, path: &str) -> Result<Option<u32>>;
    fn set_mode(&self, root: Root, path: &str, mode: u32) -> Result<()>;
}

/// 手元のディスクのリポジトリ。
pub struct LocalFiles {
    workdir: Option<PathBuf>,
    git_dir: PathBuf,
    common_dir: PathBuf,
}

impl LocalFiles {
    pub fn new(git_dir: impl Into<PathBuf>, workdir: Option<PathBuf>) -> Arc<Self> {
        let git_dir = git_dir.into();
        // linked worktree の `.git` には、共通の git dir の場所が `commondir` に書いてある（相対なら `.git` から）。
        let common_dir = std::fs::read_to_string(git_dir.join("commondir"))
            .ok()
            .map(|dir| git_dir.join(dir.trim()))
            .unwrap_or_else(|| git_dir.clone());
        Arc::new(Self {
            workdir,
            git_dir,
            common_dir,
        })
    }

    fn resolve(&self, root: Root, path: &str) -> Result<PathBuf> {
        let base = match root {
            Root::Workdir => self
                .workdir
                .as_ref()
                .ok_or_else(|| crate::tr!("The repository has no working tree"))?,
            Root::GitDir => &self.git_dir,
            Root::CommonDir => &self.common_dir,
        };
        let relative = Path::new(path);
        if !relative
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
        {
            return Err(
                crate::tr!("Can’t point outside the repository: {path}", path = path).into(),
            );
        }
        Ok(base.join(relative))
    }
}

impl RepoFiles for LocalFiles {
    fn read(&self, root: Root, path: &str) -> Result<Option<Vec<u8>>> {
        match std::fs::read(self.resolve(root, path)?) {
            Ok(data) => Ok(Some(data)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn write(&self, root: Root, path: &str, data: &[u8]) -> Result<()> {
        let file = self.resolve(root, path)?;
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        Ok(std::fs::write(file, data)?)
    }

    fn remove(&self, root: Root, path: &str) -> Result<()> {
        match std::fs::remove_file(self.resolve(root, path)?) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }

    #[cfg(unix)]
    fn mode(&self, root: Root, path: &str) -> Result<Option<u32>> {
        use std::os::unix::fs::PermissionsExt;
        Ok(std::fs::metadata(self.resolve(root, path)?)
            .ok()
            .map(|m| m.permissions().mode()))
    }

    /// Windows には実行の権限がなく、git はフックなどを権限によらず実行するので、あるファイルは実行できるものとする。
    #[cfg(not(unix))]
    fn mode(&self, root: Root, path: &str) -> Result<Option<u32>> {
        Ok(std::fs::metadata(self.resolve(root, path)?)
            .ok()
            .map(|m| if m.is_dir() { 0o040_755 } else { 0o100_755 }))
    }

    #[cfg(unix)]
    fn set_mode(&self, root: Root, path: &str, mode: u32) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        Ok(std::fs::set_permissions(
            self.resolve(root, path)?,
            std::fs::Permissions::from_mode(mode),
        )?)
    }

    #[cfg(not(unix))]
    fn set_mode(&self, _root: Root, _path: &str, _mode: u32) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_writes_and_refuses_paths_outside() {
        let dir = std::env::temp_dir().join(format!("amonhen-files-{}", std::process::id()));
        let files = LocalFiles::new(dir.join(".git"), Some(dir.clone()));
        files.write(Root::Workdir, "a/b.txt", b"x").unwrap();
        assert_eq!(
            files.read(Root::Workdir, "a/b.txt").unwrap(),
            Some(b"x".to_vec())
        );
        assert_eq!(files.read(Root::Workdir, "missing").unwrap(), None);
        files
            .write(Root::GitDir, "amonhen/undo.json", b"[]")
            .unwrap();
        assert!(dir.join(".git/amonhen/undo.json").exists());
        files.remove(Root::Workdir, "a/b.txt").unwrap();
        files.remove(Root::Workdir, "a/b.txt").unwrap();
        assert!(files.read(Root::Workdir, "../escape").is_err());
        assert!(files.write(Root::Workdir, "/etc/passwd", b"").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
