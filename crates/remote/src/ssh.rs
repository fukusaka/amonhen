//! SSH 先の daemon。初めてつなぐ SSH 先には、手元に同梱した daemon を送り込んでから起動する
//! （VSCode の Remote-SSH と同じ形）。
//!
//! 1. SSH の接続は使い回す（ControlMaster）。調べる・送る・起動するを 1 回のログインで済ませる。
//! 2. SSH 先の OS と CPU（`uname -sm`）を調べ、同じ daemon がもう置いてあるかを見る。
//! 3. なければ `~/.cache/amonhen/amonhend-<中身のハッシュ>` に送る（版が変われば別の名前になる）。
//! 4. それを `--stdio <パス>` で起動し、標準入出力でつなぐ。

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;

use amonhen_model::Result;

use crate::Connection;

/// `ssh://[user@]host[:port]/path` を分けたもの。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// `user@host` か `host`。
    pub destination: String,
    pub port: Option<String>,
    /// SSH 先のリポジトリのパス。`/~/` で始まればホームからの相対。
    pub path: String,
}

impl Target {
    pub fn parse(location: &str) -> Result<Self> {
        let rest = location
            .strip_prefix("ssh://")
            .ok_or("ssh:// で始まる場所ではありません")?;
        let slash = rest
            .find('/')
            .ok_or("ssh://host/path の形で書いてください")?;
        let (authority, path) = (&rest[..slash], &rest[slash..]);
        let (destination, port) = match authority.rsplit_once(':') {
            Some((d, p)) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => {
                (d, Some(p.to_owned()))
            }
            _ => (authority, None),
        };
        if destination.is_empty() {
            return Err("SSH の接続先がありません".into());
        }
        Ok(Self {
            destination: destination.to_owned(),
            port,
            path: path.to_owned(),
        })
    }

    /// SSH 先のシェルに渡すパス（`/~/x` は `"$HOME"/'x'`、それ以外は引用符で囲む）。
    fn shell_path(&self) -> String {
        match self.path.strip_prefix("/~/") {
            Some(rest) => format!("\"$HOME\"/{}", shell_quote(rest)),
            None => shell_quote(&self.path),
        }
    }
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// 送り込む daemon の置き場所。手元の `dir` に `amonhend-<os>-<arch>` を置いておく。
pub struct Daemons {
    pub dir: PathBuf,
    /// 手元（macOS）の daemon。SSH 先も同じ OS と CPU なら、これを送る。
    pub local: Option<(String, PathBuf)>,
}

impl Daemons {
    /// `uname -sm` の結果に合う daemon。
    fn for_platform(&self, uname: &str) -> Result<PathBuf> {
        let mut words = uname.split_whitespace();
        let os = words.next().unwrap_or_default().to_ascii_lowercase();
        let arch = match words.next().unwrap_or_default() {
            "arm64" | "aarch64" => "aarch64",
            "x86_64" | "amd64" => "x86_64",
            other => other,
        };
        let name = format!("{os}-{arch}");
        let file = self.dir.join(format!("amonhend-{name}"));
        if file.exists() {
            return Ok(file);
        }
        if let Some((local_name, path)) = &self.local
            && *local_name == name
        {
            return Ok(path.clone());
        }
        Err(format!(
            "SSH 先（{}）に合う amonhend を同梱していません",
            uname.trim()
        )
        .into())
    }
}

/// ControlMaster のソケットの場所。Unix ソケットのパスは 104 バイトまでなので、`$TMPDIR` が長い
/// （macOS の `/var/folders/...`）ときは `/tmp` に置く。`%C` は 40 文字で、ssh はさらに 17 文字ほどの
/// 一時的な接尾辞を付ける。`/tmp` では、ほかの利用者と分けるため `%i`（手元の uid）を入れる。
fn control_path() -> String {
    let tmp = std::env::temp_dir().join("amonhen-ssh-%C");
    let tmp = tmp.display().to_string();
    if tmp.len() - "%C".len() + 40 + 17 <= 100 {
        tmp
    } else {
        "/tmp/amonhen-ssh-%i-%C".to_owned()
    }
}

/// SSH の共通の引数（接続先まで）。
fn base_args(target: &Target) -> Vec<String> {
    let mut args = vec![
        "-T".to_owned(),
        // パスワードは聞けない（画面がない）。鍵か ssh-agent でつなぐ。
        "-o".into(),
        "BatchMode=yes".into(),
        // 調べる・送る・起動するで、同じ接続を使い回す。
        "-o".into(),
        "ControlMaster=auto".into(),
        "-o".into(),
        format!("ControlPath={}", control_path()),
        "-o".into(),
        "ControlPersist=60".into(),
    ];
    if let Some(p) = &target.port {
        args.extend(["-p".into(), p.clone()]);
    }
    args.push(target.destination.clone());
    args
}

/// SSH 先で `script` を実行し、標準出力を返す。`input` は標準入力に流す。
fn run(ssh: &str, target: &Target, script: &str, input: Option<&[u8]>) -> Result<String> {
    let mut child = Command::new(ssh)
        .args(base_args(target))
        .arg(script)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{ssh} を起動できない: {e}"))?;
    if let Some(data) = input {
        let mut stdin = child.stdin.take().unwrap();
        let data = data.to_vec();
        std::thread::spawn(move || stdin.write_all(&data));
    }
    let out = child.wait_with_output()?;
    if !out.status.success() {
        let message = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        return Err(format!("SSH 先（{}）でエラー: {message}", target.destination).into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// SSH 先に daemon を用意して起動し、つなぐ。`ssh` は ssh のコマンド（テストでは偽物を渡す）。
pub fn connect(ssh: &str, target: &Target, daemons: &Daemons) -> Result<Arc<Connection>> {
    const DIR: &str = "\"$HOME\"/.cache/amonhen";
    // OS と CPU を調べ、置いてある daemon の名前を並べてもらう。
    let probe = run(
        ssh,
        target,
        &format!("uname -sm; ls {DIR} 2>/dev/null || true"),
        None,
    )?;
    let mut lines = probe.lines();
    let uname = lines.next().unwrap_or_default().to_owned();
    let present: Vec<&str> = lines.collect();
    let binary = daemons.for_platform(&uname)?;
    let data =
        std::fs::read(&binary).map_err(|e| format!("{} を読めない: {e}", binary.display()))?;
    let name = format!(
        "amonhend-{}",
        &sha1_smol::Sha1::from(&data).digest().to_string()[..16]
    );
    if !present.contains(&name.as_str()) {
        // 書きかけを使わないよう、一時的な名前で書いてから置き換える。
        let script = format!(
            "mkdir -p {DIR} && cat > {DIR}/{name}.tmp.$$ && chmod 755 {DIR}/{name}.tmp.$$ && mv {DIR}/{name}.tmp.$$ {DIR}/{name}"
        );
        run(ssh, target, &script, Some(&data))?;
    }
    let mut args = base_args(target);
    args.push(format!("{DIR}/{name} --stdio {}", target.shell_path()));
    Connection::spawn(ssh, &args)
}

/// 手元の OS と CPU を、`uname -sm` から作る名前と同じ形で（`darwin-aarch64` など）。
pub fn local_platform() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    format!("{os}-{}", std::env::consts::ARCH)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn control_path_fits_a_unix_socket() {
        // %C は 40 文字の hash、%i は uid に置き換わる。
        let path = control_path()
            .replace("%C", &"0".repeat(40))
            .replace("%i", "4294967295");
        assert!(path.len() + 17 <= 104, "{path}");
    }

    #[test]
    fn parses_ssh_locations() {
        let t = Target::parse("ssh://me@dev:2222/home/me/repo").unwrap();
        assert_eq!(
            (t.destination.as_str(), t.port.as_deref(), t.path.as_str()),
            ("me@dev", Some("2222"), "/home/me/repo")
        );
        assert_eq!(
            Target::parse("ssh://dev/~/src/it's").unwrap().shell_path(),
            r#""$HOME"/'src/it'\''s'"#
        );
        assert_eq!(
            Target::parse("ssh://dev/srv/r").unwrap().shell_path(),
            "'/srv/r'"
        );
        assert!(Target::parse("ssh://dev").is_err());
        assert!(Target::parse("ssh:///x").is_err());
    }

    #[test]
    fn picks_the_daemon_for_the_host() {
        let dir = std::env::temp_dir().join(format!("amonhen-daemons-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("amonhend-linux-x86_64"), b"x").unwrap();
        let daemons = Daemons {
            dir: dir.clone(),
            local: Some(("darwin-aarch64".into(), PathBuf::from("/local"))),
        };
        assert_eq!(
            daemons.for_platform("Linux x86_64\n").unwrap(),
            dir.join("amonhend-linux-x86_64")
        );
        assert_eq!(
            daemons.for_platform("Darwin arm64").unwrap(),
            Path::new("/local")
        );
        assert!(daemons.for_platform("Linux riscv64").is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
