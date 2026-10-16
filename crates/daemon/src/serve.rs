//! `amonhend --serve [<socket>]`: 手元に常駐する共有の daemon。
//!
//! 利用者ごとに 1 つ、Unix ドメインソケットで待つ。つないだ相手が最初に送るパス（`amonhen_remote::local`）の
//! リポジトリを開き、同じリポジトリをつないでいる相手のあいだで `GitBackend` と監視を 1 つに共有する（git の
//! オブジェクトのキャッシュも共有される）。リポジトリの変化は、そのリポジトリをつないでいる全員に知らせる。
//! だれもつないでいない状態が `IDLE` 続いたら、ソケットを消して終わる（使うときにまた起動される）。
//!
//! History（読み込みとレイアウト）も、リポジトリごとに 1 つだけ作って永続キャッシュのファイルに書き、つないだ
//! 相手にはそのファイルの場所を返す（相手は mmap して使うので、中身はコピーせずに共有される）。

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
    key: PathBuf,
    backend: Arc<dyn GitBackend>,
    /// History を作り、任された命令と差分を実行するコア（初めて頼まれたときに作る）。
    core: std::sync::OnceLock<amonhen_core::Repository>,
    /// History を作るのは 1 つずつ。
    history: Mutex<()>,
    /// 検索のための数えたコミットと、件名と作者名の索引（つないでいる全員で 1 つ）。
    search: Arc<amonhen_remote::SearchCache>,
    /// つないでいる相手ごとの知らせ先（番号つき。離れたら外す）。
    listeners: Arc<Mutex<Vec<(u64, Notify)>>>,
    _watcher: Option<amonhen_watch::Watcher>,
}

#[derive(Default)]
struct State {
    /// 開いているリポジトリと、最後に使われた時刻。
    repos: Mutex<HashMap<PathBuf, (Arc<Shared>, Instant)>>,
    connections: AtomicUsize,
    next: AtomicU64,
    last_left: Mutex<Option<Instant>>,
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
    let state = Arc::new(State::default());
    *state.last_left.lock().unwrap() = Some(Instant::now());
    watch_idle(state.clone(), socket.clone());
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let state = state.clone();
        state.connections.fetch_add(1, Ordering::SeqCst);
        std::thread::spawn(move || {
            if let Err(e) = serve_connection(&state, stream) {
                eprintln!("amonhend: {e}");
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
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(5));
        // だれもつないでいない状態が続いたリポジトリを落とす（ここが最後の参照なら、監視も止まる）。
        state.repos.lock().unwrap().retain(|_, (shared, used)| {
            Arc::strong_count(shared) > 1 || used.elapsed() <= IDLE
        });
        let idle = state.connections.load(Ordering::SeqCst) == 0
            && state.last_left.lock().unwrap().is_some_and(|t| t.elapsed() > IDLE);
        if idle {
            let _ = std::fs::remove_file(&socket);
            std::process::exit(0);
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
    let writer = BufWriter::new(stream);
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
    shared.listeners.lock().unwrap().push((id, Box::new(server.notifier())));
    let result = server.run(reader);
    shared.listeners.lock().unwrap().retain(|(i, _)| *i != id);
    result.map_err(Into::into)
}

fn reply_error(reader: &mut BufReader<UnixStream>, mut writer: BufWriter<UnixStream>, message: &str) {
    use amonhen_remote::frame::{self, Kind};
    if let Ok(Some(f)) = frame::read(reader) {
        let _ = frame::write_with(&mut writer, Kind::Err, f.id, message.as_bytes(), false);
    }
}

/// `path` のリポジトリを、だれかがもう開いていればそれを、なければ新しく開く。
fn open(state: &State, path: &str) -> Result<Arc<Shared>> {
    let key = std::fs::canonicalize(path)?;
    let mut repos = state.repos.lock().unwrap();
    if let Some((shared, used)) = repos.get_mut(&key) {
        *used = Instant::now();
        return Ok(shared.clone());
    }
    let backend: Arc<dyn GitBackend> = Arc::new(GixBackend::open(&key)?);
    let listeners: Arc<Mutex<Vec<(u64, Notify)>>> = Arc::default();
    let fan_out = listeners.clone();
    let watcher = amonhen_watch::Watcher::new(backend.paths(), move |changes| {
        for (_, notify) in fan_out.lock().unwrap().iter() {
            notify(Notification::Changed(changes.0));
        }
    })
    .map_err(|e| eprintln!("amonhend: {} を監視できない: {e}", key.display()))
    .ok();
    let shared = Arc::new(Shared {
        key: key.clone(),
        backend,
        core: std::sync::OnceLock::new(),
        history: Mutex::default(),
        search: Arc::default(),
        listeners,
        _watcher: watcher,
    });
    repos.insert(key, (shared.clone(), Instant::now()));
    Ok(shared)
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
fn core(shared: &Shared) -> Result<&amonhen_core::Repository> {
    if let Some(repo) = shared.core.get() {
        return Ok(repo);
    }
    let cache = cache_path(&shared.key).ok_or("History のキャッシュの置き場所がない")?;
    let repo = amonhen_core::Repository::with_cache(GixBackend::open(&shared.key)?, cache);
    repo.load_cached_history();
    // 同時に作られたら、先にできた方を使う。
    let _ = shared.core.set(repo);
    Ok(shared.core.get().unwrap())
}

/// History を最新にして、その永続キャッシュのファイルを返す。ファイルが今の History と合わなければ（書けなかった）
/// 断る（相手が古い History を読まないように）。
fn prepared_history(shared: &Shared) -> Result<String> {
    let _one_at_a_time = shared.history.lock().unwrap();
    let repo = core(shared)?;
    let snapshot = repo.load_history()?;
    let path = repo.cache_path().ok_or("History のキャッシュの置き場所がない")?;
    let written = amonhen_core::cache::read(path).ok_or("History のキャッシュを書けない")?;
    if written.tips != snapshot.tips || written.head != snapshot.head {
        return Err("History のキャッシュを書けない（古いまま）".into());
    }
    Ok(path.display().to_string())
}

/// daemon の History のキャッシュの置き場所（アプリと同じフォルダ。名前はアプリのものと分ける）。
fn cache_path(key: &Path) -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let dir = if cfg!(target_os = "macos") {
        home.join("Library/Caches/io.github.fukusaka.amonhen")
    } else {
        std::env::var_os("XDG_CACHE_HOME").map_or_else(|| home.join(".cache"), PathBuf::from).join("amonhen")
    };
    std::fs::create_dir_all(&dir).ok()?;
    Some(amonhen_core::cache::path_for(&dir, &format!("daemon-history:{}", key.display())))
}
