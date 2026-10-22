//! 子プロセスの起動。

use std::ffi::OsStr;
use std::process::Command;

/// `program` を起動する `Command`。Windows では窓を出さない（`CREATE_NO_WINDOW`）。Unix では、子のシグナルマスクを
/// 空にする。
///
/// 共有の daemon はコンソールを持たない（`DETACHED_PROCESS` で起動する）。コンソールを持たないプロセスが
/// git や ssh のようなコンソールのプログラムを起動すると、Windows はそのたびに新しいコンソールの窓を開く。
///
/// Rust の `Command` は、起動したスレッドのシグナルマスクをそのまま子に渡す。macOS のアプリ（AppKit・Swift
/// Concurrency）のスレッドは SIGCHLD などを塞いでいるので、そこから起動した git も、git が起動する git-remote-https や
/// credential helper も、塞いだまま動く。Git Credential Manager（.NET）は子の終了を SIGCHLD で知るので、起動した
/// `git version` が終わっても気づかず、HTTPS の push・fetch・pull が永遠に終わらなかった（端末からは通る）。
/// アプリが起動した daemon も同じマスクを受け継ぐ。塞がれたシグナルがあるときだけ、fork と exec の間でマスクを
/// 空にする（`pre_exec` があると posix_spawn の速い経路を使えないので、要らないときは付けない）。
pub fn command(program: impl AsRef<OsStr>) -> Command {
    // ここだけが Command::new を直接呼ぶ（clippy.toml の disallowed-methods）
    #[allow(unused_mut, clippy::disallowed_methods)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    if any_signal_blocked() {
        use std::os::unix::process::CommandExt;
        // SAFETY: fork と exec の間で、ゼロ埋めの集合（空）を pthread_sigmask に渡すだけ。どちらも
        // async-signal-safe で、メモリを確保しない。
        unsafe {
            cmd.pre_exec(|| {
                let empty: libc::sigset_t = std::mem::zeroed();
                libc::pthread_sigmask(libc::SIG_SETMASK, &empty, std::ptr::null_mut());
                Ok(())
            });
        }
    }
    cmd
}

/// このスレッドのシグナルマスクに、塞がれたシグナルがあるか。
#[cfg(unix)]
fn any_signal_blocked() -> bool {
    // SAFETY: 今のマスクを読むだけ（`set` が NULL なら `how` は無視される）。
    unsafe {
        let mut current: libc::sigset_t = std::mem::zeroed();
        if libc::pthread_sigmask(libc::SIG_BLOCK, std::ptr::null(), &mut current) != 0 {
            return false;
        }
        (1..libc::SIGXFSZ + 1).any(|signal| libc::sigismember(&current, signal) == 1)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// AppKit のスレッドのように SIGTERM を塞いだスレッドから起動しても、子は塞がれていない（自分に送った
    /// SIGTERM で死ぬ）。
    #[test]
    fn child_starts_with_an_empty_signal_mask() {
        use std::os::unix::process::ExitStatusExt;
        // SAFETY: このテストのスレッドのマスクに SIGTERM を足し、終わったら戻す。
        let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
        unsafe {
            libc::sigemptyset(&mut set);
            libc::sigaddset(&mut set, libc::SIGTERM);
            libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
        }
        assert!(any_signal_blocked());
        let output = command("sh")
            .args(["-c", "kill -TERM $$; echo survived"])
            .output()
            .expect("sh");
        unsafe {
            libc::pthread_sigmask(libc::SIG_UNBLOCK, &set, std::ptr::null_mut());
        }
        assert_eq!(
            output.status.signal(),
            Some(libc::SIGTERM),
            "the child inherited the blocked SIGTERM: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}
