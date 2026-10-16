//! 端末まわり（幅・高さ・tty か・時刻の表示・出力）。

use std::io::Write;

pub fn stdout_is_tty() -> bool {
    #[cfg(unix)]
    {
        // SAFETY: 標準出力の番号を渡すだけ。
        unsafe { libc::isatty(libc::STDOUT_FILENO) == 1 }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

fn size() -> Option<(usize, usize)> {
    #[cfg(unix)]
    {
        // SAFETY: winsize を受け取る領域を渡すだけ。
        let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
        let ok = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) } == 0;
        (ok && ws.ws_col > 0).then_some((ws.ws_col as usize, ws.ws_row as usize))
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// 端末の幅（tty でなければ、切り詰めない）。
pub fn width() -> Option<usize> {
    if !stdout_is_tty() {
        return None;
    }
    std::env::var("COLUMNS")
        .ok()
        .and_then(|c| c.parse().ok())
        .or(size().map(|s| s.0))
}

pub fn height() -> usize {
    size().map_or(40, |s| s.1)
}

/// UNIX 時刻を手元の時刻で（`with_time` なら時刻まで）。
pub fn local_time(time: i64, with_time: bool) -> String {
    #[cfg(unix)]
    {
        // SAFETY: tm を受け取る領域を渡すだけ。
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        let t = time as libc::time_t;
        if unsafe { !libc::localtime_r(&t, &mut tm).is_null() } {
            let date = format!(
                "{:04}-{:02}-{:02}",
                tm.tm_year + 1900,
                tm.tm_mon + 1,
                tm.tm_mday
            );
            return if with_time {
                format!("{date} {:02}:{:02}", tm.tm_hour, tm.tm_min)
            } else {
                date
            };
        }
    }
    let _ = with_time;
    time.to_string()
}

/// まとめて書く。パイプの先が閉じても（`| head`）エラーにしない。
pub fn write(text: &str) {
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(text.as_bytes());
    let _ = out.flush();
}
