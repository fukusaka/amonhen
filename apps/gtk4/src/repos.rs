//! リポジトリを開く周り: 最近開いたもの（File > Open Recent）と、File > Clone…（macOS 版の `Clone.swift`）。
//! 最後に開いた日時（Repositories の一覧の Opened）も覚える。

use std::path::{Path, PathBuf};
use std::rc::Rc;

use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gio, glib};

use crate::sheets::{Response, Sheet, row};

const RECENT: &str = "recentRepositories";
const CLONE_FOLDER: &str = "cloneFolder";
const KEEP: usize = 10;
/// 最後に開いた日時（場所 → UNIX 時間。macOS 版の `repositoryOpenedAt`）。
const OPENED: &str = "repositoryOpenedAt";

/// 最近開いたリポジトリ（新しい順。手元のパス・`daemon:`・`ssh://` の場所）。
pub fn recent() -> Vec<String> {
    crate::settings::string(RECENT)
        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .unwrap_or_default()
}

/// 場所を、比べられる形にする（手元のパスは正規の形に）。
fn normalize(location: &Path) -> String {
    let text = location.to_string_lossy().into_owned();
    if amonhen_remote::location::is_remote(&text) {
        text
    } else {
        std::fs::canonicalize(location).map_or(text, |p| p.to_string_lossy().into_owned())
    }
}

/// 開いたことを覚える（先頭に）。開いた日時も覚え、Settings > General で選んでいればブックマークにも入れる。
pub fn record(location: &Path) {
    let text = normalize(location);
    let mut list = recent();
    list.retain(|l| *l != text);
    list.insert(0, text.clone());
    list.truncate(KEEP);
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

/// 最後に開いた日時（UNIX 時間）。覚える前に開いたものは None。
pub fn opened_at(location: &str) -> Option<i64> {
    opened_all()
        .get(location)
        .and_then(|v| v.as_f64())
        .map(|t| t as i64)
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
    for location in recent() {
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
            .build();
        let folder_default = crate::settings::string(CLONE_FOLDER)
            .filter(|f| !f.is_empty())
            .unwrap_or_else(|| glib::home_dir().to_string_lossy().into_owned());
        let folder = gtk::Entry::builder()
            .text(&folder_default)
            .hexpand(true)
            .build();
        let choose = gtk::Button::with_label(&tr!("Choose…"));
        let location = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        location.append(&folder);
        location.append(&choose);
        let name = gtk::Entry::builder()
            .placeholder_text(tr!("Taken from the URL"))
            .hexpand(true)
            .build();
        sheet.add(&row("URL:", &url));
        // gh があれば、自分のリポジトリから選べる（読み終えたら一覧に入れる）。
        if glib::find_program_in_path("gh").is_some() {
            let mine = gtk::DropDown::from_strings(&[&tr!("Choose from My GitHub Repositories")]);
            sheet.add(&row("", &mine));
            let (mine_w, url_w, name_w) = (mine.downgrade(), url.downgrade(), name.downgrade());
            glib::spawn_future_local(async move {
                let repos = gio::spawn_blocking(|| {
                    let out = amonhen_model::process::command("gh")
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
        let (f, w) = (folder.downgrade(), sheet.window.downgrade());
        choose.connect_clicked(move |_| {
            let (Some(f), Some(w)) = (f.upgrade(), w.upgrade()) else {
                return;
            };
            let dialog = gtk::FileDialog::builder()
                .title(tr!("Clone Location"))
                .modal(true)
                .build();
            dialog.set_initial_folder(Some(&gio::File::for_path(f.text().as_str())));
            glib::spawn_future_local(async move {
                if let Ok(picked) = dialog.select_folder_future(Some(&w)).await
                    && let Some(path) = picked.path()
                {
                    f.set_text(&path.to_string_lossy());
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
        let parent_dir = match parent_dir.strip_prefix("~/") {
            Some(rest) => glib::home_dir().join(rest),
            None => PathBuf::from(parent_dir),
        };
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
    let result = gio::spawn_blocking(move || {
        amonhen_backend_cli::clone(&url, &d).map_err(|e| e.to_string())
    })
    .await
    .unwrap_or_else(|_| Err(String::new()));
    window.destroy();
    match result {
        Ok(()) => open(dest),
        Err(e) => {
            crate::sheets::inform(Some(parent), &tr!("Couldn’t clone"), &e).await;
        }
    }
}
