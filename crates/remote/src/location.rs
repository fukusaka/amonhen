//! リポジトリの場所の書き方と、daemon を起動するコマンド。
//!
//! - `daemon:/path/to/repo` … 手元の共有の daemon（`crate::local`）で開く
//! - `ssh://[user@]host[:port]/path/to/repo` … SSH 先に daemon を送り込んで起動する（`crate::ssh`）

/// 手元のパスではなく、daemon 越しに開く場所か。
pub fn is_remote(location: &str) -> bool {
    location.starts_with("daemon:") || location.starts_with("ssh://")
}

/// `daemon:` の場所の、リポジトリのパス。
pub fn local_path(location: &str) -> Result<&str, String> {
    match location.strip_prefix("daemon:") {
        Some("") => Err("daemon: の後にリポジトリのパスを書いてください".into()),
        Some(path) => Ok(path),
        None => Err(format!("daemon 越しの場所ではありません: {location}")),
    }
}

/// `daemon:` の場所を、その場で子プロセスとして起動する daemon のコマンド（プログラムと引数）。`daemon` は手元の
/// `amonhend` のパス。共有の daemon を使わずに確かめるとき用。
pub fn launch_command(location: &str, daemon: &str) -> Result<(String, Vec<String>), String> {
    if let Some(path) = location.strip_prefix("daemon:") {
        if path.is_empty() {
            return Err("daemon: の後にリポジトリのパスを書いてください".into());
        }
        return Ok((daemon.to_owned(), vec!["--stdio".into(), path.into()]));
    }
    if location.starts_with("ssh://") {
        return Err("ssh:// の場所は ssh::connect でつなぐ（daemon を送り込むため）".into());
    }
    Err(format!("daemon 越しの場所ではありません: {location}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_command_for_a_local_daemon() {
        assert!(is_remote("daemon:/r") && is_remote("ssh://h/r") && !is_remote("/r"));
        assert_eq!(
            launch_command("daemon:/work/repo", "/bin/amonhend").unwrap(),
            (
                "/bin/amonhend".into(),
                vec!["--stdio".into(), "/work/repo".into()]
            )
        );
        assert!(launch_command("ssh://dev/repo", "x").is_err());
        assert!(launch_command("daemon:", "x").is_err());
    }
}
