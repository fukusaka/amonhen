//! 窓のメニューバー。操作の名前とメニューの区切りは、macOS 版・GTK4 版と共通の操作の表（`amonhen-commands`）から
//! 作る。この FE にまだ機能のない操作は出さない（GTK4 版の `PENDING` と同じ考え）。

use amonhen_commands::{Command, Menu};
use windows_reactor::{
    Callback, CommandBar, CommandBarCommand, GridChildExt, MenuBar, MenuBarItem, MenuBarSlot,
    MenuExt, MenuItem, MenuShortcut, SlotsControl, Symbol, View,
};

/// この FE で動く操作。
pub const IMPLEMENTED: &[&str] = &[
    "show-working-copy",
    "show-history",
    "refresh",
    "rebuild-history",
    "go-to-head",
    "go-to-parent",
    "go-to-child",
    "undo",
    "find-in-history",
    "fetch",
    "pull",
    "push",
    "new-branch",
    "merge",
    "rebase",
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
            items.extend(shown.into_iter().map(|c| {
                let item = MenuItem::item(c.id, title(c));
                match c.shortcut.and_then(shortcut) {
                    Some(s) => item.with_shortcut(s),
                    None => item,
                }
            }));
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

/// ツールバーの並び（macOS 版の既定の並び、GTK4 版の `toolbar::DEFAULT`）と印。`None` は区切り。
const TOOLBAR: &[Option<(&str, Symbol)>] = &[
    Some(("sidebar", Symbol::OpenPane)),
    Some(("repositories", Symbol::Library)),
    None,
    Some(("branch", Symbol::Switch)),
    Some(("fetch", Symbol::Download)),
    Some(("pull", Symbol::Import)),
    Some(("push", Symbol::Upload)),
    None,
    Some(("merge", Symbol::Shuffle)),
    Some(("rebase", Symbol::Sort)),
    None,
    Some(("stash", Symbol::Save)),
    Some(("pop", Symbol::OpenFile)),
    None,
    Some(("activity", Symbol::Clock)),
    Some(("refresh", Symbol::Refresh)),
    Some(("undo", Symbol::Undo)),
    Some(("search", Symbol::Find)),
];

/// ツールバーのボタンの名前（GTK4 版の `toolbar::name`。操作の名前は英語のまま）。
fn toolbar_name(id: &str) -> String {
    match id {
        "sidebar" => amonhen_model::tr!("Show Sidebar"),
        "search" => amonhen_model::tr!("Search"),
        "refresh" => amonhen_model::tr!("Refresh"),
        "repositories" => "Repositories".to_owned(),
        _ => {
            let mut name = id.to_owned();
            name[..1].make_ascii_uppercase();
            name
        }
    }
}

/// ツールバーのボタンが押す操作（表の id）。
fn toolbar_command(id: &str) -> &str {
    match id {
        "branch" => "switch-branch",
        "pop" => "pop-stash",
        "sidebar" => "show-sidebar",
        "search" => "find-in-history",
        _ => id,
    }
}

/// ツールバー（CommandBar）。押すと `on_click` が操作の id を受ける。まだ動かない操作のボタンは押せない。
pub fn toolbar(on_click: Callback<String>) -> View {
    let commands = TOOLBAR.iter().enumerate().map(|(i, entry)| match entry {
        None => CommandBarCommand::separator(format!("-{i}")),
        Some((id, icon)) => {
            let command = toolbar_command(id);
            CommandBarCommand::button_with_icon(command, toolbar_name(id), *icon)
                .enabled(IMPLEMENTED.contains(&command))
        }
    });
    CommandBar::new()
        .grid_row(1)
        .owned_commands(commands, [], on_click)
}

/// 押したキーに当たる、この FE で動く操作（メニューの項目のショートカット）。
pub fn command_for(pressed: MenuShortcut) -> Option<&'static str> {
    amonhen_commands::COMMANDS
        .iter()
        .filter(|c| IMPLEMENTED.contains(&c.id))
        .find(|c| c.shortcut.and_then(shortcut) == Some(pressed))
        .map(|c| c.id)
}

/// 表のショートカット（`Primary+Shift+B` の形）を Windows のキーにする。`Primary` は Ctrl、macOS の ⌃（`Control`）
/// と ⌥（`Alt`）はどちらも Alt（GTK4 版と同じ読み替え）。知らないキーなら `None`。
pub fn shortcut(text: &str) -> Option<MenuShortcut> {
    let mut parts: Vec<&str> = text.split('+').collect();
    let key = parts.pop()?;
    let mut out = MenuShortcut {
        key: virtual_key(key)?,
        ..MenuShortcut::default()
    };
    for m in parts {
        match m {
            "Primary" => out.control = true,
            "Shift" => out.shift = true,
            "Control" | "Alt" => out.alt = true,
            _ => return None,
        }
    }
    Some(out)
}

/// キーの名前の仮想キー（`VK_*`）。
fn virtual_key(key: &str) -> Option<u16> {
    let bytes = key.as_bytes();
    Some(match key {
        "Return" => 0x0D,
        "Left" => 0x25,
        "Up" => 0x26,
        "Right" => 0x27,
        "Down" => 0x28,
        "Comma" => 0xBC,
        "Semicolon" => 0xBA,
        _ if bytes.len() == 1 && bytes[0].is_ascii_alphanumeric() => {
            u16::from(bytes[0].to_ascii_uppercase())
        }
        _ if key.starts_with('F') => {
            let n: u16 = key[1..].parse().ok()?;
            if !(1..=24).contains(&n) {
                return None;
            }
            0x70 + n - 1
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_table_shortcuts() {
        let s = shortcut("Primary+Shift+H").unwrap();
        assert_eq!(
            (s.key, s.control, s.shift, s.alt),
            (0x48, true, true, false)
        );
        let s = shortcut("Primary+Alt+Down").unwrap();
        assert_eq!((s.key, s.control, s.alt), (0x28, true, true));
        assert_eq!(shortcut("Primary+2").unwrap().key, 0x32);
        assert_eq!(shortcut("F9").unwrap().key, 0x78);
        assert!(shortcut("Primary+Tab").is_none());
    }
}
