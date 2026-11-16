//! 窓のメニューバー。操作の名前とメニューの区切りは、macOS 版・GTK4 版と共通の操作の表（`amonhen-commands`）から
//! 作る。この FE にまだ機能のない操作は出さない（GTK4 版の `PENDING` と同じ考え）。

use amonhen_commands::{Command, Menu};
use windows_reactor::{
    Callback, GridChildExt, MenuBar, MenuBarItem, MenuBarSlot, MenuExt, MenuItem, SlotsControl,
    View,
};

/// この FE で動く操作。
pub const IMPLEMENTED: &[&str] = &[
    "show-history",
    "refresh",
    "rebuild-history",
    "go-to-head",
    "go-to-parent",
    "go-to-child",
];

/// 項目の名前（GTK4 版の `appmenu::title` と同じ）。
fn title(c: &Command) -> String {
    match c.id {
        "reveal-in-files" => amonhen_model::tr!("Show in Files"),
        _ if c.localized => amonhen_model::i18n::translate(c.title).into_owned(),
        _ => c.title.to_owned(),
    }
}

/// メニューバー。項目を選ぶと `on_click` が操作の id を受ける。
pub fn menu_bar(on_click: impl Fn() -> Callback<String>) -> View {
    let menus: [(Menu, &'static str); 7] = [
        (Menu::File, "File"),
        (Menu::Edit, "Edit"),
        (Menu::View, "View"),
        (Menu::Navigate, "Navigate"),
        (Menu::Repository, "Repository"),
        (Menu::Commit, "Commit"),
        (Menu::Window, "Window"),
    ];
    let mut bar: Vec<(String, View)> = Vec::new();
    for (menu, name) in menus {
        let mut items = Vec::new();
        for (i, section) in amonhen_commands::menu(menu).into_iter().enumerate() {
            let shown: Vec<&Command> = section
                .into_iter()
                .filter(|c| IMPLEMENTED.contains(&c.id))
                .collect();
            if shown.is_empty() {
                continue;
            }
            if !items.is_empty() {
                items.push(MenuItem::separator(format!("-{i}")));
            }
            items.extend(shown.into_iter().map(|c| MenuItem::item(c.id, title(c))));
        }
        if items.is_empty() {
            continue;
        }
        let title = amonhen_model::i18n::translate(name).into_owned();
        let item = MenuBarItem::new()
            .title(title)
            .menu(windows_reactor::Menu::new(items, on_click()));
        bar.push((name.to_owned(), item));
    }
    // 窓の上の行（根の Grid の 0 行目）に置く。
    MenuBar::new()
        .grid_row(0)
        .collection_slot(MenuBarSlot::Items, bar)
}
