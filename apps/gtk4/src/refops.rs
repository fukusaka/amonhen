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
            let elsewhere = actions.worktree_using(&name);
            if !is_head {
                match &elsewhere {
                    // ほかの worktree が checkout しているブランチは checkout できない（git が断る）。その
                    // worktree を書き込む先にする。
                    Some((worktree, path)) => {
                        let (a, p) = (actions.clone(), path.clone());
                        items.push(menu::action(
                            tr!("Open Worktree {name}", name = worktree),
                            move || a.edit_worktree(&p),
                        ));
                    }
                    None => {
                        let (a, n) = (actions.clone(), name.clone());
                        items.push(menu::action(titles::checkout(&name), move || {
                            sheets::checkout(&a, sheets::CheckoutTarget::Branch(n.clone()))
                        }));
                    }
                }
                merge(&mut items);
                rebase(&mut items);
                items.push(Item::Separator);
            }
            items.extend(sync_items(actions, entry, is_head));
            items.extend(favorite_items(actions, &name, known));
            items.push(new_branch(actions, &name));
            items.push(new_tag(actions, &name));
            items.push(export_archive(actions, "refs/heads/", &name));
            items.push(save_bundle(actions, entry.kind, &name));
            let (r, n) = (range_diff.clone(), name.clone());
            items.push(menu::action(
                tr!("Compare with Previous Version"),
                move || r.previous(Some(n.clone())),
            ));
            let (a, from) = (actions.clone(), name.clone());
            items.push(menu::action(tr!("Rename…"), move || {
                ask_rename_branch(&a, &from)
            }));
            // ほかの worktree が checkout しているブランチも出す（git が断る。macOS 版と同じ）。
            if !is_head {
                let (a, n) = (actions.clone(), name.clone());
                items.push(menu::action(titles::delete(&name), move || {
                    sheets::delete_branches(&a, vec![n.clone()])
                }));
            }
            items.extend(pull_request_items(actions, &name, known));
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
            items.push(export_archive(actions, "refs/remotes/", &name));
            items.push(save_bundle(actions, entry.kind, &name));
        }
        RefKind::Tag => {
            merge(&mut items);
            items.push(new_branch(actions, &name));
            items.push(export_archive(actions, "refs/tags/", &name));
            items.push(save_bundle(actions, entry.kind, &name));
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
            // Update は git の既定（記録したコミット、中の submodule には入らない）。Update… で変えられる（macOS 版と同じ）。
            items.push(menu::action(
                format!("Update {name}"),
                run(json!({ "op": "updateSubmodules", "paths": [name] })),
            ));
            let (a, p) = (actions.clone(), name.clone());
            items.push(menu::action("Update…", move || {
                crate::submodulesheet::update(&a, vec![p.clone()])
            }));
            let state = known.submodules.get(&name).cloned();
            if state.as_ref().is_none_or(|s| s.initialized) {
                let (open, path) = (open_submodule.clone(), name.clone());
                items.push(menu::action(tr!("Open in New Window"), move || open(&path)));
            }
            items.push(Item::Separator);
            let state = state.unwrap_or_else(|| amonhen_model::SubmoduleState {
                path: name.clone(),
                name: name.clone(),
                initialized: true,
                ..Default::default()
            });
            let (a, s) = (actions.clone(), state.clone());
            items.push(menu::action(tr!("Edit URL…"), move || {
                crate::submodulesheet::set_url(&a, &s)
            }));
            let (a, s) = (actions.clone(), state.clone());
            items.push(menu::action(tr!("Branch to Follow…"), move || {
                crate::submodulesheet::set_branch(&a, &s)
            }));
            items.push(Item::Separator);
            let (a, s) = (actions.clone(), state);
            items.push(menu::action(tr!("Remove…"), move || {
                crate::submodulesheet::remove(&a, &s)
            }));
        }
    }
    items
}

/// Remotes の見出しとその下のリモート（docs/ux.md の 4.5 のリモート: リポジトリの設定のページと同じ操作）。
/// 見出しは Add Remote…、リモートは Rename…・Edit URL…（どちらも同じシート）・Prune・Remove…。
fn remote_menu(actions: &Rc<Actions>, data: &Data) -> Vec<Item> {
    // subtree と Subtrees の見出し（macOS 版と同じ）。
    if let sidebar::Item::Subtree(subtree) = &data.item {
        let item = |title: &str, f: fn(&Rc<Actions>, &amonhen_model::Subtree)| {
            let (a, s) = (actions.clone(), subtree.clone());
            menu::action(title, move || f(&a, &s))
        };
        return vec![
            item("Pull…", |a, s| {
                crate::subtreesheet::pull_or_push(a, s, false)
            }),
            item("Push…", |a, s| {
                crate::subtreesheet::pull_or_push(a, s, true)
            }),
            item("Split…", crate::subtreesheet::split),
        ];
    }
    if matches!(data.item, sidebar::Item::Group) && data.title == "Subtrees" {
        let a = actions.clone();
        return vec![menu::action(tr!("Add Subtree…"), move || {
            crate::subtreesheet::add(&a)
        })];
    }
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
    // Submodules の見出し（macOS 版と同じ）。
    if matches!(data.item, sidebar::Item::Group) && data.title == "Submodules" {
        let a = actions.clone();
        let all = menu::action("Update All", move || {
            a.perform(json!({ "op": "updateSubmodules", "paths": [] }))
        });
        let a = actions.clone();
        let update = menu::action("Update Submodules…", move || {
            crate::submodulesheet::update(&a, Vec::new())
        });
        let a = actions.clone();
        let sync = menu::action("Sync URLs", move || {
            a.perform(
                json!({ "op": "changeSubmodule", "change": { "kind": "sync", "recursive": true } }),
            )
        });
        let a = actions.clone();
        let add = menu::action(tr!("Add Submodule…"), move || {
            crate::submodulesheet::add(&a)
        });
        return vec![all, update, sync, Item::Separator, add];
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
            } else if actions.worktree_using(&name).is_none() {
                // ほかの worktree が checkout しているなら動かせない（その作業ツリーとずれる）。上の Open Worktree
                // から（macOS 版と同じ）。
                let title = tr!(
                    "Fast-forward {branch} to {upstream}?",
                    branch = name,
                    upstream = upstream
                );
                let command = json!({ "op": "fastForward", "branch": name });
                items.push(menu::action(
                    format!("Fast-Forward {name} to {upstream}…"),
                    move || {
                        sheets::confirm_ref_then(
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
/// ブランチのプルリクエストの項目（macOS 版と同じ）: 既定のブランチ以外から。開いているものがあれば作らずに
/// それを開く。
fn pull_request_items(actions: &Rc<Actions>, name: &str, known: &Known) -> Vec<Item> {
    if !known.can_create_pull_request_from(name) {
        return Vec::new();
    }
    let item = match known.pull_request(name) {
        Some(pr) => {
            let (url, w) = (pr.url.clone(), actions.window());
            menu::action(
                tr!("Open Pull Request #{number}", number = pr.number),
                move || crate::github::open_uri(w.as_ref().map(|w| w.upcast_ref()), &url),
            )
        }
        None => {
            let (a, n) = (actions.clone(), name.to_owned());
            menu::action(tr!("Create Pull Request…"), move || {
                crate::github::create_pull_request(&a, n.clone(), false)
            })
        }
    };
    vec![Item::Separator, item]
}

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
    // 今のブランチ以外を、選んだ順（上から）に 1 つのコミットで merge する（octopus。macOS 版と同じ）。
    let sources: Vec<crate::mergeseveral::Source> = selected
        .iter()
        .filter_map(|e| {
            let kind = match e.kind {
                RefKind::Local if Some(e.name.as_str()) != head => {
                    crate::mergeseveral::Kind::Branch
                }
                RefKind::Remote => crate::mergeseveral::Kind::RemoteBranch,
                RefKind::Tag => crate::mergeseveral::Kind::Tag,
                _ => return None,
            };
            Some(crate::mergeseveral::Source {
                rev: e.name.clone(),
                kind,
            })
        })
        .collect();
    // ブランチ・タグだけなら、まとめて bundle に。
    let refs: Vec<String> = selected
        .iter()
        .filter_map(|e| crate::bundle::full_ref(e.kind, &e.name))
        .collect();
    if refs.len() == selected.len() {
        let (a, names) = (actions.clone(), names.clone());
        let label = names.first().cloned().unwrap_or_default();
        items.push(menu::action(
            tr!("Save {count} as Bundle…", count = refs.len()),
            move || crate::bundle::save(&a, refs.clone(), names.clone(), label.clone()),
        ));
    }
    if sources.len() > 1 {
        let branch = head.map(str::to_owned);
        let title = tr!(
            "Merge {count} into {branch}…",
            count = sources.len(),
            branch = head.unwrap_or("HEAD")
        );
        let a = actions.clone();
        items.push(menu::action(title, move || {
            crate::mergeseveral::confirm(&a, sources.clone(), branch.clone());
        }));
    }
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
        // 今のブランチと、ほかの worktree が checkout しているブランチは消せない（git が断る）。
        if locals
            .iter()
            .all(|n| Some(n.as_str()) != head && actions.worktree_using(n).is_none())
        {
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
        sheets::confirm_ref_then(
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
            sheets::confirm_ref_then(
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
    sheets::confirm_ref_then(
        actions,
        title,
        message,
        tr!("Replace"),
        true,
        vec![json!({ "op": "pushTags", "remote": remote, "names": tags, "force": true })],
    );
}

/// お気に入りの順を書く（`branch.<name>.amonhenPinned` に 1 から順に）命令。
pub fn pinned_order(names: &[String]) -> Vec<serde_json::Value> {
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

/// `start` から作るブランチの名前を聞いて、作って切り替える（文言は macOS 版と同じ）。
pub fn ask_new_branch(actions: &Rc<Actions>, start: &str) {
    let (a, start) = (actions.clone(), start.to_owned());
    glib::spawn_future_local(async move {
        // 見出し・説明・ボタンは macOS 版と同じ。
        let title = tr!("New Branch from {name}", name = start);
        let message = tr!("Name of the branch. It is checked out.");
        if let Some(name) = a.ask(&title, &message, "", &tr!("Create")).await {
            a.perform(
                json!({ "op": "createBranch", "name": name, "start": start, "checkout": true }),
            );
        }
    });
}

/// ブランチ `name` の新しい名前を聞いて変える（サイドバーと Repository > Rename Branch…。macOS 版と同じ）。
pub fn ask_rename_branch(actions: &Rc<Actions>, name: &str) {
    let (a, from) = (actions.clone(), name.to_owned());
    glib::spawn_future_local(async move {
        let title = tr!("Rename {name}", name = from);
        if let Some(to) = a
            .ask(
                &title,
                &tr!("New name of the branch."),
                &from,
                &tr!("Rename"),
            )
            .await
            && to != from
        {
            a.perform(json!({ "op": "renameBranch", "from": from, "to": to }));
        }
    });
}

/// `target` に新しいタグを付ける。
/// Export Archive…: ref（`kind` は `refs/heads/` など。同じ名前のブランチとタグを取り違えない）の時点を書き出す。
fn export_archive(actions: &Rc<Actions>, kind: &str, name: &str) -> Item {
    let (a, rev, label) = (actions.clone(), format!("{kind}{name}"), name.to_owned());
    menu::action(tr!("Export Archive…"), move || {
        crate::archive::export(&a, rev.clone(), label.clone())
    })
}

/// Save as Bundle…: 1 つのブランチ・リモートブランチ・タグを bundle に。
fn save_bundle(actions: &Rc<Actions>, kind: RefKind, name: &str) -> Item {
    let (a, label) = (actions.clone(), name.to_owned());
    let refs: Vec<String> = crate::bundle::full_ref(kind, name).into_iter().collect();
    menu::action(tr!("Save as Bundle…"), move || {
        crate::bundle::save(&a, refs.clone(), vec![label.clone()], label.clone())
    })
}

pub fn new_tag(actions: &Rc<Actions>, target: &str) -> Item {
    let (a, target) = (actions.clone(), target.to_owned());
    menu::action(tr!("New Tag at {name}…", name = target), move || {
        ask_new_tag(&a, &target);
    })
}

pub fn ask_new_tag(actions: &Rc<Actions>, target: &str) {
    let (a, target) = (actions.clone(), target.to_owned());
    glib::spawn_future_local(async move {
        let title = tr!("New Tag at {name}", name = target);
        if let Some(name) = a
            .ask(&title, &tr!("Name of the tag."), "", &tr!("Create"))
            .await
        {
            a.perform(
                json!({ "op": "createTag", "name": name, "target": target, "message": null }),
            );
        }
    });
}

/// `dragged`（`drop_menu` と同じ形）を `target` に落とせるか（`drop_menu` が何か出すか）。落とせない行の上では、
/// 落とせないことを先に見せる（macOS 版の `validateDrop` と同じ）。
pub fn accepts_drop(dragged: &str, target: &Data) -> bool {
    let Some((kind, dragged)) = dragged
        .split_once(':')
        .filter(|(k, _)| matches!(*k, "local" | "other"))
    else {
        return false;
    };
    match &target.item {
        sidebar::Item::Ref(e) if e.kind == RefKind::Local => e.name != dragged,
        sidebar::Item::Folder => kind == "local" && is_remote_folder(target),
        sidebar::Item::Ref(e) if e.kind == RefKind::Remote => kind == "local",
        _ => false,
    }
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
            // ほかの worktree が checkout しているブランチは、ここでは checkout できず、fast-forward でも動かせない
            // （その作業ツリーとずれる）。その worktree で行う（macOS 版と同じ）。
            if let Some((_, path)) = actions.worktree_using(&to) {
                let a = actions.clone();
                items.push(menu::action(
                    tr!(
                        "Open the Worktree of {branch} to Merge or Rebase",
                        branch = to
                    ),
                    move || a.edit_worktree(&path),
                ));
                return items;
            }
            let a = actions.clone();
            let (d, t) = (dragged.clone(), to.clone());
            // checkout していなくても、fast-forward で済むならブランチだけ進める。
            items.push(menu::action(titles::merge(&dragged, &to), move || {
                sheets::confirm_integrate(
                    &a,
                    tr!("Merge {rev} into {target}?", rev = d, target = t),
                    "Merge",
                    t.clone(),
                    vec![d.clone()],
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
                // 落とした先のブランチに取り込む見込み（rebase のときはコミットごと）。落とした先が今のブランチでも
                // 同じ確認（macOS 版と同じ）。
                sheets::confirm_integrate(
                    &a,
                    tr!("Rebase {target} onto {rev}?", target = t, rev = d),
                    "Rebase",
                    t.clone(),
                    vec![d.clone()],
                    true,
                    sequence.clone(),
                );
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

#[cfg(test)]
mod tests {
    use super::*;

    fn data(key: &str, item: sidebar::Item) -> Data {
        Data {
            key: key.into(),
            item,
            ..Data::default()
        }
    }

    fn entry(kind: RefKind, name: &str) -> sidebar::Item {
        sidebar::Item::Ref(amonhen_model::RefEntry {
            kind,
            name: name.into(),
            target: None,
            detail: None,
            ahead: 0,
            behind: 0,
            gone: false,
        })
    }

    #[test]
    fn drops_only_where_the_menu_has_items() {
        let branch = data("Local/main", entry(RefKind::Local, "main"));
        assert!(accepts_drop("local:topic", &branch));
        assert!(accepts_drop("other:origin/topic", &branch));
        // 自分の上・History のコミット。
        assert!(!accepts_drop("local:main", &branch));
        assert!(!accepts_drop("commit:0123", &branch));
        // push できるのはローカルブランチだけ。
        let remote = data("Remotes/origin", sidebar::Item::Folder);
        assert!(accepts_drop("local:topic", &remote));
        assert!(!accepts_drop("other:v1", &remote));
        let remote_branch = data("Remote/origin/x", entry(RefKind::Remote, "origin/x"));
        assert!(accepts_drop("local:topic", &remote_branch));
        // タグ・名前のフォルダ・Workspace の行。
        assert!(!accepts_drop(
            "local:topic",
            &data("Tag/v1", entry(RefKind::Tag, "v1"))
        ));
        assert!(!accepts_drop(
            "local:topic",
            &data("Branches/#feature", sidebar::Item::Folder)
        ));
        assert!(!accepts_drop(
            "local:topic",
            &data("page/history", sidebar::Item::Page(sidebar::Page::History))
        ));
    }
}
