//! History のコミットの右クリックのメニュー。項目と文言は macOS 版の `HistoryViewController` と同じ。

use std::rc::Rc;

use amonhen_core::HistorySnapshot;
use amonhen_layout::Node;
use gtk::glib;
use gtk::prelude::*;
use serde_json::json;

use crate::actions::Actions;
use crate::menu::{self, Item};
use crate::refops;

/// `row` 行目のコミットのメニュー。`widget` はコピーに使う（クリップボード）。
pub fn menu(
    actions: &Rc<Actions>,
    history: &HistorySnapshot,
    row: u32,
    widget: &gtk::Widget,
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
        format!("New Branch at {short}…"),
        move || {
            refops::ask_new_branch(&a, &h);
        },
    ));
    let (a, h) = (actions.clone(), hex.clone());
    items.push(menu::action(format!("New Tag at {short}…"), move || {
        refops::ask_new_tag(&a, &h);
    }));
    let (a, h) = (actions.clone(), hex.clone());
    items.push(menu::action(
        format!("Checkout {short} (Detached)"),
        move || {
            a.perform(json!({ "op": "detach", "oid": h }));
        },
    ));
    items.push(Item::Separator);
    if !is_merge {
        let (a, h) = (actions.clone(), hex.clone());
        items.push(menu::action("Edit Message…", move || a.reword(&h)));
        let (a, h) = (actions.clone(), hex.clone());
        items.push(menu::action("Squash into Parent…", move || a.squash(&h)));
        for (title, kind, label) in [
            ("Fixup into Parent", "fixup", "Fixup"),
            ("Edit (Stop Here)", "edit", "Edit"),
            ("Split…", "split", "Split"),
        ] {
            let (a, h, label) = (actions.clone(), hex.clone(), format!("{label} {short}"));
            items.push(menu::action(title, move || {
                a.rewrite(&label, json!({ "kind": kind, "oid": h }));
            }));
        }
        let (a, h, s) = (actions.clone(), hex.clone(), short.clone());
        items.push(menu::action("Drop", move || {
            let (a, h, s) = (a.clone(), h.clone(), s.clone());
            glib::spawn_future_local(async move {
                if a.confirm(
                    &format!("{s} を取り除きますか？"),
                    "Undo で戻せます。",
                    "取り除く",
                )
                .await
                {
                    a.rewrite(&format!("Drop {s}"), json!({ "kind": "drop", "oid": h }));
                }
            });
        }));
        items.push(Item::Separator);
    }
    let (a, h) = (actions.clone(), hex.clone());
    items.push(menu::action(format!("Cherry-pick {short}"), move || {
        a.perform(json!({ "op": "cherryPick", "oid": h, "isMerge": is_merge }));
    }));
    let (a, h) = (actions.clone(), hex.clone());
    items.push(menu::action(format!("Revert {short}"), move || {
        a.perform(json!({ "op": "revert", "oid": h, "isMerge": is_merge }));
    }));
    items.push(Item::Separator);
    let (w, h) = (widget.clone(), hex);
    items.push(menu::action("Copy Commit ID", move || {
        w.clipboard().set_text(&h);
    }));
    items
}

/// 複数選んだ行の上のメニュー（macOS 版の `buildMultipleMenu` と同じ）。`rows` は昇順（新しい順）。
pub fn menu_many(
    actions: &Rc<Actions>,
    history: &HistorySnapshot,
    rows: &[u32],
    widget: &gtk::Widget,
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
        format!("Cherry-pick {n} Commits（古い順）"),
        move || {
            let sequence = c
                .iter()
                .rev()
                .map(|(oid, merge)| json!({ "op": "cherryPick", "oid": oid, "isMerge": merge }))
                .collect();
            a.perform_sequence(sequence);
        },
    ));
    let (a, c) = (actions.clone(), commits.clone());
    items.push(menu::action(
        format!("Revert {n} Commits（新しい順）"),
        move || {
            let sequence = c
                .iter()
                .map(|(oid, merge)| json!({ "op": "revert", "oid": oid, "isMerge": merge }))
                .collect();
            a.perform_sequence(sequence);
        },
    ));
    // 連続した行（first-parent で続いているかはコアで確かめる）なら、1 つにまとめられる。
    let consecutive = rows.windows(2).all(|w| w[1] == w[0] + 1);
    if n >= 2 && consecutive && !commits.iter().any(|(_, merge)| *merge) {
        let (a, oids) = (
            actions.clone(),
            commits.iter().map(|(o, _)| o.clone()).collect::<Vec<_>>(),
        );
        items.push(menu::action(format!("Squash {n} Commits…"), move || {
            a.squash_range(oids.clone());
        }));
    }
    items.push(Item::Separator);
    let (w, ids) = (
        widget.clone(),
        commits.iter().map(|(o, _)| o.clone()).collect::<Vec<_>>(),
    );
    items.push(menu::action("Copy Commit IDs", move || {
        w.clipboard().set_text(&ids.join("\n"));
    }));
    let (w, ids) = (
        widget.clone(),
        commits
            .iter()
            .map(|(o, _)| o[..10].to_owned())
            .collect::<Vec<_>>(),
    );
    items.push(menu::action("Copy Short IDs", move || {
        w.clipboard().set_text(&ids.join(" "));
    }));
    items
}
