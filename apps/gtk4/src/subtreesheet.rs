//! subtree の Add・Pull・Push・Split のシート（macOS 版の `Subtrees.swift`）。サイドバー（Subtrees の見出しと
//! subtree の右クリック）と Repository のメニューが使う。答えをコアの `subtree` の命令にする。

use std::rc::Rc;

use amonhen_model::{Subtree, tr};
use gtk::glib;
use gtk::prelude::*;
use serde_json::json;

use crate::actions::Actions;
use crate::sheets::{self, Response, Sheet};

fn entry(value: &str, placeholder: &str) -> gtk::Entry {
    let e = gtk::Entry::new();
    e.set_text(value);
    e.set_placeholder_text(Some(placeholder));
    e.set_activates_default(true);
    e.set_width_chars(40);
    e
}

fn text(e: &gtk::Entry) -> String {
    e.text().trim().to_owned()
}

fn squash_box(on: bool) -> gtk::CheckButton {
    let b = gtk::CheckButton::with_label(&tr!("Squash its history into one commit (--squash)"));
    b.set_active(on);
    b
}

fn bell(actions: &Actions) {
    if let Some(window) = actions.window() {
        window.error_bell();
    }
}

fn fields(sheet: &Sheet, widgets: &[&gtk::Widget]) {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 8);
    for w in widgets {
        b.append(*w);
    }
    sheet.add(&b);
}

/// Add Subtree…: 別のリポジトリの ref を、このリポジトリのフォルダに取り込む。squash の既定は git と同じくオフ。
pub fn add(actions: &Rc<Actions>) {
    let actions = actions.clone();
    // この機械の git に subtree がなければ、メニューを押せない（`actions::enabled`。macOS 版と同じ）。
    glib::spawn_future_local(async move {
        let sheet = Sheet::new(
            sheets::parent(&actions).as_ref(),
            &tr!("Add Subtree"),
            &tr!(
                "Brings another repository’s branch into a folder of this one as a commit (git subtree add). Pull and Push later use the same repository and branch."
            ),
        );
        let url = entry("", &tr!("Repository URL"));
        let rev = entry("", &tr!("Branch or tag"));
        let dir = entry("", &tr!("Folder"));
        let squash = squash_box(false);
        fields(
            &sheet,
            &[
                url.upcast_ref(),
                rev.upcast_ref(),
                dir.upcast_ref(),
                squash.upcast_ref(),
            ],
        );
        let d = dir.downgrade();
        url.connect_changed(move |url| {
            let Some(dir) = d.upgrade() else { return };
            let name = crate::submodulesheet::default_path(&url.text());
            let hint = if name.is_empty() { tr!("Folder") } else { name };
            dir.set_placeholder_text(Some(&hint));
        });
        sheet.buttons(&tr!("Add"), false, None);
        url.grab_focus();
        if sheet.run().await != Response::Ok {
            return;
        }
        let (u, r) = (text(&url), text(&rev));
        let mut d = text(&dir);
        if d.is_empty() {
            d = crate::submodulesheet::default_path(&u);
        }
        if u.is_empty() || r.is_empty() || d.is_empty() {
            return bell(&actions);
        }
        actions.perform(json!({
            "op": "subtree",
            "action": { "kind": "add", "dir": d, "url": u, "ref": r, "squash": squash.is_active() },
        }));
    });
}

/// Pull…（`push` なら Push…）: 前に使った相手を入れておく。Pull の squash は前の選び方。
pub fn pull_or_push(actions: &Rc<Actions>, subtree: &Subtree, push: bool) {
    let (actions, subtree) = (actions.clone(), subtree.clone());
    glib::spawn_future_local(async move {
        let (title, message) = if push {
            (
                format!("Push {}", subtree.dir),
                tr!(
                    "Sends the commits that changed this folder to the repository’s branch, as that repository’s own history (git subtree push)."
                ),
            )
        } else {
            (
                format!("Pull {}", subtree.dir),
                tr!(
                    "Merges the repository’s branch into this folder (git subtree pull). Conflicts stop it like a merge."
                ),
            )
        };
        let sheet = Sheet::new(sheets::parent(&actions).as_ref(), &title, &message);
        let url = entry(
            subtree.url.as_deref().unwrap_or_default(),
            &tr!("Repository URL"),
        );
        let known = if push {
            subtree.push_ref.as_ref().or(subtree.ref_.as_ref())
        } else {
            subtree.ref_.as_ref()
        };
        let rev = entry(
            known.map(String::as_str).unwrap_or_default(),
            &tr!("Branch or tag"),
        );
        let squash = squash_box(subtree.squash.unwrap_or(false));
        if push {
            fields(&sheet, &[url.upcast_ref(), rev.upcast_ref()]);
        } else {
            fields(
                &sheet,
                &[url.upcast_ref(), rev.upcast_ref(), squash.upcast_ref()],
            );
        }
        sheet.buttons(if push { "Push" } else { "Pull" }, false, None);
        if url.text().is_empty() {
            url.grab_focus();
        } else {
            rev.grab_focus();
        }
        if sheet.run().await != Response::Ok {
            return;
        }
        let (u, r) = (text(&url), text(&rev));
        if u.is_empty() || r.is_empty() {
            return bell(&actions);
        }
        let mut action = json!({ "kind": if push { "push" } else { "pull" }, "dir": subtree.dir, "url": u, "ref": r });
        if !push {
            action["squash"] = json!(squash.is_active());
        }
        actions.perform(json!({ "op": "subtree", "action": action }));
    });
}

/// Split…: このフォルダの履歴だけのブランチを作る。
pub fn split(actions: &Rc<Actions>, subtree: &Subtree) {
    let (actions, subtree) = (actions.clone(), subtree.clone());
    glib::spawn_future_local(async move {
        let sheet = Sheet::new(
            sheets::parent(&actions).as_ref(),
            &format!("Split {}", subtree.dir),
            &tr!(
                "Creates a branch with only the history of this folder, its files at the top (git subtree split)."
            ),
        );
        let name = subtree.dir.rsplit('/').next().unwrap_or(&subtree.dir);
        let hint = format!("{name}-split");
        let branch = entry("", &hint);
        sheet.add(&branch);
        sheet.buttons(&tr!("Create"), false, None);
        branch.grab_focus();
        if sheet.run().await != Response::Ok {
            return;
        }
        let mut b = text(&branch);
        if b.is_empty() {
            b = hint;
        }
        actions.perform(json!({
            "op": "subtree",
            "action": { "kind": "split", "dir": subtree.dir, "branch": b },
        }));
    });
}
