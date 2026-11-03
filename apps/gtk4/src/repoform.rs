//! リポジトリの形（コアの `repositoryForm`。macOS 版の `RepositoryForm`）: shallow clone・partial clone・
//! sparse checkout・git maintenance への登録。サイドバーの印・Working Copy の見出し・リポジトリの設定の git の行と
//! 「取得とフォルダ」のページが使う。

use std::rc::Rc;
use std::sync::Arc;

use amonhen_core::Repository;
use amonhen_model::tr;
use gtk::glib;
use gtk::prelude::*;
use serde_json::{Value, json};

use crate::reposettings::{RepoSettings, help_label, label, page, section};
use crate::sheets::{Response, Sheet};

/// partial clone: 足りない中身を取ってくる remote と、取らずにおいたもの（`blob:none` など）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Partial {
    pub remote: String,
    pub filter: Option<String>,
}

/// sparse checkout: cone mode（フォルダで選ぶ）なら取り出しているフォルダ、そうでなければパターン。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sparse {
    pub cone: bool,
    pub folders: Vec<String>,
}

impl Sparse {
    /// `path`（ファイル）を取り出しているか（コアの `SparseCheckout::contains` と同じ）。cone mode は、取り出す
    /// フォルダの下すべてと、根とその途中のフォルダの直下のファイル。cone mode でなければ、取り出しているとみなす。
    pub fn contains(&self, path: &str) -> bool {
        let Some((parent, _)) = path.rsplit_once('/').filter(|_| self.cone) else {
            return true;
        };
        self.folders.iter().any(|folder| {
            parent == folder
                || parent.starts_with(&format!("{folder}/"))
                || folder.starts_with(&format!("{parent}/"))
        })
    }

    /// 1 文の説明（サイドバーの印・Working Copy の見出しのツールチップ）。
    pub fn help(&self) -> String {
        if !self.cone {
            return tr!("Sparse checkout: only the files matching its patterns are checked out.");
        }
        match self.folders.len() {
            0 => tr!("Sparse checkout: only the files at the top are checked out."),
            1 => tr!("Sparse checkout: only 1 folder is checked out."),
            n => tr!(
                "Sparse checkout: only {count} folders are checked out.",
                count = n
            ),
        }
    }
}

/// リポジトリの形。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Form {
    pub shallow: bool,
    pub partial: Option<Partial>,
    pub sparse: Option<Sparse>,
    pub maintenance: bool,
    pub working_tree: bool,
    pub git_version: Option<String>,
    /// `git fetch --refetch` が使える（git 2.36 から）。
    pub can_fetch_all_contents: bool,
    /// `git sparse-checkout` が使える（git 2.25 から）。
    pub can_sparse_checkout: bool,
    /// `git maintenance start` が使える（git 2.31 から）。
    pub can_maintain: bool,
}

impl Form {
    /// コアの答え（`info`）から読む。
    pub fn from_info(info: &Value) -> Self {
        let partial = info["partial"].as_object().map(|p| Partial {
            remote: p
                .get("remote")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            filter: p.get("filter").and_then(Value::as_str).map(str::to_owned),
        });
        let sparse = info["sparse"].as_object().map(|s| Sparse {
            cone: s.get("cone").and_then(Value::as_bool).unwrap_or(false),
            folders: s
                .get("folders")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|f| f.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
        });
        let yes = |key: &str| info[key].as_bool().unwrap_or(false);
        // 版が読めなければ使えるとみなす（コアが確かめる）。
        let can = |key: &str| info[key].as_bool().unwrap_or(true);
        Self {
            shallow: yes("shallow"),
            partial,
            sparse,
            maintenance: yes("maintenance"),
            working_tree: yes("workingTree"),
            git_version: info["gitVersion"].as_str().map(str::to_owned),
            can_fetch_all_contents: can("canFetchAllContents"),
            can_sparse_checkout: can("canSparseCheckout"),
            can_maintain: can("canMaintain"),
        }
    }

    /// 短い言い方と、その説明（リポジトリの設定の git の行の下に並べる）。ふつうの clone なら空。
    pub fn items(&self) -> Vec<(String, String)> {
        let mut items = Vec::new();
        if self.shallow {
            items.push((
                "shallow clone".to_owned(),
                tr!(
                    "Older history isn’t fetched. Fetch more in Fetching and Folders, or at the end of History."
                ),
            ));
        }
        if let Some(partial) = &self.partial {
            items.push((
                partial_title(partial),
                tr!(
                    "Partial clone: file contents are fetched from {remote} when they’re needed.",
                    remote = partial.remote
                ),
            ));
        }
        if let Some(sparse) = &self.sparse {
            items.push((tr!("Some folders only"), sparse.help()));
        }
        if self.maintenance {
            items.push((
                "maintenance".to_owned(),
                tr!("Registered with git maintenance, which tidies it up in the background."),
            ));
        }
        items
    }
}

/// partial clone の短い言い方。
pub fn partial_title(partial: &Partial) -> String {
    match &partial.filter {
        Some(filter) => tr!("Contents fetched when needed ({filter})", filter = filter),
        None => tr!("Contents fetched when needed"),
    }
}

/// git が古くて使えないときの理由（`host` は SSH 先の名前）。
pub fn too_old(needed: &str, version: Option<&str>, host: Option<&str>) -> String {
    let version = match host {
        Some(host) => format!("{} ({host})", version.unwrap_or("?")),
        None => version.unwrap_or("?").to_owned(),
    };
    tr!(
        "Needs git {needed} or later. This repository’s git is {version}.",
        needed = needed,
        version = version
    )
}

/// 裏でリポジトリの形を読む（読めなければ None）。
pub async fn load(repo: &Arc<Repository>) -> Option<Form> {
    crate::reposettings::run_one(repo, json!({ "op": "repositoryForm" }))
        .await
        .ok()
        .map(|info| Form::from_info(&info))
}

// 取得とフォルダのページ

/// 操作の行: 左に今の様子と、その下に 1 文の説明、右にボタン。使えなければボタンを止め、⚠ と理由を出す（macOS 版の
/// `ActionRow`）。
fn action_row(
    title: &str,
    help: &str,
    disabled: Option<&str>,
    controls: &[gtk::Widget],
) -> gtk::Widget {
    let widget = gtk::Box::new(gtk::Orientation::Vertical, 4);
    widget.set_margin_top(4);
    widget.set_margin_bottom(4);
    widget.set_margin_start(4);
    widget.set_margin_end(4);
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let t = label(title, &[]);
    t.set_hexpand(true);
    t.set_wrap(true);
    t.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    line.append(&t);
    for c in controls {
        c.set_valign(gtk::Align::Center);
        c.set_sensitive(disabled.is_none());
        line.append(c);
    }
    widget.append(&line);
    widget.append(&help_label(help));
    if let Some(reason) = disabled {
        let warning = label(&format!("⚠ {reason}"), &["settings-warning"]);
        warning.set_wrap(true);
        warning.set_margin_start(8);
        widget.append(&warning);
    }
    widget.upcast()
}

/// 取得とフォルダ（docs/ux.md の 4.5。macOS 版の `FetchingPage`）: shallow clone の履歴、partial clone の中身、
/// sparse checkout のフォルダ。そのリポジトリにあるものだけを出す（フォルダは作業ツリーがあればいつも）。最後に、
/// git maintenance への登録。読み直すたびに作り直す。
pub fn fetching_page(s: &Rc<RepoSettings>) -> gtk::Box {
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.set_vexpand(true);
    outer.append(&content);
    // 操作が走っている間は、右下で回る。
    let spinner = gtk::Spinner::new();
    spinner.set_halign(gtk::Align::End);
    spinner.set_margin_end(16);
    spinner.set_margin_bottom(10);
    outer.append(&spinner);
    let shown: Rc<std::cell::RefCell<Option<Form>>> = Rc::default();
    let was_running = Rc::new(std::cell::Cell::new(false));
    s.on_refresh(move |s| {
        let running = s.operating.get();
        spinner.set_spinning(running);
        spinner.set_visible(running);
        content.set_sensitive(!running);
        let finished = was_running.replace(running) && !running;
        let form = s.state.borrow().form.clone();
        // 同じ形なら作り直さない（スクロールの位置を保つ）。操作を終えたら、失敗して形が変わらなくても作り直す
        // （切り替えを今の形に戻す）。
        if *shown.borrow() == form && content.first_child().is_some() && !finished {
            return;
        }
        shown.replace(form.clone());
        while let Some(child) = content.first_child() {
            content.remove(&child);
        }
        let Some(form) = form else {
            let spinner = gtk::Spinner::new();
            spinner.set_spinning(true);
            spinner.set_margin_top(24);
            content.append(&spinner);
            return;
        };
        let mut sections = Vec::new();
        if form.shallow {
            sections.push(history_section(s));
        }
        if let Some(partial) = &form.partial {
            sections.push(contents_section(s, &form, partial));
        }
        if form.working_tree {
            sections.push(folders_section(s, &form));
        }
        if !form.shallow && form.partial.is_none() && !form.working_tree {
            let note = help_label(&tr!("This repository has all of its history and contents."));
            sections.push(section(None, &[note.upcast()]));
        }
        sections.push(maintenance_section(s, &form));
        content.append(&page(&sections));
    });
    outer
}

fn history_section(s: &Rc<RepoSettings>) -> gtk::Box {
    let more = crate::shallow::pull_down(&tr!("Fetch More"));
    more.set_tooltip_text(Some(&tr!(
        "Fetch older commits from the remote (git fetch --deepen)"
    )));
    let w = s.weak();
    more.connect_clicked(move |b| {
        let w = w.clone();
        let run: Rc<dyn Fn(Value)> = Rc::new(move |command| {
            if let Some(s) = w.upgrade() {
                s.run_operation(command);
            }
        });
        crate::shallow::popup_more(b, b.root().and_downcast(), run, Rc::new(|| None));
    });
    let all = gtk::Button::with_label(&tr!("Fetch All"));
    all.set_tooltip_text(Some(&tr!(
        "Fetch the whole history from the remote (git fetch --unshallow)"
    )));
    let w = s.weak();
    all.connect_clicked(move |_| {
        if let Some(s) = w.upgrade() {
            s.run_operation(json!({ "op": "unshallow" }));
        }
    });
    section(
        Some("History"),
        &[action_row(
            &tr!("Older history isn’t fetched (shallow clone)"),
            &tr!("Fetch more of it from the remote, as at the end of History."),
            None,
            &[more.upcast(), all.upcast()],
        )],
    )
}

fn contents_section(s: &Rc<RepoSettings>, form: &Form, partial: &Partial) -> gtk::Box {
    let button = gtk::Button::with_label(&tr!("Fetch All Contents…"));
    let (w, remote) = (s.weak(), partial.remote.clone());
    button.connect_clicked(move |b| {
        let (w, remote, parent) = (w.clone(), remote.clone(), b.root().and_downcast::<gtk::Window>());
        glib::spawn_future_local(async move {
            let sheet = Sheet::unnamed(
                parent.as_ref(),
                &tr!("Fetch All Contents?"),
                &tr!(
                    "Fetches the contents of every file in the history from {remote} (git fetch --refetch). In a large repository this can take long and use a lot of disk. Afterwards it’s no longer a partial clone.",
                    remote = remote
                ),
            );
            sheet.buttons(&tr!("Fetch All Contents"), false, None);
            if sheet.run().await == Response::Ok
                && let Some(s) = w.upgrade()
            {
                s.run_operation(json!({ "op": "fetchAllContents" }));
            }
        });
    });
    let reason = (!form.can_fetch_all_contents)
        .then(|| too_old("2.36", form.git_version.as_deref(), s.host.as_deref()));
    section(
        Some(&tr!("Contents")),
        &[action_row(
            &partial_title(partial),
            &tr!(
                "Diffs, blame and contents: searches fetch what they need from {remote}.",
                remote = partial.remote
            ),
            reason.as_deref(),
            &[button.upcast()],
        )],
    )
}

/// git maintenance への登録（コアの `maintenance`）。入れると `git maintenance start`、切ると `unregister`
/// （予定はほかのリポジトリも使うので残る）。登録してあれば、アプリは commit-graph を書かない（毎時 git が書く）。
/// git の予定は、Linux では systemd のタイマー（なければ cron）。
fn maintenance_section(s: &Rc<RepoSettings>, form: &Form) -> gtk::Box {
    let switch = gtk::Switch::new();
    switch.set_active(form.maintenance);
    let w = s.weak();
    switch.connect_state_set(move |_, on| {
        if let Some(s) = w.upgrade()
            && !s.quiet()
        {
            s.run_operation(json!({ "op": "maintenance", "on": on }));
        }
        glib::Propagation::Proceed
    });
    let help = match &s.host {
        Some(host) => tr!(
            "Every hour git prefetches and writes the commit-graph; it’s set in the global git config and the scheduler on {host} (git maintenance).",
            host = host
        ),
        None => tr!(
            "Every hour git prefetches and writes the commit-graph; it’s set in your global git config and systemd or cron (git maintenance)."
        ),
    };
    let reason = (!form.can_maintain)
        .then(|| too_old("2.31", form.git_version.as_deref(), s.host.as_deref()));
    section(
        Some(&tr!("Maintenance")),
        &[action_row(
            &tr!("Keep this repository maintained in the background"),
            &help,
            reason.as_deref(),
            &[switch.upcast()],
        )],
    )
}

fn folders_section(s: &Rc<RepoSettings>, form: &Form) -> gtk::Box {
    let reason = (!form.can_sparse_checkout)
        .then(|| too_old("2.25", form.git_version.as_deref(), s.host.as_deref()));
    let reason = reason.as_deref();
    let check_out_all = || {
        let button = gtk::Button::with_label(&tr!("Check Out All Folders…"));
        let w = s.weak();
        button.connect_clicked(move |b| check_out_all(&w, b.root().and_downcast()));
        button.upcast::<gtk::Widget>()
    };
    let mut rows = Vec::new();
    match &form.sparse {
        Some(sparse) if sparse.cone => {
            // `git clone --sparse` の直後は、最上位のファイルだけでフォルダはない。
            let (title, help) = if sparse.folders.is_empty() {
                (
                    tr!("Only the files at the top are checked out"),
                    tr!("Add the folders to check out with +."),
                )
            } else {
                (
                    tr!("Only these folders are checked out"),
                    tr!(
                        "Files at the top, and directly in the folders above these, are checked out too."
                    ),
                )
            };
            rows.push(action_row(&title, &help, reason, &[check_out_all()]));
            for folder in &sparse.folders {
                let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                line.set_margin_start(4);
                line.set_margin_end(4);
                let icon = crate::icon::Icon::default();
                icon.set(crate::icon::Kind::Folder, false);
                line.append(&icon);
                let name = label(folder, &["monospace"]);
                name.set_selectable(true);
                name.set_hexpand(true);
                line.append(&name);
                let remove = gtk::Button::with_label("−");
                remove.add_css_class("flat");
                remove.set_tooltip_text(Some(&tr!("Stop checking out this folder")));
                remove.set_sensitive(reason.is_none());
                let rest: Vec<String> = sparse
                    .folders
                    .iter()
                    .filter(|f| *f != folder)
                    .cloned()
                    .collect();
                let w = s.weak();
                remove.connect_clicked(move |_| {
                    if let Some(s) = w.upgrade() {
                        s.run_operation(json!({ "op": "sparseCheckout", "folders": rest }));
                    }
                });
                line.append(&remove);
                rows.push(line.upcast());
            }
            let add = gtk::Button::with_label("+");
            add.set_tooltip_text(Some(&tr!("Check out one more folder")));
            add.set_halign(gtk::Align::Start);
            add.set_sensitive(reason.is_none());
            let (w, folders) = (s.weak(), sparse.folders.clone());
            add.connect_clicked(move |_| add_folder(&w, folders.clone()));
            rows.push(add.upcast());
        }
        Some(sparse) => {
            // cone mode でないパターンは、ここでは変えない（git 2.37 から非推奨）。やめることだけできる。
            rows.push(action_row(
                &tr!("Only the files matching these patterns are checked out"),
                &tr!("The patterns aren’t in cone mode, so Amon Hen doesn’t change them."),
                reason,
                &[check_out_all()],
            ));
            for pattern in &sparse.folders {
                let l = label(pattern, &["monospace"]);
                l.set_selectable(true);
                l.set_margin_start(4);
                rows.push(l.upcast());
            }
        }
        None => {
            let button = gtk::Button::with_label(&tr!("Check Out Only Some Folders…"));
            let w = s.weak();
            button.connect_clicked(move |_| add_folder(&w, Vec::new()));
            rows.push(action_row(
                &tr!("All folders are checked out"),
                &tr!("In a large repository, you can check out only the folders you work in (sparse checkout)."),
                reason,
                &[button.upcast()],
            ));
        }
    }
    section(Some(&tr!("Folders")), &rows)
}

/// HEAD の木からフォルダを 1 つ選んで、取り出すフォルダに足す（sparse checkout でなければ、それだけにする）。
fn add_folder(weak: &std::rc::Weak<RepoSettings>, folders: Vec<String>) {
    let Some(s) = weak.upgrade() else { return };
    let first = folders.is_empty();
    let request = crate::filepicker::Request {
        title: if first {
            tr!("Check Out Only Some Folders")
        } else {
            tr!("Check Out One More Folder")
        },
        message: if first {
            tr!(
                "Only this folder is checked out, with the files at the top. Others are removed from disk until you check them out again; add more folders here later."
            )
        } else {
            tr!("Folders in the latest commit, including ones not checked out.")
        },
        kind: crate::filepicker::Kind::Folder,
        initial: "/".into(),
        ..Default::default()
    };
    let (repo, parent, weak) = (s.repo.clone(), s.window(), weak.clone());
    glib::spawn_future_local(async move {
        let Some(chosen) = choose_tree_folder(parent.as_ref(), request, repo).await else {
            return;
        };
        if folders.contains(&chosen) {
            return;
        }
        let mut folders = folders;
        folders.push(chosen);
        if let Some(s) = weak.upgrade() {
            s.run_operation(json!({ "op": "sparseCheckout", "folders": folders }));
        }
    });
}

/// HEAD の木のフォルダを選ぶ（根からの相対パス。取り消したら None）。
pub async fn choose_tree_folder(
    parent: Option<&gtk::Window>,
    request: crate::filepicker::Request,
    repo: Arc<Repository>,
) -> Option<String> {
    let chosen =
        crate::filepicker::choose(parent, request, crate::filepicker::Source::Tree(repo)).await?;
    let folder = chosen.path.trim_matches('/').to_owned();
    (!folder.is_empty()).then_some(folder)
}

/// 確かめてから、すべてのフォルダを取り出す（sparse checkout をやめる）。
fn check_out_all(weak: &std::rc::Weak<RepoSettings>, parent: Option<gtk::Window>) {
    let weak = weak.clone();
    glib::spawn_future_local(async move {
        let sheet = Sheet::unnamed(
            parent.as_ref(),
            &tr!("Check Out All Folders?"),
            &tr!(
                "Every folder is checked out again (git sparse-checkout disable). In a large repository this can take long and use a lot of disk."
            ),
        );
        sheet.buttons(&tr!("Check Out All"), false, None);
        if sheet.run().await == Response::Ok
            && let Some(s) = weak.upgrade()
        {
            s.run_operation(json!({ "op": "disableSparseCheckout" }));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_form_from_the_core() {
        let form = Form::from_info(&json!({
            "shallow": true,
            "partial": { "remote": "origin", "filter": "blob:none" },
            "sparse": { "cone": true, "folders": ["crates/core"] },
            "maintenance": false,
            "workingTree": true,
            "gitVersion": "2.43.0",
            "canFetchAllContents": true,
            "canSparseCheckout": true,
            "canMaintain": false,
        }));
        assert!(form.shallow && form.working_tree && !form.can_maintain);
        assert_eq!(
            form.partial,
            Some(Partial {
                remote: "origin".into(),
                filter: Some("blob:none".into())
            })
        );
        assert_eq!(form.sparse.as_ref().unwrap().folders, ["crates/core"]);
        let titles: Vec<String> = form.items().into_iter().map(|i| i.0).collect();
        assert_eq!(
            titles,
            [
                "shallow clone",
                "Contents fetched when needed (blob:none)",
                "Some folders only"
            ]
        );
        // ふつうの clone なら何も出さない。
        let plain = Form::from_info(&json!({ "shallow": false, "partial": null, "sparse": null }));
        assert!(plain.items().is_empty());
        assert!(plain.can_fetch_all_contents);
    }

    #[test]
    fn checks_out_files_as_cone_mode_does() {
        let sparse = Sparse {
            cone: true,
            folders: vec!["a/b".into()],
        };
        // 根と、途中のフォルダの直下のファイル、取り出すフォルダの下すべて。
        assert!(sparse.contains("top.txt"));
        assert!(sparse.contains("a/file"));
        assert!(sparse.contains("a/b/file"));
        assert!(sparse.contains("a/b/c/d"));
        assert!(!sparse.contains("a/c/file"));
        assert!(!sparse.contains("x/file"));
        // cone mode でなければ、取り出しているとみなす。
        let patterns = Sparse {
            cone: false,
            folders: vec!["/*.md".into()],
        };
        assert!(patterns.contains("x/file"));
    }

    #[test]
    fn says_how_many_folders_are_checked_out() {
        let help = |folders: &[&str]| {
            Sparse {
                cone: true,
                folders: folders.iter().map(|s| s.to_string()).collect(),
            }
            .help()
        };
        assert_eq!(
            help(&[]),
            "Sparse checkout: only the files at the top are checked out."
        );
        assert_eq!(
            help(&["a"]),
            "Sparse checkout: only 1 folder is checked out."
        );
        assert_eq!(
            help(&["a", "b"]),
            "Sparse checkout: only 2 folders are checked out."
        );
        assert_eq!(
            too_old("2.36", Some("2.30.1"), Some("box")),
            "Needs git 2.36 or later. This repository’s git is 2.30.1 (box)."
        );
    }
}
