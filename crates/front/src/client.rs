//! FE の側: 同じプロセスのコア（`InProcess`）か、共有の daemon のコア（`Remote`）に命令を送る。

use amonhen_model::tr;
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use amonhen_remote::frame::{self, Kind};
use serde_json::Value;

use crate::{Reply, Result, Session};

/// コアへの窓口。命令は `{"op": "...", ...}`（`docs/front.md`）。
pub trait Front: Send + Sync {
    /// `cancelled` が true になったら取り消す（検索など）。
    fn call_until(&self, request: Value, cancelled: &(dyn Fn() -> bool + Sync)) -> Result<Reply>;

    fn call(&self, request: Value) -> Result<Reply> {
        self.call_until(request, &|| false)
    }

    /// リポジトリの変化（`amonhen_watch::Changes` のビット）を知らせてもらう。
    fn subscribe(&self, f: Box<dyn Fn(u32) + Send + Sync>) -> Result<()>;

    /// daemon とのつながりが切れていれば、その理由。
    fn closed(&self) -> Option<String> {
        None
    }
}

/// `location` を開く。`ssh://host/path` と `daemon:<path | ssh://…>` は共有の daemon（`daemon` はその場所。
/// いなければ起動する）のコアで、ほかは同じプロセスのコアで（History の永続キャッシュは `cache_dir` に置く）。
pub fn open(location: &str, daemon: &str, cache_dir: Option<&Path>) -> Result<Box<dyn Front>> {
    if location.starts_with("ssh://") {
        return Ok(Box::new(Remote::connect(location, daemon)?));
    }
    if let Some(rest) = location.strip_prefix("daemon:") {
        return Ok(Box::new(Remote::connect(rest, daemon)?));
    }
    Ok(Box::new(InProcess::open(Path::new(location), cache_dir)?))
}

/// 同じプロセスのコア。
pub struct InProcess {
    session: Session,
    watcher: Mutex<Option<amonhen_watch::Watcher>>,
}

impl InProcess {
    pub fn new(repo: amonhen_core::Repository) -> Self {
        Self {
            session: Session::new(Arc::new(repo)),
            watcher: Mutex::default(),
        }
    }

    /// 作業ツリーの一番上 `root` のリポジトリを開く。
    pub fn open(root: &Path, cache_dir: Option<&Path>) -> Result<Self> {
        let backend = amonhen_backend_gix::GixBackend::open(root)?;
        let key = std::fs::canonicalize(root)
            .unwrap_or_else(|_| root.to_owned())
            .display()
            .to_string();
        Ok(Self::new(match cache_dir {
            Some(dir) => amonhen_core::Repository::with_cache(
                backend,
                amonhen_core::cache::path_for(dir, &key),
            ),
            None => amonhen_core::Repository::new(backend),
        }))
    }
}

impl Front for InProcess {
    fn call_until(&self, request: Value, cancelled: &(dyn Fn() -> bool + Sync)) -> Result<Reply> {
        self.session.handle(&request, cancelled)
    }

    fn subscribe(&self, f: Box<dyn Fn(u32) + Send + Sync>) -> Result<()> {
        let w = amonhen_watch::Watcher::new(self.session.repository().paths(), move |c| f(c.0))?;
        *self.watcher.lock().unwrap() = Some(w);
        Ok(())
    }
}

type Listener = Box<dyn Fn(u32) + Send + Sync>;

type Pending = Mutex<HashMap<u64, mpsc::Sender<std::result::Result<Vec<u8>, String>>>>;

/// 共有の daemon のコア。
pub struct Remote {
    writer: Mutex<Box<dyn Write + Send>>,
    shared: Arc<Shared>,
    next: AtomicU64,
}

#[derive(Default)]
struct Shared {
    pending: Pending,
    listeners: Mutex<Vec<Listener>>,
    closed: Mutex<Option<String>>,
}

impl Remote {
    /// 共有の daemon で `location`（手元のパスか `ssh://host/path`）を開いてつなぐ。
    pub fn connect(location: &str, daemon: &str) -> Result<Self> {
        let location = if location.starts_with("ssh://") {
            location.to_owned()
        } else {
            amonhen_remote::local::canonical_path(location)
                .map_err(|e| tr!("Can’t open {location}: {e}", location = location, e = e))?
        };
        let stream = amonhen_remote::local::stream(daemon)?;
        let mut writer = std::io::BufWriter::new(stream.writer);
        amonhen_remote::local::write_preamble(
            &mut writer,
            &format!("{}{location}", crate::PREAMBLE),
        )?;
        let shared = Arc::new(Shared::default());
        let reading = shared.clone();
        std::thread::Builder::new()
            .name("amonhen-front".into())
            .spawn(move || {
                let mut reader = std::io::BufReader::new(stream.reader);
                let reason = loop {
                    match frame::read(&mut reader) {
                        Ok(Some(f)) => match f.kind {
                            Kind::Ok | Kind::Err => {
                                let reply = if f.kind == Kind::Ok {
                                    Ok(f.payload)
                                } else {
                                    Err(String::from_utf8_lossy(&f.payload).into_owned())
                                };
                                if let Some(tx) = reading.pending.lock().unwrap().remove(&f.id) {
                                    let _ = tx.send(reply);
                                }
                            }
                            Kind::Notify => {
                                let bits = serde_json::from_slice::<Value>(&f.payload)
                                    .ok()
                                    .and_then(|v| v["changes"].as_u64())
                                    .unwrap_or(0) as u32;
                                for l in reading.listeners.lock().unwrap().iter() {
                                    l(bits);
                                }
                            }
                            _ => {}
                        },
                        Ok(None) => break tr!("Lost connection to the daemon"),
                        Err(e) => break tr!("Lost connection to the daemon: {detail}", detail = e),
                    }
                };
                *reading.closed.lock().unwrap() = Some(reason.clone());
                for (_, tx) in reading.pending.lock().unwrap().drain() {
                    let _ = tx.send(Err(reason.clone()));
                }
            })?;
        let remote = Self {
            writer: Mutex::new(Box::new(writer)),
            shared,
            next: AtomicU64::new(1),
        };
        // 開けたかを確かめる（開けなければ、daemon が理由を返して閉じる）。
        let hello = remote.call(serde_json::json!({
            "op": "hello",
            "language": amonhen_model::i18n::current().code(),
        }))?;
        if hello.json["version"] != crate::VERSION {
            return Err(tr!(
                "The daemon’s front-end interface version differs: {version}",
                version = hello.json["version"]
            )
            .into());
        }
        Ok(remote)
    }

    fn send(&self, kind: Kind, id: u64, payload: &[u8]) -> Result<()> {
        let mut w = self.writer.lock().unwrap();
        frame::write_with(&mut *w, kind, id, payload, false)?;
        w.flush()?;
        Ok(())
    }
}

impl Front for Remote {
    fn call_until(&self, request: Value, cancelled: &(dyn Fn() -> bool + Sync)) -> Result<Reply> {
        if let Some(reason) = self.closed() {
            return Err(reason.into());
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        self.shared.pending.lock().unwrap().insert(id, tx);
        if let Err(e) = self.send(Kind::Request, id, request.to_string().as_bytes()) {
            self.shared.pending.lock().unwrap().remove(&id);
            return Err(e);
        }
        loop {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(Ok(payload)) => return Reply::decode(&payload),
                Ok(Err(e)) => return Err(e.into()),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if cancelled() {
                        self.shared.pending.lock().unwrap().remove(&id);
                        let _ = self.send(Kind::Cancel, id, &[]);
                        return Err(amonhen_model::i18n::cancelled().into());
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(tr!("Lost connection to the daemon").into());
                }
            }
        }
    }

    fn subscribe(&self, f: Box<dyn Fn(u32) + Send + Sync>) -> Result<()> {
        self.shared.listeners.lock().unwrap().push(f);
        Ok(())
    }

    fn closed(&self) -> Option<String> {
        self.shared.closed.lock().unwrap().clone()
    }
}
