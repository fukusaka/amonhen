//! AI のボタン（docs/ux.md の「AI」）: History の詳細の翻訳・解説（メッセージの下に、畳める形で流しながら出す）、
//! Working Copy のメッセージの提案、差分の hunk の解説。どれも送る前にリポジトリごとの許可を確かめる
//! （[`assist::confirm`]）。

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use amonhen_assist::Purpose;
use amonhen_core::Repository;
use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gio, glib};

use crate::assist::{self, Run};
use crate::details::Details;
use crate::diff::DiffWidget;

/// 今のリポジトリ（なければ None）。
pub type CurrentRepo = Rc<dyn Fn() -> Option<Arc<Repository>>>;

fn workdir(repo: &Option<Arc<Repository>>) -> Option<PathBuf> {
    repo.as_ref().and_then(|r| r.paths().workdir)
}

/// 詳細の欄に、前に出した同じ種類の返事があれば消す。
fn remove_named(container: &gtk::Box, name: &str) {
    let mut child = container.first_child();
    while let Some(c) = child {
        child = c.next_sibling();
        if c.widget_name() == name {
            container.remove(&c);
        }
    }
}

/// History の詳細の右上に、翻訳と解説のボタンを置く。
pub fn details(details: &Rc<Details>, diff: &DiffWidget, current: CurrentRepo, fit: Rc<dyn Fn()>) {
    let translate = gtk::Button::with_label(&tr!("Translate"));
    let explain = gtk::Button::with_label(&tr!("Explain"));
    for b in [&explain, &translate] {
        b.add_css_class("flat");
        b.add_css_class("caption");
        details.tools.append(b);
    }
    assist::track(&translate, tr!("AI: Translate the message."));
    assist::track(
        &explain,
        tr!("AI: Explain this commit (or the combined diff of the selected commits)."),
    );

    let (d, c, f) = (Rc::downgrade(details), current.clone(), fit.clone());
    translate.connect_clicked(move |b| {
        let Some(details) = d.upgrade() else { return };
        let message = details.message().trim().to_owned();
        if message.is_empty() {
            return;
        }
        let parent = b.root().and_downcast::<gtk::Window>();
        let workdir = workdir(&c());
        let d = Rc::downgrade(&details);
        let fit = f.clone();
        glib::spawn_future_local(async move {
            let key = assist::repository_key(workdir.as_deref());
            let summary = tr!(
                "The message ({count} characters)",
                count = message.chars().count()
            );
            if !assist::confirm(parent.as_ref(), &key, &summary).await {
                return;
            }
            let Some(details) = d.upgrade() else { return };
            let p = parent.clone();
            let (root, view) = assist::inline(
                &tr!("Translation"),
                assist::work(move |run: Run| {
                    let vars = assist::variables([("message", message.clone())]);
                    assist::run_with(p.clone(), Purpose::Translate, vars, workdir.clone(), run)
                }),
            );
            root.set_widget_name("translation");
            remove_named(&details.ai, "translation");
            details.ai.prepend(&root);
            view.on_change.replace(Some(fit.clone()));
            fit();
        });
    });

    let (d, w, c) = (Rc::downgrade(details), diff.downgrade(), current.clone());
    explain.connect_clicked(move |b| {
        let (Some(details), Some(diff)) = (d.upgrade(), w.upgrade()) else {
            return;
        };
        let message = details.message();
        let source = diff.source();
        let files: Vec<String> = source
            .as_ref()
            .map(|s| s.0.files.iter().map(|(p, _)| p.clone()).collect())
            .unwrap_or_default();
        if files.is_empty() && message.trim().is_empty() {
            return;
        }
        let parent = b.root().and_downcast::<gtk::Window>();
        let workdir = workdir(&c());
        let d = Rc::downgrade(&details);
        let fit = fit.clone();
        glib::spawn_future_local(async move {
            let key = assist::repository_key(workdir.as_deref());
            let summary = tr!(
                "The message and the diff of {count} files",
                count = files.len()
            );
            if !assist::confirm(parent.as_ref(), &key, &summary).await {
                return;
            }
            let Some(details) = d.upgrade() else { return };
            let p = parent.clone();
            let (root, view) = assist::inline(
                &tr!("Explanation"),
                assist::work(move |run: Run| {
                    let (source, message, files, workdir, p) = (
                        source.clone(),
                        message.clone(),
                        files.join("\n"),
                        workdir.clone(),
                        p.clone(),
                    );
                    async move {
                        // 差分の文字は裏で作る（大きければ切る）。
                        let diff = gio::spawn_blocking(move || {
                            let mut out = amonhen_assist::DiffText::new();
                            if let Some(source) = source {
                                // 選んだファイルだけ読んであるなら、ここで全ファイルを読む。
                                let view = source.with_all(|all| {
                                    all.view(&[], &amonhen_projection::FrontendProfile::GTK)
                                });
                                crate::diff::ai_text(&view, None, &mut out);
                            }
                            out.finish()
                        })
                        .await
                        .unwrap_or_default();
                        let vars = assist::variables([
                            ("message", message),
                            ("diff", diff),
                            ("files", files),
                        ]);
                        assist::run_with(p, Purpose::ExplainCommit, vars, workdir, run).await
                    }
                }),
            );
            root.set_widget_name("explanation");
            remove_named(&details.ai, "explanation");
            details.ai.append(&root);
            view.on_change.replace(Some(fit.clone()));
            fit();
        });
    });
    // 確認用（`--ai translate,explain`）: コミットの詳細と差分が出てから押す。
    for (button, name) in [(translate, "translate"), (explain, "explain")] {
        let (b, c, d, w) = (
            button.downgrade(),
            button.downgrade(),
            Rc::downgrade(details),
            diff.downgrade(),
        );
        assist::auto(
            name,
            move || {
                b.upgrade()
                    .is_some_and(|b| b.is_mapped() && b.is_sensitive())
                    && d.upgrade().is_some_and(|d| !d.message().is_empty())
                    && w.upgrade().is_some_and(|w| w.source().is_some())
            },
            move || {
                if let Some(b) = c.upgrade() {
                    b.emit_clicked();
                }
            },
        );
    }
}

/// History の差分の右クリックのメニューに足す「AI でこの hunk を解説」（`y` の行が hunk の中で、AI を使える
/// とき）。
pub fn history_hunk_item(
    diff: &DiffWidget,
    details: &Details,
    repo: &Arc<Repository>,
    y: f64,
) -> Option<crate::menu::Item> {
    let (path, hunk) = diff.row_at(y).and_then(|row| diff.hunk_text(row))?;
    assist::hunk_item(
        diff.root().and_downcast::<gtk::Window>(),
        repo.paths().workdir,
        path.unwrap_or_default(),
        hunk,
        details.message(),
    )
}

/// 確認用（`--ai hunk`）: History の差分の最初の hunk を解説する。
pub fn auto_history_hunk(diff: &DiffWidget, details: &Rc<Details>, current: CurrentRepo) {
    let (a, b, d) = (diff.downgrade(), diff.downgrade(), Rc::downgrade(details));
    assist::auto(
        "hunk",
        move || a.upgrade().is_some_and(|diff| diff.first_hunk().is_some()),
        move || {
            let (Some(diff), Some(details)) = (b.upgrade(), d.upgrade()) else {
                return;
            };
            if let Some((path, hunk)) = diff.first_hunk() {
                assist::explain_hunk(
                    diff.root().and_downcast::<gtk::Window>(),
                    workdir(&current()),
                    path.unwrap_or_default(),
                    hunk,
                    details.message(),
                );
            }
        },
    );
}
