//! SSH 先のエンジン（`amonhen --stdio`）。SSH 先に入っていなければ、手元に同梱したものを送り込んでから起動する
//! （VSCode の Remote-SSH・Zed のリモート開発と同じ形）。
//!
//! 1. SSH の接続は使い回す（ControlMaster）。調べる・送る・起動するを少ないログインで済ませる。
//! 2. SSH 先の OS と CPU（`uname -sm`）、`PATH` の `amonhen` の版、置いてあるエンジンを 1 回で調べる。
//! 3. 使うエンジンを決める: `AMONHEN_REMOTE_ENGINE` で指定したもの → `PATH` の `amonhen`（通信の形の版が合えば）
//!    → 置いてある同じ中身のもの → 送り込む（`AMONHEN_PUSH=0` なら送らずにやめる）。
//! 4. 送るのは `~/.local/share/amonhen/engine/<中身のハッシュ>/amonhen`（手元の置き場所と同じ形）。書いてから
//!    SSH 先で中身のハッシュを確かめ、合わなければ消してやめる。
//! 5. ほかの版は消さない（違う版の Amon Hen が同じ SSH 先を使っても、取り合いにならない）。30 日使われていない
//!    ものだけを片付ける（使うたびに `used` の印を更新する）。

use amonhen_model::tr;
use std::io::Write;
use std::path::PathBuf;
use std::process::Stdio;
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
            .ok_or_else(|| tr!("Not an ssh:// location"))?;
        let slash = rest
            .find('/')
            .ok_or_else(|| tr!("Write it as ssh://host/path"))?;
        let (authority, path) = (&rest[..slash], &rest[slash..]);
        let (destination, port) = match authority.rsplit_once(':') {
            Some((d, p)) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => {
                (d, Some(p.to_owned()))
            }
            _ => (authority, None),
        };
        if destination.is_empty() {
            return Err(tr!("No SSH destination").into());
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

/// 送り込むエンジンの置き場所。手元の `dir` に `amonhen-<os>-<arch>` を置いておく。
pub struct Daemons {
    pub dir: PathBuf,
    /// 手元（macOS）のエンジン。SSH 先も同じ OS と CPU なら、これを送る。
    pub local: Option<(String, PathBuf)>,
    /// SSH 先で使うエンジンの場所（`AMONHEN_REMOTE_ENGINE`）。あれば、調べずにこれを使う。
    pub remote_engine: Option<String>,
    /// 使えるエンジンがなければ送り込む（`AMONHEN_PUSH=0` で止める）。
    pub push: bool,
}

impl Daemons {
    /// `dir`・`local` と、環境変数（`AMONHEN_REMOTE_ENGINE`・`AMONHEN_PUSH`）から。
    pub fn new(dir: PathBuf, local: Option<(String, PathBuf)>) -> Self {
        Self {
            dir,
            local,
            remote_engine: std::env::var("AMONHEN_REMOTE_ENGINE")
                .ok()
                .filter(|s| !s.is_empty()),
            push: std::env::var("AMONHEN_PUSH").map_or(true, |v| v != "0"),
        }
    }
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
        let file = self.dir.join(format!("amonhen-{name}"));
        if file.exists() {
            return Ok(file);
        }
        if let Some((local_name, path)) = &self.local
            && *local_name == name
        {
            return Ok(path.clone());
        }
        Err(tr!(
            "No engine (amonhen) for the SSH host ({platform}) in {dir}",
            platform = uname.trim(),
            dir = self.dir.display()
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

/// SSH の共通の引数（接続先まで）。`share` なら、調べる・送る・起動するで同じ接続を使い回す（ControlMaster）。
/// しないなら、この ssh だけの接続を張る。
fn base_args(target: &Target, share: bool) -> Vec<String> {
    let mut args = vec![
        "-T".to_owned(),
        // パスワードは聞けない（画面がない）。鍵か ssh-agent でつなぐ。
        "-o".into(),
        "BatchMode=yes".into(),
    ];
    if share {
        args.extend([
            "-o".into(),
            "ControlMaster=auto".into(),
            "-o".into(),
            format!("ControlPath={}", control_path()),
            "-o".into(),
            "ControlPersist=60".into(),
        ]);
    } else {
        args.extend(["-o".into(), "ControlPath=none".into()]);
    }
    if let Some(p) = &target.port {
        args.extend(["-p".into(), p.clone()]);
    }
    args.push(target.destination.clone());
    args
}

/// 1 つの SSH の接続の上で、sshd が開けるセッションの数には限りがある（`MaxSessions`、既定は 10）。リポジトリ・
/// worktree ごとのエンジン（`--stdio`）は開いている間ずっとセッションを使うので、接続先ごとにここまでを使い回しの
/// 接続に載せ、超えた分はそれぞれの接続を張る（調べる・送るなどの短いセッションの分を残しておく）。
const SHARED_ENGINES: usize = 5;

/// 接続先ごとの、使い回しの接続に載せているエンジンの数。
static SHARED: std::sync::Mutex<Vec<(String, usize)>> = std::sync::Mutex::new(Vec::new());

/// 使い回しの接続に載せたエンジンの数え（つながりが切れたら減らす）。
struct SharedSlot(String);

impl SharedSlot {
    /// 載せられれば数えて返す。いっぱいなら None。
    fn take(key: &str) -> Option<Self> {
        let mut shared = SHARED.lock().unwrap();
        let count = match shared.iter_mut().find(|(k, _)| k == key) {
            Some((_, count)) => count,
            None => {
                shared.push((key.to_owned(), 0));
                &mut shared.last_mut().unwrap().1
            }
        };
        (*count < SHARED_ENGINES).then(|| {
            *count += 1;
            Self(key.to_owned())
        })
    }
}

impl Drop for SharedSlot {
    fn drop(&mut self) {
        if let Some((_, count)) = SHARED
            .lock()
            .unwrap()
            .iter_mut()
            .find(|(k, _)| *k == self.0)
        {
            *count = count.saturating_sub(1);
        }
    }
}

/// 使い回しの接続が、新しいセッションを断った（`MaxSessions` を超えた）。
fn session_refused(stderr: &str) -> bool {
    stderr.contains("Session open refused") || stderr.contains("mux_client_request_session")
}

/// SSH 先で `script` を実行し、標準出力を返す。`input` は標準入力に流す。使い回しの接続がセッションを断ったら、
/// この ssh だけの接続でやり直す。
fn run(ssh: &str, target: &Target, script: &str, input: Option<&[u8]>) -> Result<String> {
    match run_on(ssh, target, script, input, true) {
        Err(RunError::Refused) => {
            run_on(ssh, target, script, input, false).map_err(RunError::into_error)
        }
        result => result.map_err(RunError::into_error),
    }
}

enum RunError {
    Refused,
    Other(amonhen_model::Error),
}

impl RunError {
    fn into_error(self) -> amonhen_model::Error {
        match self {
            RunError::Other(e) => e,
            RunError::Refused => tr!("The SSH host refused another session").into(),
        }
    }
}

impl<E: Into<amonhen_model::Error>> From<E> for RunError {
    fn from(e: E) -> Self {
        RunError::Other(e.into())
    }
}

fn run_on(
    ssh: &str,
    target: &Target,
    script: &str,
    input: Option<&[u8]>,
    share: bool,
) -> std::result::Result<String, RunError> {
    let mut child = amonhen_model::process::command(ssh)
        .args(base_args(target, share))
        .arg(script)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| tr!("Can’t start {program}: {e}", program = ssh, e = e))?;
    if let Some(data) = input {
        let mut stdin = child.stdin.take().unwrap();
        let data = data.to_vec();
        std::thread::spawn(move || stdin.write_all(&data));
    }
    let out = child.wait_with_output()?;
    if !out.status.success() {
        let message = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        if share && session_refused(&message) {
            return Err(RunError::Refused);
        }
        return Err(tr!(
            "Error on the SSH host ({host}): {message}",
            host = target.destination,
            message = amonhen_model::process::explain_agent(message)
        )
        .into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// SSH 先のエンジンの置き場所（手元の置き場所と同じ形）。
const ENGINE_DIR: &str = "${XDG_DATA_HOME:-$HOME/.local/share}/amonhen/engine";

/// 使わなくなったエンジンを片付けるまでの日数。
const KEEP_DAYS: u32 = 30;

/// SSH 先で使うエンジンと、その用意のしかた（Activity・`amonhen daemon status` に出す）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteEngine {
    /// SSH 先でのパス。
    pub path: String,
    /// `configured`（`AMONHEN_REMOTE_ENGINE`）・`installed`（`PATH` の `amonhen`）・`present`（前に送ったもの）・
    /// `pushed`（今回送った）。
    pub source: &'static str,
    /// 送ったときの大きさ（バイト）。
    pub pushed_bytes: Option<u64>,
}

impl RemoteEngine {
    pub fn describe(&self) -> String {
        let how = match self.source {
            "configured" => tr!("specified"),
            "installed" => tr!("installed on the SSH host"),
            "present" => tr!("sent earlier"),
            _ => tr!("sent now"),
        };
        match self.pushed_bytes {
            Some(n) => tr!(
                "{path} ({how}, {size} MB)",
                path = self.path,
                how = how,
                size = format!("{:.1}", n as f64 / 1e6)
            ),
            None => tr!("{path} ({how})", path = self.path, how = how),
        }
    }
}

/// SSH 先にエンジンを用意して起動し、つなぐ。`ssh` は ssh のコマンド（テストでは偽物を渡す）。
pub fn connect(ssh: &str, target: &Target, daemons: &Daemons) -> Result<Arc<Connection>> {
    let engine = prepare(ssh, target, daemons)?;
    // 使い回しの接続に載せられるのは、接続先ごとに `SHARED_ENGINES` 個まで。
    let key = format!(
        "{}:{}",
        target.destination,
        target.port.as_deref().unwrap_or("")
    );
    let slot = SharedSlot::take(&key);
    let mut args = base_args(target, slot.is_some());
    args.push(format!(
        "{}; exec {} --stdio {}",
        cleanup_script(&engine),
        shell_quote(&engine.path),
        target.shell_path()
    ));
    let conn = Connection::spawn(ssh, &args)?;
    if let Some(slot) = slot {
        conn.hold(Box::new(slot));
    }
    conn.set_remote_engine(engine.describe());
    Ok(conn)
}

/// SSH 先の `target.path` の下のリポジトリを探す（リポジトリの一覧の、SSH 先のスキャンするフォルダ）。エンジンを
/// 用意して `amonhen find-repositories` を実行し、見つけたものを `target.path` からのパスで返す（`/~/prj` の下なら
/// `/~/prj/x` の形）。
pub fn find_repositories(
    ssh: &str,
    target: &Target,
    daemons: &Daemons,
    depth: u32,
    excludes: &[String],
) -> Result<Vec<String>> {
    let engine = prepare(ssh, target, daemons)?;
    let excludes: String = excludes
        .iter()
        .map(|e| format!(" --exclude {}", shell_quote(e)))
        .collect();
    let out = run(
        ssh,
        target,
        &format!(
            "{}; exec {} find-repositories --depth {depth}{excludes} {}",
            cleanup_script(&engine),
            shell_quote(&engine.path),
            target.shell_path()
        ),
        None,
    )?;
    let base = target.path.trim_end_matches('/');
    Ok(out
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            if line == "." {
                base.to_owned()
            } else {
                format!("{base}/{line}")
            }
        })
        .collect())
}

/// SSH 先で使うエンジンを決め、なければ送り込む。
fn prepare(ssh: &str, target: &Target, daemons: &Daemons) -> Result<RemoteEngine> {
    let configured = daemons.remote_engine.clone();
    let push = daemons.push;
    // OS と CPU、置き場所、PATH の amonhen の版、置いてあるエンジン（ハッシュの名前）を 1 回で調べる。
    let probe = run(
        ssh,
        target,
        &format!(
            r#"uname -sm; D="{ENGINE_DIR}"; echo "$D"; a=$(command -v amonhen 2>/dev/null); if [ -n "$a" ]; then echo "$a"; "$a" --version 2>/dev/null | head -1; else echo; echo; fi; ls "$D" 2>/dev/null || true"#
        ),
        None,
    )?;
    let mut lines = probe.lines();
    let uname = lines.next().unwrap_or_default().to_owned();
    let dir = lines.next().unwrap_or_default().to_owned();
    let installed = lines.next().unwrap_or_default().to_owned();
    let installed_version = lines.next().unwrap_or_default().to_owned();
    let present: Vec<&str> = lines.collect();
    let wanted = format!("(build {})", crate::BUILD_ID);

    let engine = if let Some(path) = configured {
        RemoteEngine {
            path,
            source: "configured",
            pushed_bytes: None,
        }
    } else if !installed.is_empty() && installed_version.ends_with(&wanted) {
        RemoteEngine {
            path: installed,
            source: "installed",
            pushed_bytes: None,
        }
    } else {
        let binary = daemons.for_platform(&uname)?;
        let data = std::fs::read(&binary)
            .map_err(|e| tr!("Can’t read {path}: {e}", path = binary.display(), e = e))?;
        if !crate::engines::has_build_mark(&data) {
            return Err(tr!(
                "The engine for the SSH host ({path}) is a different build of Amon Hen. Rebuild it (scripts/build-engine.sh) or install the matching app",
                path = binary.display()
            )
            .into());
        }
        let digest = sha1_smol::Sha1::from(&data).digest().to_string();
        let hash = &digest[..16];
        let path = format!("{dir}/{hash}/amonhen");
        if present.contains(&hash) {
            RemoteEngine {
                path,
                source: "present",
                pushed_bytes: None,
            }
        } else if !push {
            return Err(tr!(
                "No usable engine on the SSH host ({host}). Install amonhen (on PATH) or set its location with AMONHEN_REMOTE_ENGINE (sending it is turned off by AMONHEN_PUSH=0)",
                host = target.destination
            )
            .into());
        } else {
            // 書きかけを使わないよう、一時的な名前で書いてから置き換える。置いたら中身のハッシュを確かめる。
            let d = format!("{}/{hash}", shell_quote(&dir));
            let script = format!(
                r#"mkdir -p {d} && cat > {d}/amonhen.tmp.$$ && chmod 755 {d}/amonhen.tmp.$$ && mv {d}/amonhen.tmp.$$ {d}/amonhen && (sha1sum {d}/amonhen 2>/dev/null || shasum {d}/amonhen) | cut -d' ' -f1"#
            );
            let got = run(ssh, target, &script, Some(&data))?;
            if got.trim() != digest {
                let _ = run(ssh, target, &format!("rm -rf {d}"), None);
                return Err(tr!(
                    "The engine sent to the SSH host ({host}) doesn’t match (sent {sent}, SSH host {got})",
                    host = target.destination,
                    sent = digest,
                    got = got.trim()
                )
                .into());
            }
            RemoteEngine {
                path,
                source: "pushed",
                pushed_bytes: Some(data.len() as u64),
            }
        }
    };
    Ok(engine)
}

/// 使う印を更新し、`KEEP_DAYS` 日使われていないエンジン（と前の置き場所 `~/.cache/amonhen` の古い daemon）を
/// 片付けるシェルの文。動いているエンジンのファイルを消しても、そのプロセスはそのまま動く（Linux・macOS）。
/// 書きかけの一時ファイルも、1 日たったものだけ消す。
fn cleanup_script(engine: &RemoteEngine) -> String {
    let mark = if matches!(engine.source, "present" | "pushed") {
        let dir = engine.path.trim_end_matches("/amonhen");
        format!("touch {}/used 2>/dev/null; ", shell_quote(dir))
    } else {
        String::new()
    };
    format!(
        r#"{mark}D="{ENGINE_DIR}"; for e in "$D"/*; do [ -d "$e" ] || continue; [ -e "$e/used" ] || touch "$e/used"; [ -n "$(find "$e/used" -mtime +{KEEP_DAYS} 2>/dev/null)" ] && rm -rf "$e"; find "$e" -name 'amonhen.tmp.*' -mtime +0 -exec rm -f {{}} + 2>/dev/null; done 2>/dev/null; find "$HOME/.cache/amonhen" -name 'amonhend-*' -mtime +{KEEP_DAYS} -exec rm -f {{}} + 2>/dev/null; true"#
    )
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
        std::fs::write(dir.join("amonhen-linux-x86_64"), b"x").unwrap();
        let daemons = Daemons {
            dir: dir.clone(),
            local: Some(("darwin-aarch64".into(), PathBuf::from("/local"))),
            remote_engine: None,
            push: true,
        };
        assert_eq!(
            daemons.for_platform("Linux x86_64\n").unwrap(),
            dir.join("amonhen-linux-x86_64")
        );
        assert_eq!(
            daemons.for_platform("Darwin arm64").unwrap(),
            Path::new("/local")
        );
        assert!(daemons.for_platform("Linux riscv64").is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
