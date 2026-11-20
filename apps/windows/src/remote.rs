//! SSH 先・共有の daemon 越しのリポジトリ（GTK4 版の `main.rs` の `open_remote` と `sshhosts.rs` の `connecting`）。
//! `ssh://[user@]host[:port]/path` なら SSH 先でエンジンを起動してつなぎ（合うものがなければ、利用者に聞いてから
//! 送る）、`daemon:/path`・`daemon:ssh://…` なら手元の共有の daemon につなぐ。

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::mpsc::{Sender, channel};

use amonhen_core::Repository;
use amonhen_remote::ssh::{EngineConsent, HostOptions};
use serde_json::{Map, Value};

/// SSH 先ごとの選択（エンジン・送り込み・ログインシェル。GTK4 版と同じ鍵）。
const HOST_OPTIONS: &str = "sshHostOptions";
/// SSH 先ごとの、署名を手元の鍵で検証するか（SSH 先 → bool の JSON。GTK4 版・macOS 版と同じ鍵）。
const SIGNATURE_BY_HOST: &str = "checkRemoteSignaturesLocallyByHost";

/// つなぐ必要のある場所か（`ssh://`・`daemon:`）。
pub fn is_remote(location: &str) -> bool {
    amonhen_remote::location::is_remote(location)
}

/// 場所の SSH 先（`ssh://host/path`・`daemon:ssh://host/path` の host。`user@`・`:port` も含めて ssh に渡す形）。
/// 手元なら None。
pub fn host_of(location: &str) -> Option<String> {
    let rest = location.strip_prefix("daemon:").unwrap_or(location);
    let rest = rest.strip_prefix("ssh://")?;
    let host = rest.split('/').next().unwrap_or_default();
    (!host.is_empty()).then(|| host.to_owned())
}

/// SSH の場所を組み立てる（GTK4 版の `filepicker::ssh_location`）。`host` は `[user@]host[:port]`、`path` は向こうの
/// 絶対パス（Windows の `C:\…` は `/C:/…` にする）か `~/` から始まるパス（どちらでもなければホームからの相対と
/// みなす）。終わりの `/` は落とす。書き方がおかしければ None。
pub fn ssh_location(host: &str, path: &str) -> Option<String> {
    let host = host.trim();
    let path = path.trim();
    if host.is_empty() || path.is_empty() || host.contains(|c: char| c == '/' || c.is_whitespace())
    {
        return None;
    }
    let mut path = if path == "~" || path.starts_with("~/") {
        format!("/{path}")
    } else if is_windows(path) {
        format!("/{}", path.replace('\\', "/"))
    } else if !path.starts_with('/') {
        format!("/~/{path}")
    } else {
        path.to_owned()
    };
    // ドライブの根（`/C:/`）の `/` は残す（`C:` だけはそのドライブの今のフォルダ）。
    while path.len() > 1 && path.ends_with('/') && !(path.len() == 4 && is_windows(&path[1..])) {
        path.pop();
    }
    Some(format!("ssh://{host}{path}"))
}

fn is_windows(path: &str) -> bool {
    let b = path.as_bytes();
    path.starts_with("\\\\") || (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':')
}

fn all_options() -> Map<String, Value> {
    crate::prefs::string(HOST_OPTIONS)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// `location` の SSH 先ごとの選択（手元・選んでいなければ既定）。UI のスレッドで読む。
pub fn options_for(location: &str) -> HostOptions {
    host_of(location).map_or_else(HostOptions::default, |host| host_options(&host))
}

/// SSH 先ごとの選択（選んでいなければ既定）。UI のスレッドで。
pub fn host_options(host: &str) -> HostOptions {
    all_options()
        .get(host)
        .and_then(Value::as_str)
        .map(HostOptions::from_json)
        .unwrap_or_default()
}

/// SSH 先ごとの選択を変える（次につなぐときから効く。UI のスレッドで）。
pub fn set_host_options(host: &str, options: &HostOptions) {
    let mut all = all_options();
    if options.is_empty() {
        all.remove(host);
    } else {
        all.insert(host.to_owned(), Value::String(options.to_json()));
    }
    if let Err(e) = crate::prefs::set_string(HOST_OPTIONS, &Value::Object(all).to_string()) {
        eprintln!("SSH 先の選択を書けない: {e}");
    }
}

/// SSH 先にエンジンを送り込むか（送ってよいかを聞いたときの答え。UI のスレッドで）。
fn set_push(host: &str, push: bool) {
    let mut options = host_options(host);
    options.push = Some(push);
    set_host_options(host, &options);
}

fn signature_hosts() -> Map<String, Value> {
    crate::prefs::string(SIGNATURE_BY_HOST)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// `location`（SSH 先のリポジトリ）の署名を手元の鍵で検証するか。SSH 先ごとの値、なければ手元（GTK4 版の既定と
/// 同じ。SSH でない daemon 越しも手元）。UI のスレッドで読む。
pub fn checks_signatures_locally(location: &str) -> bool {
    host_of(location)
        .and_then(|host| signature_hosts().get(&host).and_then(Value::as_bool))
        .unwrap_or(true)
}

/// SSH 先の署名を検証する鍵を選ぶ（次につなぐときから効く。UI のスレッドで）。
pub fn set_checks_locally(host: &str, local: bool) {
    let mut all = signature_hosts();
    all.insert(host.to_owned(), Value::Bool(local));
    if let Err(e) = crate::prefs::set_string(SIGNATURE_BY_HOST, &Value::Object(all).to_string()) {
        eprintln!("署名の検証の選択を書けない: {e}");
    }
}

/// エンジンを送ってよいかの問い（裏のスレッドが置き、UI のスレッドが聞いて答える）。
#[derive(Clone)]
pub struct Ask {
    pub host: String,
    pub consent: EngineConsent,
    reply: Sender<bool>,
}

impl Ask {
    /// 答える。送らないなら、その SSH 先の送り込みを切る（Settings > SSH Hosts で戻す。GTK4 版と同じ）。今後も
    /// 聞かないなら、聞かずに送る。
    pub fn answer(self, send: bool, always: bool) {
        if !send {
            set_push(&self.host, false);
        } else if always {
            set_push(&self.host, true);
        }
        let _ = self.reply.send(send);
    }
}

static ASKS: Mutex<Vec<Ask>> = Mutex::new(Vec::new());

/// 答えを待っている問い（UI のスレッドが見る）。
pub fn take_ask() -> Option<Ask> {
    let mut asks = ASKS.lock().unwrap_or_else(|e| e.into_inner());
    (!asks.is_empty()).then(|| asks.remove(0))
}

/// つなぐ（裏のスレッドから）。`options` は UI のスレッドで読んだ SSH 先ごとの選択、`local_signatures` は署名を
/// 手元の鍵で検証するか（[`checks_signatures_locally`]）、`wake` は問いを置いたことを UI のスレッドに知らせる。エンジンを送る承認が要れば、
/// UI のスレッドに聞いて待ち、送るならその中身への承認を添えてつなぎ直す（GTK4 版の `connecting`）。
pub fn open(
    location: &str,
    options: HostOptions,
    local_signatures: bool,
    wake: &dyn Fn(),
) -> Result<Repository, String> {
    with_consent(location, options, wake, |options| {
        attempt(location, options, local_signatures)
    })
}

/// `attempt` を SSH 先ごとの選択で試し、エンジンを送る承認が要れば UI のスレッドに聞いて待ち、送るならその中身への
/// 承認を添えて試し直す（GTK4 版の `connecting`）。
fn with_consent<T>(
    location: &str,
    options: HostOptions,
    wake: &dyn Fn(),
    attempt: impl Fn(&HostOptions) -> Result<T, String>,
) -> Result<T, String> {
    // 同じ SSH 先を同時に開いても、聞くのは 1 回（後のものは、前の答えの後でつなぎ直す）。
    static ASKING: Mutex<()> = Mutex::new(());
    let first = match attempt(&options) {
        Ok(v) => return Ok(v),
        Err(e) => e,
    };
    let (Some(host), Some(_)) = (host_of(location), EngineConsent::find(&first)) else {
        return Err(EngineConsent::strip(&first).to_owned());
    };
    let _asking = ASKING.lock().unwrap_or_else(|e| e.into_inner());
    let err = match attempt(&options) {
        Ok(v) => return Ok(v),
        Err(e) => e,
    };
    let Some(consent) = EngineConsent::find(&err) else {
        return Err(err);
    };
    let (tx, rx) = channel();
    ASKS.lock().unwrap_or_else(|e| e.into_inner()).push(Ask {
        host,
        consent: consent.clone(),
        reply: tx,
    });
    wake();
    // 窓を閉じた・タブを替えた（問いを捨てた）なら送らない。
    if !rx.recv().unwrap_or(false) {
        return Err(EngineConsent::strip(&err).to_owned());
    }
    let options = HostOptions {
        send_engine: Some(consent.hash),
        ..options
    };
    attempt(&options).map_err(|e| EngineConsent::strip(&e).to_owned())
}

/// SSH 先のフォルダ（`ssh://host/path`）の下のリポジトリを、SSH 先のエンジンで探す（裏のスレッドから。GTK4 版の
/// `bookmarks::scan_root`）。見つけたものは同じ形の場所（`ssh://host/…`）で返す。
pub fn find_repositories(
    location: &str,
    options: HostOptions,
    depth: u32,
    excludes: &[String],
) -> Result<Vec<String>, String> {
    use amonhen_remote::ssh;
    let target = ssh::Target::parse(location).map_err(|e| e.to_string())?;
    let program = std::env::var("AMONHEN_SSH").unwrap_or_else(|_| "ssh".into());
    let daemons = ssh::Daemons::new(engines_dir(), Some((ssh::local_platform(), engine())));
    let found = with_consent(location, options, &|| {}, |options| {
        ssh::find_repositories(&program, &target, &daemons, options, depth, excludes)
            .map_err(|e| e.to_string())
    })?;
    let prefix = &location[..location.len() - target.path.len()];
    Ok(found.iter().map(|p| format!("{prefix}{p}")).collect())
}

fn attempt(
    location: &str,
    options: &HostOptions,
    local_signatures: bool,
) -> Result<Repository, String> {
    use amonhen_remote::ssh;
    let daemon = engine();
    let conn = if location.starts_with("ssh://") {
        // SSH 先が手元と同じ OS と CPU なら、手元のエンジンを送る。
        let daemons =
            ssh::Daemons::new(engines_dir(), Some((ssh::local_platform(), daemon.clone())));
        let program = std::env::var("AMONHEN_SSH").unwrap_or_else(|_| "ssh".into());
        let target = ssh::Target::parse(location).map_err(|e| e.to_string())?;
        ssh::connect(&program, &target, &daemons, options).map_err(|e| e.to_string())?
    } else {
        // `daemon:ssh://…` なら、共有の daemon がエンジンを起動するときに SSH 先ごとの選択を使う。
        let path = amonhen_remote::location::local_path(location)?;
        amonhen_remote::local::connect_with(path, &daemon.to_string_lossy(), options)
            .map_err(|e| e.to_string())?
    };
    // 共有の daemon が開けなかった理由（承認が要る、も）は、最初の要求の答えで来る。
    let backend = amonhen_remote::RemoteBackend::connect(conn).map_err(|e| e.to_string())?;
    // SSH 先の History は手元にキャッシュする（つなぎ直したら新しいコミットだけを受け取る）。手元の共有の daemon
    // なら、daemon が用意したファイルを直接読むので持たない。
    let repo = match amonhen_remote::local::cache_dir() {
        Some(dir) if location.starts_with("ssh://") => {
            Repository::with_cache(backend, amonhen_core::cache::path_for(&dir, location))
        }
        _ => Repository::new(backend),
    };
    apply_signature_check(&repo, local_signatures);
    Ok(repo)
}

/// 署名を検証する側を決める。手元の鍵なら、コミットのオブジェクトだけを書く作業用のリポジトリで検証する（GTK4
/// 版の既定と同じ）。SSH 先の鍵なら、SSH 先の git が検証する。
fn apply_signature_check(repo: &Repository, local: bool) {
    static VERIFIER: std::sync::OnceLock<std::sync::Arc<amonhen_backend_cli::LocalVerifier>> =
        std::sync::OnceLock::new();
    if repo.connection_state().is_none() {
        return;
    }
    if !local {
        repo.set_signature_verifier(None);
        return;
    }
    let verifier = VERIFIER
        .get_or_init(|| {
            std::sync::Arc::new(amonhen_backend_cli::LocalVerifier::new(
                std::env::temp_dir().join("amonhen-verify.git"),
            ))
        })
        .clone();
    repo.set_signature_verifier(Some(
        verifier as std::sync::Arc<dyn amonhen_model::SignatureVerifier>,
    ));
}

/// SSH で `host` に入れるかを確かめる（GTK4 版の `check_ssh_host`。裏のスレッドから）。入れたら git の版と、
/// エンジンが入っているかを、文と、入れたか（git があるか）で返す。Unix の SSH 先で動かなければ、Windows の
/// SSH 先として PowerShell で試す。
pub fn check_host(host: &str) -> (String, bool) {
    use amonhen_model::tr;
    let program = std::env::var("AMONHEN_SSH").unwrap_or_else(|_| "ssh".into());
    let run = |command: &str| {
        amonhen_model::process::command(&program)
            .args([
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=10",
                host,
                command,
            ])
            .stdin(std::process::Stdio::null())
            .output()
            .ok()
    };
    let Some(mut out) =
        run("git --version 2>/dev/null; command -v amonhen >/dev/null 2>&1 && echo engine; true")
    else {
        return (tr!("Couldn’t run ssh"), false);
    };
    // ssh そのものの失敗は 255。
    let code = out.status.code().unwrap_or(-1);
    if code != 0 && code != 255 {
        let script = "git --version; if (Get-Command amonhen -CommandType Application -ErrorAction SilentlyContinue) { 'engine' }";
        // `-EncodedCommand` は UTF-16LE の base64（cmd.exe・pwsh・Windows PowerShell のどれでも同じ意味に）。
        let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
        if let Some(windows) = run(&format!(
            "powershell -NoProfile -NonInteractive -EncodedCommand {}",
            base64(&bytes)
        )) && windows.status.success()
        {
            out = windows;
        }
    }
    if !out.status.success() {
        let message = ssh_message(&out.stderr);
        return (
            if message.is_empty() {
                tr!(
                    "Couldn’t connect (exit {code})",
                    code = out.status.code().unwrap_or(-1)
                )
            } else {
                message
            },
            false,
        );
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let git = text
        .lines()
        .find_map(|l| l.trim().strip_prefix("git version ").map(str::to_owned));
    let engine = text.lines().any(|l| l.trim() == "engine");
    let mut shown = match &git {
        Some(v) => tr!("Reachable: git {version}", version = v),
        None => tr!("Reachable, but git wasn’t found on the host"),
    };
    shown += &if engine {
        tr!(", engine found")
    } else {
        tr!(", engine not installed (Amon Hen sends one)")
    };
    (shown, git.is_some())
}

/// Windows の ssh の出したメッセージ。OpenSSH は ASCII でない字を `\202\273` のような 8 進の形で書くので、バイトに
/// 戻してから、この機械の文字コード（日本語なら Shift_JIS）で読む。
fn ssh_message(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let bytes = unescape_octal(text.trim());
    if bytes.is_ascii() {
        return String::from_utf8_lossy(&bytes).into_owned();
    }
    if let Ok(text) = std::str::from_utf8(&bytes) {
        return text.to_owned();
    }
    ansi_to_string(&bytes).unwrap_or_else(|| String::from_utf8_lossy(&bytes).into_owned())
}

/// `\ooo`（3 桁の 8 進）をそのバイトに戻す。ほかの字はそのまま。
fn unescape_octal(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && i + 3 < bytes.len()
            && bytes[i + 1..i + 4]
                .iter()
                .all(|b| (b'0'..=b'7').contains(b))
        {
            let n = bytes[i + 1..i + 4]
                .iter()
                .fold(0u32, |n, b| n * 8 + u32::from(b - b'0'));
            if let Ok(b) = u8::try_from(n) {
                out.push(b);
                i += 4;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// この機械の ANSI の文字コードのバイトを読む。
fn ansi_to_string(bytes: &[u8]) -> Option<String> {
    use windows::Win32::Globalization::{
        CP_ACP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS, MultiByteToWideChar,
    };
    let flags = MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0);
    let n = unsafe { MultiByteToWideChar(CP_ACP, flags, bytes, None) };
    if n <= 0 {
        return None;
    }
    let mut wide = vec![0u16; n as usize];
    let n = unsafe { MultiByteToWideChar(CP_ACP, flags, bytes, Some(&mut wide)) };
    (n > 0).then(|| String::from_utf16_lossy(&wide[..n as usize]))
}

/// base64（標準の文字、`=` で埋める）。
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// SSH 先に送るエンジン（`amonhen-<os>-<arch>`）の置き場所: `AMONHEN_ENGINES`、なければ実行ファイルの隣の
/// `engines`、開発中はチェックアウトの `dist/engine`（GTK4 版と同じ）。
fn engines_dir() -> PathBuf {
    std::env::current_exe()
        .map(|exe| amonhen_remote::engines::bundled_dir(&exe))
        .unwrap_or_default()
}

/// 共有の daemon として起動し、SSH 先が同じ OS と CPU なら送るエンジン（GTK4 版の `engine`）:
/// `AMONHEN_DAEMON`、一緒に配ったエンジン（`engines\amonhen-windows-<arch>.exe`）、開発中はチェックアウトの
/// `target\release\amonhen.exe`、置き場所（`%LOCALAPPDATA%\amonhen`）の `current`、`bin\amonhen.exe`、PATH の
/// `amonhen` の順。
pub fn engine() -> PathBuf {
    if let Some(path) = std::env::var_os("AMONHEN_DAEMON") {
        return PathBuf::from(path);
    }
    let exe = std::env::current_exe().ok();
    let name = format!(
        "amonhen-{}{}",
        amonhen_remote::ssh::local_platform(),
        std::env::consts::EXE_SUFFIX
    );
    if let Some(bundled) = exe
        .as_ref()
        .and_then(|e| e.parent().map(|d| d.join("engines").join(&name)))
        .filter(|e| e.is_file())
    {
        return bundled;
    }
    if let Some(engine) = exe
        .as_ref()
        .and_then(|e| amonhen_remote::engines::development_engine(e))
    {
        return engine;
    }
    if let Some(data) = std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("amonhen")) {
        // `current` はエンジンの場所（`engine\<ハッシュ>`）を書いたファイル。
        if let Some(current) = std::fs::read_to_string(data.join("current"))
            .ok()
            .map(|c| data.join(c.trim()).join("amonhen.exe"))
            .filter(|c| c.is_file())
        {
            return current;
        }
        let bin = data.join("bin").join("amonhen.exe");
        if bin.is_file() {
            return bin;
        }
    }
    PathBuf::from("amonhen")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_octal_escapes_back_to_bytes() {
        assert_eq!(unescape_octal(r"a\202\273b"), [b'a', 0o202, 0o273, b'b']);
        assert_eq!(unescape_octal(r"no \x escape"), br"no \x escape");
        assert_eq!(ssh_message(b"ssh: plain\n"), "ssh: plain");
    }

    #[test]
    fn encodes_base64() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn locations_on_ssh_hosts() {
        assert_eq!(
            host_of("ssh://me@box:22/srv/repo").as_deref(),
            Some("me@box:22")
        );
        assert_eq!(host_of("daemon:ssh://box/srv/repo").as_deref(), Some("box"));
        assert_eq!(host_of(r"C:\src\repo"), None);
        assert_eq!(
            ssh_location("box", "~/prj/a").as_deref(),
            Some("ssh://box/~/prj/a")
        );
        assert_eq!(ssh_location("box", "~").as_deref(), Some("ssh://box/~"));
        assert_eq!(
            ssh_location("box", "/srv/a/").as_deref(),
            Some("ssh://box/srv/a")
        );
        assert_eq!(
            ssh_location("box", "prj/a").as_deref(),
            Some("ssh://box/~/prj/a")
        );
        assert_eq!(
            ssh_location("box", r"C:\src\a").as_deref(),
            Some("ssh://box/C:/src/a")
        );
        assert_eq!(ssh_location("my box", "/a"), None);
        assert_eq!(ssh_location("box", " "), None);
    }
}
