//! サイドバーの項目の操作（右クリックのメニューと、ドラッグ＆ドロップ）。項目と文言は macOS 版の
//! `SidebarViewController` と同じ。

use std::rc::Rc;

use amonhen_model::{RefKind, tr};
use gtk::glib;
use serde_json::json;

use crate::actions::Actions;
use crate::menu::{self, Item};
use crate::sidebar::{self, Data};
use crate::{sheets, titles};

/// サブモジュールを新しい窓で開く（作業ツリーからのパス）。
pub type OpenSubmodule = Rc<dyn Fn(&str)>;

/// 右クリックのメニュー。`head` はチェックアウト中のブランチ。
pub fn menu(
    actions: &Rc<Actions>,
    data: &Data,
    head: Option<&str>,
    open_submodule: &OpenSubmodule,
    range_diff: &crate::rangediff::Opener,
) -> Vec<Item> {
    let sidebar::Item::Ref(entry) = &data.item else {
        return Vec::new();
    };
    let name = entry.name.clone();
    let current = head.unwrap_or("HEAD").to_owned();
    let mut items = Vec::new();
    let run = |command: serde_json::Value| {
        let actions = actions.clone();
        move || actions.perform(command.clone())
    };
    let merge = |items: &mut Vec<Item>| {
        let (a, n, c) = (actions.clone(), name.clone(), current.clone());
        items.push(menu::action(titles::merge(&name, &current), move || {
            sheets::integrate(&a, &n, &c, false)
        }));
    };
    let rebase = |items: &mut Vec<Item>| {
        let (a, n, c) = (actions.clone(), name.clone(), current.clone());
        items.push(menu::action(titles::rebase(&current, &name), move || {
            sheets::integrate(&a, &n, &c, true)
        }));
    };
    match entry.kind {
        RefKind::Local => {
            let is_head = Some(name.as_str()) == head;
            if !is_head {
                let (a, n) = (actions.clone(), name.clone());
                items.push(menu::action(titles::checkout(&name), move || {
                    sheets::checkout(&a, sheets::CheckoutTarget::Branch(n.clone()))
                }));
                merge(&mut items);
                rebase(&mut items);
                items.push(Item::Separator);
            }
            items.push(new_branch(actions, &name));
            items.push(new_tag(actions, &name));
            let (r, n) = (range_diff.clone(), name.clone());
            items.push(menu::action(
                tr!("Compare with Previous Version"),
                move || r.previous(Some(n.clone())),
            ));
            let (a, from) = (actions.clone(), name.clone());
            items.push(menu::action(format!("Rename {name}…"), move || {
                let (a, from) = (a.clone(), from.clone());
                glib::spawn_future_local(async move {
                    if let Some(to) = a.ask("Rename Branch", "", &from, &tr!("Change")).await {
                        a.perform(json!({ "op": "renameBranch", "from": from, "to": to }));
                    }
                });
            }));
            if !is_head {
                let (a, n) = (actions.clone(), name.clone());
                items.push(menu::action(titles::delete(&name), move || {
                    sheets::delete_branches(&a, vec![n.clone()])
                }));
            }
        }
        RefKind::Remote => {
            let (a, n) = (actions.clone(), name.clone());
            items.push(menu::action(titles::checkout_as_local(&name), move || {
                sheets::checkout(&a, sheets::CheckoutTarget::Track(n.clone()))
            }));
            merge(&mut items);
            rebase(&mut items);
            items.push(Item::Separator);
            items.push(new_branch(actions, &name));
        }
        RefKind::Tag => {
            merge(&mut items);
            items.push(new_branch(actions, &name));
        }
        // Stashes の画面の一覧の行と同じメニュー。
        RefKind::Stash => items.extend(crate::stashes::menu(
            actions,
            &name,
            entry.detail.as_deref(),
        )),
        RefKind::Submodule => {
            items.push(menu::action(
                format!("Update {name}"),
                run(json!({ "op": "updateSubmodules", "paths": [name] })),
            ));
            let (open, path) = (open_submodule.clone(), name.clone());
            items.push(menu::action(
                format!("Open {name} in New Window"),
                move || open(&path),
            ));
        }
    }
    items
}

/// `start` から新しいブランチを作って切り替える。
pub fn new_branch(actions: &Rc<Actions>, start: &str) -> Item {
    let (a, start) = (actions.clone(), start.to_owned());
    menu::action(tr!("New Branch from {name}…", name = start), move || {
        ask_new_branch(&a, &start);
    })
}

pub fn ask_new_branch(actions: &Rc<Actions>, start: &str) {
    let (a, start) = (actions.clone(), start.to_owned());
    glib::spawn_future_local(async move {
        let message = tr!("Name of the branch to create from {start}", start = start);
        if let Some(name) = a
            .ask("New Branch", &message, "", &tr!("Create and Switch"))
            .await
        {
            a.perform(
                json!({ "op": "createBranch", "name": name, "start": start, "checkout": true }),
            );
        }
    });
}

/// `target` に新しいタグを付ける。
pub fn new_tag(actions: &Rc<Actions>, target: &str) -> Item {
    let (a, target) = (actions.clone(), target.to_owned());
    menu::action(tr!("New Tag at {name}…", name = target), move || {
        ask_new_tag(&a, &target);
    })
}

pub fn ask_new_tag(actions: &Rc<Actions>, target: &str) {
    let (a, target) = (actions.clone(), target.to_owned());
    glib::spawn_future_local(async move {
        let message = tr!("Name of the tag for {target}", target = target);
        if let Some(name) = a.ask("New Tag", &message, "", &tr!("Create")).await {
            a.perform(
                json!({ "op": "createTag", "name": name, "target": target, "message": null }),
            );
        }
    });
}

/// ドラッグ＆ドロップ: ブランチの上なら merge・rebase、リモート（かリモートブランチ）の上なら push。
/// `dragged` は `local:<名前>` か `other:<名前>`。
pub fn drop_menu(
    actions: &Rc<Actions>,
    dragged: &str,
    target: &Data,
    head: Option<&str>,
) -> Vec<Item> {
    let Some((kind, dragged)) = dragged.split_once(':') else {
        return Vec::new();
    };
    let (local, dragged) = (kind == "local", dragged.to_owned());
    let mut items = Vec::new();
    match &target.item {
        sidebar::Item::Ref(e) if e.kind == RefKind::Local && e.name != dragged => {
            let to = e.name.clone();
            let a = actions.clone();
            let (d, t) = (dragged.clone(), to.clone());
            // checkout していなくても、fast-forward で済むならブランチだけ進める。
            items.push(menu::action(titles::merge(&dragged, &to), move || {
                sheets::confirm_then(
                    &a,
                    tr!("Merge {rev} into {target}?", rev = d, target = t),
                    sheets::can_undo(),
                    "Merge".to_owned(),
                    false,
                    vec![json!({ "op": "mergeInto", "target": t, "rev": d })],
                );
            }));
            // 落とした先が今のブランチでなければ、先にチェックアウトする。
            let mut sequence = Vec::new();
            if Some(to.as_str()) != head {
                sequence.push(json!({ "op": "checkout", "branch": to }));
            }
            sequence.push(json!({ "op": "rebase", "onto": dragged }));
            let (a, d, t) = (actions.clone(), dragged.clone(), to.clone());
            items.push(menu::action(titles::rebase(&to, &dragged), move || {
                if sequence.len() == 1 {
                    sheets::integrate(&a, &d, &t, true);
                } else {
                    sheets::confirm_then(
                        &a,
                        tr!("Rebase {target} onto {rev}?", target = t, rev = d),
                        format!(
                            "{} {}",
                            tr!("Checks out {branch} first.", branch = t),
                            sheets::can_undo()
                        ),
                        "Rebase".to_owned(),
                        false,
                        sequence.clone(),
                    );
                }
            }));
        }
        // push できるのはローカルブランチだけ。リモートの見出し（フォルダ）か、リモートブランチの上。
        item if local => {
            let remote_target = match item {
                sidebar::Item::Folder if is_remote_folder(target) => {
                    Some((target.title.clone(), dragged.clone(), true))
                }
                sidebar::Item::Ref(e) if e.kind == RefKind::Remote => e
                    .name
                    .split_once('/')
                    .map(|(r, b)| (r.to_owned(), b.to_owned(), false)),
                _ => None,
            };
            if let Some((remote, branch, set_upstream)) = remote_target {
                for force in [false, true] {
                    let command = json!({
                        "op": "push", "remote": remote, "branch": dragged, "remoteBranch": branch,
                        "force": force, "setUpstream": set_upstream && !force,
                    });
                    let target = format!("{remote}/{branch}");
                    let title = if force {
                        titles::force_push(&dragged, &target)
                    } else {
                        titles::push(&dragged, &target)
                    };
                    let (a, d) = (actions.clone(), dragged.clone());
                    items.push(menu::action(title, move || {
                        sheets::push_to(&a, &d, &target, command.clone(), force)
                    }));
                }
            }
        }
        _ => {}
    }
    items
}

/// Remotes の下の、リモートそのもの（`Remotes/origin`）。
fn is_remote_folder(data: &Data) -> bool {
    data.key
        .strip_prefix("Remotes/")
        .is_some_and(|rest| !rest.contains('/'))
}
