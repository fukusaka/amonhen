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

/// メニューの項目が使う、タブの部品。
pub struct Parts {
    pub app: gtk::Application,
    /// タブの中身（今どの窓にあるかは、ここから辿る）。
    pub anchor: gtk::Box,
    /// タブのアクション（`win.*`）。
    pub group: gio::SimpleActionGroup,
    pub header: gtk::HeaderBar,
    pub actions: Rc<Actions>,
    pub pages: gtk::Stack,
    pub sidebar: Rc<Sidebar>,
    pub working: Rc<WorkingCopy>,
    pub view: HistoryView,
    pub refs: Rc<RefCell<Option<Arc<RefsSnapshot>>>>,
    /// History・ref・status を読み直す。
    pub reload: Rc<dyn Fn()>,
    /// リポジトリを開く（この窓のタブか、新しい窓に）。
    pub open: crate::repobrowser::Open,
    /// History でそのコミットを選ぶ。
    pub go_to: Rc<dyn Fn(Oid)>,
    /// Range Diff を開く。
    pub range_diff: crate::rangediff::Opener,
    /// リポジトリの場所（手元のパス、`daemon:…`、`ssh://…`）。
    pub location: String,
    /// リポジトリの worktree。
    pub worktrees: Rc<crate::worktrees::Worktrees>,
    /// History のコミットの詳細（Go to Parent・Go to Child はその親・子へ）。
    pub details: std::rc::Weak<crate::details::Details>,
    /// History の検索のボタン（ツールバーの並びの 1 つ）。
    pub search: gtk::Widget,
}

/// 見出しのメニューとツールバーのボタン（`--open-menu` で開く、ref を読んだら数を出す）。
#[derive(Clone)]
pub struct Buttons {
    pub repository: gtk::MenuButton,
    pub primary: gtk::MenuButton,
    branch: gtk::Label,
    branch_button: gtk::Button,
    pull: gtk::Button,
    pull_count: gtk::Label,
    push: gtk::Button,
    push_count: gtk::Label,
    /// Push のアイコン（上流がなければ Publish の雲に）。
    push_icon: icon::Icon,
    actions: Rc<Actions>,
    /// リポジトリの名前のボタン（見出しの真ん中か、ツールバーの並びの 1 つ）。
    name: gtk::Widget,
    /// 開けていない間に外した、リポジトリの名前のボタンのツールチップ（開けたら戻す）。
    name_tooltip: Rc<RefCell<Option<glib::GString>>>,
    /// History の検索のボタン。
    search: gtk::Widget,
    /// File History・Blame のタブの見出しの Push のアイコン（このタブのものと一緒に替える）。
    mirror_push_icons: Rc<RefCell<Vec<glib::WeakRef<icon::Icon>>>>,
}

impl Buttons {
    /// まだ開けていないリポジトリのタブ（つないでいる途中・つなげなかった）: ブランチを「—」にし、リポジトリの
    /// 名前とともに押せなくする（macOS 版と同じ）。
    pub fn show_unopened(&self) {
        // メニューとショートカットのリポジトリの操作も使えなくする（つないでいるタブはリポジトリの窓でない）。
        self.actions.update_facts(|f| f.unopened = true);
        self.branch.set_text("—");
        self.branch_button.set_sensitive(false);
        self.repository.set_sensitive(false);
        // 名前のボタンも押せなくし、「押すと切り替える」のツールチップも外す（macOS 版の `showUnopened`）。
        if self.name.is_sensitive() {
            self.name_tooltip.replace(self.name.tooltip_text());
        }
        self.name.set_sensitive(false);
        self.name.set_tooltip_text(None);
    }

    /// 今のブランチ、上流との差（↓ 取り込める・↑ 送れる）、stash があるか。
    pub fn show_refs(&self, refs: &RefsSnapshot) {
        self.actions.update_facts(|f| f.unopened = false);
        self.branch_button.set_sensitive(true);
        self.repository.set_sensitive(true);
        if !self.name.is_sensitive() {
            self.name.set_sensitive(true);
            if let Some(tooltip) = self.name_tooltip.take() {
                self.name.set_tooltip_text(Some(&tooltip));
            }
        }
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
        // 上流があって、そのリモートブランチもある（Push で送れる）。なければ Push は Publish（送り先を選ぶ）。
        let published = entry.is_none_or(|e| e.detail.is_some() && !e.gone);
        // メニューの Push・Commit and Push は Publish…・Commit and Publish… になる（macOS 版と同じ）。
        self.actions.set_publishes(!published);
        // ツールバーの Push も Publish のアイコン（icloud.and.arrow.up）になる（macOS 版と同じ）。
        let push_kind = if published {
            icon::Kind::Push
        } else {
            icon::Kind::Publish
        };
        self.push_icon.set(push_kind, false);
        self.mirror_push_icons
            .borrow_mut()
            .retain(|w| w.upgrade().inspect(|i| i.set(push_kind, false)).is_some());
        // 送るものがなければ押せない。理由はツールチップに（macOS 版と同じ）。
        self.push.set_tooltip_text(Some(&if !published {
            tr!("Publish — choose the remote to create this branch on first (Ctrl+Alt+P)")
        } else if ahead > 0 {
            tr!(
                "{count} commits ahead of upstream (Ctrl+Alt+P)",
                count = ahead
            )
        } else if behind > 0 {
            tr!(
                "Nothing to push. Pull the {count} new commits first.",
                count = behind
            )
        } else {
            tr!("Nothing to push")
        }));
        // 使えるかは、ref から決まるものをまとめて（`actions::enabled`）。
        self.actions.update_facts(|f| {
            f.refs = true;
            f.head = refs.refs.head.is_some();
            f.head_branch = head.is_some();
            f.tracking = entry.map(|e| (published, e.ahead));
            f.stash = refs.refs.entries.iter().any(|e| e.kind == RefKind::Stash);
        });
    }
}

impl Buttons {
    /// File History・Blame のタブ `tool` の見出しに、このタブと同じツールバーを作る（macOS 版の
    /// `MainToolbar.mirror()`）。ボタンはタブのアクション（リポジトリのタブに渡すもの）を押し、ブランチ名・数・説明・
    /// 押せるかはこのタブのものに合わせ続ける。リポジトリの名前と検索は、リポジトリのタブに切り替えてから開く。
    /// リポジトリのタブがほかの窓で前にあっても、閉じた後でも、そのタブに付いたまま。
    pub fn mirror(&self, tool: &Rc<crate::tabs::Tab>) {
        let header = &tool.header;
        let bind = |from: &gtk::Widget, to: &gtk::Widget, names: &[&str]| {
            for name in names {
                from.bind_property(name, to, name).sync_create().build();
            }
        };
        let repository = gtk::MenuButton::builder()
            .label("Repository")
            .menu_model(&repository_menu())
            .build();
        if let Some(popover) = repository.popover() {
            popover.set_halign(gtk::Align::Start);
        }
        retitle_on_show(&repository, &self.actions);
        crate::menu::tooltips_on_show(&repository);
        header.pack_start(&repository);

        // リポジトリの名前（下にブランチ名）。押すとリポジトリのタブに切り替えて、一覧を出す。
        let titles = gtk::Box::new(gtk::Orientation::Vertical, 0);
        titles.set_valign(gtk::Align::Center);
        let theirs = self
            .name
            .downcast_ref::<gtk::MenuButton>()
            .and_then(|b| b.child());
        let mut label = theirs.as_ref().and_then(|t| t.first_child());
        for css in ["title", "subtitle"] {
            let mine = gtk::Label::new(None);
            mine.add_css_class(css);
            if let Some(l) = &label {
                bind(l, mine.upcast_ref(), &["label", "visible"]);
                label = l.next_sibling();
            }
            titles.append(&mine);
        }
        let name = gtk::Button::builder()
            .child(&titles)
            .has_frame(false)
            .build();
        bind(
            &self.name,
            name.upcast_ref(),
            &["sensitive", "tooltip-text"],
        );
        let (t, theirs) = (Rc::downgrade(tool), self.name.downgrade());
        name.connect_clicked(move |_| {
            if let Some(owner) = t.upgrade().and_then(|t| t.owner()) {
                owner.bring_front();
                if let Some(button) = theirs.upgrade().and_downcast::<gtk::MenuButton>() {
                    button.popup();
                }
            }
        });

        let branch = gtk::Label::new(None);
        bind(self.branch.upcast_ref(), branch.upcast_ref(), &["label"]);
        let branch_icon = icon::Icon::default();
        branch_icon.set(icon::Kind::Branch, false);
        let branch_inner = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        branch_inner.append(&branch_icon);
        branch_inner.append(&branch);
        let branch_button = gtk::Button::builder()
            .child(&branch_inner)
            .action_name("win.switch-branch")
            .build();
        bind(
            self.branch_button.upcast_ref(),
            branch_button.upcast_ref(),
            &["tooltip-text", "sensitive"],
        );
        let (pull, pull_count, _) = count_button(icon::Kind::Pull, "win.pull");
        let (push, push_count, push_icon) = count_button(icon::Kind::Push, "win.push");
        for (from, to) in [
            (&self.pull_count, &pull_count),
            (&self.push_count, &push_count),
        ] {
            bind(from.upcast_ref(), to.upcast_ref(), &["label", "visible"]);
        }
        for (from, to) in [(&self.pull, &pull), (&self.push, &push)] {
            bind(from.upcast_ref(), to.upcast_ref(), &["tooltip-text"]);
        }
        push_icon.set(self.push_icon.kind(), false);
        self.mirror_push_icons
            .borrow_mut()
            .push(push_icon.downgrade());
        let undo = gtk::Button::new();
        let undo_icon = icon::Icon::default();
        undo_icon.set(icon::Kind::Undo, false);
        undo.set_child(Some(&undo_icon));
        undo.set_action_name(Some("win.undo"));
        bind(
            self.actions.undo.upcast_ref(),
            undo.upcast_ref(),
            &["tooltip-text"],
        );
        // History の検索: リポジトリのタブに切り替えて、History で探す（macOS 版と同じ）。
        let search = gtk::Button::new();
        let search_icon = icon::Icon::default();
        search_icon.set(icon::Kind::Search, false);
        search.set_child(Some(&search_icon));
        bind(
            &self.search,
            search.upcast_ref(),
            &["tooltip-text", "sensitive"],
        );
        let (t, theirs) = (Rc::downgrade(tool), self.search.downgrade());
        search.connect_clicked(move |_| {
            let Some(owner) = t.upgrade().and_then(|t| t.owner()) else {
                return;
            };
            owner.bring_front();
            let _ = WidgetExt::activate_action(&owner.root, "win.show-history", None);
            if let Some(toggle) = theirs.upgrade().and_downcast::<gtk::ToggleButton>() {
                toggle.set_active(true);
            }
        });
        let toolbar_start = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let toolbar_end = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        header.pack_start(&toolbar_start);
        crate::toolbar::install(
            &toolbar_start,
            &toolbar_end,
            crate::toolbar::Given {
                header: header.clone(),
                repository: name.upcast(),
                title: None,
                branch: branch_button,
                pull,
                push,
                undo,
                search: search.upcast(),
            },
        );
        let primary = gtk::MenuButton::builder()
            .menu_model(&primary_menu(&tool.root))
            .tooltip_text(tr!("Main Menu"))
            .primary(true)
            .build();
        set_icon(&primary, "open-menu-symbolic", "≡");
        retitle_on_show(&primary, &self.actions);
        crate::menu::tooltips_on_show(&primary);
        header.pack_end(&primary);
        header.pack_end(&toolbar_end);
    }
}

/// ▾ 付きのボタン（押すと `action`、▾ で `menu`）。アイコンの横に数を出せる。
/// 数の付くボタン（Pull・Push。▾ のメニューは付けない: 押すと必ずシートで確かめる。docs/ux.md の「Toolbar」）。
fn count_button(kind: icon::Kind, action: &str) -> (gtk::Button, gtk::Label, icon::Icon) {
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
    (button, count, icon)
}

/// メニューの項目を選んだときの処理。
type Handler = Box<dyn Fn(&Rc<Parts>)>;

/// この FE だけの操作のショートカット（表の操作のものは表から）。
const FE_ACCELS: &[(&str, &str)] = &[
    ("win.new-tab", "<Control>t"),
    ("win.new-window", "<Control>n"),
    ("win.open", "<Control>o"),
    ("win.clone", "<Control><Alt>c"),
    ("win.close", "<Control>w"),
    ("win.close-window", "<Control><Shift>w"),
    ("win.find", "<Control>f"),
    ("win.find-next", "<Control>g"),
    ("win.find-previous", "<Control><Shift>g"),
    ("win.next-tab", "<Control>Page_Down"),
    ("win.previous-tab", "<Control>Page_Up"),
    ("app.quit", "<Control>q"),
    ("win.shortcuts", "<Control>question"),
];

/// ページを出す View の項目と、そのページ（`pages` の子の名前）。出しているページの項目に ✓ を付ける。
const PAGE_ACTIONS: &[(&str, &str)] = &[
    ("show-working-copy", "working"),
    ("show-history", "history"),
    ("show-stashes", "stashes"),
    ("show-reflog", "reflog"),
    ("repository-settings", "settings"),
];

/// 表にあって、この FE にまだ機能のない操作（メニューに出さない）。
pub const PENDING: &[&str] = &[];

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
        // New Window（Ctrl+N）は新しい窓にリポジトリのページを、Open…（Ctrl+O）はこの窓のリポジトリのページを
        // 出す（一覧から選ぶ。一覧にないフォルダーはページの Add から加える。macOS 版と同じ）。
        (
            "new-window",
            Box::new(|p| {
                crate::tabs::show_home(&p.app, None);
            }),
        ),
        (
            "open",
            Box::new(|p| {
                crate::tabs::show_home(&p.app, crate::tabs::host_of(&p.window()));
            }),
        ),
        (
            "clone",
            Box::new(|p| {
                let open = p.open.clone();
                crate::repos::clone(
                    p.window().upcast_ref(),
                    Rc::new(move |path| {
                        // clone したものはブックマークにも入れる（macOS 版と同じ）。
                        crate::bookmarks::add(&path.to_string_lossy(), None);
                        open(path, crate::tabs::Place::Tab)
                    }),
                )
            }),
        ),
        // git のリモートを加える（サイドバーの Remotes の Add Remote… と同じシート。macOS 版と同じ）。
        (
            "add-remote",
            Box::new(|p| crate::remotesheet::add(&p.actions)),
        ),
        // submodule（サイドバーの Submodules の見出しと同じシート。macOS 版と同じ）。
        (
            "add-submodule",
            Box::new(|p| crate::submodulesheet::add(&p.actions)),
        ),
        (
            "update-submodules",
            Box::new(|p| crate::submodulesheet::update(&p.actions, Vec::new())),
        ),
        // View
        (
            "show-working-copy",
            Box::new(|p| show_page(p, Page::WorkingCopy)),
        ),
        ("show-history", Box::new(|p| show_page(p, Page::History))),
        (
            "find-in-history",
            Box::new(|p| {
                // History に移って、その検索欄へ（macOS 版と同じ）。
                show_page(p, Page::History);
                p.group.activate_action("search-history", None);
            }),
        ),
        ("show-stashes", Box::new(show_stashes)),
        ("show-reflog", Box::new(|p| show_page(p, Page::Reflog))),
        (
            "refresh",
            Box::new(|p| {
                (p.reload)();
                crate::github::reload(&p.actions);
            }),
        ),
        // Navigate
        ("switch-branch", Box::new(switch_branch)),
        (
            "filter-branches",
            Box::new(|p| {
                // 隠したサイドバーは先に出す（macOS 版と同じ）。
                let hidden = p
                    .group
                    .lookup_action("show-sidebar")
                    .and_then(|a| a.state())
                    .and_then(|s| s.get::<bool>())
                    == Some(false);
                if hidden {
                    p.group.activate_action("show-sidebar", None);
                }
                p.sidebar.focus_filter();
            }),
        ),
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
        (
            "new-worktree",
            Box::new(|p| crate::worktrees::new_worktree(&p.actions, &p.worktrees, start_point(p))),
        ),
        ("prune-worktrees", run(json!({ "op": "worktreePrune" }))),
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
                    .perform_work_titled(Some(tr!("Couldn’t Rebuild History")), |repo| {
                        repo.rebuild_history().map(|_| ())
                    });
            }),
        ),
        ("repository-settings", Box::new(repository_settings)),
        ("open-in-terminal", Box::new(open_in_terminal)),
        (
            "open-on-github",
            Box::new(|p| crate::github::open_on_github(&p.actions, head_branch(p))),
        ),
        (
            "create-pull-request",
            Box::new(|p| match head_branch(p) {
                Some(branch) => crate::github::create_pull_request(&p.actions, branch, false),
                None => p.window().error_bell(),
            }),
        ),
        ("reveal-in-files", Box::new(reveal_in_files)),
        // Commit
        ("commit", Box::new(|p| p.working.commit_from_menu(None))),
        ("save-patch", Box::new(save_patch)),
        ("export-archive", Box::new(export_archive)),
        ("save-bundle", Box::new(save_bundle)),
        (
            "fetch-bundle",
            Box::new(|p: &Rc<Parts>| crate::bundle::fetch(&p.actions)),
        ),
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
        ("settings", Box::new(|p| crate::settings::show(&p.window()))),
        (
            "activity",
            Box::new(|p| {
                let repo = p.view.current().map(|(r, _)| r);
                crate::activity::Activity::show(&p.window(), repo, crate::engine());
            }),
        ),
        (
            "repositories",
            Box::new(|p| {
                crate::tabs::show_home(&p.app, crate::tabs::host_of(&p.window()));
            }),
        ),
        (
            "quick-open",
            Box::new(|p| {
                crate::quickopen::show(Some(p.window().upcast_ref()), p.open.clone());
            }),
        ),
        ("shortcuts", Box::new(|p| show_shortcuts(&p.window()))),
        ("about", Box::new(|p| show_about(&p.window()))),
    ]
}

impl Parts {
    /// タブのある窓（項目はタブが窓にある間にしか選べない）。
    fn window(&self) -> gtk::ApplicationWindow {
        self.anchor.root().and_downcast().expect("タブが窓にない")
    }
}

/// アプリのアクション（終える）とショートカット。起動したときに入れる（リポジトリのページしかなくても効くように）。
pub fn install_app(app: &gtk::Application) {
    if app.lookup_action("quit").is_none() {
        let quit = gio::SimpleAction::new("quit", None);
        let weak = app.downgrade();
        quit.connect_activate(move |_, _| {
            if let Some(app) = weak.upgrade() {
                // 少し待ってから書く下書きを、終える前に書く。
                crate::commitmsg::flush_all();
                app.quit();
            }
        });
        app.add_action(&quit);
    }
    for (action, accel) in accels() {
        app.set_accels_for_action(&action, &[accel.as_str()]);
    }
    // タブの操作は、ほかのキーでも（Ctrl+Tab など）。
    crate::tabs::install_accels(app);
}

pub fn install(parts: Parts) -> Buttons {
    let parts = Rc::new(parts);
    let window = &parts.group;
    // File > Open Recent（場所を渡す）。
    let recent = gio::SimpleAction::new("open-recent", Some(glib::VariantTy::STRING));
    let p = parts.clone();
    recent.connect_activate(move |_, value| {
        if let Some(location) = value.and_then(|v| v.get::<String>()) {
            (p.open)(PathBuf::from(location), crate::tabs::Place::Tab);
        }
    });
    window.add_action(&recent);
    for (name, f) in handlers() {
        // ページを出す項目は、出しているページに ✓ を付ける（下で合わせる。macOS 版と同じ）。
        let action = if PAGE_ACTIONS.iter().any(|(n, _)| *n == name) {
            gio::SimpleAction::new_stateful(name, None, &false.to_variant())
        } else {
            gio::SimpleAction::new(name, None)
        };
        let p = parts.clone();
        action.connect_activate(move |_, _| f(&p));
        window.add_action(&action);
    }
    let check_page = {
        let group = parts.group.downgrade();
        move |pages: &gtk::Stack| {
            let Some(group) = group.upgrade() else { return };
            let shown = pages.visible_child_name();
            for (name, page) in PAGE_ACTIONS {
                if let Some(action) = group
                    .lookup_action(name)
                    .and_downcast::<gio::SimpleAction>()
                {
                    action.set_state(&(shown.as_deref() == Some(*page)).to_variant());
                }
            }
        }
    };
    check_page(&parts.pages);
    parts
        .pages
        .connect_visible_child_name_notify(move |pages| check_page(pages));
    // ref・status を読むまでは、それで決まる項目を使えなくしておく（macOS 版と同じ）。端末・ファイルマネージャーは
    // 手元のリポジトリだけ（daemon・SSH 越しでは選べない）。
    let local = !amonhen_remote::location::is_remote(&parts.location);
    parts.actions.update_facts(|f| f.local = local);
    let sidebar = gio::SimpleAction::new_stateful("show-sidebar", None, &true.to_variant());
    let p = parts.clone();
    sidebar.connect_activate(move |action, _| {
        let shown = !action.state().and_then(|s| s.get::<bool>()).unwrap_or(true);
        action.set_state(&shown.to_variant());
        p.sidebar.root.set_visible(shown);
    });
    window.add_action(&sidebar);

    install_app(&parts.app);

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
    // 右に Activity・Refresh・Undo・検索）。
    let branch = gtk::Label::new(Some("…"));
    let branch_icon = icon::Icon::default();
    branch_icon.set(icon::Kind::Branch, false);
    let branch_inner = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    branch_inner.append(&branch_icon);
    branch_inner.append(&branch);
    // 押すとブランチの切り替え（macOS 版のツールバーの Branch と同じ）。
    let branch_button = gtk::Button::builder()
        .child(&branch_inner)
        .action_name("win.switch-branch")
        .tooltip_text(tr!("Switch Branch (Ctrl+Shift+B)"))
        .build();
    let (pull, pull_count, _) = count_button(icon::Kind::Pull, "win.pull");
    let (push, push_count, push_icon) = count_button(icon::Kind::Push, "win.push");
    // ボタンの並びは View > Customize Toolbar… で変えられる（`toolbar.rs`）。左はここ、右は主メニューの左。
    let toolbar_start = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let toolbar_end = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    parts.header.pack_start(&toolbar_start);
    // リポジトリの名前（main.rs が見出しの真ん中に置いたもの）。
    let name = parts
        .header
        .title_widget()
        .expect("リポジトリの名前のボタン");
    crate::toolbar::install(
        &toolbar_start,
        &toolbar_end,
        crate::toolbar::Given {
            header: parts.header.clone(),
            repository: name.clone(),
            title: None,
            branch: branch_button.clone(),
            pull: pull.clone(),
            push: push.clone(),
            undo: parts.actions.undo.clone(),
            search: parts.search.clone(),
        },
    );
    let customize = gio::SimpleAction::new("customize-toolbar", None);
    let p = parts.clone();
    customize.connect_activate(move |_, _| crate::toolbar::customize(p.window().upcast_ref()));
    window.add_action(&customize);

    let primary = gtk::MenuButton::builder()
        .menu_model(&primary_menu(&parts.anchor))
        .tooltip_text(tr!("Main Menu"))
        .primary(true)
        .build();
    set_icon(&primary, "open-menu-symbolic", "≡");
    // 右端から: 主メニュー、ツールバーの右の並び（既定は Activity・Refresh・Undo・検索）。
    parts.header.pack_end(&primary);
    // 開くときに、状態で変わる項目の名前（Publish…・Undo <操作>…）を合わせる。
    for button in [&repository, &primary] {
        retitle_on_show(button, &parts.actions);
        crate::menu::tooltips_on_show(button);
    }
    parts.header.pack_end(&toolbar_end);
    Buttons {
        repository,
        primary,
        branch,
        branch_button,
        pull,
        pull_count,
        push,
        push_count,
        push_icon,
        actions: parts.actions.clone(),
        name,
        name_tooltip: Rc::default(),
        search: parts.search.clone(),
        mirror_push_icons: Rc::default(),
    }
}

/// リポジトリのページ（リポジトリを開いていないタブ）の見出し: リポジトリのタブと同じ Repository のメニュー・
/// ツールバー・主メニュー（macOS 版の `RepositoryHomeWindow` は同じ `MainToolbar` を `showNoRepository` で出す。
/// タブを切り替えてもボタンの位置が動かない）。リポジトリへの操作はこのタブにアクションがないので押せない。
/// リポジトリの名前は「Choose a Repository」、ブランチは「—」で、どちらも押せない。`title` は見出しの真ん中に出す。
/// 主メニューのボタンを返す。
pub fn install_home(
    header: &gtk::HeaderBar,
    group: &gio::SimpleActionGroup,
    anchor: &gtk::Box,
    title: &gtk::Widget,
) -> gtk::MenuButton {
    let repository = gtk::MenuButton::builder()
        .label("Repository")
        .menu_model(&repository_menu())
        .build();
    if let Some(popover) = repository.popover() {
        popover.set_halign(gtk::Align::Start);
    }
    crate::menu::tooltips_on_show(&repository);
    header.pack_start(&repository);

    let name = gtk::Label::new(Some(&tr!("Choose a Repository")));
    name.add_css_class("title");
    let name_button = gtk::Button::builder()
        .child(&name)
        .has_frame(false)
        .sensitive(false)
        .build();
    let branch = gtk::Label::new(Some("—"));
    let branch_icon = icon::Icon::default();
    branch_icon.set(icon::Kind::Branch, false);
    let branch_inner = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    branch_inner.append(&branch_icon);
    branch_inner.append(&branch);
    let branch_button = gtk::Button::builder()
        .child(&branch_inner)
        .sensitive(false)
        .build();
    let (pull, _, _) = count_button(icon::Kind::Pull, "win.pull");
    let (push, _, _) = count_button(icon::Kind::Push, "win.push");
    let undo = gtk::Button::new();
    let undo_icon = icon::Icon::default();
    undo_icon.set(icon::Kind::Undo, false);
    undo.set_child(Some(&undo_icon));
    undo.set_action_name(Some("win.undo"));
    undo.set_tooltip_text(Some(&tr!("Nothing to Undo")));
    // History の検索（リポジトリがないので押せない。macOS 版と同じ）。
    let search = gtk::ToggleButton::new();
    let search_icon = icon::Icon::default();
    search_icon.set(icon::Kind::Search, false);
    search.set_child(Some(&search_icon));
    search.set_tooltip_text(Some(&tr!("Search History (Ctrl+F)")));
    search.set_sensitive(false);
    let toolbar_start = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let toolbar_end = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    header.pack_start(&toolbar_start);
    crate::toolbar::install(
        &toolbar_start,
        &toolbar_end,
        crate::toolbar::Given {
            header: header.clone(),
            repository: name_button.upcast(),
            title: Some(title.clone()),
            branch: branch_button,
            pull,
            push,
            undo,
            search: search.upcast(),
        },
    );
    // View > Customize Toolbar…（macOS 版では、この窓のツールバーからも変えられる）。
    let customize = gio::SimpleAction::new("customize-toolbar", None);
    let a = anchor.downgrade();
    customize.connect_activate(move |_, _| {
        if let Some(window) = a
            .upgrade()
            .and_then(|a| a.root())
            .and_downcast::<gtk::Window>()
        {
            crate::toolbar::customize(&window);
        }
    });
    group.add_action(&customize);

    let primary = gtk::MenuButton::builder()
        .menu_model(&primary_menu(anchor))
        .tooltip_text(tr!("Main Menu"))
        .primary(true)
        .build();
    set_icon(&primary, "open-menu-symbolic", "≡");
    crate::menu::tooltips_on_show(&primary);
    header.pack_end(&primary);
    header.pack_end(&toolbar_end);
    primary
}

/// `button` のメニューを開くたびに、項目の名前を今の状態に合わせる（[`Actions::retitle`]）。
/// メニューを開くたびに作り直すボタン（`set_create_popup_func` で `set_menu_model` する Commit ▾）は
/// そのたびに新しい popover になるので、popover が替わったら付け直し、名前は開いたときのメニューで合わせる。
pub fn retitle_on_show(button: &gtk::MenuButton, actions: &Rc<Actions>) {
    let actions = Rc::downgrade(actions);
    retitle_on_show_with(button, move |_, model| {
        if let Some(actions) = actions.upgrade() {
            actions.retitle(model);
        }
    });
}

/// `retitle_on_show` の、名前の合わせ方を渡す形（`Actions` のないタブ。Repositories のページ）。
pub fn retitle_on_show_with(
    button: &gtk::MenuButton,
    retitle: impl Fn(&gtk::MenuButton, &gio::MenuModel) + 'static,
) {
    type Retitle = Rc<dyn Fn(&gtk::MenuButton, &gio::MenuModel)>;
    fn hook(button: &gtk::MenuButton, retitle: &Retitle) {
        let Some(popover) = button.popover() else {
            return;
        };
        let (button, retitle) = (button.downgrade(), retitle.clone());
        popover.connect_show(move |_| {
            let Some(button) = button.upgrade() else {
                return;
            };
            if let Some(model) = button.menu_model() {
                retitle(&button, &model);
            }
        });
    }
    let retitle: Retitle = Rc::new(retitle);
    hook(button, &retitle);
    button.connect_notify_local(Some("popover"), move |button, _| hook(button, &retitle));
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
            // 表の説明は項目のツールチップに（macOS 版と同じ）。
            let tip = c.tooltip.map(amonhen_model::i18n::translate);
            crate::menu::append_with_tip(
                &items,
                &title(c),
                &format!("win.{}", c.id),
                tip.as_deref(),
            );
        }
        if items.n_items() > 0 {
            out.append_section(None, &items);
        }
    }
    out
}

pub fn primary_menu(anchor: &gtk::Box) -> gio::Menu {
    let menu = gio::Menu::new();
    let submenus = gio::Menu::new();
    // File: New Tab・New Window・Open…・Quick Open…・Clone…・Open Recent ▸ | Close・Close Window（macOS 版と同じ
    // 並び。Close Window は GNOME の分）。
    let file = gio::Menu::new();
    let first = menu_of(&[
        Some((&tr!("New Tab"), "win.new-tab")),
        Some((&tr!("New Window"), "win.new-window")),
        Some((&tr!("Open…"), "win.open")),
    ]);
    if let Some(first) = first
        .item_link(0, gio::MENU_LINK_SECTION)
        .and_downcast::<gio::Menu>()
    {
        if let Some(c) = amonhen_commands::find("quick-open") {
            first.append(Some(&title(c)), Some("win.quick-open"));
        }
        first.append(Some("Clone…"), Some("win.clone"));
        first.append_submenu(Some(&tr!("Open Recent")), &crate::repos::recent_menu());
        file.append_section(None, &first);
    }
    file.append_section(
        None,
        &menu_of(&[
            Some((&tr!("Close"), "win.close")),
            Some((&tr!("Close Window"), "win.close-window")),
        ]),
    );
    submenus.append_submenu(Some(&tr!("File")), &file);
    // Edit: Find…・Find Next・Find Previous（History の検索か、フォーカスのある差分の中）と、表の Find in
    // History…（macOS 版と同じ。Undo・Cut などは GTK の部品のもの）。
    let edit = menu_of(&[
        Some((&tr!("Find…"), "win.find")),
        Some((&tr!("Find Next"), "win.find-next")),
        Some((&tr!("Find Previous"), "win.find-previous")),
    ]);
    if let Some(first) = edit
        .item_link(0, gio::MENU_LINK_SECTION)
        .and_downcast::<gio::Menu>()
    {
        for c in amonhen_commands::menu(Menu::Edit).into_iter().flatten() {
            first.append(Some(&title(c)), Some(&format!("win.{}", c.id)));
        }
    }
    submenus.append_submenu(Some(&tr!("Edit")), &edit);
    for (name, part) in [
        (tr!("View"), Menu::View),
        (tr!("Navigate"), Menu::Navigate),
        ("Commit".to_owned(), Menu::Commit),
    ] {
        let items = table_sections(part);
        // View は History の表示のメニューも兼ねる（docs/ux.md の 11）。
        if part == Menu::View {
            // タブが 1 つの窓でもタブの帯を出す（タブをつかんで、ほかの窓の帯に落とせる）。名前は開くときに
            // Hide にする（macOS 版と同じ）。
            items.append_section(
                None,
                &menu_of(&[Some((&tr!("Show Tab Bar"), "win.toggle-tab-bar"))]),
            );
            crate::historydisplay::append_sections(&items);
            // ツールバーの並び（macOS 版の View > Customize Toolbar…）。
            items.append_section(
                None,
                &menu_of(&[Some((&tr!("Customize Toolbar…"), "win.customize-toolbar"))]),
            );
        }
        if items.n_items() > 0 {
            submenus.append_submenu(Some(&name), &items);
        }
    }
    // Window: タブ（複数のリポジトリを 1 つの窓で扱う。macOS 版の Window のメニューと同じ項目）。
    submenus.append_submenu(
        Some(&tr!("Window")),
        &menu_of(&[
            Some((&tr!("Show Next Tab"), "win.next-tab")),
            Some((&tr!("Show Previous Tab"), "win.previous-tab")),
            None,
            Some((&tr!("Move Tab to New Window"), "win.move-tab-to-new-window")),
            Some((&tr!("Merge All Windows"), "win.merge-all-windows")),
        ]),
    );
    // Move Tab to Window ▸（ほかの窓ごとに 1 つ。macOS 版と同じく Merge All Windows の後）。
    if let Some(window) = submenus
        .item_link(submenus.n_items() - 1, gio::MENU_LINK_SUBMENU)
        .and_downcast::<gio::Menu>()
        && let Some(section) = window
            .item_link(window.n_items() - 1, gio::MENU_LINK_SECTION)
            .and_downcast::<gio::Menu>()
    {
        section.append_submenu(
            Some(&tr!("Move Tab to Window")),
            &crate::tabs::move_menu(anchor),
        );
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

/// `--print-menus`: 見出しのメニュー（Repository と主メニュー）の中身を、1 項目 1 行で出す（macOS 版の
/// `--print-menus` と同じく、操作の表を作り直す前と後で比べる用）。`場所 | 名前 | ショートカット | アクション |
/// 説明 | 出どころ`（`table` は操作の表、`fe` はこの FE だけの操作）。区切りは `場所 | ---`。
pub fn print_menus(app: &gtk::Application) {
    let anchor = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    print_menu(app, repository_menu().upcast_ref(), &tr!("Repository"));
    print_menu(app, primary_menu(&anchor).upcast_ref(), "Main Menu");
}

fn print_menu(app: &gtk::Application, model: &gio::MenuModel, path: &str) {
    let string = |i: i32, name: &str| {
        model
            .item_attribute_value(i, name, Some(glib::VariantTy::STRING))
            .and_then(|v| v.get::<String>())
            .unwrap_or_default()
    };
    for i in 0..model.n_items() {
        if let Some(section) = model.item_link(i, gio::MENU_LINK_SECTION) {
            if i > 0 {
                println!("{path} | ---");
            }
            print_menu(app, &section, path);
            continue;
        }
        let label = string(i, gio::MENU_ATTRIBUTE_LABEL);
        if let Some(submenu) = model.item_link(i, gio::MENU_LINK_SUBMENU) {
            print_menu(app, &submenu, &format!("{path} > {label}"));
            continue;
        }
        let action = string(i, gio::MENU_ATTRIBUTE_ACTION);
        let detailed = match model.item_attribute_value(i, gio::MENU_ATTRIBUTE_TARGET, None) {
            Some(target) => format!("{action}({})", target.print(true)),
            None => action.clone(),
        };
        let accels = app.accels_for_action(&detailed).join(" ");
        let command = action.strip_prefix("win.").and_then(amonhen_commands::find);
        let tooltip = command
            .and_then(|c| c.tooltip)
            .map(|t| amonhen_model::i18n::translate(t).into_owned())
            .unwrap_or_default();
        let source = if command.is_some() { "table" } else { "fe" };
        println!("{path} | {label} | {accels} | {detailed} | {tooltip} | {source}");
    }
}

fn show_page(p: &Parts, page: Page) {
    p.pages.set_visible_child_name(match page {
        Page::WorkingCopy => "working",
        Page::History => "history",
        Page::Stashes => "stashes",
        Page::RangeDiff => crate::rangediff::PAGE,
        Page::Reflog => "reflog",
        Page::Settings => "settings",
    });
    p.sidebar.select_page(page);
}

/// 書き込む先の worktree の作業ツリー。daemon・SSH 越しなら向こうのパスなので、手元のパスとしては使わない。
fn workdir(p: &Parts) -> Option<PathBuf> {
    p.actions.repo()?.paths().workdir
}

/// 手元の作業ツリー（daemon・SSH 越しなら None。端末・ファイルマネージャーは手元のものしか開けない）。
fn local_workdir(p: &Parts) -> Option<PathBuf> {
    if amonhen_remote::location::is_remote(&p.location) {
        return None;
    }
    workdir(p)
}

/// `location` と同じ接続先の、`dir`（リポジトリが返したパス）の場所。手元の場所なら `dir` のまま、daemon・SSH
/// 越しなら場所のパスの部分を `dir` に替える（macOS 版の `replacingPath` と同じ）。
pub fn same_place(location: &str, dir: &std::path::Path) -> PathBuf {
    if !amonhen_remote::location::is_remote(location) {
        return dir.to_path_buf();
    }
    let old = crate::bookmarks::path_part(location);
    match location.strip_suffix(old.as_str()) {
        Some(prefix) if !old.is_empty() => PathBuf::from(format!("{prefix}{}", dir.display())),
        _ => PathBuf::from(location),
    }
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
    match patch_commits(&p.pages, &p.view) {
        Some(oids) => crate::patches::save(&p.actions, oids),
        None => p.window().error_bell(),
    }
}

/// Commit › Export Archive…: History で 1 つ選んだコミット、なければ HEAD（macOS 版と同じ）。
fn export_archive(p: &Rc<Parts>) {
    let one = p.view.selection().len() == 1;
    let history = p.pages.visible_child_name().as_deref() == Some("history");
    match selected_commit(p).filter(|_| one && history) {
        Some(id) => {
            let label = archive_label(&p.view.ref_names(&id));
            let label = if label.is_empty() {
                id.to_string()[..8].to_owned()
            } else {
                label
            };
            crate::archive::export(&p.actions, id.to_string(), label);
        }
        None => crate::archive::export(
            &p.actions,
            "HEAD".into(),
            head_branch(p).unwrap_or_else(|| "HEAD".into()),
        ),
    }
}

/// Repository › Save Repository as Bundle…: HEAD とすべてのブランチ・タグ。
fn save_bundle(p: &Rc<Parts>) {
    crate::bundle::save(&p.actions, Vec::new(), Vec::new(), "all".into());
}

/// アーカイブの既定の名前: コミットを指すタグ（なければブランチ）の名前。どれもなければ空。
pub fn archive_label(names: &[(String, RefKind)]) -> String {
    names
        .iter()
        .find(|(_, k)| *k == RefKind::Tag)
        .or_else(|| names.iter().find(|(_, k)| *k == RefKind::Local))
        .map(|(n, _)| n.clone())
        .unwrap_or_default()
}

/// Save as Patch… にする、History で選んだコミット（古い順）。History を出していない、選んでいない、merge を
/// 含むなら None（macOS 版の `selectedCommitsForPatch` と同じ）。
pub fn patch_commits(pages: &gtk::Stack, view: &HistoryView) -> Option<Vec<String>> {
    if pages.visible_child_name().as_deref() != Some("history") {
        return None;
    }
    let (_, history) = view.current()?;
    let nodes = history.layout.nodes();
    let mut oids = Vec::new();
    for row in view.selection().into_iter().rev() {
        let merge = nodes
            .get(row as usize)
            .is_some_and(|n| n.flags & amonhen_layout::Node::MERGE != 0);
        if merge {
            return None;
        }
        oids.push(history.graph.ids().get(row as usize)?.to_string());
    }
    (!oids.is_empty()).then_some(oids)
}

/// Repository › Apply Patch…（merge などの途中なら鳴らす）。
fn apply_patch(p: &Rc<Parts>) {
    if p.actions.in_sequence() {
        return p.window().error_bell();
    }
    crate::patches::choose_and_apply(&p.actions);
}

fn rename_branch(p: &Rc<Parts>) {
    let Some(branch) = head_branch(p) else {
        return p.window().error_bell();
    };
    refops::ask_rename_branch(&p.actions, &branch);
}

/// 今のブランチの上流（pull・push の相手）を、リモートのブランチから選ぶ。
fn set_upstream(p: &Rc<Parts>) {
    let Some(branch) = head_branch(p) else {
        return p.window().error_bell();
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
        return p.window().error_bell();
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
        return p.window().error_bell();
    }
    let current = head_branch(p).unwrap_or_else(|| "HEAD".into());
    crate::sheets::integrate_choosing(&p.actions, choices, &current, rebase);
}

/// ブランチの切り替え（あいまい検索・最近切り替えたもの・追跡ブランチ・新しいブランチ。macOS 版と同じ）。
fn switch_branch(p: &Rc<Parts>) {
    let Some(refs) = p.refs.borrow().as_ref().map(|r| r.refs.clone()) else {
        return p.window().error_bell();
    };
    let location = crate::repos::normalize(std::path::Path::new(&p.location));
    let recent = crate::branchswitcher::recent(&location);
    let query = SWITCHER_QUERY.with(|q| q.take()).unwrap_or_default();
    crate::branchswitcher::show(&p.window(), refs, recent, p.actions.clone(), &query);
}

thread_local! {
    /// 確認用: ブランチの切り替えに最初から入れておく問い合わせ（`--switch-branch`）。
    static SWITCHER_QUERY: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// 確認用: 次に開くブランチの切り替えに、`query` を入れておく。
pub fn set_switcher_query(query: String) {
    SWITCHER_QUERY.with(|q| q.replace(Some(query)));
}

fn go_to_head(p: &Rc<Parts>) {
    let head = p.refs.borrow().as_ref().and_then(|r| r.refs.head);
    match head {
        Some(id) => {
            show_page(p, Page::History);
            (p.go_to)(id);
        }
        None => p.window().error_bell(),
    }
}

/// 選んでいるコミットの最初の親（`parent`）か、子のうち最も近いもの。
fn go_to_relative(p: &Rc<Parts>, parent: bool) {
    // 詳細に出しているコミットの最初の親・最初の子へ。今の History になければ、すべての履歴にして選ぶ（macOS 版と
    // 同じ）。
    match p.details.upgrade().and_then(|d| d.relative(parent)) {
        Some(id) => (p.go_to)(id),
        None => p.window().error_bell(),
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
        p.window().error_bell();
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
        None => p.window().error_bell(),
    }
}

/// 作業ツリーで端末を開く。デスクトップごとに端末が違うので、あるものを順に試す。
// 端末を開く（窓を出すのが目的）ので、process::command は使わない
#[allow(clippy::disallowed_methods)]
fn open_in_terminal(p: &Rc<Parts>) {
    let Some(dir) = local_workdir(p) else {
        return p.window().error_bell();
    };
    if !terminal_at(&dir) {
        p.window().error_bell();
    }
}

/// `dir` で端末を開く（Repositories の一覧からも使う）。開けなければ false。
pub fn terminal_at(dir: &std::path::Path) -> bool {
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
        let started = amonhen_model::process::command(program)
            .args(&args)
            .current_dir(dir)
            .spawn();
        if let Ok(mut child) = started {
            // 終わるのを待って片付ける（待たないと、アプリを終えるまでゾンビのプロセスが残る）。
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            return true;
        }
    }
    false
}

/// 作業ツリーを、ファイルマネージャーで（親のフォルダの中で選んだ状態で）見せる。
fn reveal_in_files(p: &Rc<Parts>) {
    let Some(dir) = local_workdir(p) else {
        return p.window().error_bell();
    };
    reveal(p.window().upcast_ref(), &dir);
}

/// `dir` を、ファイルマネージャーで（親のフォルダの中で選んだ状態で）見せる（Repositories の一覧からも使う）。
pub fn reveal(window: &gtk::Window, dir: &std::path::Path) {
    let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(dir)));
    let window = window.clone();
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

/// リポジトリの設定の窓（Repository > Repository Settings、Ctrl+;）。
/// Repository > Repository Settings: サイドバーの Settings のページ（編集する worktree のリポジトリ）。
fn repository_settings(p: &Rc<Parts>) {
    let Some(repo) = p.actions.repo() else {
        return;
    };
    let Some(slot) = p.pages.child_by_name("settings").and_downcast::<gtk::Box>() else {
        return;
    };
    // 取得とフォルダのページの操作（取り足す・取り直す・取り出すフォルダを変える）は、Fetch と同じく窓の操作で。
    let actions = Rc::downgrade(&p.actions);
    let operate: crate::reposettings::Operate =
        Rc::new(move |command, done| match actions.upgrade() {
            Some(actions) => actions.perform_with(vec![command], move |_| done()),
            None => done(),
        });
    crate::reposettings::show(&slot, repo, &p.location, p.reload.clone(), operate, None);
    show_page(p, Page::Settings);
}

/// ショートカットの一覧（メニューごと。表の操作は表から。リポジトリのページからも使う）。
pub fn show_shortcuts(parent: &gtk::ApplicationWindow) {
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
        ("win.new-tab".to_owned(), tr!("New Tab")),
        ("win.new-window".to_owned(), tr!("New Window")),
        ("win.open".to_owned(), tr!("Open…")),
        ("win.close".to_owned(), tr!("Close")),
        ("win.close-window".to_owned(), tr!("Close Window")),
        ("app.quit".to_owned(), tr!("Quit")),
    ];
    file.extend(table(Menu::File));
    section.add_group(&group(&tr!("File"), &file));
    let mut edit = vec![
        ("win.find".to_owned(), tr!("Find…")),
        ("win.find-next".to_owned(), tr!("Find Next")),
        ("win.find-previous".to_owned(), tr!("Find Previous")),
    ];
    edit.extend(table(Menu::Edit));
    section.add_group(&group(&tr!("Edit"), &edit));
    for (name, menu) in [
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
    let mut window = vec![
        ("win.next-tab".to_owned(), tr!("Show Next Tab")),
        ("win.previous-tab".to_owned(), tr!("Show Previous Tab")),
    ];
    window.extend(table(Menu::App));
    window.extend(table(Menu::Window));
    section.add_group(&group(&tr!("Window"), &window));
    let window = gtk::ShortcutsWindow::builder()
        .transient_for(parent)
        .modal(true)
        .build();
    window.add_section(&section);
    window.present();
}

/// About（リポジトリのページからも使う）。
pub fn show_about(parent: &gtk::ApplicationWindow) {
    gtk::AboutDialog::builder()
        .transient_for(parent)
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
    fn every_page_item_has_a_handler() {
        let names: Vec<&str> = handlers().into_iter().map(|(n, _)| n).collect();
        for (name, _) in PAGE_ACTIONS {
            assert!(names.contains(name), "{name}");
        }
    }

    #[test]
    fn keeps_the_connection_of_remote_locations() {
        let place = |l: &str, d: &str| same_place(l, std::path::Path::new(d));
        assert_eq!(place("/a/r", "/a/r/sub"), PathBuf::from("/a/r/sub"));
        assert_eq!(
            place("ssh://me@h:22/a/r", "/a/r/sub"),
            PathBuf::from("ssh://me@h:22/a/r/sub")
        );
        assert_eq!(
            place("daemon:/a/r", "/a/r/sub"),
            PathBuf::from("daemon:/a/r/sub")
        );
        assert_eq!(
            place("daemon:ssh://h/a/r", "/a/w"),
            PathBuf::from("daemon:ssh://h/a/w")
        );
        // パスのない場所は、そのまま。
        assert_eq!(place("ssh://h", "/x"), PathBuf::from("ssh://h"));
    }

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
