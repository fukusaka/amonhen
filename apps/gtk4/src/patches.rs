//! パッチ（macOS 版の `Patches.swift`。docs/ux.md の「7a. Patches」）: コミットをパッチに書き出す
//! （git format-patch）、パッチを当てる（メールの形は git am、ただの diff は git apply）。パッチのファイルは
//! いつも手元に置く（SSH 先のリポジトリでも、コアが中身をやりとりする）。

use std::path::PathBuf;
use std::rc::Rc;

use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use serde_json::{Value, json};

use crate::actions::Actions;
use crate::sheets::{self, Response, Sheet};

/// パッチのファイルとして受け取る拡張子（ドロップと、ファイルを選ぶ窓）。
pub const EXTENSIONS: &[&str] = &["patch", "diff", "mbox", "eml"];

/// 前の選び方を覚えておく鍵（macOS 版の UserDefaults と同じ）。
const SINGLE_FILE: &str = "patchSingleFile";
const COVER_LETTER: &str = "patchCoverLetter";

/// `path` がパッチのファイルの拡張子か。
fn is_patch(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// `oids`（古い順）をパッチにする。形（1 コミット 1 ファイルか 1 つの mbox か）・送り状・版を選んでもらってから、
/// 書き出す場所を選んでもらう。書いたらファイルの窓で見せる。
pub fn save(actions: &Rc<Actions>, oids: Vec<String>) {
    let Some(first) = oids.first().cloned() else {
        return;
    };
    let actions = actions.clone();
    glib::spawn_future_local(async move {
        let Some(repo) = actions.repo() else { return };
        let parent = sheets::parent(&actions);
        let n = oids.len();
        let title = if n == 1 {
            tr!("Save as Patch")
        } else {
            tr!("Save {count} Commits as Patches", count = n)
        };
        let sheet = Sheet::new(
            parent.as_ref(),
            &title,
            &tr!("Numbering, recipients and sign-offs follow git’s format.* settings."),
        );
        let format = gtk::DropDown::from_strings(&[
            tr!("One file per commit").as_str(),
            tr!("One mbox file").as_str(),
        ]);
        format.set_selected(u32::from(crate::settings::bool(SINGLE_FILE)));
        sheet.add(&sheets::row(&tr!("Format:"), &format));
        let cover = gtk::CheckButton::with_label(&tr!("Cover letter"));
        cover.set_active(crate::settings::bool(COVER_LETTER));
        sheet.add(&sheets::row("", &cover));
        let version = gtk::Entry::builder()
            .placeholder_text("1")
            .width_chars(4)
            .max_width_chars(4)
            .build();
        sheet.add(&sheets::row(&tr!("Version (-v):"), &version));
        sheet.buttons(&tr!("Save…"), false, None);
        if sheet.run().await != Response::Ok {
            return;
        }
        let single = format.selected() == 1;
        crate::settings::set_bool(SINGLE_FILE, single);
        crate::settings::set_bool(COVER_LETTER, cover.is_active());
        let mut command = json!({
            "op": "formatPatch", "oids": oids, "coverLetter": cover.is_active(),
        });
        if let Ok(v) = version.text().trim().parse::<u32>()
            && v > 1
        {
            command["reroll"] = json!(v);
        }
        if single {
            // ファイルの名前は最初のコミットの件名から（git format-patch と同じ考え）。
            let info = sheets::run(&repo, json!({ "op": "commitInfo", "oid": first })).await;
            let subject = info["info"]["message"]
                .as_str()
                .and_then(|m| m.lines().next())
                .unwrap_or("patches")
                .to_owned();
            let filter = gtk::FileFilter::new();
            filter.set_name(Some("mbox"));
            filter.add_suffix("mbox");
            let filters = gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&filter);
            let dialog = gtk::FileDialog::builder()
                .title(&title)
                .initial_name(format!("{}.mbox", file_name(&subject)))
                .filters(&filters)
                .modal(true)
                .build();
            let Ok(file) = dialog.save_future(parent.as_ref()).await else {
                return;
            };
            let Some(path) = file.path() else { return };
            command["file"] = json!(path.to_string_lossy());
        } else {
            let dialog = gtk::FileDialog::builder()
                .title(tr!(
                    "Choose the folder for the patches. Files with the same names are replaced."
                ))
                .accept_label(tr!("Save"))
                .modal(true)
                .build();
            let Ok(folder) = dialog.select_folder_future(parent.as_ref()).await else {
                return;
            };
            let Some(path) = folder.path() else { return };
            command["dir"] = json!(path.to_string_lossy());
        }
        let a = actions.clone();
        actions.perform_with(vec![command], move |reply| {
            // 書いたファイルを、ファイルの窓で見せる（最初のファイルを選んだ形で）。
            let Some(first) = reply["info"]["files"][0].as_str() else {
                return;
            };
            let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(first)));
            let window = a.window();
            glib::spawn_future_local(async move {
                let _ = launcher
                    .open_containing_folder_future(window.as_ref())
                    .await;
            });
        });
    });
}

/// git format-patch と同じ考えのファイル名（英数字と . _ のほかは - にまとめる）。
fn file_name(subject: &str) -> String {
    let mut out = String::new();
    for c in subject.chars() {
        if c.is_ascii_alphanumeric() || c == '.' || c == '_' {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches(|c| c == '-' || c == '.');
    if trimmed.is_empty() {
        "patches".to_owned()
    } else {
        trimmed.chars().take(52).collect()
    }
}

/// パッチのファイルを選んで当てる（Repository › Apply Patch…）。
pub fn choose_and_apply(actions: &Rc<Actions>) {
    let actions = actions.clone();
    glib::spawn_future_local(async move {
        let patches = gtk::FileFilter::new();
        patches.set_name(Some(&tr!(
            "Choose patch files (.patch, .diff, .mbox, .eml)."
        )));
        for e in EXTENSIONS {
            patches.add_suffix(e);
        }
        let all = gtk::FileFilter::new();
        all.set_name(Some("*"));
        all.add_pattern("*");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&patches);
        filters.append(&all);
        let dialog = gtk::FileDialog::builder()
            .title(tr!("Apply Patch…").trim_end_matches('…'))
            .filters(&filters)
            .modal(true)
            .build();
        let parent = sheets::parent(&actions);
        let Ok(files) = dialog.open_multiple_future(parent.as_ref()).await else {
            return;
        };
        let paths: Vec<PathBuf> = (0..files.n_items())
            .filter_map(|i| files.item(i)?.downcast::<gio::File>().ok()?.path())
            .collect();
        if !paths.is_empty() {
            apply(&actions, paths);
        }
    });
}

/// 窓にパッチのファイルを落としたら当てる（すべてがパッチの拡張子のときだけ受け取る）。
pub fn install_drop(window: &gtk::ApplicationWindow, actions: &Rc<Actions>) {
    let target = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
    let a = Rc::downgrade(actions);
    target.connect_drop(move |_, value, _, _| {
        let (Some(actions), Ok(list)) = (a.upgrade(), value.get::<gdk::FileList>()) else {
            return false;
        };
        let paths: Vec<PathBuf> = list.files().iter().filter_map(|f| f.path()).collect();
        if paths.is_empty() || !paths.iter().all(|p| is_patch(p)) {
            return false;
        }
        apply(&actions, paths);
        true
    });
    window.add_controller(target);
}

/// パッチのファイルを読んで、何を変えるかと当たるかを見せてから当てる。名前の順につなぐ（0001-… から）。
pub fn apply(actions: &Rc<Actions>, mut paths: Vec<PathBuf>) {
    paths.sort_by(|a, b| natural(&a.to_string_lossy(), &b.to_string_lossy()));
    let paths: Vec<String> = paths
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let actions = actions.clone();
    glib::spawn_future_local(async move {
        let Some(repo) = actions.repo() else { return };
        let reply = sheets::run(&repo, json!({ "op": "readPatch", "paths": paths })).await;
        let summary = &reply["info"];
        let patches = summary["patches"].as_array().cloned().unwrap_or_default();
        let empty = patches
            .iter()
            .all(|p| p["files"].as_array().is_none_or(|f| f.is_empty()));
        if reply["ok"].as_bool() != Some(true) || empty {
            let detail = match reply["error"].as_str() {
                Some(e) => e.to_owned(),
                None => tr!("There are no changes in the patch."),
            };
            let dialog = gtk::AlertDialog::builder()
                .message(tr!("Can’t Read the Patch"))
                .detail(detail)
                .modal(true)
                .build();
            let window = actions.window();
            let _ = dialog.choose_future(window.as_ref()).await;
            return;
        }
        confirm(&actions, summary, &patches, paths).await;
    });
}

/// 当てる前の確認: パッチごとの件名・作者・ファイル、当たるか、当て方の選び方。
async fn confirm(actions: &Rc<Actions>, summary: &Value, patches: &[Value], paths: Vec<String>) {
    let mailbox = summary["kind"].as_str() == Some("mailbox");
    let names: Vec<String> = paths
        .iter()
        .map(|p| {
            std::path::Path::new(p)
                .file_name()
                .map_or_else(|| p.clone(), |n| n.to_string_lossy().into_owned())
        })
        .collect();
    let branch = actions.head().unwrap_or_else(|| "HEAD".to_owned());
    let (title, what) = if mailbox {
        let title = match patches {
            [one] => tr!(
                "Apply “{subject}”?",
                subject = one["subject"].as_str().unwrap_or_default()
            ),
            _ => tr!("Apply {count} Patches?", count = patches.len()),
        };
        (
            title,
            tr!(
                "Each patch becomes a commit on {branch} (git am).",
                branch = branch
            ),
        )
    } else {
        let title = match &names[..] {
            [one] => tr!("Apply {name}?", name = one),
            _ => tr!("Apply {count} Patches?", count = names.len()),
        };
        (
            title,
            tr!("The changes go into the working tree (git apply)."),
        )
    };
    let sheet = Sheet::new(
        sheets::parent(actions).as_ref(),
        &title,
        &format!("{what} {}", sheets::can_undo()),
    );
    let list = gtk::Label::new(None);
    list.set_markup(&list_markup(patches));
    list.set_xalign(0.0);
    list.set_yalign(0.0);
    list.set_margin_start(6);
    list.set_margin_end(6);
    list.set_margin_top(4);
    list.set_margin_bottom(4);
    let scrolled = gtk::ScrolledWindow::builder()
        .child(&list)
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .min_content_width(440)
        .min_content_height(140)
        .build();
    scrolled.add_css_class("frame");
    sheet.add(&scrolled);
    let applies = summary["applies"].as_bool() == Some(true);
    let status = sheets::note(&if applies {
        tr!("Applies cleanly.")
    } else {
        tr!(
            "Doesn’t apply cleanly: {reason}",
            reason = first_line(summary["error"].as_str().unwrap_or_default())
        )
    });
    if !applies {
        status.remove_css_class("dim-label");
        status.add_css_class("warning");
    }
    sheet.add(&status);
    let three_way = gtk::CheckButton::with_label(&tr!("3-way merge where it doesn’t apply"));
    three_way.set_tooltip_text(Some(&tr!(
        "Leaves conflicts to resolve in Working Copy instead of stopping (--3way)."
    )));
    let index = gtk::CheckButton::with_label(&tr!("Stage the changes too (--index)"));
    if mailbox {
        // 初期値は git の am.threeWay（git の既定はオフ）。
        three_way.set_active(summary["threeWay"].as_bool() == Some(true));
    } else {
        sheet.add(&index);
    }
    sheet.add(&three_way);
    sheet.buttons("Apply", false, None);
    if sheet.run().await != Response::Ok {
        return;
    }
    if mailbox {
        actions.perform(json!({ "op": "am", "paths": paths, "threeWay": three_way.is_active() }));
    } else {
        actions.perform(json!({
            "op": "applyPatch", "paths": paths,
            "index": index.is_active(), "threeWay": three_way.is_active(),
        }));
    }
}

/// 確認のシートの一覧: パッチごとに件名と作者、その下にファイルと増減。
fn list_markup(patches: &[Value]) -> String {
    let esc = |s: &str| glib::markup_escape_text(s).to_string();
    let mut out = Vec::new();
    for patch in patches {
        let subject = patch["subject"].as_str().unwrap_or_default();
        let files = patch["files"].as_array().cloned().unwrap_or_default();
        if subject.is_empty() && files.is_empty() {
            continue;
        }
        if !subject.is_empty() {
            let mut line = format!("<b>{}</b>", esc(subject));
            if let Some(author) = patch["author"]
                .as_str()
                .or_else(|| patch["authorEmail"].as_str())
            {
                line.push_str(&format!("  <span alpha=\"60%\">{}</span>", esc(author)));
            }
            out.push(format!("<small>{line}</small>"));
        }
        for file in &files {
            let path = file["path"].as_str().unwrap_or_default();
            let name = match file["oldPath"].as_str() {
                Some(old) => format!("{old} → {path}"),
                None => path.to_owned(),
            };
            let change = if file["binary"].as_bool() == Some(true) {
                "bin".to_owned()
            } else {
                format!(
                    "+{} −{}",
                    file["added"].as_u64().unwrap_or(0),
                    file["removed"].as_u64().unwrap_or(0)
                )
            };
            let mark = match file["status"].as_str() {
                Some("added") => "A",
                Some("deleted") => "D",
                Some("renamed") => "R",
                Some("copied") => "C",
                _ => "M",
            };
            out.push(format!(
                "<small><tt>  {mark} {}  <span alpha=\"60%\">{change}</span></tt></small>",
                esc(&name)
            ));
        }
    }
    out.join("\n")
}

/// git の答えの最初の `error:` の行（なければ最初の行）。
fn first_line(text: &str) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    lines
        .iter()
        .find_map(|l| l.strip_prefix("error:").map(str::trim))
        .or_else(|| lines.first().copied())
        .unwrap_or_default()
        .to_owned()
}

/// 数字を数として比べる（`2.patch` が `10.patch` より前。Finder の並びと同じ考え）。
fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    fn chunks(s: &str) -> Vec<(bool, String)> {
        let mut out: Vec<(bool, String)> = Vec::new();
        for c in s.chars() {
            let digit = c.is_ascii_digit();
            match out.last_mut() {
                Some((d, text)) if *d == digit => text.push(c),
                _ => out.push((digit, c.to_string())),
            }
        }
        out
    }
    let (a, b) = (chunks(a), chunks(b));
    for (x, y) in a.iter().zip(&b) {
        let order = match (x.0, y.0) {
            (true, true) => {
                let (p, q) = (x.1.trim_start_matches('0'), y.1.trim_start_matches('0'));
                p.len().cmp(&q.len()).then_with(|| p.cmp(q))
            }
            _ => x.1.to_lowercase().cmp(&y.1.to_lowercase()),
        };
        if order.is_ne() {
            return order;
        }
    }
    a.len().cmp(&b.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_mbox_like_format_patch() {
        assert_eq!(file_name("Fix the bug: a/b (c)"), "Fix-the-bug-a-b-c");
        assert_eq!(file_name("日本語"), "patches");
        assert_eq!(file_name("v1.2_rc"), "v1.2_rc");
    }

    #[test]
    fn sorts_numbers_as_numbers() {
        let mut names = vec!["10-a.patch", "2-b.patch", "0001-c.patch"];
        names.sort_by(|a, b| natural(a, b));
        assert_eq!(names, ["0001-c.patch", "2-b.patch", "10-a.patch"]);
    }

    #[test]
    fn takes_the_error_line() {
        assert_eq!(
            first_line(
                "Checking patch a...\nerror: patch failed: a:1\nerror: a: patch does not apply"
            ),
            "patch failed: a:1"
        );
        assert_eq!(first_line("fatal: x"), "fatal: x");
    }
}
