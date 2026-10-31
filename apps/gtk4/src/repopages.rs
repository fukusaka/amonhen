//! リポジトリの設定の、表のあるページ（macOS 版の `RepositorySettings.swift` の RemotesTab・HooksLFSPage と
//! `RemoteSheet.swift`）と、コミットのメッセージのテンプレートのページ。

use std::rc::Rc;

use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gio, glib};
use serde_json::{Value, json};

use crate::reposettings::{Remote, RepoSettings, label, run_one};
use crate::sheets::{Response, Sheet};

/// 小さな表（行は id の文字列。中身は列の `bind` が id から引く）。行は多くないが、見えている行の部品しか作らない。
pub(crate) struct Table {
    pub view: gtk::ColumnView,
    model: gtk::StringList,
    selection: gtk::SingleSelection,
}

impl Table {
    pub fn new() -> Self {
        let model = gtk::StringList::new(&[]);
        let selection = gtk::SingleSelection::new(Some(model.clone()));
        selection.set_autoselect(false);
        selection.set_can_unselect(true);
        let view = gtk::ColumnView::new(Some(selection.clone()));
        view.set_show_row_separators(true);
        view.set_reorderable(false);
        Self {
            view,
            model,
            selection,
        }
    }

    /// 文字の列。セルの部品の名前は行の id（右クリックで行を見つける）。
    pub fn text_column(
        &self,
        title: &str,
        expand: bool,
        bind: impl Fn(&str, &gtk::Label) + 'static,
    ) -> gtk::ColumnViewColumn {
        self.column(
            title,
            expand,
            || {
                let l = gtk::Label::new(None);
                l.set_xalign(0.0);
                l.set_ellipsize(gtk::pango::EllipsizeMode::End);
                l
            },
            bind,
        )
    }

    /// 部品の列。
    pub fn column<W: IsA<gtk::Widget>>(
        &self,
        title: &str,
        expand: bool,
        setup: impl Fn() -> W + 'static,
        bind: impl Fn(&str, &W) + 'static,
    ) -> gtk::ColumnViewColumn {
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(move |_, item| {
            if let Some(item) = item.downcast_ref::<gtk::ListItem>() {
                item.set_child(Some(&setup()));
            }
        });
        factory.connect_bind(move |_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let (Some(id), Some(child)) = (
                item.item().and_downcast::<gtk::StringObject>(),
                item.child().and_downcast::<W>(),
            ) else {
                return;
            };
            child.set_widget_name(&id.string());
            bind(&id.string(), &child);
        });
        let column = gtk::ColumnViewColumn::new(Some(title), Some(factory));
        column.set_expand(expand);
        column.set_resizable(true);
        self.view.append_column(&column);
        column
    }

    /// 行を入れ替える（選んでいた行は、まだあれば選んだまま）。
    pub fn set_ids(&self, ids: &[String]) {
        let selected = self.selected();
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        self.model.splice(0, self.model.n_items(), &refs);
        let at = selected.and_then(|s| ids.iter().position(|i| *i == s));
        self.selection
            .set_selected(at.map_or(gtk::INVALID_LIST_POSITION, |i| i as u32));
    }

    pub fn selected(&self) -> Option<String> {
        self.selection
            .selected_item()
            .and_downcast::<gtk::StringObject>()
            .map(|s| s.string().into())
    }

    pub fn select(&self, id: &str) {
        let n = self.model.n_items();
        if let Some(i) = (0..n).find(|i| self.model.string(*i).as_deref() == Some(id)) {
            self.selection.set_selected(i);
        }
    }

    /// 選んだ行が変わったら呼ぶ。
    pub fn connect_selected(&self, f: impl Fn(Option<String>) + 'static) {
        self.selection.connect_selected_item_notify(move |s| {
            f(s.selected_item()
                .and_downcast::<gtk::StringObject>()
                .map(|s| s.string().into()))
        });
    }

    /// 右クリックで、その行を選んでからメニューを出す。
    pub fn context_menu(&self, menu: &gio::Menu) {
        let popover = gtk::PopoverMenu::from_model(Some(menu));
        popover.set_parent(&self.view);
        popover.set_has_arrow(false);
        popover.set_halign(gtk::Align::Start);
        let click = gtk::GestureClick::new();
        click.set_button(gtk::gdk::BUTTON_SECONDARY);
        let (view, model, selection) = (
            self.view.clone(),
            self.model.clone(),
            self.selection.clone(),
        );
        click.connect_pressed(move |_, _, x, y| {
            // 押したところのセル（部品の名前が行の id）の行を選ぶ。
            let mut widget = view.pick(x, y, gtk::PickFlags::DEFAULT);
            while let Some(w) = widget {
                let name = w.widget_name();
                let n = model.n_items();
                if let Some(i) = (0..n).find(|i| model.string(*i).as_deref() == Some(name.as_str()))
                {
                    selection.set_selected(i);
                    break;
                }
                widget = w.parent();
            }
            if selection.selected_item().is_none() {
                return;
            }
            popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.popup();
        });
        self.view.add_controller(click);
    }

    /// 表をスクロールの枠に入れる。
    pub fn scrolled(&self, height: i32) -> gtk::Frame {
        let scrolled = gtk::ScrolledWindow::builder()
            .child(&self.view)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .min_content_height(height)
            .max_content_height(height.max(320))
            .propagate_natural_height(true)
            .build();
        let frame = gtk::Frame::new(None);
        frame.set_child(Some(&scrolled));
        frame
    }
}

/// 文字の欄の内側の余白。
fn margins(view: &gtk::TextView, m: i32) {
    view.set_top_margin(m);
    view.set_bottom_margin(m);
    view.set_left_margin(m);
    view.set_right_margin(m);
}

/// ボタンの並び（左から）。
fn button_row(buttons: &[&gtk::Widget]) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    for w in buttons {
        b.append(*w);
    }
    b
}

/// 表のあるページ（スクロールしない。表が伸びる）。
fn table_page(children: &[&gtk::Widget]) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 8);
    b.set_margin_top(18);
    b.set_margin_bottom(18);
    b.set_margin_start(20);
    b.set_margin_end(20);
    for w in children {
        b.append(*w);
    }
    b
}

// MARK: リモート

/// 表の行（id）への操作。
type OnRow = Rc<dyn Fn(&Rc<RepoSettings>, String)>;

/// URL の先が何か: GitHub・GitLab などのサービス、パス、SSH 先、それ以外はホスト名。
pub(crate) fn remote_kind(url: &str) -> String {
    let lower = url.to_lowercase();
    for (host, name) in [
        ("github.com", "GitHub"),
        ("gitlab", "GitLab"),
        ("bitbucket", "Bitbucket"),
        ("codeberg", "Codeberg"),
    ] {
        if lower.contains(host) {
            return name.into();
        }
    }
    if ["/", "./", "../", "~", "file://"]
        .iter()
        .any(|p| lower.starts_with(p))
    {
        return tr!("A path");
    }
    let url_host = |rest: &str| {
        let authority = rest.split('/').next().unwrap_or_default();
        let host = authority.rsplit('@').next().unwrap_or_default();
        host.split(':').next().unwrap_or_default().to_owned()
    };
    for scheme in ["ssh://", "git+ssh://"] {
        if let Some(rest) = lower.strip_prefix(scheme) {
            return tr!("SSH host {host}", host = url_host(rest));
        }
    }
    // scp の形（user@host:path）。
    if !lower.contains("://")
        && let Some((left, _)) = url.split_once(':')
    {
        let host = left.rsplit('@').next().unwrap_or(left);
        return tr!("SSH host {host}", host = host);
    }
    if let Some((_, rest)) = lower.split_once("://") {
        let host = url_host(rest);
        if !host.is_empty() {
            return host;
        }
    }
    if url.is_empty() {
        String::new()
    } else {
        tr!("Other")
    }
}

/// リモートを消す前の確認（設定のページ、シートのどこから消しても同じ文言と同じボタン）。
async fn confirm_remove(parent: Option<&gtk::Window>, name: &str) -> bool {
    let sheet = Sheet::new(
        parent,
        &tr!("Remove {name}?", name = name),
        &tr!(
            "This removes the remote’s settings and its remote-tracking branches ({name}/…). The remote repository isn’t touched.",
            name = name
        ),
    );
    sheet.buttons(&tr!("Delete"), true, None);
    sheet.run().await == Response::Ok
}

fn remove_remote(s: &Rc<RepoSettings>, name: String) {
    let w = s.weak();
    let parent = s.window();
    glib::spawn_future_local(async move {
        if confirm_remove(parent.as_ref(), &name).await
            && let Some(s) = w.upgrade()
        {
            s.perform(
                vec![json!({"op": "changeRemote", "change": {"kind": "remove", "name": name}})],
                |_| {},
            );
        }
    });
}

fn prune_remote(s: &Rc<RepoSettings>, name: String) {
    s.perform(
        vec![json!({"op": "changeRemote", "change": {"kind": "prune", "name": name}})],
        |_| {},
    );
}

/// リモートの追加・編集のシート（docs/ux.md の 4.5）: 名前、fetch の URL、push の URL（空なら fetch と同じ）、URL の
/// 先が何か、保存する前の接続の確認（コアの `checkRemote`。SSH 先のリポジトリなら SSH 先から）。編集なら、ここから
/// 消すこともできる。
pub(crate) fn remote_sheet(s: &Rc<RepoSettings>, remote: Option<Remote>) {
    let (w, parent) = (s.weak(), s.window());
    let repo = s.repo.clone();
    glib::spawn_future_local(async move {
        let title = if remote.is_some() {
            tr!("Edit Remote")
        } else {
            tr!("Add Remote")
        };
        let sheet = Sheet::new(parent.as_ref(), &title, "");
        let name = gtk::Entry::builder()
            .text(remote.as_ref().map_or("origin", |r| r.name.as_str()))
            .hexpand(true)
            .activates_default(true)
            .build();
        let fetch = gtk::Entry::builder()
            .text(remote.as_ref().map_or("", |r| r.fetch_url.as_str()))
            .hexpand(true)
            .activates_default(true)
            .build();
        let push = gtk::Entry::builder()
            .text(remote.as_ref().map_or("", |r| {
                if r.push_url == r.fetch_url {
                    ""
                } else {
                    r.push_url.as_str()
                }
            }))
            .placeholder_text(tr!("Same as the fetch URL"))
            .hexpand(true)
            .activates_default(true)
            .build();
        let points = label("", &["dim-label"]);
        let grid = gtk::Grid::new();
        grid.set_row_spacing(8);
        grid.set_column_spacing(10);
        for (i, (text, widget)) in [
            (tr!("Name"), name.upcast_ref::<gtk::Widget>()),
            (tr!("Fetch URL"), fetch.upcast_ref()),
            (tr!("Push URL"), push.upcast_ref()),
            (tr!("Points to"), points.upcast_ref()),
        ]
        .into_iter()
        .enumerate()
        {
            let l = gtk::Label::new(Some(&text));
            l.set_xalign(1.0);
            grid.attach(&l, 0, i as i32, 1, 1);
            grid.attach(widget, 1, i as i32, 1, 1);
        }
        sheet.add(&grid);
        let check = gtk::Button::with_label(&tr!("Check Connection"));
        check.set_valign(gtk::Align::Center);
        let spinner = gtk::Spinner::new();
        let result = label("", &[]);
        result.set_wrap(true);
        result.set_max_width_chars(48);
        result.set_hexpand(true);
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        line.append(&check);
        line.append(&spinner);
        line.append(&result);
        sheet.add(&line);
        let save = if remote.is_some() {
            sheet.buttons(&tr!("Save"), false, Some(&tr!("Remove…")))
        } else {
            sheet.buttons(&tr!("Save"), false, None)
        };
        let update = {
            let (name, fetch, points, save, check) = (
                name.clone(),
                fetch.clone(),
                points.clone(),
                save.clone(),
                check.clone(),
            );
            move || {
                let url = fetch.text();
                points.set_text(&remote_kind(url.trim()));
                let filled = !name.text().trim().is_empty() && !url.trim().is_empty();
                save.set_sensitive(filled);
                check.set_sensitive(!url.trim().is_empty());
            }
        };
        update();
        let u = update.clone();
        name.connect_changed(move |_| u());
        let u = update.clone();
        fetch.connect_changed(move |_| u());
        let (f, sp, r, sv) = (fetch.clone(), spinner.clone(), result.clone(), save.clone());
        check.connect_clicked(move |b| {
            let url = f.text().trim().to_owned();
            if url.is_empty() {
                return;
            }
            // つないでいる間は保存しない（確かめた URL と違うものを保存しないよう）。
            b.set_sensitive(false);
            sv.set_sensitive(false);
            sp.start();
            r.set_text(&tr!("Connecting…"));
            r.remove_css_class("settings-error");
            r.remove_css_class("settings-ok");
            let (repo, b, sp, r, u) = (
                repo.clone(),
                b.clone(),
                sp.clone(),
                r.clone(),
                update.clone(),
            );
            glib::spawn_future_local(async move {
                let reply = run_one(&repo, json!({"op": "checkRemote", "url": url})).await;
                sp.stop();
                match reply {
                    Ok(c) => {
                        let mut text = tr!(
                            "Reachable: {branches} branches, {tags} tags",
                            branches = c["branches"].as_u64().unwrap_or(0),
                            tags = c["tags"].as_u64().unwrap_or(0)
                        );
                        if let Some(head) = c["head"].as_str() {
                            text += &tr!(", HEAD at {branch}", branch = head);
                        }
                        r.set_text(&text);
                        r.add_css_class("settings-ok");
                    }
                    Err(e) => {
                        r.set_text(e.trim());
                        r.add_css_class("settings-error");
                    }
                }
                b.set_sensitive(true);
                u();
            });
        });
        name.grab_focus();
        match sheet.run().await {
            Response::Ok => {
                let Some(s) = w.upgrade() else { return };
                let name = name.text().trim().to_owned();
                let fetch = fetch.text().trim().to_owned();
                let push = push.text().trim().to_owned();
                let mut changes: Vec<Value> = Vec::new();
                match &remote {
                    Some(r) => {
                        if r.name != name {
                            changes.push(json!({"kind": "rename", "from": r.name, "to": name}));
                        }
                        if r.fetch_url != fetch {
                            changes.push(json!({"kind": "setUrl", "name": name, "url": fetch}));
                        }
                        let had_push = if r.push_url != r.fetch_url {
                            r.push_url.as_str()
                        } else {
                            ""
                        };
                        if had_push != push {
                            changes.push(json!({"kind": "setPushUrl", "name": name,
                                "url": (!push.is_empty()).then_some(push)}));
                        }
                    }
                    None => {
                        changes.push(json!({"kind": "add", "name": name, "url": fetch}));
                        if !push.is_empty() {
                            changes.push(json!({"kind": "setPushUrl", "name": name, "url": push}));
                        }
                    }
                }
                s.perform(
                    changes
                        .into_iter()
                        .map(|c| json!({"op": "changeRemote", "change": c}))
                        .collect(),
                    |_| {},
                );
            }
            Response::Other => {
                if let (Some(s), Some(r)) = (w.upgrade(), remote) {
                    remove_remote(&s, r.name);
                }
            }
            Response::Cancel => {}
        }
    });
}

/// リモートのページ: 名前・URL・push の URL（同じなら「（同じ）」）・URL の先の表と、追加・編集・Prune・削除。
pub(crate) fn remotes_page(s: &Rc<RepoSettings>) -> gtk::Box {
    let table = Rc::new(Table::new());
    let remote = {
        let w = s.weak();
        move |name: &str| -> Option<Remote> {
            let s = w.upgrade()?;
            let st = s.state.borrow();
            st.remotes.iter().find(|r| r.name == name).cloned()
        }
    };
    let r = remote.clone();
    let name_column = table.text_column(&tr!("Name"), false, move |id, l| {
        l.set_text(&r(id).map(|r| r.name).unwrap_or_default());
    });
    name_column.set_fixed_width(110);
    let r = remote.clone();
    table.text_column("URL", true, move |id, l| {
        let url = r(id).map(|r| r.fetch_url).unwrap_or_default();
        l.set_text(&url);
        l.set_tooltip_text(Some(&url));
    });
    let r = remote.clone();
    table.text_column(&tr!("Push URL"), true, move |id, l| {
        let (text, same) = match r(id) {
            Some(r) if r.push_url != r.fetch_url => (r.push_url, false),
            _ => (tr!("(same)"), true),
        };
        l.set_text(&text);
        if same {
            l.add_css_class("dim-label");
        } else {
            l.remove_css_class("dim-label");
        }
    });
    let r = remote.clone();
    let kind_column = table.text_column(&tr!("Points to"), false, move |id, l| {
        l.set_text(&r(id).map(|r| remote_kind(&r.fetch_url)).unwrap_or_default());
        l.add_css_class("dim-label");
    });
    kind_column.set_fixed_width(140);

    // 右クリックのメニューとボタンが使う操作（選んだ行に）。
    let group = gio::SimpleActionGroup::new();
    let add_action = |name: &str, f: OnRow| {
        let action = gio::SimpleAction::new(name, None);
        let (w, t) = (s.weak(), table.clone());
        action.connect_activate(move |_, _| {
            if let (Some(s), Some(id)) = (w.upgrade(), t.selected()) {
                f(&s, id);
            }
        });
        group.add_action(&action);
        action
    };
    let r = remote.clone();
    let edit = add_action(
        "edit",
        Rc::new(move |s, id| {
            if let Some(remote) = r(&id) {
                remote_sheet(s, Some(remote));
            }
        }),
    );
    let prune = add_action("prune", Rc::new(prune_remote));
    let remove = add_action("remove", Rc::new(remove_remote));
    let menu = gio::Menu::new();
    menu.append(Some(&tr!("Edit…")), Some("remotes.edit"));
    menu.append(Some("Prune"), Some("remotes.prune"));
    let danger = gio::Menu::new();
    danger.append(Some(&tr!("Remove…")), Some("remotes.remove"));
    menu.append_section(None, &danger);
    table.context_menu(&menu);
    let e = edit.clone();
    table.view.connect_activate(move |_, _| e.activate(None));

    let add = gtk::Button::with_label(&tr!("Add…"));
    add.set_action_name(Some("remotes.add"));
    let action = gio::SimpleAction::new("add", None);
    let w = s.weak();
    action.connect_activate(move |_, _| {
        if let Some(s) = w.upgrade() {
            remote_sheet(&s, None);
        }
    });
    group.add_action(&action);
    let edit_button = gtk::Button::with_label(&tr!("Edit…"));
    edit_button.set_action_name(Some("remotes.edit"));
    let prune_button = gtk::Button::with_label("Prune");
    prune_button.set_action_name(Some("remotes.prune"));
    prune_button.set_tooltip_text(Some(&tr!(
        "Remove remote-tracking branches that no longer exist on the remote."
    )));
    let remove_button = gtk::Button::with_label(&tr!("Remove…"));
    remove_button.set_action_name(Some("remotes.remove"));
    remove_button.add_css_class("destructive-action");
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    let buttons = button_row(&[
        add.upcast_ref(),
        edit_button.upcast_ref(),
        prune_button.upcast_ref(),
        spacer.upcast_ref(),
        remove_button.upcast_ref(),
    ]);
    let enable = move |selected: bool| {
        edit.set_enabled(selected);
        prune.set_enabled(selected);
        remove.set_enabled(selected);
    };
    enable(false);
    let en = enable.clone();
    table.connect_selected(move |id| en(id.is_some()));
    let frame = table.scrolled(160);
    frame.set_vexpand(true);
    let page = table_page(&[frame.upcast_ref(), buttons.upcast_ref()]);
    page.insert_action_group("remotes", Some(&group));
    let t = table.clone();
    s.on_refresh(move |s| {
        let ids: Vec<String> = s
            .state
            .borrow()
            .remotes
            .iter()
            .map(|r| r.name.clone())
            .collect();
        t.set_ids(&ids);
        enable(t.selected().is_some());
    });
    page
}

// MARK: コミット

/// Settings > Git > Commits の共有のテンプレートの名前（Amon Hen のコミット欄だけが使う）。
const LIBRARY_KEY: &str = "amonhen.commitTemplate";
/// git の `commit.template`（ファイル）。
const FILE_KEY: &str = "commit.template";

/// メッセージのテンプレートの今の選び。共有のテンプレートの一覧は GTK4 の FE にまだないので、選べるのは
/// なし／このリポジトリ専用／ファイル。
#[derive(Clone, PartialEq, Eq)]
enum Choice {
    None,
    Own,
    File(String),
}

/// `path` がこのリポジトリ専用のテンプレートのファイルか。
fn is_own(s: &RepoSettings, path: &str) -> bool {
    s.state
        .borrow()
        .commit_template
        .as_ref()
        .is_some_and(|(_, p)| p == path)
        || path.ends_with("/amonhen/commit-template")
}

fn template_choice(s: &RepoSettings) -> Choice {
    match s.effective(FILE_KEY).filter(|e| !e.value.is_empty()) {
        None => Choice::None,
        Some(f) if is_own(s, &f.value) => Choice::Own,
        Some(f) => Choice::File(f.value),
    }
}

fn choice_value(c: &Choice) -> String {
    match c {
        Choice::None => String::new(),
        Choice::Own => "own".into(),
        Choice::File(f) => format!("file:{f}"),
    }
}

/// メッセージのテンプレートのページ: なし／このリポジトリ専用（`<git dir>/amonhen/commit-template` に置き、git の
/// `commit.template` を向ける。端末の git も使う）／ファイル…（`commit.template`）。専用のものはその下で書き、
/// ファイルは中身を見せる。
pub(crate) fn commits_page(s: &Rc<RepoSettings>) -> gtk::Box {
    let label_of = |s: &RepoSettings, value: &str| -> String {
        if value == "own" {
            return tr!("This repository’s own");
        }
        match value.strip_prefix("file:").filter(|p| !p.is_empty()) {
            Some(path) if is_own(s, path) => tr!("This repository’s own"),
            Some(path) => tr!("File: {path}", path = crate::reposettings::tilde(path)),
            None => tr!("None"),
        }
    };
    // 設定元を出すキー: 共有のテンプレートを空にして打ち消した「なし」なら `amonhen.commitTemplate`。
    let keys: crate::reposettings::Keys = Rc::new(|s| {
        if template_choice(s) == Choice::None && s.local(LIBRARY_KEY).as_deref() == Some("") {
            vec![LIBRARY_KEY.into(), FILE_KEY.into()]
        } else {
            vec![FILE_KEY.into(), LIBRARY_KEY.into()]
        }
    });
    let row = s.row(
        &tr!("Message template"),
        "",
        keys,
        Rc::new(|s, v| {
            if v.is_empty() {
                tr!("None")
            } else if is_own(s, v) {
                tr!("This repository’s own")
            } else {
                v.to_owned()
            }
        }),
        None,
    );
    let (w, e) = (s.weak(), row.error.clone());
    let picker = crate::reposettings::Picker::new(move |value| {
        let Some(s) = w.upgrade() else { return };
        let current = template_choice(&s);
        if s.quiet() || value == choice_value(&current) {
            return;
        }
        // 共有のテンプレートをこの段で選んでいれば外す（ファイルを選んだとき）。
        let drop_library =
            |s: &RepoSettings| s.local(LIBRARY_KEY).map(|_| (LIBRARY_KEY.to_owned(), None));
        match value.as_str() {
            "own" => {
                // 専用のファイルがまだなければ空で作る（commit.template もそれを向く）。あれば向け直すだけ。
                let template = s.state.borrow().commit_template.clone();
                match template {
                    Some((Some(_), path)) => {
                        let mut changes = vec![(FILE_KEY.to_owned(), Some(path))];
                        changes.extend(drop_library(&s));
                        s.write(changes, Some(&e));
                    }
                    _ => {
                        let changes: Vec<_> = drop_library(&s).into_iter().collect();
                        let e = e.clone();
                        s.perform(
                            vec![json!({"op": "saveCommitTemplate", "text": ""})],
                            move |s| s.write(changes, Some(&e)),
                        );
                    }
                }
            }
            "file" => {
                // git が読むファイルなので、SSH 先のリポジトリなら SSH 先のものを選ぶ。
                let initial = match current {
                    Choice::File(f) => f,
                    _ => "~".into(),
                };
                let (w, e) = (s.weak(), e.clone());
                s.choose_file(
                    &tr!("Message template"),
                    &tr!("Choose a commit message template file"),
                    initial,
                    move |path| {
                        if let Some(s) = w.upgrade() {
                            let mut changes = vec![(FILE_KEY.to_owned(), Some(path))];
                            changes.extend(drop_library(&s));
                            s.write(changes, Some(&e));
                        }
                    },
                );
            }
            v if v.starts_with("file:") => {
                if let Some(c) = drop_library(&s) {
                    s.write(vec![c], Some(&e));
                }
            }
            _ => {
                let mut changes = Vec::new();
                if s.local(FILE_KEY).is_some() {
                    changes.push((FILE_KEY.to_owned(), None));
                }
                // 広い段（すべてのリポジトリ）で共有のテンプレートを選んでいれば、この段では空にして打ち消す
                // （git は amonhen.* を読まないので、端末の git には関わらない）。広い段の commit.template の
                // ファイルは打ち消せない（空にすると git commit が止まる）ので、そのまま効く。
                if s.inherited(LIBRARY_KEY)
                    .is_some_and(|e| !e.value.is_empty())
                {
                    if s.local(LIBRARY_KEY).as_deref() != Some("") {
                        changes.push((LIBRARY_KEY.to_owned(), Some(String::new())));
                    }
                } else if s.local(LIBRARY_KEY).is_some() {
                    changes.push((LIBRARY_KEY.to_owned(), None));
                }
                s.write(changes, Some(&e));
            }
        }
    });
    row.control.append(&picker.button);

    // 専用のテンプレートの編集欄（窓の高さいっぱい）と、ファイルの中身。
    let editor = gtk::TextView::new();
    editor.set_monospace(true);
    margins(&editor, 6);
    let scrolled = gtk::ScrolledWindow::builder()
        .child(&editor)
        .vexpand(true)
        .min_content_height(160)
        .build();
    let frame = gtk::Frame::new(None);
    frame.set_child(Some(&scrolled));
    let revert = gtk::Button::with_label(&tr!("Revert"));
    let save = gtk::Button::with_label(&tr!("Save"));
    save.add_css_class("suggested-action");
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    buttons.append(&revert);
    buttons.append(&save);
    let own = gtk::Box::new(gtk::Orientation::Vertical, 6);
    own.set_vexpand(true);
    own.append(&frame);
    own.append(&buttons);
    let preview = label("", &["monospace", "dim-label"]);
    preview.set_selectable(true);
    preview.set_wrap(true);
    preview.set_lines(12);
    preview.set_ellipsize(gtk::pango::EllipsizeMode::End);
    preview.set_valign(gtk::Align::Start);
    preview.set_margin_start(8);

    let saved = Rc::new(std::cell::RefCell::new(String::new()));
    let buffer = editor.buffer();
    let dirty = {
        let (saved, revert, save) = (saved.clone(), revert.clone(), save.clone());
        move |b: &gtk::TextBuffer| {
            let text = b.text(&b.start_iter(), &b.end_iter(), false);
            let changed = text.as_str() != saved.borrow().as_str();
            revert.set_sensitive(changed);
            save.set_sensitive(changed);
        }
    };
    let d = dirty.clone();
    buffer.connect_changed(move |b| d(b));
    let do_save = {
        let (w, buffer) = (s.weak(), buffer.clone());
        Rc::new(move || {
            if let Some(s) = w.upgrade() {
                let text = buffer
                    .text(&buffer.start_iter(), &buffer.end_iter(), false)
                    .to_string();
                s.perform(
                    vec![json!({"op": "saveCommitTemplate", "text": text})],
                    |_| {},
                );
            }
        })
    };
    let f = do_save.clone();
    save.connect_clicked(move |_| f());
    let shortcuts = gtk::ShortcutController::new();
    shortcuts.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("<Control>s"),
        Some(gtk::CallbackAction::new(move |_, _| {
            do_save();
            glib::Propagation::Stop
        })),
    ));
    editor.add_controller(shortcuts);
    let (s2, b2) = (saved.clone(), buffer.clone());
    revert.connect_clicked(move |_| b2.set_text(&s2.borrow()));

    let (o, p) = (own.clone(), preview.clone());
    s.on_refresh(move |s| {
        let choice = template_choice(s);
        let mut items = vec![(String::new(), tr!("None"))];
        if s.level == crate::reposettings::Level::Repository {
            items.push(("own".into(), tr!("This repository’s own")));
        }
        if let Choice::File(f) = &choice {
            items.push((choice_value(&choice), label_of(s, &format!("file:{f}"))));
        }
        picker.set(
            &vec![(None, items)],
            &[("file".into(), tr!("A file…"))],
            &choice_value(&choice),
        );
        // 専用のテンプレート: 書きかけでなければ、読んだ中身に入れ替える。
        let text = s
            .state
            .borrow()
            .commit_template
            .as_ref()
            .and_then(|(t, _)| t.clone())
            .unwrap_or_default();
        let current = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
        if current.as_str() == saved.borrow().as_str() && current.as_str() != text {
            buffer.set_text(&text);
        }
        saved.replace(text);
        dirty(&buffer);
        o.set_visible(choice == Choice::Own);
        // ファイル: 中身を見せる（手元のリポジトリだけ。SSH 先は読まない）。
        let shown = match &choice {
            Choice::File(path) if s.host.is_none() => file_preview(s, path),
            _ => None,
        };
        p.set_text(shown.as_deref().unwrap_or_default());
        p.set_visible(shown.is_some_and(|t| !t.is_empty()));
    });

    let column = gtk::Box::new(gtk::Orientation::Vertical, 6);
    column.set_vexpand(true);
    column.append(&label(&tr!("Message"), &["heading"]));
    column.append(&crate::reposettings::section(None, &[row.widget.upcast()]));
    column.append(&preview);
    column.append(&own);
    table_page(&[column.upcast_ref()])
}

/// ファイルのテンプレートの中身（`~` はホーム、相対パスは作業ツリーから）。
fn file_preview(s: &RepoSettings, path: &str) -> Option<String> {
    let path = match path.strip_prefix('~') {
        Some(rest) => format!("{}{rest}", glib::home_dir().display()),
        None => path.to_owned(),
    };
    let mut full = std::path::PathBuf::from(&path);
    if full.is_relative() {
        full = s.repo.paths().workdir?.join(full);
    }
    std::fs::read_to_string(full)
        .ok()
        .map(|t| t.trim().to_owned())
}

// MARK: Hooks · LFS

/// git が呼ぶフックの名前（`git help hooks` の順。コアの一覧と同じ）。
const HOOK_NAMES: &[&str] = &[
    "applypatch-msg",
    "pre-applypatch",
    "post-applypatch",
    "pre-commit",
    "pre-merge-commit",
    "prepare-commit-msg",
    "commit-msg",
    "post-commit",
    "pre-rebase",
    "post-checkout",
    "post-merge",
    "pre-push",
    "pre-receive",
    "update",
    "proc-receive",
    "post-receive",
    "post-update",
    "reference-transaction",
    "push-to-checkout",
    "pre-auto-gc",
    "post-rewrite",
    "sendemail-validate",
    "fsmonitor-watchman",
    "p4-changelist",
    "p4-prepare-changelist",
    "p4-post-changelist",
    "p4-pre-submit",
    "post-index-change",
];

/// フックが動くとき（git の説明の要約）。
fn hook_when(hook: &str) -> String {
    match hook {
        "pre-commit" => tr!("Before a commit"),
        "pre-merge-commit" => tr!("Before a merge commit"),
        "prepare-commit-msg" => tr!("Before the message is edited"),
        "commit-msg" => tr!("On the commit message"),
        "post-commit" => tr!("After a commit"),
        "pre-rebase" => tr!("Before a rebase"),
        "post-checkout" => tr!("After a checkout"),
        "post-merge" => tr!("After a merge"),
        "pre-push" => tr!("Before a push"),
        "pre-auto-gc" => tr!("Before an automatic gc"),
        "post-rewrite" => tr!("After commits are rewritten"),
        "applypatch-msg" | "pre-applypatch" | "post-applypatch" => tr!("Around git am"),
        "post-index-change" => tr!("After the index changes"),
        "fsmonitor-watchman" => tr!("For fsmonitor"),
        "reference-transaction" => tr!("When refs change"),
        "push-to-checkout" | "pre-receive" | "update" | "post-receive" | "post-update"
        | "proc-receive" | "sendemail-validate" => tr!("On the receiving side"),
        _ => String::new(),
    }
}

/// フックの置き場所（手元のリポジトリだけ。SSH 先は開けない）。
fn hooks_directory(s: &RepoSettings) -> Option<std::path::PathBuf> {
    if s.host.is_some() {
        return None;
    }
    let paths = s.repo.paths();
    match s.state.borrow().hooks_path.as_deref() {
        None => Some(paths.git_dir.join("hooks")),
        Some(p) if p.starts_with('/') => Some(p.into()),
        Some(p) if p.starts_with('~') => Some(std::path::PathBuf::from(format!(
            "{}{}",
            glib::home_dir().display(),
            &p[1..]
        ))),
        Some(p) => paths.workdir.map(|w| w.join(p)),
    }
}

/// フックのファイルを開く（`ask` ならアプリを選んでもらう）か、ファイルの場所を開く。
fn open_hook(s: &RepoSettings, name: &str, how: &str) {
    let Some(file) = hooks_directory(s).map(|d| d.join(name)) else {
        return;
    };
    let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(file)));
    let window = s.window();
    let (w, how) = (s.widget.downgrade(), how.to_owned());
    glib::spawn_future_local(async move {
        let result = match how.as_str() {
            "reveal" => {
                launcher
                    .open_containing_folder_future(window.as_ref())
                    .await
            }
            _ => {
                launcher.set_always_ask(how == "ask");
                launcher.launch_future(window.as_ref()).await
            }
        };
        if let (Err(e), Some(_)) = (result, w.upgrade()) {
            eprintln!("フックを開けない: {e}");
        }
    });
}

/// フックのスクリプトに構文の色を付ける（言語は 1 行目の `#!` から。差分と同じ色）。
fn highlight(view: &gtk::TextView, name: &str, text: &str) {
    let buffer = view.buffer();
    buffer.set_text(text);
    let spans = amonhen_core::diff::text_syntax(name, text.as_bytes());
    if spans.is_empty() {
        return;
    }
    // UTF-16 の位置を文字の位置に。
    let mut units = Vec::with_capacity(text.len() + 1);
    for (i, c) in text.chars().enumerate() {
        for _ in 0..c.len_utf16() {
            units.push(i as i32);
        }
    }
    units.push(text.chars().count() as i32);
    let fg = view.color();
    let table = buffer.tag_table();
    for span in spans {
        let Some((r, g, b)) = crate::diff::syntax_color(span.kind, &fg) else {
            continue;
        };
        let name = format!("syntax-{}", span.kind);
        let tag = table.lookup(&name).unwrap_or_else(|| {
            let tag = gtk::TextTag::new(Some(&name));
            tag.set_foreground_rgba(Some(&gtk::gdk::RGBA::new(r, g, b, 1.0)));
            table.add(&tag);
            tag
        });
        let at = |u: u32| units.get(u as usize).copied().unwrap_or(0);
        let (start, end) = (
            buffer.iter_at_offset(at(span.start)),
            buffer.iter_at_offset(at(span.end)),
        );
        buffer.apply_tag(&tag, &start, &end);
    }
}

/// まだないフックから 1 つ選んで雛形を置き、エディタで開く。
fn create_hook(s: &Rc<RepoSettings>, table: Rc<Table>) {
    let present: Vec<String> = s
        .state
        .borrow()
        .hooks
        .iter()
        .map(|h| h.name.clone())
        .collect();
    let names: Vec<&str> = HOOK_NAMES
        .iter()
        .copied()
        .filter(|n| !present.iter().any(|p| p == n))
        .collect();
    if names.is_empty() {
        return;
    }
    let (w, parent) = (s.weak(), s.window());
    glib::spawn_future_local(async move {
        let sheet = Sheet::new(
            parent.as_ref(),
            &tr!("Create Hook"),
            &tr!("Puts a #!/bin/sh skeleton in place and makes it executable."),
        );
        let titles: Vec<String> = names
            .iter()
            .map(|n| match hook_when(n) {
                w if w.is_empty() => (*n).to_owned(),
                w => format!("{n} — {w}"),
            })
            .collect();
        let refs: Vec<&str> = titles.iter().map(String::as_str).collect();
        let choice = gtk::DropDown::from_strings(&refs);
        sheet.add(&choice);
        sheet.buttons(&tr!("Create"), false, None);
        if sheet.run().await != Response::Ok {
            return;
        }
        let Some(name) = names
            .get(choice.selected() as usize)
            .map(|n| (*n).to_owned())
        else {
            return;
        };
        let Some(s) = w.upgrade() else { return };
        s.perform(vec![json!({"op": "createHook", "name": name})], move |s| {
            // 読み直してから選ぶ（表の行ができてから）。
            let (w, name) = (s.weak(), name.clone());
            glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || {
                if let Some(s) = w.upgrade() {
                    table.select(&name);
                    open_hook(&s, &name, "open");
                }
            });
        });
    });
}

/// フックの表（名前・いつ動くか・有効＝実行の許可）と選んだフックの中身、LFS の様子と追跡するパターン。
pub(crate) fn hooks_page(s: &Rc<RepoSettings>) -> gtk::ScrolledWindow {
    let table = Rc::new(Table::new());
    let hook = {
        let w = s.weak();
        move |name: &str| {
            let s = w.upgrade()?;
            let st = s.state.borrow();
            st.hooks.iter().find(|h| h.name == name).cloned()
        }
    };
    let name_column = table.text_column(&tr!("Name"), false, |id, l| {
        l.set_text(id);
        l.add_css_class("monospace");
    });
    name_column.set_fixed_width(200);
    table.text_column(&tr!("Runs"), true, |id, l| {
        l.set_text(&hook_when(id));
        l.add_css_class("dim-label");
    });
    let w = s.weak();
    let h = hook.clone();
    let enabled = table.column(
        &tr!("Enabled"),
        false,
        move || {
            let switch = gtk::Switch::new();
            switch.set_halign(gtk::Align::Center);
            switch.set_tooltip_text(Some(&tr!(
                "The executable bit. git runs only executable hooks."
            )));
            let w = w.clone();
            switch.connect_active_notify(move |sw| {
                // 表を作り直すときに入れた値では書かない。
                let Some(s) = w.upgrade() else { return };
                let name = sw.widget_name();
                if s.quiet() || name.is_empty() {
                    return;
                }
                let current = s
                    .state
                    .borrow()
                    .hooks
                    .iter()
                    .find(|h| h.name == name.as_str())
                    .map(|h| h.enabled);
                if current.is_some_and(|c| c != sw.is_active()) {
                    s.perform(
                        vec![json!({"op": "setHookEnabled", "name": name.as_str(), "enabled": sw.is_active()})],
                        |_| {},
                    );
                }
            });
            switch
        },
        move |id, switch| {
            // 部品の名前（行の id）を空にしている間は、知らせで書かない。
            switch.set_widget_name("");
            switch.set_active(h(id).is_some_and(|h| h.enabled));
            switch.set_widget_name(id);
        },
    );
    enabled.set_fixed_width(80);

    let group = gio::SimpleActionGroup::new();
    let local = s.host.is_none();
    for (action, how) in [("open", "open"), ("open-with", "ask"), ("reveal", "reveal")] {
        let a = gio::SimpleAction::new(action, None);
        let (w, t) = (s.weak(), table.clone());
        a.connect_activate(move |_, _| {
            if let (Some(s), Some(name)) = (w.upgrade(), t.selected()) {
                open_hook(&s, &name, how);
            }
        });
        a.set_enabled(local);
        group.add_action(&a);
    }
    let create = gio::SimpleAction::new("create", None);
    let (w, t) = (s.weak(), table.clone());
    create.connect_activate(move |_, _| {
        if let Some(s) = w.upgrade() {
            create_hook(&s, t.clone());
        }
    });
    group.add_action(&create);
    let menu = gio::Menu::new();
    if local {
        menu.append(Some(&tr!("Open")), Some("hooks.open"));
        menu.append(Some(&tr!("Open With…")), Some("hooks.open-with"));
        menu.append(Some(&tr!("Show in Files")), Some("hooks.reveal"));
    }
    let more = gio::Menu::new();
    more.append(Some(&tr!("Create Hook…")), Some("hooks.create"));
    menu.append_section(None, &more);
    table.context_menu(&menu);
    table.view.connect_activate(|view, _| {
        let _ = view.activate_action("hooks.open", None);
    });

    let empty = label("", &["dim-label"]);
    empty.set_wrap(true);
    let frame = table.scrolled(120);
    let create_button = gtk::Button::with_label(&tr!("Create Hook…"));
    create_button.set_action_name(Some("hooks.create"));
    create_button.set_tooltip_text(Some(&tr!(
        "Puts a #!/bin/sh skeleton in place and makes it executable."
    )));
    let location = label("", &["dim-label", "caption"]);
    let line = button_row(&[create_button.upcast_ref(), location.upcast_ref()]);

    // 選んだフックの中身。
    let script_name = label("", &["monospace", "heading"]);
    script_name.set_hexpand(true);
    let open_button = gtk::MenuButton::new();
    open_button.set_label(&tr!("Open"));
    let open_menu = gio::Menu::new();
    open_menu.append(Some(&tr!("Open")), Some("hooks.open"));
    open_menu.append(Some(&tr!("Open With…")), Some("hooks.open-with"));
    open_menu.append(Some(&tr!("Show in Files")), Some("hooks.reveal"));
    open_button.set_menu_model(Some(&open_menu));
    open_button.set_visible(local);
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    header.set_margin_top(8);
    header.append(&script_name);
    header.append(&open_button);
    let script = gtk::TextView::new();
    script.set_monospace(true);
    script.set_editable(false);
    script.set_cursor_visible(false);
    margins(&script, 8);
    let script_frame = gtk::Frame::new(None);
    script_frame.set_child(Some(
        &gtk::ScrolledWindow::builder()
            .child(&script)
            .min_content_height(220)
            .build(),
    ));
    let script_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    script_box.append(&header);
    script_box.append(&script_frame);
    script_box.set_visible(false);

    // 選んだフックの中身を読む（裏で。読み終わる前に選び直したら捨てる）。
    let show_script = {
        let (w, script, script_name, script_box) = (
            s.weak(),
            script.clone(),
            script_name.clone(),
            script_box.clone(),
        );
        Rc::new(move |name: Option<String>| {
            let Some(s) = w.upgrade() else { return };
            let name = name.filter(|n| s.state.borrow().hooks.iter().any(|h| h.name == *n));
            script_box.set_visible(name.is_some());
            let Some(name) = name else { return };
            script_name.set_text(&name);
            let (repo, script, script_name) = (s.repo.clone(), script.clone(), script_name.clone());
            glib::spawn_future_local(async move {
                let text = run_one(&repo, json!({"op": "hook", "name": name}))
                    .await
                    .ok()
                    .and_then(|v| v["text"].as_str().map(str::to_owned))
                    .unwrap_or_default();
                if script_name.text() == name.as_str() {
                    highlight(&script, &name, &text);
                }
            });
        })
    };
    let f = show_script.clone();
    table.connect_selected(move |name| f(name));

    // LFS
    let lfs_status = label("", &[]);
    lfs_status.set_wrap(true);
    let no_patterns = label(&tr!("No LFS patterns in .gitattributes."), &["dim-label"]);
    let patterns = Rc::new(Table::new());
    patterns.text_column(&tr!("Pattern"), true, |id, l| {
        l.set_text(id);
        l.add_css_class("monospace");
    });
    let patterns_frame = patterns.scrolled(80);
    let track = gtk::Button::with_label(&tr!("Track Pattern…"));
    track.set_action_name(Some("hooks.track"));
    let track_action = gio::SimpleAction::new("track", None);
    let w = s.weak();
    track_action.connect_activate(move |_, _| {
        if let Some(s) = w.upgrade() {
            track_pattern(&s);
        }
    });
    group.add_action(&track_action);
    let remove = gtk::Button::with_label(&tr!("Remove"));
    remove.set_action_name(Some("hooks.untrack"));
    let untrack = gio::SimpleAction::new("untrack", None);
    let (w, p) = (s.weak(), patterns.clone());
    untrack.connect_activate(move |_, _| {
        if let (Some(s), Some(pattern)) = (w.upgrade(), p.selected()) {
            s.perform(
                vec![json!({"op": "lfsTrack", "pattern": pattern, "track": false})],
                |_| {},
            );
        }
    });
    group.add_action(&untrack);
    let lfs_buttons = button_row(&[track.upcast_ref(), remove.upcast_ref()]);
    let (u, s2) = (untrack.clone(), s.weak());
    patterns.connect_selected(move |p| {
        u.set_enabled(p.is_some() && s2.upgrade().is_some_and(|s| s.state.borrow().lfs));
    });

    let page = gtk::Box::new(gtk::Orientation::Vertical, 8);
    page.set_margin_top(18);
    page.set_margin_bottom(18);
    page.set_margin_start(20);
    page.set_margin_end(20);
    page.append(&label("Hooks", &["heading"]));
    page.append(&empty);
    page.append(&frame);
    page.append(&line);
    page.append(&script_box);
    let lfs = label("LFS", &["heading"]);
    lfs.set_margin_top(12);
    page.append(&lfs);
    page.append(&lfs_status);
    page.append(&no_patterns);
    page.append(&patterns_frame);
    page.append(&lfs_buttons);
    let (t, p) = (table.clone(), patterns.clone());
    let (fr, em, sb) = (frame.clone(), empty.clone(), show_script.clone());
    s.on_refresh(move |s| {
        let st = s.state.borrow();
        let names: Vec<String> = st.hooks.iter().map(|h| h.name.clone()).collect();
        t.set_ids(&names);
        fr.set_visible(!names.is_empty());
        em.set_visible(names.is_empty());
        em.set_text(&match st.hooks_path.as_deref() {
            Some(p) if p.starts_with('/') || p.starts_with('~') => tr!(
                "core.hooksPath ({path}) is outside the working tree, so its contents can’t be shown.",
                path = p
            ),
            _ => tr!("No hooks. .sample files aren’t counted."),
        });
        create.set_enabled(names.len() < HOOK_NAMES.len());
        location.set_text(&tr!(
            "Location: {path}",
            path = st
                .hooks_path
                .as_ref()
                .map_or_else(|| ".git/hooks".to_owned(), |p| format!("core.hooksPath = {p}"))
        ));
        lfs_status.set_text(&if st.lfs {
            match st.lfs_files {
                Some(n) => tr!("Installed. {count} files are in LFS.", count = n),
                None => tr!("Installed (filter.lfs is set)"),
            }
        } else {
            tr!("Not installed. Install it with your package manager (git-lfs) and git lfs install.")
        });
        lfs_status.set_css_classes(&if st.lfs || !st.lfs_patterns.is_empty() {
            vec![]
        } else {
            vec!["dim-label"]
        });
        if !st.lfs && !st.lfs_patterns.is_empty() {
            lfs_status.add_css_class("settings-warning");
        }
        p.set_ids(&st.lfs_patterns);
        patterns_frame.set_visible(!st.lfs_patterns.is_empty());
        no_patterns.set_visible(st.lfs_patterns.is_empty());
        track_action.set_enabled(st.lfs);
        untrack.set_enabled(st.lfs && p.selected().is_some());
        drop(st);
        // 中身が変わったかもしれない（有効を切り替えた、作った）。
        sb(t.selected());
    });

    let scrolled = gtk::ScrolledWindow::builder()
        .child(&page)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();
    // 操作はページのいちばん外に付ける（ページを出している間、どこからでも呼べる）。
    scrolled.insert_action_group("hooks", Some(&group));
    scrolled
}

/// LFS で追跡するパターンを足す（`git lfs track`）。
fn track_pattern(s: &Rc<RepoSettings>) {
    let (w, parent) = (s.weak(), s.window());
    glib::spawn_future_local(async move {
        let sheet = Sheet::new(
            parent.as_ref(),
            &tr!("Track Pattern"),
            &tr!("Files matching the pattern are stored in LFS from now on (git lfs track)."),
        );
        let entry = gtk::Entry::builder()
            .placeholder_text("*.psd")
            .activates_default(true)
            .build();
        sheet.add(&entry);
        sheet.buttons(&tr!("Track"), false, None);
        if sheet.run().await != Response::Ok {
            return;
        }
        let pattern = entry.text().trim().to_owned();
        if let (false, Some(s)) = (pattern.is_empty(), w.upgrade()) {
            s.perform(
                vec![json!({"op": "lfsTrack", "pattern": pattern, "track": true})],
                |_| {},
            );
        }
    });
}
