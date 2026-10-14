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
