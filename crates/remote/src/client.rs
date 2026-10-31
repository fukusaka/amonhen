//! 手元の側: daemon につなぎ、`GitBackend`・`Operations`・`RepoFiles` を要求に置き換える。
//! コア（レイアウト・キャッシュ・差分・色付け）はこれまでどおり手元で動き、git に触れる所だけが通信になる。

use amonhen_model::tr;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use amonhen_model::{
    ApplyPatch, CommitGraph, CommitText, ConfigEntry, ConnectionState, FileChange, FormatPatch,
    GitBackend, HeadState, Oid, Operations, Outcome, PatchFile, PullMode, RawCommit, RefUpdate,
    Refs, RemoteChange, RemoteCheck, RemoteEntry, RepoFiles, RepoPaths, ResetMode, Result, Root,
    Sequence, SigningKey, Status, WorktreeChange,
};
use serde::de::DeserializeOwned;

use crate::frame::{self, Kind};
use crate::wire::{
    BUILD_ID, Bytes, CommitTextWire, GraphWire, HELLO_VERSION, Hello, Notification, Op, Request,
};

type Reply = std::result::Result<Vec<u8>, String>;
type Listener = Box<dyn Fn(Notification) + Send + Sync>;

/// daemon とのつながり。要求に番号を振り、応答を番号で待ち手に渡す（複数のスレッドから同時に呼べる）。
pub struct Connection {
    writer: Mutex<Box<dyn Write + Send>>,
    pending: Mutex<HashMap<u64, mpsc::Sender<Reply>>>,
    next: AtomicU64,
    closed: AtomicBool,
    listeners: Mutex<Vec<Listener>>,
    child: Mutex<Option<Child>>,
    /// 送った・受け取ったバイト数（フレームの中身。SSH での転送量の見積もりに使う）。
    sent: AtomicU64,
    /// 読んだ分だけ増やす（大きな応答を受け取っている途中の進み具合も分かる）。
    received: Arc<AtomicU64>,
    /// 切れた理由。
    reason: Mutex<Option<String>>,
    /// 手元の共有の daemon とのつながりか（History を用意してもらえる）。
    local: AtomicBool,
    /// daemon（や ssh）が標準エラーに書いた最後の部分。つながらなかったときの理由に添える。
    stderr: Arc<Mutex<String>>,
    /// SSH 先で使っているエンジンと、その用意のしかた（`crate::ssh::RemoteEngine::describe`）。
    remote_engine: Mutex<Option<String>>,
    /// つながっている間だけ持っておくもの（SSH の接続の上のセッションの数え。`crate::ssh`）。
    held: Mutex<Option<Box<dyn Send>>>,
}

impl Connection {
    /// `reader`・`writer` の上でつなぐ。読む側は裏のスレッドで回す。
    pub fn new(
        reader: impl Read + Send + 'static,
        writer: impl Write + Send + 'static,
    ) -> Arc<Self> {
        let conn = Arc::new(Self {
            writer: Mutex::new(Box::new(writer)),
            pending: Mutex::default(),
            next: AtomicU64::new(1),
            closed: AtomicBool::new(false),
            listeners: Mutex::default(),
            child: Mutex::new(None),
            sent: AtomicU64::new(0),
            received: Arc::default(),
            reason: Mutex::default(),
            stderr: Arc::default(),
            local: AtomicBool::new(false),
            remote_engine: Mutex::default(),
            held: Mutex::default(),
        });
        let weak = Arc::downgrade(&conn);
        let reader = Counting {
            inner: reader,
            count: conn.received.clone(),
        };
        std::thread::Builder::new()
            .name("amonhen-remote".into())
            .spawn(move || {
                let mut reader = std::io::BufReader::new(reader);
                loop {
                    let frame = frame::read(&mut reader);
                    let Some(conn) = weak.upgrade() else { break };
                    match frame {
                        Ok(Some(f)) => conn.dispatch(f),
                        _ => {
                            // 標準エラーの最後の行を読み終えるのを少し待ってから、理由に添える。
                            std::thread::sleep(Duration::from_millis(100));
                            let detail = amonhen_model::process::explain_agent(
                                conn.stderr.lock().unwrap().trim().to_owned(),
                            );
                            let reason = if detail.is_empty() {
                                tr!("Lost connection to the daemon")
                            } else {
                                tr!("Lost connection to the daemon: {detail}", detail = detail)
                            };
                            conn.close(&reason);
                            break;
                        }
                    }
                }
            })
            .expect("通信のスレッドを作れない");
        conn
    }

    /// `program args...` を起動し、その標準入出力でつなぐ（`amonhend --stdio <repo>`、`ssh host amonhend ...`）。
    pub fn spawn(program: &str, args: &[String]) -> Result<Arc<Self>> {
        let mut child = amonhen_model::process::command(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| tr!("Can’t start {program}: {e}", program = program, e = e))?;
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let conn = Self::new(stdout, std::io::BufWriter::new(stdin));
        // 標準エラーは最後の 2000 文字ほどを残す（つながらなかった理由を伝えるため）。
        let tail = conn.stderr.clone();
        std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(stderr)
                .lines()
                .map_while(std::result::Result::ok)
            {
                let mut t = tail.lock().unwrap();
                t.push_str(&line);
                t.push('\n');
                if t.len() > 2000 {
                    let cut = t.len() - 2000;
                    let cut = (cut..t.len()).find(|&i| t.is_char_boundary(i)).unwrap_or(0);
                    t.drain(..cut);
                }
            }
        });
        *conn.child.lock().unwrap() = Some(child);
        Ok(conn)
    }

    /// つながっている間だけ `value` を持っておく（切れたら捨てる）。
    pub fn hold(&self, value: Box<dyn Send>) {
        *self.held.lock().unwrap() = Some(value);
    }

    /// daemon からの知らせを受け取る。
    /// 手元の共有の daemon とのつながりにする（`crate::local::connect`）。
    pub fn mark_local(&self) {
        self.local.store(true, Ordering::Relaxed);
    }

    pub fn is_local(&self) -> bool {
        self.local.load(Ordering::Relaxed)
    }

    pub fn set_remote_engine(&self, description: String) {
        *self.remote_engine.lock().unwrap() = Some(description);
    }

    /// SSH 先で使っているエンジン（SSH のつながりでなければ None）。
    pub fn remote_engine(&self) -> Option<String> {
        self.remote_engine.lock().unwrap().clone()
    }

    pub fn on_notification(&self, f: impl Fn(Notification) + Send + Sync + 'static) {
        self.listeners.lock().unwrap().push(Box::new(f));
    }

    /// これまでに送った・受け取ったバイト数。
    pub fn traffic(&self) -> (u64, u64) {
        (
            self.sent.load(Ordering::Relaxed),
            self.received.load(Ordering::Relaxed),
        )
    }

    fn dispatch(&self, f: frame::Frame) {
        match f.kind {
            Kind::Ok | Kind::Err => {
                let reply = if f.kind == Kind::Ok {
                    Ok(f.payload)
                } else {
                    Err(String::from_utf8_lossy(&f.payload).into_owned())
                };
                if let Some(tx) = self.pending.lock().unwrap().remove(&f.id) {
                    let _ = tx.send(reply);
                }
            }
            Kind::Notify => {
                if let Ok(n) = postcard::from_bytes::<Notification>(&f.payload) {
                    for l in self.listeners.lock().unwrap().iter() {
                        l(n.clone());
                    }
                }
            }
            _ => {}
        }
    }

    /// 切れていれば、その理由。
    pub fn closed(&self) -> Option<String> {
        self.reason.lock().unwrap().clone()
    }

    fn close(&self, reason: &str) {
        *self.reason.lock().unwrap() = Some(reason.to_owned());
        self.closed.store(true, Ordering::Relaxed);
        for (_, tx) in self.pending.lock().unwrap().drain() {
            let _ = tx.send(Err(reason.to_owned()));
        }
    }

    fn send(&self, request: &Request) -> Result<(u64, mpsc::Receiver<Reply>)> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(tr!("The connection to the daemon is closed").into());
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        self.pending.lock().unwrap().insert(id, tx);
        let payload = postcard::to_allocvec(request)?;
        match frame::write(
            &mut *self.writer.lock().unwrap(),
            Kind::Request,
            id,
            &payload,
        ) {
            Ok(n) => {
                self.sent.fetch_add(n as u64, Ordering::Relaxed);
            }
            Err(e) => {
                self.pending.lock().unwrap().remove(&id);
                return Err(tr!("Can’t send to the daemon: {e}", e = e).into());
            }
        }
        Ok((id, rx))
    }

    fn decode<T: DeserializeOwned>(reply: Reply) -> Result<T> {
        match reply {
            Ok(payload) => Ok(postcard::from_bytes(&payload)?),
            Err(e) => Err(e.into()),
        }
    }

    pub fn call<T: DeserializeOwned>(&self, request: &Request) -> Result<T> {
        let (_, rx) = self.send(request)?;
        Self::decode(
            rx.recv()
                .map_err(|_| tr!("Lost connection to the daemon"))?,
        )
    }

    /// `cancelled` が true になったら daemon に取り消しを送り、`Err` を返す。
    pub fn call_cancellable<T: DeserializeOwned>(
        &self,
        request: &Request,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<T> {
        let (id, rx) = self.send(request)?;
        loop {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(reply) => return Self::decode(reply),
                Err(RecvTimeoutError::Timeout) => {
                    if cancelled() {
                        self.pending.lock().unwrap().remove(&id);
                        let _ =
                            frame::write(&mut *self.writer.lock().unwrap(), Kind::Cancel, id, &[]);
                        return Err(amonhen_model::i18n::cancelled().into());
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(tr!("Lost connection to the daemon").into());
                }
            }
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        // daemon の標準入力（書く側）を閉じれば daemon は終わる。子プロセスは待って片付ける。
        if let Some(mut child) = self.child.lock().unwrap().take() {
            *self.writer.lock().unwrap() = Box::new(std::io::sink());
            let _ = child.wait();
        }
    }
}

/// daemon の向こうのリポジトリ。
pub struct RemoteBackend {
    conn: Arc<Connection>,
    paths: RepoPaths,
    ops: RemoteOperations,
    files: Arc<RemoteFiles>,
    mailmap: Arc<Mutex<MailmapCache>>,
}

/// 受け取った mailmap。リポジトリの変化を知らされたか、しばらく経ったら、変わったかを daemon に聞き直す（変わって
/// いなければ答えは空）。
#[derive(Default)]
struct MailmapCache {
    mailmap: Option<Arc<amonhen_model::Mailmap>>,
    checked: Option<std::time::Instant>,
}

/// 変化の知らせがなくても、mailmap を聞き直すまでの間（`mailmap.file` は監視の外にありうる）。
const MAILMAP_RECHECK: std::time::Duration = std::time::Duration::from_secs(5);

impl RemoteBackend {
    /// つないで通信の形を確かめ、リポジトリの場所を受け取る。
    pub fn connect(conn: Arc<Connection>) -> Result<Self> {
        let hello: Hello = conn.call(&Request::Hello {
            version: HELLO_VERSION,
            language: amonhen_model::i18n::current().code().to_owned(),
            build: BUILD_ID.to_owned(),
        })?;
        if hello.build != BUILD_ID {
            return Err(crate::server::other_build(BUILD_ID, &hello.build).into());
        }
        let paths: RepoPaths = conn.call(&Request::Paths)?;
        // 手元の共有の daemon 越しの SSH 先なら、daemon がつないでいるエンジンを覚えておく（画面に出す）。
        if conn.is_local()
            && conn.remote_engine().is_none()
            && let Ok(Some(engine)) = conn.call::<Option<String>>(&Request::RemoteEngine)
        {
            conn.set_remote_engine(engine);
        }
        let mailmap = Arc::new(Mutex::new(MailmapCache::default()));
        let stale = Arc::downgrade(&mailmap);
        conn.on_notification(move |n| match n {
            Notification::Changed(_) => {
                if let Some(cache) = stale.upgrade() {
                    cache.lock().unwrap().checked = None;
                }
            }
        });
        Ok(Self {
            ops: RemoteOperations { conn: conn.clone() },
            files: Arc::new(RemoteFiles { conn: conn.clone() }),
            conn,
            paths,
            mailmap,
        })
    }

    pub fn connection(&self) -> &Arc<Connection> {
        &self.conn
    }
}

impl GitBackend for RemoteBackend {
    fn head(&self) -> Result<Option<Oid>> {
        self.conn.call(&Request::Head)
    }

    fn tips(&self) -> Result<Vec<Oid>> {
        self.conn.call(&Request::Tips)
    }

    fn shallow(&self) -> Result<Vec<Oid>> {
        self.conn.call(&Request::Shallow)
    }

    /// 関数は送れないので、`known` は使わずにすべて読む（コアは `read_new_commits` を使う）。
    fn read_commits(
        &self,
        tips: &[Oid],
        _known: &(dyn Fn(&Oid) -> bool + Sync),
    ) -> Result<Vec<RawCommit>> {
        self.conn.call(&Request::NewCommits {
            tips: tips.to_vec(),
            known_tips: Vec::new(),
        })
    }

    fn read_new_commits(
        &self,
        tips: &[Oid],
        known_tips: &[Oid],
        _known: &(dyn Fn(&Oid) -> bool + Sync),
    ) -> Result<Vec<RawCommit>> {
        self.commits_since(tips, known_tips)
    }

    fn commits_since(&self, tips: &[Oid], known_tips: &[Oid]) -> Result<Vec<RawCommit>> {
        self.conn.call(&Request::NewCommits {
            tips: tips.to_vec(),
            known_tips: known_tips.to_vec(),
        })
    }

    /// 初めての読み込みは、daemon が並べた History を親を行番号にした形で受け取る（ID を繰り返さない）。
    fn commit_graph(&self, tips: &[Oid]) -> Result<CommitGraph> {
        let wire: GraphWire = self.conn.call(&Request::CommitGraph {
            tips: tips.to_vec(),
        })?;
        wire.into_graph()
    }

    fn paths(&self) -> RepoPaths {
        self.paths.clone()
    }

    fn files(&self) -> Arc<dyn RepoFiles> {
        self.files.clone()
    }

    fn refs(&self) -> Result<Refs> {
        self.conn.call(&Request::Refs)
    }

    fn commit_changes(&self, commit: &Oid) -> Result<Vec<FileChange>> {
        self.conn.call(&Request::CommitChanges(*commit))
    }

    fn stash_changes(&self, stash: &Oid) -> Result<Vec<FileChange>> {
        self.conn.call(&Request::StashChanges(*stash))
    }

    fn changes_between(&self, old: Option<&Oid>, new: &Oid) -> Result<Vec<FileChange>> {
        self.conn.call(&Request::ChangesBetween(old.copied(), *new))
    }

    fn blob(&self, id: &Oid) -> Result<Arc<Vec<u8>>> {
        let bytes: Bytes = self.conn.call(&Request::Blob(*id))?;
        Ok(Arc::new(bytes.0))
    }

    fn operations(&self) -> Result<&dyn Operations> {
        Ok(&self.ops)
    }

    fn status(&self) -> Result<Status> {
        self.conn.call(&Request::Status)
    }

    fn worktree_file(&self, path: &str) -> Result<Arc<Vec<u8>>> {
        let bytes: Bytes = self.conn.call(&Request::WorktreeFile(path.to_owned()))?;
        Ok(Arc::new(bytes.0))
    }

    fn to_worktree_form(&self, path: &str, content: &[u8]) -> Result<Vec<u8>> {
        let bytes: Bytes = self.conn.call(&Request::ToWorktreeForm {
            path: path.to_owned(),
            content: Bytes(content.to_vec()),
        })?;
        Ok(bytes.0)
    }

    fn line_ending_rule(&self, path: &str) -> Result<String> {
        self.conn.call(&Request::LineEndingRule(path.to_owned()))
    }

    fn stage(&self, paths: &[&str]) -> Result<()> {
        self.conn.call(&Request::Stage(owned(paths)))
    }

    fn unstage(&self, paths: &[&str]) -> Result<()> {
        self.conn.call(&Request::Unstage(owned(paths)))
    }

    fn set_index_content(&self, path: &str, mode: u32, content: &[u8]) -> Result<()> {
        self.conn.call(&Request::SetIndexContent {
            path: path.to_owned(),
            mode,
            content: Bytes(content.to_vec()),
        })
    }

    fn commit(&self, message: &str, amend: bool) -> Result<()> {
        self.conn.call(&Request::Commit {
            message: message.to_owned(),
            amend,
        })
    }

    fn has_commit_graph(&self) -> Result<bool> {
        self.conn.call(&Request::HasCommitGraph)
    }

    fn write_commit_graph(&self, split: bool) -> Result<()> {
        self.conn.call(&Request::WriteCommitGraph { split })
    }

    fn commit_extras(&self, commit: &Oid) -> Result<amonhen_model::CommitExtras> {
        self.conn.call(&Request::CommitExtras(*commit))
    }

    fn verify_signatures(&self, ids: &[Oid]) -> Result<Vec<amonhen_model::SignatureCheck>> {
        self.conn.call(&Request::VerifySignatures(ids.to_vec()))
    }

    fn tag_objects(&self, ids: &[Oid]) -> Result<Vec<Option<Vec<u8>>>> {
        let objects: Vec<Option<Bytes>> = self.conn.call(&Request::TagObjects(ids.to_vec()))?;
        Ok(objects.into_iter().map(|o| o.map(|b| b.0)).collect())
    }

    fn verify_tags(&self, tags: &[(Oid, Vec<u8>)]) -> Result<Vec<amonhen_model::SignatureCheck>> {
        let tags = tags
            .iter()
            .map(|(id, raw)| (*id, Bytes(raw.clone())))
            .collect();
        self.conn.call(&Request::VerifyTags(tags))
    }

    fn branch_created_at(&self, names: &[&str]) -> Result<Vec<Option<Oid>>> {
        self.conn.call(&Request::BranchCreatedAt(owned(names)))
    }

    fn reflog(&self, name: &str, limit: usize) -> Result<Vec<amonhen_model::ReflogEntry>> {
        self.conn.call(&Request::Reflog {
            name: name.to_owned(),
            limit,
        })
    }

    fn connection_state(&self) -> Option<ConnectionState> {
        let (sent, received) = self.conn.traffic();
        Some(ConnectionState {
            sent,
            received,
            closed: self.conn.closed(),
            engine: self.conn.remote_engine(),
        })
    }

    fn prepared_history(&self) -> Result<Option<std::path::PathBuf>> {
        // SSH 先のファイルは手元で読めないので、手元の共有の daemon だけ。
        if !self.conn.is_local() {
            return Ok(None);
        }
        let path: Option<String> = self.conn.call(&Request::PreparedHistory)?;
        Ok(path.map(std::path::PathBuf::from))
    }

    fn delegate(&self, request: &[u8]) -> Result<Option<amonhen_model::Delegated>> {
        if !self.conn.is_local() {
            return Ok(None);
        }
        let reply: crate::wire::DelegateReply = self
            .conn
            .call(&Request::Delegate(Bytes(request.to_vec())))?;
        let parts = match &reply.region {
            #[cfg(all(unix, not(target_os = "android")))]
            Some(name) => crate::shm::parts(crate::shm::open(name)?, &reply.parts)?,
            #[cfg(not(all(unix, not(target_os = "android"))))]
            Some(_) => return Err(tr!("Shared memory isn’t available in this environment").into()),
            None => reply
                .inline
                .into_iter()
                .map(|b| amonhen_layout::Blob::from(b.0))
                .collect(),
        };
        Ok(Some(amonhen_model::Delegated {
            reply: reply.reply.0,
            parts,
        }))
    }

    fn subscribe_changes(&self, f: Box<dyn Fn(u32) + Send + Sync>) -> bool {
        self.conn.on_notification(move |n| match n {
            Notification::Changed(bits) => f(bits),
        });
        true
    }

    fn commit_summaries(&self, ids: &[Oid]) -> Result<Vec<CommitText>> {
        let texts: Vec<CommitTextWire> = self.conn.call(&Request::CommitSummaries(ids.to_vec()))?;
        Ok(texts.into_iter().map(Into::into).collect())
    }

    fn search_texts(
        &self,
        tips: &[Oid],
        terms: &[amonhen_model::search::TextTerm],
        mailmap: Option<&amonhen_model::Mailmap>,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Option<Vec<Vec<Oid>>>> {
        let request = Request::SearchTexts {
            tips: tips.to_vec(),
            terms: terms.to_vec(),
            mailmap: mailmap.is_some(),
        };
        Ok(Some(self.conn.call_cancellable(&request, cancelled)?))
    }

    fn mailmap(&self) -> Result<Arc<amonhen_model::Mailmap>> {
        let mut cache = self.mailmap.lock().unwrap();
        if let (Some(mailmap), Some(checked)) = (&cache.mailmap, cache.checked)
            && checked.elapsed() < MAILMAP_RECHECK
        {
            return Ok(mailmap.clone());
        }
        let known = cache.mailmap.as_ref().map(|m| m.fingerprint());
        let fresh: Option<amonhen_model::Mailmap> = self.conn.call(&Request::Mailmap { known })?;
        if let Some(fresh) = fresh {
            cache.mailmap = Some(Arc::new(fresh));
        }
        cache.checked = Some(std::time::Instant::now());
        Ok(cache.mailmap.clone().unwrap_or_default())
    }

    fn commit_texts(&self, ids: &[Oid]) -> Result<Vec<CommitText>> {
        let texts: Vec<CommitTextWire> = self.conn.call(&Request::CommitTexts(ids.to_vec()))?;
        Ok(texts.into_iter().map(Into::into).collect())
    }
}

fn owned(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|p| (*p).to_owned()).collect()
}

pub struct RemoteFiles {
    conn: Arc<Connection>,
}

impl RepoFiles for RemoteFiles {
    fn read(&self, root: Root, path: &str) -> Result<Option<Vec<u8>>> {
        let data: Option<Bytes> = self.conn.call(&Request::FileRead {
            root,
            path: path.to_owned(),
        })?;
        Ok(data.map(|b| b.0))
    }

    fn write(&self, root: Root, path: &str, data: &[u8]) -> Result<()> {
        self.conn.call(&Request::FileWrite {
            root,
            path: path.to_owned(),
            data: Bytes(data.to_vec()),
        })
    }

    fn remove(&self, root: Root, path: &str) -> Result<()> {
        self.conn.call(&Request::FileRemove {
            root,
            path: path.to_owned(),
        })
    }

    fn mode(&self, root: Root, path: &str) -> Result<Option<u32>> {
        self.conn.call(&Request::FileMode {
            root,
            path: path.to_owned(),
        })
    }

    fn set_mode(&self, root: Root, path: &str, mode: u32) -> Result<()> {
        self.conn.call(&Request::FileSetMode {
            root,
            path: path.to_owned(),
            mode,
        })
    }
}

pub struct RemoteOperations {
    conn: Arc<Connection>,
}

impl RemoteOperations {
    fn op<T: DeserializeOwned>(&self, op: Op) -> Result<T> {
        self.conn.call(&Request::Op(op))
    }
}

impl Operations for RemoteOperations {
    fn log_matching(
        &self,
        tips: &[Oid],
        path: Option<&str>,
        contents: Option<&str>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<Oid>> {
        let op = Op::LogMatching {
            tips: tips.to_vec(),
            path: path.map(str::to_owned),
            contents: contents.map(str::to_owned),
        };
        self.conn.call_cancellable(&Request::Op(op), cancelled)
    }

    fn blame(&self, path: &str, rev: Option<&Oid>) -> Result<Vec<u8>> {
        let bytes: Bytes = self.op(Op::Blame {
            path: path.to_owned(),
            rev: rev.copied(),
        })?;
        Ok(bytes.0)
    }

    fn file_log(&self, path: &str, limit: Option<usize>, merges: bool) -> Result<Vec<u8>> {
        let bytes: Bytes = self.op(Op::FileLog {
            path: path.to_owned(),
            limit,
            merges,
        })?;
        Ok(bytes.0)
    }

    fn head_state(&self) -> Result<HeadState> {
        self.op(Op::HeadState)
    }

    fn resolve_refs(&self, names: &[String]) -> Result<Vec<(String, Option<Oid>)>> {
        self.op(Op::ResolveRefs(names.to_vec()))
    }

    fn update_ref(&self, name: &str, value: Option<&Oid>, expected: Option<&Oid>) -> Result<()> {
        self.op(Op::UpdateRef {
            name: name.to_owned(),
            value: value.copied(),
            expected: expected.copied(),
        })
    }

    fn update_refs(&self, updates: &[RefUpdate]) -> Result<()> {
        self.op(Op::UpdateRefs(updates.to_vec()))
    }

    fn reset(&self, mode: ResetMode, oid: &Oid) -> Result<()> {
        self.op(Op::Reset { mode, oid: *oid })
    }

    fn switch_to(&self, head: &HeadState) -> Result<()> {
        self.op(Op::SwitchTo(head.clone()))
    }

    fn write_blob(&self, content: &[u8]) -> Result<Oid> {
        self.op(Op::WriteBlob(Bytes(content.to_vec())))
    }

    fn read_blob(&self, oid: &Oid) -> Result<Vec<u8>> {
        let bytes: Bytes = self.op(Op::ReadBlob(*oid))?;
        Ok(bytes.0)
    }

    fn switch_branch(&self, name: &str) -> Result<()> {
        self.op(Op::SwitchBranch(name.to_owned()))
    }

    fn track_branch(&self, remote_branch: &str) -> Result<()> {
        self.op(Op::TrackBranch(remote_branch.to_owned()))
    }

    fn create_branch(&self, name: &str, start: &str, checkout: bool) -> Result<()> {
        self.op(Op::CreateBranch {
            name: name.to_owned(),
            start: start.to_owned(),
            checkout,
        })
    }

    fn delete_branch(&self, name: &str, force: bool) -> Result<()> {
        self.op(Op::DeleteBranch {
            name: name.to_owned(),
            force,
        })
    }

    fn rename_branch(&self, from: &str, to: &str) -> Result<()> {
        self.op(Op::RenameBranch {
            from: from.to_owned(),
            to: to.to_owned(),
        })
    }

    fn create_tag(&self, name: &str, target: &str, message: Option<&str>) -> Result<()> {
        self.op(Op::CreateTag {
            name: name.to_owned(),
            target: target.to_owned(),
            message: message.map(str::to_owned),
        })
    }

    fn delete_tag(&self, name: &str) -> Result<()> {
        self.op(Op::DeleteTag(name.to_owned()))
    }

    fn remote_tags(&self, remote: &str) -> Result<Vec<(String, Oid)>> {
        self.op(Op::RemoteTags(remote.to_owned()))
    }

    fn push_tags(&self, remote: &str, names: &[&str], force: bool, delete: bool) -> Result<()> {
        self.op(Op::PushTags {
            remote: remote.to_owned(),
            names: owned(names),
            force,
            delete,
        })
    }

    fn check_branch_name(&self, name: &str) -> Result<Option<String>> {
        self.op(Op::CheckBranchName(name.to_owned()))
    }

    fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool> {
        self.op(Op::IsAncestor {
            ancestor: ancestor.to_owned(),
            descendant: descendant.to_owned(),
        })
    }

    fn predict_merge(&self, ours: &str, theirs: &[String]) -> Result<Option<Vec<Vec<String>>>> {
        self.op(Op::PredictMerge {
            ours: ours.to_owned(),
            theirs: theirs.to_vec(),
        })
    }

    fn predict_replay(&self, onto: &str, picks: &[String]) -> Result<Option<Vec<Vec<String>>>> {
        self.op(Op::PredictReplay {
            onto: onto.to_owned(),
            picks: picks.to_vec(),
        })
    }

    fn range_diff(
        &self,
        ranges: &[String],
        creation_factor: Option<u32>,
    ) -> Result<Vec<amonhen_model::RangePair>> {
        self.op(Op::RangeDiff(ranges.to_vec(), creation_factor))
    }

    fn resolve(&self, rev: &str) -> Result<Oid> {
        self.op(Op::Resolve(rev.to_owned()))
    }

    fn merge(&self, revs: &[String]) -> Result<Outcome> {
        self.op(Op::Merge(revs.to_vec()))
    }

    fn rebase(&self, onto: &str) -> Result<Outcome> {
        self.op(Op::Rebase(onto.to_owned()))
    }

    fn cherry_pick(&self, oid: &Oid, is_merge: bool) -> Result<Outcome> {
        self.op(Op::CherryPick {
            oid: *oid,
            is_merge,
        })
    }

    fn revert(&self, oid: &Oid, is_merge: bool) -> Result<Outcome> {
        self.op(Op::Revert {
            oid: *oid,
            is_merge,
        })
    }

    /// `todo` は daemon の側のパス（コアは daemon から受け取った `.git` の場所で組み立てる）。
    fn rebase_todo(
        &self,
        base: Option<&Oid>,
        todo: &Path,
        message: Option<&Path>,
        rebase_merges: bool,
    ) -> Result<Outcome> {
        self.op(Op::RebaseTodo {
            base: base.copied(),
            todo: todo.to_owned(),
            message: message.map(Path::to_owned),
            rebase_merges,
        })
    }

    fn sequence_in_progress(&self) -> Result<Option<Sequence>> {
        self.op(Op::SequenceInProgress)
    }

    fn rebase_progress(&self) -> Result<Option<(u32, u32)>> {
        self.op(Op::RebaseProgress)
    }

    fn abort(&self, sequence: Sequence) -> Result<()> {
        self.op(Op::Abort(sequence))
    }

    fn continue_sequence(&self, sequence: Sequence) -> Result<Outcome> {
        self.op(Op::ContinueSequence(sequence))
    }

    fn skip_sequence(&self, sequence: Sequence) -> Result<Outcome> {
        self.op(Op::SkipSequence(sequence))
    }

    fn format_patch(&self, commits: &[Oid], options: &FormatPatch) -> Result<Vec<PatchFile>> {
        self.op(Op::FormatPatch {
            commits: commits.to_vec(),
            options: options.clone(),
        })
    }

    fn am(&self, mbox: &[u8], three_way: Option<bool>) -> Result<Outcome> {
        self.op(Op::Am {
            mbox: Bytes(mbox.to_vec()),
            three_way,
        })
    }

    fn am_three_way(&self) -> Result<Outcome> {
        self.op(Op::AmThreeWay)
    }

    fn apply_patch(&self, patch: &[u8], options: &ApplyPatch) -> Result<Outcome> {
        self.op(Op::ApplyPatch {
            patch: Bytes(patch.to_vec()),
            options: *options,
        })
    }

    fn take_side(&self, path: &str, ours: bool) -> Result<()> {
        self.op(Op::TakeSide {
            path: path.to_owned(),
            ours,
        })
    }

    fn conflict_stages(&self, path: &str) -> Result<[Option<Oid>; 3]> {
        self.op(Op::ConflictStages(path.to_owned()))
    }

    fn fetch(&self) -> Result<()> {
        self.op(Op::Fetch)
    }

    fn pull(&self, mode: PullMode) -> Result<Outcome> {
        self.op(Op::Pull(mode))
    }

    fn branch_tracking(&self) -> Result<Vec<(String, u32, u32, bool)>> {
        self.op(Op::BranchTracking)
    }

    fn push(
        &self,
        remote: &str,
        branch: &str,
        remote_branch: &str,
        force: bool,
        expect: Option<&Oid>,
        set_upstream: bool,
    ) -> Result<()> {
        self.op(Op::Push {
            remote: remote.to_owned(),
            branch: branch.to_owned(),
            remote_branch: remote_branch.to_owned(),
            force,
            expect: expect.copied(),
            set_upstream,
        })
    }

    fn restore_worktree(&self, paths: &[&str]) -> Result<()> {
        self.op(Op::RestoreWorktree(owned(paths)))
    }

    fn stash_push(&self, message: Option<&str>, include_untracked: bool) -> Result<()> {
        self.op(Op::StashPush {
            message: message.map(str::to_owned),
            include_untracked,
        })
    }

    fn stash_apply(&self, name: &str, pop: bool) -> Result<Outcome> {
        self.op(Op::StashApply {
            name: name.to_owned(),
            pop,
        })
    }

    fn stash_drop(&self, name: &str) -> Result<()> {
        self.op(Op::StashDrop(name.to_owned()))
    }

    fn stash_store(&self, oid: &Oid, message: &str) -> Result<()> {
        self.op(Op::StashStore {
            oid: *oid,
            message: message.to_owned(),
        })
    }

    fn stash_branch(&self, branch: &str, name: &str) -> Result<Outcome> {
        self.op(Op::StashBranch {
            branch: branch.to_owned(),
            name: name.to_owned(),
        })
    }

    fn count_apart(&self, left: &str, right: &str) -> Result<(u32, u32)> {
        self.op(Op::CountApart {
            left: left.to_owned(),
            right: right.to_owned(),
        })
    }

    fn update_submodules(&self, paths: &[&str]) -> Result<()> {
        self.op(Op::UpdateSubmodules(owned(paths)))
    }

    fn config_list(&self) -> Result<Vec<ConfigEntry>> {
        self.op(Op::ConfigList)
    }

    fn config_keys(&self) -> Result<Vec<String>> {
        self.op(Op::ConfigKeys)
    }

    fn config_set(&self, key: &str, value: Option<&str>) -> Result<()> {
        self.op(Op::ConfigSet(key.to_owned(), value.map(str::to_owned)))
    }

    fn config_set_global(&self, key: &str, value: Option<&str>) -> Result<()> {
        self.op(Op::ConfigSetGlobal(
            key.to_owned(),
            value.map(str::to_owned),
        ))
    }

    fn git_version(&self) -> Result<Option<(u32, u32, u32)>> {
        self.op(Op::GitVersion)
    }

    fn remotes(&self) -> Result<Vec<RemoteEntry>> {
        self.op(Op::Remotes)
    }

    fn signing_keys(&self, format: &str) -> Result<Vec<SigningKey>> {
        self.op(Op::SigningKeys(format.to_owned()))
    }

    fn programs(&self, name: &str) -> Result<Vec<amonhen_model::Program>> {
        self.op(Op::Programs(name.to_owned()))
    }

    fn list_directory(&self, path: &str) -> Result<amonhen_model::Directory> {
        self.op(Op::ListDirectory(path.to_owned()))
    }

    fn excludes_file(&self) -> Result<(String, Option<String>)> {
        self.op(Op::ExcludesFile)
    }

    fn save_excludes_file(&self, text: &str) -> Result<()> {
        self.op(Op::SaveExcludesFile(text.to_owned()))
    }

    fn process_environment(&self) -> Result<Vec<(String, String)>> {
        self.op(Op::ProcessEnvironment)
    }

    fn machine_git(&self) -> Result<amonhen_model::MachineGit> {
        self.op(Op::MachineGit)
    }

    fn set_machine_git(&self, git: Option<&str>) -> Result<()> {
        self.op(Op::SetMachineGit(git.map(str::to_owned)))
    }

    fn check_remote(&self, url: &str) -> Result<RemoteCheck> {
        self.op(Op::CheckRemote(url.to_owned()))
    }

    fn lfs_track(&self, pattern: &str, track: bool) -> Result<()> {
        self.op(Op::LfsTrack(pattern.to_owned(), track))
    }

    fn lfs_file_count(&self) -> Result<Option<usize>> {
        self.op(Op::LfsFileCount)
    }

    fn change_remote(&self, change: &RemoteChange) -> Result<()> {
        self.op(Op::ChangeRemote(change.clone()))
    }

    fn worktree_list(&self) -> Result<Vec<u8>> {
        let bytes: Bytes = self.op(Op::WorktreeList)?;
        Ok(bytes.0)
    }

    fn change_worktree(&self, change: &WorktreeChange) -> Result<()> {
        self.op(Op::ChangeWorktree(change.clone()))
    }

    fn worktree_changes(
        &self,
        paths: &[String],
        fresh: bool,
    ) -> Result<Vec<amonhen_model::WorktreeChanges>> {
        self.op(Op::WorktreeChanges(paths.to_vec(), fresh))
    }

    fn working_stats(&self) -> Result<Vec<amonhen_model::FileStat>> {
        self.op(Op::WorkingStats)
    }

    fn ignore_matches(
        &self,
        patterns: &[String],
        limit: usize,
    ) -> Result<Vec<(Vec<String>, usize)>> {
        self.op(Op::IgnoreMatches(patterns.to_vec(), limit))
    }

    fn activity(&self) -> Result<Vec<amonhen_model::activity::Entry>> {
        self.op(Op::Activity)
    }

    fn cancel_activity(&self, seq: u64) -> Result<bool> {
        self.op(Op::CancelActivity(seq))
    }
}

/// 読んだバイト数を数える。
struct Counting<R> {
    inner: R,
    count: Arc<AtomicU64>,
}

impl<R: Read> Read for Counting<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.count.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}
