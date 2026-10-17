//! サイドバーの項目の操作（右クリックのメニューと、ドラッグ＆ドロップ）。macOS 版の `SidebarViewController`
//! と同じ項目。

use std::rc::Rc;

use amonhen_model::RefKind;
use gtk::glib;
use serde_json::json;

use crate::actions::Actions;
use crate::menu::{self, Item};
use crate::sidebar::{self, Data};

/// 右クリックのメニュー。`head` はチェックアウト中のブランチ。
pub fn menu(actions: &Rc<Actions>, data: &Data, head: Option<&str>) -> Vec<Item> {
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
    let merge_rebase = |items: &mut Vec<Item>| {
        items.push(menu::action(
            format!("{name} を {current} に merge"),
            run(json!({ "op": "merge", "rev": name })),
        ));
        items.push(menu::action(
            format!("{current} を {name} の上に rebase"),
            run(json!({ "op": "rebase", "onto": name })),
        ));
    };
    match entry.kind {
        RefKind::Local => {
            let is_head = Some(name.as_str()) == head;
            if !is_head {
                items.push(menu::action(
                    format!("{name} をチェックアウト"),
                    run(json!({ "op": "checkout", "branch": name })),
                ));
                merge_rebase(&mut items);
            }
            // 上流があれば、そこへ push する。
            if let Some((remote, branch)) = entry.detail.as_deref().and_then(|u| u.split_once('/'))
            {
                items.push(menu::action(
                    format!("{name} を {remote}/{branch} に push"),
                    run(json!({
                        "op": "push", "remote": remote, "branch": name, "remoteBranch": branch,
                        "force": false, "setUpstream": false,
                    })),
                ));
            }
            items.push(Item::Separator);
            items.push(new_branch(actions, &name));
            items.push(new_tag(actions, &name));
            let (a, from) = (actions.clone(), name.clone());
            items.push(menu::action(
                format!("{name} の名前を変える…"),
                move || {
                    let (a, from) = (a.clone(), from.clone());
                    glib::spawn_future_local(async move {
                        if let Some(to) = a.ask("ブランチの名前を変える", &from, "変更").await
                        {
                            a.perform(json!({ "op": "renameBranch", "from": from, "to": to }));
                        }
                    });
                },
            ));
            if !is_head {
                items.push(menu::action(
                    format!("{name} を削除"),
                    run(json!({ "op": "deleteBranch", "name": name, "force": false })),
                ));
            }
        }
        RefKind::Remote => {
            items.push(menu::action(
                format!("{name} をローカルブランチとしてチェックアウト"),
                run(json!({ "op": "track", "remoteBranch": name })),
            ));
            merge_rebase(&mut items);
            items.push(Item::Separator);
            items.push(new_branch(actions, &name));
        }
        RefKind::Tag => {
            items.push(menu::action(
                format!("{name} を {current} に merge"),
                run(json!({ "op": "merge", "rev": name })),
            ));
            items.push(new_branch(actions, &name));
        }
        RefKind::Stash => {
            items.push(menu::action(
                format!("{name} を当てる"),
                run(json!({ "op": "stashApply", "name": name, "pop": false })),
            ));
            items.push(menu::action(
                format!("{name} を当てて消す（pop）"),
                run(json!({ "op": "stashApply", "name": name, "pop": true })),
            ));
            items.push(menu::action(
                format!("{name} を消す"),
                run(json!({ "op": "stashDrop", "name": name })),
            ));
        }
        RefKind::Submodule => {
            items.push(menu::action(
                format!("{name} を更新"),
                run(json!({ "op": "updateSubmodules", "paths": [name] })),
            ));
        }
    }
    items
}

/// `start` から新しいブランチを作って切り替える。
fn new_branch(actions: &Rc<Actions>, start: &str) -> Item {
    let (a, start) = (actions.clone(), start.to_owned());
    menu::action(
        format!("{start} から新しいブランチ…"),
        move || {
            let (a, start) = (a.clone(), start.clone());
            glib::spawn_future_local(async move {
                if let Some(name) = a.ask("新しいブランチ", "", "作成").await {
                    a.perform(json!({ "op": "createBranch", "name": name, "start": start, "checkout": true }));
                }
            });
        },
    )
}

/// `target` に新しいタグを付ける。
fn new_tag(actions: &Rc<Actions>, target: &str) -> Item {
    let (a, target) = (actions.clone(), target.to_owned());
    menu::action(format!("{target} に新しいタグ…"), move || {
        let (a, target) = (a.clone(), target.clone());
        glib::spawn_future_local(async move {
            if let Some(name) = a.ask("新しいタグ", "", "作成").await {
                a.perform(
                    json!({ "op": "createTag", "name": name, "target": target, "message": null }),
                );
            }
        });
    })
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
                format!("{dragged} を {to} に merge"),
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
                format!("{to} を {dragged} の上に rebase"),
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
                let push = |force: bool| {
                    json!({
                        "op": "push", "remote": remote, "branch": dragged, "remoteBranch": branch,
                        "force": force, "setUpstream": set_upstream && !force,
                    })
                };
                let (a, command) = (actions.clone(), push(false));
                items.push(menu::action(
                    format!("{dragged} を {remote}/{branch} に push"),
                    move || a.perform(command.clone()),
                ));
                let (a, command) = (actions.clone(), push(true));
                let message = format!(
                    "{remote}/{branch} を {dragged} で上書きします。リモートにしかないコミットは失われます。"
                );
                items.push(menu::action(
                    format!("{dragged} を {remote}/{branch} に強制 push"),
                    move || {
                        let (a, command, message) = (a.clone(), command.clone(), message.clone());
                        glib::spawn_future_local(async move {
                            if a.confirm("強制 push", &message, "強制 push").await {
                                a.perform(command);
                            }
                        });
                    },
                ));
            }
        }
        _ => {}
    }
    if !items.is_empty() {
        items.push(Item::Separator);
        items.push(menu::action("キャンセル", || {}));
    }
    items
}

/// Remotes の下の、リモートそのもの（`Remotes/origin`）。
fn is_remote_folder(data: &Data) -> bool {
    data.key
        .strip_prefix("Remotes/")
        .is_some_and(|rest| !rest.contains('/'))
}
