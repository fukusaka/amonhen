//! submodule を足す・取り出す・外す・URL や追うブランチを変えるシート（macOS 版の `Submodules.swift`）。サイドバー
//! （Submodules の見出しと submodule の右クリック）と Repository のメニューが使う。どれも答えをコアの命令にする。
//! submodule は worktree ごとのもの（作業ツリーと index）なので、シートは選んだときの worktree の名前を書き、
//! 答えもそこで実行する（シートを出している間に書き込む先を替えても）。

use std::rc::Rc;

use amonhen_model::{SubmoduleState, tr};
use gtk::glib;
use gtk::prelude::*;
use serde_json::{Value, json};

use crate::actions::Actions;
use crate::sheets::{self, Response, Target};

/// URL の最後の部分から `.git` を除いたもの（git が submodule の場所の既定にするもの）。
pub fn default_path(url: &str) -> String {
    let last = url
        .trim()
        .trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()
        .unwrap_or_default();
    last.strip_suffix(".git").unwrap_or(last).to_owned()
}

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

/// 空なら null（git の既定）。
fn optional(s: String) -> Value {
    if s.is_empty() { Value::Null } else { json!(s) }
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

/// Add Submodule…: URL・場所・追うブランチ（なくてもよい）。
pub fn add(actions: &Rc<Actions>) {
    let Some(target) = Target::current(actions) else {
        return;
    };
    let actions = actions.clone();
    glib::spawn_future_local(async move {
        let sheet = target.sheet(
            sheets::parent(&actions).as_ref(),
            &tr!("Add Submodule"),
            &tr!(
                "Clones the repository into a folder of this one and records it in .gitmodules (git submodule add). Commit to keep it."
            ),
        );
        let url = entry("", &tr!("Repository URL"));
        let path = entry("", &tr!("Folder"));
        let branch = entry("", &tr!("Branch to follow (optional)"));
        let fields = gtk::Box::new(gtk::Orientation::Vertical, 8);
        for e in [&url, &path, &branch] {
            fields.append(e);
        }
        sheet.add(&fields);
        let p = path.downgrade();
        url.connect_changed(move |url| {
            let Some(path) = p.upgrade() else { return };
            let name = default_path(&url.text());
            let hint = if name.is_empty() { tr!("Folder") } else { name };
            path.set_placeholder_text(Some(&hint));
        });
        sheet.buttons(&tr!("Add"), false, None);
        url.grab_focus();
        if sheet.run().await != Response::Ok {
            return;
        }
        let u = text(&url);
        let mut p = text(&path);
        if p.is_empty() {
            p = default_path(&u);
        }
        if u.is_empty() || p.is_empty() {
            return bell(&actions);
        }
        perform(
            &actions,
            target,
            json!({
                "op": "changeSubmodule",
                "change": { "kind": "add", "url": u, "path": p, "branch": optional(text(&branch)) },
            }),
        );
    });
}

/// Update…: git の既定（記録したコミット、中の submodule には入らない）から変えて取り出す。`paths` が空ならすべて。
pub fn update(actions: &Rc<Actions>, paths: Vec<String>) {
    let Some(target) = Target::current(actions) else {
        return;
    };
    let actions = actions.clone();
    glib::spawn_future_local(async move {
        let title = match &paths[..] {
            [path] => tr!("Update {path}", path = path),
            _ => tr!("Update All Submodules"),
        };
        let sheet = target.sheet(
            sheets::parent(&actions).as_ref(),
            &title,
            &tr!("Checks out the commit recorded in this repository, unless you choose otherwise."),
        );
        let recursive = gtk::CheckButton::with_label(&tr!(
            "Also update the submodules inside it (--recursive)"
        ));
        let remote = gtk::CheckButton::with_label(&tr!(
            "Use the latest commit of the branch it follows (--remote)"
        ));
        sheet.add(&recursive);
        sheet.add(&remote);
        sheet.buttons("Update", false, None);
        if sheet.run().await != Response::Ok {
            return;
        }
        perform(
            &actions,
            target,
            json!({
                "op": "updateSubmodules",
                "paths": paths,
                "recursive": recursive.is_active(),
                "remote": remote.is_active(),
            }),
        );
    });
}

/// Run in Each Submodule…: それぞれの submodule の中で、シェルで命令を動かす（git submodule foreach）。出力は
/// Activity に残る（macOS 版と同じ）。
pub fn foreach(actions: &Rc<Actions>) {
    let Some(target) = Target::current(actions) else {
        return;
    };
    let actions = actions.clone();
    glib::spawn_future_local(async move {
        let sheet = target.sheet(
            sheets::parent(&actions).as_ref(),
            &tr!("Run in Each Submodule"),
            &tr!(
                "Runs the command with the shell in each submodule’s folder (git submodule foreach). Its output is in Activity."
            ),
        );
        let command = entry("", &tr!("Command"));
        let recursive =
            gtk::CheckButton::with_label(&tr!("Also in the submodules inside them (--recursive)"));
        sheet.add(&command);
        sheet.add(&recursive);
        sheet.buttons(&tr!("Run"), false, None);
        command.grab_focus();
        if sheet.run().await != Response::Ok {
            return;
        }
        let c = text(&command);
        if c.is_empty() {
            return bell(&actions);
        }
        perform(
            &actions,
            target,
            json!({
                "op": "changeSubmodule",
                "change": { "kind": "foreach", "command": c, "recursive": recursive.is_active() },
            }),
        );
    });
}

/// 中のリポジトリのデータを `.git/modules` に移す（git submodule absorbgitdirs）。確かめずに（戻す必要がない）。
pub fn absorb(actions: &Rc<Actions>, path: &str) {
    let Some(target) = Target::current(actions) else {
        return;
    };
    perform(
        actions,
        target,
        json!({ "op": "changeSubmodule", "change": { "kind": "absorb", "paths": [path] } }),
    );
}

/// Edit URL…: `.gitmodules` の URL を変え、このリポジトリの設定にも写す。
pub fn set_url(actions: &Rc<Actions>, submodule: &SubmoduleState) {
    let Some(target) = Target::current(actions) else {
        return;
    };
    let (actions, submodule) = (actions.clone(), submodule.clone());
    glib::spawn_future_local(async move {
        let sheet = target.sheet(
            sheets::parent(&actions).as_ref(),
            &tr!("Edit URL of {path}", path = &submodule.path),
            &tr!(
                "Changes the URL in .gitmodules and copies it to this repository’s settings (git submodule set-url). Commit .gitmodules to share it."
            ),
        );
        let url = entry(
            submodule.url.as_deref().unwrap_or_default(),
            &tr!("Repository URL"),
        );
        sheet.add(&url);
        sheet.buttons(&tr!("Save"), false, None);
        url.grab_focus();
        if sheet.run().await != Response::Ok {
            return;
        }
        let u = text(&url);
        if u.is_empty() {
            return bell(&actions);
        }
        perform(
            &actions,
            target,
            json!({
                "op": "changeSubmodule",
                "change": { "kind": "setUrl", "path": submodule.path, "url": u },
            }),
        );
    });
}

/// Branch to Follow…: Update で「追うブランチの最新」を選んだときに進める先。空なら既定（リモートの HEAD）。
pub fn set_branch(actions: &Rc<Actions>, submodule: &SubmoduleState) {
    let Some(target) = Target::current(actions) else {
        return;
    };
    let (actions, submodule) = (actions.clone(), submodule.clone());
    glib::spawn_future_local(async move {
        let sheet = target.sheet(
            sheets::parent(&actions).as_ref(),
            &tr!("Branch to Follow for {path}", path = &submodule.path),
            &tr!(
                "Updating with “the latest commit of the branch it follows” moves to this branch’s latest commit. Leave it empty for the remote’s default branch."
            ),
        );
        let branch = entry(
            submodule.branch.as_deref().unwrap_or_default(),
            &tr!("Default branch"),
        );
        sheet.add(&branch);
        sheet.buttons(&tr!("Save"), false, None);
        branch.grab_focus();
        if sheet.run().await != Response::Ok {
            return;
        }
        perform(
            &actions,
            target,
            json!({
                "op": "changeSubmodule",
                "change": { "kind": "setBranch", "path": submodule.path, "branch": optional(text(&branch)) },
            }),
        );
    });
}

/// Remove…: 作業ツリーと `.gitmodules` から外す。git の既定どおり `.git/modules/<name>` は残し、消すかは選ぶ。
pub fn remove(actions: &Rc<Actions>, submodule: &SubmoduleState) {
    let Some(target) = Target::current(actions) else {
        return;
    };
    let (actions, submodule) = (actions.clone(), submodule.clone());
    glib::spawn_future_local(async move {
        let sheet = target.sheet(
            sheets::parent(&actions).as_ref(),
            &tr!("Remove {name}?", name = &submodule.path),
            &tr!(
                "Removes the folder and its entry in .gitmodules, and stages that. Commit to keep it."
            ),
        );
        let delete_git_dir = gtk::CheckButton::with_label(&tr!(
            "Also delete its repository data (.git/modules/{name})",
            name = &submodule.name
        ));
        sheet.add(&delete_git_dir);
        sheet.add(&sheets::note(&tr!(
            "Kept, it brings back the submodule’s commits and stashes when you add it again, including ones not pushed."
        )));
        sheet.buttons(&tr!("Remove"), false, None);
        if sheet.run().await != Response::Ok {
            return;
        }
        perform(
            &actions,
            target,
            json!({
                "op": "changeSubmodule",
                "change": { "kind": "remove", "path": submodule.path, "deleteGitDir": delete_git_dir.is_active() },
            }),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_the_folder_from_the_url_as_git_does() {
        assert_eq!(default_path("https://example.com/a/lib.git"), "lib");
        assert_eq!(default_path("git@example.com:lib.git/"), "lib");
        assert_eq!(default_path("../lib"), "lib");
    }
}
