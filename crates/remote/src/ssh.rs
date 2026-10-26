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
    /// SSH 先のリポジトリのパス。`/~/` で始まればホームからの相対。空なら（`ssh://host` だけ）リポジトリではなく、
    /// その機械の設定（`git config --global` など）を扱うエンジン（`amonhen --stdio` にパスを渡さない）。
    pub path: String,
}

impl Target {
    pub fn parse(location: &str) -> Result<Self> {
        let rest = location
            .strip_prefix("ssh://")
            .ok_or_else(|| tr!("Not an ssh:// location"))?;
        // `/` がなければ、ホストだけ（その機械の設定を扱う）。
        let (authority, path) = match rest.find('/') {
            Some(slash) => (&rest[..slash], &rest[slash..]),
            None => (rest, ""),
        };
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

    /// SSH 先のシェルに渡すパス（`/~/x` は `"$HOME"/'x'`、それ以外は引用符で囲む。空なら空）。
    fn shell_path(&self) -> String {
        if self.path.is_empty() {
            return String::new();
        }
        match self.path.strip_prefix("/~/") {
            Some(rest) => format!("\"$HOME\"/{}", shell_quote(rest)),
            None => shell_quote(&self.path),
        }
    }
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// SSH 先ごとの選択（アプリの Settings > SSH Hosts の「接続」と「プログラム」）。FE がつなぐときに送る
/// （`crate::local::opening`。共有の daemon は、そのつながりでエンジンを起動するときに使う）。環境変数
/// （`AMONHEN_REMOTE_ENGINE`・`AMONHEN_PUSH`）があれば、そちらが優先。
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct HostOptions {
    /// SSH 先で使う git（エンジンに `AMONHEN_GIT` として渡す）。なければ SSH 先の PATH のもの。
    pub git: Option<String>,
    /// SSH 先で動かすエンジン: `installed`（PATH の amonhen。このビルドでなければつながない）、`sent`（Amon Hen が
    /// 置いたもの。なければ送る）、`/` で始まればそのパス。なければ自動（PATH のもの → 置いたもの → 送る）。
    pub engine: Option<String>,
    /// 合うものがなければ送り込むか。なければ送る。
    pub push: Option<bool>,
}

impl HostOptions {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// JSON から。読めなければ何も選んでいないものとする。
    pub fn from_json(text: &str) -> Self {
        serde_json::from_str(text).unwrap_or_default()
    }
}

/// 送り込むエンジンの置き場所。手元の `dir` に `amonhen-<os>-<arch>` を置いておく。
pub struct Daemons {
    pub dir: PathBuf,
    /// 手元（macOS）のエンジン。SSH 先も同じ OS と CPU なら、これを送る。
    pub local: Option<(String, PathBuf)>,
    /// SSH 先で使うエンジンの場所（`AMONHEN_REMOTE_ENGINE`）。あれば、調べずにこれを使う。
    pub remote_engine: Option<String>,
    /// 使えるエンジンがなければ送り込むか（`AMONHEN_PUSH`。`0` で止める）。環境になければ None（SSH 先ごとの
    /// 選択、それもなければ送る）。
    pub push: Option<bool>,
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
            push: std::env::var("AMONHEN_PUSH").ok().map(|v| v != "0"),
        }
    }
}

impl Daemons {
    /// `uname -sm` の結果に合う、このビルドのエンジン（パスと中身）。置き場所の `amonhen-<os>-<arch>` と、SSH 先が
    /// 手元と同じ OS と CPU なら手元のエンジンのうち、このビルドのもの（置き場所のものが古くても、手元のものが
    /// このビルドなら送れる）。
    fn for_platform(&self, uname: &str) -> Result<(PathBuf, Vec<u8>)> {
        let mut words = uname.split_whitespace();
        let os = words.next().unwrap_or_default().to_ascii_lowercase();
        let arch = match words.next().unwrap_or_default() {
            "arm64" | "aarch64" => "aarch64",
            "x86_64" | "amd64" => "x86_64",
            other => other,
        };
        let name = format!("{os}-{arch}");
        let mut candidates = vec![self.dir.join(format!("amonhen-{name}"))];
        if let Some((local_name, path)) = &self.local
            && *local_name == name
        {
            candidates.push(path.clone());
        }
        let mut other_build = None;
        for path in candidates.into_iter().filter(|p| p.is_file()) {
            let data = std::fs::read(&path)
                .map_err(|e| tr!("Can’t read {path}: {e}", path = path.display(), e = e))?;
            if crate::engines::has_build_mark(&data) {
                return Ok((path, data));
            }
            other_build.get_or_insert(path);
        }
        Err(match other_build {
            Some(path) => tr!(
                "The engine for the SSH host ({path}) is a different build of Amon Hen, and the SSH host has none of this build. Rebuild it (scripts/build-engine.sh) or install the matching app",
                path = path.display()
            ),
            None => tr!(
                "No engine (amonhen) for the SSH host ({platform}) in {dir}, and the SSH host has none of this build",
                platform = uname.trim(),
                dir = self.dir.display()
            ),
        }
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

/// エンジンのフォルダに置く、そのエンジンのビルドの指紋（`amonhen install` も置く）。
pub const BUILD_FILE: &str = "build";

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
pub fn connect(
    ssh: &str,
    target: &Target,
    daemons: &Daemons,
    options: &HostOptions,
) -> Result<Arc<Connection>> {
    let engine = prepare(ssh, target, daemons, options)?;
    // 使い回しの接続に載せられるのは、接続先ごとに `SHARED_ENGINES` 個まで。
    let key = format!(
        "{}:{}",
        target.destination,
        target.port.as_deref().unwrap_or("")
    );
    let slot = SharedSlot::take(&key);
    let mut args = base_args(target, slot.is_some());
    args.push(
        format!(
            "{}; {}exec {} --stdio {}",
            cleanup_script(&engine),
            git_env(options),
            shell_quote(&engine.path),
            target.shell_path()
        )
        .trim_end()
        .to_owned(),
    );
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
    options: &HostOptions,
    depth: u32,
    excludes: &[String],
) -> Result<Vec<String>> {
    let engine = prepare(ssh, target, daemons, options)?;
    let excludes: String = excludes
        .iter()
        .map(|e| format!(" --exclude {}", shell_quote(e)))
        .collect();
    let out = run(
        ssh,
        target,
        &format!(
            "{}; {}exec {} find-repositories --depth {depth}{excludes} {}",
            cleanup_script(&engine),
            git_env(options),
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

/// SSH 先の git の選択（`HostOptions::git`）を、エンジンに渡す `AMONHEN_GIT=… ` の形で（なければ空）。
fn git_env(options: &HostOptions) -> String {
    match options.git.as_deref().filter(|g| !g.is_empty()) {
        Some(git) => format!("AMONHEN_GIT={} ", shell_quote(git)),
        None => String::new(),
    }
}

/// SSH 先で使うエンジンを決め、なければ送り込む。
fn prepare(
    ssh: &str,
    target: &Target,
    daemons: &Daemons,
    options: &HostOptions,
) -> Result<RemoteEngine> {
    // 環境変数が先、次に SSH 先ごとの選択。
    let chosen = options.engine.as_deref().filter(|e| !e.is_empty());
    let configured = daemons
        .remote_engine
        .clone()
        .or_else(|| chosen.filter(|e| e.starts_with('/')).map(str::to_owned));
    let (only_installed, skip_installed) = match chosen {
        _ if configured.is_some() => (false, false),
        Some("installed") => (true, false),
        Some("sent") => (false, true),
        _ => (false, false),
    };
    let push = daemons.push.or(options.push).unwrap_or(true);
    // OS と CPU、置き場所、PATH の amonhen の版、置いてあるエンジン（ハッシュの名前と、記録したビルドの指紋）を
    // 1 回で調べる。
    let probe = run(
        ssh,
        target,
        &format!(
            r#"uname -sm; D="{ENGINE_DIR}"; echo "$D"; a=$(command -v amonhen 2>/dev/null); if [ -n "$a" ]; then echo "$a"; "$a" --version 2>/dev/null | head -1; else echo; echo; fi; for e in "$D"/*/; do [ -x "${{e}}amonhen" ] || continue; e=${{e%/}}; echo "${{e##*/}} $(cat "$e/{BUILD_FILE}" 2>/dev/null)"; done; true"#
        ),
        None,
    )?;
    let mut lines = probe.lines();
    let uname = lines.next().unwrap_or_default().to_owned();
    let dir = lines.next().unwrap_or_default().to_owned();
    let installed = lines.next().unwrap_or_default().to_owned();
    let installed_version = lines.next().unwrap_or_default().to_owned();
    // （ハッシュの名前, ビルドの指紋。前の版が置いたものは記録がなく空）
    let present: Vec<(&str, &str)> = lines
        .filter_map(|line| line.split_once(' '))
        .map(|(hash, build)| (hash, build.trim()))
        .collect();
    let wanted = format!("(build {})", crate::BUILD_ID);

    let engine = if let Some(path) = configured {
        RemoteEngine {
            path,
            source: "configured",
            pushed_bytes: None,
        }
    } else if !skip_installed && !installed.is_empty() && installed_version.ends_with(&wanted) {
        RemoteEngine {
            path: installed,
            source: "installed",
            pushed_bytes: None,
        }
    } else if only_installed {
        // PATH のものを使うと決めたのに、ないか、このビルドではない。
        return Err(if installed.is_empty() {
            tr!(
                "No amonhen on the PATH of the SSH host ({host}). Settings > SSH Hosts chooses the one on the PATH; choose another engine there, or install this build on the host",
                host = target.destination
            )
        } else {
            tr!(
                "The amonhen on the PATH of the SSH host ({host}) is another build ({version}). Settings > SSH Hosts chooses the one on the PATH; choose another engine there, or install this build on the host",
                host = target.destination,
                version = installed_version.trim()
            )
        }
        .into());
    } else if let Some((hash, _)) = present.iter().find(|(_, b)| *b == crate::BUILD_ID) {
        // このビルドのものがもう置いてある（ほかの機械・VSCode 拡張が置いた）。手元のエンジンは読まなくてよい。
        RemoteEngine {
            path: format!("{dir}/{hash}/amonhen"),
            source: "present",
            pushed_bytes: None,
        }
    } else {
        let (_, data) = daemons.for_platform(&uname)?;
        let digest = sha1_smol::Sha1::from(&data).digest().to_string();
        let hash = &digest[..16];
        let path = format!("{dir}/{hash}/amonhen");
        if present.iter().any(|(h, _)| *h == hash) {
            RemoteEngine {
                path,
                source: "present",
                pushed_bytes: None,
            }
        } else if !push {
            return Err(tr!(
                "No usable engine on the SSH host ({host}), and sending one is turned off (Settings > SSH Hosts, or AMONHEN_PUSH=0). Install amonhen on the host’s PATH, or set its location with AMONHEN_REMOTE_ENGINE",
                host = target.destination
            )
            .into());
        } else {
            // 書きかけを使わないよう、一時的な名前で書いてから置き換える。置いたら中身のハッシュを確かめる。
            let d = format!("{}/{hash}", shell_quote(&dir));
            // ビルドの指紋も置く（次から、手元のエンジンを読まずにこれを選べる）。
            let script = format!(
                r#"mkdir -p {d} && printf '%s' {build} > {d}/{BUILD_FILE} && cat > {d}/amonhen.tmp.$$ && chmod 755 {d}/amonhen.tmp.$$ && mv {d}/amonhen.tmp.$$ {d}/amonhen && (sha1sum {d}/amonhen 2>/dev/null || shasum {d}/amonhen) | cut -d' ' -f1"#,
                build = crate::BUILD_ID
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
        // 前の版が置いた（指紋の記録がない）このビルドのものにも、指紋を記録する。
        let dir = shell_quote(engine.path.trim_end_matches("/amonhen"));
        format!(
            "touch {dir}/used 2>/dev/null; [ -f {dir}/{BUILD_FILE} ] || printf '%s' {} > {dir}/{BUILD_FILE} 2>/dev/null; ",
            crate::BUILD_ID
        )
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
mod host_options_tests {
    use super::HostOptions;

    #[test]
    fn host_options_round_trip_through_json() {
        let options = HostOptions {
            git: Some("/usr/local/bin/git".into()),
            engine: Some("sent".into()),
            push: Some(false),
        };
        assert_eq!(HostOptions::from_json(&options.to_json()), options);
        assert!(HostOptions::from_json("").is_empty());
        assert!(HostOptions::from_json("{}").is_empty());
        assert_eq!(
            super::git_env(&options),
            "AMONHEN_GIT='/usr/local/bin/git' "
        );
        assert_eq!(super::git_env(&HostOptions::default()), "");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // ホストだけなら、その機械の設定を扱う（パスは空）。
        let host = Target::parse("ssh://me@dev:22").unwrap();
        assert_eq!(
            (
                host.destination.as_str(),
                host.port.as_deref(),
                host.path.as_str()
            ),
            ("me@dev", Some("22"), "")
        );
        assert_eq!(host.shell_path(), "");
        assert!(Target::parse("ssh:///x").is_err());
        assert!(Target::parse("ssh://").is_err());
    }

    #[test]
    fn picks_the_daemon_for_the_host() {
        let dir = std::env::temp_dir().join(format!("amonhen-daemons-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut daemons = Daemons {
            dir: dir.clone(),
            local: Some(("darwin-aarch64".into(), PathBuf::from("/local"))),
            remote_engine: None,
            push: None,
        };
        let this = crate::BUILD_MARK.as_bytes();
        std::fs::write(dir.join("amonhen-linux-x86_64"), this).unwrap();
        // 置き場所の macOS 向けは別のビルドで、手元のエンジンがこのビルド。
        std::fs::write(dir.join("amonhen-darwin-aarch64"), b"amonhen-build=0;").unwrap();
        let local = dir.join("local");
        std::fs::write(&local, this).unwrap();
        daemons.local = Some(("darwin-aarch64".into(), local.clone()));
        assert_eq!(
            daemons.for_platform("Linux x86_64\n").unwrap().0,
            dir.join("amonhen-linux-x86_64")
        );
        assert_eq!(daemons.for_platform("Darwin arm64").unwrap().0, local);
        assert!(daemons.for_platform("Linux riscv64").is_err());
        std::fs::write(dir.join("amonhen-linux-x86_64"), b"amonhen-build=0;").unwrap();
        let err = daemons
            .for_platform("Linux x86_64")
            .unwrap_err()
            .to_string();
        assert!(err.contains("different build"), "{err}");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
