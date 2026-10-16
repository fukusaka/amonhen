//! `amonhend --serve [<socket>]`: 手元に常駐する共有の daemon。
//!
//! 利用者ごとに 1 つ、Unix ドメインソケットで待つ。つないだ相手が最初に送るパス（`amonhen_remote::local`）の
//! リポジトリを開き、同じリポジトリをつないでいる相手のあいだで `GitBackend` と監視を 1 つに共有する（git の
//! オブジェクトのキャッシュも共有される）。リポジトリの変化は、そのリポジトリをつないでいる全員に知らせる。
//! だれもつないでいない状態が `IDLE` 続いたら、ソケットを消して終わる（使うときにまた起動される）。
//!
//! History（読み込みとレイアウト）も、リポジトリごとに 1 つだけ作って永続キャッシュのファイルに書き、つないだ
//! 相手にはそのファイルの場所を返す（相手は mmap して使うので、中身はコピーせずに共有される）。
//!
//! 送られてきた場所が `ssh://host/path` なら、SSH 先に `amonhend --stdio` を送り込んでつなぎ、そのつながりを
//! つないでいる相手のあいだで共有する。コア（History・検索の索引・差分）はこの daemon で動き、SSH 先には git に
//! 触れる処理だけを頼む（アプリ・CLI・VSCode 拡張が、同じ SSH のつながりと索引を使う）。

use std::collections::HashMap;
use std::io::{BufReader, BufWriter};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use amonhen_backend_gix::GixBackend;
use amonhen_model::{GitBackend, Result};
use amonhen_remote::{Notification, Server, local};

/// だれもつないでいない状態がこれだけ続いたら終わる。
const IDLE: Duration = Duration::from_secs(10 * 60);

type Notify = Box<dyn Fn(Notification) + Send + Sync>;

/// 共有しているリポジトリ 1 つ。最後の相手が離れても `IDLE` のあいだは残す（CLI のように 1 回ごとにつないで
/// 切る相手でも、History・検索の索引を作り直さない）。その後、監視も止めて落とす。
struct Shared {
    /// 開いた場所（手元のリポジトリの絶対パス、または `ssh://host/path`）。
    key: String,
    /// SSH 先なら、そのつながり（コアもこれを使う）。
    ssh: Option<Arc<amonhen_remote::Connection>>,
    backend: Arc<dyn GitBackend>,
    /// History を作り、任された命令と差分を実行するコア（初めて頼まれたときに作る）。
    core: std::sync::OnceLock<Arc<amonhen_core::Repository>>,
    /// History を作るのは 1 つずつ。
    history: Mutex<()>,
    /// 検索のための数えたコミットと、件名と作者名の索引（つないでいる全員で 1 つ）。
    search: Arc<amonhen_remote::SearchCache>,
    /// つないでいる相手ごとの知らせ先（番号つき。離れたら外す）。
    listeners: Arc<Mutex<Vec<(u64, Notify)>>>,
    /// 手元のリポジトリの監視（SSH 先なら、SSH 先の daemon の知らせを受けるので None）。
    _watcher: Option<amonhen_watch::Watcher>,
}

impl Shared {
    /// 変化を見張っているか（手元なら監視、SSH 先なら知らせを受けている）。
    fn watching(&self) -> bool {
        self._watcher.is_some() || self.ssh.is_some()
    }

    /// SSH のつながりが切れていれば、その理由。
    fn closed(&self) -> Option<String> {
        self.ssh.as_ref().and_then(|c| c.closed())
    }
}

struct State {
    /// 開いているリポジトリと、最後に使われた時刻。
    repos: Mutex<HashMap<String, (Arc<Shared>, Instant)>>,
    connections: AtomicUsize,
    next: AtomicU64,
    last_left: Mutex<Option<Instant>>,
    socket: PathBuf,
    started: Instant,
}

pub fn run(socket: Option<PathBuf>) -> Result<()> {
    let socket = socket.unwrap_or_else(local::socket_path);
    if let Some(dir) = socket.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // もう動いている daemon があれば、それに任せて終わる。
    if UnixStream::connect(&socket).is_ok() {
        return Ok(());
    }
    let _ = std::fs::remove_file(&socket);
    let listener = match UnixListener::bind(&socket) {
        Ok(l) => l,
        // 同時に起動された別の daemon が先に作った。
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    restrict(&socket);
    let state = Arc::new(State {
        repos: Mutex::default(),
        connections: AtomicUsize::new(0),
        next: AtomicU64::new(0),
        last_left: Mutex::default(),
        socket: socket.clone(),
        started: Instant::now(),
    });
    *state.last_left.lock().unwrap() = Some(Instant::now());
    watch_idle(state.clone(), socket.clone());
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let state = state.clone();
        state.connections.fetch_add(1, Ordering::SeqCst);
        std::thread::spawn(move || {
            if let Err(e) = serve_connection(&state, stream) {
                eprintln!("amonhen: {e}");
            }
            if state.connections.fetch_sub(1, Ordering::SeqCst) == 1 {
                *state.last_left.lock().unwrap() = Some(Instant::now());
            }
        });
    }
    Ok(())
}

/// ソケットは自分だけが読み書きできるようにする。
fn restrict(socket: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600));
}

/// だれもつないでいない状態が続いたら、ソケットを消して終わる。
fn watch_idle(state: Arc<State>, socket: PathBuf) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(5));
            // だれもつないでいない状態が続いたリポジトリを落とす（ここが最後の参照なら、監視も止まる）。
            state.repos.lock().unwrap().retain(|_, (shared, used)| {
                Arc::strong_count(shared) > 1 || used.elapsed() <= IDLE
            });
            let idle = state.connections.load(Ordering::SeqCst) == 0
                && state
                    .last_left
                    .lock()
                    .unwrap()
                    .is_some_and(|t| t.elapsed() > IDLE);
            if idle {
                let _ = std::fs::remove_file(&socket);
                std::process::exit(0);
            }
        }
    });
}

fn serve_connection(state: &State, stream: UnixStream) -> Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let path = match local::read_preamble(&mut reader) {
        Ok(p) => p,
        // 動いているかを確かめるだけでつないで閉じた（別の daemon の起動など）。
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    let mut writer = BufWriter::new(stream);
    // リポジトリではなく、daemon への指示。
    if let Some(command) = path.strip_prefix(local::CONTROL) {
        use amonhen_remote::frame::{self, Kind};
        let (kind, reply) = match command {
            "status" => (Kind::Ok, status(state).to_string()),
            "stop" => (Kind::Ok, "stopping".to_owned()),
            other => (Kind::Err, format!("知らない指示: {other}")),
        };
        frame::write_with(&mut writer, kind, 0, reply.as_bytes(), false)?;
        if command == "stop" {
            let _ = std::fs::remove_file(&state.socket);
            std::process::exit(0);
        }
        return Ok(());
    }
    // FE 向けの窓口（`amonhen-front`）: コアはこの daemon で動き、相手は JSON とレコードを受け取る。
    if let Some(location) = path.strip_prefix(amonhen_front::PREAMBLE) {
        return serve_front(state, location, reader, writer);
    }
    let shared = match open(state, &path) {
        Ok(s) => s,
        Err(e) => {
            // 開けなければ、最初の要求（Hello）に理由を返して閉じる。
            reply_error(&mut reader, writer, &format!("{path} を開けない: {e}"));
            return Ok(());
        }
    };
    let host: Arc<dyn amonhen_remote::Host> = Arc::new(CoreHost(shared.clone()));
    let server = Server::new(shared.backend.clone(), writer)
        .without_compression()
        .with_host(host)
        .with_search_cache(shared.search.clone());
    let id = state.next.fetch_add(1, Ordering::Relaxed);
    shared
        .listeners
        .lock()
        .unwrap()
        .push((id, Box::new(server.notifier())));
    let result = server.run(reader);
    shared.listeners.lock().unwrap().retain(|(i, _)| *i != id);
    result.map_err(Into::into)
}

/// FE 向けの窓口のやりとり。要求ごとにスレッドで実行し、答えは番号つきで返す（順不同）。
fn serve_front(
    state: &State,
    location: &str,
    mut reader: BufReader<UnixStream>,
    writer: BufWriter<UnixStream>,
) -> Result<()> {
    use amonhen_remote::frame::{self, Kind};
    use std::sync::atomic::AtomicBool;
    let opened = open(state, location).and_then(|shared| Ok((core(&shared)?.clone(), shared)));
    let (repo, shared) = match opened {
        Ok(r) => r,
        Err(e) => {
            reply_error(&mut reader, writer, &format!("{location} を開けない: {e}"));
            return Ok(());
        }
    };
    let session = Arc::new(amonhen_front::Session::new(repo));
    let writer = Arc::new(Mutex::new(writer));
    let id = state.next.fetch_add(1, Ordering::Relaxed);
    let notify_writer = writer.clone();
    shared.listeners.lock().unwrap().push((
        id,
        Box::new(move |n| {
            let Notification::Changed(bits) = n;
            let payload = serde_json::json!({ "changes": bits }).to_string();
            let _ = frame::write_with(
                &mut *notify_writer.lock().unwrap(),
                Kind::Notify,
                0,
                payload.as_bytes(),
                false,
            );
        }),
    ));
    let cancels: Arc<Mutex<HashMap<u64, Arc<AtomicBool>>>> = Arc::default();
    let result = (|| -> Result<()> {
        while let Some(f) = frame::read(&mut reader)? {
            match f.kind {
                Kind::Request => {
                    let cancel = Arc::new(AtomicBool::new(false));
                    cancels.lock().unwrap().insert(f.id, cancel.clone());
                    let (session, writer, cancels) =
                        (session.clone(), writer.clone(), cancels.clone());
                    std::thread::spawn(move || {
                        let result = serde_json::from_slice::<serde_json::Value>(&f.payload)
                            .map_err(|e| format!("命令を読めない: {e}").into())
                            .and_then(|request| {
                                session.handle(&request, &|| cancel.load(Ordering::Relaxed))
                            });
                        cancels.lock().unwrap().remove(&f.id);
                        let mut out = writer.lock().unwrap();
                        let _ = match result {
                            Ok(reply) => {
                                frame::write_with(&mut *out, Kind::Ok, f.id, &reply.encode(), false)
                            }
                            Err(e) => frame::write_with(
                                &mut *out,
                                Kind::Err,
                                f.id,
                                e.to_string().as_bytes(),
                                false,
                            ),
                        };
                    });
                }
                Kind::Cancel => {
                    if let Some(c) = cancels.lock().unwrap().get(&f.id) {
                        c.store(true, Ordering::Relaxed);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    })();
    shared.listeners.lock().unwrap().retain(|(i, _)| *i != id);
    result
}

fn reply_error(
    reader: &mut BufReader<UnixStream>,
    mut writer: BufWriter<UnixStream>,
    message: &str,
) {
    use amonhen_remote::frame::{self, Kind};
    if let Ok(Some(f)) = frame::read(reader) {
        let _ = frame::write_with(&mut writer, Kind::Err, f.id, message.as_bytes(), false);
    }
}

/// `location`（手元のパスか `ssh://host/path`）のリポジトリを、だれかがもう開いていればそれを、なければ新しく
/// 開く。SSH のつながりが切れていれば、つなぎ直す。
fn open(state: &State, location: &str) -> Result<Arc<Shared>> {
    let key = if location.starts_with("ssh://") {
        location.to_owned()
    } else {
        std::fs::canonicalize(location)?.display().to_string()
    };
    {
        let mut repos = state.repos.lock().unwrap();
        if let Some((shared, used)) = repos.get_mut(&key) {
            if shared.closed().is_none() {
                *used = Instant::now();
                return Ok(shared.clone());
            }
            repos.remove(&key);
        }
    }
    // SSH はつなぐのに数秒かかるので、一覧のロックの外で開く。
    let listeners: Arc<Mutex<Vec<(u64, Notify)>>> = Arc::default();
    let fan_out = listeners.clone();
    let notify_all = move |changes: u32| {
        for (_, notify) in fan_out.lock().unwrap().iter() {
            notify(Notification::Changed(changes));
        }
    };
    let (backend, ssh, watcher): (Arc<dyn GitBackend>, _, _) = if key.starts_with("ssh://") {
        let conn = connect_ssh(&key)?;
        conn.on_notification(move |n| {
            let Notification::Changed(changes) = n;
            notify_all(changes);
        });
        (
            Arc::new(amonhen_remote::RemoteBackend::connect(conn.clone())?),
            Some(conn),
            None,
        )
    } else {
        let backend: Arc<dyn GitBackend> = Arc::new(GixBackend::open(&key)?);
        let watcher =
            amonhen_watch::Watcher::new(backend.paths(), move |changes| notify_all(changes.0))
                .map_err(|e| eprintln!("amonhen: {key} を監視できない: {e}"))
                .ok();
        (backend, None, watcher)
    };
    let shared = Arc::new(Shared {
        key: key.clone(),
        ssh,
        backend,
        core: std::sync::OnceLock::new(),
        history: Mutex::default(),
        search: Arc::default(),
        listeners,
        _watcher: watcher,
    });
    let mut repos = state.repos.lock().unwrap();
    // 同時に開いた相手がいれば、先に入った方を使う。
    let entry = repos.entry(key).or_insert((shared, Instant::now()));
    entry.1 = Instant::now();
    Ok(entry.0.clone())
}

/// SSH 先のエンジンを用意してつなぐ（アプリが SSH 先を開くときと同じ）。ssh のコマンドは `AMONHEN_SSH` で、
/// 送るエンジンの置き場所は `AMONHEN_ENGINES` で差し替えられる。
fn connect_ssh(location: &str) -> Result<Arc<amonhen_remote::Connection>> {
    use amonhen_remote::ssh;
    let exe = std::env::current_exe()?;
    let daemons = ssh::Daemons::new(daemons_dir(&exe), Some((ssh::local_platform(), exe)));
    let program = std::env::var("AMONHEN_SSH").unwrap_or_else(|_| "ssh".into());
    ssh::connect(&program, &ssh::Target::parse(location)?, &daemons)
}

/// SSH 先に送るエンジン（`amonhen-<os>-<arch>`）の置き場所: `AMONHEN_ENGINES`、`.app` なら
/// `Contents/Resources/engines`、エンジンの置き場所なら隣の `engines`、開発中ならリポジトリの `dist/engine`
/// （`scripts/build-engine.sh` が作る）。
fn daemons_dir(exe: &Path) -> PathBuf {
    if let Some(dir) = std::env::var_os("AMONHEN_ENGINES") {
        return PathBuf::from(dir);
    }
    let resolved = exe.canonicalize().unwrap_or_else(|_| exe.to_path_buf());
    // `.app` の中か、エンジンの置き場所（`amonhen install` が隣に `engines` を置く）。
    let beside = ["../Resources/engines", "engines"].into_iter();
    if let Some(bundled) = beside
        .filter_map(|p| resolved.parent().map(|d| d.join(p)))
        .find(|d| d.is_dir())
    {
        return bundled;
    }
    resolved
        .ancestors()
        .map(|d| d.join("dist/engine"))
        .find(|d| d.is_dir())
        .unwrap_or_else(|| PathBuf::from("dist/engine"))
}

/// daemon のコアで、つないだ側を手伝う。
struct CoreHost(Arc<Shared>);

impl amonhen_remote::Host for CoreHost {
    fn prepared_history(&self) -> Result<String> {
        prepared_history(&self.0)
    }

    fn delegate(&self, request: &[u8]) -> Result<amonhen_model::Delegated> {
        core(&self.0)?.serve_delegated(request)
    }
}

/// リポジトリのコア（初めて使うときに、前回の History のキャッシュから作る）。
fn core(shared: &Shared) -> Result<&Arc<amonhen_core::Repository>> {
    if let Some(repo) = shared.core.get() {
        return Ok(repo);
    }
    let cache = cache_path(&shared.key).ok_or("History のキャッシュの置き場所がない")?;
    let repo = match &shared.ssh {
        // SSH 先は、同じつながりの上でコアを動かす（SSH を 2 本にしない）。
        Some(conn) => amonhen_core::Repository::with_cache(
            amonhen_remote::RemoteBackend::connect(conn.clone())?,
            cache,
        ),
        None => amonhen_core::Repository::with_cache(GixBackend::open(&shared.key)?, cache),
    };
    repo.load_cached_history();
    // 同時に作られたら、先にできた方を使う。
    let _ = shared.core.set(Arc::new(repo));
    Ok(shared.core.get().unwrap())
}

/// History を最新にして、その永続キャッシュのファイルを返す。ファイルが今の History と合わなければ（書けなかった）
/// 断る（相手が古い History を読まないように）。
fn prepared_history(shared: &Shared) -> Result<String> {
    let _one_at_a_time = shared.history.lock().unwrap();
    let repo = core(shared)?;
    let snapshot = repo.load_history()?;
    let path = repo
        .cache_path()
        .ok_or("History のキャッシュの置き場所がない")?;
    let written = amonhen_core::cache::read(path).ok_or("History のキャッシュを書けない")?;
    if written.tips != snapshot.tips || written.head != snapshot.head {
        return Err("History のキャッシュを書けない（古いまま）".into());
    }
    Ok(path.display().to_string())
}

/// daemon の History のキャッシュの置き場所（アプリと同じフォルダ。名前はアプリのものと分ける）。
fn cache_path(key: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let dir = if cfg!(target_os = "macos") {
        home.join("Library/Caches/io.github.fukusaka.amonhen")
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map_or_else(|| home.join(".cache"), PathBuf::from)
            .join("amonhen")
    };
    std::fs::create_dir_all(&dir).ok()?;
    Some(amonhen_core::cache::path_for(
        &dir,
        &format!("daemon-history:{key}"),
    ))
}

/// daemon の様子（`amonhen daemon status`、Activity の Daemon のタブ）。
fn status(state: &State) -> serde_json::Value {
    let repos: Vec<_> = state
        .repos
        .lock()
        .unwrap()
        .iter()
        .map(|(path, (shared, used))| {
            let index = shared.search.text_index_stats();
            let traffic = shared.ssh.as_ref().map(|c| c.traffic());
            serde_json::json!({
                "path": path,
                "ssh": shared.ssh.is_some(),
                "sent": traffic.map(|t| t.0),
                "received": traffic.map(|t| t.1),
                "closed": shared.closed(),
                "engine": shared.ssh.as_ref().and_then(|c| c.remote_engine()),
                "clients": shared.listeners.lock().unwrap().len(),
                "idleMs": used.elapsed().as_millis() as u64,
                "history": shared.core.get().is_some(),
                "watching": shared.watching(),
                "indexRows": index.map(|(rows, _)| rows),
                "indexBytes": index.map(|(_, bytes)| bytes),
            })
        })
        .collect();
    serde_json::json!({
        "pid": std::process::id(),
        "version": env!("CARGO_PKG_VERSION"),
        "protocol": amonhen_remote::PROTOCOL_VERSION,
        "socket": state.socket.display().to_string(),
        "uptimeMs": state.started.elapsed().as_millis() as u64,
        // この問い合わせのつながりは数えない。
        "connections": state.connections.load(Ordering::SeqCst).saturating_sub(1),
        "idleExitMs": IDLE.as_millis() as u64,
        "repos": repos,
    })
}
