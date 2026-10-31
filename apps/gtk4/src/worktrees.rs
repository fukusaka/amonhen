//! worktree（docs/ux.md の「Worktrees」。macOS 版の `Worktrees.swift` と `RepositoryWindow` の worktree の部分）。
//!
//! 一覧は開いたリポジトリの `worktrees` で読み、変更の数は `worktreeChanges` で 1 回に頼む（worktree ごとには
//! 開かない。SSH 先でも 1 本のつながりで済む）。窓が前にある間は 10 秒ごとに頼み（変わっていなければエンジンは
//! git を動かさない）、5 分ごとに全部を数え直す（stage せずに書き換えた、変更のない worktree を見つけるため）。
//!
//! 窓は書き込む先の worktree（git の「今いる worktree」）を 1 つ持つ。HEAD・index・作業ツリーに触れる操作と
//! Working Copy はそこに向け、History・ref は開いたリポジトリのものを全部の worktree で共通に使う。ほかの
//! worktree を書き込む先にしたら、その worktree をリポジトリとして開いて見張る（開いたままにするのは開いた
//! リポジトリと、最近書き込む先にした 3 つまで。SSH 先では 1 つごとにつながりを使う）。

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use amonhen_core::{RefsSnapshot, Repository, StatusSnapshot};
use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gio, glib};
use serde_json::{Value, json};

/// コアの `worktrees` の 1 つ（`crates/core/src/worktree.rs`）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Info {
    pub path: String,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub detached: bool,
    pub bare: bool,
    pub locked: Option<String>,
    pub prunable: Option<String>,
    pub main: bool,
    /// 開いたリポジトリの worktree。
    pub current: bool,
    /// 見せる名前: フォルダの名前。同じ名前のものがほかにあれば、見分けがつくところまで親のフォルダを足す
    /// （`build/aslr/src` と `build/ssp/src` なら `aslr/src`・`ssp/src`）。
    pub name: String,
    /// 比べるためのパス（手元ならシンボリックリンクを解いたもの）。
    pub key: String,
}

impl Info {
    fn from_json(v: &Value, remote: bool) -> Option<Self> {
        let s = |k: &str| v[k].as_str().map(str::to_owned);
        let path = s("path")?;
        Some(Self {
            key: canonical(&path, remote),
            name: path
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or(&path)
                .to_owned(),
            head: s("head"),
            branch: s("branch"),
            detached: v["detached"].as_bool().unwrap_or(false),
            bare: v["bare"].as_bool().unwrap_or(false),
            locked: s("locked"),
            prunable: s("prunable"),
            main: v["main"].as_bool().unwrap_or(false),
            current: v["current"].as_bool().unwrap_or(false),
            path,
        })
    }

    /// 札・行の頭の印: ✎ 書き込む先、⌂ メインの作業ツリー、⑂ 追加した worktree。
    pub fn mark(&self, editing: bool) -> &'static str {
        if editing {
            "✎"
        } else if self.main {
            "⌂"
        } else {
            "⑂"
        }
    }
}

/// `worktrees` の答えを読む（名前は重ならないところまで親のフォルダを足す）。
pub fn parse_list(info: &Value, remote: bool) -> Vec<Info> {
    let mut list: Vec<Info> = info["worktrees"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| Info::from_json(v, remote))
        .collect();
    disambiguate(&mut list);
    list
}

/// 同じ名前のものがなくなるまで、親のフォルダを足す。
pub fn disambiguate(list: &mut [Info]) {
    let parts: Vec<Vec<String>> = list
        .iter()
        .map(|w| {
            w.path
                .split('/')
                .filter(|p| !p.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .collect();
    let mut depth = vec![1usize; list.len()];
    let label = |i: usize, depth: &[usize]| {
        let p = &parts[i];
        p[p.len().saturating_sub(depth[i])..].join("/")
    };
    loop {
        let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
        for i in 0..list.len() {
            groups.entry(label(i, &depth)).or_default().push(i);
        }
        let mut grew = false;
        for members in groups.values().filter(|m| m.len() > 1) {
            for &i in members {
                if depth[i] < parts[i].len() {
                    depth[i] += 1;
                    grew = true;
                }
            }
        }
        if !grew {
            break;
        }
    }
    for (i, w) in list.iter_mut().enumerate() {
        if !parts[i].is_empty() {
            w.name = label(i, &depth);
        }
    }
}

/// 名前の順（大文字小文字を区別せず、数は数として比べる。w2 は w10 より前）。
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut n = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        n.push(c);
                        it.next();
                    }
                    n
                };
                let (m, n) = (take(&mut a), take(&mut b));
                let (m, n) = (m.trim_start_matches('0'), n.trim_start_matches('0'));
                let order = m.len().cmp(&n.len()).then_with(|| m.cmp(n));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                let order = x.to_lowercase().cmp(y.to_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
                a.next();
                b.next();
            }
        }
    }
}

/// Working Copy の下に並べる worktree: メインの作業ツリー（変更がなくてもいつも出す）、書き込む先、変更のあるもの、
/// フォルダが消えた・ロックしたもの（メインを先に、残りは名前の順）と、「No changes」にまとめるもの（2 つ以上
/// あるときだけまとめる）。worktree が 1 つなら出さない。
pub fn layout(
    list: &[Info],
    counts: &HashMap<String, usize>,
    editing: &str,
) -> (Vec<Info>, Vec<Info>) {
    if list.len() < 2 {
        return (Vec::new(), Vec::new());
    }
    let mut ordered = list.to_vec();
    ordered.sort_by(|a, b| {
        b.main
            .cmp(&a.main)
            .then_with(|| natural_cmp(&a.name, &b.name))
    });
    let (mut shown, mut clean) = (Vec::new(), Vec::new());
    for w in &ordered {
        let changes = counts.get(&w.key).copied().unwrap_or(0);
        let always = w.main || w.key == editing || w.prunable.is_some() || w.locked.is_some();
        if always || changes > 0 {
            shown.push(w.clone());
        } else {
            clean.push(w.clone());
        }
    }
    if clean.len() < 2 {
        (ordered, Vec::new())
    } else {
        (shown, clean)
    }
}

thread_local! {
    /// 解いたパス（一覧を描くたびに引かないよう覚えておく）。
    static CANONICAL: RefCell<HashMap<String, String>> = RefCell::default();
}

/// シンボリックリンクを解いたパス（macOS の /var → /private/var など）。SSH 先・daemon 越しのパスは手元の
/// ものではないので、そのまま比べる。
pub fn canonical(path: &str, remote: bool) -> String {
    if remote {
        return path.trim_end_matches('/').to_owned();
    }
    if let Some(known) = CANONICAL.with(|c| c.borrow().get(path).cloned()) {
        return known;
    }
    let resolved = std::fs::canonicalize(path)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| path.trim_end_matches('/').to_owned());
    CANONICAL.with(|c| c.borrow_mut().insert(path.to_owned(), resolved.clone()));
    resolved
}

/// サイドバーに出す worktree の様子。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    pub list: Vec<Info>,
    /// 書き込む先の worktree（`Info::key`）。
    pub editing: String,
    /// worktree ごとの変更の数（`Info::key` で引く）。
    pub counts: HashMap<String, usize>,
}

impl View {
    /// Working Copy の下に並べるもの（[`layout`]）。
    pub fn layout(&self) -> (Vec<Info>, Vec<Info>) {
        layout(&self.list, &self.counts, &self.editing)
    }

    /// 書き込む先の worktree（worktree が 2 つ以上あるときだけ）。
    pub fn editing_info(&self) -> Option<&Info> {
        (self.list.len() > 1)
            .then(|| self.list.iter().find(|w| w.key == self.editing))
            .flatten()
    }
}

type Listener = Rc<dyn Fn()>;
type EditListener = Rc<dyn Fn(Arc<Repository>)>;

/// 開いた worktree の見張りからの知らせ（裏のスレッドから）。
pub enum Note {
    /// その worktree の status を読み直した。
    Status(Arc<Repository>, Arc<StatusSnapshot>),
    /// ref（全部の worktree で共通）か、その worktree の HEAD が動いた。
    Refs,
}

pub type Notify = Arc<dyn Fn(Note) + Send + Sync>;

/// 見張りのスレッドに止まれと伝える値（変化の知らせのビットとは重ならない）。
const STOP: u32 = u32::MAX;

/// 書き込む先にした、開いたリポジトリでない worktree: そのリポジトリと、変化の見張り。
struct Slot {
    key: String,
    repo: Arc<Repository>,
    stop: std::sync::mpsc::Sender<u32>,
}

impl Drop for Slot {
    fn drop(&mut self) {
        let _ = self.stop.send(STOP);
    }
}

/// `repo` の index・作業ツリー・ref を見張り、変わったら status を読み直して知らせる（裏のスレッドで呼ぶ）。
fn watch(
    repo: Arc<Repository>,
    notify: Notify,
    tx: std::sync::mpsc::Sender<u32>,
    rx: std::sync::mpsc::Receiver<u32>,
) {
    let status = |repo: &Arc<Repository>| {
        if let Ok(snapshot) = repo.load_status() {
            notify(Note::Status(repo.clone(), snapshot));
        }
    };
    status(&repo);
    let remote_tx = tx.clone();
    let remote = repo.subscribe_changes(Box::new(move |bits| {
        let _ = remote_tx.send(bits);
    }));
    let _watcher = if remote {
        None
    } else {
        amonhen_watch::Watcher::new(repo.paths(), move |changes| {
            let _ = tx.send(changes.0);
        })
        .ok()
    };
    use amonhen_watch::Changes;
    while let Ok(mut bits) = rx.recv() {
        if bits == STOP {
            return;
        }
        // まとめて来た知らせは 1 回の読み直しにする。
        while let Ok(more) = rx.try_recv() {
            if more == STOP {
                return;
            }
            bits |= more;
        }
        let changes = Changes(bits);
        if changes.contains(Changes::REFS) {
            notify(Note::Refs);
        }
        if changes.contains(Changes::REFS)
            || changes.contains(Changes::INDEX)
            || changes.contains(Changes::WORKTREE)
        {
            status(&repo);
        }
    }
}

/// 窓の場所（手元のパス、`daemon:…`、`ssh://…`）の、パスを `path` に替えたもの。
fn location_for(location: &str, path: &str) -> String {
    if let Some(rest) = location.strip_prefix("ssh://") {
        let host = rest.split('/').next().unwrap_or(rest);
        let sep = if path.starts_with('/') { "" } else { "/" };
        return format!("ssh://{host}{sep}{path}");
    }
    if location.starts_with("daemon:") {
        return format!("daemon:{path}");
    }
    path.to_owned()
}

/// 窓の worktree: 一覧と変更の数。
pub struct Worktrees {
    opened: RefCell<Option<Arc<Repository>>>,
    /// 窓の場所（ほかの worktree を同じつなぎ方で開く）。
    location: String,
    remote: bool,
    /// 書き込む先（None なら開いたリポジトリ）。
    editing: RefCell<Option<Rc<Slot>>>,
    /// 開いたままにしている worktree（古い順。書き込む先は最後）。
    slots: RefCell<Vec<Rc<Slot>>>,
    /// 開いている途中の worktree（重ねて開かない）。
    opening: RefCell<HashSet<String>>,
    /// 書き込む先を変えた（そのリポジトリ）。
    on_edit: RefCell<Option<EditListener>>,
    /// 見張りの知らせ。
    notify: RefCell<Option<Notify>>,
    list: RefCell<Vec<Info>>,
    counts: RefCell<HashMap<String, usize>>,
    /// ほかの worktree の status の指紋（変わっていなければ画面を変えない）。
    stamps: RefCell<HashMap<String, u64>>,
    /// 数を頼んでいる最中（重ねて頼まない）。
    counting: Cell<bool>,
    /// 最後に全部を数え直した時刻。
    last_fresh: Cell<Option<Instant>>,
    /// 一覧を読む番号（古い答えは使わない）。
    loading: Cell<u64>,
    /// 一覧（と書き込む先）が変わった。
    on_list: RefCell<Vec<Listener>>,
    /// 変更の数が変わった。
    on_counts: RefCell<Vec<Listener>>,
    /// 裏で読んでいる数（`--snapshot` はなくなるまで待つ）。
    pending: Rc<Cell<u32>>,
}

/// 窓が前にある間に数を頼む間隔と、全部を数え直す間隔。
const POLL: Duration = Duration::from_secs(10);
const FRESH: Duration = Duration::from_secs(300);
/// 開いたままにする worktree の数（開いたリポジトリのほか）。
const KEPT: usize = 3;

impl Worktrees {
    pub fn new(location: &str, pending: Rc<Cell<u32>>) -> Rc<Self> {
        Rc::new(Self {
            opened: RefCell::default(),
            location: location.to_owned(),
            remote: amonhen_remote::location::is_remote(location),
            editing: RefCell::default(),
            slots: RefCell::default(),
            opening: RefCell::default(),
            on_edit: RefCell::default(),
            notify: RefCell::default(),
            list: RefCell::default(),
            counts: RefCell::default(),
            stamps: RefCell::default(),
            counting: Cell::new(false),
            last_fresh: Cell::new(None),
            loading: Cell::new(0),
            on_list: RefCell::default(),
            on_counts: RefCell::default(),
            pending,
        })
    }

    /// 開いたリポジトリ（History を読むたびに入れる。初めてなら一覧を読む）。
    pub fn set_opened(self: &Rc<Self>, repo: Arc<Repository>) {
        let first = self.opened.replace(Some(repo)).is_none();
        if first {
            self.reload_list();
        }
    }

    pub fn opened(&self) -> Option<Arc<Repository>> {
        self.opened.borrow().clone()
    }

    pub fn list(&self) -> Vec<Info> {
        self.list.borrow().clone()
    }

    /// 開いたリポジトリの worktree の鍵。
    pub fn opened_key(&self) -> String {
        if let Some(w) = self.list.borrow().iter().find(|w| w.current) {
            return w.key.clone();
        }
        self.opened
            .borrow()
            .as_ref()
            .and_then(|r| r.paths().workdir)
            .map(|p| canonical(&p.to_string_lossy(), self.remote))
            .unwrap_or_default()
    }

    /// 書き込む先の worktree の鍵。
    pub fn editing_key(&self) -> String {
        match self.editing.borrow().as_ref() {
            Some(slot) => slot.key.clone(),
            None => self.opened_key(),
        }
    }

    /// 書き込む先の worktree のリポジトリ（まだ開いていなければ None）。
    pub fn editing_repo(&self) -> Option<Arc<Repository>> {
        match self.editing.borrow().as_ref() {
            Some(slot) => Some(slot.repo.clone()),
            None => self.opened(),
        }
    }

    /// `repo` が書き込む先の worktree のものか。
    pub fn is_editing(&self, repo: &Arc<Repository>) -> bool {
        self.editing_repo().is_none_or(|e| Arc::ptr_eq(&e, repo))
    }

    /// 書き込む先の worktree の名前（worktree が 2 つ以上あるときだけ）。確認のシートに出す。
    pub fn editing_name(&self) -> Option<String> {
        self.view().editing_info().map(|w| w.name.clone())
    }

    /// ツールバーのブランチの横に出す、書き込む先の worktree の名前（開いたものでないか、開いたものが
    /// メインの作業ツリーでないときだけ）。
    pub fn toolbar_name(&self) -> Option<String> {
        let view = self.view();
        let info = view.editing_info()?;
        (self.editing.borrow().is_some() || !info.main).then(|| info.name.clone())
    }

    /// History の、各 worktree の HEAD の札（コミット、札の文字、書き込む先か）。札は印と名前と、変更のある
    /// ファイルの数。worktree が 1 つなら空。
    pub fn heads(&self) -> Vec<(amonhen_model::Oid, String, bool)> {
        let list = self.list.borrow();
        if list.len() < 2 {
            return Vec::new();
        }
        let editing = self.editing_key();
        // 同じコミットにいくつもあれば、書き込む先、メイン、名前の順（札は 3 つまでしか出さない）。
        let mut ordered: Vec<&Info> = list.iter().collect();
        ordered.sort_by(|a, b| {
            (b.key == editing)
                .cmp(&(a.key == editing))
                .then(b.main.cmp(&a.main))
                .then_with(|| natural_cmp(&a.name, &b.name))
        });
        ordered
            .into_iter()
            .filter_map(|w| {
                let head = amonhen_model::Oid::from_hex(w.head.as_deref()?)?;
                let is_editing = w.key == editing;
                let changes = self.counts.borrow().get(&w.key).copied().unwrap_or(0);
                let mut name = format!("{} {}", w.mark(is_editing), w.name);
                if changes > 0 {
                    name.push_str(&format!(" · {changes}"));
                }
                Some((head, name, is_editing))
            })
            .collect()
    }

    /// 書き込む先でない worktree が checkout しているブランチ → その worktree（名前、パス）。git はこれらを
    /// ここでは checkout させない。
    pub fn branches_elsewhere(&self) -> HashMap<String, Info> {
        let list = self.list.borrow();
        if list.len() < 2 {
            return HashMap::new();
        }
        let editing = self.editing_key();
        list.iter()
            .filter(|w| w.key != editing)
            .filter_map(|w| Some((w.branch.clone()?, w.clone())))
            .collect()
    }

    /// `key` の worktree の変更の数。
    pub fn count(&self, key: &str) -> usize {
        self.counts.borrow().get(key).copied().unwrap_or(0)
    }

    /// `key` の worktree を閉じる（消す前に）。書き込む先なら開いたリポジトリに戻す。
    pub fn forget(&self, key: &str) {
        if self.editing.borrow().as_ref().is_some_and(|s| s.key == key) {
            self.activate(None);
        }
        self.slots.borrow_mut().retain(|s| s.key != key);
    }

    /// `key` の worktree。
    pub fn info(&self, key: &str) -> Option<Info> {
        self.list.borrow().iter().find(|w| w.key == key).cloned()
    }

    /// 開いたリポジトリの ref を、書き込む先の worktree から見たものにする（今のブランチと HEAD をその
    /// worktree のものに）。サイドバー・History・ツールバーの「今のブランチ」がそれに従う。
    pub fn adjust(&self, refs: &Arc<RefsSnapshot>) -> Arc<RefsSnapshot> {
        if self.editing.borrow().is_none() {
            return refs.clone();
        }
        let Some(info) = self.info(&self.editing_key()) else {
            return refs.clone();
        };
        use std::hash::{Hash, Hasher};
        let mut hasher = std::hash::DefaultHasher::new();
        (&info.key, &info.branch, &info.head).hash(&mut hasher);
        let mut adjusted = refs.refs.clone();
        adjusted.head_branch = info.branch.clone();
        adjusted.head = info.head.as_deref().and_then(amonhen_model::Oid::from_hex);
        Arc::new(RefsSnapshot {
            // 書き込む先が替われば別の ref として出し直す（サイドバーは同じ世代なら作り直さない）。
            generation: refs.generation ^ (hasher.finish() | 1 << 63),
            refs: adjusted,
        })
    }

    /// 書き込む先を変えたら `f(そのリポジトリ)` を呼ぶ。
    pub fn connect_edit(&self, f: impl Fn(Arc<Repository>) + 'static) {
        self.on_edit.replace(Some(Rc::new(f)));
    }

    /// 開いた worktree の見張りの知らせを `notify` で送る。
    pub fn set_notify(&self, notify: Notify) {
        self.notify.replace(Some(notify));
    }

    /// `path` の worktree を書き込む先にする。まだ開いていなければ裏で開く（SSH 先なら同じ接続先の上のパス）。
    pub fn edit(self: &Rc<Self>, path: &str) {
        let key = canonical(path, self.remote);
        if key == self.editing_key() {
            return;
        }
        if key == self.opened_key() {
            return self.activate(None);
        }
        let known = self.slots.borrow().iter().find(|s| s.key == key).cloned();
        if let Some(slot) = known {
            return self.activate(Some(slot));
        }
        if !self.opening.borrow_mut().insert(key.clone()) {
            return;
        }
        let location = location_for(&self.location, path);
        let this = Rc::downgrade(self);
        let pending = self.pending.clone();
        pending.set(pending.get() + 1);
        glib::spawn_future_local(async move {
            let started = Instant::now();
            let opened = gio::spawn_blocking(move || {
                crate::open(std::path::Path::new(&location)).map(Arc::new)
            })
            .await
            .unwrap_or_else(|_| Err(String::new()));
            pending.set(pending.get() - 1);
            let Some(this) = this.upgrade() else { return };
            this.opening.borrow_mut().remove(&key);
            crate::activity::record(
                "Worktrees: open one",
                started.elapsed().as_secs_f64() * 1000.0,
            );
            let repo = match opened {
                Ok(repo) => repo,
                Err(e) => {
                    let window = gtk::Window::list_toplevels()
                        .into_iter()
                        .filter_map(|w| w.downcast::<gtk::Window>().ok())
                        .find(|w| w.is_active());
                    crate::sheets::inform(window.as_ref(), &tr!("Can’t open the worktree"), &e)
                        .await;
                    return;
                }
            };
            let (tx, rx) = std::sync::mpsc::channel();
            if let Some(notify) = this.notify.borrow().clone() {
                let (r, t) = (repo.clone(), tx.clone());
                std::thread::spawn(move || watch(r, notify, t, rx));
            }
            let slot = Rc::new(Slot {
                key,
                repo,
                stop: tx,
            });
            this.activate(Some(slot));
        });
    }

    /// 書き込む先を `slot`（None なら開いたリポジトリ）にする。開いたままにするのは最近の 3 つまで。
    fn activate(&self, slot: Option<Rc<Slot>>) {
        if let Some(slot) = &slot {
            let mut slots = self.slots.borrow_mut();
            slots.retain(|s| s.key != slot.key);
            slots.push(slot.clone());
            let extra = slots.len().saturating_sub(KEPT);
            slots.drain(..extra);
        }
        self.editing.replace(slot);
        if let (Some(f), Some(repo)) = (self.on_edit.borrow().clone(), self.editing_repo()) {
            f(repo);
        }
        self.list_changed();
    }

    /// サイドバーに出す様子。
    pub fn view(&self) -> View {
        View {
            list: self.list(),
            editing: self.editing_key(),
            counts: self.counts.borrow().clone(),
        }
    }

    pub fn connect_list(&self, f: impl Fn() + 'static) {
        self.on_list.borrow_mut().push(Rc::new(f));
    }

    pub fn connect_counts(&self, f: impl Fn() + 'static) {
        self.on_counts.borrow_mut().push(Rc::new(f));
    }

    fn notify(listeners: &RefCell<Vec<Listener>>) {
        let all: Vec<Listener> = listeners.borrow().clone();
        for f in all {
            f();
        }
    }

    pub(crate) fn list_changed(&self) {
        Self::notify(&self.on_list);
    }

    /// 書き込む先の worktree の変更の数（Working Copy が数えたもの）。
    pub fn set_editing_count(&self, count: usize) {
        let key = self.editing_key();
        if key.is_empty() || self.counts.borrow().get(&key) == Some(&count) {
            return;
        }
        self.counts.borrow_mut().insert(key, count);
        if self.list.borrow().len() > 1 {
            Self::notify(&self.on_counts);
        }
    }

    /// 一覧を読み直す（増えた・消えた・ブランチが変わった）。変わっていたら知らせ、ほかの数を頼む。
    pub fn reload_list(self: &Rc<Self>) {
        let Some(repo) = self.opened() else { return };
        let token = self.loading.get() + 1;
        self.loading.set(token);
        let this = Rc::downgrade(self);
        let pending = self.pending.clone();
        pending.set(pending.get() + 1);
        glib::spawn_future_local(async move {
            let reply = crate::sheets::run(&repo, json!({ "op": "worktrees" })).await;
            pending.set(pending.get() - 1);
            let Some(this) = this.upgrade() else { return };
            if this.loading.get() != token || !reply["info"].is_object() {
                return;
            }
            let list = parse_list(&reply["info"], this.remote);
            if list == *this.list.borrow() {
                return;
            }
            // なくなった worktree の数は捨てる。
            let keys: Vec<String> = list.iter().map(|w| w.key.clone()).collect();
            this.counts.borrow_mut().retain(|k, _| keys.contains(k));
            this.stamps.borrow_mut().retain(|k, _| keys.contains(k));
            this.list.replace(list);
            this.list_changed();
            this.read_counts(false);
        });
    }

    /// ほかの worktree の変更の数を読み直す。開いたリポジトリに 1 回で頼む。
    pub fn read_counts(self: &Rc<Self>, fresh: bool) {
        let Some(repo) = self.opened() else { return };
        if self.counting.get() {
            return;
        }
        let editing = self.editing_key();
        let paths: Vec<String> = self
            .list
            .borrow()
            .iter()
            .filter(|w| w.key != editing && w.prunable.is_none() && !w.bare)
            .map(|w| w.path.clone())
            .collect();
        if self.list.borrow().len() < 2 || paths.is_empty() {
            return;
        }
        if fresh {
            self.last_fresh.set(Some(Instant::now()));
        }
        self.counting.set(true);
        let this = Rc::downgrade(self);
        let pending = self.pending.clone();
        pending.set(pending.get() + 1);
        glib::spawn_future_local(async move {
            let command = json!({ "op": "worktreeChanges", "paths": paths, "fresh": fresh });
            let started = Instant::now();
            let reply = crate::sheets::run(&repo, command).await;
            pending.set(pending.get() - 1);
            let Some(this) = this.upgrade() else { return };
            this.counting.set(false);
            crate::activity::record(
                if fresh {
                    "Worktrees: count all the others’ changes"
                } else {
                    "Worktrees: count the others’ changes"
                },
                started.elapsed().as_secs_f64() * 1000.0,
            );
            let mut changed = false;
            let mut moved = false;
            for w in reply["info"]["worktrees"].as_array().into_iter().flatten() {
                let (Some(path), Some(stamp)) = (w["path"].as_str(), w["stamp"].as_u64()) else {
                    continue;
                };
                let key = canonical(path, this.remote);
                let count = w["count"].as_u64().unwrap_or(0) as usize;
                let before = this.stamps.borrow_mut().insert(key.clone(), stamp);
                if before.is_some_and(|b| b != stamp) {
                    moved = true;
                }
                if this.counts.borrow_mut().insert(key, count) != Some(count) {
                    changed = true;
                }
            }
            if changed {
                Self::notify(&this.on_counts);
            }
            // HEAD・index が動いた worktree があれば、ブランチが変わったかもしれない。
            if moved {
                this.reload_list();
            }
        });
    }

    /// タブが前にある間（窓も前にある）は 10 秒ごとに数を頼み、5 分ごとに全部を数え直す。前に戻ってきたら少し
    /// 待って数え直す（ほかの worktree の変化は知らせが来ない）。タブを閉じたら止める。
    pub fn install_timer(self: &Rc<Self>, tab: &Rc<crate::tabs::Tab>) {
        let (this, t) = (Rc::downgrade(self), Rc::downgrade(tab));
        glib::timeout_add_local(POLL, move || {
            let (Some(this), Some(tab)) = (this.upgrade(), t.upgrade()) else {
                return glib::ControlFlow::Break;
            };
            if tab.closed() {
                return glib::ControlFlow::Break;
            }
            if tab.is_front() {
                let fresh = this.last_fresh.get().is_none_or(|t| t.elapsed() >= FRESH);
                this.read_counts(fresh);
            }
            glib::ControlFlow::Continue
        });
        let this = Rc::downgrade(self);
        tab.connect_front(move || {
            let this = this.clone();
            glib::timeout_add_local_once(Duration::from_millis(500), move || {
                if let Some(this) = this.upgrade() {
                    this.read_counts(false);
                }
            });
        });
    }
}

/// サイドバーの worktree の右クリック: Edit This Worktree・Show in Files・Open in Terminal（フォルダがあるときだけ。
/// 手元のものだけ）、メインでなければ Remove Worktree…（フォルダがなければ Prune Worktrees）。
pub fn menu(
    actions: &Rc<crate::actions::Actions>,
    worktrees: &Rc<Worktrees>,
    info: &Info,
) -> Vec<crate::menu::Item> {
    use crate::menu::{self, Item};
    let mut items = Vec::new();
    let path = info.path.clone();
    if info.prunable.is_none() {
        let (a, p) = (actions.clone(), path.clone());
        items.push(menu::action(tr!("Edit This Worktree"), move || {
            a.edit_worktree(&p)
        }));
        if !worktrees.remote {
            let (a, p) = (actions.clone(), path.clone());
            items.push(menu::action(tr!("Show in Files"), move || {
                if let Some(window) = a.window() {
                    crate::appmenu::reveal(window.upcast_ref(), std::path::Path::new(&p));
                }
            }));
            let (a, p) = (actions.clone(), path.clone());
            items.push(menu::action(tr!("Open in Terminal"), move || {
                if !crate::appmenu::terminal_at(std::path::Path::new(&p))
                    && let Some(window) = a.window()
                {
                    window.error_bell();
                }
            }));
        }
    }
    // メインの worktree と、開いたリポジトリの worktree は消せない。
    if !info.main && !info.current {
        items.push(Item::Separator);
        if info.prunable.is_some() {
            let a = actions.clone();
            items.push(menu::action(tr!("Prune Worktrees"), move || {
                a.perform(json!({ "op": "worktreePrune" }))
            }));
        } else {
            let (a, w, info) = (actions.clone(), worktrees.clone(), info.clone());
            items.push(menu::action(tr!("Remove Worktree…"), move || {
                remove(&a, &w, &info)
            }));
        }
    }
    items
}

/// worktree を消す（`git worktree remove`）。変更が残っていれば、それもフォルダと一緒に消えると確かめてから
/// --force（ロックしたものも）。ブランチは残る。Undo では戻せない。
pub fn remove(actions: &Rc<crate::actions::Actions>, worktrees: &Rc<Worktrees>, info: &Info) {
    if info.main || info.key == worktrees.opened_key() {
        if let Some(window) = actions.window() {
            window.error_bell();
        }
        return;
    }
    let (actions, worktrees, info) = (actions.clone(), worktrees.clone(), info.clone());
    glib::spawn_future_local(async move {
        // 消すと戻せないので、覚えている数（5 分前のこともある）ではなく、今の数を数えて確かめる。
        let mut changes = worktrees.count(&info.key);
        if let Some(repo) = worktrees.opened() {
            let command = json!({ "op": "worktreeChanges", "paths": [info.path], "fresh": true });
            let reply = crate::sheets::run(&repo, command).await;
            if let Some(count) = reply["info"]["worktrees"][0]["count"].as_u64() {
                changes = count as usize;
            }
        }
        confirm_remove(&actions, &worktrees, &info, changes).await;
    });
}

async fn confirm_remove(
    actions: &Rc<crate::actions::Actions>,
    worktrees: &Rc<Worktrees>,
    info: &Info,
    changes: usize,
) {
    use crate::sheets::{Response, Sheet};
    let message = if changes > 0 {
        format!(
            "{} {}",
            tr!(
                "{path} has uncommitted changes in {count} files. They are lost with the folder. The branch stays.",
                path = &info.path,
                count = changes
            ),
            crate::sheets::cant_undo()
        )
    } else {
        tr!(
            "Removes the folder {path}. The branch stays.",
            path = &info.path
        )
    };
    let sheet = Sheet::unnamed(
        crate::sheets::parent(actions).as_ref(),
        &tr!("Remove the worktree “{name}”?", name = &info.name),
        &message,
    );
    // 作業中の変更があれば、消えたら戻せない（赤く、Return では押さない）。
    sheet.buttons(&tr!("Remove Worktree"), changes > 0, None);
    if sheet.run().await != Response::Ok {
        return;
    }
    worktrees.forget(&info.key);
    actions.perform(json!({
        "op": "worktreeRemove",
        "path": info.path,
        "force": changes > 0 || info.locked.is_some(),
    }));
}

/// Repository > New Worktree…: `start` からブランチを作り、それを checkout した worktree を新しいフォルダに作って、
/// 書き込む先にする。フォルダの既定は、リポジトリの隣の「<名前>-<ブランチ>」（git の worktree でよくある置き方）。
pub fn new_worktree(
    actions: &Rc<crate::actions::Actions>,
    worktrees: &Rc<Worktrees>,
    start: String,
) {
    use crate::sheets::{Response, Sheet};
    let base = worktrees
        .opened()
        .and_then(|r| r.paths().workdir)
        .map(|p| p.to_string_lossy().trim_end_matches('/').to_owned())
        .unwrap_or_default();
    let (parent, repo_name) = base.rsplit_once('/').unwrap_or(("", base.as_str()));
    let (parent, repo_name) = (parent.to_owned(), repo_name.to_owned());
    let remote = worktrees.remote;
    let actions = actions.clone();
    glib::spawn_future_local(async move {
        let sheet = Sheet::unnamed(
            crate::sheets::parent(&actions).as_ref(),
            &tr!("New Worktree"),
            &tr!(
                "Creates a branch from {start} and checks it out in a new folder.",
                start = &start
            ),
        );
        let branch = gtk::Entry::new();
        branch.set_placeholder_text(Some(&tr!("Branch name")));
        branch.set_activates_default(true);
        let folder = gtk::Entry::new();
        folder.set_placeholder_text(Some(&tr!("Folder")));
        folder.set_activates_default(true);
        let fields = gtk::Box::new(gtk::Orientation::Vertical, 8);
        fields.append(&branch);
        fields.append(&folder);
        sheet.add(&fields);
        let f = folder.downgrade();
        branch.connect_changed(move |branch| {
            let Some(folder) = f.upgrade() else { return };
            let name = branch.text().trim().replace('/', "-");
            let hint = if name.is_empty() {
                tr!("Folder")
            } else {
                format!("{parent}/{repo_name}-{name}")
            };
            folder.set_placeholder_text(Some(&hint));
        });
        sheet.buttons(&tr!("Create"), false, None);
        branch.grab_focus();
        if sheet.run().await != Response::Ok {
            return;
        }
        let name = branch.text().trim().to_owned();
        if name.is_empty() {
            if let Some(window) = actions.window() {
                window.error_bell();
            }
            return;
        }
        let mut path = folder.text().trim().to_owned();
        if path.is_empty() {
            path = folder
                .placeholder_text()
                .map(|t| t.to_string())
                .unwrap_or_default();
        }
        // ~ は手元のホーム（SSH 先のパスはそのまま）。
        if !remote
            && let Some(rest) = path.strip_prefix("~/")
            && let Some(home) = std::env::var_os("HOME")
        {
            path = format!("{}/{rest}", home.to_string_lossy());
        }
        let a = actions.clone();
        actions.perform_with(
            vec![json!({ "op": "worktreeAdd", "path": path, "newBranch": name, "start": start })],
            move |reply| {
                if reply["ok"].as_bool() == Some(true) {
                    a.edit_worktree(&path);
                }
            },
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(path: &str) -> Info {
        Info {
            path: path.into(),
            key: path.into(),
            name: path.rsplit('/').next().unwrap().into(),
            ..Info::default()
        }
    }

    #[test]
    fn names_grow_until_they_differ() {
        let mut list = vec![
            info("/r/build/aslr/src"),
            info("/r/build/ssp/src"),
            info("/r/w1"),
        ];
        disambiguate(&mut list);
        let names: Vec<&str> = list.iter().map(|w| w.name.as_str()).collect();
        assert_eq!(names, ["aslr/src", "ssp/src", "w1"]);
    }

    #[test]
    fn numbers_sort_as_numbers() {
        let mut names = vec!["w10", "W2", "w1", "a"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, ["a", "w1", "W2", "w10"]);
    }

    #[test]
    fn clean_worktrees_fold_when_two_or_more() {
        let mut main = info("/r/main");
        main.main = true;
        let list = vec![main, info("/r/w2"), info("/r/w10"), info("/r/w3")];
        let counts = HashMap::from([("/r/w3".to_owned(), 2)]);
        let (shown, clean) = layout(&list, &counts, "/r/main");
        let names = |l: &[Info]| l.iter().map(|w| w.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&shown), ["main", "w3"]);
        assert_eq!(names(&clean), ["w2", "w10"]);
        // まとめるのが 1 つだけなら、まとめない。
        let (shown, clean) = layout(&list[..3], &HashMap::new(), "/r/w2");
        assert_eq!(names(&shown), ["main", "w2", "w10"]);
        assert!(clean.is_empty());
    }
}
