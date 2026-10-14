//! `git` を起動して行う操作（デスクトップ向け）。書き込み系の操作は正しさと互換性を優先して
//! git 本体に任せる。読み取りでも、git の仕組み（untracked cache・fsmonitor）が効く status はここ。

mod status;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use amonhen_model::Result;

pub use status::parse_status;

/// 1 つのリポジトリで git を実行する。
#[derive(Clone, Debug)]
pub struct Git {
    workdir: PathBuf,
}

impl Git {
    pub fn new(workdir: impl Into<PathBuf>) -> Self {
        Self {
            workdir: workdir.into(),
        }
    }

    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new("git");
        cmd.current_dir(&self.workdir)
            // 利用者の端末に依存しない出力にする。
            .env("LC_ALL", "C")
            .env("GIT_TERMINAL_PROMPT", "0")
            .args(["-c", "core.quotepath=off", "-c", "color.ui=false"])
            .args(args);
        cmd
    }

    /// 読み取り系。利用者自身の git 操作とロックを奪い合わないよう、任意のロックを取らない。
    pub fn read(&self, args: &[&str]) -> Result<Vec<u8>> {
        let mut cmd = self.command(args);
        cmd.env("GIT_OPTIONAL_LOCKS", "0");
        run(cmd, None)
    }

    /// 書き込み系。
    pub fn write(&self, args: &[&str]) -> Result<Vec<u8>> {
        run(self.command(args), None)
    }

    /// 書き込み系で、標準入力を渡す。
    pub fn write_with_input(&self, args: &[&str], input: &[u8]) -> Result<Vec<u8>> {
        run(self.command(args), Some(input))
    }
}

fn run(mut cmd: Command, input: Option<&[u8]>) -> Result<Vec<u8>> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.stdin(if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    let mut child = cmd.spawn()?;
    if let Some(input) = input {
        // 大きな入力で詰まらないよう、書き込みは別のスレッドで行う。
        let mut stdin = child.stdin.take().unwrap();
        let input = input.to_vec();
        std::thread::spawn(move || stdin.write_all(&input));
    }
    let out = child.wait_with_output()?;
    if !out.status.success() {
        let message = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        return Err(if message.is_empty() {
            format!("git が失敗した: {:?}", out.status)
        } else {
            message
        }
        .into());
    }
    Ok(out.stdout)
}

/// 作業ツリーと index の操作。
impl Git {
    pub fn status(&self) -> Result<amonhen_model::Status> {
        parse_status(&self.read(&[
            "status",
            "--porcelain=v2",
            "-z",
            "--branch",
            "--untracked-files=all",
        ])?)
    }

    /// `paths` の変更（追加・削除を含む）を index に入れる。
    pub fn stage(&self, paths: &[&str]) -> Result<()> {
        let mut args = vec!["add", "-A", "--"];
        args.extend(paths);
        self.write(&args).map(drop)
    }

    /// `paths` を HEAD の状態に戻す（index だけ）。HEAD がなければ index から外す。
    pub fn unstage(&self, paths: &[&str]) -> Result<()> {
        let has_head = self.read(&["rev-parse", "-q", "--verify", "HEAD"]).is_ok();
        let mut args = if has_head {
            vec!["reset", "-q", "--"]
        } else {
            vec!["rm", "--cached", "-q", "-r", "--"]
        };
        args.extend(paths);
        self.write(&args).map(drop)
    }

    /// `path` の index の中身を `content` にする（行単位の stage・unstage に使う）。
    pub fn set_index_content(&self, path: &str, mode: u32, content: &[u8]) -> Result<()> {
        // 作業ツリーのファイルではないので、改行の変換などのフィルターは通さない。
        let oid =
            self.write_with_input(&["hash-object", "-w", "--no-filters", "--stdin"], content)?;
        let oid = String::from_utf8(oid)?;
        let entry = format!("{mode:o},{},{path}", oid.trim());
        self.write(&["update-index", "--add", "--cacheinfo", &entry])
            .map(drop)
    }

    /// stage した変更をコミットする。`amend` なら直前のコミットを作り直す。
    pub fn commit(&self, message: &str, amend: bool) -> Result<()> {
        let mut args = vec!["commit", "-q", "--cleanup=strip", "-F", "-"];
        if amend {
            args.push("--amend");
        }
        self.write_with_input(&args, message.as_bytes()).map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> (tempfile::TempDir, Git) {
        let tmp = tempfile::tempdir().unwrap();
        let git = Git::new(tmp.path());
        for args in [
            &["init", "-q", "-b", "main"][..],
            &["config", "user.name", "a"],
            &["config", "user.email", "a@example.com"],
        ] {
            git.write(args).unwrap();
        }
        (tmp, git)
    }

    #[test]
    fn stages_unstages_and_commits() {
        let (tmp, git) = repo();
        std::fs::write(tmp.path().join("a.txt"), "1\n").unwrap();
        assert_eq!(
            git.status().unwrap().entries[0].kind,
            amonhen_model::StatusKind::Untracked
        );

        git.stage(&["a.txt"]).unwrap();
        assert!(git.status().unwrap().entries[0].is_staged());
        // HEAD がなくても外せる。
        git.unstage(&["a.txt"]).unwrap();
        assert!(!git.status().unwrap().entries[0].is_staged());

        git.stage(&["a.txt"]).unwrap();
        git.commit("first\n\nbody", false).unwrap();
        assert!(git.status().unwrap().entries.is_empty());
        let log = git.read(&["log", "--format=%B"]).unwrap();
        assert_eq!(String::from_utf8(log).unwrap().trim(), "first\n\nbody");

        std::fs::write(tmp.path().join("a.txt"), "1\n2\n3\n").unwrap();
        git.set_index_content("a.txt", 0o100644, b"1\n2\n").unwrap();
        let s = git.status().unwrap();
        assert_eq!((s.entries[0].index, s.entries[0].worktree), (b'M', b'M'));
        let staged = git.read(&["show", ":a.txt"]).unwrap();
        assert_eq!(staged, b"1\n2\n");

        git.commit("second", false).unwrap();
        git.commit("second (amended)", true).unwrap();
        let log = git.read(&["log", "--format=%s"]).unwrap();
        assert_eq!(String::from_utf8(log).unwrap(), "second (amended)\nfirst\n");
    }
}
