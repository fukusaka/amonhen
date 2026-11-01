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

/// 開いたことを覚える（先頭に）。開いた日時も覚え、Settings > General で選んでいればブックマークにも入れる。
pub fn record(location: &Path) {
    // 覚えるのは開いた形のまま（リンクは解かない。ブックマークと同じ形にして、一覧に二度出さない。macOS 版と同じ）。
    let text = standardize(location).to_string_lossy().into_owned();
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
pub fn forget(location: &str) {
    let mut list = recent();
    list.retain(|l| l != location);
    crate::settings::set_string(RECENT, &serde_json::json!(list).to_string());
    let mut opened = opened_all();
    if opened.remove(location).is_some() {
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
    let m = menu.clone();
    crate::settings::connect_changed(move |key| {
        if key == RECENT {
            m.remove_all();
            fill(&m);
        }
    });
    menu
}

fn fill(menu: &gio::Menu) {
    let home = glib::home_dir();
    let list = recent();
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

/// File > Clone…: URL（gh があれば自分の GitHub のリポジトリからも選べる）と置き場所を決めて clone し、開く。
pub fn clone(parent: &gtk::Window, open: Rc<dyn Fn(PathBuf)>) {
    let parent = parent.clone();
    glib::spawn_future_local(async move {
        let sheet = Sheet::new(Some(&parent), "Clone Repository", "");
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
        run(&parent, source, dest, open).await;
    });
}

/// clone している間は、進み具合の小さな窓を出す。
async fn run(parent: &gtk::Window, url: String, dest: PathBuf, open: Rc<dyn Fn(PathBuf)>) {
    let spinner = gtk::Spinner::builder().spinning(true).build();
    let label = gtk::Label::new(Some(&tr!("Cloning {url}…", url = url)));
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
    let d = dest.clone();
    // 走っている操作に数える（Activity の印が回る。macOS 版と同じ）。
    crate::activity::begin();
    let result = gio::spawn_blocking(move || {
        amonhen_backend_cli::clone(&url, &d).map_err(|e| e.to_string())
    })
    .await
    .unwrap_or_else(|_| Err(String::new()));
    crate::activity::end();
    window.destroy();
    match result {
        Ok(()) => open(dest),
        Err(e) => {
            crate::sheets::inform(Some(parent), &tr!("Couldn’t clone"), &e).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::main_of_worktree;

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
