//! 窓のメニュー。項目の名前・並び・区切りは macOS 版（`MainMenu.swift`）と同じにし、置き場所は GNOME の
//! 決まり（HIG）に合わせる: 見出しの左の「Repository」は macOS の Repository メニュー、右端の主メニュー（≡）
//! には File・View・Navigate・Commit を同じ名前のサブメニューで入れ、アプリのこと（Keyboard Shortcuts・
//! About）を最後に置く。項目は窓のアクション（`win.*`）で、ショートカットは ⌘ を Ctrl に、⌃・⌥ を Alt に
//! 読み替える（Ubuntu が全体で使うもの、たとえば Ctrl+Alt+T は外す）。
//!
//! macOS 版にあって、ここにないもの: まだ機能のない項目（Edit・検索・表示の切り替え・設定・GitHub・タブ）。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use amonhen_core::RefsSnapshot;
use amonhen_model::{Oid, RefKind};
use gtk::prelude::*;
use gtk::{gio, glib};
use serde_json::json;

use crate::actions::Actions;
use crate::history::HistoryView;
use crate::icon;
use crate::refops;
use crate::sidebar::{Page, Sidebar};
use crate::workcopy::WorkingCopy;

/// メニューの項目が使う、窓の部品。
pub struct Parts {
    pub app: gtk::Application,
    pub window: gtk::ApplicationWindow,
    pub header: gtk::HeaderBar,
    pub actions: Rc<Actions>,
    pub pages: gtk::Stack,
    pub sidebar: Rc<Sidebar>,
    pub working: Rc<WorkingCopy>,
    pub view: HistoryView,
    pub refs: Rc<RefCell<Option<Arc<RefsSnapshot>>>>,
    /// History・ref・status を読み直す。
    pub reload: Rc<dyn Fn()>,
    /// リポジトリを新しい窓で開く。
    pub open: Rc<dyn Fn(PathBuf)>,
    /// History でそのコミットを選ぶ。
    pub go_to: Rc<dyn Fn(Oid)>,
}

/// 見出しのメニューとツールバーのボタン（`--open-menu` で開く、ref を読んだら数を出す）。
pub struct Buttons {
    pub repository: gtk::MenuButton,
    pub primary: gtk::MenuButton,
    branch: gtk::Label,
    branch_button: gtk::MenuButton,
    pull: gtk::Button,
    pull_count: gtk::Label,
    push: gtk::Button,
    push_count: gtk::Label,
    window: gtk::ApplicationWindow,
}

impl Buttons {
    /// 今のブランチ、上流との差（↓ 取り込める・↑ 送れる）、stash があるか。
    pub fn show_refs(&self, refs: &RefsSnapshot) {
        let head = refs.refs.head_branch.as_deref();
        self.branch.set_text(head.unwrap_or("detached"));
        self.branch_button.set_tooltip_text(Some(&format!(
            "{} — 押すとブランチを切り替える（Ctrl+Shift+B）",
            head.unwrap_or("detached HEAD")
        )));
        let entry = refs
            .refs
            .entries
            .iter()
            .find(|e| e.kind == RefKind::Local && Some(e.name.as_str()) == head);
        let (ahead, behind) = entry.map_or((0, 0), |e| (e.ahead, e.behind));
        let count = |label: &gtk::Label, n: u32, mark: &str| {
            label.set_text(&format!("{mark}{n}"));
            label.set_visible(n > 0);
        };
        count(&self.pull_count, behind, "↓");
        count(&self.push_count, ahead, "↑");
        self.pull.set_tooltip_text(Some(&if behind > 0 {
            format!("上流より {behind} コミット後ろにあります（Ctrl+Alt+L）")
        } else {
            "Pull（Ctrl+Alt+L）".to_owned()
        }));
        self.push.set_tooltip_text(Some(&if ahead > 0 {
            format!("上流より {ahead} コミット先にあります（Ctrl+Alt+P）")
        } else {
            "Push（Ctrl+Alt+P）".to_owned()
        }));
        let has_stash = refs.refs.entries.iter().any(|e| e.kind == RefKind::Stash);
        if let Some(action) = self
            .window
            .lookup_action("pop-stash")
            .and_downcast::<gio::SimpleAction>()
        {
            action.set_enabled(has_stash);
        }
    }
}

/// アイコンのボタン（`action` を実行する）。
fn icon_button(kind: icon::Kind, action: &str, tip: &str) -> gtk::Button {
    let icon = icon::Icon::default();
    icon.set(kind, false);
    let button = gtk::Button::new();
    button.set_child(Some(&icon));
    button.set_action_name(Some(action));
    button.set_tooltip_text(Some(tip));
    button
}

/// ▾ 付きのボタン（押すと `action`、▾ で `menu`）。アイコンの横に数を出せる。
fn split_button(
    kind: icon::Kind,
    action: &str,
    menu: &gio::Menu,
) -> (gtk::Box, gtk::Button, gtk::Label) {
    let icon = icon::Icon::default();
    icon.set(kind, false);
    let count = gtk::Label::new(None);
    count.add_css_class("caption");
    count.set_visible(false);
    let inner = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    inner.append(&icon);
    inner.append(&count);
    let button = gtk::Button::new();
    button.set_child(Some(&inner));
    button.set_action_name(Some(action));
    let more = gtk::MenuButton::builder().menu_model(menu).build();
    let group = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    group.add_css_class("linked");
    group.append(&button);
    group.append(&more);
    (group, button, count)
}

/// ボタンの群れの間の空き（macOS のツールバーの space）。
fn space() -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    b.set_width_request(8);
    b
}

/// メニューの項目を選んだときの処理。
type Handler = Box<dyn Fn(&Rc<Parts>)>;

/// (アクションの名前, ショートカット)。macOS 版のキーを読み替えたもの。
const ACCELS: &[(&str, &str)] = &[
    ("win.new-window", "<Control>n"),
    ("win.open", "<Control>o"),
    ("win.close", "<Control>w"),
    ("app.quit", "<Control>q"),
    ("win.show-working-copy", "<Control>1"),
    ("win.show-history", "<Control>2"),
    ("win.refresh", "<Control>r"),
    ("win.show-sidebar", "F9"),
    ("win.switch-branch", "<Control><Shift>b"),
    ("win.go-to-head", "<Control><Shift>h"),
    ("win.go-to-parent", "<Alt>Down"),
    ("win.go-to-child", "<Alt>Up"),
    ("win.fetch", "<Control><Alt>f"),
    ("win.pull", "<Control><Alt>l"),
    ("win.push", "<Control><Alt>p"),
    ("win.new-branch", "<Control><Alt>b"),
    ("win.merge", "<Control><Alt>m"),
    ("win.rebase", "<Control><Alt>r"),
    ("win.stash", "<Control><Alt>s"),
    ("win.pop-stash", "<Control><Alt><Shift>s"),
    ("win.undo", "<Control><Alt>z"),
    ("win.commit", "<Control>Return"),
    ("win.commit-and-push", "<Control><Shift>Return"),
    ("win.activity", "<Control><Alt>a"),
    ("win.settings", "<Control>comma"),
    ("win.shortcuts", "<Control>question"),
];

pub fn install(parts: Parts) -> Buttons {
    let parts = Rc::new(parts);
    let window = &parts.window;
    let add = |name: &str, f: Handler| {
        let action = gio::SimpleAction::new(name, None);
        let p = parts.clone();
        action.connect_activate(move |_, _| f(&p));
        window.add_action(&action);
    };
    let run = |command: serde_json::Value| -> Handler {
        Box::new(move |p| p.actions.perform(command.clone()))
    };

    // File
    add(
        "new-window",
        Box::new(|p| {
            if let Some(dir) = workdir(p) {
                (p.open)(dir);
            }
        }),
    );
    add("open", Box::new(open_folder));
    add("add-remote", Box::new(add_remote));
    add("close", Box::new(|p| p.window.close()));
    // View
    add(
        "show-working-copy",
        Box::new(|p| show_page(p, Page::WorkingCopy)),
    );
    add("show-history", Box::new(|p| show_page(p, Page::History)));
    add("refresh", Box::new(|p| (p.reload)()));
    let sidebar = gio::SimpleAction::new_stateful("show-sidebar", None, &true.to_variant());
    let p = parts.clone();
    sidebar.connect_activate(move |action, _| {
        let shown = !action.state().and_then(|s| s.get::<bool>()).unwrap_or(true);
        action.set_state(&shown.to_variant());
        p.sidebar.root.set_visible(shown);
    });
    window.add_action(&sidebar);
    // Navigate
    add("switch-branch", Box::new(switch_branch));
    add("go-to-head", Box::new(go_to_head));
    add("go-to-parent", Box::new(|p| go_to_relative(p, true)));
    add("go-to-child", Box::new(|p| go_to_relative(p, false)));
    // Repository
    add("fetch", run(json!({ "op": "fetch" })));
    add(
        "pull",
        Box::new(|p| {
            p.actions
                .perform(json!({ "op": "pull", "mode": crate::settings::pull_mode() }))
        }),
    );
    add("pull-merge", run(json!({ "op": "pull", "mode": "merge" })));
    add(
        "pull-rebase",
        run(json!({ "op": "pull", "mode": "rebase" })),
    );
    add(
        "pull-ff-only",
        run(json!({ "op": "pull", "mode": "fastForwardOnly" })),
    );
    add("push", Box::new(|p| p.actions.push(false)));
    add("force-push", Box::new(|p| p.actions.push(true)));
    add(
        "new-branch",
        Box::new(|p| refops::ask_new_branch(&p.actions, &start_point(p))),
    );
    add("rename-branch", Box::new(rename_branch));
    add("set-upstream", Box::new(set_upstream));
    add(
        "new-tag",
        Box::new(|p| refops::ask_new_tag(&p.actions, &start_point(p))),
    );
    add("merge", Box::new(|p| merge_or_rebase(p, false)));
    add("rebase", Box::new(|p| merge_or_rebase(p, true)));
    add("continue", run(json!({ "op": "continue" })));
    add("skip", run(json!({ "op": "skip" })));
    add("abort", run(json!({ "op": "abort" })));
    add("stash", Box::new(|p| p.actions.stash()));
    add("pop-stash", Box::new(pop_stash));
    add("undo", run(json!({ "op": "undo" })));
    add(
        "rebuild-history",
        Box::new(|p| {
            p.actions
                .perform_work(|repo| repo.rebuild_history().map(|_| ()));
        }),
    );
    add("open-in-terminal", Box::new(open_in_terminal));
    add("reveal-in-files", Box::new(reveal_in_files));
    // Commit
    add("commit", Box::new(|p| p.working.commit_from_menu(None)));
    add(
        "commit-and-push",
        Box::new(|p| {
            let actions = p.actions.clone();
            p.working
                .commit_from_menu(Some(Box::new(move || actions.push(false))));
        }),
    );
    add("stage-all", Box::new(|p| p.working.stage_all()));
    add(
        "amend-last-commit",
        Box::new(|p| {
            show_page(p, Page::WorkingCopy);
            p.working.amend_last_commit();
        }),
    );
    // アプリ
    add("settings", Box::new(|p| crate::settings::show(&p.window)));
    add(
        "activity",
        Box::new(|p| {
            let repo = p.view.current().map(|(r, _)| r);
            crate::activity::Activity::show(&p.window, repo, p.actions.clone(), crate::engine());
        }),
    );
    add("shortcuts", Box::new(show_shortcuts));
    add("about", Box::new(show_about));

    if parts.app.lookup_action("quit").is_none() {
        let quit = gio::SimpleAction::new("quit", None);
        let app = parts.app.downgrade();
        quit.connect_activate(move |_, _| {
            if let Some(app) = app.upgrade() {
                app.quit();
            }
        });
        parts.app.add_action(&quit);
    }
    for (action, accel) in ACCELS {
        parts.app.set_accels_for_action(action, &[accel]);
    }

    let repository = gtk::MenuButton::builder()
        .label("Repository")
        .menu_model(&repository_menu())
        .build();
    // 窓の左端にあるので、ボタンの左端からそろえて開く（中央だと窓の外にはみ出す）。
    if let Some(popover) = repository.popover() {
        popover.set_halign(gtk::Align::Start);
    }
    parts.header.pack_start(&repository);

    // ツールバー（macOS 版の既定の並び: Branch・Fetch・Pull・Push、Merge・Rebase、Stash・Pop、
    // 右に Refresh・Undo）。Activity と検索は、まだ機能がないので出さない。
    let branch = gtk::Label::new(Some("…"));
    let branch_icon = icon::Icon::default();
    branch_icon.set(icon::Kind::Branch, false);
    let branch_inner = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    branch_inner.append(&branch_icon);
    branch_inner.append(&branch);
    let branch_button = gtk::MenuButton::builder()
        .child(&branch_inner)
        .always_show_arrow(true)
        .menu_model(&menu_of(&[
            Some(("Switch Branch…", "win.switch-branch")),
            Some(("New Branch…", "win.new-branch")),
            Some(("Rename Branch…", "win.rename-branch")),
            Some(("Set Upstream…", "win.set-upstream")),
        ]))
        .build();
    parts.header.pack_start(&branch_button);
    parts.header.pack_start(&icon_button(
        icon::Kind::Fetch,
        "win.fetch",
        "すべてのリモートを fetch（Ctrl+Alt+F）",
    ));
    let (pull_group, pull, pull_count) = split_button(
        icon::Kind::Pull,
        "win.pull",
        &menu_of(&[
            Some(("Pull with Merge", "win.pull-merge")),
            Some(("Pull with Rebase", "win.pull-rebase")),
            Some(("Pull Fast-Forward Only", "win.pull-ff-only")),
        ]),
    );
    parts.header.pack_start(&pull_group);
    let (push_group, push, push_count) = split_button(
        icon::Kind::Push,
        "win.push",
        &menu_of(&[
            Some(("Push", "win.push")),
            Some(("Force Push (with Lease)…", "win.force-push")),
        ]),
    );
    parts.header.pack_start(&push_group);
    parts.header.pack_start(&space());
    parts.header.pack_start(&icon_button(
        icon::Kind::Merge,
        "win.merge",
        "ブランチ・タグ・選んだコミットを今のブランチへ merge（Ctrl+Alt+M）",
    ));
    parts.header.pack_start(&icon_button(
        icon::Kind::Rebase,
        "win.rebase",
        "今のブランチを、ブランチ・タグ・選んだコミットの上へ rebase（Ctrl+Alt+R）",
    ));
    parts.header.pack_start(&space());
    parts.header.pack_start(&icon_button(
        icon::Kind::StashPush,
        "win.stash",
        "作業中の変更を退避（Ctrl+Alt+S）",
    ));
    parts.header.pack_start(&icon_button(
        icon::Kind::StashPop,
        "win.pop-stash",
        "最新の stash を戻して消す（Ctrl+Alt+Shift+S）",
    ));

    let primary = gtk::MenuButton::builder()
        .menu_model(&primary_menu())
        .tooltip_text("Main Menu")
        .primary(true)
        .build();
    set_icon(&primary, "open-menu-symbolic", "≡");
    // 右端から: 主メニュー、Undo、Refresh。
    parts.header.pack_end(&primary);
    parts.header.pack_end(&parts.actions.undo);
    parts.header.pack_end(&icon_button(
        icon::Kind::Refresh,
        "win.refresh",
        "History・ブランチ・Working Copy を読み直す（Ctrl+R）",
    ));
    Buttons {
        repository,
        primary,
        branch,
        branch_button,
        pull,
        pull_count,
        push,
        push_count,
        window: parts.window.clone(),
    }
}

/// テーマにアイコンがあればアイコン、なければ文字（macOS の GTK などは GNOME のアイコンを持たない）。
fn set_icon(button: &gtk::MenuButton, icon: &str, fallback: &str) {
    let has_icon = gtk::IconTheme::for_display(&button.display()).has_icon(icon);
    if has_icon {
        button.set_icon_name(icon);
    } else {
        button.set_label(fallback);
    }
}

/// 区切りでまとめた項目（`None` が区切り）。
fn menu_of(items: &[Option<(&str, &str)>]) -> gio::Menu {
    let menu = gio::Menu::new();
    let mut section = gio::Menu::new();
    for item in items {
        match item {
            Some((label, action)) => section.append(Some(label), Some(action)),
            None => {
                menu.append_section(None, &section);
                section = gio::Menu::new();
            }
        }
    }
    menu.append_section(None, &section);
    menu
}

fn primary_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    let submenus = gio::Menu::new();
    submenus.append_submenu(
        Some("File"),
        &menu_of(&[
            Some(("New Window", "win.new-window")),
            Some(("Open…", "win.open")),
            Some(("Add Remote…", "win.add-remote")),
            None,
            Some(("Close", "win.close")),
        ]),
    );
    submenus.append_submenu(
        Some("View"),
        &menu_of(&[
            Some(("Working Copy", "win.show-working-copy")),
            Some(("History", "win.show-history")),
            None,
            Some(("Refresh", "win.refresh")),
            None,
            Some(("Show Sidebar", "win.show-sidebar")),
        ]),
    );
    submenus.append_submenu(
        Some("Navigate"),
        &menu_of(&[
            Some(("Switch Branch…", "win.switch-branch")),
            Some(("Go to HEAD", "win.go-to-head")),
            Some(("Go to Parent", "win.go-to-parent")),
            Some(("Go to Child", "win.go-to-child")),
        ]),
    );
    submenus.append_submenu(
        Some("Commit"),
        &menu_of(&[
            Some(("Commit", "win.commit")),
            Some(("Commit and Push", "win.commit-and-push")),
            Some(("Stage All", "win.stage-all")),
        ]),
    );
    menu.append_section(None, &submenus);
    menu.append_section(
        None,
        &menu_of(&[
            Some(("Settings", "win.settings")),
            Some(("Activity", "win.activity")),
            Some(("Keyboard Shortcuts", "win.shortcuts")),
            Some(("About Amon Hen", "win.about")),
        ]),
    );
    menu
}

fn repository_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    let sync = gio::Menu::new();
    sync.append(Some("Fetch"), Some("win.fetch"));
    sync.append(Some("Pull"), Some("win.pull"));
    sync.append_submenu(
        Some("Pull With"),
        &menu_of(&[
            Some(("Merge", "win.pull-merge")),
            Some(("Rebase", "win.pull-rebase")),
            Some(("Fast-Forward Only", "win.pull-ff-only")),
        ]),
    );
    sync.append(Some("Push"), Some("win.push"));
    sync.append(Some("Force Push (with Lease)…"), Some("win.force-push"));
    menu.append_section(None, &sync);
    let rest = menu_of(&[
        Some(("New Branch…", "win.new-branch")),
        Some(("Rename Branch…", "win.rename-branch")),
        Some(("Set Upstream…", "win.set-upstream")),
        Some(("New Tag…", "win.new-tag")),
        Some(("Merge…", "win.merge")),
        Some(("Rebase…", "win.rebase")),
        None,
        Some(("Continue", "win.continue")),
        Some(("Skip", "win.skip")),
        Some(("Abort", "win.abort")),
        None,
        Some(("Stash…", "win.stash")),
        Some(("Pop Stash", "win.pop-stash")),
        None,
        Some(("Undo", "win.undo")),
        None,
        Some(("Rebuild History", "win.rebuild-history")),
        None,
        Some(("Open in Terminal", "win.open-in-terminal")),
        Some(("Reveal in Files", "win.reveal-in-files")),
    ]);
    for i in 0..rest.n_items() {
        if let Some(section) = rest.item_link(i, gio::MENU_LINK_SECTION) {
            menu.append_section(None, &section);
        }
    }
    menu
}

fn show_page(p: &Parts, page: Page) {
    p.pages.set_visible_child_name(match page {
        Page::WorkingCopy => "working",
        Page::History => "history",
    });
    p.sidebar.select_page(page);
}

fn workdir(p: &Parts) -> Option<PathBuf> {
    p.view.current()?.0.paths().workdir
}

fn head_branch(p: &Parts) -> Option<String> {
    p.refs.borrow().as_ref()?.refs.head_branch.clone()
}

/// History で選んでいるコミット（History を出しているときだけ）。
fn selected_commit(p: &Parts) -> Option<Oid> {
    if p.pages.visible_child_name().as_deref() != Some("history") {
        return None;
    }
    let (_, history) = p.view.current()?;
    history
        .graph
        .ids()
        .get(p.view.selected()? as usize)
        .copied()
}

/// 新しいブランチ・タグの起点。History で選んでいるコミットがあればそれ、なければ今のブランチ。
fn start_point(p: &Parts) -> String {
    match selected_commit(p) {
        Some(id) => id.to_string()[..12].to_owned(),
        None => head_branch(p).unwrap_or_else(|| "HEAD".into()),
    }
}

fn rename_branch(p: &Rc<Parts>) {
    let Some(branch) = head_branch(p) else {
        return p.window.error_bell();
    };
    let actions = p.actions.clone();
    glib::spawn_future_local(async move {
        let message = format!("{branch} の新しい名前");
        if let Some(to) = actions
            .ask("Rename Branch", &message, &branch, "変更")
            .await
            && to != branch
        {
            actions.perform(json!({ "op": "renameBranch", "from": branch, "to": to }));
        }
    });
}

/// 今のブランチの上流（pull・push の相手）を、リモートのブランチから選ぶ。
fn set_upstream(p: &Rc<Parts>) {
    let Some(branch) = head_branch(p) else {
        return p.window.error_bell();
    };
    let remotes: Vec<(String, String)> = p
        .refs
        .borrow()
        .as_ref()
        .map(|r| {
            r.refs
                .entries
                .iter()
                .filter(|e| e.kind == RefKind::Remote && !e.name.ends_with("/HEAD"))
                .map(|e| (e.name.clone(), e.name.clone()))
                .collect()
        })
        .unwrap_or_default();
    if remotes.is_empty() {
        return p.window.error_bell();
    }
    let same = remotes
        .iter()
        .position(|(n, _)| n.ends_with(&format!("/{branch}")))
        .unwrap_or(0);
    let actions = p.actions.clone();
    glib::spawn_future_local(async move {
        let title = format!("{branch} の上流");
        let Some(upstream) = actions
            .choose(
                &title,
                "pull で取り込み、push で送るリモートのブランチ",
                &remotes,
                same,
                "設定",
            )
            .await
        else {
            return;
        };
        let Some((remote, remote_branch)) = upstream.split_once('/') else {
            return;
        };
        actions.perform_sequence(vec![
            json!({ "op": "configSet", "key": format!("branch.{branch}.remote"), "value": remote }),
            json!({ "op": "configSet", "key": format!("branch.{branch}.merge"), "value": format!("refs/heads/{remote_branch}") }),
        ]);
    });
}

/// Merge・Rebase の相手の候補: History で選んでいるコミット（あれば先頭）、今のブランチ以外のローカル
/// ブランチ、リモートブランチ、タグ。
fn revision_choices(p: &Parts) -> Vec<(String, String)> {
    let mut choices = Vec::new();
    let refs = p.refs.borrow();
    let head = refs.as_ref().and_then(|r| r.refs.head);
    if let Some(id) = selected_commit(p)
        && Some(id) != head
    {
        let hex = id.to_string();
        choices.push((format!("選んでいるコミット {}", &hex[..8]), hex));
    }
    if let Some(r) = refs.as_ref() {
        for kind in [RefKind::Local, RefKind::Remote, RefKind::Tag] {
            for e in r.refs.entries.iter().filter(|e| e.kind == kind) {
                if Some(&e.name) != r.refs.head_branch.as_ref() {
                    choices.push((e.name.clone(), e.name.clone()));
                }
            }
        }
    }
    choices
}

fn merge_or_rebase(p: &Rc<Parts>, rebase: bool) {
    let choices = revision_choices(p);
    if choices.is_empty() {
        return p.window.error_bell();
    }
    let current = head_branch(p).unwrap_or_else(|| "HEAD".into());
    let actions = p.actions.clone();
    glib::spawn_future_local(async move {
        let (title, message, ok) = if rebase {
            (
                format!("{current} を rebase する"),
                "上に載せる先のブランチ・タグ・コミット",
                "Rebase",
            )
        } else {
            (
                format!("{current} に merge する"),
                "取り込むブランチ・タグ・コミット",
                "Merge",
            )
        };
        if let Some(rev) = actions.choose(&title, message, &choices, 0, ok).await {
            actions.perform(if rebase {
                json!({ "op": "rebase", "onto": rev })
            } else {
                json!({ "op": "merge", "rev": rev })
            });
        }
    });
}

/// ブランチを選んで切り替える（リモートブランチなら、追跡するローカルブランチを作る）。
fn switch_branch(p: &Rc<Parts>) {
    let mut choices = Vec::new();
    if let Some(r) = p.refs.borrow().as_ref() {
        for e in &r.refs.entries {
            match e.kind {
                RefKind::Local if Some(&e.name) != r.refs.head_branch.as_ref() => {
                    choices.push((e.name.clone(), format!("local:{}", e.name)));
                }
                RefKind::Remote if !e.name.ends_with("/HEAD") => {
                    choices.push((e.name.clone(), format!("remote:{}", e.name)));
                }
                _ => {}
            }
        }
    }
    if choices.is_empty() {
        return p.window.error_bell();
    }
    let actions = p.actions.clone();
    glib::spawn_future_local(async move {
        let Some(choice) = actions
            .choose(
                "Switch Branch",
                "切り替えるブランチ",
                &choices,
                0,
                "切り替え",
            )
            .await
        else {
            return;
        };
        match choice.split_once(':') {
            Some(("local", name)) => actions.perform(json!({ "op": "checkout", "branch": name })),
            Some((_, name)) => actions.perform(json!({ "op": "track", "remoteBranch": name })),
            None => {}
        }
    });
}

fn go_to_head(p: &Rc<Parts>) {
    let head = p.refs.borrow().as_ref().and_then(|r| r.refs.head);
    match head {
        Some(id) => {
            show_page(p, Page::History);
            (p.go_to)(id);
        }
        None => p.window.error_bell(),
    }
}

/// 選んでいるコミットの最初の親（`parent`）か、子のうち最も近いもの。
fn go_to_relative(p: &Rc<Parts>, parent: bool) {
    let (Some(row), Some((_, history))) = (p.view.selected(), p.view.current()) else {
        return p.window.error_bell();
    };
    let graph = &history.graph;
    let target = if parent {
        graph.parents(row as usize).first().copied()
    } else {
        // 子は上の行にある（子が親より先に並ぶ）。
        (0..row)
            .rev()
            .find(|&r| graph.parents(r as usize).contains(&row))
    };
    match target {
        Some(r) => p.view.select(r),
        None => p.window.error_bell(),
    }
}

/// 最新の stash（`stash@{0}`）を戻して消す。
fn pop_stash(p: &Rc<Parts>) {
    let latest = p.refs.borrow().as_ref().and_then(|r| {
        r.refs
            .entries
            .iter()
            .find(|e| e.kind == RefKind::Stash)
            .map(|e| e.name.clone())
    });
    match latest {
        Some(name) => p
            .actions
            .perform(json!({ "op": "stashApply", "name": name, "pop": true })),
        None => p.window.error_bell(),
    }
}

/// 作業ツリーで端末を開く。デスクトップごとに端末が違うので、あるものを順に試す。
fn open_in_terminal(p: &Rc<Parts>) {
    let Some(dir) = workdir(p) else {
        return p.window.error_bell();
    };
    let dir_arg = dir.display().to_string();
    let candidates: Vec<(&str, Vec<String>)> = if cfg!(target_os = "macos") {
        vec![("open", vec!["-a".into(), "Terminal".into(), dir_arg])]
    } else {
        vec![
            ("xdg-terminal-exec", vec![]),
            ("ptyxis", vec![format!("--working-directory={dir_arg}")]),
            (
                "gnome-terminal",
                vec![format!("--working-directory={dir_arg}")],
            ),
            ("kgx", vec![format!("--working-directory={dir_arg}")]),
            ("konsole", vec!["--workdir".into(), dir_arg.clone()]),
            ("x-terminal-emulator", vec![]),
        ]
    };
    for (program, args) in candidates {
        let started = std::process::Command::new(program)
            .args(&args)
            .current_dir(&dir)
            .spawn();
        if started.is_ok() {
            return;
        }
    }
    p.window.error_bell();
}

/// 作業ツリーを、ファイルマネージャーで（親のフォルダの中で選んだ状態で）見せる。
fn reveal_in_files(p: &Rc<Parts>) {
    let Some(dir) = workdir(p) else {
        return p.window.error_bell();
    };
    let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(dir)));
    let window = p.window.clone();
    glib::spawn_future_local(async move {
        if launcher
            .open_containing_folder_future(Some(&window))
            .await
            .is_err()
        {
            window.error_bell();
        }
    });
}

/// daemon・SSH 越しのリポジトリを、新しい窓で開く。
fn add_remote(p: &Rc<Parts>) {
    let (actions, open) = (p.actions.clone(), p.open.clone());
    glib::spawn_future_local(async move {
        if let Some(location) = actions
            .ask(
                "Add Remote",
                "ssh://[user@]host[:port]/path（SSH 先の daemon）か daemon:/path（手元の共有の daemon）",
                "ssh://",
                "開く",
            )
            .await
            && amonhen_remote::location::is_remote(&location)
        {
            open(PathBuf::from(location));
        }
    });
}

fn open_folder(p: &Rc<Parts>) {
    let dialog = gtk::FileDialog::builder().title("Open").modal(true).build();
    let (window, open) = (p.window.clone(), p.open.clone());
    glib::spawn_future_local(async move {
        if let Ok(folder) = dialog.select_folder_future(Some(&window)).await
            && let Some(path) = folder.path()
        {
            open(path);
        }
    });
}

fn show_shortcuts(p: &Rc<Parts>) {
    let group = |title: &str, items: &[(&str, &str)]| {
        let group = gtk::ShortcutsGroup::builder().title(title).build();
        for (action, label) in items {
            let accel = ACCELS
                .iter()
                .find(|(a, _)| a == action)
                .map_or(*action, |(_, k)| *k);
            let shortcut = gtk::ShortcutsShortcut::builder()
                .accelerator(accel)
                .title(*label)
                .build();
            group.add_shortcut(&shortcut);
        }
        group
    };
    let section = gtk::ShortcutsSection::builder()
        .section_name("main")
        .build();
    section.add_group(&group(
        "File",
        &[
            ("win.new-window", "New Window"),
            ("win.open", "Open…"),
            ("win.close", "Close"),
            ("app.quit", "Quit"),
        ],
    ));
    section.add_group(&group(
        "View",
        &[
            ("win.show-working-copy", "Working Copy"),
            ("win.show-history", "History"),
            ("win.refresh", "Refresh"),
            ("win.show-sidebar", "Show Sidebar"),
        ],
    ));
    section.add_group(&group(
        "Navigate",
        &[
            ("win.switch-branch", "Switch Branch…"),
            ("win.go-to-head", "Go to HEAD"),
            ("win.go-to-parent", "Go to Parent"),
            ("win.go-to-child", "Go to Child"),
        ],
    ));
    section.add_group(&group(
        "Repository",
        &[
            ("win.fetch", "Fetch"),
            ("win.pull", "Pull"),
            ("win.push", "Push"),
            ("win.new-branch", "New Branch…"),
            ("win.merge", "Merge…"),
            ("win.rebase", "Rebase…"),
            ("win.stash", "Stash…"),
            ("win.pop-stash", "Pop Stash"),
            ("win.undo", "Undo"),
        ],
    ));
    section.add_group(&group(
        "Commit",
        &[
            ("win.commit", "Commit"),
            ("win.commit-and-push", "Commit and Push"),
        ],
    ));
    section.add_group(&group(
        "Window",
        &[("win.settings", "Settings"), ("win.activity", "Activity")],
    ));
    let window = gtk::ShortcutsWindow::builder()
        .transient_for(&p.window)
        .modal(true)
        .build();
    window.add_section(&section);
    window.present();
}

fn show_about(p: &Rc<Parts>) {
    gtk::AboutDialog::builder()
        .transient_for(&p.window)
        .modal(true)
        .program_name("Amon Hen")
        .version(env!("CARGO_PKG_VERSION"))
        .website("https://github.com/fukusaka/amonhen")
        .license("MIT OR Apache-2.0")
        .authors(["Shoichi Fukusaka"])
        .build()
        .present();
}
