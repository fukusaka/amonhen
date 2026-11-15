//! subtree の Add・Pull・Push・Split のシート（macOS 版の `Subtrees.swift`）。サイドバー（Subtrees の見出しと
//! subtree の右クリック）と Repository のメニューが使う。答えをコアの `subtree` の命令にする。
//! 取り込み・送り出し・分けるのは書き込む先の worktree（HEAD とその作業ツリー）なので、シートは選んだときの
//! worktree の名前を書き、答えもそこで実行する（シートを出している間に書き込む先を替えても）。

use std::rc::Rc;

use amonhen_model::{Subtree, tr};
use gtk::glib;
use gtk::prelude::*;
use serde_json::{Value, json};

use crate::actions::Actions;
use crate::sheets::{self, Response, Target};

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

/// 選んだときの worktree で `command` を実行する。
fn perform(actions: &Rc<Actions>, target: Target, command: Value) {
    actions.perform_in(target.repo, vec![command], |_| {});
}

fn bell(actions: &Actions) {
    if let Some(window) = actions.window() {
        window.error_bell();
    }
}

/// Add Subtree…: 別のリポジトリの ref を、このリポジトリのフォルダに取り込む。squash の既定は git と同じくオフ。
pub fn add(actions: &Rc<Actions>) {
    let Some(target) = Target::current(actions) else {
        return;
    };
    let actions = actions.clone();
    // この機械の git に subtree がなければ、メニューを押せない（`actions::enabled`。macOS 版と同じ）。
    glib::spawn_future_local(async move {
        let sheet = target.sheet(
            sheets::parent(&actions).as_ref(),
            &tr!("Add Subtree"),
            &tr!(
                "Brings another repository’s branch into a folder of this one as a commit (git subtree add). Pull and Push later use the same repository and branch."
            ),
        );
        let url = entry("", "");
        let rev = entry("", "");
        let dir = entry("", "");
        let squash = squash_box(false);
        sheets::form(
            &sheet,
            &[
                (tr!("Repository URL"), url.upcast_ref()),
                (tr!("Branch or tag"), rev.upcast_ref()),
                (tr!("Folder"), dir.upcast_ref()),
                (String::new(), squash.upcast_ref()),
            ],
        );
        // フォルダは空なら URL から決める。その名前を欄の薄い文字で見せる。
        let d = dir.downgrade();
        url.connect_changed(move |url| {
            let Some(dir) = d.upgrade() else { return };
            dir.set_placeholder_text(Some(&crate::submodulesheet::default_path(&url.text())));
        });
        let add = sheet.buttons(&tr!("Add"), false, None);
        sheets::require(&[&url, &rev], &add);
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
        perform(
            &actions,
            target,
            json!({
                "op": "subtree",
                "action": { "kind": "add", "dir": d, "url": u, "ref": r, "squash": squash.is_active() },
            }),
        );
    });
}

/// Pull…（`push` なら Push…）: 前に使った相手を入れておく。Pull の squash は前の選び方。
pub fn pull_or_push(actions: &Rc<Actions>, subtree: &Subtree, push: bool) {
    let Some(target) = Target::current(actions) else {
        return;
    };
    let (actions, subtree) = (actions.clone(), subtree.clone());
    glib::spawn_future_local(async move {
        let (title, message) = if push {
            (
                tr!("Push {dir}", dir = &subtree.dir),
                tr!(
                    "Sends the commits that changed this folder to the repository’s branch, as that repository’s own history (git subtree push)."
                ),
            )
        } else {
            (
                tr!("Pull {dir}", dir = &subtree.dir),
                tr!(
                    "Merges the repository’s branch into this folder (git subtree pull). Conflicts stop it like a merge."
                ),
            )
        };
        let sheet = target.sheet(sheets::parent(&actions).as_ref(), &title, &message);
        let url = entry(subtree.url.as_deref().unwrap_or_default(), "");
        let known = if push {
            subtree.push_ref.as_ref().or(subtree.ref_.as_ref())
        } else {
            subtree.ref_.as_ref()
        };
        let rev = entry(known.map(String::as_str).unwrap_or_default(), "");
        let squash = squash_box(subtree.squash.unwrap_or(false));
        let mut rows = vec![
            (tr!("Repository URL"), url.upcast_ref::<gtk::Widget>()),
            (tr!("Branch or tag"), rev.upcast_ref()),
        ];
        if !push {
            rows.push((String::new(), squash.upcast_ref()));
        }
        sheets::form(&sheet, &rows);
        let ok = sheet.buttons(if push { "Push" } else { "Pull" }, false, None);
        sheets::require(&[&url, &rev], &ok);
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
        perform(
            &actions,
            target,
            json!({ "op": "subtree", "action": action }),
        );
    });
}

/// Split…: このフォルダの履歴だけのブランチを作る。
pub fn split(actions: &Rc<Actions>, subtree: &Subtree) {
    let Some(target) = Target::current(actions) else {
        return;
    };
    let (actions, subtree) = (actions.clone(), subtree.clone());
    glib::spawn_future_local(async move {
        let sheet = target.sheet(
            sheets::parent(&actions).as_ref(),
            &tr!("Split {dir}", dir = &subtree.dir),
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
        perform(
            &actions,
            target,
            json!({
                "op": "subtree",
                "action": { "kind": "split", "dir": subtree.dir, "branch": b },
            }),
        );
    });
}
