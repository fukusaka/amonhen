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
    // 同じ SSH 先を同時に開いても、聞くのは 1 回（後のものは、前の答えの後でつなぎ直す）。
    static ASKING: Mutex<()> = Mutex::new(());
    let first = match attempt(location, &options, local_signatures) {
        Ok(repo) => return Ok(repo),
        Err(e) => e,
    };
    let (Some(host), Some(_)) = (host_of(location), EngineConsent::find(&first)) else {
        return Err(EngineConsent::strip(&first).to_owned());
    };
    let _asking = ASKING.lock().unwrap_or_else(|e| e.into_inner());
    let err = match attempt(location, &options, local_signatures) {
        Ok(repo) => return Ok(repo),
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
    attempt(location, &options, local_signatures).map_err(|e| EngineConsent::strip(&e).to_owned())
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
