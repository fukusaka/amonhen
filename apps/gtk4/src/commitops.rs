//! History のコミットの右クリックのメニュー。項目と文言は macOS 版の `HistoryViewController` と同じ。

use std::rc::Rc;

use amonhen_core::HistorySnapshot;
use amonhen_layout::Node;
use amonhen_model::tr;
use gtk::glib;
use gtk::prelude::*;
use serde_json::json;

use crate::actions::Actions;
use crate::historyedit::{Opener, Preset};
use crate::menu::{self, Item};
use crate::refops;

/// `row` 行目のコミットのメニュー。`widget` はコピーに使う（クリップボード）。`edit` は History の編集モード。
pub fn menu(
    actions: &Rc<Actions>,
    history: &HistorySnapshot,
    row: u32,
    widget: &gtk::Widget,
    edit: &Opener,
) -> Vec<Item> {
    let Some(id) = history.graph.ids().get(row as usize) else {
        return Vec::new();
    };
    let hex = id.to_string();
    let short = hex[..8].to_owned();
    let is_merge = history
        .layout
        .nodes()
        .get(row as usize)
        .is_some_and(|n| n.flags & Node::MERGE != 0);
    let mut items = Vec::new();
    let (a, h) = (actions.clone(), hex.clone());
    items.push(menu::action(
        crate::titles::new_branch_from(&short),
        move || {
            refops::ask_new_branch(&a, &h);
        },
    ));
    let (a, h) = (actions.clone(), hex.clone());
    items.push(menu::action(
        tr!("New Tag at {name}…", name = short),
        move || {
            refops::ask_new_tag(&a, &h);
        },
    ));
    let (a, h) = (actions.clone(), hex.clone());
    items.push(menu::action(
        crate::titles::checkout_detached(&hex),
        move || {
            crate::sheets::checkout(&a, crate::sheets::CheckoutTarget::Detach(h.clone()));
        },
    ));
    items.push(reset(actions, &hex));
    items.push(Item::Separator);
    if !is_merge {
        // コミットの編集は、History の編集モードで（変更を積んで Apply）。ここにあるのは、最初の変更を入れて
        // 編集モードを開く近道。
        for (title, preset) in [
            (tr!("Edit Message…"), Some(Preset::Reword(hex.clone()))),
            (
                tr!("Combine with Older Commit…"),
                Some(Preset::Combine(vec![hex.clone()])),
            ),
            ("Drop…".to_owned(), Some(Preset::Drop(hex.clone()))),
            (tr!("Edit Commits from Here…"), None),
        ] {
            let (e, h) = (edit.clone(), hex.clone());
            items.push(menu::action(title, move || e.open(&h, preset.clone())));
        }
        // 分ける（止めてから Working Copy で分けてコミットする）は、編集モードの外の操作。
        let (a, h, s) = (actions.clone(), hex.clone(), short.clone());
        items.push(menu::action("Split…", move || {
            let (a, h, s) = (a.clone(), h.clone(), s.clone());
            glib::spawn_future_local(async move {
                let message = format!(
                    "{} {}",
                    tr!("Its changes go back to the working tree; commit them in parts, then Continue."),
                    crate::sheets::can_undo()
                );
                // worktree の今のブランチを書き換えるので、Cherry-pick・Revert と同じく、どの worktree かを書く
                // （macOS 版と同じ）。
                let sheet = crate::sheets::Sheet::new(
                    crate::sheets::parent(&a).as_ref(),
                    &tr!("Split {commit}?", commit = s),
                    &message,
                );
                sheet.buttons("Split", false, None);
                if sheet.run().await == crate::sheets::Response::Ok {
                    a.rewrite(&format!("Split {s}"), json!({ "kind": "split", "oid": h }));
                }
            });
        }));
        items.push(Item::Separator);
    }
    let (a, h) = (actions.clone(), hex.clone());
    items.push(menu::action(crate::titles::cherry_pick(&hex), move || {
        crate::sheets::pick(&a, false, vec![(h.clone(), is_merge)]);
    }));
    let (a, h) = (actions.clone(), hex.clone());
    items.push(menu::action(crate::titles::revert(&hex), move || {
        crate::sheets::pick(&a, true, vec![(h.clone(), is_merge)]);
    }));
    // マージはパッチにできない（git format-patch は飛ばす）。押せない項目で残す（macOS 版と同じ）。
    if is_merge {
        items.push(Item::Disabled(tr!("Save as Patch…")));
    } else {
        let (a, h) = (actions.clone(), hex.clone());
        items.push(menu::action(tr!("Save as Patch…"), move || {
            crate::patches::save(&a, vec![h.clone()]);
        }));
    }
    let label = widget
        .downcast_ref::<crate::history::HistoryView>()
        .map(|view| crate::appmenu::archive_label(&view.ref_names(id)))
        .unwrap_or_default();
    let label = if label.is_empty() {
        short.clone()
    } else {
        label
    };
    let (a, h) = (actions.clone(), hex.clone());
    items.push(menu::action(tr!("Export Archive…"), move || {
        crate::archive::export(&a, h.clone(), label.clone());
    }));
    items.push(Item::Separator);
    let (w, h) = (widget.clone(), hex);
    items.push(menu::action(tr!("Copy Commit ID"), move || {
        w.clipboard().set_text(&h);
    }));
    // このコミットを指すブランチ・タグの名前（リモートブランチは origin/ を付けたまま。5 つまで。macOS 版と同じ）。
    if let Some(view) = widget.downcast_ref::<crate::history::HistoryView>() {
        for (name, kind) in view.ref_names(id).into_iter().take(5) {
            let title = if kind == amonhen_model::RefKind::Tag {
                tr!("Copy Tag Name “{name}”", name = name)
            } else {
                tr!("Copy Branch Name “{name}”", name = name)
            };
            let w = widget.clone();
            items.push(menu::action(title, move || {
                w.clipboard().set_text(&name);
            }));
        }
    }
    items
}

/// 「Reset <今のブランチ> to <コミット>…」（History と Reflog の右クリック。macOS 版の `resetMenuItem`）。動かし方
/// （Soft・Mixed・Hard）は確認のシートで選ぶ。merge などの途中ならコアが断る。
pub fn reset(actions: &Rc<Actions>, hex: &str) -> Item {
    let branch = actions.head().unwrap_or_else(|| "HEAD".to_owned());
    let (a, h) = (actions.clone(), hex.to_owned());
    menu::action(crate::titles::reset(&branch, hex), move || {
        ask_reset(&a, &h)
    })
}

/// 今のブランチ（detached なら HEAD）を `hex` に動かす前に確かめる。動かし方はシートの中のラジオボタンで選ぶ
/// （既定は Mixed。macOS 版の `reset(to:)` と同じ）。
fn ask_reset(actions: &Rc<Actions>, hex: &str) {
    let (actions, hex) = (actions.clone(), hex.to_owned());
    glib::spawn_future_local(async move {
        let branch = actions.head().unwrap_or_else(|| "HEAD".to_owned());
        // 前と後の絵と、今との違いがどこへ行くかの材料（シートを出す前に読む。出してから背が伸びないように）。
        let mut command = json!({ "op": "movePreview", "from": "HEAD", "to": hex });
        if let Some(head) = actions.head() {
            command["branch"] = json!(head);
        }
        let preview = match actions.repo() {
            Some(repo) => crate::sheets::run(&repo, command).await["info"].clone(),
            None => serde_json::Value::Null,
        };
        let parent = crate::sheets::parent(&actions);
        let sheet = crate::sheets::Sheet::new(
            parent.as_ref(),
            &tr!(
                "Reset {branch} to {id}?",
                branch = branch,
                id = crate::titles::short(&hex)
            ),
            &format!(
                "{} {}",
                tr!("Commits taken off stay in Reflog."),
                crate::sheets::can_undo()
            ),
        );
        let modes = [
            ("soft", tr!("Soft: keep the changes staged")),
            ("mixed", tr!("Mixed: keep the changes in the working tree")),
            (
                "hard",
                tr!("Hard: match the commit (uncommitted changes are stashed first)"),
            ),
        ];
        if !preview["drawing"].is_null() {
            let (drawing, set) = crate::rewritedraw::new_move(crate::rewritedraw::Move {
                moving: branch.clone(),
                kind: "local",
                after_title: tr!("After the reset"),
                lost_note: tr!("taken off"),
                lost_red: false,
                removes: false,
            });
            set(&preview);
            sheet.add(&drawing);
        }
        let column = gtk::Box::new(gtk::Orientation::Vertical, 4);
        let radios: Vec<gtk::CheckButton> = modes
            .iter()
            .map(|(_, title)| gtk::CheckButton::with_label(title))
            .collect();
        for r in &radios {
            if r != &radios[0] {
                r.set_group(Some(&radios[0]));
            }
            column.append(r);
        }
        radios[1].set_active(true);
        // 選んだ動かし方で、今との違いがどこへ行くか（macOS 版と同じ）。
        let files = preview["files"].as_u64().unwrap_or(0);
        let note = gtk::Label::new(None);
        note.set_xalign(0.0);
        note.set_wrap(true);
        note.set_max_width_chars(60);
        note.add_css_class("dim-label");
        note.add_css_class("caption");
        note.set_visible(files > 0);
        let update = {
            let (radios, note) = (radios.clone(), note.clone());
            move || {
                let text = if radios[0].is_active() {
                    tr!(
                        "The difference from now ({count} files) stays staged.",
                        count = files
                    )
                } else if radios[2].is_active() {
                    tr!(
                        "The working tree matches the commit; the difference from now ({count} files) goes away (Undo brings it back).",
                        count = files
                    )
                } else {
                    tr!(
                        "The difference from now ({count} files) stays in the working tree.",
                        count = files
                    )
                };
                note.set_text(&text);
            }
        };
        update();
        let update = Rc::new(update);
        for r in &radios {
            let u = update.clone();
            r.connect_toggled(move |_| u());
        }
        column.append(&note);
        // 外れるコミットを上流が持っていれば、このあとの Push は Force Push になる。
        if preview["upstreamHas"].as_u64().unwrap_or(0) > 0
            && let Some(upstream) = preview["upstream"].as_str()
        {
            let warning = gtk::Label::new(Some(&format!(
                "⚠︎ {}",
                tr!(
                    "{upstream} has commits this takes off, so pushing afterwards needs Force Push.",
                    upstream = upstream
                )
            )));
            warning.set_xalign(0.0);
            warning.set_wrap(true);
            warning.set_max_width_chars(60);
            warning.add_css_class("warning");
            warning.add_css_class("caption");
            column.append(&warning);
        }
        sheet.add(&column);
        sheet.buttons("Reset", false, None);
        if sheet.run().await != crate::sheets::Response::Ok {
            return;
        }
        let Some(mode) = radios
            .iter()
            .zip(modes.iter())
            .find(|(r, _)| r.is_active())
            .map(|(_, (mode, _))| *mode)
        else {
            return;
        };
        actions.perform(json!({ "op": "reset", "oid": hex, "mode": mode }));
    });
}

/// 複数選んだ行の上のメニュー（macOS 版の `buildMultipleMenu` と同じ）。`rows` は昇順（新しい順）。
pub fn menu_many(
    actions: &Rc<Actions>,
    history: &HistorySnapshot,
    rows: &[u32],
    widget: &gtk::Widget,
    range_diff: &crate::rangediff::Opener,
    edit: &Opener,
) -> Vec<Item> {
    let commits: Vec<(String, bool)> = rows
        .iter()
        .filter_map(|&r| {
            let id = history.graph.ids().get(r as usize)?;
            let merge = history
                .layout
                .nodes()
                .get(r as usize)
                .is_some_and(|n| n.flags & Node::MERGE != 0);
            Some((id.to_string(), merge))
        })
        .collect();
    let n = commits.len();
    let mut items = Vec::new();
    let (a, c) = (actions.clone(), commits.clone());
    items.push(menu::action(
        tr!("Cherry-pick {count} Commits…", count = n),
        move || crate::sheets::pick(&a, false, c.iter().rev().cloned().collect()),
    ));
    let (a, c) = (actions.clone(), commits.clone());
    items.push(menu::action(
        tr!("Revert {count} Commits…", count = n),
        move || crate::sheets::pick(&a, true, c.clone()),
    ));
    // マージはパッチにできない（git format-patch は飛ばす）。押せない項目で残す（macOS 版と同じ）。古い順に。
    if commits.iter().any(|(_, merge)| *merge) {
        items.push(Item::Disabled(tr!(
            "Save {count} Commits as Patches…",
            count = n
        )));
    } else {
        let (a, oids) = (
            actions.clone(),
            commits
                .iter()
                .rev()
                .map(|(o, _)| o.clone())
                .collect::<Vec<_>>(),
        );
        items.push(menu::action(
            tr!("Save {count} Commits as Patches…", count = n),
            move || crate::patches::save(&a, oids.clone()),
        ));
    }
    // 連続した行（first-parent で続いているかはコアで確かめる）なら、1 つにまとめられる。
    let consecutive = rows.windows(2).all(|w| w[1] == w[0] + 1);
    if n >= 2 && consecutive && !commits.iter().any(|(_, merge)| *merge) {
        let (e, oids) = (
            edit.clone(),
            commits.iter().map(|(o, _)| o.clone()).collect::<Vec<_>>(),
        );
        let oldest = oids.last().cloned().unwrap_or_default();
        items.push(menu::action(
            tr!("Combine {count} Commits…", count = n),
            move || e.open(&oldest, Some(Preset::Combine(oids.clone()))),
        ));
    }
    // 今のコミット以外を、上の行から順に 1 つのコミットで merge する（octopus）。
    let sources = crate::mergeseveral::sources(actions, history, rows);
    if sources.len() > 1 {
        let branch = actions.head();
        let (a, title) = (
            actions.clone(),
            tr!(
                "Merge {count} into {branch}…",
                count = sources.len(),
                branch = branch.as_deref().unwrap_or("HEAD")
            ),
        );
        items.push(menu::action(title, move || {
            crate::mergeseveral::confirm(&a, sources.clone(), branch.clone());
        }));
    }
    // 2 つなら、それぞれを先端とする 2 つの版として比べられる（rebase の前と後など）。
    if let [(newer, _), (older, _)] = &commits[..] {
        let (r, ranges) = (range_diff.clone(), vec![format!("{older}...{newer}")]);
        items.push(menu::action(
            tr!("Compare as Two Versions (Range Diff)"),
            move || r.ranges(ranges.clone()),
        ));
    }
    items.push(Item::Separator);
    let (w, ids) = (
        widget.clone(),
        commits.iter().map(|(o, _)| o.clone()).collect::<Vec<_>>(),
    );
    items.push(menu::action(tr!("Copy Commit IDs"), move || {
        w.clipboard().set_text(&ids.join("\n"));
    }));
    let (w, ids) = (
        widget.clone(),
        commits
            .iter()
            .map(|(o, _)| crate::titles::short(o).to_owned())
            .collect::<Vec<_>>(),
    );
    items.push(menu::action(tr!("Copy Short IDs"), move || {
        w.clipboard().set_text(&ids.join(" "));
    }));
    items
}

/// History でコミット `id` を運び、`row` 行目の上（行の数なら最後の行の下）に落とした: 編集できる並び（HEAD から
/// 最初の merge まで）の中なら、編集モードを開いて動かしておく（並びを見て Apply。範囲は、動かす 2 か所の古い方
/// から HEAD まで。macOS 版と同じ）。並びの外ならベルを鳴らす。
pub fn move_by_drop(
    actions: &Rc<Actions>,
    view: &crate::history::HistoryView,
    edit: &Opener,
    id: amonhen_model::Oid,
    row: u32,
) {
    let Some((_, history)) = view.current() else {
        return;
    };
    // 動かすのは書き込む先の worktree の並び（ドラッグを始めたときと同じ）。
    let Some(repo) = actions.repo() else {
        return;
    };
    let ids = history.graph.ids();
    let count = ids.len() as u32;
    let Some(below) = ids.get(row.min(count.saturating_sub(1)) as usize) else {
        return;
    };
    let (oid, below) = (id.to_string(), below.to_string());
    let (actions, edit) = (actions.clone(), edit.clone());
    glib::spawn_future_local(async move {
        let reply = crate::sheets::run(&repo, json!({ "op": "editableCommits" })).await;
        let chain: Vec<String> = reply["text"]
            .as_str()
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.split(' ').next().map(str::to_owned))
            .filter(|o| !o.is_empty())
            .collect();
        let at = |o: &str| chain.iter().position(|c| c == o);
        let (Some(from), Some(mut to)) = (at(&oid), at(&below)) else {
            if let Some(w) = actions.window() {
                w.error_bell();
            }
            return;
        };
        if row >= count {
            to = chain.len() - 1;
        } else if from < to {
            to -= 1;
        }
        if from == to {
            return;
        }
        edit.open(&chain[from.max(to)], Some(Preset::Move(oid, to)));
    });
}
