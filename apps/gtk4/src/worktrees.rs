//! worktree（docs/ux.md の「Worktrees」。macOS 版の `Worktrees.swift` と `RepositoryWindow` の worktree の部分）。
//!
//! 一覧は開いたリポジトリの `worktrees` で読み、変更の数は `worktreeChanges` で 1 回に頼む（worktree ごとには
//! 開かない。SSH 先でも 1 本のつながりで済む）。窓が前にある間は 10 秒ごとに頼み（変わっていなければエンジンは
//! git を動かさない）、5 分ごとに全部を数え直す（stage せずに書き換えた、変更のない worktree を見つけるため）。

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use amonhen_core::Repository;
use gtk::glib;
use gtk::prelude::*;
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
}

type Listener = Rc<dyn Fn()>;

/// 窓の worktree: 一覧と変更の数。
pub struct Worktrees {
    opened: RefCell<Option<Arc<Repository>>>,
    remote: bool,
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

impl Worktrees {
    pub fn new(remote: bool, pending: Rc<Cell<u32>>) -> Rc<Self> {
        Rc::new(Self {
            opened: RefCell::default(),
            remote,
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
        self.opened_key()
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

    /// 窓が前にある間は 10 秒ごとに数を頼み、5 分ごとに全部を数え直す。前に戻ってきたら少し待って数え直す
    /// （ほかの worktree の変化は知らせが来ない）。
    pub fn install_timer(self: &Rc<Self>, window: &gtk::ApplicationWindow) {
        let (this, w) = (Rc::downgrade(self), window.downgrade());
        glib::timeout_add_local(POLL, move || {
            let (Some(this), Some(window)) = (this.upgrade(), w.upgrade()) else {
                return glib::ControlFlow::Break;
            };
            if window.is_active() {
                let fresh = this.last_fresh.get().is_none_or(|t| t.elapsed() >= FRESH);
                this.read_counts(fresh);
            }
            glib::ControlFlow::Continue
        });
        let this = Rc::downgrade(self);
        window.connect_is_active_notify(move |window| {
            if !window.is_active() {
                return;
            }
            let this = this.clone();
            glib::timeout_add_local_once(Duration::from_millis(500), move || {
                if let Some(this) = this.upgrade() {
                    this.read_counts(false);
                }
            });
        });
    }
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
