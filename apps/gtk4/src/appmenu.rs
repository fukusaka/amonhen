//! 窓のメニュー。GNOME の決まり（HIG）に合わせ、見出しの右の主メニュー（≡）にアプリ全体のもの
//! （開く・表示・ショートカット・このアプリについて）を、左の「リポジトリ」にリポジトリの操作を置く
//! （macOS 版の Repository メニューと同じ項目）。項目は窓のアクション（`win.*`）で、ショートカットも付ける。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use amonhen_core::RefsSnapshot;
use amonhen_model::RefKind;
use gtk::prelude::*;
use gtk::{gio, glib};
use serde_json::json;

use crate::actions::Actions;
use crate::history::HistoryView;
use crate::sidebar::{Page, Sidebar};

/// メニューの項目が使う、窓の部品。
pub struct Parts {
    pub app: gtk::Application,
    pub window: gtk::ApplicationWindow,
    pub header: gtk::HeaderBar,
    pub actions: Rc<Actions>,
    pub pages: gtk::Stack,
    pub sidebar: Rc<Sidebar>,
    pub view: HistoryView,
    pub refs: Rc<RefCell<Option<Arc<RefsSnapshot>>>>,
    /// History・ref・status を読み直す。
    pub reload: Rc<dyn Fn()>,
    /// 別のリポジトリを新しい窓で開く。
    pub open: Rc<dyn Fn(PathBuf)>,
}

/// 見出しのメニューのボタン（`--open-menu` で開く）。
pub struct Buttons {
    pub repository: gtk::MenuButton,
    pub primary: gtk::MenuButton,
}

/// メニューの項目を選んだときの処理。
type Handler = Box<dyn Fn(&Rc<Parts>)>;

/// (アクションの名前, ショートカット)。ショートカットの一覧にも使う。
const ACCELS: &[(&str, &str)] = &[
    ("win.open", "<Control>o"),
    ("win.close", "<Control>w"),
    ("app.quit", "<Control>q"),
    ("win.show-working-copy", "<Control>1"),
    ("win.show-history", "<Control>2"),
    ("win.refresh", "<Control>r"),
    ("win.fetch", "<Control><Shift>f"),
    ("win.pull", "<Control><Shift>l"),
    ("win.push", "<Control><Shift>p"),
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

    add("open", Box::new(open_folder));
    add("close", Box::new(|p| p.window.close()));
    add(
        "show-working-copy",
        Box::new(|p| show_page(p, Page::WorkingCopy)),
    );
    add("show-history", Box::new(|p| show_page(p, Page::History)));
    add("refresh", Box::new(|p| (p.reload)()));
    add("fetch", run(json!({ "op": "fetch" })));
    add("pull", run(json!({ "op": "pull", "mode": "merge" })));
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
    add("new-branch", Box::new(new_branch));
    add("new-tag", Box::new(new_tag));
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
        .label("リポジトリ")
        .menu_model(&repository_menu())
        .tooltip_text("リポジトリの操作")
        .build();
    // 窓の左端にあるので、ボタンの左端からそろえて開く（中央だと窓の外にはみ出す）。
    if let Some(popover) = repository.popover() {
        popover.set_halign(gtk::Align::Start);
    }
    parts.header.pack_start(&repository);
    let primary = gtk::MenuButton::builder()
        .menu_model(&primary_menu())
        .tooltip_text("メインメニュー")
        .primary(true)
        .build();
    set_icon(&primary, "open-menu-symbolic", "≡");
    parts.header.pack_end(&primary);
    Buttons {
        repository,
        primary,
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

fn section(items: &[(&str, &str)]) -> gio::Menu {
    let menu = gio::Menu::new();
    for (label, action) in items {
        menu.append(Some(label), Some(action));
    }
    menu
}

fn primary_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    menu.append_section(None, &section(&[("リポジトリを開く…", "win.open")]));
    menu.append_section(
        None,
        &section(&[
            ("Working Copy", "win.show-working-copy"),
            ("History", "win.show-history"),
            ("読み直す", "win.refresh"),
        ]),
    );
    menu.append_section(
        None,
        &section(&[
            ("キーボードショートカット", "win.shortcuts"),
            ("Amon Hen について", "win.about"),
        ]),
    );
    menu
}

fn repository_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    let pull_with = section(&[
        ("merge", "win.pull"),
        ("rebase", "win.pull-rebase"),
        ("fast-forward だけ", "win.pull-ff-only"),
    ]);
    let sync = section(&[("fetch", "win.fetch"), ("pull", "win.pull")]);
    sync.append_submenu(Some("pull の取り込み方"), &pull_with);
    sync.append(Some("push"), Some("win.push"));
    sync.append(
        Some("強制 push（--force-with-lease）…"),
        Some("win.force-push"),
    );
    menu.append_section(None, &sync);
    menu.append_section(
        None,
        &section(&[
            ("新しいブランチ…", "win.new-branch"),
            ("新しいタグ…", "win.new-tag"),
        ]),
    );
    menu.append_section(
        None,
        &section(&[
            ("続行", "win.continue"),
            ("スキップ", "win.skip"),
            ("中止", "win.abort"),
        ]),
    );
    menu.append_section(
        None,
        &section(&[
            ("変更を stash…", "win.stash"),
            ("最新の stash を当てて消す", "win.pop-stash"),
        ]),
    );
    menu.append_section(
        None,
        &section(&[
            ("取り消す", "win.undo"),
            ("History を作り直す", "win.rebuild-history"),
        ]),
    );
    menu
}

fn show_page(p: &Parts, page: Page) {
    p.pages.set_visible_child_name(match page {
        Page::WorkingCopy => "working",
        Page::History => "history",
    });
    p.sidebar.select_page(page);
}

/// 新しいブランチ・タグの起点。History で選んだコミットがあればそれ、なければ今のブランチ。
fn start_point(p: &Parts) -> String {
    let on_history = p.pages.visible_child_name().as_deref() == Some("history");
    if on_history
        && let (Some(row), Some((_, history))) = (p.view.selected(), p.view.current())
        && let Some(id) = history.graph.ids().get(row as usize)
    {
        return id.to_string()[..12].to_owned();
    }
    p.refs
        .borrow()
        .as_ref()
        .and_then(|r| r.refs.head_branch.clone())
        .unwrap_or_else(|| "HEAD".into())
}

fn new_branch(p: &Rc<Parts>) {
    let (actions, start) = (p.actions.clone(), start_point(p));
    glib::spawn_future_local(async move {
        let title = format!("{start} から新しいブランチ");
        if let Some(name) = actions.ask(&title, "", "作成して切り替え").await {
            actions.perform(
                json!({ "op": "createBranch", "name": name, "start": start, "checkout": true }),
            );
        }
    });
}

fn new_tag(p: &Rc<Parts>) {
    let (actions, target) = (p.actions.clone(), start_point(p));
    glib::spawn_future_local(async move {
        let title = format!("{target} に新しいタグ");
        if let Some(name) = actions.ask(&title, "", "作成").await {
            actions.perform(
                json!({ "op": "createTag", "name": name, "target": target, "message": null }),
            );
        }
    });
}

/// 最新の stash（`stash@{0}`）を当てて消す。
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

fn open_folder(p: &Rc<Parts>) {
    let dialog = gtk::FileDialog::builder()
        .title("リポジトリを開く")
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

fn show_shortcuts(p: &Rc<Parts>) {
    let group = |title: &str, items: &[(&str, &str)]| {
        let group = gtk::ShortcutsGroup::builder().title(title).build();
        for (accel, label) in items {
            let shortcut = gtk::ShortcutsShortcut::builder()
                .accelerator(*accel)
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
        "全体",
        &[
            ("<Control>o", "リポジトリを開く"),
            ("<Control>w", "窓を閉じる"),
            ("<Control>q", "終了"),
            ("<Control>question", "キーボードショートカット"),
        ],
    ));
    section.add_group(&group(
        "表示",
        &[
            ("<Control>1", "Working Copy"),
            ("<Control>2", "History"),
            ("<Control>r", "読み直す"),
            ("Up Down Page_Up Page_Down Home End", "History で行を選ぶ"),
        ],
    ));
    section.add_group(&group(
        "リポジトリ",
        &[
            ("<Control><Shift>f", "fetch"),
            ("<Control><Shift>l", "pull"),
            ("<Control><Shift>p", "push"),
            ("<Control>z", "取り消す（入力欄の外で）"),
        ],
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
        .comments("低遅延の Git クライアント")
        .website("https://github.com/fukusaka/amonhen")
        .license("MIT OR Apache-2.0")
        .authors(["Shoichi Fukusaka"])
        .build()
        .present();
}
