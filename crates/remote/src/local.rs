//! 手元に常駐する共有の daemon（`amonhend --serve`）につなぐ側と、つないだときの取り決め。
//!
//! daemon は利用者ごとに 1 つ、Unix ドメインソケット（Windows は名前付きパイプ。`crate::endpoint`）で待つ。アプリ・CLI・VSCode 拡張は、同じリポジトリを
//! 開くと同じ daemon の同じ `GitBackend`（git の読み込みと監視）を共有する。つないだら、最初に開くリポジトリの
//! パスを送り（長さ u32 と UTF-8）、その後はふつうのフレーム（`crate::frame`）でやりとりする。
//! ソケットの名前にプロトコルの版を入れるので、版の違う daemon にはつながない。

use amonhen_model::tr;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use amonhen_model::Result;

use crate::Connection;

/// 共有の daemon のソケット。macOS は利用者ごとの `$TMPDIR`、ほかは `$XDG_RUNTIME_DIR`（なければ
/// `/tmp/amonhen-<uid>`）に置く。
pub fn socket_path() -> PathBuf {
    let dir = std::env::var_os("AMONHEN_DAEMON_DIR")
        .or_else(|| {
            if cfg!(target_os = "macos") {
                std::env::var_os("TMPDIR")
            } else {
                None
            }
        })
        .or_else(|| std::env::var_os("XDG_RUNTIME_DIR"))
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            #[cfg(unix)]
            // SAFETY: 引数のない呼び出し。
            let uid = unsafe { libc::getuid() };
            #[cfg(not(unix))]
            let uid = 0;
            PathBuf::from(format!("/tmp/amonhen-{uid}"))
        });
    dir.join(format!("amonhend-{}.sock", crate::PROTOCOL_VERSION))
}

/// History の永続キャッシュの置き場所（アプリと同じ）: macOS は `~/Library/Caches/io.github.fukusaka.amonhen`、Windows は
/// `%LOCALAPPDATA%\amonhen\cache`、ほかは `$XDG_CACHE_HOME/amonhen`（なければ `~/.cache/amonhen`）。
pub fn cache_dir() -> Option<PathBuf> {
    let dir = if cfg!(windows) {
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
            .join("amonhen")
            .join("cache")
    } else {
        let home = PathBuf::from(std::env::var_os("HOME")?);
        if cfg!(target_os = "macos") {
            home.join("Library/Caches/io.github.fukusaka.amonhen")
        } else {
            std::env::var_os("XDG_CACHE_HOME")
                .map_or_else(|| home.join(".cache"), PathBuf::from)
                .join("amonhen")
        }
    };
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// 手元のリポジトリの場所を、daemon で共有するときの名前に（絶対パス。Windows の `\\?\C:\...` は `C:\...` に）。
pub fn canonical_path(path: &str) -> std::io::Result<String> {
    let full = std::fs::canonicalize(path)?.display().to_string();
    Ok(match full.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => rest.to_owned(),
        _ => full,
    })
}

/// つないで最初に送るものが、この文字で始まれば、リポジトリではなく daemon への指示（`status`・`stop`）。
pub const CONTROL: char = '\0';

/// 共有の daemon に指示を送り、答え（`status` なら JSON）を返す。daemon が動いていなければ `None`。
pub fn control(command: &str) -> Result<Option<String>> {
    let Ok(mut duplex) = crate::endpoint::connect(&crate::endpoint::endpoint()) else {
        return Ok(None);
    };
    write_preamble(&mut duplex.writer, &format!("{CONTROL}{command}"))?;
    // 答えは 5 秒まで待つ。
    let (tx, rx) = std::sync::mpsc::channel();
    let mut reader = std::io::BufReader::new(duplex.reader);
    std::thread::spawn(move || {
        let _ = tx.send(crate::frame::read(&mut reader));
    });
    let frame = rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| tr!("The daemon didn’t answer"))??;
    match frame {
        Some(f) if f.kind == crate::frame::Kind::Ok => {
            Ok(Some(String::from_utf8_lossy(&f.payload).into_owned()))
        }
        Some(f) => Err(String::from_utf8_lossy(&f.payload).into_owned().into()),
        None => Err(tr!("The daemon closed without answering").into()),
    }
}

/// 共有の daemon を起動する（もう動いていれば何もしない）。
pub fn start(daemon: &str) -> Result<()> {
    let endpoint = crate::endpoint::endpoint();
    if crate::endpoint::connect(&endpoint).is_ok() {
        return Ok(());
    }
    launch(daemon, &endpoint)?;
    wait_for(&endpoint, Duration::from_secs(5)).map(drop)
}

/// つないで最初に送る、開くリポジトリのパス。
pub fn write_preamble(out: &mut impl Write, repo: &str) -> std::io::Result<()> {
    out.write_all(&(repo.len() as u32).to_le_bytes())?;
    out.write_all(repo.as_bytes())?;
    out.flush()
}

pub fn read_preamble(input: &mut impl Read) -> std::io::Result<String> {
    let mut len = [0u8; 4];
    input.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len > 64 * 1024 {
        return Err(std::io::Error::other(tr!(
            "The repository path is too long"
        )));
    }
    let mut path = vec![0u8; len];
    input.read_exact(&mut path)?;
    String::from_utf8(path).map_err(std::io::Error::other)
}

/// 共有の daemon で `repo` を開いてつなぐ。daemon がいなければ `daemon`（エンジンのパス）を起動してから。
pub fn connect(repo: &str, daemon: &str) -> Result<Arc<Connection>> {
    let mut duplex = stream(daemon)?;
    write_preamble(&mut duplex.writer, repo)?;
    let conn = Connection::new(duplex.reader, std::io::BufWriter::new(duplex.writer));
    conn.mark_local();
    Ok(conn)
}

/// 共有の daemon につないだ通り道（最初に送るものはまだ送っていない）。daemon がいなければ `daemon` を起動してから。
pub fn stream(daemon: &str) -> Result<crate::endpoint::Duplex> {
    let endpoint = crate::endpoint::endpoint();
    match crate::endpoint::connect(&endpoint) {
        Ok(s) => Ok(s),
        Err(_) => {
            launch(daemon, &endpoint)?;
            wait_for(&endpoint, Duration::from_secs(5))
        }
    }
}

/// daemon を、つないだ側と切り離して起動する（つないだ側が終わっても残る）。
fn launch(daemon: &str, endpoint: &str) -> Result<()> {
    let mut command = std::process::Command::new(daemon);
    command
        .arg("--serve")
        .arg(endpoint)
        // 共有の daemon が SSH 先につなぐときなどに使う、既定の言語（起動した側の言語）。
        .env("AMONHEN_LANG", amonhen_model::i18n::current().code())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // 窓を出さず、親のコンソールから切り離す（DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP）。
        command.creation_flags(0x0000_0008 | 0x0000_0200);
        // Windows では、引き継げる形のハンドルはすべて子に渡る。自分の標準入出力（呼んだ側のパイプのことがある）を
        // daemon に渡すと、daemon が終わるまで呼んだ側はこちらが終わったと分からない。起動する間だけ渡さない。
    }
    #[cfg(windows)]
    let spawned = no_inherit_std(|| command.spawn());
    #[cfg(not(windows))]
    let spawned = command.spawn();
    spawned
        .map(drop)
        .map_err(|e| tr!("Can’t start {program}: {e}", program = daemon, e = e).into())
}

/// `f` の間だけ、自分の標準入出力のハンドルを子に引き継がない形にする（終わったら戻す）。
#[cfg(windows)]
fn no_inherit_std<T>(f: impl FnOnce() -> T) -> T {
    use windows_sys::Win32::Foundation::{
        GetHandleInformation, HANDLE_FLAG_INHERIT, SetHandleInformation,
    };
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    let mut saved = Vec::new();
    // SAFETY: 自分の標準入出力のハンドルの、引き継ぎの印を読み書きするだけ。
    unsafe {
        for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            let h = GetStdHandle(which);
            let mut flags = 0u32;
            if !h.is_null()
                && GetHandleInformation(h, &mut flags) != 0
                && flags & HANDLE_FLAG_INHERIT != 0
            {
                SetHandleInformation(h, HANDLE_FLAG_INHERIT, 0);
                saved.push(h);
            }
        }
    }
    let result = f();
    // SAFETY: 上で外した印を戻すだけ。
    unsafe {
        for h in saved {
            SetHandleInformation(h, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT);
        }
    }
    result
}

fn wait_for(endpoint: &str, limit: Duration) -> Result<crate::endpoint::Duplex> {
    let started = Instant::now();
    loop {
        match crate::endpoint::connect(endpoint) {
            Ok(s) => return Ok(s),
            Err(e) if started.elapsed() > limit => {
                return Err(tr!(
                    "Can’t connect to the daemon ({endpoint}): {e}",
                    endpoint = endpoint,
                    e = e
                )
                .into());
            }
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn round_trips_the_preamble() {
        let mut buf = Vec::new();
        super::write_preamble(&mut buf, "/work/リポジトリ").unwrap();
        assert_eq!(
            super::read_preamble(&mut buf.as_slice()).unwrap(),
            "/work/リポジトリ"
        );
    }
}
