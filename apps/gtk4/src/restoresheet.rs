//! 作業ツリーのファイルを別のコミットの版に戻す（macOS 版の `RestoreSheet.swift`）。History・Reflog の変更ファイルと
//! File History の右クリックから。index にも入れるかを選べる（既定は git restore と同じく作業ツリーだけ）。

use std::rc::Rc;

use amonhen_model::{ChangeKind, FileChange, tr};
use gtk::glib;
use gtk::prelude::*;
use serde_json::json;

use crate::actions::Actions;
use crate::menu::{self, Item};
use crate::sheets::{self, Response, Sheet};

/// コミット `commit` の変更 `change` から、「この版に戻す…」「このコミットの前の版に戻す…」。このコミットで消した
/// ファイルは、このコミットの版にはない。前の版に戻すと、このコミットで足したファイルは消え、名前を変えたファイルは
/// 元の名前に戻る。
pub fn items(actions: &Rc<Actions>, commit: &str, change: &FileChange) -> Vec<Item> {
    let mut items = Vec::new();
    if change.kind != ChangeKind::Deleted {
        let (a, c, p) = (actions.clone(), commit.to_owned(), change.path.clone());
        items.push(menu::action(tr!("Restore This Version…"), move || {
            ask(&a, &p, vec![p.clone()], &c, false)
        }));
    }
    let paths: Vec<String> = change
        .old_path
        .iter()
        .chain([&change.path])
        .cloned()
        .collect();
    let (a, c, p) = (actions.clone(), format!("{commit}^"), change.path.clone());
    items.push(menu::action(
        tr!("Restore Version Before This Commit…"),
        move || ask(&a, &p, paths.clone(), &c, true),
    ));
    items
}

/// `path` を `rev` の版（`before` なら `rev` の前の版）に戻してよいか確かめ、戻す。`paths` は実際に戻すパス。
pub fn ask(actions: &Rc<Actions>, path: &str, paths: Vec<String>, rev: &str, before: bool) {
    let (actions, path, rev) = (actions.clone(), path.to_owned(), rev.to_owned());
    glib::spawn_future_local(async move {
        let short: String = rev.chars().take(8).collect();
        let title = if before {
            tr!(
                "Restore {path} as It Was Before {commit}?",
                path = &path,
                commit = short.trim_end_matches('^')
            )
        } else {
            tr!(
                "Restore {path} as It Is in {commit}?",
                path = &path,
                commit = &short
            )
        };
        let sheet = Sheet::new(
            sheets::parent(&actions).as_ref(),
            &title,
            &tr!(
                "Replaces the file in the working tree with that version (git restore); a file that version doesn’t have is deleted. Undo brings back the file as it is now."
            ),
        );
        let staged = gtk::CheckButton::with_label(&tr!("Stage it too"));
        sheet.add(&staged);
        sheet.buttons(&tr!("Restore"), false, None);
        if sheet.run().await != Response::Ok {
            return;
        }
        actions.perform(json!({
            "op": "restoreFiles", "rev": rev, "paths": paths, "staged": staged.is_active(),
        }));
    });
}
