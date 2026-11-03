//! アーカイブ（macOS 版の `Archive.swift`）: ある時点のファイルを zip・tar.gz・tar に書き出す（git archive）。
//! ファイルはいつも手元に書く（SSH 先のリポジトリでも、コアが中身をやりとりする）。何を外すかは git の
//! `.gitattributes`（`export-ignore`）に任せる。

use std::cell::RefCell;
use std::rc::Rc;

use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gio, glib};
use serde_json::json;

use crate::actions::Actions;
use crate::sheets::{self, Response, Sheet};

/// 書き出せる形（コアの `archive` の `format`）。並びは選ぶ欄の順。
const FORMATS: &[&str] = &["tar.gz", "zip", "tar"];

/// 前の選び方を覚えておく鍵（macOS 版の UserDefaults と同じ）。Linux では tar.gz を既定にする。
const FORMAT: &str = "archiveFormat";

/// `rev` の時点を書き出す。`label` は既定の名前に使う（タグ・ブランチの名前、なければ短い ID）。形・先頭の
/// フォルダ・書き出すフォルダを選んでもらい、partial clone で取ってくる中身があれば確かめてから、保存する場所を
/// 選んでもらう。書いたらファイルの窓で見せる。
pub fn export(actions: &Rc<Actions>, rev: String, label: String) {
    let actions = actions.clone();
    glib::spawn_future_local(async move {
        let Some(repo) = actions.repo() else { return };
        let parent = sheets::parent(&actions);
        let base = repo
            .paths()
            .workdir
            .and_then(|w| w.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let name = file_name(&if base.is_empty() {
            label.clone()
        } else {
            format!("{base}-{label}")
        });
        let sheet = Sheet::new(
            parent.as_ref(),
            &tr!("Export Archive"),
            &tr!(
                "The files of this commit, as git archive writes them; .gitattributes decides what is left out (export-ignore)."
            ),
        );
        let format = gtk::DropDown::from_strings(FORMATS);
        let remembered = crate::settings::string(FORMAT).unwrap_or_default();
        format.set_selected(FORMATS.iter().position(|f| *f == remembered).unwrap_or(0) as u32);
        sheet.add(&sheets::row(&tr!("Format:"), &format));
        let top = gtk::Entry::builder()
            .text(&name)
            .placeholder_text(tr!("None"))
            .width_chars(28)
            .build();
        sheet.add(&sheets::row(&tr!("Top folder:"), &top));
        // 書き出すフォルダ（選ばなければ全体）。その時点の木から選ぶ。
        let chosen: Rc<RefCell<Option<String>>> = Rc::default();
        let folder = gtk::Label::new(Some(&tr!("All files")));
        folder.set_xalign(0.0);
        folder.set_hexpand(true);
        let choose = gtk::Button::with_label(&tr!("Choose…"));
        let all = gtk::Button::with_label(&tr!("All Files"));
        all.set_visible(false);
        let folder_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        folder_row.append(&folder);
        folder_row.append(&choose);
        folder_row.append(&all);
        sheet.add(&sheets::row(&tr!("Folder:"), &folder_row));
        {
            let (repo, rev, chosen) = (repo.clone(), rev.clone(), chosen.clone());
            let (window, folder, all) = (sheet.window.downgrade(), folder.clone(), all.clone());
            choose.connect_clicked(move |_| {
                let request = crate::filepicker::Request {
                    title: tr!("Export One Folder"),
                    message: tr!("Folders in this commit."),
                    kind: crate::filepicker::Kind::Folder,
                    initial: "/".into(),
                    ..Default::default()
                };
                let (repo, rev, chosen) = (repo.clone(), rev.clone(), chosen.clone());
                let (window, folder, all) = (window.upgrade(), folder.clone(), all.clone());
                glib::spawn_future_local(async move {
                    let picked = crate::repoform::choose_tree_folder(
                        window.as_ref(),
                        request,
                        repo,
                        Some(rev),
                    )
                    .await;
                    if let Some(picked) = picked {
                        folder.set_text(&picked);
                        all.set_visible(true);
                        chosen.replace(Some(picked));
                    }
                });
            });
        }
        {
            let (chosen, folder) = (chosen.clone(), folder.clone());
            all.connect_clicked(move |b| {
                chosen.replace(None);
                folder.set_text(&tr!("All files"));
                b.set_visible(false);
            });
        }
        sheet.buttons(&tr!("Export…"), false, None);
        if sheet.run().await != Response::Ok {
            return;
        }
        let picked = FORMATS[(format.selected() as usize).min(FORMATS.len() - 1)];
        crate::settings::set_string(FORMAT, picked);
        let prefix = top.text().trim().trim_matches('/').to_owned();
        let paths: Vec<String> = chosen.borrow().iter().cloned().collect();
        // partial clone で手元にない中身があれば、先にまとめて取ることを確かめる。
        let need = sheets::run(
            &repo,
            json!({ "op": "archiveContents", "rev": rev, "paths": paths }),
        )
        .await;
        let count = need["info"]["count"].as_u64().unwrap_or(0);
        if count > 0 {
            let count = if need["info"]["more"] == true {
                format!("{count}+")
            } else {
                count.to_string()
            };
            let message = tr!(
                "This is a partial clone: the contents of {count} files in the archive aren’t here yet. They are fetched from the remote in one go before the archive is written.",
                count = count
            );
            if !actions
                .confirm(
                    &tr!("Fetch File Contents First?"),
                    &message,
                    &tr!("Fetch and Export"),
                )
                .await
            {
                return;
            }
        }
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(picked));
        filter.add_suffix(picked);
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let stem = if prefix.is_empty() { &name } else { &prefix };
        let dialog = gtk::FileDialog::builder()
            .title(tr!("Export Archive"))
            .initial_name(format!("{stem}.{picked}"))
            .filters(&filters)
            .modal(true)
            .build();
        let Ok(file) = dialog.save_future(parent.as_ref()).await else {
            return;
        };
        let Some(path) = file.path() else { return };
        let command = json!({
            "op": "archive",
            "rev": rev,
            "format": picked,
            "prefix": if prefix.is_empty() { String::new() } else { format!("{prefix}/") },
            "paths": paths,
            "file": path.to_string_lossy(),
        });
        let a = actions.clone();
        actions.perform_with(vec![command], move |_| {
            // 書いたファイルを、ファイルの窓で見せる。
            let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(&path)));
            let window = a.window();
            glib::spawn_future_local(async move {
                let _ = launcher
                    .open_containing_folder_future(window.as_ref())
                    .await;
            });
        });
    });
}

/// ファイル名と先頭のフォルダに使える名前（`/` と空白などは `-` に。macOS 版の `archiveName` と同じ）。
fn file_name(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_alphanumeric() || matches!(c, '.' | '_' | '-') {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches(|c| c == '-' || c == '.');
    if trimmed.is_empty() {
        "archive".to_owned()
    } else {
        trimmed.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makes_a_name_for_the_file_and_the_top_folder() {
        assert_eq!(file_name("app-feature/x y"), "app-feature-x-y");
        assert_eq!(file_name("app-v1.2"), "app-v1.2");
        assert_eq!(file_name("//"), "archive");
    }
}
