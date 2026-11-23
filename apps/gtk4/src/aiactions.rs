//! AI のボタン（docs/ux.md の「AI」）: History の詳細の翻訳・解説（macOS 版と同じく、返事を流しながら別の窓に出す）、
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

/// History の詳細の翻訳と解説（返事は macOS 版と同じく別の窓に出す）。帯の表示の設定に出す項目を作る閉包を返す
/// （AI を使えないときは空。macOS 版と同じく「解説」「翻訳」の順）。
pub fn details(
    details: &Rc<Details>,
    diff: &DiffWidget,
    current: CurrentRepo,
) -> Rc<dyn Fn() -> Vec<crate::menu::Item>> {
    let (d, c, w) = (Rc::downgrade(details), current.clone(), diff.downgrade());
    let translate: Rc<dyn Fn()> = Rc::new(move || {
        let Some(details) = d.upgrade() else { return };
        let message = details.message().trim().to_owned();
        if message.is_empty() {
            return;
        }
        let parent = w
            .upgrade()
            .and_then(|w| w.root())
            .and_downcast::<gtk::Window>();
        let workdir = workdir(&c());
        glib::spawn_future_local(async move {
            let key = assist::repository_key(workdir.as_deref());
            let summary = tr!(
                "The message ({count} characters)",
                count = message.chars().count()
            );
            if !assist::confirm(parent.as_ref(), &key, &summary).await {
                return;
            }
            let p = parent.clone();
            assist::panel(
                parent.as_ref(),
                &tr!("Translation"),
                None,
                None,
                assist::work(move |run: Run| {
                    let vars = assist::variables([("message", message.clone())]);
                    assist::run_with(p.clone(), Purpose::Translate, vars, workdir.clone(), run)
                }),
            );
        });
    });

    let (d, w, c) = (Rc::downgrade(details), diff.downgrade(), current.clone());
    let explain: Rc<dyn Fn()> = Rc::new(move || {
        let (Some(details), Some(diff)) = (d.upgrade(), w.upgrade()) else {
            return;
        };
        let message = details.message();
        let source = diff.source();
        let files: Vec<String> = source.as_ref().map(|s| s.names()).unwrap_or_default();
        if files.is_empty() && message.trim().is_empty() {
            return;
        }
        let parent = diff.root().and_downcast::<gtk::Window>();
        let workdir = workdir(&c());
        glib::spawn_future_local(async move {
            let key = assist::repository_key(workdir.as_deref());
            let summary = tr!(
                "The message and the diff of {count} files",
                count = files.len()
            );
            if !assist::confirm(parent.as_ref(), &key, &summary).await {
                return;
            }
            let p = parent.clone();
            assist::panel(
                parent.as_ref(),
                &tr!("Explanation"),
                None,
                None,
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
                                // まだ取っていない差分（ファイルの多いコミット）は、ここで取る。
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
        });
    });
    // 確認用（`--ai translate,explain`）: コミットの詳細と差分が出てから押す。
    for (name, f) in [
        ("translate", translate.clone()),
        ("explain", explain.clone()),
    ] {
        let (d, w) = (Rc::downgrade(details), diff.downgrade());
        assist::auto(
            name,
            move || {
                d.upgrade().is_some_and(|d| !d.message().is_empty())
                    && w.upgrade()
                        .is_some_and(|w| w.is_mapped() && w.source().is_some())
            },
            move || f(),
        );
    }
    Rc::new(move || {
        if !assist::is_ready() {
            return Vec::new();
        }
        let (e, t) = (explain.clone(), translate.clone());
        vec![
            crate::menu::hint(
                tr!("AI: Explain This Commit"),
                tr!("AI: Explain this commit (or the combined diff of the selected commits)."),
                move || e(),
            ),
            crate::menu::hint(
                tr!("AI: Translate the Message"),
                tr!("AI: Translate the message."),
                move || t(),
            ),
        ]
    })
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
