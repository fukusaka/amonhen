//! Stashes・Reflog・Range Diff の右の列: 1 つのコミット（か stash）の詳細と、変更ファイルの一覧と差分。History の
//! 右の列と同じ部品（[`Details`]・[`CommitDiff`]）で、macOS 版がこの 3 つの画面に埋め込む
//! `CommitDetailViewController` に当たる。変更ファイルの一覧・畳む・Viewed・見出しの右クリック（Blame・
//! File History・パスのコピーなど）、AI の翻訳・解説、差分の中の検索は History と同じ。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use amonhen_core::Repository;
use amonhen_model::{Oid, tr};
use gtk::prelude::*;
use gtk::{gio, glib};

use crate::commitfiles::{self, CommitDiff, Prepared};
use crate::details::Details;
use crate::filetools::OnCommit;
use crate::menu;

pub struct CommitPane {
    /// 詳細（上）と、変更ファイルと差分（下）。
    pub root: gtk::Paned,
    pub details: Rc<Details>,
    files: Rc<CommitDiff>,
    /// 出しているコミットのリポジトリ（変更ファイルのメニュー・AI が使う）。
    repo: RefCell<Option<Arc<Repository>>>,
    /// 出しているコミット（stash なら None。Blame はコミットでだけ出す）。
    commit: RefCell<Option<String>>,
    /// 詳細の親・メッセージの中の ID、Blame・File History の行を押したら、History でそのコミットを選ぶ。
    on_commit: RefCell<Option<OnCommit>>,
    /// 裏で読んでいる数（`--snapshot` はなくなるまで待つ）。
    pending: Rc<Cell<u32>>,
    /// 選び直したら増やし、古い結果を捨てる。
    generation: Cell<u64>,
}

impl CommitPane {
    pub fn new(pending: Rc<Cell<u32>>) -> Rc<Self> {
        Rc::new_cyclic(|weak: &std::rc::Weak<Self>| {
            let current: crate::aiactions::CurrentRepo = {
                let weak = weak.clone();
                Rc::new(move || weak.upgrade().and_then(|p| p.repo.borrow().clone()))
            };
            let details = Rc::new(Details::new());
            details.show_style_switch();
            let files = Rc::new(CommitDiff::new(pending.clone(), current.clone()));
            let root = gtk::Paned::new(gtk::Orientation::Vertical);
            root.set_start_child(Some(&details.root));
            root.set_end_child(Some(&files.root));
            root.set_resize_start_child(false);
            root.set_position(170);
            // AI の翻訳・解説と、差分の中の検索・詳細を広げるボタン（詳細の右上。History と同じ）。
            let fit: Rc<dyn Fn()> = {
                let (r, d) = (root.downgrade(), details.clone());
                Rc::new(move || {
                    if let Some(r) = r.upgrade() {
                        crate::fit_details(&r, d.clone());
                    }
                })
            };
            crate::aiactions::details(&details, files.diff(), current, fit);
            details.tools.append(&files.find_button());
            details.tools.append(&files.expand_button());
            // 変更ファイルの右クリック: AI でこの hunk を解説、Blame（コミットのときだけ）・File History。
            let (w, dw, dt) = (weak.clone(), files.diff().downgrade(), details.clone());
            files.connect_file_items(move |path, y| {
                let Some(pane) = w.upgrade() else {
                    return Vec::new();
                };
                let (Some(repo), Some(window)) = (pane.repo.borrow().clone(), pane.window()) else {
                    return Vec::new();
                };
                let on_commit = pane
                    .on_commit
                    .borrow()
                    .clone()
                    .unwrap_or_else(|| Rc::new(|_| {}));
                let mut items = Vec::new();
                if let Some(y) = y
                    && let Some(item) = dw
                        .upgrade()
                        .and_then(|d| crate::aiactions::history_hunk_item(&d, &dt, &repo, y))
                {
                    items.push(item);
                    items.push(menu::Item::Separator);
                }
                if let Some(hex) = pane.commit.borrow().clone() {
                    let (repo, window, path, oc) = (
                        repo.clone(),
                        window.clone(),
                        path.to_owned(),
                        on_commit.clone(),
                    );
                    items.push(menu::action(
                        tr!("Blame at {commit}", commit = &hex[..8.min(hex.len())]),
                        move || {
                            crate::filetools::blame(
                                &window,
                                repo.clone(),
                                &path,
                                Some(&hex),
                                oc.clone(),
                            );
                        },
                    ));
                }
                let path = path.to_owned();
                items.push(menu::action("File History", move || {
                    crate::filetools::file_history(&window, repo.clone(), &path, on_commit.clone());
                }));
                items
            });
            Self {
                root,
                details,
                files,
                repo: RefCell::default(),
                commit: RefCell::default(),
                on_commit: RefCell::default(),
                pending,
                generation: Cell::new(0),
            }
        })
    }

    /// 詳細の親・子・メッセージの中の ID と、Blame・File History の窓の行を押したら `f(ID)` を呼ぶ（History で
    /// そのコミットを選ぶ。macOS 版の `detail.onSelectCommit`）。
    pub fn connect_select_commit(self: &Rc<Self>, f: OnCommit) {
        self.on_commit.replace(Some(f.clone()));
        let weak = Rc::downgrade(self);
        self.details.connect_links(
            Rc::new(move || weak.upgrade().and_then(|p| p.repo.borrow().clone())),
            f,
            None,
        );
    }

    fn window(&self) -> Option<gtk::ApplicationWindow> {
        self.root.root().and_downcast::<gtk::ApplicationWindow>()
    }

    /// 何も選んでいない: 詳細を消し、差分の代わりに「コミットを選ぶと…」を出す（macOS 版と同じ）。
    pub fn clear(&self) {
        self.generation.set(self.generation.get() + 1);
        self.commit.replace(None);
        self.details.clear();
        self.files
            .show_message(&tr!("Select a commit to see its changes."));
    }

    /// `repo` の `id` の詳細と変更を裏で読んで出す。`stash` なら stash の中身（untracked のファイルを含む）で、
    /// 時間のかかる分（署名の検証・含むブランチなど）は読まない（macOS 版と同じ）。全ファイルを並べるか・畳む
    /// ファイル・Viewed は History と同じ。
    pub fn show(self: &Rc<Self>, repo: Arc<Repository>, id: Oid, stash: bool) {
        let g = self.generation.get() + 1;
        self.generation.set(g);
        let hex = id.to_string();
        self.repo.replace(Some(repo.clone()));
        self.commit.replace((!stash).then(|| hex.clone()));
        self.details.clear();
        self.files.clear();
        // すぐ読める分を先に、時間のかかる分はその後に。
        for extras in [false, true] {
            if extras && stash {
                continue;
            }
            self.pending.set(self.pending.get() + 1);
            let (this, repo) = (self.clone(), repo.clone());
            glib::spawn_future_local(async move {
                let value = gio::spawn_blocking(move || {
                    if extras {
                        repo.commit_extras(&id)
                    } else {
                        repo.commit_info(&id)
                    }
                    .map_err(|e| e.to_string())
                })
                .await;
                this.pending.set(this.pending.get() - 1);
                if this.generation.get() != g {
                    return;
                }
                match value {
                    Ok(Ok(value)) if extras => this.details.show_extras(&value),
                    Ok(Ok(value)) => this.details.show(&value),
                    Ok(Err(e)) => eprintln!("コミットの詳細を読めない: {e}"),
                    Err(_) => return,
                }
                crate::fit_details(&this.root, this.details.clone());
            });
        }
        let viewed = commitfiles::viewed_paths(&hex);
        let all = commitfiles::stacked();
        self.pending.set(self.pending.get() + 1);
        let this = self.clone();
        glib::spawn_future_local(async move {
            // History にないコミット（どのブランチにもない）もあるので、History の行ではなくコミットから読む。
            let result = gio::spawn_blocking(move || {
                let changes = if stash {
                    repo.stash_changes(&id)
                } else {
                    repo.commit_changes(&id)
                }
                .map_err(|e| e.to_string())?;
                Prepared::from_changes(repo, changes, &viewed, all)
            })
            .await;
            this.pending.set(this.pending.get() - 1);
            if this.generation.get() != g {
                return;
            }
            match result {
                Ok(Ok(prepared)) => this.files.show(prepared, hex),
                Ok(Err(e)) => {
                    eprintln!("差分を読めない: {e}");
                    this.files.show_message(&tr!("Couldn’t load the diff"));
                }
                Err(_) => {}
            }
        });
    }
}
