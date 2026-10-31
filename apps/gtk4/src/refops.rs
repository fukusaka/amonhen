//! サイドバーの項目の操作（右クリックのメニューと、ドラッグ＆ドロップ）。項目と文言は macOS 版の
//! `SidebarViewController` と同じ。

use std::rc::Rc;

use amonhen_model::{RefKind, tr};
use gtk::glib;
use serde_json::json;

use crate::actions::Actions;
use crate::menu::{self, Item};
use crate::sidebar::{self, Data};

/// サブモジュールを新しい窓で開く（作業ツリーからのパス）。
pub type OpenSubmodule = Rc<dyn Fn(&str)>;

/// 右クリックのメニュー。`head` はチェックアウト中のブランチ。
pub fn menu(
    actions: &Rc<Actions>,
    data: &Data,
    head: Option<&str>,
    open_submodule: &OpenSubmodule,
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
        items.push(menu::action(
            format!("Merge {name} into {current}"),
            run(json!({ "op": "merge", "rev": name })),
        ));
    };
    let rebase = |items: &mut Vec<Item>| {
        items.push(menu::action(
            format!("Rebase {current} onto {name}"),
            run(json!({ "op": "rebase", "onto": name })),
        ));
    };
    match entry.kind {
        RefKind::Local => {
            let is_head = Some(name.as_str()) == head;
            if !is_head {
                items.push(menu::action(
                    format!("Checkout {name}"),
                    run(json!({ "op": "checkout", "branch": name })),
                ));
                merge(&mut items);
                rebase(&mut items);
                items.push(Item::Separator);
            }
            items.push(new_branch(actions, &name));
            items.push(new_tag(actions, &name));
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
                items.push(menu::action(
                    tr!("Delete {name}", name = name),
                    run(json!({ "op": "deleteBranch", "name": name, "force": false })),
                ));
            }
        }
        RefKind::Remote => {
            items.push(menu::action(
                format!("Checkout {name} as Local Branch"),
                run(json!({ "op": "track", "remoteBranch": name })),
            ));
            merge(&mut items);
            rebase(&mut items);
            items.push(Item::Separator);
            items.push(new_branch(actions, &name));
        }
        RefKind::Tag => {
            merge(&mut items);
            items.push(new_branch(actions, &name));
        }
        RefKind::Stash => {
            items.push(menu::action(
                format!("Apply {name}"),
                run(json!({ "op": "stashApply", "name": name, "pop": false })),
            ));
            items.push(menu::action(
                format!("Pop {name}"),
                run(json!({ "op": "stashApply", "name": name, "pop": true })),
            ));
            items.push(menu::action(
                format!("Drop {name}"),
                run(json!({ "op": "stashDrop", "name": name })),
            ));
        }
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
            items.push(menu::action(
                format!("Merge {dragged} into {to}"),
                move || {
                    a.perform(json!({ "op": "mergeInto", "target": t, "rev": d }));
                },
            ));
            // 落とした先が今のブランチでなければ、先にチェックアウトする。
            let mut sequence = Vec::new();
            if Some(to.as_str()) != head {
                sequence.push(json!({ "op": "checkout", "branch": to }));
            }
            sequence.push(json!({ "op": "rebase", "onto": dragged }));
            let a = actions.clone();
            items.push(menu::action(
                format!("Rebase {to} onto {dragged}"),
                move || {
                    a.perform_sequence(sequence.clone());
                },
            ));
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
                    let title = if force { "Force Push" } else { "Push" };
                    let a = actions.clone();
                    items.push(menu::action(
                        format!("{title} {dragged} to {remote}/{branch}"),
                        move || a.perform(command.clone()),
                    ));
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
