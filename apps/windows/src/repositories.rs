//! Repositories（Window ▸ Repositories、Ctrl+Shift+R。GTK4 版の `repobrowser.rs`・`bookmarks.rs`、macOS 版の
//! `RepositoryBrowser.swift`）: ブックマーク・最近開いたもの・スキャンで見つけたものを見渡して開くページ。左に分類
//! （すべて・お気に入り・このコンピューターとスキャンするフォルダ・グループ）と件数、右にその中のリポジトリを列
//! （名前とパス・ブランチ・状態・最後のコミット・開いた日時）で並べる。
//!
//! ブックマークは macOS 版・GTK4 版と同じ形の `bookmarks.json`（設定と同じフォルダ）。知らない鍵（自動 fetch・
//! 窓の背景など）は読んだ JSON のまま書き戻す。窓にタブがないので、開くと新しい窓（別のプロセス）で開く。
//!
//! まだないもの: 列の並べ替え・幅、表示名の変更、SSH・daemon の場所、スキャンの深さ。

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

/// `path` のリポジトリだけの窓の背景（ブックマークの `backdrop`。macOS 版・GTK4 版と同じ形）。
pub fn backdrop(path: &str) -> Option<Value> {
    item(&read(), path)
        .map(|b| b["backdrop"].clone())
        .filter(Value::is_object)
}

/// `path` のリポジトリだけの窓の背景を書く（None なら消して、アプリの設定に従う）。
pub fn set_backdrop(path: &str, backdrop: Option<Value>) -> Result<(), String> {
    edit(|d| {
        update(d, path, |b| match backdrop {
            Some(v) => {
                b.insert("backdrop".into(), v);
            }
            None => {
                b.remove("backdrop");
            }
        })
    })
    .map(|_| ())
}

/// `bookmarks.json` を最後に書いた時刻（読んだものを使い回してよいか）。
pub fn modified() -> Option<std::time::SystemTime> {
    file()
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.modified().ok())
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

pub fn is_remote(location: &str) -> bool {
    amonhen_remote::location::is_remote(location)
}

pub fn is_repository(location: &str) -> bool {
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
    // 開いたものをブックマークに入れるか（Settings > General）。開いた日時と最近のものは、いつも覚える。
    if crate::prefs::bool(crate::prefs::AUTO_BOOKMARK)
        && !scanned
        && item(&data, &location).is_none()
    {
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

/// SSH 先でスキャンするフォルダ（`ssh://host/path`・深さ・入らない名前）。
fn remote_roots(data: &Value) -> Vec<Root> {
    data["scanRoots"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let path = r["path"].as_str()?.to_owned();
            let depth = r["depth"].as_u64().unwrap_or(3) as u32;
            Some((path, depth, strings(&r["excludes"])))
        })
        .filter(|(path, ..)| is_remote(path))
        .collect()
}

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
    scan_with_dirs(roots).0
}

/// [`scan`] と、中を読んだフォルダ（その中でリポジトリが増えたら見つかるフォルダ。見張る先）と、見つけた
/// リポジトリ（`.git` ができた・消えたを知る。GTK4 版の `scan_local_with_dirs`）。
pub fn scan_with_dirs(roots: Vec<Root>) -> (Vec<(String, Vec<String>)>, Vec<PathBuf>) {
    fn walk(
        dir: &Path,
        depth: u32,
        excludes: &[String],
        found: &mut Vec<String>,
        dirs: &mut Vec<PathBuf>,
    ) {
        if dir.join(".git").exists() {
            found.push(dir.display().to_string());
            dirs.push(dir.to_path_buf());
            return;
        }
        if depth == 0 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        dirs.push(dir.to_path_buf());
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_dir = entry
                .file_type()
                .is_ok_and(|t| t.is_dir() && !t.is_symlink());
            if !is_dir || name.starts_with('.') || excludes.contains(&name) {
                continue;
            }
            walk(&entry.path(), depth - 1, excludes, found, dirs);
        }
    }
    let mut dirs = Vec::new();
    let found = roots
        .into_iter()
        .map(|(path, depth, excludes)| {
            let mut found = Vec::new();
            walk(&expand(&path), depth, &excludes, &mut found, &mut dirs);
            found.sort_by_key(|p| p.to_lowercase());
            (path, found)
        })
        .collect();
    (found, dirs)
}

/// スキャンするフォルダの中の `path` の変化が、リポジトリの増減かもしれないか（GTK4 版の
/// `scan_change_matters`）。
pub fn scan_change_matters(path: &Path) -> bool {
    let name = path.file_name().map(|n| n.to_string_lossy());
    // `git init`・`.git` を消した。
    if name.as_deref() == Some(".git") {
        return true;
    }
    // 見つけたリポジトリの中の、ほかの変化は増減ではない。
    if path.parent().is_some_and(|p| p.join(".git").exists()) {
        return false;
    }
    // 隠しフォルダと、今あるファイル（フォルダでないもの）の出入りも増減ではない。
    !name.is_some_and(|n| n.starts_with('.')) && (path.is_dir() || !path.exists())
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

/// [`switch_targets`] の前の結果（bookmarks.json の更新時刻、開いた日時の記録、結果）。
type SwitchCache = (
    Option<std::time::SystemTime>,
    Option<String>,
    Vec<(String, String, bool)>,
);

/// ツールバーのリポジトリ名から切り替える先（GTK4 版・macOS 版のポップオーバー）: ブックマークのリポジトリを、
/// お気に入りを先に、その後は最後に開いた順に（開いたことがなければ名前の順で後ろに）。場所・表示名・お気に入りか。
pub fn switch_targets() -> Vec<(String, String, bool)> {
    // 描くたびに呼ぶので、bookmarks.json の更新時刻と開いた日時の記録が同じなら前の結果を使う。
    thread_local! {
        static CACHE: std::cell::RefCell<Option<SwitchCache>> =
            const { std::cell::RefCell::new(None) };
    }
    let modified = file()
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.modified().ok());
    let opened_text = crate::prefs::string(OPENED);
    if let Some(list) = CACHE.with(|c| {
        c.borrow()
            .as_ref()
            .filter(|(m, o, _)| *m == modified && *o == opened_text)
            .map(|(_, _, l)| l.clone())
    }) {
        return list;
    }
    let data = read();
    let opened = opened();
    let mut list: Vec<(String, String, bool, i64)> = items(&data)
        .filter_map(|b| {
            let location = b["path"].as_str()?.to_owned();
            let name = b["name"]
                .as_str()
                .filter(|n| !n.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| folder_name(&location));
            let favorite = b["favorite"].as_bool() == Some(true);
            let when = opened.get(&key(&location)).copied().unwrap_or(i64::MIN);
            Some((location, name, favorite, when))
        })
        .collect();
    list.sort_by(|a, b| {
        b.2.cmp(&a.2)
            .then(b.3.cmp(&a.3))
            .then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase()))
    });
    let list: Vec<(String, String, bool)> =
        list.into_iter().map(|(l, n, f, _)| (l, n, f)).collect();
    CACHE.with(|c| *c.borrow_mut() = Some((modified, opened_text, list.clone())));
    list
}

/// 場所の表示名（ブックマークの上書きがなければフォルダの名前）。
pub fn display_name(location: &str) -> String {
    switch_targets()
        .into_iter()
        .find(|(l, ..)| same(l, location))
        .map_or_else(|| folder_name(location), |(_, n, _)| n)
}

/// 場所の最後のフォルダの名前（表示名の上書きがないとき）。
fn folder_name(location: &str) -> String {
    location
        .trim_end_matches(['\\', '/'])
        .rsplit(['\\', '/'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(location)
        .to_owned()
}

/// Places にある SSH 先（リポジトリかスキャンするフォルダ。Settings > SSH Hosts の一覧）。
pub fn remote_hosts() -> Vec<String> {
    let data = read();
    items(&data)
        .filter_map(|b| b["path"].as_str())
        .chain(
            data["scanRoots"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|r| r["path"].as_str()),
        )
        .filter_map(crate::remote::host_of)
        .collect()
}

/// SSH 先 `host` でスキャンするフォルダ（場所と深さ。Settings > SSH Hosts の接続のページ）。
pub fn host_scan_roots(host: &str) -> Vec<(String, u32)> {
    read()["scanRoots"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let path = r["path"].as_str()?;
            (crate::remote::host_of(path).as_deref() == Some(host))
                .then(|| (path.to_owned(), r["depth"].as_u64().unwrap_or(3) as u32))
        })
        .collect()
}

/// スキャンするフォルダを足す（もうあれば何もしない。深さと入らない名前は、この機械のものと同じ既定）。
pub fn add_scan_root(location: &str) {
    let _ = edit(|d| {
        let roots = array_mut(d, "scanRoots");
        if !roots
            .iter()
            .any(|r| r["path"].as_str().is_some_and(|p| same(p, location)))
        {
            roots.push(json!({
                "path": location,
                "depth": 3,
                "excludes": ["node_modules", "target", "build", ".build", "vendor",
                    "Pods", "DerivedData", "dist"],
            }));
        }
    });
}

/// スキャンするフォルダの深さを変える。
pub fn set_scan_depth(location: &str, depth: u32) {
    let _ = edit(|d| {
        for r in array_mut(d, "scanRoots") {
            if r["path"].as_str().is_some_and(|p| same(p, location)) {
                r["depth"] = json!(depth);
            }
        }
    });
}

/// スキャンするフォルダを外す。
pub fn remove_scan_root(location: &str) {
    let _ = edit(|d| {
        array_mut(d, "scanRoots").retain(|r| !r["path"].as_str().is_some_and(|p| same(p, location)))
    });
}

/// Places のリポジトリとスキャンするフォルダのうち、SSH 先 `host` のものの数。
pub fn places_on_host(host: &str) -> (usize, usize) {
    let data = read();
    let count = |values: Vec<&Value>| {
        values
            .into_iter()
            .filter_map(|v| v["path"].as_str())
            .filter(|p| crate::remote::host_of(p).as_deref() == Some(host))
            .count()
    };
    (
        count(items(&data).collect()),
        count(data["scanRoots"].as_array().into_iter().flatten().collect()),
    )
}

/// SSH 先 `host` のリポジトリとスキャンするフォルダを Places から外す（SSH 先には触れない。GTK4 版と同じ）。
pub fn forget_host(host: &str) {
    let on = |v: &Value| {
        v["path"]
            .as_str()
            .and_then(crate::remote::host_of)
            .as_deref()
            == Some(host)
    };
    let _ = edit(|d| {
        array_mut(d, "items").retain(|b| !on(b));
        array_mut(d, "scanRoots").retain(|r| !on(r));
    });
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

/// リポジトリを新しい窓（このプロセスの窓）で開く。
pub fn open_window(location: &str) -> Result<(), String> {
    open_window_with(location, &[])
}

/// リポジトリを新しい窓で開く（`extra` は足す引数。`--repositories` なら Repositories の画面から、`--merge` と
/// `--merge-text` なら別の窓に出したマージエディタ）。
pub fn open_window_with(location: &str, extra: &[&str]) -> Result<(), String> {
    if !is_repository(location) {
        return Err(tr!("Not found — {path}", path = location));
    }
    let mut tab = crate::tabs::make(
        &expand(location).display().to_string(),
        extra.contains(&"--repositories"),
    );
    let value = |name: &str| {
        extra
            .iter()
            .position(|e| *e == name)
            .and_then(|i| extra.get(i + 1))
            .map(|v| (*v).to_owned())
    };
    tab.merge = value("--merge");
    tab.merge_text = value("--merge-text");
    crate::app::request_window(tab);
    Ok(())
}

/// 左のグループを引いたときに、引いた名前の前に付ける印（右の一覧から引いたリポジトリと見分ける）。
pub const GROUP_DRAG: &str = "\u{1}group:";

// 画面の状態

#[derive(Clone, PartialEq)]
enum Category {
    All,
    Favorites,
    Local,
    Root(String),
    /// SSH 先（そこのリポジトリすべて）。
    Host(String),
    /// この PC の、スキャンするフォルダの外にあるリポジトリを入れているフォルダ（木の 1 つ）。
    Folder(String),
    Group(String),
}

pub struct State {
    data: Value,
    /// スキャンするフォルダごとの見つけたもの（探している間は None）。
    scanned: Option<Vec<(String, Vec<String>)>>,
    /// SSH 先のスキャンするフォルダごとの見つけたもの（つながらなければその理由。探している間はない）。
    remote_scanned: HashMap<String, Result<Vec<String>, String>>,
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
    /// お気に入り・グループ（分類の名前）に、一覧から引いた行（場所）を落とした。
    DroppedOn(String, Vec<String>),
    Selected(Option<usize>),
    Filter(String),
    /// 行・分類の右クリックのメニュー。
    Menu(String),
    /// 選んでいるリポジトリを開く（ダブルクリック・Enter）。
    Open,
    AddRepository,
    /// SSH 先のリポジトリを加える（Add SSH Connection…）。
    AddSsh,
    AddScanFolder,
    /// 列で選ぶ画面で選んだ: そのリポジトリを足す・そのフォルダをスキャンする。
    AddRepositoryAt(String),
    AddScanFolderAt(String),
    Close,
    /// 探し終えた（見つけたものと、見張るフォルダ）。
    Scanned(Vec<(String, Vec<String>)>, Vec<PathBuf>),
    /// SSH 先のスキャンするフォルダを探した（その場所と、見つけたもの・つながらなかった理由）。
    RemoteScanned(String, Result<Vec<String>, String>),
    /// 見張っているフォルダでリポジトリが増えた・消えたかもしれない（見張りの何回目か）。
    ScanChanged(u64),
    Peeked(Vec<(String, Peek)>),
    /// fetch が終わった（場所と、失敗なら理由）。
    Fetched(String, Option<String>),
    /// 列の見出しを押した。
    Sort(&'static str),
    /// 列の見出し（`COLUMN_IDS` の名前）を引いて、左から何番目（Name の後ろから）の所へ落とした。
    ColumnDropped(String, usize),
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
    /// SSH 先のリポジトリの接続先とパスを聞く。
    AddSsh,
    /// そのリポジトリだけの窓の背景を決めるダイアログを出す。
    Backdrop(String),
    /// この機械のフォルダを列で選ぶ画面を出す（選んだら `AddRepositoryAt`・`AddScanFolderAt`）。
    PickLocal(crate::pathpicker::Purpose),
}

impl State {
    pub fn new() -> (Self, Vec<Root>) {
        let data = read();
        let roots = roots(&data);
        (
            Self {
                data,
                scanned: None,
                remote_scanned: HashMap::new(),
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

    /// SSH 先でスキャンするフォルダ（探しに行く先。窓が SSH 先ごとに探す）。
    pub fn remote_roots(&self) -> Vec<Root> {
        remote_roots(&self.data)
    }

    /// Places の SSH 先（ブックマークのリポジトリかスキャンするフォルダがあるもの。名前の順）。
    fn hosts(&self) -> Vec<String> {
        let mut hosts: Vec<String> = items(&self.data)
            .filter_map(|b| b["path"].as_str())
            .chain(
                self.data["scanRoots"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|r| r["path"].as_str()),
            )
            .filter_map(crate::remote::host_of)
            .collect();
        hosts.sort_by_key(|h| h.to_lowercase());
        hosts.dedup();
        hosts
    }

    /// この PC のリポジトリのうち、スキャンするフォルダの下にないもの（Places のフォルダの木に出す）。
    fn unscanned(&self) -> Vec<String> {
        let roots: Vec<String> = roots(&self.data)
            .into_iter()
            .map(|(p, ..)| key(&expand(&p).display().to_string()) + "/")
            .collect();
        self.all()
            .into_iter()
            .filter(|p| !is_remote(p) && !roots.iter().any(|r| key(p).starts_with(r)))
            .collect()
    }

    fn scanned_all(&self) -> impl Iterator<Item = &String> {
        self.scanned
            .iter()
            .flatten()
            .flat_map(|(_, found)| found)
            .chain(
                self.remote_scanned
                    .values()
                    .flat_map(|r| r.iter().flatten()),
            )
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

    /// 左の分類（分類・名前・見出し・字下げの段）。
    fn categories(&self) -> Vec<(Category, String, Option<String>, usize)> {
        let mut out = vec![
            (Category::All, tr!("All"), None, 0),
            (Category::Favorites, tr!("Favorites"), None, 0),
            (
                Category::Local,
                tr!("This Computer"),
                Some(tr!("Places")),
                0,
            ),
        ];
        for (path, ..) in roots(&self.data) {
            out.push((
                Category::Root(path.clone()),
                format!("🔍 {}", tilde(&path)),
                None,
                1,
            ));
        }
        // スキャンするフォルダの外のリポジトリを、入れているフォルダの木で（GTK4 版・macOS 版と同じ）。
        for (path, depth, label) in folder_tree(&self.unscanned()) {
            out.push((Category::Folder(path), label, None, depth + 1));
        }
        // SSH 先ごとに、その下にスキャンするフォルダ（GTK4 版・macOS 版の Places の SSH 先）。
        let remote = remote_roots(&self.data);
        for host in self.hosts() {
            out.push((Category::Host(host.clone()), host.clone(), None, 0));
            for (path, ..) in remote
                .iter()
                .filter(|(p, ..)| crate::remote::host_of(p).as_deref() == Some(host.as_str()))
            {
                let shown = amonhen_remote::ssh::Target::parse(path).map_or_else(
                    |_| path.clone(),
                    |t| t.path.trim_start_matches('/').to_owned(),
                );
                let shown = match self.remote_scanned.get(path) {
                    Some(Err(_)) => format!("🔍 {shown} ⚠"),
                    _ => format!("🔍 {shown}"),
                };
                out.push((Category::Root(path.clone()), shown, None, 1));
            }
        }
        for (i, g) in self.groups().into_iter().enumerate() {
            let head = (i == 0).then(|| tr!("Groups"));
            out.push((Category::Group(g.clone()), g, head, 0));
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
            Category::Folder(folder) => {
                let prefix = key(folder) + "/";
                self.unscanned()
                    .into_iter()
                    .filter(|p| key(p).starts_with(&prefix))
                    .collect()
            }
            Category::Root(root) if is_remote(root) => self
                .remote_scanned
                .get(root)
                .and_then(|r| r.as_ref().ok())
                .cloned()
                .unwrap_or_default(),
            Category::Root(root) => self
                .scanned
                .iter()
                .flatten()
                .filter(|(r, _)| r == root)
                .flat_map(|(_, found)| found.clone())
                .collect(),
            Category::Host(host) => self
                .all()
                .into_iter()
                .filter(|p| crate::remote::host_of(p).as_deref() == Some(host.as_str()))
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
    /// 左の分類の並び（変わったら一覧が作り直される）。
    pub fn side_key(&self) -> String {
        // 件数も含める（SSH 先を探し終えたら、件数の変わった行を作り直す）。
        self.categories()
            .iter()
            .map(|c| format!("{}:{}\n", c.1, self.locations(&c.0).len()))
            .collect()
    }

    /// 引いてきた行を落とせる左の行の名前（お気に入りとグループ）。
    pub fn drop_targets(&self) -> Vec<String> {
        self.categories()
            .into_iter()
            .filter(|c| matches!(c.0, Category::Favorites | Category::Group(_)))
            .map(|c| c.1)
            .collect()
    }

    /// 左の行のうち、引いて並べ替えられるもの（グループ）。
    pub fn draggable_groups(&self) -> Vec<String> {
        self.categories()
            .into_iter()
            .filter(|c| matches!(c.0, Category::Group(_)))
            .map(|c| c.1)
            .collect()
    }

    /// 出している並び（変わったら一覧が作り直される）。
    pub fn shown_key(&self) -> String {
        self.shown().iter().map(|l| key(l) + "\n").collect()
    }

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

    /// SSH 先のリポジトリを一覧に加えて、このタブで開く（GTK4 版の `add_remote`）。つながらなくても一覧には
    /// 残る。
    pub fn add_remote(&mut self, location: &str) {
        self.change(|d| update(d, location, |_| {}));
        self.selected = Some(location.to_owned());
        self.open(location);
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
            Event::ColumnDropped(id, slot) => place_column(&id, slot),
            Event::Open => {
                if let Some(location) = self.selected.clone() {
                    self.open(&location);
                }
            }
            Event::AddSsh => return Action::AddSsh,
            // グループを引いて、ほかのグループに落とした: その前に移す（GTK4 版・macOS 版のグループの並べ替え）。
            Event::DroppedOn(target, dragged)
                if dragged.iter().all(|d| d.starts_with(GROUP_DRAG)) =>
            {
                let moved: Vec<String> = dragged
                    .iter()
                    .map(|d| d[GROUP_DRAG.len()..].to_owned())
                    .collect();
                let groups = self.groups();
                if !groups.contains(&target) || moved.contains(&target) {
                    return Action::None;
                }
                let mut order: Vec<String> =
                    groups.into_iter().filter(|g| !moved.contains(g)).collect();
                let at = order
                    .iter()
                    .position(|g| *g == target)
                    .unwrap_or(order.len());
                for (i, g) in moved.into_iter().enumerate() {
                    order.insert(at + i, g);
                }
                self.change(|d| d["groups"] = json!(order));
            }
            Event::DroppedOn(target, locations) => {
                // 同じ名前なら、お気に入りを先に（グループに「お気に入り」と名付けたときも、上の行に入る）。
                let category = self
                    .categories()
                    .into_iter()
                    .find(|c| {
                        c.1 == target && matches!(c.0, Category::Favorites | Category::Group(_))
                    })
                    .map(|c| c.0);
                match category {
                    Some(Category::Favorites) if !locations.is_empty() => self.change(|d| {
                        for l in &locations {
                            update(d, l, |b| {
                                b.insert("favorite".into(), json!(true));
                            });
                        }
                    }),
                    Some(Category::Group(name)) if !locations.is_empty() => self.change(|d| {
                        for l in &locations {
                            update(d, l, |b| {
                                b.insert("group".into(), json!(name));
                            });
                        }
                    }),
                    _ => {}
                }
            }
            // この機械のフォルダは、列で選ぶ画面で選ぶ（GTK4 版・macOS 版と同じ）。
            Event::AddRepository => return Action::PickLocal(crate::pathpicker::Purpose::AddLocal),
            Event::AddScanFolder => {
                return Action::PickLocal(crate::pathpicker::Purpose::ScanLocal);
            }
            Event::AddRepositoryAt(path) => {
                if !is_repository(&path) {
                    self.note = tr!("Not found — {path}", path = path);
                    return Action::None;
                }
                self.change(|d| update(d, &path, |_| {}));
                self.selected = Some(path);
                return Action::Peek(self.unpeeked());
            }
            Event::AddScanFolderAt(path) => {
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
            Event::ScanChanged(_) => {
                self.scanned = None;
                return Action::Scan(roots(&self.data));
            }
            Event::Scanned(found, _) => {
                self.scanned = Some(found);
                return Action::Peek(self.unpeeked());
            }
            Event::RemoteScanned(root, found) => {
                self.remote_scanned.insert(root, found);
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

    /// このタブの隣の新しいタブで開く（Repositories のタブは残す。GTK4 版・macOS 版の Open と同じ）。
    fn open(&mut self, location: &str) {
        self.open_with(location, crate::tabs::open(location));
    }

    fn open_with(&mut self, location: &str, result: Result<(), String>) {
        match result {
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
            "open-daemon" => {
                let through = if is_remote(&location) {
                    format!("daemon:{location}")
                } else {
                    amonhen_remote::local::canonical_path(&expand(&location).to_string_lossy())
                        .map_or_else(|_| format!("daemon:{location}"), |p| format!("daemon:{p}"))
                };
                self.open_with(&location, crate::tabs::open(&through));
            }
            "open-window" => self.open_with(&location, open_window(&location)),
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
            "backdrop" => return Action::Backdrop(location),
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
            "column-left" | "column-right" => {
                move_column(&location, what == "column-left");
            }
            "rescan" => {
                self.scanned = None;
                return Action::Scan(roots(&self.data));
            }
            // 探す深さ（GTK4 版・macOS 版の Search Depth）。変えたら探し直す。
            "depth" => {
                let Ok(depth) = value.parse::<u32>() else {
                    return Action::None;
                };
                self.change(|d| {
                    for r in array_mut(d, "scanRoots") {
                        if r["path"].as_str().is_some_and(|p| same(p, &location)) {
                            r["depth"] = json!(depth);
                        }
                    }
                });
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
    /// 左の分類の一覧を包む Grid（引いてきた行を受ける）。
    pub side_host: &'a ElementRef<Grid>,
    /// リポジトリの一覧を包む Grid（ダブルクリック・Enter を付ける）。
    pub list_host: &'a ElementRef<Grid>,
    /// 見出しの列の境目のつまみ（Branch・Status・Last Commit・Opened の左端。動かすと幅が変わる）。
    pub column_handles: &'a [ElementRef<Grid>; 4],
    /// 見出しの列（押して並べ替え、引いて列を動かす）。
    pub header_host: &'a ElementRef<Grid>,
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
    let mut items = vec![
        MenuItem::item(format!("open:{location}"), tr!("Open")),
        MenuItem::item(format!("open-window:{location}"), tr!("Open in New Window")),
    ];
    // 共有の daemon 越しに（SSH 先も `daemon:ssh://…`。GTK4 版と同じく、いつも daemon 越しなら出さない）。
    if !location.starts_with("daemon:") && !crate::prefs::bool(crate::prefs::OPEN_THROUGH_DAEMON) {
        items.push(MenuItem::item(
            format!("open-daemon:{location}"),
            tr!("Open Through the Shared daemon"),
        ));
    }
    items.extend([
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
        MenuItem::item(format!("backdrop:{location}"), tr!("Window Background…")),
        MenuItem::separator("-files"),
    ]);
    // SSH 先のフォルダは、この機械では開けない。
    if !is_remote(location) {
        items.extend([
            MenuItem::item(format!("reveal:{location}"), tr!("Show in Files")),
            MenuItem::item(format!("terminal:{location}"), tr!("Open in Terminal")),
            MenuItem::item(format!("editor:{location}"), tr!("Open in Editor")),
        ]);
    }
    items.extend([
        MenuItem::item(format!("copy:{location}"), tr!("Copy Path")),
        MenuItem::separator("-remove"),
        MenuItem::item(format!("remove:{location}"), tr!("Remove from List")),
    ]);
    items
}

/// 分類の右クリックのメニュー（`data` は今の探す深さに印を付けるため）。
fn category_menu(category: &Category, data: &Value) -> Vec<MenuItem> {
    let add = [
        MenuItem::item("add-group:", tr!("New Group…")),
        MenuItem::separator("-add"),
    ];
    match category {
        Category::Root(path) => vec![
            MenuItem::submenu("depth", tr!("Search Depth"), {
                let depth = roots(data)
                    .into_iter()
                    .chain(remote_roots(data))
                    .find(|(p, ..)| same(p, path))
                    .map_or(3, |(_, d, _)| d);
                (1..=6u32)
                    .map(|d| {
                        let label = if d == depth {
                            format!("✓ {d}")
                        } else {
                            d.to_string()
                        };
                        MenuItem::item(format!("depth:{d}|{path}"), label)
                    })
                    .collect::<Vec<_>>()
            }),
            MenuItem::item("rescan:", tr!("Search Again")),
            if is_remote(path) {
                MenuItem::item(format!("copy:{path}"), tr!("Copy Path"))
            } else {
                MenuItem::item(format!("reveal:{path}"), tr!("Show in Files"))
            },
            MenuItem::separator("-remove"),
            MenuItem::item(format!("remove-root:{path}"), tr!("Stop Scanning")),
        ],
        Category::Folder(path) => vec![
            MenuItem::item(format!("reveal:{path}"), tr!("Show in Files")),
            MenuItem::item(format!("copy:{path}"), tr!("Copy Path")),
        ],
        Category::Group(name) => vec![
            add[0].clone(),
            MenuItem::separator("-delete"),
            MenuItem::item(format!("delete-group:{name}"), tr!("Delete Group")),
        ],
        _ => vec![add[0].clone()],
    }
}

/// ツールバーのリポジトリ名の一覧に添える、ブランチと状態の短い文字（GTK4 版の `show_compact`）。
pub fn peek_summary(p: &Peek) -> String {
    let mut parts = vec![p.branch.clone().unwrap_or_else(|| tr!("detached HEAD"))];
    if p.changed > 0 {
        parts.push(tr!("Changes {count}", count = p.changed));
    }
    if p.ahead > 0 {
        parts.push(format!("↑{}", p.ahead));
    }
    if p.behind > 0 {
        parts.push(format!("↓{}", p.behind));
    }
    parts.join("  ")
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

/// Branch・Status・Last Commit・Opened の列の初めの幅。
const COLUMNS: [f64; 4] = [150.0, 110.0, 280.0, 110.0];
/// 名前の列の最低限の幅（窓が狭ければ、表ごと横にスクロールする）。
const NAME_MIN_WIDTH: f64 = 200.0;
/// 列の幅を覚える設定の鍵（`,` 区切り）。
const COLUMN_WIDTHS_KEY: &str = "repositoryColumnWidths";

/// 状態の列（Name の後ろ。並べ替えられる。幅はこの順の列ごとに覚える）。
const COLUMN_IDS: [&str; 4] = ["branch", "status", "commit", "opened"];
/// 列の並びを覚える設定の鍵（`COLUMN_IDS` の名前を `,` 区切りで。GTK4 版の `repositoryPageColumnOrder`）。
const COLUMN_ORDER_KEY: &str = "repositoryColumnOrder";

thread_local! {
    /// 列の幅（見出しの境目を動かすと変わる。`COLUMN_IDS` の順）。
    static COLUMN_WIDTHS: std::cell::Cell<[f64; 4]> = std::cell::Cell::new(load_column_widths());
    /// 並べている列（左から。`COLUMN_IDS` の何番目か）。
    static COLUMN_ORDER: std::cell::Cell<[usize; 4]> = std::cell::Cell::new(load_column_order());
}

fn load_column_order() -> [usize; 4] {
    parse_column_order(&crate::prefs::string(COLUMN_ORDER_KEY).unwrap_or_default())
}

/// 覚えた並び（知らない・足りない列は、残りを元の順で後ろに足す）。
fn parse_column_order(text: &str) -> [usize; 4] {
    let mut order: Vec<usize> = Vec::new();
    for id in text.split(',').map(str::trim) {
        if let Some(i) = COLUMN_IDS.iter().position(|c| *c == id)
            && !order.contains(&i)
        {
            order.push(i);
        }
    }
    for i in 0..4 {
        if !order.contains(&i) {
            order.push(i);
        }
    }
    [order[0], order[1], order[2], order[3]]
}

/// 並べている列（左から。`COLUMN_IDS` の何番目か）。
fn column_order() -> [usize; 4] {
    COLUMN_ORDER.with(std::cell::Cell::get)
}

/// 列 `id` を左（`left`）か右に 1 つ動かして覚える。動かせなければ何もしない。
fn move_column(id: &str, left: bool) {
    let Some(column) = COLUMN_IDS.iter().position(|c| *c == id) else {
        return;
    };
    let mut order = column_order();
    let Some(at) = order.iter().position(|&c| c == column) else {
        return;
    };
    let to = if left {
        at.checked_sub(1)
    } else {
        Some(at + 1).filter(|&t| t < 4)
    };
    let Some(to) = to else {
        return;
    };
    order.swap(at, to);
    COLUMN_ORDER.with(|c| c.set(order));
    let ids: Vec<&str> = order.iter().map(|&i| COLUMN_IDS[i]).collect();
    let _ = crate::prefs::set_string(COLUMN_ORDER_KEY, &ids.join(","));
}

/// 列 `id` を、左から `slot` 番目（Name の後ろから）に置いて覚える（macOS 版・GTK4 版の見出しのドラッグ）。
fn place_column(id: &str, slot: usize) {
    let Some(column) = COLUMN_IDS.iter().position(|c| *c == id) else {
        return;
    };
    let order = placed(column_order(), column, slot);
    COLUMN_ORDER.with(|c| c.set(order));
    let ids: Vec<&str> = order.iter().map(|&i| COLUMN_IDS[i]).collect();
    let _ = crate::prefs::set_string(COLUMN_ORDER_KEY, &ids.join(","));
}

/// `order` の中の列 `column` を、`slot` 番目に移した並び。
fn placed(order: [usize; 4], column: usize, slot: usize) -> [usize; 4] {
    let mut rest: Vec<usize> = order.into_iter().filter(|&c| c != column).collect();
    rest.insert(slot.min(3), column);
    [rest[0], rest[1], rest[2], rest[3]]
}

/// 見出しのマスの並び（見出しの Grid の子の順。Name が 0、`COLUMN_IDS` の順に 1〜4）。
pub const HEADER_IDS: [&str; 5] = ["name", "branch", "status", "commit", "opened"];

/// 列 `id` が左から何番目（Name の後ろから）に並んでいるか（Name なら 0）。
pub fn slot_of(id: &str) -> usize {
    COLUMN_IDS
        .iter()
        .position(|c| *c == id)
        .and_then(|i| column_order().iter().position(|&c| c == i))
        .unwrap_or(0)
}

fn load_column_widths() -> [f64; 4] {
    let mut widths = COLUMNS;
    if let Some(text) = crate::prefs::string(COLUMN_WIDTHS_KEY) {
        for (w, part) in widths.iter_mut().zip(text.split(',')) {
            if let Ok(v) = part.trim().parse::<f64>() {
                *w = v;
            }
        }
    }
    widths
}

/// 左から `i` 番目（Name の後ろから）の列の幅。
pub fn column_width(i: usize) -> f64 {
    COLUMN_WIDTHS.with(std::cell::Cell::get)[column_order()[i.min(3)]]
}

/// 左から `i` 番目の列の幅を変える（狭すぎ・広すぎにはしない。幅はその列に付いていく）。
pub fn set_column_width(i: usize, width: f64) {
    let column = column_order()[i.min(3)];
    COLUMN_WIDTHS.with(|c| {
        let mut widths = c.get();
        widths[column] = width.clamp(60.0, 600.0);
        c.set(widths);
    });
}

/// 列の幅を覚える。
pub fn save_column_widths() {
    let text: Vec<String> = COLUMN_WIDTHS
        .with(std::cell::Cell::get)
        .iter()
        .map(|w| w.round().to_string())
        .collect();
    let _ = crate::prefs::set_string(COLUMN_WIDTHS_KEY, &text.join(","));
}

fn columns() -> Vec<GridLength> {
    let widths = COLUMN_WIDTHS.with(std::cell::Cell::get);
    let mut c = vec![GridLength::Star(1.0)];
    c.extend(column_order().map(|i| GridLength::Pixel(widths[i])));
    c
}

/// 列 `id`（`COLUMN_IDS` の名前）を出している列の番号（Name が 0）。
fn grid_column_of(id: &str) -> i32 {
    let i = COLUMN_IDS.iter().position(|c| *c == id).unwrap_or(0);
    column_order().iter().position(|&c| c == i).unwrap_or(0) as i32 + 1
}

pub fn view(state: &State, callbacks: Callbacks) -> View {
    let head = Grid::new()
        .columns([
            GridLength::Auto,
            GridLength::Star(1.0),
            GridLength::Auto,
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
            crate::small::button(tr!("Add Local Repository…"), |b| {
                b.on_click((callbacks.button)("add-repository"))
                    .grid_column(2)
            }),
            crate::small::button(tr!("Add SSH Connection…"), |b| {
                b.on_click((callbacks.button)("add-ssh")).grid_column(3)
            }),
            crate::small::button(tr!("Add Folder to Scan…"), |b| {
                b.on_click((callbacks.button)("add-scan-folder"))
                    .grid_column(4)
            }),
            crate::small::button(tr!("Close"), |b| {
                b.on_click((callbacks.button)("close")).grid_column(5)
            }),
        ));
    // 左: 分類と件数。
    let categories = state.categories();
    let side_rows = categories
        .iter()
        .enumerate()
        .map(|(i, (c, name, head, depth))| {
            let count = state.locations(c).len();
            let line = Grid::new()
                .columns([GridLength::Star(1.0), GridLength::Auto])
                .margin(Thickness::new(*depth as f64 * 14.0, 0.0, 0.0, 0.0))
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
            let content =
                content.menu(Menu::new(category_menu(c, &state.data), (callbacks.menu)()));
            (
                format!("{i}:{name}"),
                ListViewItem::new()
                    .tag(name.clone())
                    .min_height(crate::small::ROW_HEIGHT)
                    .content(content),
            )
        });
    let side_key = state.side_key();
    // お気に入り・グループには、右の一覧の行を引いて入れられる（`xaml::drop_on_rows`。GTK4 版・macOS 版と同じ）。
    let side = ListView::new()
        .can_drag_items(true)
        .selected_index(Some(state.category))
        .on_selection_changed(callbacks.category.clone())
        .collection_slot(ListViewSlot::Items, side_rows);
    let side = Grid::new()
        .element_ref(callbacks.side_host)
        .grid_column(0)
        .grid_row(1)
        .children((View::keyed_fragment([(side_key, side)]),));
    // 右: リポジトリの一覧。
    let shown = state.shown();
    // 見出しを押すとその列で並べる（並べている列に ▲・▼）。
    let header_cell = |text: String, id: &'static str, column: i32| -> View {
        let text = match (state.sort == id, state.ascending) {
            (true, true) => format!("{text} ▲"),
            (true, false) => format!("{text} ▼"),
            _ => text,
        };
        let title = text.trim_end_matches([' ', '▲', '▼']).to_owned();
        // 押す（並べ替え）・引く（列を動かす。macOS 版・GTK4 版と同じ）は、見出し全体に付けた指の処理が受ける
        // （app.rs の `attach_column_header`）。
        let button = TextBlock::new()
            .text(text)
            .font_size(11.0)
            .opacity(0.6)
            .margin(Thickness::new(10.0, 6.0, 10.0, 6.0));
        // 右クリックでも、その列を左右に動かせる（Name は動かさない）。
        let mut items = Vec::new();
        if column > 1 {
            items.push(MenuItem::item(
                format!("column-left:{id}"),
                format!("◀ {title}"),
            ));
        }
        if (1..4).contains(&column) {
            items.push(MenuItem::item(
                format!("column-right:{id}"),
                format!("{title} ▶"),
            ));
        }
        let cell = Grid::new()
            .grid_column(column)
            .background(Brush::Solid(Color::transparent()))
            .children((button,));
        if items.is_empty() {
            cell
        } else {
            cell.menu(Menu::new(items, (callbacks.menu)()))
        }
    };
    let header = Grid::new()
        .element_ref(callbacks.header_host)
        .background(Brush::Solid(Color::transparent()))
        .columns(columns())
        .margin(Thickness::new(16.0, 0.0, 16.0, 4.0))
        .grid_row(0)
        .children((
            header_cell(tr!("Name"), "name", 0),
            header_cell(tr!("Branch"), "branch", grid_column_of("branch")),
            header_cell(tr!("Status"), "status", grid_column_of("status")),
            header_cell(tr!("Last Commit"), "commit", grid_column_of("commit")),
            header_cell(tr!("Opened"), "opened", grid_column_of("opened")),
            View::keyed_fragment(callbacks.column_handles.iter().enumerate().map(|(i, h)| {
                (
                    format!("handle-{i}"),
                    Grid::new()
                        .element_ref(h)
                        .background(Brush::Solid(Color::transparent()))
                        .width(6.0)
                        .grid_column(i as i32 + 1)
                        .horizontal_alignment(HorizontalAlignment::Left),
                )
            })),
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
        let status_cell = cell(status, grid_column_of("status"));
        let status_cell: View = match tip {
            Some(tip) => status_cell.tooltip(tip),
            None => status_cell.into(),
        };
        let row = Grid::new()
            .columns(columns())
            .margin(Thickness::new(0.0, 3.0, 0.0, 3.0))
            .children((
                name_cell,
                cell(branch, grid_column_of("branch")),
                status_cell,
                cell(commit, grid_column_of("commit")),
                cell(opened, grid_column_of("opened")).opacity(0.7),
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
    // Ctrl・Shift で複数選べる（選んだ行の右クリックの Fetch は、選んだものすべて。macOS 版と同じ）。
    let list = ListView::new()
        .selection_mode(ListViewSelectionMode::Extended)
        // 左のお気に入り・グループへ引いて入れる（引いた行の場所は `xaml::drag_tags` が覚える）。
        .can_drag_items(true)
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
    // 窓が狭いと、幅の決まった列に押されて名前の列がなくなる。名前の列に最低限の幅を残し、足りなければ表ごと
    // 横にスクロールする。
    let min_width =
        NAME_MIN_WIDTH + COLUMN_WIDTHS.with(std::cell::Cell::get).iter().sum::<f64>() + 32.0;
    let main = Grid::new()
        .rows([GridLength::Auto, GridLength::Star(1.0), GridLength::Auto])
        .min_width(min_width)
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
    let main = ScrollViewer::new()
        .horizontal_scroll_bar_visibility(ScrollBarVisibility::Auto)
        .vertical_scroll_bar_visibility(ScrollBarVisibility::Disabled)
        .grid_column(1)
        .grid_row(1)
        .content(main);
    Grid::new()
        .rows([GridLength::Auto, GridLength::Star(1.0)])
        .columns([GridLength::Pixel(240.0), GridLength::Star(1.0)])
        .children((head, side, main))
}

/// `repos` を入れているフォルダの木（GTK4 版・macOS 版の Places の下）。行は（フォルダ・段・名前）で、上から順。
/// 子が 1 つだけで自分のリポジトリもないフォルダは、子と 1 行にまとめる（`~\prj\oss` のように）。一番上の行は
/// ホームの下を `~` で、下の行は親からの相対の名前で出す。
fn folder_tree(repos: &[String]) -> Vec<(String, usize, String)> {
    use std::collections::BTreeMap;
    #[derive(Default)]
    struct Node {
        /// 元の綴りのフォルダ（区切りは元のまま）。
        path: String,
        holds: bool,
        children: BTreeMap<String, Node>,
    }
    let mut root = Node::default();
    for repo in repos {
        let trimmed = repo.trim_end_matches(['\\', '/']);
        let Some(cut) = trimmed.rfind(['\\', '/']) else {
            continue;
        };
        let parent = &trimmed[..cut];
        let mut node = &mut root;
        let mut at = 0;
        for part in parent.split(['\\', '/']) {
            at += part.len();
            let path = parent[..at].to_owned();
            at += 1;
            if part.is_empty() {
                continue;
            }
            node = node.children.entry(part.to_lowercase()).or_default();
            if node.path.is_empty() {
                node.path = path;
            }
        }
        node.holds = true;
    }
    fn walk(node: &Node, depth: usize, from: Option<&str>, out: &mut Vec<(String, usize, String)>) {
        for child in node.children.values() {
            // 子が 1 つだけで自分のリポジトリもなければ、子へ下りる。
            let mut shown = child;
            while !shown.holds && shown.children.len() == 1 {
                shown = shown.children.values().next().expect("1 つある");
            }
            let label = match from {
                Some(parent) => shown.path[parent.len()..]
                    .trim_start_matches(['\\', '/'])
                    .to_owned(),
                None => tilde(&shown.path),
            };
            out.push((shown.path.clone(), depth, label));
            walk(shown, depth + 1, Some(&shown.path), out);
        }
    }
    let mut out = Vec::new();
    walk(&root, 0, None, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_holding_repositories_form_a_folded_tree() {
        let repos = [
            r"D:\src\prj\oss\a".to_owned(),
            r"D:\src\prj\oss\b".to_owned(),
            r"D:\src\prj\work\c".to_owned(),
            r"D:\src\prj\work\deep\d".to_owned(),
            r"E:\x\y\z\e".to_owned(),
        ];
        let rows: Vec<(String, usize, String)> = folder_tree(&repos);
        let shown: Vec<(usize, &str)> = rows.iter().map(|(_, d, l)| (*d, l.as_str())).collect();
        assert_eq!(
            shown,
            [
                (0, r"D:\src\prj"),
                (1, "oss"),
                (1, "work"),
                (2, "deep"),
                (0, r"E:\x\y\z"),
            ]
        );
        assert_eq!(rows[2].0, r"D:\src\prj\work");
    }

    #[test]
    fn a_dropped_column_goes_to_its_slot() {
        assert_eq!(placed([0, 1, 2, 3], 3, 0), [3, 0, 1, 2]);
        assert_eq!(placed([0, 1, 2, 3], 0, 2), [1, 2, 0, 3]);
        assert_eq!(placed([0, 1, 2, 3], 1, 9), [0, 2, 3, 1]);
    }

    #[test]
    fn column_order_keeps_known_columns_once() {
        assert_eq!(parse_column_order(""), [0, 1, 2, 3]);
        assert_eq!(parse_column_order("opened,branch"), [3, 0, 1, 2]);
        assert_eq!(parse_column_order("commit,commit,x,status"), [2, 1, 0, 3]);
    }

    #[test]
    fn scan_reports_repositories_and_the_folders_to_watch() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let root =
            std::env::temp_dir().join(format!("amonhen-scan-{}-{nanos}", std::process::id()));
        // root/a/.git（リポジトリ）、root/b/c/.git（深さ 2）、root/.hidden/d/.git（隠しフォルダは見ない）。
        for dir in ["a/.git", "b/c/.git", ".hidden/d/.git"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let path = root.display().to_string();
        let (found, dirs) = scan_with_dirs(vec![(path.clone(), 1, Vec::new())]);
        assert_eq!(found[0].1, [root.join("a").display().to_string()]);
        assert!(dirs.contains(&root) && dirs.contains(&root.join("a")));
        // 深さ 2 なら b/c も見つかり、b も見張る。
        let (found, dirs) = scan_with_dirs(vec![(path, 2, Vec::new())]);
        assert_eq!(found[0].1.len(), 2);
        assert!(dirs.contains(&root.join("b")));
        // リポジトリの中の変化は増減ではない。`.git` と新しいフォルダは増減かもしれない。
        assert!(!scan_change_matters(&root.join("a").join("src")));
        assert!(scan_change_matters(&root.join("a").join(".git")));
        assert!(scan_change_matters(&root.join("b")));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
