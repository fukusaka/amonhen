//! 手元に常駐する共有の daemon（`amonhend --serve`）につなぐ側と、つないだときの取り決め。
//!
//! daemon は利用者ごとに 1 つ、Unix ドメインソケットで待つ。アプリ・CLI・VSCode 拡張は、同じリポジトリを
//! 開くと同じ daemon の同じ `GitBackend`（git の読み込みと監視）を共有する。つないだら、最初に開くリポジトリの
//! パスを送り（長さ u32 と UTF-8）、その後はふつうのフレーム（`crate::frame`）でやりとりする。
//! ソケットの名前にプロトコルの版を入れるので、版の違う daemon にはつながない。

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use amonhen_model::Result;

use crate::Connection;

/// 共有の daemon のソケット。macOS は利用者ごとの `$TMPDIR`、ほかは `$XDG_RUNTIME_DIR`（なければ
/// `/tmp/amonhen-<uid>`）に置く。
pub fn socket_path() -> PathBuf {
    let dir = std::env::var_os("AMONHEN_DAEMON_DIR")
        .or_else(|| if cfg!(target_os = "macos") { std::env::var_os("TMPDIR") } else { None })
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
        return Err(std::io::Error::other("リポジトリのパスが長すぎる"));
    }
    let mut path = vec![0u8; len];
    input.read_exact(&mut path)?;
    String::from_utf8(path).map_err(std::io::Error::other)
}

/// 共有の daemon で `repo` を開いてつなぐ。daemon がいなければ `daemon`（`amonhend` のパス）を起動してから。
#[cfg(unix)]
pub fn connect(repo: &str, daemon: &str) -> Result<Arc<Connection>> {
    use std::os::unix::net::UnixStream;
    let socket = socket_path();
    let stream = match UnixStream::connect(&socket) {
        Ok(s) => s,
        Err(_) => {
            launch(daemon, &socket)?;
            wait_for(&socket, Duration::from_secs(5))?
        }
    };
    let mut writer = stream.try_clone()?;
    write_preamble(&mut writer, repo)?;
    Ok(Connection::new(stream, std::io::BufWriter::new(writer)))
}

#[cfg(not(unix))]
pub fn connect(_repo: &str, _daemon: &str) -> Result<Arc<Connection>> {
    Err("共有の daemon は Unix だけです".into())
}

/// daemon を、つないだ側と切り離して起動する（つないだ側が終わっても残る）。
#[cfg(unix)]
fn launch(daemon: &str, socket: &Path) -> Result<()> {
    use std::os::unix::process::CommandExt;
    std::process::Command::new(daemon)
        .arg("--serve")
        .arg(socket)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|e| format!("{daemon} を起動できない: {e}"))?;
    Ok(())
}

#[cfg(unix)]
fn wait_for(socket: &Path, limit: Duration) -> Result<std::os::unix::net::UnixStream> {
    let started = Instant::now();
    loop {
        match std::os::unix::net::UnixStream::connect(socket) {
            Ok(s) => return Ok(s),
            Err(e) if started.elapsed() > limit => {
                return Err(format!("daemon（{}）につなげない: {e}", socket.display()).into());
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
        assert_eq!(super::read_preamble(&mut buf.as_slice()).unwrap(), "/work/リポジトリ");
    }
}
