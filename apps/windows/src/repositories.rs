//! Repositories（Window ▸ Repositories、Ctrl+Shift+R。GTK4 版の `repobrowser.rs`・`bookmarks.rs`、macOS 版の
//! `RepositoryBrowser.swift`）: ブックマーク・最近開いたもの・スキャンで見つけたものを見渡して開くページ。左に分類
//! （すべて・お気に入り・このコンピューターとスキャンするフォルダ・グループ）と件数、右にその中のリポジトリを列
//! （名前とパス・ブランチ・状態・最後のコミット・開いた日時）で並べる。
//!
//! ブックマークは macOS 版・GTK4 版と同じ形の `bookmarks.json`（設定と同じフォルダ）。知らない鍵（自動 fetch・
//! 窓の背景など）は読んだ JSON のまま書き戻す。窓にタブがないので、開くと新しい窓（別のプロセス）で開く。
//!
//! まだないもの: 列の並べ替え・幅、表示名の変更、SSH・daemon の場所、スキャンの深さ、一覧からの fetch。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use amonhen_backend_cli::Peek;
use amonhen_model::tr;
use serde_json::{Map, Value, json};
use windows_reactor::*;

/// 最後に開いた日時（場所 → UNIX 時間。macOS 版・GTK4 版と同じ鍵）と、最近開いたもの（新しい順）。
const OPENED: &str = "repositoryOpenedAt";
const RECENT: &str = "recentRepositories";
/// 並べた列と向き（GTK4 版・macOS 版と同じ鍵。既定は開いた日時の新しい順）。
const SORT_KEY: &str = "repositoryPageSort";
const SORT_ASCENDING_KEY: &str = "repositoryPageSortAscending";
/// 最近開いたものを覚える数。
const RECENT_LIMIT: usize = 50;

/// 色ラベル（名前・表示・色）。並びは macOS 版（Finder のタグ）と同じ。
const COLORS: &[(&str, &str, u32)] = &[
    ("red", "Red", 0xe01b24),
    ("orange", "Orange", 0xff7800),
    ("yellow", "Yellow", 0xf6d32d),
    ("green", "Green", 0x33d17a),
    ("blue", "Blue", 0x3584e4),
    ("purple", "Purple", 0x9141ac),
    ("gray", "Gray", 0x9a9996),
];

// ブックマークの読み書き

fn file() -> Option<PathBuf> {
    amonhen_model::settings::path().map(|p| p.with_file_name("bookmarks.json"))
}

/// `bookmarks.json`（なければ空）。
pub fn read() -> Value {
    let data = file()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|d| serde_json::from_slice::<Value>(&d).ok())
        .filter(Value::is_object);
    data.unwrap_or_else(|| json!({ "items": [], "groups": [], "scanRoots": [] }))
}

fn write(data: &Value) -> Result<(), String> {
    let path = file().ok_or("bookmarks.json")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(data).map_err(|e| e.to_string())?;
    let temp = path.with_extension(format!("json.{}", std::process::id()));
    std::fs::write(&temp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&temp, &path).map_err(|e| e.to_string())
}

/// 読み直して変え、書く（ほかの窓が書いたものを消さない）。
fn edit(change: impl FnOnce(&mut Value)) -> Result<Value, String> {
    let mut data = read();
    change(&mut data);
    write(&data)?;
    Ok(data)
}

fn array_mut<'a>(data: &'a mut Value, key: &str) -> &'a mut Vec<Value> {
    if !data[key].is_array() {
        data[key] = json!([]);
    }
    data[key].as_array_mut().expect("配列にした")
}

/// Windows のパスは大文字・小文字と区切りを区別しない。
fn key(path: &str) -> String {
    path.replace('\\', "/").trim_end_matches('/').to_lowercase()
}

fn same(a: &str, b: &str) -> bool {
    key(a) == key(b)
}

fn items(data: &Value) -> impl Iterator<Item = &Value> {
    data["items"].as_array().into_iter().flatten()
}

fn item<'a>(data: &'a Value, path: &str) -> Option<&'a Value> {
    items(data).find(|b| b["path"].as_str().is_some_and(|p| same(p, path)))
}

/// `path` のブックマークを変える（なければ作る）。
fn update(data: &mut Value, path: &str, change: impl FnOnce(&mut Map<String, Value>)) {
    let items = array_mut(data, "items");
    let index = items
        .iter()
        .position(|b| b["path"].as_str().is_some_and(|p| same(p, path)))
        .unwrap_or_else(|| {
            items.push(json!({ "path": path, "favorite": false }));
            items.len() - 1
        });
    if let Some(object) = items[index].as_object_mut() {
        change(object);
    }
}

fn items_mut_find<'a>(data: &'a mut Value, path: &str) -> Option<&'a mut Map<String, Value>> {
    data["items"]
        .as_array_mut()?
        .iter_mut()
        .find(|b| b["path"].as_str().is_some_and(|p| same(p, path)))?
        .as_object_mut()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s.as_str().map(str::to_owned))
        .collect()
}

/// `~` で始まるパスを、ホームの下のパスにする。
fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        Some(rest) => std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join(rest),
        None => PathBuf::from(path),
    }
}

/// ホームの下のパスを `~\…` で出す。
fn tilde(path: &str) -> String {
    let Some(home) = std::env::var_os("USERPROFILE") else {
        return path.to_owned();
    };
    let home = home.to_string_lossy().into_owned();
    if path.len() > home.len() && same(&path[..home.len()], &home) {
        let rest = &path[home.len()..];
        if rest.starts_with(['\\', '/']) {
            return format!("~{rest}");
        }
    }
    path.to_owned()
}

fn is_remote(location: &str) -> bool {
    amonhen_remote::location::is_remote(location)
}

fn is_repository(location: &str) -> bool {
    is_remote(location) || expand(location).join(".git").exists()
}

/// 開いたリポジトリを覚える（最後に開いた日時・最近開いたもの）。まだどこにもなければ（ブックマークにも、
/// スキャンするフォルダの下にもない）ブックマークにも入れる（GTK4 版の `bookmarks::register`）。
pub fn record_opened(path: &Path) {
    let location = path.display().to_string();
    let mut opened: Map<String, Value> = crate::prefs::string(OPENED)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    opened.retain(|k, _| !same(k, &location));
    opened.insert(location.clone(), json!(now));
    let _ = crate::prefs::set_string(OPENED, &Value::Object(opened).to_string());
    let mut recent = recent();
    recent.retain(|p| !same(p, &location));
    recent.insert(0, location.clone());
    recent.truncate(RECENT_LIMIT);
    let _ = crate::prefs::set_string(RECENT, &json!(recent).to_string());
    let data = read();
    let scanned = data["scanRoots"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r["path"].as_str())
        .any(|root| key(&location).starts_with(&(key(&expand(root).display().to_string()) + "/")));
    if !scanned && item(&data, &location).is_none() {
        let _ = edit(|d| update(d, &location, |_| {}));
    }
}

fn recent() -> Vec<String> {
    crate::prefs::string(RECENT)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn opened() -> HashMap<String, i64> {
    crate::prefs::string(OPENED)
        .and_then(|s| serde_json::from_str::<Map<String, Value>>(&s).ok())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(k, v)| Some((key(&k), v.as_i64()?)))
        .collect()
}

// スキャン

/// スキャンするフォルダ（パス・深さ・入らない名前）。
pub type Root = (String, u32, Vec<String>);

fn roots(data: &Value) -> Vec<Root> {
    data["scanRoots"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let path = r["path"].as_str()?.to_owned();
            let depth = r["depth"].as_u64().unwrap_or(3) as u32;
            Some((path, depth, strings(&r["excludes"])))
        })
        .filter(|(path, ..)| !is_remote(path))
        .collect()
}

/// フォルダの下のリポジトリを探す（`.git` のあるフォルダ。その中には入らない。GTK4 版の `scan_local` と同じ）。
pub fn scan(roots: Vec<Root>) -> Vec<(String, Vec<String>)> {
    fn walk(dir: &Path, depth: u32, excludes: &[String], found: &mut Vec<String>) {
        if dir.join(".git").exists() {
            found.push(dir.display().to_string());
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
    roots
        .into_iter()
        .map(|(path, depth, excludes)| {
            let mut found = Vec::new();
            walk(&expand(&path), depth, &excludes, &mut found);
            found.sort_by_key(|p| p.to_lowercase());
            (path, found)
        })
        .collect()
}

/// 行の状態を読む（4 つずつ並べて。GTK4 版と同じ数）。
pub fn peek(locations: Vec<String>) -> Vec<(String, Peek)> {
    let mut out = Vec::new();
    for chunk in locations.chunks(4) {
        std::thread::scope(|s| {
            let handles: Vec<_> = chunk
                .iter()
                .map(|l| {
                    s.spawn(move || {
                        amonhen_backend_cli::Git::new(expand(l))
                            .peek()
                            .ok()
                            .map(|p| (l.clone(), p))
                    })
                })
                .collect();
            out.extend(handles.into_iter().filter_map(|h| h.join().ok().flatten()));
        });
    }
    out
}

// Quick Open（Ctrl+Shift+O。GTK4 版の `quickopen.rs`、macOS 版の `QuickOpen.swift`）

/// Quick Open の候補（場所と、比べるための小文字の名前・場所）。並びはすべての一覧と同じ（お気に入り・
/// ブックマーク・最近開いたもの・スキャンで見つけたもの）。
#[derive(Clone)]
pub struct Candidate {
    pub location: String,
    pub name: String,
    lower_name: String,
    lower: String,
}

/// 候補を読む（`scanned` はスキャンで見つけたもの。まだなければ空）。
pub fn candidates(scanned: Vec<(String, Vec<String>)>) -> Vec<Candidate> {
    let mut state = State::new().0;
    state.scanned = Some(scanned);
    state
        .all()
        .into_iter()
        .filter(|l| !is_remote(l))
        .map(|location| {
            let name = state.name(&location);
            Candidate {
                lower_name: name.to_lowercase(),
                lower: location.to_lowercase(),
                name,
                location,
            }
        })
        .collect()
}

/// スキャンするフォルダ（Quick Open が候補を足すため）。
pub fn scan_roots() -> Vec<Root> {
    roots(&read())
}

/// 問い合わせに合う候補を、合い方の良い順に（名前で合えば高く、パスでしか合わなければ低く。GTK4 版と同じ）。
pub fn matches<'a>(candidates: &'a [Candidate], query: &str) -> Vec<&'a Candidate> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return candidates.iter().collect();
    }
    let mut scored: Vec<(&Candidate, i64)> = candidates
        .iter()
        .filter_map(|c| {
            if let Some(s) = crate::branchswitcher::score(&query, &c.lower_name) {
                return Some((c, s + 1000));
            }
            crate::branchswitcher::score(&query, &c.lower).map(|s| (c, s))
        })
        .collect();
    // 同じ点なら元の並び。
    scored.sort_by_key(|(_, s)| -s);
    scored.into_iter().map(|(c, _)| c).collect()
}

/// Quick Open の行の 2 行目（ホームの下は `~`）。
pub fn shown_path(location: &str) -> String {
    tilde(location)
}

/// ブックマークに入れる（clone したもの。macOS 版・GTK4 版と同じ）。
pub fn add_bookmark(path: &str) {
    let _ = edit(|d| update(d, path, |_| {}));
}

/// 窓に開いていないリポジトリを開く入口の、置き場所の初期値（前に clone した場所、なければスキャンするフォルダの
/// 最初、なければホーム。GTK4 版の `repos::clone` と同じ）。
pub fn clone_folder() -> String {
    crate::prefs::string(CLONE_FOLDER)
        .filter(|f| !f.is_empty())
        .or_else(|| roots(&read()).into_iter().map(|r| r.0).next())
        .or_else(|| std::env::var("USERPROFILE").ok())
        .unwrap_or_default()
}

/// clone した親のフォルダを覚える（次の Clone の初期値）。
pub fn remember_clone_folder(folder: &str) {
    let _ = crate::prefs::set_string(CLONE_FOLDER, folder);
}

/// 前に clone した場所（GTK4 版と同じ鍵）。
const CLONE_FOLDER: &str = "cloneFolder";

/// 欄に打ったパス（`~` はホーム）。
pub fn path_of(text: &str) -> PathBuf {
    expand(text.trim())
}

/// Clone の URL の欄が bundle のファイルなら、その見出し（GTK4 版の Clone のシートと同じ）。
pub fn clone_bundle(url: &str) -> Option<amonhen_model::BundleInfo> {
    if url.trim().is_empty() {
        return None;
    }
    let path = path_of(url);
    path.is_file()
        .then(|| amonhen_backend_cli::bundle_info(&path))
        .flatten()
}

/// HEAD のない bundle から取り出せるブランチ（短い名前。clone できないときは空）。
pub fn bundle_branches(info: &amonhen_model::BundleInfo) -> Vec<String> {
    if info.head || info.prerequisites > 0 {
        return Vec::new();
    }
    info.refs
        .iter()
        .map(|r| {
            r.strip_prefix("refs/heads/")
                .or_else(|| r.strip_prefix("refs/tags/"))
                .unwrap_or(r)
                .to_owned()
        })
        .collect()
}

/// URL の最後の部分（`.git`・`.bundle` を除く。GTK4 版の `name_from_url`）。
pub fn name_from_url(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches(['/', '\\']);
    let last = trimmed.rsplit(['/', ':', '\\']).next().unwrap_or_default();
    let last = last.strip_suffix(".git").unwrap_or(last);
    last.strip_suffix(".bundle").unwrap_or(last).to_owned()
}

/// 開いていないリポジトリを fetch する（GTK4 版の `bookmarks::fetch` と同じく、daemon を通さずにその場で開く）。
/// 失敗なら理由。
pub fn fetch(location: &str) -> Option<String> {
    let backend = match amonhen_backend_gix::GixBackend::open(expand(location)) {
        Ok(b) => b,
        Err(e) => return Some(e.to_string()),
    };
    let repo = amonhen_core::Repository::new(backend);
    let reply: Value =
        serde_json::from_str(&repo.run_json(r#"{"op":"fetch"}"#)).unwrap_or_default();
    reply["error"].as_str().map(str::to_owned)
}

/// リポジトリを新しい窓（別のプロセス）で開く。
pub fn open_window(location: &str) -> Result<(), String> {
    if !is_repository(location) {
        return Err(tr!("Not found — {path}", path = location));
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    amonhen_model::process::command(exe)
        .arg(expand(location))
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

// 画面の状態

#[derive(Clone, PartialEq)]
enum Category {
    All,
    Favorites,
    Local,
    Root(String),
    Group(String),
}

pub struct State {
    data: Value,
    /// スキャンするフォルダごとの見つけたもの（探している間は None）。
    scanned: Option<Vec<(String, Vec<String>)>>,
    peeks: HashMap<String, Peek>,
    opened: HashMap<String, i64>,
    recent: Vec<String>,
    category: usize,
    filter: String,
    /// 選んでいるリポジトリの場所。
    selected: Option<String>,
    /// 知らせ（書けなかったなど）。
    note: String,
    /// 並べる列（`name`・`branch`・`status`・`commit`・`opened`）と、上る向きか。
    sort: String,
    ascending: bool,
    /// fetch している・失敗したリポジトリ（[`key`] の形）。
    fetching: HashSet<String>,
    fetch_errors: HashMap<String, String>,
}

/// ページの操作。
pub enum Event {
    Category(Option<usize>),
    Selected(Option<usize>),
    Filter(String),
    /// 行・分類の右クリックのメニュー。
    Menu(String),
    /// 選んでいるリポジトリを開く（ダブルクリック・Enter）。
    Open,
    AddRepository,
    AddScanFolder,
    Close,
    Scanned(Vec<(String, Vec<String>)>),
    Peeked(Vec<(String, Peek)>),
    /// fetch が終わった（場所と、失敗なら理由）。
    Fetched(String, Option<String>),
    /// 列の見出しを押した。
    Sort(&'static str),
}

/// ページの外でする続き。
pub enum Action {
    None,
    Close,
    /// スキャンし直す（フォルダが変わった）。
    Scan(Vec<Root>),
    /// 行の状態を読む。
    Peek(Vec<String>),
    /// グループの名前を聞いて、リポジトリをそこに移す（None なら名前だけ足す）。
    NewGroup(Option<String>),
    /// 表示名を聞く（場所と、今の名前）。
    Rename(String, String),
    /// 裏で fetch する。
    Fetch(String),
}

impl State {
    pub fn new() -> (Self, Vec<Root>) {
        let data = read();
        let roots = roots(&data);
        (
            Self {
                data,
                scanned: None,
                peeks: HashMap::new(),
                opened: opened(),
                recent: recent(),
                category: 0,
                filter: String::new(),
                selected: None,
                note: String::new(),
                fetching: HashSet::new(),
                fetch_errors: HashMap::new(),
                sort: crate::prefs::string(SORT_KEY).unwrap_or_else(|| "opened".to_owned()),
                ascending: crate::prefs::bool(SORT_ASCENDING_KEY),
            },
            roots,
        )
    }

    fn groups(&self) -> Vec<String> {
        let mut groups = strings(&self.data["groups"]);
        for b in items(&self.data) {
            if let Some(g) = b["group"].as_str()
                && !groups.iter().any(|x| x == g)
            {
                groups.push(g.to_owned());
            }
        }
        groups
    }

    fn scanned_all(&self) -> impl Iterator<Item = &String> {
        self.scanned.iter().flatten().flat_map(|(_, found)| found)
    }

    /// すべて（お気に入り・ブックマーク・最近開いたもの・スキャンで見つけたもの。同じものは 1 つ）。
    fn all(&self) -> Vec<String> {
        let mut seen = HashSet::new();
        let favorites = items(&self.data)
            .filter(|b| b["favorite"].as_bool() == Some(true))
            .filter_map(|b| b["path"].as_str().map(str::to_owned));
        let bookmarks = items(&self.data).filter_map(|b| b["path"].as_str().map(str::to_owned));
        favorites
            .chain(bookmarks)
            .chain(self.recent.iter().cloned())
            .chain(self.scanned_all().cloned())
            .filter(|p| seen.insert(key(p)))
            .collect()
    }

    /// 左の分類（分類・名前・見出しか・字下げするか）。
    fn categories(&self) -> Vec<(Category, String, Option<String>, bool)> {
        let mut out = vec![
            (Category::All, tr!("All"), None, false),
            (Category::Favorites, tr!("Favorites"), None, false),
            (
                Category::Local,
                tr!("This Computer"),
                Some(tr!("Places")),
                false,
            ),
        ];
        for (path, ..) in roots(&self.data) {
            out.push((Category::Root(path.clone()), tilde(&path), None, true));
        }
        for (i, g) in self.groups().into_iter().enumerate() {
            let head = (i == 0).then(|| tr!("Groups"));
            out.push((Category::Group(g.clone()), g, head, false));
        }
        out
    }

    fn locations(&self, category: &Category) -> Vec<String> {
        match category {
            Category::All => self.all(),
            Category::Favorites => items(&self.data)
                .filter(|b| b["favorite"].as_bool() == Some(true))
                .filter_map(|b| b["path"].as_str().map(str::to_owned))
                .collect(),
            Category::Local => self.all().into_iter().filter(|p| !is_remote(p)).collect(),
            Category::Root(root) => self
                .scanned
                .iter()
                .flatten()
                .filter(|(r, _)| r == root)
                .flat_map(|(_, found)| found.clone())
                .collect(),
            Category::Group(g) => items(&self.data)
                .filter(|b| b["group"].as_str() == Some(g))
                .filter_map(|b| b["path"].as_str().map(str::to_owned))
                .collect(),
        }
    }

    fn category(&self) -> Category {
        self.categories()
            .into_iter()
            .nth(self.category)
            .map_or(Category::All, |c| c.0)
    }

    /// 右に出すリポジトリ（絞り込んだもの）。
    fn shown(&self) -> Vec<String> {
        let filter = self.filter.trim().to_lowercase();
        let mut shown: Vec<(String, String)> = self
            .locations(&self.category())
            .into_iter()
            .map(|p| (self.name(&p).to_lowercase(), p))
            .filter(|(name, p)| {
                filter.is_empty() || name.contains(&filter) || p.to_lowercase().contains(&filter)
            })
            .collect();
        // 選んだ列で並べる（同じ値なら名前の昇順。GTK4 版の `sorter` と同じ）。
        shown.sort_by(|(an, a), (bn, b)| {
            let order = self.sort_value(a, an).cmp(&self.sort_value(b, bn));
            let order = if self.ascending {
                order
            } else {
                order.reverse()
            };
            order.then_with(|| an.cmp(bn))
        });
        shown.into_iter().map(|(_, p)| p).collect()
    }

    /// 列で並べる値（文字の列は小文字の文字、ほかは数）。
    fn sort_value(&self, location: &str, name: &str) -> (i64, String) {
        let peek = self.peeks.get(&key(location));
        match self.sort.as_str() {
            "name" => (0, name.to_owned()),
            "branch" => (
                0,
                peek.and_then(|p| p.branch.clone())
                    .unwrap_or_default()
                    .to_lowercase(),
            ),
            "status" => (
                peek.map_or(-1, |p| {
                    p.changed as i64 * 10_000 + p.ahead as i64 + p.behind as i64
                }),
                String::new(),
            ),
            "commit" => (
                peek.and_then(|p| p.last_commit.as_ref())
                    .map_or(i64::MIN, |c| c.0),
                String::new(),
            ),
            _ => (
                self.opened.get(&key(location)).copied().unwrap_or(i64::MIN),
                String::new(),
            ),
        }
    }

    /// 見出しを押した: 同じ列なら向きを替え、ほかの列ならその列で（日時・状態は新しい順・多い順から）。
    fn sort_by(&mut self, column: &str) {
        if self.sort == column {
            self.ascending = !self.ascending;
        } else {
            self.sort = column.to_owned();
            self.ascending = !matches!(column, "status" | "commit" | "opened");
        }
        let _ = crate::prefs::set_string(SORT_KEY, &self.sort);
        let _ = crate::prefs::set_bool(SORT_ASCENDING_KEY, self.ascending);
    }

    /// 表示名（上書きがなければフォルダの名前）。
    fn name(&self, location: &str) -> String {
        if let Some(name) = item(&self.data, location)
            .and_then(|b| b["name"].as_str())
            .filter(|n| !n.is_empty())
        {
            return name.to_owned();
        }
        location
            .trim_end_matches(['\\', '/'])
            .rsplit(['\\', '/'])
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or(location)
            .to_owned()
    }

    /// 状態をまだ読んでいない手元のリポジトリ。
    fn unpeeked(&self) -> Vec<String> {
        self.all()
            .into_iter()
            .filter(|p| !is_remote(p) && is_repository(p) && !self.peeks.contains_key(&key(p)))
            .collect()
    }

    /// ブックマークを変えて、読み直す。
    fn change(&mut self, change: impl FnOnce(&mut Value)) {
        match edit(change) {
            Ok(data) => self.data = data,
            Err(e) => self.note = e,
        }
    }

    /// 一覧に出す名前を変える（空ならフォルダの名前に戻す）。
    pub fn rename(&mut self, location: &str, name: &str) {
        let name = name.trim();
        let folder = {
            let mut copy = self.data.clone();
            if let Some(b) = items_mut_find(&mut copy, location) {
                b.remove("name");
            }
            let data = std::mem::replace(&mut self.data, copy);
            let folder = self.name(location);
            self.data = data;
            folder
        };
        self.change(|d| {
            update(d, location, |b| {
                if name.is_empty() || name == folder {
                    b.remove("name");
                } else {
                    b.insert("name".into(), json!(name));
                }
            })
        });
    }

    /// グループを足し、`location` があればそこに移す。
    pub fn add_group(&mut self, name: &str, location: Option<&str>) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        self.change(|d| {
            let groups = array_mut(d, "groups");
            if !groups.iter().any(|g| g.as_str() == Some(name)) {
                groups.push(json!(name));
            }
            if let Some(location) = location {
                update(d, location, |b| {
                    b.insert("group".into(), json!(name));
                });
            }
        });
    }

    pub fn handle(&mut self, event: Event) -> Action {
        match event {
            Event::Category(Some(i)) => {
                self.category = i;
                self.selected = None;
            }
            Event::Category(None) => {}
            Event::Selected(i) => {
                self.selected = i.and_then(|i| self.shown().into_iter().nth(i));
            }
            Event::Filter(text) => self.filter = text,
            Event::Sort(column) => self.sort_by(column),
            Event::Open => {
                if let Some(location) = self.selected.clone() {
                    self.open(&location);
                }
            }
            Event::AddRepository => {
                let Some(path) = crate::files::pick_folder(&tr!("Add Repositories"), &tr!("Add"))
                else {
                    return Action::None;
                };
                let path = path.display().to_string();
                if !is_repository(&path) {
                    self.note = tr!("Not found — {path}", path = path);
                    return Action::None;
                }
                self.change(|d| update(d, &path, |_| {}));
                self.selected = Some(path);
                return Action::Peek(self.unpeeked());
            }
            Event::AddScanFolder => {
                let Some(path) =
                    crate::files::pick_folder(&tr!("Add a Folder to Scan"), &tr!("Add"))
                else {
                    return Action::None;
                };
                let path = path.display().to_string();
                self.change(|d| {
                    let roots = array_mut(d, "scanRoots");
                    if !roots
                        .iter()
                        .any(|r| r["path"].as_str().is_some_and(|p| same(p, &path)))
                    {
                        roots.push(json!({
                            "path": path,
                            "depth": 3,
                            "excludes": ["node_modules", "target", "build", ".build", "vendor",
                                "Pods", "DerivedData", "dist"],
                        }));
                    }
                });
                self.scanned = None;
                return Action::Scan(roots(&self.data));
            }
            Event::Close => return Action::Close,
            Event::Scanned(found) => {
                self.scanned = Some(found);
                return Action::Peek(self.unpeeked());
            }
            Event::Fetched(location, error) => {
                self.fetching.remove(&key(&location));
                match error {
                    Some(e) => {
                        self.fetch_errors.insert(key(&location), e);
                    }
                    None => {
                        self.fetch_errors.remove(&key(&location));
                    }
                }
                // 上流との差が変わる。
                return Action::Peek(vec![location]);
            }
            Event::Peeked(peeks) => {
                self.peeks
                    .extend(peeks.into_iter().map(|(l, p)| (key(&l), p)));
            }
            Event::Menu(id) => return self.menu(&id),
        }
        Action::None
    }

    fn open(&mut self, location: &str) {
        match open_window(location) {
            Ok(_) => {
                self.note.clear();
                self.opened.insert(key(location), now());
            }
            Err(e) => self.note = e,
        }
    }

    fn menu(&mut self, id: &str) -> Action {
        let Some((what, rest)) = id.split_once(':') else {
            return Action::None;
        };
        // `what:<場所>` か `what:<値>|<場所>`。
        let (value, location) = rest.split_once('|').unwrap_or(("", rest));
        let location = location.to_owned();
        match what {
            "open" => self.open(&location),
            "favorite" | "unfavorite" => {
                let yes = what == "favorite";
                self.change(|d| {
                    update(d, &location, |b| {
                        b.insert("favorite".into(), json!(yes));
                    })
                });
            }
            "group" => self.change(|d| {
                update(d, &location, |b| {
                    if value.is_empty() {
                        b.remove("group");
                    } else {
                        b.insert("group".into(), json!(value));
                    }
                })
            }),
            "new-group" => return Action::NewGroup(Some(location)),
            "rename" => return Action::Rename(location.clone(), self.name(&location)),
            "fetch" => {
                if is_remote(&location) || !is_repository(&location) {
                    return Action::None;
                }
                if self.fetching.insert(key(&location)) {
                    self.fetch_errors.remove(&key(&location));
                    return Action::Fetch(location);
                }
            }
            "color" => self.change(|d| {
                update(d, &location, |b| {
                    if value.is_empty() {
                        b.remove("color");
                    } else {
                        b.insert("color".into(), json!(value));
                    }
                })
            }),
            "reveal" => crate::files::reveal(&expand(&location)),
            "terminal" => {
                crate::files::open_terminal(&expand(&location));
            }
            "editor" => {
                if let Err(e) = crate::editor::open_folder(&expand(&location)) {
                    self.note = e;
                }
            }
            "copy" => crate::app::copy_text(&location),
            "remove" => {
                self.change(|d| {
                    array_mut(d, "items")
                        .retain(|b| !b["path"].as_str().is_some_and(|p| same(p, &location)))
                });
                // 最近開いたものからも外す（すべての一覧に残らないように）。
                self.recent.retain(|p| !same(p, &location));
                let _ = crate::prefs::set_string(RECENT, &json!(self.recent).to_string());
                if self.selected.as_deref().is_some_and(|s| same(s, &location)) {
                    self.selected = None;
                }
            }
            "remove-root" => {
                self.change(|d| {
                    array_mut(d, "scanRoots")
                        .retain(|r| !r["path"].as_str().is_some_and(|p| same(p, &location)))
                });
                self.category = 0;
                self.scanned = None;
                return Action::Scan(roots(&self.data));
            }
            "rescan" => {
                self.scanned = None;
                return Action::Scan(roots(&self.data));
            }
            "delete-group" => {
                self.change(|d| {
                    array_mut(d, "groups").retain(|g| g.as_str() != Some(&location));
                    for b in array_mut(d, "items") {
                        if b["group"].as_str() == Some(&location)
                            && let Some(b) = b.as_object_mut()
                        {
                            b.remove("group");
                        }
                    }
                });
                self.category = 0;
            }
            "add-group" => return Action::NewGroup(None),
            _ => {}
        }
        Action::None
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

// 画面

pub struct Callbacks<'a> {
    pub category: Callback<Option<usize>>,
    pub selected: Callback<Option<usize>>,
    pub filter: Callback<String>,
    pub menu: Box<dyn Fn() -> Callback<String> + 'a>,
    pub button: Box<dyn Fn(&'static str) -> Callback<()> + 'a>,
    /// リポジトリの一覧を包む Grid（ダブルクリック・Enter を付ける）。
    pub list_host: &'a ElementRef<Grid>,
}

/// リポジトリの行の右クリックのメニュー（GTK4 版の `repobrowser::row_menu` の手元の分）。
fn row_menu(state: &State, location: &str) -> Vec<MenuItem> {
    let bookmark = item(&state.data, location);
    let favorite = bookmark.is_some_and(|b| b["favorite"].as_bool() == Some(true));
    let group = bookmark.and_then(|b| b["group"].as_str());
    let color = bookmark.and_then(|b| b["color"].as_str());
    let mark = |on: bool, label: String| if on { format!("✓ {label}") } else { label };
    let mut groups = vec![MenuItem::item(
        format!("group:|{location}"),
        mark(group.is_none(), tr!("None")),
    )];
    for g in state.groups() {
        groups.push(MenuItem::item(
            format!("group:{g}|{location}"),
            mark(group == Some(g.as_str()), g.clone()),
        ));
    }
    groups.push(MenuItem::separator("-new"));
    groups.push(MenuItem::item(
        format!("new-group:{location}"),
        tr!("New Group…"),
    ));
    let mut colors = vec![MenuItem::item(
        format!("color:|{location}"),
        mark(color.is_none(), tr!("None")),
    )];
    for (name, label, _) in COLORS {
        colors.push(MenuItem::item(
            format!("color:{name}|{location}"),
            mark(
                color == Some(*name),
                amonhen_model::i18n::translate(label).into_owned(),
            ),
        ));
    }
    vec![
        MenuItem::item(format!("open:{location}"), tr!("Open in New Window")),
        MenuItem::separator("-fetch"),
        MenuItem::item(format!("fetch:{location}"), "Fetch"),
        MenuItem::separator("-favorite"),
        if favorite {
            MenuItem::item(
                format!("unfavorite:{location}"),
                tr!("Remove from Favorites"),
            )
        } else {
            MenuItem::item(format!("favorite:{location}"), tr!("Add to Favorites"))
        },
        MenuItem::submenu("groups", tr!("Group"), groups),
        MenuItem::submenu("colors", tr!("Color Label"), colors),
        MenuItem::item(format!("rename:{location}"), tr!("Rename…")),
        MenuItem::separator("-files"),
        MenuItem::item(format!("reveal:{location}"), tr!("Show in Files")),
        MenuItem::item(format!("terminal:{location}"), tr!("Open in Terminal")),
        MenuItem::item(format!("editor:{location}"), tr!("Open in Editor")),
        MenuItem::item(format!("copy:{location}"), tr!("Copy Path")),
        MenuItem::separator("-remove"),
        MenuItem::item(format!("remove:{location}"), tr!("Remove from List")),
    ]
}

/// 分類の右クリックのメニュー。
fn category_menu(category: &Category) -> Vec<MenuItem> {
    let add = [
        MenuItem::item("add-group:", tr!("New Group…")),
        MenuItem::separator("-add"),
    ];
    match category {
        Category::Root(path) => vec![
            MenuItem::item("rescan:", tr!("Search Again")),
            MenuItem::item(format!("reveal:{path}"), tr!("Show in Files")),
            MenuItem::separator("-remove"),
            MenuItem::item(format!("remove-root:{path}"), tr!("Stop Scanning")),
        ],
        Category::Group(name) => vec![
            add[0].clone(),
            MenuItem::separator("-delete"),
            MenuItem::item(format!("delete-group:{name}"), tr!("Delete Group")),
        ],
        _ => vec![add[0].clone()],
    }
}

/// 状態の列（変更の数・上流との差。なければ ✓）とツールチップ。
fn status(
    location: &str,
    peek: Option<&Peek>,
    fetching: bool,
    fetch_error: Option<&str>,
) -> (String, Option<String>) {
    if is_remote(location) {
        return (
            "—".into(),
            Some(tr!("The status isn’t read until you open it")),
        );
    }
    if !is_repository(location) {
        return (tr!("Not found"), None);
    }
    // fetch の失敗・途中を先に（GTK4 版の `state_text` と同じ）。
    let mut parts = Vec::new();
    if fetch_error.is_some() {
        parts.push(tr!("Fetch failed"));
    }
    if fetching {
        parts.push(tr!("Fetching…"));
    }
    let Some(p) = peek else {
        return (parts.join("  "), None);
    };
    let before = parts.len();
    if p.changed > 0 {
        parts.push(tr!("Changes {count}", count = p.changed));
    }
    if p.ahead > 0 {
        parts.push(format!("↑{}", p.ahead));
    }
    if p.behind > 0 {
        parts.push(format!("↓{}", p.behind));
    }
    if parts.len() == before {
        parts.push("✓".into());
    }
    if let Some(e) = fetch_error {
        return (
            parts.join("  "),
            Some(tr!("Fetch failed: {error}", error = e)),
        );
    }
    let tip = [
        if p.changed > 0 {
            tr!("{count} files with changes", count = p.changed)
        } else {
            tr!("No changes")
        },
        if p.upstream {
            tr!(
                "{ahead} ahead of the upstream, {behind} behind",
                ahead = p.ahead,
                behind = p.behind
            )
        } else {
            tr!("No upstream")
        },
    ]
    .join("\n");
    (parts.join("  "), Some(tip))
}

const COLUMNS: [f64; 4] = [150.0, 110.0, 280.0, 110.0];

fn columns() -> Vec<GridLength> {
    let mut c = vec![GridLength::Star(1.0)];
    c.extend(COLUMNS.map(GridLength::Pixel));
    c
}

pub fn view(state: &State, callbacks: Callbacks) -> View {
    let head = Grid::new()
        .columns([
            GridLength::Auto,
            GridLength::Star(1.0),
            GridLength::Auto,
            GridLength::Auto,
            GridLength::Auto,
        ])
        .column_spacing(6.0)
        .margin(Thickness::new(10.0, 6.0, 8.0, 6.0))
        .grid_row(0)
        .grid_column_span(2)
        .children((
            TextBlock::new()
                .text("Repositories")
                .font_size(14.0)
                .vertical_alignment(VerticalAlignment::Center)
                .grid_column(0),
            TextBox::new()
                .text(state.filter.clone())
                .placeholder_text(tr!("Filter by name or path"))
                .on_text_changed(callbacks.filter.clone())
                .width(260.0)
                .horizontal_alignment(HorizontalAlignment::Right)
                .grid_column(1),
            Button::new()
                .on_click((callbacks.button)("add-repository"))
                .grid_column(2)
                .content(TextBlock::new().text(tr!("Add Local Repository…"))),
            Button::new()
                .on_click((callbacks.button)("add-scan-folder"))
                .grid_column(3)
                .content(TextBlock::new().text(tr!("Add Folder to Scan…"))),
            Button::new()
                .on_click((callbacks.button)("close"))
                .grid_column(4)
                .content(TextBlock::new().text(tr!("Close"))),
        ));
    // 左: 分類と件数。
    let categories = state.categories();
    let side_rows = categories
        .iter()
        .enumerate()
        .map(|(i, (c, name, head, indent))| {
            let count = state.locations(c).len();
            let line = Grid::new()
                .columns([GridLength::Star(1.0), GridLength::Auto])
                .margin(Thickness::new(
                    if *indent { 16.0 } else { 0.0 },
                    0.0,
                    0.0,
                    0.0,
                ))
                .children((
                    TextBlock::new()
                        .text(name.clone())
                        .text_trimming(TextTrimming::CharacterEllipsis)
                        .vertical_alignment(VerticalAlignment::Center)
                        .grid_column(0),
                    TextBlock::new()
                        .text(count.to_string())
                        .opacity(0.5)
                        .font_size(12.0)
                        .vertical_alignment(VerticalAlignment::Center)
                        .grid_column(1),
                ));
            // 見出し（場所・グループ）は、その分類の行の上に添える。
            let content: View = match head {
                Some(head) => StackPanel::new().children((
                    TextBlock::new()
                        .text(head.clone())
                        .font_size(11.0)
                        .opacity(0.55)
                        .margin(Thickness::new(0.0, 8.0, 0.0, 2.0)),
                    line,
                )),
                None => line,
            };
            let content = content.menu(Menu::new(category_menu(c), (callbacks.menu)()));
            (
                format!("{i}:{name}"),
                ListViewItem::new().tag(name.clone()).content(content),
            )
        });
    let side_key: String = categories.iter().map(|c| c.1.as_str()).collect();
    let side = ListView::new()
        .selected_index(Some(state.category))
        .on_selection_changed(callbacks.category.clone())
        .collection_slot(ListViewSlot::Items, side_rows);
    let side = Border::new()
        .grid_column(0)
        .grid_row(1)
        .content(View::keyed_fragment([(side_key, side)]));
    // 右: リポジトリの一覧。
    let shown = state.shown();
    // 見出しを押すとその列で並べる（並べている列に ▲・▼）。
    let header_cell = |text: String, id: &'static str, column: i32| -> View {
        let text = match (state.sort == id, state.ascending) {
            (true, true) => format!("{text} ▲"),
            (true, false) => format!("{text} ▼"),
            _ => text,
        };
        Button::new()
            .style(ButtonStyle::Subtle)
            .horizontal_alignment(HorizontalAlignment::Stretch)
            .horizontal_content_alignment(HorizontalAlignment::Left)
            .on_click((callbacks.button)(match id {
                "name" => "sort:name",
                "branch" => "sort:branch",
                "status" => "sort:status",
                "commit" => "sort:commit",
                _ => "sort:opened",
            }))
            .grid_column(column)
            .content(TextBlock::new().text(text).font_size(11.0).opacity(0.6))
    };
    let header = Grid::new()
        .columns(columns())
        .margin(Thickness::new(16.0, 0.0, 16.0, 4.0))
        .grid_row(0)
        .children((
            header_cell(tr!("Name"), "name", 0),
            header_cell(tr!("Branch"), "branch", 1),
            header_cell(tr!("Status"), "status", 2),
            header_cell(tr!("Last Commit"), "commit", 3),
            header_cell(tr!("Opened"), "opened", 4),
        ));
    let rows = shown.iter().map(|location| {
        let peek = state.peeks.get(&key(location));
        let bookmark = item(&state.data, location);
        let favorite = bookmark.is_some_and(|b| b["favorite"].as_bool() == Some(true));
        let color = bookmark
            .and_then(|b| b["color"].as_str())
            .and_then(|c| COLORS.iter().find(|x| x.0 == c))
            .map(|x| x.2);
        let mut name = state.name(location);
        if favorite {
            name = format!("★ {name}");
        }
        let dot: View = match color {
            Some(rgb) => Border::new()
                .width(8.0)
                .height(8.0)
                .corner_radius(4.0)
                .background(Brush::Solid(Color::rgb(
                    (rgb >> 16) as u8,
                    (rgb >> 8) as u8,
                    rgb as u8,
                )))
                .margin(Thickness::new(0.0, 0.0, 6.0, 0.0))
                .vertical_alignment(VerticalAlignment::Center)
                .into(),
            None => View::empty(),
        };
        let name_cell = StackPanel::new()
            .orientation(Orientation::Horizontal)
            .grid_column(0)
            .children((
                dot,
                StackPanel::new().spacing(1.0).children((
                    TextBlock::new()
                        .text(name)
                        .text_trimming(TextTrimming::CharacterEllipsis),
                    TextBlock::new()
                        .text(tilde(location))
                        .font_size(11.0)
                        .opacity(0.6)
                        .text_trimming(TextTrimming::CharacterEllipsis),
                )),
            ));
        let (status, tip) = status(
            location,
            peek,
            state.fetching.contains(&key(location)),
            state.fetch_errors.get(&key(location)).map(String::as_str),
        );
        let cell = |text: String, column: i32| {
            TextBlock::new()
                .text(text)
                .font_size(12.0)
                .text_trimming(TextTrimming::CharacterEllipsis)
                .vertical_alignment(VerticalAlignment::Center)
                .margin(Thickness::new(4.0, 0.0, 4.0, 0.0))
                .grid_column(column)
        };
        let branch = peek.map_or(String::new(), |p| {
            p.branch.clone().unwrap_or_else(|| "HEAD".into())
        });
        let commit = peek
            .and_then(|p| p.last_commit.as_ref())
            .map_or(String::new(), |(time, subject)| {
                format!("{} — {subject}", crate::render::local_date(*time))
            });
        let opened = state
            .opened
            .get(&key(location))
            .map_or("—".to_owned(), |t| crate::render::local_date(*t));
        let status_cell = cell(status, 2);
        let status_cell: View = match tip {
            Some(tip) => status_cell.tooltip(tip),
            None => status_cell.into(),
        };
        let row = Grid::new()
            .columns(columns())
            .margin(Thickness::new(0.0, 3.0, 0.0, 3.0))
            .children((
                name_cell,
                cell(branch, 1),
                status_cell,
                cell(commit, 3),
                cell(opened, 4).opacity(0.7),
            ))
            .menu(Menu::new(row_menu(state, location), (callbacks.menu)()));
        (
            key(location),
            ListViewItem::new().tag(location.clone()).content(row),
        )
    });
    let index = state
        .selected
        .as_deref()
        .and_then(|s| shown.iter().position(|l| same(l, s)));
    let list = ListView::new()
        .selected_index(index)
        .on_selection_changed(callbacks.selected)
        .collection_slot(ListViewSlot::Items, rows);
    // 並びが変わるたびに一覧ごと作り直す（reactor の ListView は消した項目の場所が空いたまま残る）。
    let list_key: String = shown.iter().map(|l| key(l) + "\n").collect();
    let note = if state.scanned.is_none() && state.note.is_empty() {
        tr!("Loading…")
    } else {
        state.note.clone()
    };
    let main = Grid::new()
        .rows([GridLength::Auto, GridLength::Star(1.0), GridLength::Auto])
        .grid_column(1)
        .grid_row(1)
        .children((
            header,
            Grid::new()
                .element_ref(callbacks.list_host)
                .background(Brush::Solid(Color::transparent()))
                .grid_row(1)
                .children((View::keyed_fragment([(list_key, list)]),)),
            TextBlock::new()
                .text(note)
                .opacity(0.6)
                .margin(Thickness::new(16.0, 4.0, 8.0, 6.0))
                .grid_row(2),
        ));
    Grid::new()
        .rows([GridLength::Auto, GridLength::Star(1.0)])
        .columns([GridLength::Pixel(240.0), GridLength::Star(1.0)])
        .children((head, side, main))
}
