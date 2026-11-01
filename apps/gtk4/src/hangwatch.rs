//! メインスレッドが止まった瞬間の記録（開発用。`AMONHEN_HANG_LOG=<フォルダ>` を付けて起動したときだけ動く。
//! macOS 版の `HangWatch.swift`）。裏のスレッドから 50 ms ごとにメインループへ合図を送り、0.15 秒返事がなければ、
//! その最中のスタックを採って `hang-<時刻>.txt` に残し（macOS は `sample`、ほかは `gdb`。なければ採らない）、
//! 止まっていた時間を `hangs.log` に書く。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use gtk::glib;

pub fn start_if_requested() {
    let Some(dir) = std::env::var_os("AMONHEN_HANG_LOG").filter(|d| !d.is_empty()) else {
        return;
    };
    let dir = expand_tilde(PathBuf::from(dir));
    let _ = std::fs::create_dir_all(&dir);
    let pid = std::process::id();
    let context = glib::MainContext::default();
    std::thread::spawn(move || {
        loop {
            let started = Instant::now();
            let stamp = glib::DateTime::now_local()
                .ok()
                .and_then(|t| t.format_iso8601().ok())
                .map_or_else(
                    || format!("{:?}", std::time::SystemTime::now()),
                    |s| s.to_string(),
                );
            let (tx, rx) = mpsc::channel();
            context.invoke(move || {
                let _ = tx.send(());
            });
            if rx.recv_timeout(Duration::from_millis(150)).is_err() {
                sample(pid, &dir.join(format!("hang-{stamp}.txt")));
                // 返事が来るまで待つ（止まっていた時間を測る）。
                let _ = rx.recv();
                let ms = started.elapsed().as_secs_f64() * 1000.0;
                if let Ok(mut log) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(dir.join("hangs.log"))
                {
                    let _ = writeln!(log, "{stamp} {ms:.0} ms");
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    });
}

/// 止まっているメインスレッドのスタックを `out` に採る（待たない）。
fn sample(pid: u32, out: &Path) {
    let pid = pid.to_string();
    let mut command = if cfg!(target_os = "macos") {
        let mut c = amonhen_model::process::command("/usr/bin/sample");
        c.args([pid.as_str(), "1", "1", "-file"])
            .arg(out)
            .stdout(std::process::Stdio::null());
        c
    } else {
        // gdb がなければ採れない（止まった時間だけ書く）。
        let Ok(file) = std::fs::File::create(out) else {
            return;
        };
        let mut c = amonhen_model::process::command("gdb");
        c.args(["-batch", "-p", pid.as_str(), "-ex", "thread apply 1 bt"])
            .stdout(file);
        c
    };
    let spawned = command
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    // 終わりを待って片付ける（待たないと終わったプロセスが残る）。
    if let Ok(mut child) = spawned {
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

fn expand_tilde(path: PathBuf) -> PathBuf {
    match (path.strip_prefix("~"), std::env::var_os("HOME")) {
        (Ok(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => path,
    }
}
