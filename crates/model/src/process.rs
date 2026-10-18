//! 子プロセスの起動。

use std::ffi::OsStr;
use std::process::Command;

/// `program` を起動する `Command`。Windows では窓を出さない（`CREATE_NO_WINDOW`）。
///
/// 共有の daemon はコンソールを持たない（`DETACHED_PROCESS` で起動する）。コンソールを持たないプロセスが
/// git や ssh のようなコンソールのプログラムを起動すると、Windows はそのたびに新しいコンソールの窓を開く。
pub fn command(program: impl AsRef<OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}
