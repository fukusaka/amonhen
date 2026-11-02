//! リポジトリを開く周り: 最近開いたもの（File > Open Recent）と、File > Clone…（macOS 版の `Clone.swift`）。
//! 最後に開いた日時（Repositories の一覧の Opened）と、開いている窓（次の起動で開き直す）も覚える。

use std::path::{Path, PathBuf};
use std::rc::Rc;

use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gio, glib};

use crate::sheets::{Response, Sheet, row};

const RECENT: &str = "recentRepositories";
const CLONE_FOLDER: &str = "cloneFolder";
/// File > Open Recent に並べる数（記録はすべて残す。macOS 版と同じ）。
const MENU_ITEMS: usize = 15;
/// 最後に開いた日時（場所 → UNIX 時間。macOS 版の `repositoryOpenedAt`）。
const OPENED: &str = "repositoryOpenedAt";

/// 最近開いたリポジトリ（新しい順。手元のパス・`daemon:`・`ssh://` の場所）。
pub fn recent() -> Vec<String> {
    crate::settings::string(RECENT)
        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .unwrap_or_default()
}

/// 追加した worktree のフォルダーなら、そのメインのリポジトリのフォルダー（macOS 版の
/// `RepositoryList.mainRepository(ofWorktree:)`）。`.git` の `gitdir:` が `<main>/.git/worktrees/<名前>` を
/// 指すものだけ。git は起動しない。daemon・SSH 越しの場所は None。
pub fn main_of_worktree(location: &Path) -> Option<PathBuf> {
    if amonhen_remote::location::is_remote(&location.to_string_lossy()) {
        return None;
    }
    let dot_git = location.join(".git");
    if !dot_git.is_file() {
        return None;
    }
    let text = std::fs::read_to_string(&dot_git).ok()?;
    let target = Path::new(text.strip_prefix("gitdir:")?.trim());
    let git_dir = location.join(target);
    let git_dir = git_dir.to_string_lossy();
    let (main, _) = git_dir.split_once("/.git/worktrees/")?;
    Some(PathBuf::from(main))
}

/// 手元のリポジトリの中のフォルダーなら、その作業ツリーの一番上（`.git` のあるフォルダー）。git と同じく上へ
/// たどる。見つからない・daemon・SSH 越しの場所はそのまま。パスは [`standardize`] した形（リンクは解かない）。
pub fn top_of(location: &Path) -> PathBuf {
    if amonhen_remote::location::is_remote(&location.to_string_lossy()) {
        return location.to_owned();
    }
    let full = standardize(location);
    full.ancestors()
        .find(|d| d.join(".git").exists())
        .map_or(full.clone(), Path::to_path_buf)
}

/// 手元のパスを、開く・覚える形にする（macOS 版の `standardizedFileURL`）: 絶対パスにし、`.`・`..` を字面で
/// 除く。シンボリックリンクは解かない（リンクの下で選んだリポジトリは、そのパスのまま一覧・最近開いたものに
/// 出す。ブックマーク・スキャンで見つけたものと同じ形）。daemon・SSH 越しの場所はそのまま。
pub fn standardize(location: &Path) -> PathBuf {
    if amonhen_remote::location::is_remote(&location.to_string_lossy()) {
        return location.to_owned();
    }
    let absolute = if location.is_absolute() {
        location.to_owned()
    } else {
        std::env::current_dir().unwrap_or_default().join(location)
    };
    let mut out = PathBuf::new();
    for part in absolute.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// 場所を、比べられる形にする（手元のパスは正規の形に。リンクを解く。同じリポジトリのタブを探すときなど）。
pub fn normalize(location: &Path) -> String {
    let text = location.to_string_lossy().into_owned();
    if amonhen_remote::location::is_remote(&text) {
        text
    } else {
        std::fs::canonicalize(location).map_or(text, |p| p.to_string_lossy().into_owned())
    }
}

/// Settings > Advanced で共有の daemon 越しに開くなら、場所を `daemon:` の形にする（SSH 先も `daemon:ssh://…`）。
/// 開いているかを比べる・覚える・Activity に出すのもこの形（macOS 版と同じ）。
pub fn through_daemon(location: &Path) -> PathBuf {
    PathBuf::from(with_daemon(
        &standardize(location).to_string_lossy(),
        crate::settings::bool(crate::settings::OPEN_THROUGH_DAEMON),
    ))
}

/// 一覧・Open Recent に覚える形。Settings で共有の daemon 越しに開くときに付けた `daemon:` は外す（開き方の設定で、
/// 別のリポジトリではない。同じリポジトリを `daemon:` の有無で二度並べない。macOS 版と同じ）。設定が切れていて
/// `daemon:` で開いたもの（右クリックの「共有の daemon 越しに開く」）は、そのまま。
fn listed_form(location: &str, through_daemon: bool) -> String {
    match location.strip_prefix("daemon:") {
        Some(rest) if through_daemon => rest.to_owned(),
        _ => location.to_owned(),
    }
}

/// 一覧の場所のうち、`daemon:` を外した同じ場所もあるもの（`daemon:X` と `X`）は `X` だけにする（前の版が覚えた
/// `daemon:` の形と、今の形が並ばないように）。
pub fn without_daemon_twins(list: Vec<String>) -> Vec<String> {
    let plain: std::collections::HashSet<String> = list
        .iter()
        .filter(|l| !l.starts_with("daemon:"))
        .cloned()
        .collect();
    list.into_iter()
        .filter(|l| {
            l.strip_prefix("daemon:")
                .is_none_or(|rest| !plain.contains(rest))
        })
        .collect()
}

/// `location` と、一覧で同じものとして扱う `daemon:` の付いた場所（外すときは両方外す）。
pub fn with_twin(location: &str) -> Vec<String> {
    if location.starts_with("daemon:") {
        vec![location.to_owned()]
    } else {
        vec![location.to_owned(), format!("daemon:{location}")]
    }
}

fn with_daemon(location: &str, on: bool) -> String {
    if on && !location.starts_with("daemon:") {
        format!("daemon:{location}")
    } else {
        location.to_owned()
    }
}

/// 開いたことを覚える（先頭に）。開いた日時も覚え、Settings > General で選んでいればブックマークにも入れる。
pub fn record(location: &Path) {
    // 覚えるのは開いた形のまま（リンクは解かない。ブックマークと同じ形にして、一覧に二度出さない。macOS 版と同じ）。
    let text = listed_form(
        &standardize(location).to_string_lossy(),
        crate::settings::bool(crate::settings::OPEN_THROUGH_DAEMON),
    );
    let mut list = recent();
    list.retain(|l| *l != text);
    list.insert(0, text.clone());
    crate::settings::set_string(RECENT, &serde_json::json!(list).to_string());
    let mut opened = opened_all();
    opened.insert(
        text.clone(),
        serde_json::json!(glib::real_time() / 1_000_000),
    );
    crate::settings::set_string(OPENED, &serde_json::Value::Object(opened).to_string());
    if crate::settings::bool(crate::settings::AUTO_BOOKMARK) {
        crate::bookmarks::register(&text);
    }
}

fn opened_all() -> serde_json::Map<String, serde_json::Value> {
    crate::settings::string(OPENED)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// 開いた日時のすべて（場所 → UNIX 時間）。たくさんの場所の日時を引くとき用（一度だけ読む）。
pub fn opened_all_at() -> std::collections::HashMap<String, i64> {
    opened_all()
        .into_iter()
        .filter_map(|(k, v)| v.as_f64().map(|t| (k, t as i64)))
        .collect()
}

/// 最近開いたもの・開いた日時から外す（Repositories の「一覧から外す」）。
/// `daemon:` の付いた同じ場所も外す（一覧には 1 つにまとめて出している。`without_daemon_twins`）。
pub fn forget(location: &str) {
    let twins = with_twin(location);
    let mut list = recent();
    list.retain(|l| !twins.contains(l));
    crate::settings::set_string(RECENT, &serde_json::json!(list).to_string());
    let mut opened = opened_all();
    let before = opened.len();
    opened.retain(|k, _| !twins.contains(k));
    if opened.len() != before {
        crate::settings::set_string(OPENED, &serde_json::Value::Object(opened).to_string());
    }
}

/// 開いている窓とタブ（次の起動で開き直す。macOS 版の `openWindows` と同じく、窓ごとにタブの順で並べる）。
const OPEN_WINDOWS: &str = "openWindows";

/// 開いている窓とタブの場所を覚える（`crate::tabs` が、開く・閉じる・並べ替えるたびに呼ぶ）。
pub fn save_open(groups: &[Vec<String>]) {
    crate::settings::set_string(OPEN_WINDOWS, &serde_json::json!(groups).to_string());
}

/// 前回の終わりに開いていた窓ごとのタブの場所（フォルダがなくなったもの、前の窓と重なるものは除く）。
pub fn restorable() -> Vec<Vec<String>> {
    let groups: Vec<Vec<String>> = crate::settings::string(OPEN_WINDOWS)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    groups
        .into_iter()
        .map(|group| {
            group
                .into_iter()
                .filter(|l| crate::bookmarks::is_repository(l))
                .filter(|l| seen.insert(l.clone()))
                .collect::<Vec<_>>()
        })
        .filter(|group| !group.is_empty())
        .collect()
}

/// `dir` がリポジトリの中か（`.git` を上に辿る。git は起動しない）。
pub fn inside_repository(dir: &Path) -> bool {
    dir.ancestors().any(|d| d.join(".git").exists())
}

/// File > Open Recent の中身（`win.open-recent` に場所を渡す）。最近のものが変わったら作り直す。
pub fn recent_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    fill(&menu);
    // メニューがある間だけ見る（閉じたタブ・Repositories のページのメニューは手放す）。
    let weak = menu.downgrade();
    crate::settings::connect_changed_while(&menu, move |key| {
        if key == RECENT
            && let Some(m) = weak.upgrade()
        {
            m.remove_all();
            fill(&m);
        }
    });
    menu
}

fn fill(menu: &gio::Menu) {
    let home = glib::home_dir();
    let list = without_daemon_twins(recent());
    if list.is_empty() {
        // 何もないときは押せない「None」（macOS 版と同じ）。
        menu.append(Some(&tr!("None")), Some("win.open-recent-none"));
    }
    for location in list.into_iter().take(MENU_ITEMS) {
        // ホームの下は ~ で短くする。
        let shown = match Path::new(&location).strip_prefix(&home) {
            Ok(rest) if !amonhen_remote::location::is_remote(&location) => {
                format!("~/{}", rest.display())
            }
            _ => location.clone(),
        };
        let item = gio::MenuItem::new(Some(&shown), None);
        item.set_action_and_target_value(Some("win.open-recent"), Some(&location.to_variant()));
        menu.append_item(&item);
    }
}

/// `~`・`~/…` をホームの下のパスにする。
fn expand_tilde(text: &str) -> PathBuf {
    if text == "~" {
        return glib::home_dir();
    }
    match text.strip_prefix("~/") {
        Some(rest) => glib::home_dir().join(rest),
        None => PathBuf::from(text),
    }
}

/// URL の最後の部分（`.git` を除く）。
fn name_from_url(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    let last = trimmed.rsplit(['/', ':']).next().unwrap_or_default();
    last.strip_suffix(".git").unwrap_or(last).to_owned()
}

/// 取得する範囲（macOS 版の `CloneSheet.Download`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Download {
    /// すべて（ふつうの `git clone`）。
    Everything,
    /// ファイルの中身は必要なときに取る（`--filter=blob:none`）。
    ContentsWhenNeeded,
    /// 最近の履歴だけ（`--depth`）。
    RecentHistory,
}

impl Download {
    const ALL: [Download; 3] = [
        Download::Everything,
        Download::ContentsWhenNeeded,
        Download::RecentHistory,
    ];

    fn title(self) -> String {
        match self {
            Download::Everything => tr!("Everything"),
            Download::ContentsWhenNeeded => tr!("File contents when needed"),
            Download::RecentHistory => tr!("Recent history only"),
        }
    }

    /// 選んだものの 1 文の説明。
    fn help(self) -> String {
        match self {
            Download::Everything => {
                tr!("The whole history and the contents of every file, as git does by default.")
            }
            Download::ContentsWhenNeeded => tr!(
                "Good for large repositories: file contents are fetched when a diff or file needs them."
            ),
            Download::RecentHistory => {
                tr!("Older history can be fetched later from the end of History.")
            }
        }
    }

    /// clone の選び方（`depth` は最近の履歴だけのときのコミットの数）。
    fn options(self, depth: u32, sparse: bool) -> amonhen_backend_cli::CloneOptions {
        amonhen_backend_cli::CloneOptions {
            filter: (self == Download::ContentsWhenNeeded).then(|| "blob:none".to_owned()),
            depth: (self == Download::RecentHistory).then_some(depth),
            sparse,
        }
    }
}

/// `git clone --sparse` が使える git の版。
const SPARSE_VERSION: (u32, u32) = (2, 27);

/// 選んだものの説明（小さく、薄く。選び直して文の長さが変わっても、シートの高さが変わらないよう 2 行分を取っておく）。
fn clone_help() -> gtk::Label {
    let label = crate::sheets::note("");
    label.set_lines(2);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_height_request(34);
    label.set_valign(gtk::Align::Start);
    label
}

/// File > Clone…: URL（gh があれば自分の GitHub のリポジトリからも選べる）と置き場所を決めて clone し、開く。
/// 取得する範囲（すべて・ファイルの中身は必要なときに・最近の履歴だけ）と、一部のフォルダだけを取り出すかも選べる
/// （docs/ux.md の「Clone」）。既定は git と同じく、すべてを取得してすべてを取り出す。
pub fn clone(parent: &gtk::Window, open: Rc<dyn Fn(PathBuf)>) {
    let parent = parent.clone();
    glib::spawn_future_local(async move {
        let sheet = Sheet::unnamed(Some(&parent), "Clone Repository", "");
        let url = gtk::Entry::builder()
            .placeholder_text(tr!(
                "https://github.com/owner/repo.git or git@host:owner/repo.git"
            ))
            .hexpand(true)
            // Enter で Clone（macOS 版と同じ）。
            .activates_default(true)
            .build();
        // 置き場所の初期値: 前に clone した場所、なければスキャンするフォルダーの最初（手元のもの）、なければ
        // ホーム。ホームの下は ~ で短く出す（macOS 版と同じ）。
        let folder_default = crate::settings::string(CLONE_FOLDER)
            .filter(|f| !f.is_empty())
            .or_else(|| {
                crate::bookmarks::data()
                    .scan_roots
                    .into_iter()
                    .map(|r| r.path)
                    .find(|p| !amonhen_remote::location::is_remote(p))
            })
            .unwrap_or_else(|| glib::home_dir().to_string_lossy().into_owned());
        let folder = gtk::Entry::builder()
            .text(crate::bookmarks::tilde(&folder_default))
            .hexpand(true)
            // Enter で Clone（macOS 版と同じ）。
            .activates_default(true)
            .build();
        let choose = gtk::Button::with_label(&tr!("Choose…"));
        let location = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        location.append(&folder);
        location.append(&choose);
        let name = gtk::Entry::builder()
            .placeholder_text(tr!("Taken from the URL"))
            .hexpand(true)
            // Enter で Clone（macOS 版と同じ）。
            .activates_default(true)
            .build();
        sheet.add(&row("URL:", &url));
        // gh があれば、自分のリポジトリから選べる（読み終えたら一覧に入れる）。
        if crate::github::available() {
            let mine = gtk::DropDown::from_strings(&[&tr!("Choose from My GitHub Repositories")]);
            sheet.add(&row("", &mine));
            let (mine_w, url_w, name_w) = (mine.downgrade(), url.downgrade(), name.downgrade());
            glib::spawn_future_local(async move {
                let repos = gio::spawn_blocking(|| {
                    let out = crate::github::gh_command()?
                        .args([
                            "repo",
                            "list",
                            "--json",
                            "nameWithOwner,url",
                            "--limit",
                            "200",
                        ])
                        .output()
                        .ok()?;
                    let list: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).ok()?;
                    let mut repos: Vec<(String, String)> = list
                        .iter()
                        .filter_map(|r| {
                            Some((
                                r["nameWithOwner"].as_str()?.to_owned(),
                                r["url"].as_str()?.to_owned(),
                            ))
                        })
                        .collect();
                    repos.sort_by_key(|r| r.0.to_lowercase());
                    Some(repos)
                })
                .await
                .ok()
                .flatten()
                .unwrap_or_default();
                let Some(mine) = mine_w.upgrade() else { return };
                let mut titles = vec![tr!("Choose from My GitHub Repositories")];
                if repos.is_empty() {
                    titles.push(tr!("None found"));
                }
                titles.extend(repos.iter().map(|r| r.0.clone()));
                let refs: Vec<&str> = titles.iter().map(String::as_str).collect();
                mine.set_model(Some(&gtk::StringList::new(&refs)));
                mine.connect_selected_notify(move |d| {
                    let i = d.selected() as usize;
                    if i == 0 || repos.is_empty() {
                        return;
                    }
                    if let (Some(url), Some(name)) = (url_w.upgrade(), name_w.upgrade()) {
                        url.set_text(&format!("{}.git", repos[i - 1].1));
                        name.set_text("");
                    }
                });
            });
        }
        sheet.add(&row(&tr!("Location:"), &location));
        sheet.add(&row(&tr!("Folder Name:"), &name));
        // 取得する範囲と、最近の履歴だけなら何コミットか（既定は 100）。下に選んだものの説明。
        let titles: Vec<String> = Download::ALL.iter().map(|d| d.title()).collect();
        let refs: Vec<&str> = titles.iter().map(String::as_str).collect();
        let download = gtk::DropDown::from_strings(&refs);
        // 1（いちばん新しいコミットだけ）・決まった数・「その他…」（好きな数を尋ねる。決めた数は項目にして選ぶ）。
        let depths: Rc<std::cell::RefCell<Vec<u32>>> = Rc::new(std::cell::RefCell::new(
            std::iter::once(1)
                .chain(crate::shallow::more_choices().into_iter().map(|c| c.1))
                .collect(),
        ));
        let depth_titles = gtk::StringList::new(&[]);
        for &d in depths.borrow().iter() {
            depth_titles.append(&crate::shallow::commits_title(d));
        }
        depth_titles.append(&tr!("Other…"));
        let depth = gtk::DropDown::new(Some(depth_titles.clone()), gtk::Expression::NONE);
        depth.set_selected(1);
        let depth_chosen = Rc::new(std::cell::Cell::new(100u32));
        {
            let (depths, chosen, titles, w) = (
                depths.clone(),
                depth_chosen.clone(),
                depth_titles.clone(),
                sheet.window.downgrade(),
            );
            depth.connect_selected_notify(move |d| {
                let i = d.selected() as usize;
                let count = depths.borrow().len();
                if i < count {
                    chosen.set(depths.borrow()[i]);
                    return;
                }
                // 「その他…」: 選び直しを戻してから尋ねる。
                let back = depths
                    .borrow()
                    .iter()
                    .position(|&x| x == chosen.get())
                    .unwrap_or(0);
                d.set_selected(back as u32);
                let (d, depths, chosen, titles, w) = (
                    d.clone(),
                    depths.clone(),
                    chosen.clone(),
                    titles.clone(),
                    w.clone(),
                );
                glib::spawn_future_local(async move {
                    let Some(commits) = crate::shallow::ask_count(
                        w.upgrade().as_ref(),
                        &tr!("Recent History"),
                        &tr!("How many recent commits to fetch (git clone --depth)."),
                        &tr!("Choose"),
                        chosen.get(),
                    )
                    .await
                    else {
                        return;
                    };
                    let at = depths.borrow().iter().position(|&x| x == commits);
                    let at = match at {
                        Some(at) => at,
                        None => {
                            // 数の順に入れる。
                            let mut list = depths.borrow_mut();
                            let at = list.partition_point(|&x| x < commits);
                            list.insert(at, commits);
                            titles.splice(at as u32, 0, &[&crate::shallow::commits_title(commits)]);
                            at
                        }
                    };
                    chosen.set(commits);
                    d.set_selected(at as u32);
                });
            });
        }
        let download_line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        download_line.append(&download);
        download_line.append(&depth);
        sheet.add(&row(&tr!("Download:"), &download_line));
        let download_help = clone_help();
        sheet.add(&row("", &download_help));
        // 一部のフォルダだけ取り出す（git 2.27 から）。下に説明か、使えなければ ⚠ と理由。
        let sparse = gtk::CheckButton::with_label(&tr!("Check out only some folders"));
        sheet.add(&row("", &sparse));
        let sparse_help = clone_help();
        sheet.add(&row("", &sparse_help));
        let sparse_unavailable: Rc<std::cell::RefCell<Option<String>>> = Rc::default();
        let update: Rc<dyn Fn()> = {
            let (download, depth, download_help, sparse, sparse_help, unavailable) = (
                download.downgrade(),
                depth.downgrade(),
                download_help.downgrade(),
                sparse.downgrade(),
                sparse_help.downgrade(),
                sparse_unavailable.clone(),
            );
            Rc::new(move || {
                let (Some(download), Some(depth), Some(dh), Some(sparse), Some(sh)) = (
                    download.upgrade(),
                    depth.upgrade(),
                    download_help.upgrade(),
                    sparse.upgrade(),
                    sparse_help.upgrade(),
                ) else {
                    return;
                };
                let choice = Download::ALL[(download.selected() as usize).min(2)];
                depth.set_visible(choice == Download::RecentHistory);
                dh.set_text(&choice.help());
                match unavailable.borrow().as_deref() {
                    Some(reason) => {
                        sparse.set_sensitive(false);
                        sparse.set_active(false);
                        sh.set_text(&format!("⚠ {reason}"));
                        sh.remove_css_class("dim-label");
                        sh.add_css_class("warning");
                    }
                    None => {
                        sparse.set_sensitive(true);
                        sh.set_text(&tr!("Only the files at the top are checked out; right after the clone, choose a folder to check out."));
                        sh.remove_css_class("warning");
                        sh.add_css_class("dim-label");
                    }
                }
            })
        };
        let u = update.clone();
        download.connect_selected_notify(move |_| u());
        // 見た目の確認用の起動（`AMONHEN_CLONE_SHEET=history,sparse` など）では、その選び方で出す。
        let preset = std::env::var("AMONHEN_CLONE_SHEET").unwrap_or_default();
        if preset.contains("contents") {
            download.set_selected(1);
        }
        if preset.contains("history") {
            download.set_selected(2);
        }
        if preset.contains("sparse") {
            sparse.set_active(true);
        }
        update();
        // この機械の git の版を裏で読み、古ければ止める（読めなければ使えるとみなす。clone のときに確かめる）。
        {
            let (unavailable, update) = (sparse_unavailable.clone(), update.clone());
            glib::spawn_future_local(async move {
                let version = gio::spawn_blocking(amonhen_backend_cli::git_version)
                    .await
                    .ok()
                    .flatten();
                if let Some((a, b, c)) = version
                    && (a, b) < SPARSE_VERSION
                {
                    unavailable.replace(Some(tr!(
                        "Needs git 2.27 or later. This computer’s git is {version}.",
                        version = format!("{a}.{b}.{c}")
                    )));
                    update();
                }
            });
        }
        // 置き場所はアプリの列で選ぶシートで（フォルダを作れる。macOS 版の `chooseLocation`）。
        let (f, w) = (folder.downgrade(), sheet.window.downgrade());
        choose.connect_clicked(move |_| {
            let (Some(f), Some(w)) = (f.upgrade(), w.upgrade()) else {
                return;
            };
            let request = crate::filepicker::Request {
                title: tr!("Clone Location"),
                message: tr!("Choose the folder to clone into."),
                kind: crate::filepicker::Kind::Folder,
                initial: f.text().to_string(),
                new_folder: true,
                ..Default::default()
            };
            glib::spawn_future_local(async move {
                if let Some(picked) =
                    crate::filepicker::choose(Some(&w), request, crate::filepicker::Source::Local)
                        .await
                {
                    f.set_text(&crate::bookmarks::tilde(&picked.path));
                }
            });
        });
        sheet.buttons("Clone", false, None);
        url.grab_focus();
        if sheet.run().await != Response::Ok {
            return;
        }
        let source = url.text().trim().to_owned();
        let folder_name = match name.text().trim() {
            "" => name_from_url(&source),
            n => n.to_owned(),
        };
        let parent_dir = folder.text().trim().to_owned();
        if source.is_empty() || folder_name.is_empty() || parent_dir.is_empty() {
            parent.error_bell();
            return;
        }
        let parent_dir = expand_tilde(&parent_dir);
        let dest = parent_dir.join(&folder_name);
        if dest.exists() {
            crate::sheets::inform(
                Some(&parent),
                &tr!("{path} already exists", path = dest.display()),
                &tr!("Choose a different name or location."),
            )
            .await;
            return;
        }
        crate::settings::set_string(CLONE_FOLDER, &parent_dir.to_string_lossy());
        let choice = Download::ALL[(download.selected() as usize).min(2)];
        let options = choice.options(
            depth_chosen.get(),
            sparse.is_sensitive() && sparse.is_active(),
        );
        run(&parent, source, dest, options, open).await;
    });
}

/// 進み具合の小さな窓。
fn progress_window(parent: &gtk::Window, text: &str) -> gtk::Window {
    let spinner = gtk::Spinner::builder().spinning(true).build();
    let label = gtk::Label::new(Some(text));
    label.set_wrap(true);
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    content.set_margin_top(16);
    content.set_margin_bottom(16);
    content.set_margin_start(16);
    content.set_margin_end(16);
    content.append(&spinner);
    content.append(&label);
    let window = gtk::Window::builder()
        .title("Clone")
        .modal(true)
        .transient_for(parent)
        .deletable(false)
        .resizable(false)
        .child(&content)
        .build();
    window.present();
    window
}

/// clone している間は、進み具合の小さな窓を出す。`options.sparse` なら、clone の後で取り出すフォルダを選んで
/// もらってから開く（取り消したら根のファイルだけのまま。フォルダは Settings の「取得とフォルダ」で足せる）。
async fn run(
    parent: &gtk::Window,
    url: String,
    dest: PathBuf,
    options: amonhen_backend_cli::CloneOptions,
    open: Rc<dyn Fn(PathBuf)>,
) {
    let window = progress_window(parent, &tr!("Cloning {url}…", url = url));
    let d = dest.clone();
    let sparse = options.sparse;
    // 走っている操作に数える（Activity の印が回る。macOS 版と同じ）。
    crate::activity::begin();
    let result = gio::spawn_blocking(move || {
        amonhen_backend_cli::clone(&url, &d, &options).map_err(|e| e.to_string())
    })
    .await
    .unwrap_or_else(|_| Err(String::new()));
    crate::activity::end();
    window.destroy();
    match result {
        Ok(()) => {
            if sparse {
                choose_folder(parent, &dest).await;
            }
            open(dest)
        }
        Err(e) => {
            crate::sheets::inform(Some(parent), &tr!("Couldn’t clone"), &e).await;
        }
    }
}

/// 根のファイルだけを取り出した clone で、取り出すフォルダを HEAD の木から選び（取得とフォルダのページと同じ
/// シート）、取り出す。取り消しても、失敗しても戻る。
async fn choose_folder(parent: &gtk::Window, dest: &Path) {
    let Ok(backend) = amonhen_backend_gix::GixBackend::open(dest) else {
        return;
    };
    let repo = std::sync::Arc::new(amonhen_core::Repository::new(backend));
    let request = crate::filepicker::Request {
        title: tr!("Check Out Only Some Folders"),
        message: tr!(
            "Only the files at the top are checked out. Choose a folder to check out too; add more later in Settings > Fetching and Folders."
        ),
        kind: crate::filepicker::Kind::Folder,
        initial: "/".into(),
        ..Default::default()
    };
    let Some(folder) =
        crate::repoform::choose_tree_folder(Some(parent), request, repo.clone()).await
    else {
        return;
    };
    let window = progress_window(parent, &tr!("Checking out {folder}…", folder = folder));
    crate::activity::begin();
    let result = crate::reposettings::run_one(
        &repo,
        serde_json::json!({ "op": "sparseCheckout", "folders": [folder] }),
    )
    .await;
    crate::activity::end();
    window.destroy();
    if let Err(e) = result {
        crate::sheets::inform(
            Some(parent),
            &tr!("Couldn’t check out {folder}", folder = folder),
            &e,
        )
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::{main_of_worktree, with_daemon};

    #[test]
    fn clones_with_what_was_chosen_to_download() {
        use super::Download;
        let o = Download::Everything.options(100, false);
        assert_eq!((o.filter, o.depth, o.sparse), (None, None, false));
        let o = Download::ContentsWhenNeeded.options(100, true);
        assert_eq!(
            (o.filter.as_deref(), o.depth, o.sparse),
            (Some("blob:none"), None, true)
        );
        let o = Download::RecentHistory.options(250, false);
        assert_eq!((o.filter, o.depth), (None, Some(250)));
    }

    #[test]
    fn opens_through_the_daemon_in_the_daemon_form() {
        assert_eq!(with_daemon("/a/r", true), "daemon:/a/r");
        assert_eq!(
            with_daemon("ssh://me@h:22/a/r", true),
            "daemon:ssh://me@h:22/a/r"
        );
        assert_eq!(with_daemon("daemon:/a/r", true), "daemon:/a/r");
        assert_eq!(with_daemon("/a/r", false), "/a/r");
        assert_eq!(with_daemon("ssh://h/a/r", false), "ssh://h/a/r");
    }

    #[test]
    fn lists_a_repository_once_with_or_without_the_daemon() {
        use super::{listed_form, with_twin, without_daemon_twins};
        // 設定で付けた `daemon:` は覚えない。設定なしで選んだ `daemon:` は残す。
        assert_eq!(listed_form("daemon:ssh://h/~/r", true), "ssh://h/~/r");
        assert_eq!(listed_form("daemon:/a/r", true), "/a/r");
        assert_eq!(listed_form("daemon:/a/r", false), "daemon:/a/r");
        assert_eq!(listed_form("ssh://h/~/r", true), "ssh://h/~/r");
        let list = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            without_daemon_twins(list(&[
                "daemon:ssh://h/~/r",
                "ssh://h/~/r",
                "daemon:/only",
                "/a"
            ])),
            list(&["ssh://h/~/r", "daemon:/only", "/a"])
        );
        assert_eq!(with_twin("/a"), list(&["/a", "daemon:/a"]));
        assert_eq!(with_twin("daemon:/a"), list(&["daemon:/a"]));
    }

    #[test]
    fn finds_the_main_repository_of_a_worktree() {
        let dir = std::env::temp_dir().join(format!("amonhen-gtk-wt-{}", std::process::id()));
        let (main, wt, plain) = (dir.join("main"), dir.join("wt"), dir.join("plain"));
        std::fs::create_dir_all(main.join(".git/worktrees/wt")).unwrap();
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::create_dir_all(plain.join(".git")).unwrap();
        let gitdir = main.join(".git/worktrees/wt");
        std::fs::write(wt.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();
        assert_eq!(main_of_worktree(&wt), Some(main.clone()));
        let inner = wt.join("src");
        std::fs::create_dir_all(&inner).unwrap();
        assert_eq!(super::top_of(&inner), wt);
        // `.`・`..` は字面で除く（リンクは解かない）。
        assert_eq!(super::standardize(&inner.join("../src/.")), inner);
        // メインの作業ツリーと、`.git` のないフォルダーは worktree ではない。
        assert_eq!(main_of_worktree(&plain), None);
        assert_eq!(main_of_worktree(&dir), None);
        // サブモジュール（`.git/modules/…` を指す）も worktree ではない。
        std::fs::write(wt.join(".git"), "gitdir: ../main/.git/modules/wt\n").unwrap();
        assert_eq!(main_of_worktree(&wt), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
