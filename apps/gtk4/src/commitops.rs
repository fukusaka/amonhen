//! History のコミットの右クリックのメニュー。項目と文言は macOS 版の `HistoryViewController` と同じ。

use std::rc::Rc;

use amonhen_core::HistorySnapshot;
use amonhen_layout::Node;
use amonhen_model::tr;
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
    items.extend(reset(actions, &hex));
    items.push(Item::Separator);
    if !is_merge {
        let (a, h) = (actions.clone(), hex.clone());
        items.push(menu::action(tr!("Edit Message…"), move || a.reword(&h)));
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
                    &tr!("Drop {commit}?", commit = s),
                    &tr!("You can Undo this."),
                    &tr!("Drop"),
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
    items.push(menu::action(crate::titles::cherry_pick(&hex), move || {
        crate::sheets::pick(&a, false, vec![(h.clone(), is_merge)]);
    }));
    let (a, h) = (actions.clone(), hex.clone());
    items.push(menu::action(crate::titles::revert(&hex), move || {
        crate::sheets::pick(&a, true, vec![(h.clone(), is_merge)]);
    }));
    items.push(Item::Separator);
    let (w, h) = (widget.clone(), hex);
    items.push(menu::action(tr!("Copy Commit ID"), move || {
        w.clipboard().set_text(&h);
    }));
    items
}

/// 「Reset <今のブランチ> to <コミット> ▸」と、その下の Soft・Mixed・Hard（History と Reflog の右クリック。
/// docs/ux.md の「5c. Reset」）。Soft などは git の言葉のまま。merge・rebase などの途中なら出さない（コアも断る）。
pub fn reset(actions: &Rc<Actions>, hex: &str) -> Option<Item> {
    if actions.in_sequence() {
        return None;
    }
    let branch = actions.head().unwrap_or_else(|| "HEAD".to_owned());
    let modes = [
        ("Soft…", "soft", tr!("Soft: keep the changes staged")),
        (
            "Mixed…",
            "mixed",
            tr!("Mixed: keep the changes in the working tree"),
        ),
        (
            "Hard…",
            "hard",
            tr!("Hard: match the commit (uncommitted changes are stashed first)"),
        ),
    ];
    let children = modes
        .into_iter()
        .map(|(title, mode, what)| {
            let (a, h, b) = (actions.clone(), hex.to_owned(), branch.clone());
            menu::action(title, move || {
                // 動かし方と、外れたコミットの行き先（Reflog）と、戻せること。
                let message = format!(
                    "{what}\n{} {}",
                    tr!("Commits taken off stay in Reflog."),
                    crate::sheets::can_undo()
                );
                crate::sheets::confirm_then(
                    &a,
                    tr!(
                        "Reset {branch} to {id}?",
                        branch = b,
                        id = crate::titles::short(&h)
                    ),
                    message,
                    "Reset".to_owned(),
                    false,
                    vec![json!({ "op": "reset", "oid": h, "mode": mode })],
                );
            })
        })
        .collect();
    Some(Item::Submenu(crate::titles::reset(&branch, hex), children))
}

/// 複数選んだ行の上のメニュー（macOS 版の `buildMultipleMenu` と同じ）。`rows` は昇順（新しい順）。
pub fn menu_many(
    actions: &Rc<Actions>,
    history: &HistorySnapshot,
    rows: &[u32],
    widget: &gtk::Widget,
    range_diff: &crate::rangediff::Opener,
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
            .map(|(o, _)| o[..10].to_owned())
            .collect::<Vec<_>>(),
    );
    items.push(menu::action(tr!("Copy Short IDs"), move || {
        w.clipboard().set_text(&ids.join(" "));
    }));
    items
}
