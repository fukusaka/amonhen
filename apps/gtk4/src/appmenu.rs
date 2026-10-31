//! 窓のメニュー。操作の名前・ショートカット・メニューの区切りは、macOS 版と共通の操作の表（`amonhen-commands`）
//! から作り、置き場所は GNOME の決まり（HIG）に合わせる: 見出しの左の「Repository」は表の Repository の区切り、
//! 右端の主メニュー（≡）には File・Edit・View・Navigate・Commit を同じ名前のサブメニューで入れ、Settings・
//! Activity・Keyboard Shortcuts・About を最後に置く。項目は窓のアクション（`win.<id>`）。表にあってこの FE に
//! まだ機能のない操作は [`PENDING`] に並べ、メニューに出さない。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use amonhen_commands::{COMMANDS, Command, Menu};
use amonhen_core::RefsSnapshot;
use amonhen_model::{Oid, RefKind, tr};
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
    /// Range Diff を開く。
    pub range_diff: crate::rangediff::Opener,
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
        self.branch_button.set_tooltip_text(Some(&tr!(
            "{branch} — click to switch branch (Ctrl+Shift+B)",
            branch = head.unwrap_or("detached HEAD")
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
            tr!(
                "{count} commits behind upstream (Ctrl+Alt+L)",
                count = behind
            )
        } else {
            tr!("Pull (Ctrl+Alt+L)")
        }));
        self.push.set_tooltip_text(Some(&if ahead > 0 {
            tr!(
                "{count} commits ahead of upstream (Ctrl+Alt+P)",
                count = ahead
            )
        } else {
            tr!("Push (Ctrl+Alt+P)")
        }));
        let has_stash = refs.refs.entries.iter().any(|e| e.kind == RefKind::Stash);
        for name in ["pop-stash", "show-stashes"] {
            if let Some(action) = self
                .window
                .lookup_action(name)
                .and_downcast::<gio::SimpleAction>()
            {
                action.set_enabled(has_stash);
            }
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
/// 数の付くボタン（Pull・Push。▾ のメニューは付けない: 押すと必ずシートで確かめる。docs/ux.md の「Toolbar」）。
fn count_button(kind: icon::Kind, action: &str) -> (gtk::Button, gtk::Label) {
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
    (button, count)
}

/// ボタンの群れの間の空き（macOS のツールバーの space）。
fn space() -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    b.set_width_request(8);
    b
}

/// メニューの項目を選んだときの処理。
type Handler = Box<dyn Fn(&Rc<Parts>)>;

/// この FE だけの操作のショートカット（表の操作のものは表から）。
const FE_ACCELS: &[(&str, &str)] = &[
    ("win.new-window", "<Control>n"),
    ("win.open", "<Control>o"),
    ("win.clone", "<Control><Alt>c"),
    ("win.close", "<Control>w"),
    ("app.quit", "<Control>q"),
    ("win.shortcuts", "<Control>question"),
];

/// 表にあって、この FE にまだ機能のない操作（メニューに出さない）。
pub const PENDING: &[&str] = &[
    "new-worktree",
    "prune-worktrees",
    "filter-branches",
    "find-in-history",
    "repository-settings",
    "quick-open",
    "repositories",
];

/// 状態を持つので [`handlers`] とは別に登録する操作。
#[cfg(test)]
const STATEFUL: &[&str] = &["show-sidebar"];

/// アクションのショートカット（`win.<id>` と、それに付けるキー）。
fn accels() -> Vec<(String, String)> {
    let table = COMMANDS
        .iter()
        .filter(|c| !PENDING.contains(&c.id))
        .filter_map(|c| Some((format!("win.{}", c.id), c.gtk_accel()?)));
    FE_ACCELS
        .iter()
        .map(|(a, k)| (a.to_string(), k.to_string()))
        .chain(table)
        .collect()
}

/// 表の操作の、この FE での名前（訳す。Finder は GTK では Files）。
pub fn title(c: &Command) -> String {
    match c.id {
        "reveal-in-files" => amonhen_model::tr!("Show in Files"),
        _ if c.localized => amonhen_model::i18n::translate(c.title).into_owned(),
        _ => c.title.to_owned(),
    }
}

/// 窓のアクションの処理（`win.<id>`）。表の操作と、この FE だけの操作。
fn handlers() -> Vec<(&'static str, Handler)> {
    let run = |command: serde_json::Value| -> Handler {
        Box::new(move |p| p.actions.perform(command.clone()))
    };
    vec![
        // File
        (
            "new-window",
            Box::new(|p| {
                if let Some(dir) = workdir(p) {
                    (p.open)(dir);
                }
            }),
        ),
        ("open", Box::new(open_folder)),
        (
            "clone",
            Box::new(|p| crate::repos::clone(p.window.upcast_ref(), p.open.clone())),
        ),
        ("add-remote", Box::new(add_remote)),
        ("close", Box::new(|p| p.window.close())),
        // View
        (
            "show-working-copy",
            Box::new(|p| show_page(p, Page::WorkingCopy)),
        ),
        ("show-history", Box::new(|p| show_page(p, Page::History))),
        ("show-stashes", Box::new(show_stashes)),
        ("show-reflog", Box::new(|p| show_page(p, Page::Reflog))),
        ("refresh", Box::new(|p| (p.reload)())),
        // Navigate
        ("switch-branch", Box::new(switch_branch)),
        ("go-to-head", Box::new(go_to_head)),
        ("go-to-parent", Box::new(|p| go_to_relative(p, true))),
        ("go-to-child", Box::new(|p| go_to_relative(p, false))),
        // Repository
        ("fetch", Box::new(|p| crate::sheets::fetch(&p.actions))),
        ("pull", Box::new(|p| crate::sheets::pull(&p.actions, "git"))),
        (
            "push",
            Box::new(|p| crate::sheets::push(&p.actions, None, false, None)),
        ),
        (
            "force-push",
            Box::new(|p| crate::sheets::push(&p.actions, None, true, None)),
        ),
        (
            "new-branch",
            Box::new(|p| refops::ask_new_branch(&p.actions, &start_point(p))),
        ),
        ("rename-branch", Box::new(rename_branch)),
        ("set-upstream", Box::new(set_upstream)),
        (
            "new-tag",
            Box::new(|p| refops::ask_new_tag(&p.actions, &start_point(p))),
        ),
        ("merge", Box::new(|p| merge_or_rebase(p, false))),
        ("rebase", Box::new(|p| merge_or_rebase(p, true))),
        (
            "compare-with-previous",
            Box::new(|p| p.range_diff.previous(head_branch(p))),
        ),
        ("apply-patch", Box::new(apply_patch)),
        ("continue", run(json!({ "op": "continue" }))),
        ("skip", run(json!({ "op": "skip" }))),
        ("abort", run(json!({ "op": "abort" }))),
        ("stash", Box::new(|p| p.actions.stash())),
        ("pop-stash", Box::new(pop_stash)),
        ("undo", Box::new(|p| crate::sheets::undo(&p.actions))),
        (
            "rebuild-history",
            Box::new(|p| {
                p.actions
                    .perform_work(|repo| repo.rebuild_history().map(|_| ()));
            }),
        ),
        ("open-in-terminal", Box::new(open_in_terminal)),
        (
            "open-on-github",
            Box::new(|p| crate::github::open_on_github(&p.actions, head_branch(p))),
        ),
        (
            "create-pull-request",
            Box::new(|p| match head_branch(p) {
                Some(branch) => crate::github::create_pull_request(&p.actions, branch, false),
                None => p.window.error_bell(),
            }),
        ),
        ("reveal-in-files", Box::new(reveal_in_files)),
        // Commit
        ("commit", Box::new(|p| p.working.commit_from_menu(None))),
        ("save-patch", Box::new(save_patch)),
        (
            "commit-and-push",
            Box::new(|p| {
                let actions = p.actions.clone();
                p.working.commit_from_menu(Some(Box::new(move || {
                    crate::sheets::push_after_commit(&actions)
                })));
            }),
        ),
        ("stage-all", Box::new(|p| p.working.stage_all())),
        (
            "amend-last-commit",
            Box::new(|p| {
                show_page(p, Page::WorkingCopy);
                p.working.amend_last_commit();
            }),
        ),
        // アプリ
        ("settings", Box::new(|p| crate::settings::show(&p.window))),
        (
            "activity",
            Box::new(|p| {
                let repo = p.view.current().map(|(r, _)| r);
                crate::activity::Activity::show(
                    &p.window,
                    repo,
                    p.actions.clone(),
                    crate::engine(),
                );
            }),
        ),
        ("shortcuts", Box::new(show_shortcuts)),
        ("about", Box::new(show_about)),
    ]
}

pub fn install(parts: Parts) -> Buttons {
    let parts = Rc::new(parts);
    let window = &parts.window;
    // File > Open Recent（場所を渡す）。
    let recent = gio::SimpleAction::new("open-recent", Some(glib::VariantTy::STRING));
    let p = parts.clone();
    recent.connect_activate(move |_, value| {
        if let Some(location) = value.and_then(|v| v.get::<String>()) {
            (p.open)(PathBuf::from(location));
        }
    });
    window.add_action(&recent);
    for (name, f) in handlers() {
        let action = gio::SimpleAction::new(name, None);
        let p = parts.clone();
        action.connect_activate(move |_, _| f(&p));
        window.add_action(&action);
    }
    let sidebar = gio::SimpleAction::new_stateful("show-sidebar", None, &true.to_variant());
    let p = parts.clone();
    sidebar.connect_activate(move |action, _| {
        let shown = !action.state().and_then(|s| s.get::<bool>()).unwrap_or(true);
        action.set_state(&shown.to_variant());
        p.sidebar.root.set_visible(shown);
    });
    window.add_action(&sidebar);

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
    for (action, accel) in accels() {
        parts.app.set_accels_for_action(&action, &[accel.as_str()]);
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
            Some((&tr!("Switch Branch…"), "win.switch-branch")),
            Some(("New Branch…", "win.new-branch")),
            Some((&tr!("Rename Branch…"), "win.rename-branch")),
            Some((&tr!("Set Upstream…"), "win.set-upstream")),
        ]))
        .build();
    parts.header.pack_start(&branch_button);
    parts.header.pack_start(&icon_button(
        icon::Kind::Fetch,
        "win.fetch",
        &tr!("Fetch all remotes (Ctrl+Alt+F)"),
    ));
    let (pull, pull_count) = count_button(icon::Kind::Pull, "win.pull");
    parts.header.pack_start(&pull);
    let (push, push_count) = count_button(icon::Kind::Push, "win.push");
    parts.header.pack_start(&push);
    parts.header.pack_start(&space());
    parts.header.pack_start(&icon_button(
        icon::Kind::Merge,
        "win.merge",
        &tr!("Merge a branch, tag or the selected commit into the current branch (Ctrl+Alt+M)"),
    ));
    parts.header.pack_start(&icon_button(
        icon::Kind::Rebase,
        "win.rebase",
        &tr!("Rebase the current branch onto a branch, tag or the selected commit (Ctrl+Alt+R)"),
    ));
    parts.header.pack_start(&space());
    parts.header.pack_start(&icon_button(
        icon::Kind::StashPush,
        "win.stash",
        &tr!("Stash your uncommitted changes (Ctrl+Alt+S)"),
    ));
    parts.header.pack_start(&icon_button(
        icon::Kind::StashPop,
        "win.pop-stash",
        &tr!("Apply the latest stash and remove it (Ctrl+Alt+Shift+S)"),
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
        &tr!("Reload History, branches and Working Copy (Ctrl+R)"),
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

/// 表の `menu` の区切りのうち、この FE に機能のある操作（区切りごと）。
fn table_sections(menu: Menu) -> gio::Menu {
    let out = gio::Menu::new();
    for section in amonhen_commands::menu(menu) {
        let items = gio::Menu::new();
        for c in section.iter().filter(|c| !PENDING.contains(&c.id)) {
            items.append(Some(&title(c)), Some(&format!("win.{}", c.id)));
        }
        if items.n_items() > 0 {
            out.append_section(None, &items);
        }
    }
    out
}

fn primary_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    let submenus = gio::Menu::new();
    let file = menu_of(&[
        Some((&tr!("New Window"), "win.new-window")),
        Some((&tr!("Open…"), "win.open")),
        Some(("Clone…", "win.clone")),
        None,
        Some((&tr!("Close"), "win.close")),
    ]);
    if let Some(first) = file
        .item_link(0, gio::MENU_LINK_SECTION)
        .and_downcast::<gio::Menu>()
    {
        first.append_submenu(Some(&tr!("Open Recent")), &crate::repos::recent_menu());
    }
    let quick_open = table_sections(Menu::File);
    if quick_open.n_items() > 0 {
        file.insert_section(1, None, &quick_open);
    }
    submenus.append_submenu(Some(&tr!("File")), &file);
    for (name, part) in [
        (tr!("Edit"), Menu::Edit),
        (tr!("View"), Menu::View),
        (tr!("Navigate"), Menu::Navigate),
        ("Commit".to_owned(), Menu::Commit),
    ] {
        let items = table_sections(part);
        if items.n_items() > 0 {
            submenus.append_submenu(Some(&name), &items);
        }
    }
    menu.append_section(None, &submenus);
    let app = gio::Menu::new();
    for part in [Menu::App, Menu::Window] {
        for c in amonhen_commands::menu(part).into_iter().flatten() {
            if !PENDING.contains(&c.id) {
                app.append(Some(&title(c)), Some(&format!("win.{}", c.id)));
            }
        }
    }
    app.append(Some(&tr!("Keyboard Shortcuts")), Some("win.shortcuts"));
    app.append(Some(&tr!("About Amon Hen")), Some("win.about"));
    menu.append_section(None, &app);
    menu
}

fn repository_menu() -> gio::Menu {
    table_sections(Menu::Repository)
}

fn show_page(p: &Parts, page: Page) {
    p.pages.set_visible_child_name(match page {
        Page::WorkingCopy => "working",
        Page::History => "history",
        Page::Stashes => "stashes",
        Page::RangeDiff => crate::rangediff::PAGE,
        Page::Reflog => "reflog",
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

/// Commit › Save as Patch…: History で選んだコミットを古い順に（マージを含む・選んでいなければ鳴らす）。
fn save_patch(p: &Rc<Parts>) {
    let oids = (|| {
        if p.pages.visible_child_name().as_deref() != Some("history") {
            return None;
        }
        let (_, history) = p.view.current()?;
        let nodes = history.layout.nodes();
        let mut oids = Vec::new();
        for row in p.view.selection().into_iter().rev() {
            let merge = nodes
                .get(row as usize)
                .is_some_and(|n| n.flags & amonhen_layout::Node::MERGE != 0);
            if merge {
                return None;
            }
            oids.push(history.graph.ids().get(row as usize)?.to_string());
        }
        (!oids.is_empty()).then_some(oids)
    })();
    match oids {
        Some(oids) => crate::patches::save(&p.actions, oids),
        None => p.window.error_bell(),
    }
}

/// Repository › Apply Patch…（merge などの途中なら鳴らす）。
fn apply_patch(p: &Rc<Parts>) {
    if p.actions.in_sequence() {
        return p.window.error_bell();
    }
    crate::patches::choose_and_apply(&p.actions);
}

fn rename_branch(p: &Rc<Parts>) {
    let Some(branch) = head_branch(p) else {
        return p.window.error_bell();
    };
    let actions = p.actions.clone();
    glib::spawn_future_local(async move {
        let message = tr!("New name for {branch}", branch = branch);
        if let Some(to) = actions
            .ask("Rename Branch", &message, &branch, &tr!("Change"))
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
        let title = tr!("Upstream of {branch}", branch = branch);
        let Some(upstream) = actions
            .choose(
                &title,
                &tr!("The remote branch that Pull brings in from and Push sends to."),
                &remotes,
                same,
                &tr!("Set"),
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
        choices.push((tr!("Selected Commit {id}", id = &hex[..8]), hex));
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
    crate::sheets::integrate_choosing(&p.actions, choices, &current, rebase);
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
                &tr!("Branch to switch to"),
                &choices,
                0,
                &tr!("Switch"),
            )
            .await
        else {
            return;
        };
        use crate::sheets::{CheckoutTarget, checkout};
        match choice.split_once(':') {
            Some(("local", name)) => checkout(&actions, CheckoutTarget::Branch(name.to_owned())),
            Some((_, name)) => checkout(&actions, CheckoutTarget::Track(name.to_owned())),
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

/// Stashes の画面を開く（stash があるときだけ）。
fn show_stashes(p: &Rc<Parts>) {
    let has_stash = p
        .refs
        .borrow()
        .as_ref()
        .is_some_and(|r| r.refs.entries.iter().any(|e| e.kind == RefKind::Stash));
    if has_stash {
        show_page(p, Page::Stashes);
    } else {
        p.window.error_bell();
    }
}

/// 最新の stash（`stash@{0}`）を戻して消す。
fn pop_stash(p: &Rc<Parts>) {
    let latest = p.refs.borrow().as_ref().and_then(|r| {
        r.refs
            .entries
            .iter()
            .find(|e| e.kind == RefKind::Stash)
            .map(|e| (e.name.clone(), e.detail.clone()))
    });
    match latest {
        Some((name, message)) => crate::sheets::pop(&p.actions, &name, message.as_deref(), true),
        None => p.window.error_bell(),
    }
}

/// 作業ツリーで端末を開く。デスクトップごとに端末が違うので、あるものを順に試す。
// 端末を開く（窓を出すのが目的）ので、process::command は使わない
#[allow(clippy::disallowed_methods)]
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
                &tr!("Add Remote"),
                &tr!("ssh://[user@]host[:port]/path (a daemon on an SSH host) or daemon:/path (the local shared daemon)"),
                "ssh://",
                &tr!("Open"),
            )
            .await
            && amonhen_remote::location::is_remote(&location)
        {
            open(PathBuf::from(location));
        }
    });
}

fn open_folder(p: &Rc<Parts>) {
    let dialog = gtk::FileDialog::builder()
        .title(tr!("Open"))
        .modal(true)
        .build();
    let (window, open) = (p.window.clone(), p.open.clone());
    glib::spawn_future_local(async move {
        if let Ok(folder) = dialog.select_folder_future(Some(&window)).await
            && let Some(path) = folder.path()
        {
            open(path);
        }
    });
}

/// ショートカットの一覧（メニューごと。表の操作は表から）。
fn show_shortcuts(p: &Rc<Parts>) {
    let accels = accels();
    let group = |title: &str, items: &[(String, String)]| {
        let group = gtk::ShortcutsGroup::builder().title(title).build();
        for (action, label) in items {
            if let Some((_, accel)) = accels.iter().find(|(a, _)| a == action) {
                let shortcut = gtk::ShortcutsShortcut::builder()
                    .accelerator(accel.as_str())
                    .title(label.as_str())
                    .build();
                group.add_shortcut(&shortcut);
            }
        }
        group
    };
    let table = |menu: Menu| -> Vec<(String, String)> {
        amonhen_commands::menu(menu)
            .into_iter()
            .flatten()
            .filter(|c| !PENDING.contains(&c.id))
            .map(|c| (format!("win.{}", c.id), title(c)))
            .collect()
    };
    let section = gtk::ShortcutsSection::builder()
        .section_name("main")
        .build();
    let mut file = vec![
        ("win.new-window".to_owned(), tr!("New Window")),
        ("win.open".to_owned(), tr!("Open…")),
        ("win.close".to_owned(), tr!("Close")),
        ("app.quit".to_owned(), tr!("Quit")),
    ];
    file.extend(table(Menu::File));
    section.add_group(&group(&tr!("File"), &file));
    for (name, menu) in [
        (tr!("Edit"), Menu::Edit),
        (tr!("View"), Menu::View),
        (tr!("Navigate"), Menu::Navigate),
        ("Repository".to_owned(), Menu::Repository),
        ("Commit".to_owned(), Menu::Commit),
    ] {
        let items = table(menu);
        if !items.is_empty() {
            section.add_group(&group(&name, &items));
        }
    }
    let mut window = table(Menu::App);
    window.extend(table(Menu::Window));
    section.add_group(&group(&tr!("Window"), &window));
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
        // 配るときは scripts/package-gtk4.sh が版を埋め込む。
        .version(option_env!("AMONHEN_VERSION").unwrap_or(env!("CARGO_PKG_VERSION")))
        .website("https://github.com/fukusaka/amonhen")
        .license("MIT OR Apache-2.0")
        .authors(["Shoichi Fukusaka"])
        .build()
        .present();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_in_the_table_has_an_action_or_is_pending() {
        let handled: Vec<&str> = handlers()
            .iter()
            .map(|(id, _)| *id)
            .chain(STATEFUL.iter().copied())
            .collect();
        for c in COMMANDS {
            let has = handled.contains(&c.id);
            let pending = PENDING.contains(&c.id);
            assert!(
                has != pending,
                "{}: 処理があるか PENDING のどちらか一方",
                c.id
            );
        }
        for id in PENDING {
            assert!(amonhen_commands::find(id).is_some(), "表にない: {id}");
        }
    }

    /// 画面の文（`tr!` の鍵）に、どれも日本語の訳があるか（鍵はソースの書き方のまま比べる）。
    #[test]
    fn every_text_has_a_japanese_translation() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let strings =
            std::fs::read_to_string(dir.join("../../crates/model/src/i18n/ja.strings")).unwrap();
        let mut missing = Vec::new();
        for entry in std::fs::read_dir(dir.join("src")).unwrap() {
            let path = entry.unwrap().path();
            let source = std::fs::read_to_string(&path).unwrap();
            for marker in ["tr!(", "translate("] {
                for (i, _) in source.match_indices(marker) {
                    // この検査の中の "tr!(" などは飛ばす。
                    if source[..i].ends_with('"') {
                        continue;
                    }
                    let rest = source[i + marker.len()..].trim_start();
                    let Some(rest) = rest.strip_prefix('"') else {
                        continue;
                    };
                    // 閉じる引用符（\" は飛ばす）。
                    let mut end = None;
                    let mut escaped = false;
                    for (j, c) in rest.char_indices() {
                        match c {
                            '\\' if !escaped => escaped = true,
                            '"' if !escaped => {
                                end = Some(j);
                                break;
                            }
                            _ => escaped = false,
                        }
                    }
                    let key = &rest[..end.unwrap()];
                    if !strings.contains(&format!("\"{key}\" = ")) {
                        missing.push(format!("{}: {key}", path.display()));
                    }
                }
            }
        }
        assert!(missing.is_empty(), "訳がない:\n{}", missing.join("\n"));
    }

    #[test]
    fn shortcuts_are_unique() {
        let accels = accels();
        for (i, (a, k)) in accels.iter().enumerate() {
            assert!(
                !accels[i + 1..].iter().any(|(_, other)| other == k),
                "{a} のキー {k} がぶつかる"
            );
        }
    }
}
