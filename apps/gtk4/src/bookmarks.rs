//! ブックマーク（macOS 版の `Bookmarks.swift`）: 個別に登録したリポジトリ（グループ・お気に入り・色ラベル・表示名）と、
//! リポジトリを探すフォルダ。`~/.config/amonhen/bookmarks.json` に置く（設定の KeyFile ではなく、構造があり手で
//! 直せる JSON。形は macOS 版の `BookmarkData` と同じ）。スキャンの結果は保存せず、一覧を開くたびに裏で取り直す。
//! 一覧の行の軽い状態（ブランチ・上流との差・変更の数）は、見えている行だけを裏で 4 つずつ読む。

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use amonhen_backend_cli::Peek;
use gtk::{gio, glib};
use serde_json::{Map, Value, json};

/// 個別に登録したリポジトリ。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bookmark {
    pub path: String,
    /// 表示名の上書き（なければフォルダの名前）。
    pub name: Option<String>,
    /// 手で作ったグループ（なければグループなし）。
    pub group: Option<String>,
    pub favorite: bool,
    /// 色ラベル（[`COLORS`] の名前）。
    pub color: Option<String>,
    /// 開いている間、何分ごとに自動で fetch するか（macOS 版の設定。この FE では使わないが、書き戻すときに残す）。
    pub auto_fetch: Option<i64>,
}

/// リポジトリを探すフォルダ。手元のパスか、SSH 先のフォルダ（`ssh://host/path`。SSH 先のエンジンが探す）。
#[derive(Clone, Debug, PartialEq)]
pub struct ScanRoot {
    pub path: String,
    /// フォルダから何段下まで探すか。
    pub depth: u32,
    /// 入らないフォルダの名前。
    pub excludes: Vec<String>,
}

impl ScanRoot {
    pub fn new(path: String) -> Self {
        Self {
            path,
            depth: 3,
            excludes: [
                "node_modules",
                "target",
                "build",
                ".build",
                "vendor",
                "Pods",
                "DerivedData",
                "dist",
            ]
            .map(str::to_owned)
            .to_vec(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Data {
    pub items: Vec<Bookmark>,
    pub groups: Vec<String>,
    pub scan_roots: Vec<ScanRoot>,
}

fn opt_str(v: &Value, key: &str) -> Option<String> {
    v[key].as_str().map(str::to_owned)
}

impl Data {
    /// macOS 版の JSON（Codable）から読む。
    fn from_json(v: &Value) -> Self {
        let strings = |v: &Value| -> Vec<String> {
            v.as_array()
                .into_iter()
                .flatten()
                .filter_map(|s| s.as_str().map(str::to_owned))
                .collect()
        };
        let items = v["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|b| {
                Some(Bookmark {
                    path: b["path"].as_str()?.to_owned(),
                    name: opt_str(b, "name"),
                    group: opt_str(b, "group"),
                    favorite: b["favorite"].as_bool().unwrap_or(false),
                    color: opt_str(b, "color"),
                    auto_fetch: b["autoFetch"].as_i64(),
                })
            })
            .collect();
        let scan_roots = v["scanRoots"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| {
                let mut root = ScanRoot::new(r["path"].as_str()?.to_owned());
                if let Some(d) = r["depth"].as_u64() {
                    root.depth = d as u32;
                }
                if r["excludes"].is_array() {
                    root.excludes = strings(&r["excludes"]);
                }
                Some(root)
            })
            .collect();
        Self {
            items,
            groups: strings(&v["groups"]),
            scan_roots,
        }
    }

    /// macOS 版と同じ形で書く（ないものは鍵ごと省く。鍵の順は名前順）。
    fn to_json(&self) -> Value {
        let items: Vec<Value> = self
            .items
            .iter()
            .map(|b| {
                let mut m = Map::new();
                m.insert("path".into(), json!(b.path));
                m.insert("favorite".into(), json!(b.favorite));
                for (key, value) in [("name", &b.name), ("group", &b.group), ("color", &b.color)] {
                    if let Some(value) = value {
                        m.insert(key.into(), json!(value));
                    }
                }
                if let Some(minutes) = b.auto_fetch {
                    m.insert("autoFetch".into(), json!(minutes));
                }
                Value::Object(m)
            })
            .collect();
        let roots: Vec<Value> = self
            .scan_roots
            .iter()
            .map(|r| json!({ "path": r.path, "depth": r.depth, "excludes": r.excludes }))
            .collect();
        json!({ "items": items, "groups": self.groups, "scanRoots": roots })
    }
}

/// 色ラベル（名前・表示・色）。並びは macOS 版（Finder のタグ）と同じ、色は GNOME の配色。
pub const COLORS: &[(&str, &str, &str)] = &[
    ("red", "Red", "#e01b24"),
    ("orange", "Orange", "#ff7800"),
    ("yellow", "Yellow", "#f6d32d"),
    ("green", "Green", "#33d17a"),
    ("blue", "Blue", "#3584e4"),
    ("purple", "Purple", "#9141ac"),
    ("gray", "Gray", "#9a9996"),
];

pub fn color(name: Option<&str>) -> Option<&'static str> {
    let name = name?;
    COLORS.iter().find(|c| c.0 == name).map(|c| c.2)
}

/// 中身・スキャンの結果が変わったとき、行の状態が変わったときの知らせ。false を返したら外す（窓を閉じた）。
type Listener = Rc<dyn Fn(Option<&str>) -> bool>;

/// 裏で 4 つずつ進める仕事。
enum Job {
    Peek(String),
    Fetch(String),
}

#[derive(Default)]
struct State {
    data: Option<Data>,
    /// スキャンで見つけたリポジトリ（フォルダ → リポジトリの並び）。
    scanned: HashMap<String, Vec<String>>,
    scanning: bool,
    /// フォルダを探せなかった理由（フォルダ → エラー）。
    scan_errors: HashMap<String, String>,
    /// 行の状態（読めなかったものは None）。
    peeks: HashMap<String, Option<Peek>>,
    queued: HashSet<String>,
    queue: VecDeque<Job>,
    running: usize,
    fetching: HashSet<String>,
    fetch_errors: HashMap<String, String>,
    listeners: Vec<Listener>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::default();
}

/// 同時に走らせる git の数（macOS 版の `peekQueue` と同じ）。
const LIMIT: usize = 4;

fn file() -> PathBuf {
    glib::user_config_dir()
        .join("amonhen")
        .join("bookmarks.json")
}

fn read() -> Option<Data> {
    let text = std::fs::read_to_string(file()).ok()?;
    serde_json::from_str::<Value>(&text)
        .ok()
        .map(|v| Data::from_json(&v))
}

/// 今の中身（まだ読んでいなければファイルから）。
pub fn data() -> Data {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.data
            .get_or_insert_with(|| read().unwrap_or_default())
            .clone()
    })
}

/// ほかのプロセス（ほかの窓の Amon Hen・macOS 版ではない同じ FE）が書き換えていたら読み直す（一覧を開くとき）。
pub fn refresh() {
    let fresh = read().unwrap_or_default();
    let changed = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let changed = s.data.as_ref() != Some(&fresh);
        s.data = Some(fresh);
        changed
    });
    if changed {
        notify(None);
    }
}

/// `f(None)`（中身が変わった）・`f(Some(場所))`（その行の状態が変わった）を呼ぶ。false を返すと外す。
pub fn connect(f: impl Fn(Option<&str>) -> bool + 'static) {
    STATE.with(|s| s.borrow_mut().listeners.push(Rc::new(f)));
}

fn notify(path: Option<&str>) {
    let listeners: Vec<Listener> = STATE.with(|s| s.borrow().listeners.clone());
    let mut dropped = Vec::new();
    for (i, f) in listeners.iter().enumerate() {
        if !f(path) {
            dropped.push(i);
        }
    }
    if !dropped.is_empty() {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            let mut i = 0;
            s.listeners.retain(|_| {
                let keep = !dropped.contains(&i);
                i += 1;
                keep
            });
        });
    }
}

/// 変える。先にファイルを読み直し（ほかのプロセスの変更を手元の古い内容で消さない）、その上で `change` を当てて書く。
fn edit(change: impl FnOnce(&mut Data)) {
    let mut next = read().unwrap_or_else(data);
    let before = next.clone();
    change(&mut next);
    STATE.with(|s| s.borrow_mut().data = Some(next.clone()));
    if next != before {
        write(&next);
    }
    notify(None);
}

fn write(data: &Data) {
    let path = file();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let text = serde_json::to_string_pretty(&data.to_json()).unwrap_or_default();
    // 書きかけを読まれないよう、隣に書いてから置き換える。
    let tmp = path.with_extension("json.tmp");
    if let Err(e) = std::fs::write(&tmp, text + "\n").and_then(|()| std::fs::rename(&tmp, &path)) {
        eprintln!("ブックマークを保存できない: {e}");
    }
}

// 編集

pub fn bookmark(path: &str) -> Option<Bookmark> {
    data().items.into_iter().find(|b| b.path == path)
}

pub fn add(path: &str, group: Option<&str>) {
    edit(|d| {
        if !d.items.iter().any(|b| b.path == path) {
            d.items.push(Bookmark {
                path: path.to_owned(),
                group: group.map(str::to_owned),
                ..Bookmark::default()
            });
        }
    });
}

/// 開いたリポジトリを、まだどこにもなければ（ブックマークにも、スキャンするフォルダの下にもない）入れる。
pub fn register(path: &str) {
    let d = data();
    let real = real(path);
    let scanned = d
        .scan_roots
        .iter()
        .any(|r| real.starts_with(&(real_root(&r.path) + "/")));
    if !scanned && !d.items.iter().any(|b| b.path == path) {
        add(path, None);
    }
}

pub fn remove(path: &str) {
    edit(|d| d.items.retain(|b| b.path != path));
}

/// `path` のブックマークを変える（なければ作る。最近開いたもの・スキャンで見つけたものをお気に入りにするときなど）。
fn update(path: &str, change: impl FnOnce(&mut Bookmark)) {
    edit(|d| {
        if !d.items.iter().any(|b| b.path == path) {
            d.items.push(Bookmark {
                path: path.to_owned(),
                ..Bookmark::default()
            });
        }
        if let Some(b) = d.items.iter_mut().find(|b| b.path == path) {
            change(b);
        }
    });
}

pub fn toggle_favorite(path: &str) {
    update(path, |b| b.favorite = !b.favorite);
}

pub fn set_favorite(path: &str, yes: bool) {
    if bookmark(path).is_some_and(|b| b.favorite) != yes {
        toggle_favorite(path);
    }
}

pub fn set_color(path: &str, color: Option<&str>) {
    update(path, |b| b.color = color.map(str::to_owned));
}

pub fn rename(path: &str, name: Option<String>) {
    update(path, |b| b.name = name.filter(|n| !n.is_empty()));
}

pub fn move_to(path: &str, group: Option<&str>) {
    update(path, |b| b.group = group.map(str::to_owned));
}

pub fn add_group(name: &str) {
    edit(|d| {
        if !d.groups.iter().any(|g| g == name) {
            d.groups.push(name.to_owned());
        }
    });
}

/// グループを消す（中のリポジトリはグループなしに戻す）。
pub fn remove_group(name: &str) {
    edit(|d| {
        d.groups.retain(|g| g != name);
        for b in &mut d.items {
            if b.group.as_deref() == Some(name) {
                b.group = None;
            }
        }
    });
}

/// グループの名前を変える（macOS 版と同じく、新しいグループに移してから元を消す）。
pub fn rename_group(old: &str, new: &str) {
    edit(|d| {
        for g in &mut d.groups {
            if g == old {
                *g = new.to_owned();
            }
        }
        for b in &mut d.items {
            if b.group.as_deref() == Some(old) {
                b.group = Some(new.to_owned());
            }
        }
    });
}

pub fn add_scan_root(path: &str) {
    edit(|d| {
        if !d.scan_roots.iter().any(|r| r.path == path) {
            d.scan_roots.push(ScanRoot::new(path.to_owned()));
        }
    });
    rescan();
}

pub fn remove_scan_root(path: &str) {
    STATE.with(|s| s.borrow_mut().scanned.remove(path));
    edit(|d| d.scan_roots.retain(|r| r.path != path));
}

pub fn set_depth(path: &str, depth: u32) {
    edit(|d| {
        if let Some(r) = d.scan_roots.iter_mut().find(|r| r.path == path) {
            r.depth = depth;
        }
    });
    rescan();
}

// 場所

/// SSH 先の場所なら、そのホスト（`ssh://host/…`・`daemon:ssh://host/…`）。手元のものは None。
pub fn ssh_host(location: &str) -> Option<String> {
    let rest = location.strip_prefix("daemon:").unwrap_or(location);
    let rest = rest.strip_prefix("ssh://")?;
    Some(rest.split('/').next().unwrap_or_default().to_owned())
}

/// 場所のパスの部分（`ssh://host/path` なら `/path`、`daemon:/path` なら `/path`）。
pub fn path_part(location: &str) -> String {
    let rest = location.strip_prefix("daemon:").unwrap_or(location);
    match rest.strip_prefix("ssh://") {
        Some(r) => r.find('/').map_or_else(String::new, |i| r[i..].to_owned()),
        None => rest.to_owned(),
    }
}

/// ホームの下を `~` で短くする（手元のパスだけ）。
pub fn tilde(path: &str) -> String {
    let home = glib::home_dir();
    match Path::new(path).strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.to_owned(),
    }
}

/// 表示名（上書きがなければフォルダの名前。SSH 先ならホストを添える）。
pub fn display_name(location: &str) -> String {
    if let Some(name) = bookmark(location).and_then(|b| b.name)
        && !name.is_empty()
    {
        return name;
    }
    let path = path_part(location);
    let last = path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(location)
        .to_owned();
    match ssh_host(location) {
        Some(host) => format!("{last} — {host}"),
        None => last,
    }
}

/// `path` が Git のリポジトリ（作業ツリーの直下に `.git` がある）か。daemon 越しの場所は、つなぐまでわからない
/// ので、あるものとする。
pub fn is_repository(location: &str) -> bool {
    amonhen_remote::location::is_remote(location) || Path::new(location).join(".git").exists()
}

/// 今のブランチ。`.git/HEAD` を読むだけで、git は起動しない（状態を読む前にすぐ出す）。
pub fn head_branch(location: &str) -> Option<String> {
    if amonhen_remote::location::is_remote(location) {
        return None;
    }
    let dir = Path::new(location);
    let mut git_dir = dir.join(".git");
    // worktree やサブモジュールでは `.git` が `gitdir: <path>` のファイルになる。
    if git_dir.is_file() {
        let text = std::fs::read_to_string(&git_dir).ok()?;
        let target = text.strip_prefix("gitdir:")?.trim();
        git_dir = dir.join(target);
    }
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let line = head.trim();
    match line.strip_prefix("ref: refs/heads/") {
        Some(branch) => Some(branch.to_owned()),
        None => Some(format!("detached {}", &line[..line.len().min(8)])),
    }
}

/// シンボリックリンクを解いたパス（比べるため）。
fn real(path: &str) -> String {
    if amonhen_remote::location::is_remote(path) {
        return path.to_owned();
    }
    std::fs::canonicalize(path).map_or_else(|_| path.to_owned(), |p| p.to_string_lossy().into())
}

fn real_root(path: &str) -> String {
    real(&expand(path).to_string_lossy())
}

/// `~/` で始まるパスを広げる。
pub fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => glib::home_dir().join(rest),
        None if path == "~" => glib::home_dir(),
        None => PathBuf::from(path),
    }
}

/// スキャンで見つけたもの（フォルダごと）。
pub fn scanned(root: &str) -> Vec<String> {
    STATE.with(|s| s.borrow().scanned.get(root).cloned().unwrap_or_default())
}

pub fn has_scanned() -> bool {
    STATE.with(|s| !s.borrow().scanned.is_empty())
}

pub fn is_scanning() -> bool {
    STATE.with(|s| s.borrow().scanning)
}

pub fn scan_error(root: &str) -> Option<String> {
    STATE.with(|s| s.borrow().scan_errors.get(root).cloned())
}

/// Quick Open・一覧のすべて: お気に入り、ブックマーク、最近開いたもの、スキャンで見つけたもの（重複なし）。
pub fn all_locations() -> Vec<String> {
    let d = data();
    let mut seen = HashSet::new();
    let favorites = d
        .items
        .iter()
        .filter(|b| b.favorite)
        .map(|b| b.path.clone());
    let items = d.items.iter().map(|b| b.path.clone());
    let scanned: Vec<String> = d.scan_roots.iter().flat_map(|r| scanned(&r.path)).collect();
    favorites
        .chain(items)
        .chain(crate::repos::recent())
        .chain(scanned)
        .filter(|p| seen.insert(p.clone()))
        .collect()
}

// スキャン

/// SSH 先に送るエンジンの置き場所（main.rs の `open_remote` と同じ決め方）。
fn ssh_daemons() -> amonhen_remote::ssh::Daemons {
    use amonhen_remote::ssh;
    let engines = std::env::var_os("AMONHEN_ENGINES")
        .map(PathBuf::from)
        .or_else(|| {
            let exe = std::env::current_exe().ok()?;
            Some(exe.parent()?.join("engines"))
        })
        .unwrap_or_default();
    ssh::Daemons::new(engines, Some((ssh::local_platform(), crate::engine())))
}

/// すべてのフォルダを裏で探し直す（SSH 先は時間がかかるので、フォルダごとに並べて探す）。
pub fn rescan() {
    if STATE.with(|s| std::mem::replace(&mut s.borrow_mut().scanning, true)) {
        return;
    }
    notify(None);
    let roots = data().scan_roots;
    glib::spawn_future_local(async move {
        let results = gio::spawn_blocking(move || {
            std::thread::scope(|scope| {
                let jobs: Vec<_> = roots
                    .iter()
                    .map(|root| scope.spawn(move || (root.path.clone(), scan_root(root))))
                    .collect();
                jobs.into_iter()
                    .filter_map(|j| j.join().ok())
                    .collect::<Vec<_>>()
            })
        })
        .await
        .unwrap_or_default();
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            let mut found = HashMap::new();
            let mut errors = HashMap::new();
            for (path, result) in results {
                match result {
                    Ok(list) => {
                        found.insert(path, list);
                    }
                    Err(e) => {
                        // 探せなかったら、前に見つけたものを残す（つながらないだけで消えたわけではない）。
                        found.insert(
                            path.clone(),
                            s.scanned.get(&path).cloned().unwrap_or_default(),
                        );
                        errors.insert(path, e);
                    }
                }
            }
            s.scanned = found;
            s.scan_errors = errors;
            s.scanning = false;
        });
        notify(None);
    });
}

fn scan_root(root: &ScanRoot) -> Result<Vec<String>, String> {
    if !amonhen_remote::location::is_remote(&root.path) {
        return Ok(scan_local(root));
    }
    use amonhen_remote::ssh;
    let target = ssh::Target::parse(&root.path).map_err(|e| e.to_string())?;
    let program = std::env::var("AMONHEN_SSH").unwrap_or_else(|_| "ssh".into());
    // SSH 先ごとの選択（macOS の Settings > SSH Hosts）は、この FE にはまだない。
    let found = ssh::find_repositories(
        &program,
        &target,
        &ssh_daemons(),
        &ssh::HostOptions::default(),
        root.depth,
        &root.excludes,
    )
    .map_err(|e| e.to_string())?;
    // 場所の形に戻す（`ssh://` と接続先はそのまま、パスだけ見つけたものに）。
    let prefix = &root.path[..root.path.len() - target.path.len()];
    Ok(found.iter().map(|p| format!("{prefix}{p}")).collect())
}

/// `root` の下のリポジトリ（作業ツリーの直下に `.git` があるフォルダ）。リポジトリの中には入らない。隠しフォルダ・
/// リンク・`excludes` の名前のフォルダには入らない。
fn scan_local(root: &ScanRoot) -> Vec<String> {
    fn walk(dir: &Path, depth: u32, excludes: &[String], found: &mut Vec<String>) {
        if dir.join(".git").exists() {
            found.push(dir.to_string_lossy().into_owned());
            return;
        }
        if depth == 0 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_dir = entry
                .file_type()
                .is_ok_and(|t| t.is_dir() && !t.is_symlink());
            if !is_dir || name.starts_with('.') || excludes.contains(&name) {
                continue;
            }
            walk(&entry.path(), depth - 1, excludes, found);
        }
    }
    let mut found = Vec::new();
    walk(&expand(&root.path), root.depth, &root.excludes, &mut found);
    found.sort_by_key(|p| p.to_lowercase());
    found
}

// 行の状態

/// `location` の状態。まだ読んでいなければ裏で読み始めて None を返し、読めたら知らせる（見えている行だけを
/// 読むため、一覧が描くときに呼ぶ）。SSH 先・daemon 越しは読まない。
pub fn peek(location: &str) -> Option<Peek> {
    if let Some(cached) = STATE.with(|s| s.borrow().peeks.get(location).cloned()) {
        return cached;
    }
    if amonhen_remote::location::is_remote(location) || !is_repository(location) {
        return None;
    }
    let queued = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let new = s.queued.insert(location.to_owned());
        if new {
            s.queue.push_back(Job::Peek(location.to_owned()));
        }
        new
    });
    if queued {
        pump();
    }
    None
}

/// 一覧を開き直したら、状態を読み直す。
pub fn forget_peeks() {
    STATE.with(|s| s.borrow_mut().peeks.clear());
}

/// 裏で読んでいる・待っているものがない（`--snapshot` で撮る前に待つ）。
pub fn idle() -> bool {
    STATE.with(|s| {
        let s = s.borrow();
        s.running == 0 && s.queue.is_empty() && !s.scanning
    })
}

pub fn is_fetching(location: &str) -> bool {
    STATE.with(|s| s.borrow().fetching.contains(location))
}

pub fn fetch_error(location: &str) -> Option<String> {
    STATE.with(|s| s.borrow().fetch_errors.get(location).cloned())
}

/// 選んだリポジトリをまとめて fetch する（同時に 4 つまで）。終わったら行の状態を読み直す。SSH 先は除く。
pub fn fetch(locations: &[String]) {
    for location in locations {
        if amonhen_remote::location::is_remote(location) || !is_repository(location) {
            continue;
        }
        let new = STATE.with(|s| {
            let mut s = s.borrow_mut();
            let new = s.fetching.insert(location.clone());
            if new {
                s.fetch_errors.remove(location);
                s.queue.push_back(Job::Fetch(location.clone()));
            }
            new
        });
        if new {
            notify(Some(location));
        }
    }
    pump();
}

/// 待っている仕事を、空いている枠の分だけ裏で始める。
fn pump() {
    loop {
        let job = STATE.with(|s| {
            let mut s = s.borrow_mut();
            if s.running >= LIMIT {
                return None;
            }
            let job = s.queue.pop_front()?;
            s.running += 1;
            Some(job)
        });
        let Some(job) = job else { return };
        glib::spawn_future_local(async move {
            match job {
                Job::Peek(path) => {
                    let p = path.clone();
                    let peek =
                        gio::spawn_blocking(move || amonhen_backend_cli::Git::new(p).peek().ok())
                            .await
                            .ok()
                            .flatten();
                    STATE.with(|s| {
                        let mut s = s.borrow_mut();
                        s.queued.remove(&path);
                        s.peeks.insert(path.clone(), peek);
                        s.running -= 1;
                    });
                    notify(Some(&path));
                }
                Job::Fetch(path) => {
                    let p = path.clone();
                    let error = gio::spawn_blocking(move || {
                        let repo = crate::open(Path::new(&p))?;
                        let reply: Value =
                            serde_json::from_str(&repo.run_json(r#"{"op":"fetch"}"#))
                                .unwrap_or_default();
                        match reply["error"].as_str() {
                            Some(e) => Err(e.to_owned()),
                            None => Ok(()),
                        }
                    })
                    .await
                    .unwrap_or_else(|_| Err(String::new()))
                    .err();
                    STATE.with(|s| {
                        let mut s = s.borrow_mut();
                        s.fetching.remove(&path);
                        if let Some(e) = error {
                            s.fetch_errors.insert(path.clone(), e);
                        }
                        s.peeks.remove(&path);
                        s.running -= 1;
                    });
                    notify(Some(&path));
                }
            }
            pump();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_writes_the_macos_shape() {
        let text = r#"{"groups":["work"],"items":[{"favorite":true,"group":"work","path":"/a","color":"red","autoFetch":15},{"favorite":false,"path":"ssh://h/b"}],"scanRoots":[{"depth":2,"excludes":["x"],"path":"~/prj"}]}"#;
        let data = Data::from_json(&serde_json::from_str(text).unwrap());
        assert_eq!(data.items.len(), 2);
        assert!(data.items[0].favorite);
        assert_eq!(data.items[0].auto_fetch, Some(15));
        assert_eq!(data.scan_roots[0].depth, 2);
        assert_eq!(data.to_json(), serde_json::from_str::<Value>(text).unwrap());
    }

    #[test]
    fn splits_locations() {
        assert_eq!(ssh_host("ssh://me@h:22/x/y").as_deref(), Some("me@h:22"));
        assert_eq!(ssh_host("daemon:ssh://h/x").as_deref(), Some("h"));
        assert_eq!(ssh_host("/x"), None);
        assert_eq!(path_part("ssh://h/x/y"), "/x/y");
        assert_eq!(path_part("daemon:/x"), "/x");
    }

    #[test]
    fn scans_below_a_folder() {
        let dir = std::env::temp_dir().join(format!("amonhen-scan-{}", std::process::id()));
        for p in [
            "a/.git",
            "b/c/.git",
            "node_modules/d/.git",
            ".hidden/e/.git",
            "a/inner/.git",
        ] {
            std::fs::create_dir_all(dir.join(p)).unwrap();
        }
        let mut root = ScanRoot::new(dir.to_string_lossy().into());
        let found: Vec<String> = scan_local(&root)
            .iter()
            .map(|p| p[dir.to_string_lossy().len() + 1..].to_owned())
            .collect();
        assert_eq!(found, ["a", "b/c"]);
        root.depth = 1;
        assert_eq!(scan_local(&root).len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
