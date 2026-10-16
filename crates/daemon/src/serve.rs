//! `amonhend --serve [<socket>]`: 手元に常駐する共有の daemon。
//!
//! 利用者ごとに 1 つ、Unix ドメインソケットで待つ。つないだ相手が最初に送るパス（`amonhen_remote::local`）の
//! リポジトリを開き、同じリポジトリをつないでいる相手のあいだで `GitBackend` と監視を 1 つに共有する（git の
//! オブジェクトのキャッシュも共有される）。リポジトリの変化は、そのリポジトリをつないでいる全員に知らせる。
//! だれもつないでいない状態が `IDLE` 続いたら、ソケットを消して終わる（使うときにまた起動される）。

use std::collections::HashMap;
use std::io::{BufReader, BufWriter};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use amonhen_backend_gix::GixBackend;
use amonhen_model::{GitBackend, Result};
use amonhen_remote::{Notification, Server, local};

/// だれもつないでいない状態がこれだけ続いたら終わる。
const IDLE: Duration = Duration::from_secs(10 * 60);

type Notify = Box<dyn Fn(Notification) + Send + Sync>;

/// 共有しているリポジトリ 1 つ。最後の相手が離れたら落ちる（監視も止まる）。
struct Shared {
    backend: Arc<dyn GitBackend>,
    /// つないでいる相手ごとの知らせ先（番号つき。離れたら外す）。
    listeners: Arc<Mutex<Vec<(u64, Notify)>>>,
    _watcher: Option<amonhen_watch::Watcher>,
}

#[derive(Default)]
struct State {
    repos: Mutex<HashMap<PathBuf, Weak<Shared>>>,
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
    let server = Server::new(shared.backend.clone(), writer).without_compression();
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
    if let Some(shared) = repos.get(&key).and_then(Weak::upgrade) {
        return Ok(shared);
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
    let shared = Arc::new(Shared { backend, listeners, _watcher: watcher });
    repos.retain(|_, w| w.strong_count() > 0);
    repos.insert(key, Arc::downgrade(&shared));
    Ok(shared)
}
