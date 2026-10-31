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
            .build();
        let frame = gtk::Frame::new(None);
        frame.set_child(Some(&scrolled));
        frame
    }
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
