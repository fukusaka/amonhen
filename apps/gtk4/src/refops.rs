//! サイドバーの項目の操作（右クリックのメニューと、ドラッグ＆ドロップ）。項目と文言は macOS 版の
//! `SidebarViewController` と同じ。

use std::rc::Rc;

use amonhen_model::{RefKind, tr};
use gtk::glib;
use gtk::prelude::*;
use serde_json::json;

use crate::actions::Actions;
use crate::menu::{self, Item};
use crate::reffacts::TagState;
use crate::sidebar::{self, Data, Known};
use crate::{sheets, titles};

/// サブモジュールを新しい窓で開く（作業ツリーからのパス）。
pub type OpenSubmodule = Rc<dyn Fn(&str)>;

/// 右クリックのメニュー。`head` はチェックアウト中のブランチ。
pub fn menu(
    actions: &Rc<Actions>,
    data: &Data,
    known: &Known,
    head: Option<&str>,
    open_submodule: &OpenSubmodule,
    range_diff: &crate::rangediff::Opener,
) -> Vec<Item> {
    let sidebar::Item::Ref(entry) = &data.item else {
        return remote_menu(actions, data);
    };
    // 複数選んだ行の上で開いたら、選んだものをまとめて扱う。
    if known.selected.len() > 1 {
        return several(actions, known, head);
    }
    let name = entry.name.clone();
    let current = head.unwrap_or("HEAD").to_owned();
    let mut items = Vec::new();
    // 名前をコピー（ターミナルや PR に貼るため。リモートブランチは origin/ を付けたまま）。
    if entry.kind != RefKind::Stash {
        let title = match entry.kind {
            RefKind::Local | RefKind::Remote => tr!("Copy Branch Name"),
            RefKind::Tag => tr!("Copy Tag Name"),
            _ => tr!("Copy Name"),
        };
        items.push(copy(title, name.clone()));
        items.push(Item::Separator);
    }
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
            items.extend(sync_items(actions, entry, is_head));
            items.extend(favorite_items(actions, &name, known));
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
            items.push(Item::Separator);
            items.extend(tag_items(actions, std::slice::from_ref(&name), known));
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

/// Remotes の見出しとその下のリモート（docs/ux.md の 4.5 のリモート: リポジトリの設定のページと同じ操作）。
/// 見出しは Add Remote…、リモートは Rename…・Edit URL…（どちらも同じシート）・Prune・Remove…。
fn remote_menu(actions: &Rc<Actions>, data: &Data) -> Vec<Item> {
    if let Some(remote) = &data.remote {
        let edit = |title: String| {
            let (a, r) = (actions.clone(), remote.clone());
            menu::action(title, move || crate::remotesheet::edit(&a, &r))
        };
        let (a, r) = (actions.clone(), remote.clone());
        let prune = menu::action("Prune", move || {
            a.perform(json!({ "op": "changeRemote", "change": { "kind": "prune", "name": r } }))
        });
        let (a, r) = (actions.clone(), remote.clone());
        let remove = menu::action(tr!("Remove…"), move || {
            crate::remotesheet::confirm_remove(&a, &r)
        });
        return vec![
            edit(tr!("Rename…")),
            edit(tr!("Edit URL…")),
            prune,
            Item::Separator,
            remove,
        ];
    }
    if matches!(data.item, sidebar::Item::Group) && data.title == "Remotes" {
        let a = actions.clone();
        return vec![menu::action(tr!("Add Remote…"), move || {
            crate::remotesheet::add(&a)
        })];
    }
    Vec::new()
}

/// 上流と合わせる（最後の Fetch の時点）。上流がなければ Publish、遅れているだけなら Fast-Forward（チェックアウト
/// しないで進める）、先にあるだけなら Push。チェックアウト中のブランチはツールバーの Pull・Push で。分かれていれば
/// チェックアウトして merge か rebase をする（上の Checkout）。
fn sync_items(actions: &Rc<Actions>, entry: &amonhen_model::RefEntry, is_head: bool) -> Vec<Item> {
    let name = entry.name.clone();
    let mut items = Vec::new();
    match &entry.detail {
        Some(upstream) if !entry.gone => {
            if is_head || (entry.ahead == 0) == (entry.behind == 0) {
                return items;
            }
            let a = actions.clone();
            if entry.ahead > 0 {
                items.push(menu::action(titles::push(&name, upstream), move || {
                    sheets::push(&a, Some(name.clone()), false, None)
                }));
            } else {
                let title = tr!(
                    "Fast-forward {branch} to {upstream}?",
                    branch = name,
                    upstream = upstream
                );
                let command = json!({ "op": "fastForward", "branch": name });
                items.push(menu::action(
                    format!("Fast-Forward {name} to {upstream}…"),
                    move || {
                        sheets::confirm_then(
                            &a,
                            title.clone(),
                            sheets::can_undo(),
                            "Fast-Forward".to_owned(),
                            false,
                            vec![command.clone()],
                        )
                    },
                ));
            }
        }
        _ => {
            let a = actions.clone();
            items.push(menu::action(format!("Publish {name}…"), move || {
                sheets::publish(&a, &name, None)
            }));
        }
    }
    items.push(Item::Separator);
    items
}

/// `text` をクリップボードに入れる項目。
fn copy(title: String, text: String) -> Item {
    menu::action(title, move || {
        if let Some(display) = gtk::gdk::Display::default() {
            display.clipboard().set_text(&text);
        }
    })
}

/// 複数選んだブランチ・タグのメニュー: 名前をコピー、ローカルブランチだけならまとめてお気に入り・アーカイブ・削除。
fn several(actions: &Rc<Actions>, known: &Known, head: Option<&str>) -> Vec<Item> {
    let selected = &known.selected;
    let names: Vec<String> = selected.iter().map(|e| e.name.clone()).collect();
    let mut items = vec![copy(
        tr!("Copy {count} Names", count = names.len()),
        names.join("\n"),
    )];
    let locals: Vec<String> = selected
        .iter()
        .filter(|e| e.kind == RefKind::Local)
        .map(|e| e.name.clone())
        .collect();
    if locals.len() == selected.len() {
        items.push(Item::Separator);
        // お気に入りは、全部入っていれば外す、そうでなければ（まだのものを）入れる。
        let all_pinned = locals.iter().all(|n| known.pinned.contains(n));
        let commands = favorite_commands(&locals, known, all_pinned);
        let a = actions.clone();
        items.push(menu::action(
            if all_pinned {
                tr!("Remove {count} from Favorites", count = locals.len())
            } else {
                tr!("Add {count} to Favorites", count = locals.len())
            },
            move || a.perform_sequence(commands.clone()),
        ));
        let all_archived = locals
            .iter()
            .all(|n| known.groups.as_ref().is_some_and(|g| g.is_archived(n)));
        let commands = archive_commands(&locals, all_archived);
        let a = actions.clone();
        items.push(menu::action(
            if all_archived {
                tr!("Unarchive {count} Branches", count = locals.len())
            } else {
                tr!("Archive {count} Branches", count = locals.len())
            },
            move || a.perform_sequence(commands.clone()),
        ));
        // 今のブランチは消せない（git が断る）。
        if locals.iter().all(|n| Some(n.as_str()) != head) {
            let a = actions.clone();
            items.push(menu::action(
                tr!("Delete {count} Branches…", count = locals.len()),
                move || sheets::delete_branches(&a, locals.clone()),
            ));
        }
    }
    let tags: Vec<String> = selected
        .iter()
        .filter(|e| e.kind == RefKind::Tag)
        .map(|e| e.name.clone())
        .collect();
    if tags.len() == selected.len() {
        items.push(Item::Separator);
        items.extend(tag_items(actions, &tags, known));
    }
    items
}

/// タグの push と削除。fetch のときに読んだリモートの様子に合わせて、まだないリモートにだけ Push、同じ名前の別の
/// タグがあるリモートには Replace（置き換え）、あるリモートにだけ「… から削除」を出す。まだ読んでいないリモートには
/// 全部出し、そう断る。手元の削除は Undo できる。リモートでの置き換え・削除は戻せない（もう一度 push する
/// しかない）ので、名前を並べて確かめる（赤いボタン）。
fn tag_items(actions: &Rc<Actions>, names: &[String], known: &Known) -> Vec<Item> {
    let tags = known.tags.as_ref();
    let list = |names: &[String]| format!("{}\n\n", names.join("\n"));
    let (mut pushes, mut replaces, mut deletes) = (Vec::new(), Vec::new(), Vec::new());
    for remote in known.remote_names() {
        let checked = tags.is_some_and(|t| t.checked(&remote).is_some());
        let state = |name: &String| tags.and_then(|t| t.state(name, &remote));
        let pick = |want: fn(Option<TagState>) -> bool| -> Vec<String> {
            names.iter().filter(|n| want(state(n))).cloned().collect()
        };
        let to_push = if checked {
            pick(|s| s == Some(TagState::Missing))
        } else {
            names.to_vec()
        };
        let to_replace = if checked {
            pick(|s| s == Some(TagState::Different))
        } else {
            Vec::new()
        };
        let on_remote = if checked {
            pick(|s| s != Some(TagState::Missing))
        } else {
            names.to_vec()
        };
        if !to_push.is_empty() {
            pushes.push((remote.clone(), to_push));
        }
        if !to_replace.is_empty() {
            replaces.push((remote.clone(), to_replace));
        }
        if !on_remote.is_empty() {
            deletes.push((remote.clone(), on_remote, !checked));
        }
    }
    let mut items = Vec::new();
    for (remote, tags) in &pushes {
        let title = if tags.len() == 1 {
            format!("Push {} to {remote}", tags[0])
        } else {
            tr!(
                "Push {count} Tags to {remote}",
                count = tags.len(),
                remote = remote
            )
        };
        let command = json!({ "op": "pushTags", "remote": remote, "names": tags });
        let a = actions.clone();
        items.push(menu::action(title, move || a.perform(command.clone())));
    }
    for (remote, tags) in &replaces {
        let title = if tags.len() == 1 {
            tr!("Replace {tag} on {remote}…", tag = tags[0], remote = remote)
        } else {
            tr!(
                "Replace {count} Tags on {remote}…",
                count = tags.len(),
                remote = remote
            )
        };
        let (a, t, r) = (actions.clone(), tags.clone(), remote.clone());
        items.push(menu::action(title, move || confirm_replace(&a, &t, &r)));
    }
    if !pushes.is_empty() || !replaces.is_empty() {
        items.push(Item::Separator);
    }
    let (title, question) = if names.len() == 1 {
        (
            titles::delete(&names[0]),
            tr!("Delete tag {name}?", name = names[0]),
        )
    } else {
        (
            tr!("Delete {count} Tags…", count = names.len()),
            tr!("Delete {count} tags?", count = names.len()),
        )
    };
    let message = format!(
        "{}{} {}",
        list(names),
        tr!("The remotes keep theirs."),
        sheets::can_undo()
    );
    let commands: Vec<serde_json::Value> = names
        .iter()
        .map(|n| json!({ "op": "deleteTag", "name": n }))
        .collect();
    let a = actions.clone();
    items.push(menu::action(title, move || {
        sheets::confirm_then(
            &a,
            question.clone(),
            message.clone(),
            tr!("Delete"),
            false,
            commands.clone(),
        )
    }));
    for (remote, tags, unchecked) in deletes {
        let (title, question) = if tags.len() == 1 {
            (
                tr!(
                    "Delete {tag} from {remote}…",
                    tag = tags[0],
                    remote = remote
                ),
                tr!(
                    "Delete {tag} from {remote}?",
                    tag = tags[0],
                    remote = remote
                ),
            )
        } else {
            (
                tr!(
                    "Delete {count} Tags from {remote}…",
                    count = tags.len(),
                    remote = remote
                ),
                tr!(
                    "Delete {count} tags from {remote}?",
                    count = tags.len(),
                    remote = remote
                ),
            )
        };
        let mut message = format!(
            "{}{} {}",
            list(&tags),
            tr!(
                "Everyone who fetches from {remote} loses them; yours stay.",
                remote = remote
            ),
            sheets::cant_undo()
        );
        if unchecked {
            message.push(' ');
            message.push_str(&tr!(
                "Whether {remote} has them hasn’t been checked yet (Fetch reads it).",
                remote = remote
            ));
        }
        let command = json!({ "op": "pushTags", "remote": remote, "names": tags, "delete": true });
        let a = actions.clone();
        items.push(menu::action(title, move || {
            sheets::confirm_then(
                &a,
                question.clone(),
                message.clone(),
                tr!("Delete"),
                true,
                vec![command.clone()],
            )
        }));
    }
    items
}

/// リモートにある同じ名前の別のタグを、手元のもので置き換える前に確かめる（戻せないので赤いボタン）。
pub fn confirm_replace(actions: &Rc<Actions>, tags: &[String], remote: &str) {
    let title = if tags.len() == 1 {
        tr!("Replace {tag} on {remote}?", tag = tags[0], remote = remote)
    } else {
        tr!(
            "Replace {count} tags on {remote}?",
            count = tags.len(),
            remote = remote
        )
    };
    let message = format!(
        "{}\n{} {}",
        tags.join("\n"),
        tr!(
            "{remote} has a different tag with this name; yours replaces it for everyone who fetches.",
            remote = remote
        ),
        sheets::cant_undo()
    );
    sheets::confirm_then(
        actions,
        title,
        message,
        tr!("Replace"),
        true,
        vec![json!({ "op": "pushTags", "remote": remote, "names": tags, "force": true })],
    );
}

/// お気に入りの順を書く（`branch.<name>.amonhenPinned` に 1 から順に）命令。
fn pinned_order(names: &[String]) -> Vec<serde_json::Value> {
    names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            json!({ "op": "configSet", "key": format!("branch.{name}.amonhenPinned"), "value": (i + 1).to_string() })
        })
        .collect()
}

/// `names` をお気に入りから外す・入れる命令（外すときは残りの順を詰める）。
pub fn favorite_commands(names: &[String], known: &Known, remove: bool) -> Vec<serde_json::Value> {
    if remove {
        let rest: Vec<String> = known
            .pinned
            .iter()
            .filter(|n| !names.contains(n))
            .cloned()
            .collect();
        let mut commands = pinned_order(&rest);
        commands.extend(names.iter().map(|name| {
            json!({ "op": "configSet", "key": format!("branch.{name}.amonhenPinned"), "value": null })
        }));
        commands
    } else {
        let mut order = known.pinned.clone();
        order.extend(names.iter().filter(|n| !known.pinned.contains(n)).cloned());
        pinned_order(&order)
    }
}

/// `names` をアーカイブする・戻す命令。
pub fn archive_commands(names: &[String], unarchive: bool) -> Vec<serde_json::Value> {
    names
        .iter()
        .map(|name| {
            let value = if unarchive { json!(null) } else { json!("true") };
            json!({ "op": "configSet", "key": format!("branch.{name}.amonhenArchived"), "value": value })
        })
        .collect()
}

/// お気に入りとアーカイブ（git の設定に書く。ブランチを消せば一緒に消え、名前を変えればついていく）。
fn favorite_items(actions: &Rc<Actions>, name: &str, known: &Known) -> Vec<Item> {
    let names = vec![name.to_owned()];
    let pinned = known.pinned.iter().any(|n| n == name);
    let commands = favorite_commands(&names, known, pinned);
    let a = actions.clone();
    let favorite = menu::action(
        if pinned {
            tr!("Remove from Favorites")
        } else {
            tr!("Add to Favorites")
        },
        move || a.perform_sequence(commands.clone()),
    );
    let archived = known.groups.as_ref().is_some_and(|g| g.is_archived(name));
    let commands = archive_commands(&names, archived);
    let a = actions.clone();
    let archive = menu::action(
        if archived {
            tr!("Unarchive")
        } else {
            tr!("Archive")
        },
        move || a.perform_sequence(commands.clone()),
    );
    vec![favorite, archive]
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
