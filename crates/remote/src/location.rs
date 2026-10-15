//! リポジトリの場所の書き方と、daemon を起動するコマンド。
//!
//! - `daemon:/path/to/repo` … 手元で daemon を起動する（確認用）
//! - `ssh://[user@]host[:port]/path/to/repo` … SSH 先で `amonhend` を起動する

/// 手元のパスではなく、daemon 越しに開く場所か。
pub fn is_remote(location: &str) -> bool {
    location.starts_with("daemon:") || location.starts_with("ssh://")
}

/// daemon を起動するコマンド（プログラムと引数）。`daemon` は手元の `amonhend` のパス。
pub fn launch_command(location: &str, daemon: &str) -> Result<(String, Vec<String>), String> {
    if let Some(path) = location.strip_prefix("daemon:") {
        if path.is_empty() {
            return Err("daemon: の後にリポジトリのパスを書いてください".into());
        }
        return Ok((daemon.to_owned(), vec!["--stdio".into(), path.into()]));
    }
    if let Some(rest) = location.strip_prefix("ssh://") {
        let (authority, path) = rest
            .find('/')
            .map(|i| (&rest[..i], &rest[i..]))
            .ok_or("ssh://host/path の形で書いてください")?;
        let (target, port) = match authority.rsplit_once(':') {
            Some((t, p)) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => (t, Some(p)),
            _ => (authority, None),
        };
        if target.is_empty() {
            return Err("SSH の接続先がありません".into());
        }
        let mut args = vec!["-T".into(), "-o".into(), "BatchMode=yes".into()];
        if let Some(p) = port {
            args.extend(["-p".into(), p.into()]);
        }
        // SSH 先ではシェルが読むので、パスは引用符で囲む。
        args.extend([
            target.to_owned(),
            "amonhend".into(),
            "--stdio".into(),
            shell_quote(path),
        ]);
        return Ok(("ssh".into(), args));
    }
    Err(format!("daemon 越しの場所ではありません: {location}"))
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_commands_for_each_kind_of_location() {
        assert!(is_remote("daemon:/r") && is_remote("ssh://h/r") && !is_remote("/r"));
        assert_eq!(
            launch_command("daemon:/work/repo", "/bin/amonhend").unwrap(),
            (
                "/bin/amonhend".into(),
                vec!["--stdio".into(), "/work/repo".into()]
            )
        );
        let (program, args) =
            launch_command("ssh://me@dev.example:2222/home/me/it's", "x").unwrap();
        assert_eq!(program, "ssh");
        assert_eq!(
            args,
            [
                "-T",
                "-o",
                "BatchMode=yes",
                "-p",
                "2222",
                "me@dev.example",
                "amonhend",
                "--stdio",
                r"'/home/me/it'\''s'"
            ]
        );
        let (_, args) = launch_command("ssh://dev/repo", "x").unwrap();
        assert_eq!(args[3..], ["dev", "amonhend", "--stdio", "'/repo'"]);
        assert!(launch_command("ssh://dev", "x").is_err());
        assert!(launch_command("daemon:", "x").is_err());
    }
}
