//! bundle（macOS 版の `Bundle.swift`）: コミットを、git が clone や fetch できる 1 つのファイルにまとめる
//! （git bundle create）。ファイルはいつも手元に書く（SSH 先のリポジトリでも、コアが中身をやりとりする）。

use std::rc::Rc;

use amonhen_model::{RefKind, tr};
use gtk::prelude::*;
use gtk::{gio, glib};
use serde_json::json;

use crate::actions::Actions;
use crate::sheets::{self, Response, Sheet};

/// bundle に入れる、ref の名前（ブランチ・リモートブランチ・タグ）。ほかの種類は None。
pub fn full_ref(kind: RefKind, name: &str) -> Option<String> {
    match kind {
        RefKind::Local => Some(format!("refs/heads/{name}")),
        RefKind::Remote => Some(format!("refs/remotes/{name}")),
        RefKind::Tag => Some(format!("refs/tags/{name}")),
        _ => None,
    }
}

/// `refs`（空なら HEAD とすべてのブランチ・タグ）を bundle に書く。`names` は見せる名前、`label` は既定のファイル名
/// に使う。入れる範囲（全体か、ある基点より後か）を選んでもらい、入るコミットの数と partial clone で取ってくる
/// 中身を確かめてから、保存する場所を選んでもらう。書いたらファイルの窓で見せる。
pub fn save(actions: &Rc<Actions>, refs: Vec<String>, names: Vec<String>, label: String) {
    let actions = actions.clone();
    glib::spawn_future_local(async move {
        let Some(repo) = actions.repo() else { return };
        let parent = sheets::parent(&actions);
        let base = repo
            .paths()
            .workdir
            .and_then(|w| w.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let name = crate::archive::file_name(&if base.is_empty() {
            label.clone()
        } else {
            format!("{base}-{label}")
        });
        let what = if names.is_empty() {
            tr!("HEAD and all branches and tags")
        } else {
            names.join(", ")
        };
        let sheet = Sheet::new(
            parent.as_ref(),
            &tr!("Save as Bundle"),
            &tr!(
                "{what}, in one file that git can clone or fetch from (git bundle).",
                what = what
            ),
        );
        let everything = gtk::CheckButton::with_label(&tr!("All history"));
        let newer = gtk::CheckButton::with_label(&tr!("Only commits after:"));
        newer.set_group(Some(&everything));
        everything.set_active(true);
        let field = gtk::Entry::builder()
            .placeholder_text(tr!("Tag, branch or commit"))
            .width_chars(24)
            .sensitive(false)
            .build();
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        row.append(&newer);
        row.append(&field);
        sheet.add(&everything);
        sheet.add(&row);
        sheet.add(&sheets::note(&tr!(
            "Whoever receives the bundle needs that commit already."
        )));
        {
            let field = field.clone();
            newer.connect_toggled(move |b| {
                field.set_sensitive(b.is_active());
                if b.is_active() {
                    field.grab_focus();
                }
            });
        }
        sheet.buttons(&tr!("Save…"), false, None);
        if sheet.run().await != Response::Ok {
            return;
        }
        let after = field.text().trim().to_owned();
        let bases: Vec<String> = if newer.is_active() && !after.is_empty() {
            vec![after]
        } else {
            Vec::new()
        };
        let preview = sheets::run(
            &repo,
            json!({ "op": "bundlePreview", "refs": refs, "bases": bases }),
        )
        .await;
        if preview["ok"] != true {
            let error = preview["error"].as_str().unwrap_or_default();
            actions.alert(&tr!("Can’t Save the Bundle"), error).await;
            return;
        }
        let info = &preview["info"];
        let commits = info["commits"].as_u64().unwrap_or(0);
        if commits == 0 {
            actions
                .alert(
                    &tr!("Nothing to Save"),
                    &tr!("There are no commits to put in the bundle."),
                )
                .await;
            return;
        }
        if info["beyondShallow"] == true {
            actions
                .alert(
                    &tr!("Can’t Save the Bundle"),
                    &tr!(
                        "This shallow clone doesn’t have all the history the bundle needs. Fetch more history, or choose a newer base."
                    ),
                )
                .await;
            return;
        }
        let contents = info["contents"].as_u64().unwrap_or(0);
        if contents > 0 {
            let count = if info["more"] == true {
                format!("{contents}+")
            } else {
                contents.to_string()
            };
            let message = tr!(
                "This is a partial clone: the contents of {count} files in the bundle aren’t here yet. They are fetched from the remote in one go before the bundle is written.",
                count = count
            );
            if !actions
                .confirm(
                    &tr!("Fetch File Contents First?"),
                    &message,
                    &tr!("Fetch and Save"),
                )
                .await
            {
                return;
            }
        }
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("bundle"));
        filter.add_suffix("bundle");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title(tr!("Save as Bundle"))
            .initial_name(format!("{name}.bundle"))
            .filters(&filters)
            .modal(true)
            .build();
        let Ok(file) = dialog.save_future(parent.as_ref()).await else {
            return;
        };
        let Some(path) = file.path() else { return };
        let command = json!({
            "op": "bundle", "refs": refs, "bases": bases, "file": path.to_string_lossy(),
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
